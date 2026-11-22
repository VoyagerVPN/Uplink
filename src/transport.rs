use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs, io,
    os::windows::process::CommandExt,
    path::PathBuf,
    process::{Command, Stdio},
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

#[derive(Clone, Serialize, Deserialize)]
pub struct InviteMethods {
    pub link: bool,
    pub qr: bool,
    #[serde(default)]
    pub password: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password_revision: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Membership {
    pub host: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_revision: Option<String>,
    #[serde(default)]
    pub methods: Option<InviteMethods>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport: Option<String>,
    pub id: String,
    pub name: String,
    pub node: String,
    pub session: String,
    #[serde(default)]
    pub rows: Vec<Participant>,
    #[serde(default)]
    pub own_ip: String,
    #[serde(default)]
    pub server_ms: Option<u32>,
    #[serde(default)]
    pub health: String,
}

fn home() -> io::Result<PathBuf> {
    let data = std::env::var_os("ProgramData")
        .ok_or_else(|| io::Error::other("ProgramData unavailable"))?;
    Ok(PathBuf::from(data).join("ZeroTier/One"))
}

fn zt(args: &[&str]) -> io::Result<Vec<u8>> {
    let output = Command::new(home()?.join("zerotier-one_x64.exe"))
        .args(["-q", "-j"])
        .args(args)
        .creation_flags(0x08000000)
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other("Local ZeroTier service unavailable"));
    }
    Ok(output.stdout)
}

fn query(args: &[&str]) -> io::Result<Value> {
    Ok(serde_json::from_slice(&zt(args)?)?)
}

fn post(host: &str, endpoint: &str, body: Value) -> io::Result<Value> {
    request(host, endpoint, body).map(|(data, _)| data)
}

thread_local! {
    // Поток фоновой синхронизации живёт между опросами. libcurl разделяет соединения по серверу.
    static HTTPS: std::cell::RefCell<curl::easy::Easy> = std::cell::RefCell::new(curl::easy::Easy::new());
}

fn request(host: &str, endpoint: &str, body: Value) -> io::Result<(Value, u32)> {
    let body = serde_json::to_vec(&body)?;
    let (status, response, latency) = HTTPS
        .with_borrow_mut(|client| {
            let result = send_https(client, host, endpoint, &body);
            // Сбрасываем тело и заголовки в том числе при ошибке. Открытые соединения сохраняются.
            client.reset();
            result
        })
        .map_err(|_| io::Error::other("Could not reach Uplink over HTTPS"))?;

    match status {
        200 => Ok((serde_json::from_slice(&response)?, latency)),
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
                    | "Invalid device proof"
                    | "Device proof required"
                    | "Membership revoked"
                    | "Existing device session required"
                    | "Node already enrolled"),
                ) => s,
                _ => "Invitation or membership denied",
            };
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                reason.to_owned(),
            ))
        }
        _ => Err(io::Error::other("Uplink API unavailable")),
    }
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
        .ok_or_else(|| io::Error::other("Invalid ZeroTier identifier"))
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

fn device_is_prepared(home: &std::path::Path) -> bool {
    let proof = home.join("uplink-proof");
    home.join("zerotier-one_x64.exe").is_file()
        && ["certificate", "delegate.public", "delegate.secret"]
            .iter()
            .all(|name| {
                fs::File::open(proof.join(name))
                    .and_then(|file| file.metadata())
                    .is_ok_and(|info| info.is_file() && info.len() > 0)
            })
}

#[cfg(test)]
#[test]
fn incomplete_device_proof_is_prepared_again() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let proof = home.join("uplink-proof");
    fs::create_dir(&proof).unwrap();
    fs::write(home.join("zerotier-one_x64.exe"), b"fixture").unwrap();
    fs::write(proof.join("certificate"), b"certificate").unwrap();
    assert!(!device_is_prepared(home));
    fs::write(proof.join("delegate.public"), b"public").unwrap();
    fs::write(proof.join("delegate.secret"), b"").unwrap();
    assert!(!device_is_prepared(home));
    fs::write(proof.join("delegate.secret"), b"private fixture").unwrap();
    assert!(device_is_prepared(home));
}

fn prepare_device(home: &std::path::Path, host: &str, token: &str) -> io::Result<()> {
    if device_is_prepared(home) {
        return Ok(());
    }
    let helper = std::env::current_exe()?.with_file_name("UplinkNetwork.exe");
    if !helper.is_file() {
        return Err(io::Error::other(
            "Run the Uplink transport installer to verify this device",
        ));
    }
    // Ссылка не попадает в командную строку или журнал запуска процессов.
    let root = PathBuf::from(
        std::env::var_os("LOCALAPPDATA")
            .ok_or_else(|| io::Error::other("Missing local profile"))?,
    )
    .join("Uplink/requests");
    fs::create_dir_all(&root)?;
    let request = uuid::Uuid::new_v4().simple().to_string();
    let file = root.join(&request);
    use std::io::Write;
    let mut out = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&file)?;
    let guard = tempfile::TempPath::try_from_path(&file)?;
    write!(out, "uplink://{host}/invite/{token}")?;
    drop(out);
    let ps = PathBuf::from(
        std::env::var_os("SystemRoot").ok_or_else(|| io::Error::other("Missing Windows path"))?,
    )
    .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let result = Command::new(ps)
        .args(["-NoProfile","-NonInteractive","-Command",
            "$ErrorActionPreference='Stop'; $sid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value; try { $p=Start-Process -FilePath $env:UPLINK_NETWORK_HELPER -Verb RunAs -ArgumentList ('prepare '+$sid+' '+$env:UPLINK_NETWORK_REQUEST) -WindowStyle Hidden -Wait -PassThru; exit $p.ExitCode } catch { if ($_.Exception.GetBaseException().NativeErrorCode -eq 1223) {exit 38}; exit 2 }"])
        .env("UPLINK_NETWORK_HELPER",helper).env("UPLINK_NETWORK_REQUEST",request)
        .creation_flags(0x08000000).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status()?;
    drop(guard);
    let _ = fs::remove_dir(root);
    if !result.success() {
        return Err(io::Error::other(match result.code() {
            Some(31) => "Could not get the ZeroTier configuration from the server.",
            Some(32) => "The existing ZeroTier installation could not be used safely.",
            Some(33) => "Could not download the ZeroTier installer.",
            Some(34) => "Could not verify or install the ZeroTier package.",
            Some(35) => "The local ZeroTier service could not be prepared.",
            Some(36) => "Could not prepare device verification for this Windows account.",
            Some(37) => "Network setup requires elevation with the same Windows account.",
            Some(38) => "Windows permission to prepare ZeroTier was cancelled.",
            _ => "Network setup was not completed.",
        }));
    }
    Ok(())
}

pub fn bootstrap(
    host: &str,
    token: &str,
    allow_setup_prompt: bool,
    progress: &dyn Fn(crate::settings::JoinStage),
) -> io::Result<Membership> {
    let home = home()?;
    if !allow_setup_prompt && !device_is_prepared(&home) {
        return Err(io::Error::other(
            "Prepare the Uplink transport before joining headlessly",
        ));
    }
    let info = post(host, "bootstrap", json!({"token": token, "version":2}))?;
    let planet = STANDARD
        .decode(info["planet"].as_str().unwrap_or(""))
        .map_err(io::Error::other)?;
    if !(100..=65536).contains(&planet.len()) {
        return Err(io::Error::other("Invalid ZeroTier root"));
    }
    progress(crate::settings::JoinStage::Device);
    if allow_setup_prompt {
        prepare_device(&home, host, token)?;
    }
    if fs::read(home.join("planet"))? != planet {
        return Err(io::Error::other(
            "This network requires a different ZeroTier root configuration",
        ));
    }
    Ok(Membership {
        host: host.into(),
        avatar_revision: None,
        methods: None,
        id: hex(
            &info["id"],
            if info.get("transport").is_some() {
                32
            } else {
                16
            },
        )?,
        transport: info.get("transport").map(|v| hex(v, 16)).transpose()?,
        name: info["name"].as_str().unwrap_or(host).into(),
        node: hex(&query(&["info"])?["address"], 10)?,
        session: format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        ),
        own_ip: String::new(),
        rows: Vec::new(),
        server_ms: None,
        health: "ok".into(),
    })
}

pub fn interface(id: &str) -> io::Result<Option<Value>> {
    let list = query(&["listnetworks"])?;
    Ok(list
        .as_array()
        .ok_or_else(|| io::Error::other("Invalid ZeroTier network list"))?
        .iter()
        .find(|n| n["id"] == id)
        .cloned())
}

pub fn attach(id: &str) -> io::Result<()> {
    zt(&["join", id]).map(|_| ())
}

fn retry_redeem(
    mut redeem: impl FnMut() -> io::Result<Value>,
    mut wait: impl FnMut(Duration) -> bool,
) -> io::Result<Value> {
    let mut delay = Duration::from_millis(400);
    loop {
        match redeem() {
            Err(e) if e.kind() == io::ErrorKind::WouldBlock && wait(delay) => {
                delay = (delay * 2).min(Duration::from_millis(1600));
            }
            result => return result,
        }
    }
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
pub fn detach(id: &str) -> io::Result<()> {
    if interface(id)?.is_some() {
        zt(&["leave", id])?;
    }
    Ok(())
}

fn peer_ping(source: &str, destination: &str) -> Option<u32> {
    use std::{net::Ipv4Addr, ptr};
    use windows_sys::Win32::{Foundation::INVALID_HANDLE_VALUE, NetworkManagement::IpHelper::*};
    let source = source.parse::<Ipv4Addr>().ok()?;
    let target = destination.parse::<Ipv4Addr>().ok()?;
    if !source.is_private()
        || source.octets()[..3] != target.octets()[..3]
        || matches!(target.octets()[3], 0 | 255)
    {
        return None;
    }
    let payload = b"Uplink";
    let mut buffer = [0usize; 16];
    // Буфер выровнен для ICMP_ECHO_REPLY и остаётся жив до завершения синхронного вызова.
    unsafe {
        let handle = IcmpCreateFile();
        if handle == INVALID_HANDLE_VALUE {
            return None;
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
            600,
        );
        IcmpCloseHandle(handle);
        if received == 0 {
            return None;
        }
        let reply = &*buffer.as_ptr().cast::<ICMP_ECHO_REPLY>();
        (reply.Status == 0 && reply.Address == u32::from_ne_bytes(target.octets()))
            .then_some(reply.RoundTripTime)
    }
}

pub fn shared_power(id: &str, enabled: bool) -> io::Result<()> {
    let Some(net) = interface(id)? else {
        return if enabled { attach(id) } else { Ok(()) };
    };
    let mac = net["mac"].as_str().unwrap_or("").replace(':', "-");
    // Значения передаются через окружение, чтобы имя интерфейса не интерпретировалось оболочкой.
    let script = r#"$ErrorActionPreference='Stop'
$a=@(Get-NetAdapter -IncludeHidden | Where-Object { $_.InterfaceDescription -like 'ZeroTier Virtual Port*' -and
    ($_.MacAddress -eq $env:UPLINK_DEVICE_MAC -or $_.Name -eq ('ZeroTier One ['+$env:UPLINK_DEVICE_ID+']')) })
if($a.Count -ne 1){exit 2}
if($env:UPLINK_DEVICE_ENABLE -eq '1'){
    if($a[0].Status -eq 'Disabled'){$a[0] | Enable-NetAdapter -Confirm:$false}
} elseif($a[0].Status -ne 'Disabled'){$a[0] | Disable-NetAdapter -Confirm:$false}"#;
    let exe = PathBuf::from(
        std::env::var_os("SystemRoot").ok_or_else(|| io::Error::other("Windows unavailable"))?,
    )
    .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let result = Command::new(exe)
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("UPLINK_DEVICE_ID", id)
        .env("UPLINK_DEVICE_MAC", mac)
        .env("UPLINK_DEVICE_ENABLE", if enabled { "1" } else { "0" })
        .creation_flags(0x08000000)
        .output()?;
    if result.status.success() {
        Ok(())
    } else {
        Err(io::Error::other("Could not change Uplink adapter power"))
    }
}

impl Membership {
    pub fn interface_id(&self) -> &str {
        self.transport.as_deref().unwrap_or(&self.id)
    }

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
        prior: Option<&str>,
        progress: &dyn Fn(crate::settings::JoinStage),
    ) -> io::Result<()> {
        if interface(self.interface_id())?.is_none() {
            attach(self.interface_id())?;
        }
        let start = Instant::now();
        progress(crate::settings::JoinStage::Approval);
        let resume = returning
            && post(
                &self.host,
                "sync",
                json!({"session":self.session,"nick":nick}),
            )
            .is_ok();
        if !resume {
            let proof = crate::device_proof::sign(&home()?, &self.node, token, &self.session)?;
            let deadline = start + Duration::from_secs(25);
            retry_redeem(
                || {
                    post(
                        &self.host,
                        "redeem",
                        json!({"token":token,"node":self.node,"session":self.session,"nick":nick,"prior_session":prior,"proof":proof}),
                    )
                },
                |delay| {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        return false;
                    }
                    thread::sleep(delay.min(remaining));
                    true
                },
            )?;
        }
        progress(crate::settings::JoinStage::Address);
        let net = wait_for_virtual_ip(start + Duration::from_secs(35), || {
            interface(self.interface_id())
        })?;
        self.refresh_using(nick, prefs, |_| Ok(Some(net)))
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

    pub fn refresh(&mut self, nick: &str, prefs: &std::path::Path) -> io::Result<()> {
        self.refresh_using(nick, prefs, interface)
    }

    fn refresh_using(
        &mut self,
        nick: &str,
        prefs: &std::path::Path,
        read_interface: impl FnOnce(&str) -> io::Result<Option<Value>>,
    ) -> io::Result<()> {
        let sync =
            json!({"session":self.session,"nick":nick,"avatar_revision":self.known_avatar(prefs)});
        let mut snapshot = post(&self.host, "sync", sync.clone());
        if snapshot.as_ref().is_ok_and(|v| v["needs_proof"] == true) {
            self.rows.clear();
            self.methods = None;
            snapshot = (|| {
                let proof =
                    crate::device_proof::sign(&home()?, &self.node, &self.id, &self.session)?;
                post(
                    &self.host,
                    "attest",
                    json!({"session":self.session,"proof":proof}),
                )?;
                post(&self.host, "sync", sync)
            })();
        }
        let avatar_status = match &snapshot {
            Ok(data) if data["id"] == self.id => self.apply_avatar(prefs, data),
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
                self.apply_avatar(prefs, &Value::Null)
            }
            _ => Ok(()),
        };
        let ms = if snapshot.is_ok() {
            request(&self.host, "ping", json!({}))
                .ok()
                .map(|(_, ms)| ms)
        } else {
            None
        };
        let net = read_interface(self.interface_id());
        let status = self.update_status(
            net.as_ref()
                .ok()
                .and_then(|it| it.as_ref())
                .unwrap_or(&Value::Null),
            snapshot,
            ms,
            peer_ping,
        );
        if net.is_err() {
            self.server_ms = None;
            for member in &mut self.rows {
                member.ping_ms = None;
                member.present = false;
            }
        }
        status.and(net.map(|_| ())).and(avatar_status)
    }

    fn update_status(
        &mut self,
        net: &Value,
        snapshot: io::Result<Value>,
        ms: Option<u32>,
        probe: fn(&str, &str) -> Option<u32>,
    ) -> io::Result<()> {
        self.server_ms = ms;
        if snapshot
            .as_ref()
            .is_err_and(|e| e.kind() == io::ErrorKind::PermissionDenied)
        {
            self.rows.clear();
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
            self.name = data["name"].as_str().unwrap_or(&self.host).to_owned();
        } else {
            for row in &mut self.rows {
                row.present = false;
            }
        }
        for group in self.rows.chunks_mut(8) {
            thread::scope(|scope| {
                for row in group {
                    if row.node == self.node {
                        row.ping_ms = self.server_ms;
                    } else {
                        let source = &self.own_ip;
                        scope.spawn(move || {
                            row.ping_ms = probe(source, &row.ip);
                            row.present |= row.ping_ms.is_some();
                        });
                    }
                }
            });
        }
        snapshot.map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invitation_pending_at_two_seconds_uses_four_requests() {
        let elapsed = std::cell::Cell::new(Duration::ZERO);
        let mut attempts = 0;
        let result = retry_redeem(
            || {
                attempts += 1;
                if elapsed.get() < Duration::from_secs(2) {
                    Err(io::ErrorKind::WouldBlock.into())
                } else {
                    Ok(json!({"id":"joined"}))
                }
            },
            |delay| {
                elapsed.set(elapsed.get() + delay);
                true
            },
        );
        assert_eq!(result.unwrap()["id"], "joined");
        assert_eq!(attempts, 4);
    }

    #[test]
    fn slow_controller_stops_at_the_retry_budget() {
        let mut pauses = Vec::new();
        let pending = retry_redeem(
            || Err(io::ErrorKind::WouldBlock.into()),
            |delay| {
                pauses.push(delay.as_millis());
                pauses.len() < 5
            },
        );
        assert_eq!(pending.unwrap_err().kind(), io::ErrorKind::WouldBlock);
        assert_eq!(pauses, [400, 800, 1600, 1600, 1600]);
        let denied = retry_redeem(
            || Err(io::ErrorKind::PermissionDenied.into()),
            |_| panic!("a rejected invitation must not be retried"),
        );
        assert_eq!(denied.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
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
    fn https_outage_keeps_adapter_ip_and_rechecks_cached_peers() {
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
                |source, target| {
                    assert_eq!(source, "10.3.4.5");
                    (target == "10.3.4.6").then_some(7)
                }
            )
            .is_err()
        );
        assert_eq!(m.own_ip, "10.3.4.5");
        assert_eq!(m.server_ms, None);
        assert!(m.rows[0].present && !m.rows[1].present);
        assert_eq!(m.rows[0].ping_ms, Some(7));
        assert_eq!(m.rows[1].ping_ms, None);
        assert!(
            m.update_status(
                &net,
                Err(io::ErrorKind::PermissionDenied.into()),
                None,
                |_, _| panic!()
            )
            .is_err()
        );
        assert!(m.rows.is_empty() && m.own_ip.is_empty());
    }

    #[test]
    fn old_profiles_and_logical_networks_resolve_their_interface() {
        let mut data = json!({"host":"example.org","id":"1111111111111111","session":"s","node":"n","name":"A"});
        let old: Membership = serde_json::from_value(data.clone()).unwrap();
        assert_eq!(old.interface_id(), old.id);
        data["transport"] = json!("2222222222222222");
        let group: Membership = serde_json::from_value(data).unwrap();
        assert_eq!(group.interface_id(), "2222222222222222");
        assert_eq!(group.key(), old.key());
    }
}
