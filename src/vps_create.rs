use crate::{invite, settings, vps::Target};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD},
};
use flate2::{Compression, write::ZlibEncoder};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    io::{self, BufRead, BufReader, Read, Write},
    net::Ipv4Addr,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Creation {
    pub name: String,
    pub ip: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub identity: String,
    pub fingerprint: String,
    pub trust_on_first_use: bool,
    pub public_host: String,
    pub proxy_bind: String,
    pub udp_port: Option<u16>,
}

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, msg)
}
pub fn field_error(field: &str, value: &str) -> &'static str {
    match field {
        "name"
            if value.trim().is_empty()
                || value.trim().chars().count() > 64
                || value.chars().any(char::is_control) =>
        {
            "Enter a network name (1 to 64 characters)."
        }
        "ip" if value.parse::<Ipv4Addr>().is_err() => "Enter an IPv4 address.",
        "port" if !value.trim().parse::<u16>().is_ok_and(|n| n > 0) => "Enter a port from 1 to 65535.",
        "user" if Target::parse(&["127.0.0.1", "22", value]).is_err() => "Enter the SSH login.",
        "proxy_host" if !invite::valid_host(value) => "Enter the HTTPS proxy hostname.",
        "proxy_bind" if !value.parse::<Ipv4Addr>().is_ok_and(|ip| ip.is_private() || ip.is_loopback()) =>
            "Enter the private IPv4 address of the VPS.",
        "fingerprint"
            if !value
                .trim()
                .strip_prefix("SHA256:")
                .and_then(|v| STANDARD_NO_PAD.decode(v).ok())
                .is_some_and(|v| v.len() == 32) =>
        {
            "Enter the SHA256 fingerprint from your hosting provider."
        }
        _ => "",
    }
}
impl Creation {
    pub fn validate(&self) -> io::Result<()> {
        self.validate_access()?;
        let name_error = field_error("name", &self.name);
        if !name_error.is_empty() {
            return Err(invalid(name_error));
        }
        if !self.public_host.is_empty() && !invite::valid_host(&self.public_host)
            || self.public_host.is_empty() != self.proxy_bind.is_empty()
            || self.udp_port == Some(0)
            || !self.proxy_bind.is_empty() && self.proxy_bind.parse::<Ipv4Addr>().is_err()
        {
            return Err(invalid("Invalid public endpoint configuration."));
        }
        Ok(())
    }

    fn validate_access(&self) -> io::Result<()> {
        for (field, value) in [
            ("ip", self.ip.as_str()),
            ("user", &self.user),
            ("port", &self.port.to_string()),
            ("fingerprint", &self.fingerprint),
        ] {
            if field == "fingerprint" && value.is_empty() && self.trust_on_first_use {
                continue;
            }
            let error = field_error(field, value);
            if !error.is_empty() {
                return Err(invalid(error));
            }
        }
        if self.identity.is_empty() == self.password.is_empty() {
            return Err(invalid("Enter a VPS password or choose an SSH key."));
        }
        if !self.identity.is_empty() && !Path::new(&self.identity).is_file() {
            return Err(invalid("SSH file not found"));
        }
        Ok(())
    }

    fn host(&self) -> &str {
        if self.public_host.is_empty() {
            &self.ip
        } else {
            &self.public_host
        }
    }
}

fn openssh(name: &str) -> io::Result<PathBuf> {
    Ok(PathBuf::from(
        std::env::var_os("SystemRoot")
            .ok_or_else(|| io::Error::other("Windows directory unavailable"))?,
    )
    .join("System32/OpenSSH")
    .join(name))
}

fn known_hosts_option(pin: &Path) -> io::Result<String> {
    let pin = pin.to_string_lossy().replace('\\', "/");
    if pin.contains(['"', '%', '$', '\n', '\r']) {
        return Err(invalid("Invalid SSH file path"));
    }
    Ok(format!("UserKnownHostsFile=\"{pin}\""))
}

fn pinned_keys(config: &Creation, prefs: &Path) -> io::Result<tempfile::TempDir> {
    let directory = tempfile::tempdir()?;
    let pin = directory.path().join("known_hosts");
    // Этот обмен получает только открытый ключ. Пароль и ключ пользователя передаются
    // отдельному SSH-процессу после проверки сохранённого отпечатка.
    Command::new(openssh("ssh.exe")?)
        .args(["-F", "none", "-T", "-o", "BatchMode=yes", "-o", "PreferredAuthentications=none",
            "-o", "IdentityAgent=none", "-o", "StrictHostKeyChecking=accept-new",
            "-o", "GlobalKnownHostsFile=none", "-o", "UpdateHostKeys=no", "-o", "HashKnownHosts=no",
            "-o", "HostKeyAlgorithms=ssh-ed25519", "-o", "ConnectTimeout=7", "-o", "ConnectionAttempts=1",
            "-o", &known_hosts_option(&pin)?, "-p", &config.port.to_string(), "-l", &config.user, &config.ip, "exit"])
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
        .creation_flags(0x08000000).status()?;
    let keys = std::fs::read_to_string(&pin).map_err(|_| io::Error::other("Could not obtain the VPS SSH key. Check the address and SSH port."))?;
    for line in keys.lines() {
        let mut fields = line.split_whitespace();
        if let (Some(_), Some("ssh-ed25519"), Some(key), None) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        {
            if let Ok(raw) = STANDARD.decode(key) {
                let fingerprint = format!("SHA256:{}", STANDARD_NO_PAD.encode(Sha256::digest(raw)));
                if !config.fingerprint.is_empty() && fingerprint != config.fingerprint.trim() {
                    return Err(io::Error::other("Could not verify the VPS Ed25519 fingerprint."));
                }
                settings::remember_vps(prefs, &format!("{}:{}", config.ip, config.port), &fingerprint)?;
                let host = if config.port == 22 {
                    config.ip.clone()
                } else {
                    format!("[{}]:{}", config.ip, config.port)
                };
                std::fs::write(
                    directory.path().join("known_hosts"),
                    format!("{host} ssh-ed25519 {key}\n"),
                )?;
                return Ok(directory);
            }
        }
    }
    Err(io::Error::other(
        "Could not verify the VPS Ed25519 fingerprint.",
    ))
}

macro_rules! bundle { ($($f:literal),+) => { json!({$($f:include_str!(concat!("../server/",$f)).replace("\r\n","\n")),+}) }; }
fn packet(request: Value) -> io::Result<String> {
    let files = bundle!(
        "provision.py",
        "instance.py",
        "membership.py",
        "easytier.py",
        "engine_config.py",
        "engine.py",
        "rpc_guard.py",
        "client_api.py",
        "uplink-engine.service",
        "uplink-rpc-guard.service",
        "uplink-owner.socket",
        "uplink-owner.service",
        "uplink-api.service",
        "upgrade.py",
        "revision.txt"
    );
    let mut body = ZlibEncoder::new(Vec::new(), Compression::fast());
    serde_json::to_writer(&mut body, &json!({"files":files,"request":request}))?;
    Ok(STANDARD.encode(body.finish()?))
}

fn connection(c: &Creation, pin: &Path) -> io::Result<Child> {
    let mut cmd = Command::new(openssh("ssh.exe")?);
    cmd.creation_flags(0x08000000).args([
        "-F",
        "none",
        "-T",
        "-o",
        "StrictHostKeyChecking=yes",
        "-o",
        "UpdateHostKeys=no",
        "-o",
        "HostKeyAlgorithms=ssh-ed25519",
        "-o",
        "GlobalKnownHostsFile=none",
        "-o",
        "ClearAllForwardings=yes",
        "-o",
        "ForwardAgent=no",
        "-o",
        "ConnectTimeout=7",
        "-o",
        "ConnectionAttempts=1",
        "-o",
        "NumberOfPasswordPrompts=1",
        "-o",
        "ServerAliveInterval=25",
        "-o",
        "ServerAliveCountMax=2",
    ]);
    cmd.arg("-o").arg(known_hosts_option(pin)?);
    if c.identity.is_empty() {
        cmd.args([
            "-o",
            "PreferredAuthentications=password",
            "-o",
            "PubkeyAuthentication=no",
        ])
        .env("SSH_ASKPASS_REQUIRE", "force")
        .env("DISPLAY", "uplink")
        .env(
            "SSH_ASKPASS",
            std::env::current_exe()?.with_file_name("uplink.exe"),
        )
        .env("UPLINK_SSH_ASKPASS", &c.password);
    } else {
        cmd.args(["-o", "BatchMode=yes", "-o", "IdentitiesOnly=yes", "-i"])
            .arg(&c.identity);
    }
    // Передаём только статический скрипт. Все параметры VPS и сети идут через stdin.
    let script = include_str!("../server/setup_entry.py").replace("\r\n", "\n");
    let encoded = STANDARD.encode(script);
    let prefix = if c.user == "root" { "" } else { "sudo -n " };
    let remote = format!(
        "{prefix}python3 -I -B -u -c \"import base64;exec(base64.b64decode('{encoded}'))\""
    );
    cmd.args(["-p", &c.port.to_string(), "-l", &c.user, &c.ip, &remote])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
}

fn read_reply(reader: &mut impl BufRead) -> io::Result<Value> {
    let mut line = String::new();
    reader.take(2048).read_line(&mut line)?;
    if !line.ends_with('\n') {
        return Err(io::Error::other(
            "The SSH setup session ended. Check the login, key and administrator access.",
        ));
    }
    Ok(serde_json::from_str(&line)?)
}

fn reply(reader: &mut impl BufRead) -> io::Result<Value> {
    let value = read_reply(reader)?;
    if let Some(msg) = value["error"].as_str() {
        return Err(io::Error::other(msg.to_owned()));
    }
    Ok(value)
}

fn authenticated(c: &Creation, path: &Path) -> io::Result<(
    std::process::ChildStdin, BufReader<std::process::ChildStdout>, mpsc::Sender<()>,
)> {
    let keys = pinned_keys(c, path)?;
    let mut process = connection(c, &keys.path().join("known_hosts"))?;
    let input = process.stdin.take().unwrap();
    let mut output = BufReader::new(process.stdout.take().unwrap());
    let (lifetime, end) = mpsc::channel::<()>();
    thread::spawn(move || {
        // Файл закреплённого ключа нужен до завершения SSH-аутентификации.
        let _keys = keys;
        let _ = end.recv_timeout(Duration::from_secs(1200));
        let _ = process.kill();
        let _ = process.wait();
    });
    if reply(&mut output)?["stage"] != "authenticated" {
        return Err(io::Error::other("Unexpected SSH response."));
    }
    Ok((input, output, lifetime))
}

fn send_bundle(c: &Creation, input: &mut impl Write, request: Value) -> io::Result<()> {
    let body = packet(request)?;
    for chunk in body.as_bytes().chunks(850) {
        input.write_all(chunk)?;
        if !c.public_host.is_empty() || c.port != 22 {
            input.flush()?;
            thread::sleep(Duration::from_millis(250));
        }
    }
    writeln!(input)?;
    input.flush()
}

fn ticket(v: &Value, host: &str) -> io::Result<String> {
    let url = format!(
        "uplink://{host}/invite/{}",
        v["token"].as_str().unwrap_or_default()
    );
    if invite::instance_link(&url).is_none() {
        return Err(io::Error::other("Invalid invitation from the VPS."));
    }
    Ok(url)
}

pub struct CreatedNetwork {
    pub invitation: String,
    pub key: String,
}

pub fn create(
    c: Creation,
    path: &Path,
    allow_prompt: bool,
    step: impl Fn(i32),
) -> io::Result<CreatedNetwork> {
    c.validate()?;
    step(1);
    let (mut input, mut output, _lifetime) = authenticated(&c, path)?;
    step(2);
    let scope = format!("{}:{}|{}|{}", c.ip, c.port, c.host(), c.name.trim());
    let group = settings::creation_id(path, &scope)?;
    send_bundle(&c, &mut input, json!({"name":c.name.trim(),"ip":c.ip,"port":c.udp_port.unwrap_or(9993),
        "host":c.host(),"external_tls":!c.public_host.is_empty(),"bind":if c.proxy_bind.is_empty(){"127.0.0.1"}else{&c.proxy_bind},"group":group}))?;
    let created = reply(&mut output)?;
    if created["group"] != group {
        return Err(io::Error::other("Unexpected network identifier from VPS."));
    }
    let host = created["host"]
        .as_str()
        .filter(|s| invite::valid_host(s) && (c.public_host.is_empty() || *s == c.public_host))
        .ok_or_else(|| invalid("Invalid public endpoint configuration."))?;
    let enrollment = ticket(&created, host)?;
    step(3);
    let membership = || -> io::Result<Option<crate::transport::Membership>> {
        Ok(settings::rooms(path)?
            .into_iter()
            .filter_map(|r| r.live)
            .find(|r| r.id == group && r.host == host))
    };
    if membership()?.is_none() {
        settings::join_with_progress(path, &enrollment, allow_prompt, &|_| {})?;
    }
    let own =
        membership()?.ok_or_else(|| io::Error::other("Could not find the owner's membership."))?;
    step(4);
    writeln!(input, "{}", json!({"node":own.node}))?;
    input.flush()?;
    drop(input);
    let invitation = ticket(&reply(&mut output)?, host)?;
    settings::finish_creation(path, &scope, &own)?;
    let _ = crate::changed::notify();
    Ok(CreatedNetwork { invitation, key: own.key() })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Upgrade {
    pub server: String,
    pub access: Creation,
}

pub struct UpgradeReport {
    pub result: String,
    pub detail: String,
    pub log_file: PathBuf,
    pub log_text: String,
}

pub fn server_revision() -> u16 {
    include_str!("../server/revision.txt").trim().parse().expect("invalid bundled server revision")
}

pub fn upgrade(request: Upgrade, path: &Path, progress: impl Fn(i32)) -> io::Result<UpgradeReport> {
    let Upgrade { server, mut access } = request;
    access.validate_access()?;
    if !invite::valid_host(&server) {
        return Err(invalid("Enter the address shown in the network menu."));
    }
    // Прокси и нестандартный SSH-порт используют тот же темп отправки, что и установка.
    if server != access.ip {
        access.public_host = server.clone();
    }
    let log_file = path.with_file_name("server-upgrade.log");
    std::fs::create_dir_all(log_file.parent().ok_or_else(|| invalid("Invalid profile directory"))?)?;
    let attempt = uuid::Uuid::new_v4().to_string();
    let mut log_text = String::new();
    let mut record = |fields: Value| {
        let line = format!("{}\n", json!({"attempt":attempt,"server":server,"time":chrono::Utc::now().to_rfc3339(),"event":fields}));
        let _ = settings::diagnostics::append(&log_file, &line);
        log_text.push_str(&line);
    };
    progress(1);
    record(json!({"stage":"ssh","target":server_revision()}));
    let mut dispatched = false;
    let result: io::Result<Value> = (|| {
        let (mut input, mut output, _lifetime) = authenticated(&access, path)?;
        record(json!({"stage":"authenticated"}));
        progress(2);
        send_bundle(&access, &mut input, json!({"operation":"upgrade","host":server,"version":server_revision()}))?;
        dispatched = true;
        drop(input);
        for _ in 0..1000 {
            let event = read_reply(&mut output)?;
            if let Some(detail) = event["error"].as_str() {
                let response = json!({"stage":"done","result":"interrupted","detail":detail});
                record(response.clone());
                return Ok(response);
            }
            let stage = match event["stage"].as_str() {
                Some("queued" | "prepare") => 2,
                Some("backup") => 3,
                Some("apply") => 4,
                Some("verify") => 5,
                Some("rollback") => 6,
                Some("done") => 0,
                _ => return Err(io::Error::other("Unexpected SSH response.")),
            };
            let fields = ["stage", "time", "target", "source", "version", "job", "failure", "result", "detail"];
            record(Value::Object(fields.into_iter().filter_map(|field| event.get(field).map(|value| (field.to_owned(), value.clone()))).collect()));
            if stage == 0 {
                return Ok(event);
            }
            progress(stage);
        }
        Err(io::Error::other("Unexpected SSH response."))
    })();
    let (result, detail) = match result {
        Ok(event) => (
            event["result"].as_str().unwrap_or("interrupted").to_owned(),
            event["detail"].as_str().unwrap_or_default().to_owned(),
        ),
        Err(error) => {
            let result = if dispatched { "interrupted" } else { "failed" };
            let kind = format!("{:?}", error.kind());
            record(json!({"stage":"finish","result":result,"kind":kind,"win32":error.raw_os_error()}));
            (result.to_owned(), error.to_string())
        }
    };
    Ok(UpgradeReport { result, detail, log_file, log_text })
}
