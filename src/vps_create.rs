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
        "port" if !value.parse::<u16>().is_ok_and(|n| n > 0) => "Enter a port from 1 to 65535.",
        "user" if Target::parse(&["127.0.0.1", "22", value]).is_err() => "Enter the SSH login.",
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
        for (field, value) in [
            ("name", self.name.as_str()),
            ("ip", &self.ip),
            ("user", &self.user),
            ("port", &self.port.to_string()),
            ("fingerprint", &self.fingerprint),
        ] {
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
        if !self.public_host.is_empty() && !invite::valid_host(&self.public_host)
            || self.public_host.is_empty() != self.proxy_bind.is_empty()
            || self.udp_port == Some(0)
            || !self.proxy_bind.is_empty() && self.proxy_bind.parse::<Ipv4Addr>().is_err()
        {
            return Err(invalid("Invalid public endpoint configuration."));
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

fn pinned_keys(config: &Creation) -> io::Result<tempfile::TempDir> {
    let scan = Command::new(openssh("ssh-keyscan.exe")?)
        .args([
            "-T",
            "6",
            "-p",
            &config.port.to_string(),
            "-t",
            "ed25519",
            &config.ip,
        ])
        .creation_flags(0x08000000)
        .output()?;
    let directory = tempfile::tempdir()?;
    for line in String::from_utf8_lossy(&scan.stdout).lines() {
        let mut fields = line.split_whitespace();
        if let (Some(_), Some("ssh-ed25519"), Some(key), None) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        {
            if let Ok(raw) = STANDARD.decode(key) {
                if format!("SHA256:{}", STANDARD_NO_PAD.encode(Sha256::digest(raw)))
                    == config.fingerprint.trim()
                {
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
    }
    Err(io::Error::other(
        "Could not verify the VPS Ed25519 fingerprint.",
    ))
}

macro_rules! bundle { ($($f:literal),+) => { json!({$($f:include_str!(concat!("../server/",$f)).replace("\r\n","\n")),+}) }; }
fn packet(config: &Creation, group: &str) -> io::Result<String> {
    let files = bundle!(
        "provision.py",
        "prepare_root.py",
        "instance.py",
        "groups.py",
        "invites.py",
        "device_proof.py",
        "client_api.py",
        "uplink-zt.service",
        "uplink-client-api.service",
        "uplink-instance.service",
        "uplink-instance.socket"
    );
    let request = json!({"name":config.name.trim(),"ip":config.ip,"port":config.udp_port.unwrap_or(9993),
        "host":config.host(),"external_tls":!config.public_host.is_empty(),"bind":if config.proxy_bind.is_empty(){"127.0.0.1"}else{&config.proxy_bind},"group":group});
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
    let pin = pin.to_string_lossy().replace('\\', "/");
    if pin.contains(['"', '%', '$', '\n', '\r']) {
        return Err(invalid("Invalid SSH file path"));
    }
    cmd.arg("-o").arg(format!("UserKnownHostsFile=\"{pin}\""));
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

fn reply(reader: &mut impl BufRead) -> io::Result<Value> {
    let mut line = String::new();
    reader.take(2048).read_line(&mut line)?;
    if !line.ends_with('\n') {
        return Err(io::Error::other(
            "The SSH setup session ended. Check the login, key and administrator access.",
        ));
    }
    let value: Value = serde_json::from_str(&line)?;
    if let Some(msg) = value["error"].as_str() {
        return Err(io::Error::other(msg.to_owned()));
    }
    Ok(value)
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

pub fn create(
    c: Creation,
    path: &Path,
    allow_prompt: bool,
    step: impl Fn(i32),
) -> io::Result<String> {
    c.validate()?;
    step(1);
    let keys = pinned_keys(&c)?;
    let mut process = connection(&c, &keys.path().join("known_hosts"))?;
    let mut input = process.stdin.take().unwrap();
    let mut output = BufReader::new(process.stdout.take().unwrap());
    // Закрытие канала и таймер завершают дочерний SSH даже при ошибке чтения или зависшем VPS.
    let (_lifetime, end) = mpsc::channel::<()>();
    thread::spawn(move || {
        let _ = end.recv_timeout(Duration::from_secs(900));
        let _ = process.kill();
        let _ = process.wait();
    });
    if reply(&mut output)?["stage"] != "authenticated" {
        return Err(io::Error::other("Unexpected SSH response."));
    }
    settings::remember_vps(path, &format!("{}:{}", c.ip, c.port), c.fingerprint.trim())?;
    step(2);
    let scope = format!("{}:{}|{}|{}", c.ip, c.port, c.host(), c.name.trim());
    let group = settings::creation_id(path, &scope)?;
    let body = packet(&c, &group)?;
    for chunk in body.as_bytes().chunks(850) {
        input.write_all(chunk)?;
        if !c.public_host.is_empty() || c.port != 22 {
            input.flush()?;
            thread::sleep(Duration::from_millis(250));
        }
    }
    writeln!(input)?;
    input.flush()?;
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
    Ok(invitation)
}
