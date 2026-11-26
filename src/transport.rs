use crate::settings::join_log;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    io,
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Serialize, Deserialize)]
pub struct Participant {
    pub node: String,
    pub nick: String,
    pub ip: String,
    pub present: bool,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub ping_ms: Option<u32>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct SshRequest {
    pub id: String,
    pub requester: String,
    pub recipient: String,
    pub state: String,
    #[serde(default)]
    pub occurred_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_key: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct InviteMethods {
    pub link: bool,
    pub qr: bool,
    #[serde(default)]
    pub password: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password_revision: Option<String>,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    #[default]
    Available,
    Unreachable,
    Deleted,
    Denied,
    SyncFailed,
}

impl Availability {
    pub fn membership_gone(self) -> bool {
        matches!(self, Self::Deleted | Self::Denied)
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::Available => "",
            Self::Unreachable => "unreachable",
            Self::Deleted => "deleted",
            Self::Denied => "denied",
            Self::SyncFailed => "sync_failed",
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Membership {
    pub host: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_revision: Option<String>,
    #[serde(default)]
    pub methods: Option<InviteMethods>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assigned_ip: Option<String>,
    #[serde(default)]
    pub credential_expires_at: u64,
    pub id: String,
    pub name: String,
    pub node: String,
    pub session: String,
    #[serde(default)]
    pub rows: Vec<Participant>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ssh_requests: Vec<SshRequest>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ssh_history: Vec<SshRequest>,
    #[serde(default)]
    pub ssh_supported: bool,
    #[serde(default)]
    pub ssh_key_requests: bool,
    #[serde(default)]
    pub own_ip: String,
    #[serde(default)]
    pub server_ms: Option<u32>,
    #[serde(default)]
    pub health: String,
    #[serde(default)]
    pub availability: Availability,
}

fn parse_reply(bytes: &[u8]) -> io::Result<Value> {
    serde_json::from_slice(bytes).map_err(|err| {
        join_log::number("json_line", err.line() as i64);
        join_log::number("json_column", err.column() as i64);
        err.into()
    })
}

fn decode_optional_list<T: serde::de::DeserializeOwned>(
    data: &Value,
    name: &str,
) -> io::Result<Vec<T>> {
    data.get(name).map_or(Ok(Vec::new()), |entries| {
        serde_json::from_value(entries.clone()).map_err(io::Error::from)
    })
}

fn post(host: &str, endpoint: &str, body: Value) -> io::Result<Value> {
    request(host, endpoint, body).map(|(data, _)| data)
}

thread_local! {
    // Поток фоновой синхронизации живёт между опросами. libcurl разделяет соединения по серверу.
    static HTTPS: std::cell::RefCell<curl::easy::Easy> = std::cell::RefCell::new(curl::easy::Easy::new());
}

fn request(host: &str, endpoint: &str, body: Value) -> io::Result<(Value, u32)> {
    request_with_diagnostics(host, endpoint, body, None)
}

fn request_with_diagnostics(
    host: &str,
    endpoint: &str,
    body: Value,
    background: Option<(&std::path::Path, &'static str)>,
) -> io::Result<(Value, u32)> {
    use crate::settings::diagnostics;

    join_log::operation(match endpoint {
        "login" => "http_login",
        "bootstrap" => "http_bootstrap",
        "redeem" => "http_redeem",
        "sync" => "http_sync",
        "attest" => "http_attest",
        "ping" => "http_ping",
        _ => "http_request",
    });
    let body = serde_json::to_vec(&body)?;
    let (status, response, latency) = HTTPS
        .with_borrow_mut(|client| {
            let result = send_https(client, host, endpoint, &body);
            // Сбрасываем тело и заголовки в том числе при ошибке. Открытые соединения сохраняются.
            client.reset();
            result
        })
        .map_err(|err| {
            let code = err.code();
            join_log::number("curl_code", code.into());
            let error = io::Error::new(io::ErrorKind::NotConnected, "Could not reach Uplink over HTTPS");
            if let Some((prefs, operation)) = background {
                diagnostics::failure(prefs, operation, format_args!(" curl_code={code}"), &error);
            }
            error
        })?;
    join_log::number("http_status", status.into());

    let result = match status {
        200 => parse_reply(&response).map(|reply| (reply, latency)),
        409 if endpoint == "ssh/request" => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "An SSH request from this device already uses another key.",
        )),
        410 if endpoint == "sync" && serde_json::from_slice::<Value>(&response)
            .is_ok_and(|v| v["error"] == "Network deleted") =>
        {
            Err(io::Error::new(io::ErrorKind::NotFound, "Network deleted"))
        }
        409 if serde_json::from_slice::<Value>(&response)
            .is_ok_and(|v| v["error"] == "Network name already in use") =>
        {
            Err(io::Error::other("Network name already in use"))
        }
        413 if endpoint == "avatar" => Err(io::Error::other(
            "The server upload limit is too low for this avatar.",
        )),
        400 if endpoint == "avatar" => Err(io::Error::other("Invalid network avatar")),
        429 => Err(io::Error::other("Too many login attempts")),
        403 if endpoint == "login" => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Invalid network credentials",
        )),
        409 => Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "Waiting for network join",
        )),
        403 => {
            let body = serde_json::from_slice::<Value>(&response).unwrap_or(Value::Null);
            let reason = match body["error"].as_str() {
                Some(
                    s @ ("Invalid or expired invitation"
                    | "Invitation already used"
                    | "Membership revoked"
                    | "Invalid session"
                    | "Session already enrolled"),
                ) => s,
                _ => "Invitation or membership denied",
            };
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                reason.to_owned(),
            ))
        }
        _ => Err(io::Error::other("Uplink API unavailable")),
    };
    if let (Some((prefs, operation)), Err(error)) = (background, &result) {
        diagnostics::failure(prefs, operation, format_args!(" http_status={status}"), error);
    }
    result
}

fn send_https(
    client: &mut curl::easy::Easy,
    host: &str,
    endpoint: &str,
    body: &[u8],
) -> Result<(u32, Vec<u8>, u32), curl::Error> {
    let max_response = if endpoint == "sync" {
        1_700_000
    } else {
        262_144
    };
    client.url(&format!("https://{host}/v1/{endpoint}"))?;
    client.connect_timeout(Duration::from_secs(4))?;
    client.timeout(Duration::from_secs(8))?;
    client.max_filesize(max_response as u64)?;
    client.follow_location(false)?;
    #[cfg(test)]
    https_tests::configure_tls(client)?;

    let mut headers = curl::easy::List::new();
    headers.append("Content-Type: application/json")?;
    client.http_headers(headers)?;
    client.post_fields_copy(body)?;
    let mut response = Vec::new();
    {
        let mut transfer = client.transfer();
        transfer.write_function(|chunk| {
            if chunk.len() > max_response - response.len() {
                return Ok(0);
            }
            response.extend_from_slice(chunk);
            Ok(chunk.len())
        })?;
        transfer.perform()?;
    }
    let latency = client
        .starttransfer_time()?
        .saturating_sub(client.pretransfer_time()?);
    Ok((
        client.response_code()?,
        response,
        (latency.as_secs_f64() * 1000.0).round() as u32,
    ))
}

#[cfg(test)]
mod https_tests;

fn hex(value: &Value, length: usize) -> io::Result<String> {
    value
        .as_str()
        .filter(|s| s.len() == length && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(str::to_owned)
        .ok_or_else(|| io::Error::other("Invalid Uplink identifier"))
}

pub fn password_invite(host: &str, name: &str, password: &str) -> io::Result<String> {
    if !crate::invite::valid_host(host) {
        return Err(io::Error::other("Enter a server address"));
    }
    let response = post(host, "login", json!({"name":name,"password":password}))?;
    Ok(format!(
        "uplink://{host}/invite/{}",
        hex(&response["token"], 64)?
    ))
}

pub fn bootstrap(host: &str, token: &str, progress: &dyn Fn(crate::settings::JoinStage)) -> io::Result<Membership> {
    let info = post(host, "bootstrap", json!({"token":token}))?;
    if info["engine"] != "easytier" {
        return Err(io::Error::other("Update the Uplink server to join this network."));
    }
    progress(crate::settings::JoinStage::Device);
    join_log::operation("start_network_service");
    crate::service_control::power(true)?;
    Ok(Membership {
        host: host.into(), avatar_revision: None, methods: None, assigned_ip: None, credential_expires_at: 0,
        id: hex(&info["id"], 32)?, name: info["name"].as_str().unwrap_or(host).into(), node: String::new(),
        session: format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple()),
        own_ip: String::new(), rows: Vec::new(), server_ms: None, health: "ok".into(),
        ssh_requests: Vec::new(), ssh_history: Vec::new(), ssh_supported: false, ssh_key_requests: false,
        availability: Availability::Available,
    })
}

fn interface(ip: &str) -> io::Result<Option<Value>> {
    use windows_sys::Win32::{Foundation::*, NetworkManagement::{IpHelper::*, Ndis::NET_LUID_LH}};
    use std::{ffi::CStr, ptr};
    if ip.is_empty() { return Ok(None); }
    let alias: Vec<u16> = "Uplink".encode_utf16().chain(Some(0)).collect();
    unsafe {
        let mut luid = NET_LUID_LH::default();
        if ConvertInterfaceAliasToLuid(alias.as_ptr(), &mut luid) != 0 { return Ok(None); }
        let mut index = 0;
        let code = ConvertInterfaceLuidToIndex(&luid, &mut index);
        if code != 0 { return Err(io::Error::from_raw_os_error(code as i32)); }
        let mut size = 0;
        if GetAdaptersInfo(ptr::null_mut(), &mut size) != ERROR_BUFFER_OVERFLOW { return Ok(None); }
        let mut storage = vec![0usize; (size as usize).div_ceil(size_of::<usize>())];
        let mut adapter: *mut IP_ADAPTER_INFO = storage.as_mut_ptr().cast();
        let code = GetAdaptersInfo(adapter, &mut size);
        if matches!(code, ERROR_BUFFER_OVERFLOW | ERROR_NO_DATA) { return Ok(None); }
        if code != 0 { return Err(io::Error::from_raw_os_error(code as i32)); }
        while !adapter.is_null() {
            if (*adapter).Index == index {
                let mut address = &(*adapter).IpAddressList as *const IP_ADDR_STRING;
                while !address.is_null() {
                    if CStr::from_ptr((*address).IpAddress.String.as_ptr()).to_bytes() == ip.as_bytes() {
                        return Ok(Some(json!({"status":"OK", "assignedAddresses":[format!("{ip}/24")]})));
                    }
                    address = (*address).Next;
                }
            }
            adapter = (*adapter).Next;
        }
    }
    Ok(None)
}

fn wait_for_virtual_ip(
    deadline: Instant,
    mut read: impl FnMut() -> io::Result<Option<Value>>,
) -> io::Result<Value> {
    loop {
        if let Some(net) = read()?.filter(|net| assigned_ipv4(net).is_some()) {
            return Ok(net);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Waiting for virtual IP",
            ));
        }
        thread::sleep(remaining.min(Duration::from_millis(500)));
    }
}

fn assigned_ipv4(net: &Value) -> Option<std::net::Ipv4Addr> {
    if net["status"] != "OK" {
        return None;
    }
    net["assignedAddresses"]
        .as_array()?
        .iter()
        .find_map(|address| address.as_str()?.split_once('/')?.0.parse().ok())
}
pub(crate) enum PingError {
    Address,
    Open(u32),
    Send(u32),
    Status(u32),
    UnexpectedResponder(std::net::Ipv4Addr),
}

impl PingError {
    pub(crate) fn timeout(&self) -> bool {
        matches!(self, Self::Send(11010 | 1460) | Self::Status(11010))
    }
}

impl std::fmt::Display for PingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = if self.timeout() { "timeout" } else { "error" };
        match self {
            Self::Address => write!(f, "result=skipped reason=invalid_address"),
            Self::Open(code) => write!(f, "result=error operation=open win32={code}"),
            Self::Send(code) => write!(f, "result={kind} operation=send win32={code}"),
            Self::Status(code) => write!(f, "result={kind} operation=reply ip_status={code}"),
            Self::UnexpectedResponder(ip) => write!(f, "result=error reason=unexpected_responder responder={ip}"),
        }
    }
}

pub(crate) const ECHO_TIMEOUT_MS: u32 = 600;

pub(crate) fn peer_ping(source: &str, destination: &str) -> Result<u32, PingError> {
    use std::{net::Ipv4Addr, ptr};
    use windows_sys::Win32::{Foundation::INVALID_HANDLE_VALUE, NetworkManagement::IpHelper::*};
    let source = source.parse::<Ipv4Addr>().map_err(|_| PingError::Address)?;
    let target = destination.parse::<Ipv4Addr>().map_err(|_| PingError::Address)?;
    if !source.is_private()
        || source.octets()[..3] != target.octets()[..3]
        || matches!(target.octets()[3], 0 | 255)
    {
        return Err(PingError::Address);
    }
    let payload = b"Uplink";
    let mut buffer = [0usize; 16];
    // Буфер выровнен для ICMP_ECHO_REPLY и остаётся жив до завершения синхронного вызова.
    unsafe {
        let handle = IcmpCreateFile();
        if handle == INVALID_HANDLE_VALUE {
            return Err(PingError::Open(windows_sys::Win32::Foundation::GetLastError()));
        }
        let received = IcmpSendEcho2Ex(
            handle,
            ptr::null_mut(),
            None,
            ptr::null(),
            u32::from_ne_bytes(source.octets()),
            u32::from_ne_bytes(target.octets()),
            payload.as_ptr().cast(),
            payload.len() as u16,
            ptr::null(),
            buffer.as_mut_ptr().cast(),
            size_of_val(&buffer) as u32,
            ECHO_TIMEOUT_MS,
        );
        let error = (received == 0).then(|| windows_sys::Win32::Foundation::GetLastError());
        IcmpCloseHandle(handle);
        if let Some(code) = error {
            return Err(PingError::Send(code));
        }
        let reply = &*buffer.as_ptr().cast::<ICMP_ECHO_REPLY>();
        let responder = Ipv4Addr::from(reply.Address.to_ne_bytes());
        if reply.Status != 0 {
            Err(PingError::Status(reply.Status))
        } else if responder != target {
            Err(PingError::UnexpectedResponder(responder))
        } else {
            Ok(reply.RoundTripTime)
        }
    }
}

impl Membership {
    pub fn leave_group(&self) -> io::Result<()> {
        post(&self.host, "leave", json!({"session":self.session})).map(|_| ())
    }

    pub fn key(&self) -> String {
        format!("uplink://{}/networks/{}", self.host, self.id)
    }
    pub fn enroll(
        &mut self,
        token: &str,
        nick: &str,
        prefs: &std::path::Path,
        returning: bool,
        progress: &dyn Fn(crate::settings::JoinStage),
    ) -> io::Result<()> {
        progress(crate::settings::JoinStage::Approval);
        let grant = if returning && !self.node.is_empty() {
            post(&self.host, "profile", json!({"session":self.session}))?
        } else {
            post(&self.host, "redeem", json!({"token":token, "session":self.session, "nick":nick}))?
        };
        join_log::operation("configure_uplink_adapter");
        let ip = self.install_grant(&grant)?;
        progress(crate::settings::JoinStage::Address);
        let net = wait_for_virtual_ip(Instant::now() + Duration::from_secs(35), || interface(&ip))?;
        self.refresh_using(nick, prefs, |_| Ok(Some(net)))
    }

    fn install_grant(&mut self, grant: &Value) -> io::Result<String> {
        let node = hex(&grant["node"], 10)?;
        if grant["id"] != self.id || (!self.node.is_empty() && self.node != node) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Unexpected network membership"));
        }
        let expiry = grant.get("credential_expires_at").map(|v| {
            v.as_u64().filter(|t| *t > 0).ok_or(io::ErrorKind::InvalidData)
        }).transpose()?.unwrap_or_default();
        let profile: crate::tunnel_profile::Profile = serde_json::from_value(json!({
            "id":self.id, "ip":grant["ip"], "endpoint":grant["endpoint"], "credential":grant["credential"]
        }))?;
        crate::tunnel_profile::save(&profile)?;
        let ip = profile.ip.to_string();
        self.node = node;
        self.assigned_ip = Some(ip.clone());
        self.credential_expires_at = expiry;
        Ok(ip)
    }

    pub fn manage(&self, action: &str, value: &str) -> io::Result<Value> {
        post(
            &self.host,
            if action == "avatar" {
                "avatar"
            } else {
                "manage"
            },
            json!({"session":self.session,"action":action,"value":value}),
        )
    }
    pub fn share(&self) -> io::Result<String> {
        let response = post(&self.host, "share", json!({"session":self.session}))?;
        Ok(format!(
            "uplink://{}/invite/{}",
            self.host,
            hex(&response["token"], 64)?
        ))
    }

    pub fn rename_member(&self, node: &str, name: &str) -> io::Result<()> {
        post(&self.host, "nickname", json!({"session":self.session,"node":node,"nick":name}))?;
        Ok(())
    }

    pub fn request_ssh(&self, peer: &str, public_key: &str) -> io::Result<SshRequest> {
        Ok(serde_json::from_value(post(&self.host, "ssh/request",
            json!({"session":self.session,"recipient":peer,"public_key":public_key}))?)?)
    }

    pub fn respond_ssh(&self, id: &str, decision: &str) -> io::Result<SshRequest> {
        Ok(serde_json::from_value(post(&self.host, "ssh/respond",
            json!({"session":self.session,"id":id,"decision":decision}))?)?)
    }

    pub fn refresh(&mut self, nick: &str, prefs: &std::path::Path) -> io::Result<()> {
        self.refresh_using(nick, prefs, interface)
    }

    fn refresh_using(
        &mut self,
        nick: &str,
        prefs: &std::path::Path,
        read_interface: impl FnOnce(&str) -> io::Result<Option<Value>>,
    ) -> io::Result<()> {
        use crate::settings::diagnostics;

        let sync =
            json!({"session":self.session,"network":self.id,"nick":nick,"avatar_revision":self.known_avatar(prefs),
                "credential_expires_at":self.credential_expires_at,"ssh_requests":true});
        let snapshot = request_with_diagnostics(&self.host, "sync", sync, Some((prefs, "sync_membership")))
            .map(|(data, _)| data);
        let renewal = match &snapshot {
            Ok(data) if data["id"] == self.id => data.get("transport_profile")
                .map(|grant| self.install_grant(grant)).transpose(),
            _ => Ok(None),
        };
        let avatar_status = match &snapshot {
            Ok(data) if data["id"] == self.id => self.apply_avatar(prefs, data),
            Err(e) if matches!(e.kind(), io::ErrorKind::PermissionDenied | io::ErrorKind::NotFound) => {
                self.apply_avatar(prefs, &Value::Null)
            }
            _ => Ok(()),
        };
        let sync_ok = snapshot.is_ok();
        let ms = sync_ok.then(|| {
            request_with_diagnostics(&self.host, "ping", json!({}), Some((prefs, "server_ping")))
                .ok()
                .map(|(_, ms)| ms)
        }).flatten();
        let net = read_interface(self.assigned_ip.as_deref().unwrap_or(""));
        let status = self.update_status(
            net.as_ref()
                .ok()
                .and_then(|it| it.as_ref())
                .unwrap_or(&Value::Null),
            snapshot,
            ms,
        );
        if net.is_err() {
            self.server_ms = None;
            for member in &mut self.rows {
                member.ping_ms = None;
                member.present = false;
            }
        }
        let renewal = renewal.map(|_| ());
        let net = net.map(|_| ());
        if sync_ok && let Err(error) = &status {
            diagnostics::failure(prefs, "apply_membership_snapshot", format_args!(""), error);
        }
        for (operation, result) in [
            ("renew_transport_credential", &renewal),
            ("query_adapter_ip", &net),
            ("sync_network_avatar", &avatar_status),
        ] {
            if let Err(error) = result {
                diagnostics::failure(prefs, operation, format_args!(""), error);
            }
        }
        status.and(renewal).and(net).and(avatar_status)
    }

    fn update_status(
        &mut self,
        net: &Value,
        snapshot: io::Result<Value>,
        ms: Option<u32>,
    ) -> io::Result<()> {
        self.availability = match &snapshot {
            Ok(data) if data["id"] == self.id => Availability::Available,
            Err(e) if e.kind() == io::ErrorKind::NotFound && e.to_string() == "Network deleted" => Availability::Deleted,
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied && matches!(e.to_string().as_str(), "Invalid session" | "Membership revoked") => Availability::Denied,
            Err(e) if e.kind() == io::ErrorKind::NotConnected => Availability::Unreachable,
            _ => Availability::SyncFailed,
        };
        self.server_ms = ms;
        if self.availability.membership_gone() || snapshot
            .as_ref()
            .is_err_and(|e| e.kind() == io::ErrorKind::PermissionDenied)
        {
            self.methods = None;
            self.rows.clear();
            self.ssh_requests.clear();
            self.ssh_history.clear();
            self.ssh_supported = false;
            self.ssh_key_requests = false;
            self.own_ip.clear();
            return snapshot.map(|_| ());
        }
        self.own_ip = assigned_ipv4(net)
            .map(|a| a.to_string())
            .unwrap_or_default();
        if let Ok(data) = &snapshot {
            if data["id"] != self.id {
                return Err(io::Error::other("Unexpected network"));
            }
            self.rows = serde_json::from_value(data["members"].clone())?;
            self.methods = serde_json::from_value(data["methods"].clone())?;
            self.ssh_requests = decode_optional_list(data, "ssh_requests")?;
            self.ssh_history = decode_optional_list(data, "ssh_history")?;
            self.ssh_supported = data["ssh_requests"].is_array();
            self.ssh_key_requests = self.ssh_supported && data["ssh_key_requests"] == true;
            self.name = data["name"].as_str().unwrap_or(&self.host).to_owned();
        } else {
            for row in &mut self.rows {
                row.present = false;
            }
        }
        for row in &mut self.rows {
            row.ping_ms = if row.node == self.node { self.server_ms } else { None };
        }
        snapshot.map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use super::*;

    #[test]
    fn the_ssh_inbox_works_with_an_updated_server_and_clears_when_access_is_lost() {
        let mut member: Membership = serde_json::from_value(json!({
            "host":"example.org","id":"1234","session":"fixture","node":"alice","name":"Network"
        })).unwrap();
        let previous_server = json!({"id":"1234","name":"Network","members":[],"methods":null});
        member.update_status(&Value::Null, Ok(previous_server.clone()), None).unwrap();
        assert!(!member.ssh_supported);
        let mut updated_server = previous_server;
        updated_server["ssh_requests"] = json!([{
            "id":"request","requester":"bob","recipient":"alice","state":"pending","occurred_at":1234
        }]);
        updated_server["ssh_history"] = json!([{
            "id":"old","requester":"alice","recipient":"bob","state":"cancelled","occurred_at":1230
        }]);
        member.update_status(&Value::Null, Ok(updated_server), None).unwrap();
        assert!(member.ssh_supported);
        assert_eq!(member.ssh_requests[0].recipient, "alice");
        assert_eq!(member.ssh_history[0].occurred_at, 1230);
        assert!(member.update_status(&Value::Null, Err(io::Error::new(
            io::ErrorKind::PermissionDenied, "Membership revoked",
        )), None).is_err());
        assert!(!member.ssh_supported);
        assert!(member.ssh_requests.is_empty());
        assert!(member.ssh_history.is_empty());
    }

    #[test]
    fn join_waits_for_a_ready_local_ipv4() {
        let ready = json!({"status":"OK","assignedAddresses":["fd00::1/64","10.3.4.5/24"]});
        let mut samples = [
            None,
            Some(json!({"status":"REQUESTING_CONFIGURATION","assignedAddresses":["10.3.4.5/24"]})),
            Some(json!({"status":"OK","assignedAddresses":["fd00::1/64"]})),
            Some(ready.clone()),
        ]
        .into_iter();
        let interface = wait_for_virtual_ip(Instant::now() + Duration::from_secs(10), || {
            Ok(samples.next().expect("unexpected extra poll"))
        })
        .unwrap();
        assert_eq!(interface, ready);
    }

    #[test]
    fn virtual_ip_wait_distinguishes_timeout_from_a_failed_local_query() {
        let now = Instant::now();
        assert_eq!(
            wait_for_virtual_ip(now, || Ok(None)).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        let failed = wait_for_virtual_ip(now, || Err(io::ErrorKind::BrokenPipe.into()));
        assert_eq!(failed.unwrap_err().kind(), io::ErrorKind::BrokenPipe);
    }

    #[test]
    fn avatar_sync_handles_replacement_missing_cache_and_older_servers() {
        let dir = tempfile::tempdir().unwrap();
        let prefs = dir.path().join("prefs.json");
        let mut m: Membership = serde_json::from_value(json!({
            "host":"example.org","id":"1234","session":"s","node":"me","name":"A"
        }))
        .unwrap();
        let photo = crate::network_avatar::encode(&vec![77; 512 * 512 * 4]).unwrap();
        let bytes = STANDARD.decode(&photo).unwrap();
        use sha2::{Digest, Sha256};
        let revision = format!("{:x}", Sha256::digest(&bytes));
        m.apply_avatar(&prefs, &json!({"avatar_revision":revision, "avatar":photo}))
            .unwrap();
        let file = m.avatar_file(&prefs).unwrap();
        assert_eq!(
            image::open(&file).unwrap().into_rgba8().get_pixel(10, 10).0,
            [77; 4]
        );
        assert_eq!(m.known_avatar(&prefs), revision);
        m.apply_avatar(&prefs, &json!({"avatar_revision":revision}))
            .unwrap();
        std::fs::remove_file(file).unwrap();
        assert_eq!(m.known_avatar(&prefs), "");
        assert!(
            m.apply_avatar(
                &prefs,
                &json!({"avatar_revision":revision,"avatar":"invalid"})
            )
            .is_err()
        );
        m.apply_avatar(&prefs, &json!({})).unwrap();
        assert!(m.avatar_revision.is_none());
    }

    #[test]
    fn https_outage_keeps_adapter_ip_and_cached_peers_for_independent_probes() {
        let mut m: Membership = serde_json::from_value(json!({
            "host":"example.org","id":"1234","session":"s","node":"me","name":"A","server_ms":20,
            "rows":[{"node":"peer","nick":"Peer","ip":"10.3.4.6","present":false},
                    {"node":"gone","nick":"Gone","ip":"10.3.4.7","present":true,"ping_ms":42}]
        }))
        .unwrap();
        let net = json!({"status":"OK","assignedAddresses":["10.3.4.5/24"]});
        assert!(
            m.update_status(
                &net,
                Err(io::Error::other("timeout")),
                None,
            )
            .is_err()
        );
        assert_eq!(m.own_ip, "10.3.4.5");
        assert_eq!(m.server_ms, None);
        assert_eq!(m.rows.len(), 2);
        assert!(
            m.update_status(
                &net,
                Err(io::ErrorKind::PermissionDenied.into()),
                None,
            )
            .is_err()
        );
        assert!(m.rows.is_empty() && m.own_ip.is_empty());
    }

    #[test]
    fn unavailable_networks_keep_their_name_and_recover_after_a_valid_sync() {
        let mut m: Membership = serde_json::from_value(json!({"host":"example.org","id":"1234",
            "session":"s","node":"me","name":"Friday","own_ip":"10.0.0.1"})).unwrap();
        for (kind, reason, expected) in [
            (io::ErrorKind::NotConnected, "Could not reach Uplink over HTTPS", "unreachable"),
            (io::ErrorKind::PermissionDenied, "Invalid session", "denied"),
            (io::ErrorKind::NotFound, "Network deleted", "deleted"),
        ] {
            assert!(m.update_status(&Value::Null, Err(io::Error::new(kind, reason)), None).is_err());
            assert_eq!(serde_json::to_value(&m).unwrap()["availability"], expected);
            assert_eq!(m.name, "Friday");
        }
        m.update_status(&Value::Null, Ok(json!({"id":"1234","name":"Friday","members":[],"methods":null})), Some(12)).unwrap();
        assert_eq!(serde_json::to_value(&m).unwrap()["availability"], "available");
    }

}
