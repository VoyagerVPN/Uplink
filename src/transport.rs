use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, Write},
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
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Membership {
    pub host: String,
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

fn request(host: &str, endpoint: &str, body: Value) -> io::Result<(Value, u32)> {
    let windows = std::env::var_os("SystemRoot")
        .ok_or_else(|| io::Error::other("Windows directory unavailable"))?;
    let mut child = Command::new(PathBuf::from(windows).join("System32/curl.exe"))
        .args([
            "-q",
            "-sS",
            "--proto",
            "=https",
            "--connect-timeout",
            "4",
            "--max-time",
            "8",
            "--max-filesize",
            "262144",
            "-H",
            "Content-Type: application/json",
            "--data-binary",
            "@-",
            "-w",
            "\n%{http_code} %{time_pretransfer} %{time_starttransfer}",
            &format!("https://{host}/v1/{endpoint}"),
        ])
        .creation_flags(0x08000000)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(&body)?)?;
    let out = child.wait_with_output()?;
    if !out.status.success() {
        return Err(io::Error::other("Could not reach Uplink over HTTPS"));
    }
    let split = out
        .stdout
        .iter()
        .rposition(|&b| b == b'\n')
        .ok_or_else(|| io::Error::other("Invalid HTTPS response"))?;
    let metrics = std::str::from_utf8(&out.stdout[split + 1..]).map_err(io::Error::other)?;
    let mut metrics = metrics.split_whitespace();
    match metrics.next() {
        Some("200") => {
            let mut timing = || {
                metrics
                    .next()
                    .and_then(|n| n.parse::<f64>().ok())
                    .ok_or_else(|| io::Error::other("Invalid HTTPS timing"))
            };
            let sent = timing()?;
            let received = timing()?;
            Ok((
                serde_json::from_slice(&out.stdout[..split])?,
                ((received - sent).max(0.0) * 1000.0).round() as u32,
            ))
        }
        Some("409")
            if serde_json::from_slice::<Value>(&out.stdout[..split])
                .ok()
                .is_some_and(|v| v["error"] == "Network name already in use") =>
        {
            Err(io::Error::other("Network name already in use"))
        }
        Some("429") => Err(io::Error::other("Too many login attempts")),
        Some("403") if endpoint == "login" => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Invalid network credentials",
        )),
        Some("409") => Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "Waiting for network join",
        )),
        Some("403") => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Invitation or membership denied",
        )),
        _ => Err(io::Error::other("Uplink API unavailable")),
    }
}

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

fn prepare_device(home: &std::path::Path, host: &str, token: &str) -> io::Result<()> {
    if home.join("zerotier-one_x64.exe").is_file()
        && home.join("uplink-proof/certificate").is_file()
    {
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
            "$ErrorActionPreference='Stop'; $sid=[Security.Principal.WindowsIdentity]::GetCurrent().User.Value; $p=Start-Process -FilePath $env:UPLINK_NETWORK_HELPER -Verb RunAs -ArgumentList ('prepare '+$sid+' '+$env:UPLINK_NETWORK_REQUEST) -WindowStyle Hidden -Wait -PassThru; exit $p.ExitCode"])
        .env("UPLINK_NETWORK_HELPER",helper).env("UPLINK_NETWORK_REQUEST",request)
        .creation_flags(0x08000000).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status()?;
    drop(guard);
    let _ = fs::remove_dir(root);
    if !result.success() {
        return Err(io::Error::other("Network setup was not completed."));
    }
    Ok(())
}

pub fn bootstrap(host: &str, token: &str) -> io::Result<Membership> {
    let info = post(host, "bootstrap", json!({"token": token, "version":2}))?;
    let planet = STANDARD
        .decode(info["planet"].as_str().unwrap_or(""))
        .map_err(io::Error::other)?;
    if !(100..=65536).contains(&planet.len()) {
        return Err(io::Error::other("Invalid ZeroTier root"));
    }
    let home = home()?;
    prepare_device(&home, host, token)?;
    if fs::read(home.join("planet"))? != planet {
        return Err(io::Error::other(
            "This network requires a different ZeroTier root configuration",
        ));
    }
    Ok(Membership {
        host: host.into(),
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
        returning: bool,
        prior: Option<&str>,
    ) -> io::Result<()> {
        if interface(self.interface_id())?.is_none() {
            attach(self.interface_id())?;
        }
        let start = Instant::now();
        let resume = returning
            && post(
                &self.host,
                "sync",
                json!({"session":self.session,"nick":nick}),
            )
            .is_ok();
        if !resume {
            let proof = crate::device_proof::sign(&home()?, &self.node, token, &self.session)?;
            loop {
                match post(
                    &self.host,
                    "redeem",
                    json!({"token":token,"node":self.node,"session":self.session,"nick":nick,"prior_session":prior,"proof":proof}),
                ) {
                    Err(e)
                        if e.kind() == io::ErrorKind::WouldBlock
                            && start.elapsed() < Duration::from_secs(25) =>
                    {
                        thread::sleep(Duration::from_millis(400))
                    }
                    result => {
                        result?;
                        break;
                    }
                }
            }
        }
        loop {
            self.refresh(nick)?;
            if !self.own_ip.is_empty() {
                return Ok(());
            }
            if start.elapsed() > Duration::from_secs(35) {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Waiting for virtual IP",
                ));
            }
            thread::sleep(Duration::from_millis(500));
        }
    }

    pub fn manage(&self, action: &str, value: &str) -> io::Result<()> {
        post(
            &self.host,
            "manage",
            json!({"session":self.session,"action":action,"value":value}),
        )
        .map(|_| ())
    }
    pub fn share(&self) -> io::Result<String> {
        let response = post(&self.host, "share", json!({"session":self.session}))?;
        Ok(format!(
            "uplink://{}/invite/{}",
            self.host,
            hex(&response["token"], 64)?
        ))
    }

    pub fn refresh(&mut self, nick: &str) -> io::Result<()> {
        let mut snapshot = post(
            &self.host,
            "sync",
            json!({"session":self.session,"nick":nick}),
        );
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
                post(
                    &self.host,
                    "sync",
                    json!({"session":self.session,"nick":nick}),
                )
            })();
        }
        let ms = if snapshot.is_ok() {
            request(&self.host, "ping", json!({}))
                .ok()
                .map(|(_, ms)| ms)
        } else {
            None
        };
        let net = interface(self.interface_id());
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
        status.and(net.map(|_| ()))
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
        self.own_ip = net["assignedAddresses"]
            .as_array()
            .filter(|_| net["status"] == "OK")
            .and_then(|ips| {
                ips.iter().filter_map(Value::as_str).find_map(|s| {
                    s.split_once('/')
                        .and_then(|(ip, _)| ip.parse::<std::net::Ipv4Addr>().ok())
                })
            })
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
