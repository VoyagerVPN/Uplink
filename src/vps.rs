use serde_json::{Value, json};
use std::{
    io::{self, Read},
    net::Ipv4Addr,
    os::windows::process::CommandExt,
    path::PathBuf,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

// Только фиксированные проверки. Адрес, имя пользователя и пути не попадают в удалённую оболочку.
const PROBE: &str = r#"printf 'uplink-vps-v1\n'
uname -s
uname -m
if [ "$(cat /proc/1/comm 2>/dev/null)" = systemd ]; then echo yes; else echo no; fi
if [ -c /dev/net/tun ]; then echo yes; else echo no; fi
if command -v python3 >/dev/null 2>&1; then echo yes; else echo no; fi
if [ "$(id -u)" = 0 ]; then echo root; elif sudo -n true >/dev/null 2>&1; then echo sudo; else echo none; fi
if [ -d /var/lib/uplink-engine ]; then echo yes; else echo no; fi"#;

pub struct Target {
    address: Ipv4Addr,
    port: u16,
    user: String,
    identity: Option<PathBuf>,
    known_hosts: Option<PathBuf>,
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

impl Target {
    pub fn parse(args: &[&str]) -> io::Result<Self> {
        let [host, port, user, options @ ..] = args else {
            return Err(invalid("Expected IPv4 address, SSH port and SSH user"));
        };
        let address = host
            .parse()
            .map_err(|_| invalid("Invalid VPS IPv4 address"))?;
        let port = port
            .parse::<u16>()
            .ok()
            .filter(|v| *v != 0)
            .ok_or_else(|| invalid("Invalid SSH port"))?;
        if user.is_empty()
            || user.len() > 64
            || user.starts_with('-')
            || !user
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.'))
        {
            return Err(invalid("Invalid SSH user"));
        }
        let mut target = Self {
            address,
            port,
            user: user.to_string(),
            identity: None,
            known_hosts: None,
        };
        for pair in options.chunks(2) {
            let [flag, file] = pair else {
                return Err(invalid("Expected an SSH file path"));
            };
            let field = match *flag {
                "--identity" if target.identity.is_none() => &mut target.identity,
                "--known-hosts" if target.known_hosts.is_none() => &mut target.known_hosts,
                _ => return Err(invalid("Unexpected SSH option")),
            };
            if file.is_empty()
                || file
                    .chars()
                    .any(|c| c.is_control() || matches!(c, '"' | '%' | '$'))
            {
                return Err(invalid("Invalid SSH file path"));
            }
            let path = std::path::absolute(file)?;
            if !path.is_file() {
                return Err(invalid("SSH file not found"));
            }
            *field = Some(path);
        }
        Ok(target)
    }

    pub fn check(&self) -> io::Result<Value> {
        let windows = std::env::var_os("SystemRoot")
            .ok_or_else(|| io::Error::other("Windows directory unavailable"))?;
        let mut ssh = Command::new(PathBuf::from(windows).join("System32/OpenSSH/ssh.exe"));
        ssh.args([
            "-F",
            "none",
            "-T",
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=yes",
            "-o",
            "UpdateHostKeys=no",
            "-o",
            "ClearAllForwardings=yes",
            "-o",
            "ForwardAgent=no",
            "-o",
            "ConnectionAttempts=1",
            "-o",
            "ConnectTimeout=6",
        ]);
        if let Some(path) = &self.identity {
            ssh.args(["-o", "IdentitiesOnly=yes", "-i"]).arg(path);
        }
        if let Some(path) = &self.known_hosts {
            let file = path.to_string_lossy().replace('\\', "/");
            ssh.arg("-o").arg(format!("UserKnownHostsFile=\"{file}\""));
        }
        let mut child = ssh
            .args([
                "-p",
                &self.port.to_string(),
                "-l",
                &self.user,
                &self.address.to_string(),
                PROBE,
            ])
            .creation_flags(0x08000000)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let deadline = Instant::now() + Duration::from_secs(15);
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                child.wait()?;
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "SSH preflight timed out",
                ));
            }
            thread::sleep(Duration::from_millis(30));
        };
        if !status.success() {
            let mut error = String::new();
            child
                .stderr
                .take()
                .unwrap()
                .take(8192)
                .read_to_string(&mut error)?;
            let reason = if error.contains("Host key verification failed")
                || error.contains("HOST IDENTIFICATION HAS CHANGED")
            {
                "SSH host key is unknown or has changed. Verify its fingerprint with the VPS owner."
            } else if error.contains("Permission denied") {
                "SSH public key authentication failed"
            } else {
                "SSH connection or remote preflight failed"
            };
            return Err(io::Error::other(reason));
        }
        let mut reply = String::new();
        child
            .stdout
            .take()
            .unwrap()
            .take(2048)
            .read_to_string(&mut reply)?;
        parse_report(&reply)
    }
}

fn parse_report(reply: &str) -> io::Result<Value> {
    let lines: Vec<_> = reply.lines().collect();
    let fail = || {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "Unexpected VPS preflight response",
        )
    };
    let [
        "uplink-vps-v1",
        os,
        arch,
        systemd,
        tun,
        python,
        access,
        existing,
    ] = lines.as_slice()
    else {
        return Err(fail());
    };
    let flag = |s: &str| match s {
        "yes" => Ok(true),
        "no" => Ok(false),
        _ => Err(fail()),
    };
    if !matches!(*access, "root" | "sudo" | "none") {
        return Err(fail());
    }
    let (systemd, tun, python) = (flag(systemd)?, flag(tun)?, flag(python)?);
    Ok(
        json!({"os":os,"architecture":arch,"systemd":systemd,"tun":tun,"python3":python,
        "privilege_check":access,"runtime_directory_present":flag(existing)?,
        "base_checks_passed":*os=="Linux" && *arch=="x86_64" && systemd && python && *access!="none"}),
    )
}

#[path = "vps_create.rs"]
mod creation;
pub use creation::{Creation, Upgrade, create, field_error, server_revision, upgrade};
