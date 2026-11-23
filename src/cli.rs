#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_extra_arguments_and_unknown_commands() {
        assert!(validate(&["members", "extra"]).is_err());
        assert!(validate(&["nick"]).is_err());
        assert!(validate(&["run", "anything"]).is_err());
        assert!(validate(&["nick", "Грай"]).is_ok());
        assert!(validate(&["language", "en"]).is_ok());
        assert!(validate(&["language", "missing"]).is_err());
    }

    #[test]
    fn vps_check_requires_one_ipv4_address_and_valid_ssh_credentials() {
        assert!(uplink::vps::Target::parse(&["192.0.2.10", "22", "ubuntu"]).is_ok());
        for (host, port, user) in [
            ("192.0.2.10/24", "22", "ubuntu"),
            ("192.0.2.10", "0", "ubuntu"),
            ("192.0.2.10", "22", "-oProxyCommand"),
        ] {
            assert!(uplink::vps::Target::parse(&[host, port, user]).is_err());
        }
    }
}

use serde_json::{Value, json};
use std::{
    io, thread,
    time::{Duration, Instant},
};
use uplink::{changed, control, invite, peers, settings};

const USAGE: &str = "uplinkctl vps create (JSON on stdin) | vps check IPV4 PORT USER [--identity FILE] [--known-hosts FILE] | update status|check|download|install | status | rooms | select URL | power on|off | language CODE | setting autostart|autoconnect|autoupdate on|off | demo-health ok|warning|critical | members | invite | nick NAME | rename-room NAME | join [--headless] URL | leave | tray green|ping|system | show | hide";

fn validate(args: &[&str]) -> io::Result<()> {
    match args {
        ["language", code] if uplink::i18n::supports(code) => Ok(()),
        ["status" | "rooms" | "members" | "invite" | "leave" | "show" | "hide"]
        | ["update", "status" | "check" | "download" | "install"]
        | ["power", "on" | "off"]
        | [
            "setting",
            "autostart" | "autoconnect" | "autoupdate",
            "on" | "off",
        ]
        | ["demo-health", "ok" | "warning" | "critical"]
        | ["select" | "nick" | "rename-room" | "join" | "tray", _]
        | ["join", "--headless", _] => Ok(()),
        _ => Err(io::Error::new(io::ErrorKind::InvalidInput, USAGE)),
    }
}

fn window_state() -> io::Result<&'static str> {
    if control::tray_window().is_null() {
        return Ok("stopped");
    }
    let hwnd = control::send(control::WINDOW)?;
    if hwnd == 0 {
        return Ok("tray");
    }
    if unsafe { windows_sys::Win32::UI::WindowsAndMessaging::IsIconic(hwnd as _) } != 0 {
        return Ok("minimized");
    }
    Ok("open")
}

fn change_window(show: bool) -> io::Result<()> {
    let state = window_state()?;
    if show {
        if state == "stopped" {
            std::process::Command::new(std::env::current_exe()?.with_file_name("uplink.exe"))
                .spawn()?;
        } else {
            control::send(control::SHOW)?;
        }
    } else if state != "stopped" {
        control::send(control::HIDE)?;
    }
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        let state = window_state()?;
        if (show && state == "open") || (!show && matches!(state, "stopped" | "tray")) {
            return Ok(());
        }
        if show && state == "minimized" {
            control::send(control::SHOW)?;
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "The window did not reach the requested state",
            ));
        }
        thread::sleep(Duration::from_millis(40));
    }
}

fn execute(args: &[&str]) -> io::Result<Value> {
    if args == ["vps", "create"] {
        use std::io::Read;
        let config = serde_json::from_reader(io::stdin().lock().take(32768))?;
        let invitation = uplink::vps::create(config, &settings::path()?, false, |_| {})?;
        return Ok(json!({"ok":true,"invitation":invitation}));
    }
    if let ["vps", "check", target @ ..] = args {
        let checks = uplink::vps::Target::parse(target)?.check()?;
        return Ok(json!({"ok":true,"source":"ssh","data":checks}));
    }
    validate(args)?;
    let path = settings::path()?;
    match args {
        ["update", action] => {
            if control::tray_window().is_null() {
                return Err(io::Error::other("Uplink is not running"));
            }
            if *action == "status" {
                return Ok(serde_json::to_value(uplink::updates::Status::load(&path)?)?);
            }
            let cmd = match *action {
                "check" => control::UPDATE_CHECK,
                "download" => control::UPDATE_DOWNLOAD,
                "install" => control::UPDATE_INSTALL,
                _ => unreachable!(),
            };
            if control::send(cmd)? == 0 {
                return Err(io::Error::other("Update request failed"));
            }
            return Ok(json!({"requested":action}));
        }
        ["language", code] => {
            settings::save_language(&path, code)?;
            changed::notify()?;
        }
        ["power", value] => {
            settings::set_option(&path, "power", *value == "on")?;
            changed::notify()?;
        }
        ["setting", key, value] => {
            settings::set_option(&path, key, *value == "on")?;
            if *key == "autoupdate" && *value == "on" {
                let _ = control::send(control::UPDATE_AUTO);
            }
            changed::notify()?;
        }
        ["demo-health", health] => {
            settings::set_demo_health(&path, health)?;
            changed::notify()?;
        }
        ["nick", name] => {
            settings::save_nickname(&path, name)?;
            changed::notify()?;
        }
        ["rename-room", name] => {
            settings::rename_room(&path, name)?;
            changed::notify()?;
        }
        ["join", url] | ["join", "--headless", url] => {
            if !invite::accepts_invite(url) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Unknown invitation",
                ));
            }
            let headless = args.len() == 3;
            if headless {
                settings::join_headless(&path, url)?;
            } else {
                settings::join(&path, url)?;
            }
            changed::notify()?;
            if !headless {
                change_window(true)?;
            }
        }
        ["select", url] => {
            settings::select(&path, url)?;
            changed::notify()?;
        }
        ["leave"] => {
            settings::save(&path, false)?;
            changed::notify()?;
        }
        ["tray", mode] => {
            settings::save_tray_mode(&path, mode)?;
            if !control::tray_window().is_null() && control::send(control::ICON)? == 0 {
                return Err(io::Error::other("Could not update the tray icon"));
            }
        }
        ["show"] => change_window(true)?,
        ["hide"] => change_window(false)?,
        _ => {}
    }
    let nick = settings::nickname(&path, &peers::windows_name())?;
    let rooms = settings::rooms(&path)?;
    let active = rooms.first();
    let options = settings::options(&path)?;
    let connected_room = |r: &settings::Room| {
        options.enabled && options.health != "critical" && !r.own_ip.is_empty()
    };
    let connected = active.is_some_and(connected_room);
    let result = match args {
        ["rooms"] => {
            json!({"rooms":rooms.iter().enumerate().map(|(i,r)| json!({"name":r.name,"url":r.url,"selected":i==0,"source":if r.live.is_some(){"zerotier"}else{"local_fixture"},"connected":connected_room(r),"ip":connected_room(r).then_some(&r.own_ip)})).collect::<Vec<_>>()})
        }
        ["members"] => {
            let creator = active.map(|r| r.creator_ip.as_str()).unwrap_or_default();
            let members = if let Some(r) = active.filter(|_| connected) {
                r.participants(&nick)
                    .into_iter()
                    .filter(|m| !r.policy.removed.iter().any(|ip| ip == m.ip))
                    .collect()
            } else {
                vec![]
            };
            json!({"members":members.into_iter().map(|m|json!({"nick":m.nick,"ip":m.ip,"online":m.online,"self":m.own,"admin":active.is_some_and(|r|r.policy.admins.iter().any(|ip|ip==m.ip)),"creator":m.ip == creator,"ping_ms":m.ping_ms})).collect::<Vec<_>>()})
        }
        ["invite"] if let Some(live) = active.and_then(|r| r.live.as_ref()) => {
            json!({"url":live.share()?})
        }
        ["invite"] if let Some(r) = active => {
            json!({"url":(!r.policy.link_disabled).then_some(&r.url),"qr":r.policy.qr.then(||format!("{}?via=qr",r.url)),"credentials_enabled":r.policy.password.is_some()})
        }
        ["invite"] => {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "You are not a member of a network",
            ));
        }
        _ => {
            json!({"window":window_state()?,"nick":nick,"room":active.map(|r| &r.name),
                    "language":settings::language(&path)?, "power":options.enabled, "health":if options.enabled { options.health.as_str() } else { "off" },
                    "autoconnect":options.autoconnect,"autostart":uplink::startup::enabled()?,"autoupdate":options.autoupdate,
                    "ip":active.filter(|_|connected).map(|r|&r.own_ip),"server_ping_ms":active.filter(|_|connected).map(|r|r.server_ms),"tray_icon":settings::tray_mode(&path)?})
        }
    };
    Ok(
        json!({"ok":true,"source":if active.is_some_and(|r|r.live.is_some()) {"zerotier"} else {"local_fixture"},"data":result}),
    )
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    if matches!(refs.as_slice(), [] | ["--help" | "-h" | "help"]) {
        println!("{USAGE}");
        return;
    }
    match execute(&refs) {
        Ok(value) => println!("{value}"),
        Err(e) => {
            let (code, exit) = match e.kind() {
                io::ErrorKind::InvalidInput => ("invalid_argument", 2),
                io::ErrorKind::NotConnected => ("not_in_room", 1),
                io::ErrorKind::TimedOut => ("timeout", 1),
                io::ErrorKind::PermissionDenied => ("permission_denied", 1),
                _ => ("operation_failed", 1),
            };
            eprintln!(
                "{}",
                json!({"ok":false,"code":code,"message":uplink::i18n::message(&uplink::i18n::current(), &e.to_string())})
            );
            std::process::exit(exit);
        }
    }
}
