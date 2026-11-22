use serde_json::Value;
mod network;
pub use network::{Policy, join_credentials, join_on_server, network_action};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub fn path() -> io::Result<PathBuf> {
    let root = std::env::var_os("UPLINK_PREFS_DIR")
        .or_else(|| std::env::var_os("RUNE_PREFS_DIR"))
        .or_else(|| std::env::var_os("APPDATA"))
        .ok_or_else(|| io::Error::other("Settings directory unavailable"))?;
    Ok(PathBuf::from(root).join("Uplink").join("prefs.json"))
}

fn read(path: &Path) -> io::Result<Value> {
    match fs::read(path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(serde_json::json!({})),
        Err(error) => Err(error),
    }
}

pub struct Room {
    pub policy: Policy,
    pub url: String,
    pub name: String,
    pub own_ip: String,
    pub creator_ip: String,
    pub server_ms: Option<u32>,
    pub live: Option<crate::transport::Membership>,
    pub server: String,
}

fn saved_urls(prefs: &Value) -> io::Result<Vec<&str>> {
    use crate::invite;
    let Some(saved) = prefs.get("rooms") else {
        return Ok(if prefs["room"] == "demo" {
            vec![invite::INVITATION]
        } else {
            vec![]
        });
    };
    saved
        .as_array()
        .ok_or_else(|| io::Error::other("Invalid network list"))?
        .iter()
        .map(|url| {
            url.as_str()
                .filter(|url| invite::canonical(url).is_some() || prefs["live"][*url].is_object())
                .ok_or_else(|| io::Error::other("Unknown network in profile"))
        })
        .collect()
}

fn room_info(prefs: &Value, url: &str) -> Room {
    use crate::invite::{ARENA, COOP, NIGHT, PINE};
    if let Ok(live) =
        serde_json::from_value::<crate::transport::Membership>(prefs["live"][url].clone())
    {
        return Room {
            url: url.into(),
            name: live.name.clone(),
            server: live.host.clone(),
            own_ip: live.own_ip.clone(),
            creator_ip: live
                .rows
                .iter()
                .find(|r| r.role == "owner")
                .map(|r| r.ip.clone())
                .unwrap_or_default(),
            server_ms: live.server_ms,
            policy: Policy {
                admins: live
                    .rows
                    .iter()
                    .filter(|r| r.role == "admin")
                    .map(|r| r.ip.clone())
                    .collect(),
                link_disabled: live.methods.as_ref().is_some_and(|m| !m.link),
                qr: live.methods.as_ref().is_some_and(|m| m.qr),
                password: live
                    .methods
                    .as_ref()
                    .is_some_and(|m| m.password)
                    .then(String::new),
                ..Policy::default()
            },
            live: Some(live),
        };
    }
    let guest = match url {
        COOP => Some(("Кооп", "coop.example", "198.51.100.11", "198.51.100.20", 54)),
        ARENA => Some(("Арена", "arena.example", "10.42.0.11", "10.42.0.20", 42)),
        PINE => Some(("Еловый бор", "pine.example", "10.64.0.11", "10.64.0.20", 31)),
        NIGHT => Some((
            "Ночная смена",
            "night.example",
            "10.85.0.11",
            "10.85.0.20",
            67,
        )),
        _ => None,
    };
    let policy = Policy::load(prefs, url);
    if let Some((name, server, own_ip, creator_ip, server_ms)) = guest {
        Room {
            url: url.into(),
            live: None,
            name: policy.name.clone().unwrap_or_else(|| name.into()),
            own_ip: own_ip.into(),
            creator_ip: policy.owner.clone().unwrap_or_else(|| creator_ip.into()),
            policy,
            server_ms: Some(server_ms),
            server: server.into(),
        }
    } else {
        Room {
            url: url.into(),
            live: None,
            name: policy.name.clone().unwrap_or_else(|| {
                prefs["demo_name"]
                    .as_str()
                    .unwrap_or(crate::peers::ROOM_NAME)
                    .into()
            }),
            own_ip: crate::peers::SELF_IP.into(),
            creator_ip: policy
                .owner
                .clone()
                .unwrap_or_else(|| fixture_creator(prefs).into()),
            policy,
            server_ms: Some(38),
            server: "after-hours.example".into(),
        }
    }
}

pub fn rooms(path: &Path) -> io::Result<Vec<Room>> {
    let prefs = read(path)?;
    saved_urls(&prefs)?
        .into_iter()
        .map(|url| {
            if crate::invite::canonical(url).is_none() {
                let membership: crate::transport::Membership =
                    serde_json::from_value(prefs["live"][url].clone())?;
                if membership.key() != url {
                    return Err(io::Error::other("Invalid saved network"));
                }
            }
            Ok(room_info(&prefs, url))
        })
        .collect()
}

pub fn load(path: &Path) -> io::Result<bool> {
    Ok(!saved_urls(&read(path)?)?.is_empty())
}

pub fn refresh_transport(path: &Path) -> io::Result<bool> {
    if !read(path)?["pending_power"].is_boolean() && !rooms(path)?.iter().any(|r| r.live.is_some())
    {
        return Ok(false);
    }
    let _guard = transport_lock(path)?;
    let mut changed = false;
    if let Some(desired) = read(path)?["pending_power"].as_bool() {
        apply_power(path, desired)?;
        changed = true;
    }
    let enabled = options(path)?.enabled;
    let nick = nickname(path, &crate::peers::windows_name())?;
    for mut live in rooms(path)?.into_iter().filter_map(|room| room.live) {
        let key = live.key();
        let before = serde_json::to_value(&live)?;
        let mut adapter_available = false;
        let result = if enabled {
            (|| {
                if crate::transport::interface(live.interface_id())?.is_none() {
                    crate::transport::attach(live.interface_id())?;
                }
                adapter_available = true;
                live.refresh(&nick)
            })()
        } else if live.transport.is_some() {
            Ok(())
        } else {
            crate::transport::detach(&live.id)
        };
        live.health = if !enabled {
            "ok"
        } else if !adapter_available {
            "critical"
        } else if result.is_err() || live.own_ip.is_empty() || live.server_ms.is_none() {
            "warning"
        } else {
            "ok"
        }
        .into();
        if !enabled || !adapter_available {
            live.own_ip.clear();
            live.server_ms = None;
            for row in &mut live.rows {
                row.ping_ms = None;
                row.present = false;
            }
        }
        let next = serde_json::to_value(live)?;
        if next != before {
            update(path, |prefs| {
                if prefs["live"][&key]["session"] == before["session"]
                    && saved_urls(prefs)?.contains(&key.as_str())
                    && prefs["power"].as_bool().unwrap_or(true) == enabled
                {
                    prefs["live"][&key] = next;
                    changed = true;
                }
                Ok(())
            })?;
        }
    }
    Ok(changed)
}

fn transport_lock(path: &Path) -> io::Result<fs::File> {
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::other("No settings directory"))?;
    fs::create_dir_all(dir)?;
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join("transport.lock"))?;
    file.lock()?;
    Ok(file)
}

fn store_urls(prefs: &mut Value, urls: Value) {
    prefs["rooms"] = urls;
    if let Some(obj) = prefs.as_object_mut() {
        obj.remove("room");
    }
}

fn open_room(path: &Path, url: &str, allow_join: bool) -> io::Result<()> {
    if !allow_join && read(path)?["live"][url].is_object() {
        return update(path, |prefs| select_url(prefs, url, false));
    }
    let qr = url.trim().ends_with("?via=qr");
    let url = crate::invite::canonical(url)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Unknown invitation"))?;
    update(path, |prefs| {
        let p = Policy::load(prefs, url);
        if allow_join && (if qr { !p.qr } else { p.link_disabled }) {
            return Err(io::Error::other("This invitation is disabled"));
        }
        select_url(prefs, url, allow_join)
    })
}
fn select_url(prefs: &mut Value, url: &str, allow_join: bool) -> io::Result<()> {
    let mut urls = saved_urls(prefs)?;
    if !allow_join && !urls.contains(&url) {
        return Err(io::Error::other("You have left this network"));
    }
    urls.retain(|&saved| saved != url);
    urls.insert(0, url);
    let saved = serde_json::json!(urls);
    store_urls(prefs, saved);
    Ok(())
}

pub fn join(path: &Path, url: &str) -> io::Result<()> {
    join_with_setup(path, url, true)
}

pub fn join_headless(path: &Path, url: &str) -> io::Result<()> {
    join_with_setup(path, url, false)
}

fn join_with_setup(path: &Path, url: &str, allow_setup_prompt: bool) -> io::Result<()> {
    if let Some((host, token)) = crate::invite::instance_link(url) {
        let _guard = transport_lock(path)?;
        if !options(path)?.enabled {
            return Err(io::Error::other("Uplink is disabled"));
        }
        let mut live = crate::transport::bootstrap(host, token, allow_setup_prompt)?;
        let key = live.key();
        let existing = crate::transport::interface(live.interface_id())?.is_some();
        let prefs = read(path)?;
        let returning = prefs["live"][&key].is_object() || prefs["pending"][&key].is_object();
        let known = live
            .transport
            .as_ref()
            .is_some_and(|id| prefs["shared_devices"][id] == host);
        let prior = rooms(path)?.into_iter().filter_map(|r| r.live).find(|r| {
            live.transport.is_some()
                && r.transport == live.transport
                && r.host == host
                && r.node == live.node
                && r.id != live.id
        });
        update(path, |prefs| {
            if let Some(prev) = prefs["live"][&key]
                .as_object()
                .or_else(|| prefs["pending"][&key].as_object())
            {
                let prev: crate::transport::Membership =
                    serde_json::from_value(Value::Object(prev.clone()))?;
                if prev.node != live.node {
                    return Err(io::Error::other("ZeroTier identity has changed"));
                }
                live.session = prev.session;
            } else if existing && !known && prior.is_none() {
                return Err(io::Error::other("Network already managed outside Uplink"));
            }
            prefs["pending"][&key] = serde_json::to_value(&live)?;
            Ok(())
        })?;
        let nick = nickname(path, &crate::peers::windows_name())?;
        if let Err(err) = live.enroll(
            token,
            &nick,
            returning,
            prior.as_ref().map(|r| r.session.as_str()),
        ) {
            if !existing && !known && prior.is_none() {
                crate::transport::detach(live.interface_id())?;
            }
            return Err(err);
        }
        return update(path, |prefs| {
            if let Some(id) = &live.transport {
                prefs["shared_devices"][id] = Value::String(host.to_owned());
            }
            prefs["live"][&key] = serde_json::to_value(live)?;
            prefs["pending"].as_object_mut().map(|p| p.remove(&key));
            select_url(prefs, &key, true)
        });
    }
    open_room(path, url, true)
}
pub fn select(path: &Path, url: &str) -> io::Result<()> {
    open_room(path, url, false)
}

pub fn save(path: &Path, joined: bool) -> io::Result<()> {
    if joined {
        return join(path, crate::invite::INVITATION);
    }
    leave(path, None)
}

pub fn leave(path: &Path, target: Option<&str>) -> io::Result<()> {
    let _guard = transport_lock(path)?;
    update(path, |prefs| {
        let mut urls: Vec<String> = saved_urls(prefs)?.into_iter().map(str::to_owned).collect();
        if let Some(target) = target.or_else(|| urls.first().map(String::as_str)) {
            if let Ok(live) = serde_json::from_value::<crate::transport::Membership>(
                prefs["live"][target].clone(),
            ) {
                if live.transport.is_some() {
                    live.leave_group()?;
                    prefs["live"].as_object_mut().map(|v| v.remove(target));
                } else {
                    crate::transport::detach(&live.id)?;
                }
            }
            let removed = target.to_owned();
            urls.retain(|url| url != &removed);
        }
        let saved = serde_json::json!(urls);
        store_urls(prefs, saved);
        Ok(())
    })
}

pub struct Options {
    pub enabled: bool,
    pub autoconnect: bool,
    pub autoupdate: bool,
    pub health: String,
}

#[cfg(test)]
#[test]
fn partial_network_failure_preserves_other_connections() {
    assert_eq!(connection_health(["critical", "ok"].into_iter()), "warning");
    assert_eq!(
        connection_health(["critical", "critical"].into_iter()),
        "critical"
    );
    assert_eq!(connection_health(["warning"].into_iter()), "warning");
    assert_eq!(connection_health(["ok"].into_iter()), "ok");
}

pub fn options(path: &Path) -> io::Result<Options> {
    let prefs = read(path)?;
    let urls = saved_urls(&prefs)?;
    let mut live = urls
        .into_iter()
        .filter(|key| prefs["live"][*key].is_object())
        .map(|key| prefs["live"][key]["health"].as_str().unwrap_or("ok"))
        .peekable();
    let health = if live.peek().is_some() {
        connection_health(live)
    } else {
        prefs["demo_health"].as_str().unwrap_or("ok")
    };
    Ok(Options {
        enabled: prefs["power"].as_bool().unwrap_or(true),
        autoconnect: prefs["autoconnect"].as_bool().unwrap_or(true),
        health: health.to_owned(),
        autoupdate: prefs["autoupdate"].as_bool().unwrap_or(false),
    })
}

fn connection_health<'a>(states: impl Iterator<Item = &'a str>) -> &'static str {
    let mut healthy = 0;
    let mut critical = 0;
    let mut warning = 0;
    for state in states {
        match state {
            "critical" => critical += 1,
            "warning" => warning += 1,
            _ => healthy += 1,
        }
    }
    if critical > 0 && warning + healthy == 0 {
        "critical"
    } else if warning + critical > 0 {
        "warning"
    } else {
        "ok"
    }
}

pub fn set_option(path: &Path, key: &str, value: bool) -> io::Result<()> {
    let _guard = if key == "power" {
        Some(transport_lock(path)?)
    } else {
        None
    };
    if key == "autostart" {
        return crate::startup::set(value);
    }
    if !matches!(key, "power" | "autoconnect" | "autoupdate") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Unknown setting",
        ));
    }
    if key == "power" {
        update(path, |prefs| {
            prefs["pending_power"] = Value::Bool(value);
            Ok(())
        })?;
        return apply_power(path, value);
    }
    update(path, |prefs| {
        prefs[key] = Value::Bool(value);
        Ok(())
    })
}

fn apply_power(path: &Path, value: bool) -> io::Result<()> {
    if let Some(devices) = read(path)?["shared_devices"].as_object() {
        for id in devices.keys() {
            crate::transport::shared_power(id, value)?;
        }
    }
    for room in rooms(path)? {
        if let Some(live) = room.live {
            if live.transport.is_some() {
                continue;
            }
            if value {
                crate::transport::attach(live.interface_id())?;
            } else {
                crate::transport::detach(&live.id)?;
            }
        }
    }
    update(path, |prefs| {
        prefs["power"] = Value::Bool(value);
        prefs.as_object_mut().unwrap().remove("pending_power");
        Ok(())
    })
}

pub fn set_demo_health(path: &Path, health: &str) -> io::Result<()> {
    if !matches!(health, "ok" | "warning" | "critical") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Supported states: ok, warning, critical",
        ));
    }
    write_field(path, "demo_health", health)
}

pub fn start_session(path: &Path) -> io::Result<()> {
    if !options(path)?.autoconnect {
        set_option(path, "power", false)?;
    }
    Ok(())
}

pub fn nickname(path: &Path, fallback: &str) -> io::Result<String> {
    Ok(read(path)?["nickname"]
        .as_str()
        .unwrap_or(fallback)
        .to_owned())
}

fn validated_name(name: &str, limit: usize) -> io::Result<&str> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > limit || name.chars().any(char::is_control) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            if limit == 32 {
                "Enter between 1 and 32 visible characters"
            } else {
                "Enter between 1 and 64 visible characters"
            },
        ));
    }
    Ok(name)
}

pub fn save_nickname(path: &Path, name: &str) -> io::Result<()> {
    write_field(path, "nickname", validated_name(name, 32)?)
}

pub fn room_name(path: &Path) -> io::Result<String> {
    let prefs = read(path)?;
    let url = saved_urls(&prefs)?
        .first()
        .copied()
        .unwrap_or(crate::invite::INVITATION);
    Ok(room_info(&prefs, url).name)
}

// В локальной модели адрес закреплён за участником и сохраняется при смене ника.
fn fixture_creator(prefs: &Value) -> &str {
    prefs["demo_creator_ip"]
        .as_str()
        .unwrap_or(crate::peers::SELF_IP)
}

pub fn rename_room(path: &Path, name: &str) -> io::Result<()> {
    rename_selected(path, None, name)
}

pub fn rename_selected(path: &Path, expected: Option<&str>, name: &str) -> io::Result<()> {
    if let Some(r) = rooms(path)?.first().filter(|r| r.live.is_some()) {
        if expected.is_some_and(|key| key != r.url) {
            return Err(io::Error::other("The selected network has changed"));
        }
        return network_action(path, &r.url, "name", name);
    }
    update(path, |prefs| {
        let urls = saved_urls(prefs)?;
        let Some(&url) = urls.first() else {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "You are not a member of a network",
            ));
        };
        if expected.is_some_and(|id| id != url) {
            return Err(io::Error::other("The selected network has changed"));
        }
        let room = room_info(prefs, url);
        if !room.manages() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Network management permission required",
            ));
        }
        let mut p = room.policy;
        p.name = Some(validated_name(name, 64)?.into());
        let key = url.to_owned();
        prefs["network_settings"][key] = serde_json::to_value(p)?;
        Ok(())
    })
}

pub fn tray_mode(path: &Path) -> io::Result<String> {
    Ok(read(path)?["tray_icon"].as_str().unwrap_or("green").into())
}

pub fn save_tray_mode(path: &Path, mode: &str) -> io::Result<()> {
    if !matches!(mode, "green" | "ping" | "system") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Icon mode: green, ping or system",
        ));
    }
    write_field(path, "tray_icon", mode)
}

pub fn language(path: &Path) -> io::Result<String> {
    let prefs = read(path)?;
    Ok(prefs["language"]
        .as_str()
        .filter(|v| crate::i18n::supports(v))
        .unwrap_or("ru")
        .to_owned())
}

pub fn save_language(path: &Path, code: &str) -> io::Result<()> {
    if !crate::i18n::supports(code) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Unsupported language",
        ));
    }
    write_field(path, "language", code)
}

fn write_field(path: &Path, key: &str, text: &str) -> io::Result<()> {
    update(path, |prefs| {
        prefs[key] = Value::from(text);
        Ok(())
    })
}

fn update(path: &Path, change: impl FnOnce(&mut Value) -> io::Result<()>) -> io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::other("No settings directory"))?;
    fs::create_dir_all(dir)?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join("prefs.lock"))?;
    lock.lock()?;
    let mut value = read(path)?;
    if !value.is_object() {
        return Err(io::Error::other("Invalid settings"));
    }
    change(&mut value)?;

    let mut file = tempfile::NamedTempFile::new_in(dir)?;
    serde_json::to_writer(&mut file, &value)?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_survives_reopen_and_unknown_choices_preserve_the_profile() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prefs.json");
        save_nickname(&path, "Mira").unwrap();
        assert_eq!(language(&path).unwrap(), "ru");
        save_language(&path, "en").unwrap();
        assert!(save_language(&path, "missing").is_err());
        assert_eq!(language(&path).unwrap(), "en");
        assert_eq!(nickname(&path, "").unwrap(), "Mira");
    }

    #[test]
    fn all_rooms_and_last_view_survive_power_cycle_and_reopen() {
        use crate::invite::{COOP, INVITATION};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prefs.json");
        fs::write(
            &path,
            r#"{"room":"demo","demo_name":"Вечер","nickname":"Я"}"#,
        )
        .unwrap();
        join(&path, COOP).unwrap();
        join(&path, COOP).unwrap();
        set_option(&path, "power", false).unwrap();
        start_session(&path).unwrap();
        set_option(&path, "power", true).unwrap();
        let saved = rooms(&path).unwrap();
        assert_eq!(
            saved.iter().map(|r| r.url.as_str()).collect::<Vec<_>>(),
            [COOP, INVITATION]
        );
        assert_eq!(saved[1].name, "Вечер");
        assert_ne!(saved[0].own_ip, saved[1].own_ip);
        save(&path, false).unwrap();
        assert_eq!(rooms(&path).unwrap()[0].url, INVITATION);
        assert_eq!(rooms(&path).unwrap().len(), 1);
    }

    #[test]
    fn each_rail_fixture_joins_with_its_own_server_and_roster() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prefs.json");
        for (slug, server, ip) in [
            ("arena", "arena.example", "10.42.0.11"),
            ("pine", "pine.example", "10.64.0.11"),
            ("night", "night.example", "10.85.0.11"),
        ] {
            let url = format!("uplink://join/{slug}");
            join(&path, &url).unwrap();
            let room = &rooms(&path).unwrap()[0];
            assert_eq!((room.server.as_str(), room.own_ip.as_str()), (server, ip));
            let roster = crate::peers::room_members(&room.url, "Я");
            assert!(roster.iter().any(|p| p.own && p.ip == ip));
            assert!(roster.iter().any(|p| p.ip == room.creator_ip));
        }
    }

    #[test]
    fn power_and_startup_policy_preserve_membership() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prefs.json");
        save(&path, true).unwrap();
        assert!(options(&path).unwrap().enabled);
        set_option(&path, "power", false).unwrap();
        start_session(&path).unwrap();
        assert!(!options(&path).unwrap().enabled);
        assert!(load(&path).unwrap());
        set_option(&path, "power", true).unwrap();
        set_option(&path, "autoconnect", false).unwrap();
        assert!(options(&path).unwrap().enabled);
        start_session(&path).unwrap();
        assert!(!options(&path).unwrap().enabled);
        assert!(load(&path).unwrap());
        set_option(&path, "autoupdate", true).unwrap();
        assert!(options(&path).unwrap().autoupdate);
    }

    #[test]
    fn only_joined_creator_can_rename_the_fixture_room() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prefs.json");
        assert_eq!(room_name(&path).unwrap(), crate::peers::ROOM_NAME);
        assert!(rename_room(&path, "Четверг").is_err());
        save(&path, true).unwrap();
        rename_room(&path, "  Четверг  ").unwrap();
        save(&path, false).unwrap();
        save(&path, true).unwrap();
        assert_eq!(room_name(&path).unwrap(), "Четверг");
        assert!(rename_room(&path, " ").is_err());
        write_field(&path, "demo_creator_ip", "192.0.2.10").unwrap();
        assert_eq!(
            rename_room(&path, "Пятница").unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(room_name(&path).unwrap(), "Четверг");
    }

    #[test]
    fn opens_existing_rune_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prefs.json");
        fs::write(&path, r#"{"room":"demo","other":42}"#).unwrap();
        assert!(load(&path).unwrap());
        save(&path, false).unwrap();
        assert!(!load(&path).unwrap());
        assert_eq!(read(&path).unwrap()["other"], 42);
        save(&path, true).unwrap();
        assert!(load(&path).unwrap());
    }
}
#[cfg(test)]
mod nickname_tests {
    use super::*;
    #[test]
    fn nickname_survives_reopen_and_leaving_the_room() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("prefs.json");
        assert_eq!(nickname(&file, "Windows User").unwrap(), "Windows User");
        save_nickname(&file, "  Мир  ").unwrap();
        assert!(save_nickname(&file, " \n ").is_err());
        save(&file, false).unwrap();
        assert_eq!(nickname(&file, "Windows User").unwrap(), "Мир");
    }
}
