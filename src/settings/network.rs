use super::*;
use serde::{Deserialize, Serialize};

#[cfg(test)]
#[test]
fn saved_password_reopens_only_for_the_current_network_revision_and_manager() {
    let dir = tempfile::tempdir().unwrap();
    let profile = dir.path().join("prefs.json");
    let url = "uplink://sample.example/networks/1234";
    let secret = "тестовый пароль";
    let live = serde_json::json!({
        "host":"sample.example", "id":"1234", "node":"alice", "name":"Example", "session":"fixture",
        "rows":[{"node":"alice","nick":"Alice","ip":"10.0.0.2","present":true,"role":"owner"}],
        "methods":{"link":true,"qr":false,"password":true,"password_revision":"first"}
    });
    update(&profile, |prefs| {
        prefs["rooms"] = serde_json::json!([url]);
        prefs["live"][url] = live;
        Ok(())
    })
    .unwrap();
    let sealed = super::password::seal(url, secret).unwrap();
    store_network_password(&profile, url, Some(sealed), Some("first")).unwrap();
    assert!(!fs::read_to_string(&profile).unwrap().contains(secret));
    assert_eq!(network_password(&profile, url).unwrap(), secret);
    update(&profile, |prefs| {
        prefs["live"][url]["methods"]["password_revision"] = "second".into();
        Ok(())
    })
    .unwrap();
    assert_eq!(network_password(&profile, url).unwrap(), "");
    update(&profile, |prefs| {
        prefs["live"][url]["methods"]["password_revision"] = "first".into();
        prefs["live"][url]["rows"][0]["role"] = "member".into();
        Ok(())
    })
    .unwrap();
    assert_eq!(network_password(&profile, url).unwrap(), "");
    store_network_password(&profile, url, None, None).unwrap();
    assert!(
        read(&profile).unwrap()["saved_passwords"]
            .get(url)
            .is_none()
    );
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Policy {
    pub owner: Option<String>,
    pub name: Option<String>,
    pub admins: Vec<String>,
    pub removed: Vec<String>,
    pub link_disabled: bool,
    pub qr: bool,
    pub password: Option<String>,
    pub avatar: String,
}

impl Policy {
    pub(super) fn load(prefs: &Value, url: &str) -> Self {
        let saved = &prefs["network_settings"][url];
        if !saved.is_null() {
            if let Ok(policy) = serde_json::from_value(saved.clone()) {
                return policy;
            }
        }
        Self {
            admins: if url == crate::invite::INVITATION {
                vec!["192.0.2.12".into()]
            } else {
                vec![]
            },
            ..Self::default()
        }
    }
    fn store(&self, prefs: &mut Value, url: &str) -> io::Result<()> {
        prefs["network_settings"][url] = serde_json::to_value(self)?;
        Ok(())
    }
}

impl Room {
    pub fn participants<'a>(&'a self, nick: &'a str) -> Vec<crate::peers::Peer<'a>> {
        if let Some(live) = &self.live {
            live.rows
                .iter()
                .map(|row| crate::peers::Peer {
                    nick: if row.node == live.node {
                        nick
                    } else {
                        &row.nick
                    },
                    ip: &row.ip,
                    own: row.node == live.node,
                    online: row.present,
                    ping_ms: row.ping_ms,
                })
                .collect()
        } else {
            crate::peers::room_members(&self.url, nick)
        }
    }
    pub fn manages(&self) -> bool {
        if let Some(live) = &self.live {
            return live
                .rows
                .iter()
                .any(|p| p.node == live.node && matches!(p.role.as_str(), "admin" | "owner"));
        }
        (!self.creator_ip.is_empty() && self.creator_ip == self.own_ip)
            || self.policy.admins.iter().any(|ip| ip == &self.own_ip)
    }
}

fn modify(
    path: &Path,
    url: &str,
    apply: impl FnOnce(&mut Room) -> io::Result<()>,
) -> io::Result<()> {
    update(path, |prefs| {
        let urls = saved_urls(prefs)?;
        let active = urls
            .first()
            .filter(|&&item| item == url)
            .ok_or_else(|| io::Error::other("The selected network has changed"))?;
        let mut room = room_info(prefs, active);
        if !room.manages() {
            return Err(io::Error::other("Network management permission required"));
        }
        apply(&mut room)?;
        room.policy.store(prefs, url)
    })
}

pub fn network_password(path: &Path, url: &str) -> io::Result<String> {
    let prefs = read(path)?;
    let entry = &prefs["saved_passwords"][url];
    let Some(sealed) = entry["sealed"].as_str() else {
        return Ok(String::new());
    };
    if !saved_urls(&prefs)?.contains(&url) {
        return Ok(String::new());
    }
    let room = room_info(&prefs, url);
    if !room.manages() {
        return Ok(String::new());
    }
    let Some(methods) = room.live.and_then(|live| live.methods) else {
        return Ok(String::new());
    };
    if !methods.password || methods.password_revision.as_deref() != entry["revision"].as_str() {
        return Ok(String::new());
    }
    super::password::open(url, sealed)
}

fn store_network_password(
    path: &Path,
    url: &str,
    sealed: Option<String>,
    revision: Option<&str>,
) -> io::Result<()> {
    update(path, |prefs| {
        let enabled = sealed.is_some();
        if let Some(sealed) = sealed {
            prefs["saved_passwords"][url] =
                serde_json::json!({"sealed":sealed,"revision":revision});
        } else if let Some(entries) = prefs["saved_passwords"].as_object_mut() {
            entries.remove(url);
        }
        if let Some(methods) = prefs["live"][url]["methods"].as_object_mut() {
            methods.insert("password".into(), enabled.into());
            methods.insert("password_revision".into(), serde_json::json!(revision));
        }
        Ok(())
    })
}

pub fn network_action(path: &Path, url: &str, action: &str, value: &str) -> io::Result<()> {
    let password_change = matches!(action, "password" | "password-mode");
    let guard = if password_change {
        Some(transport_lock(path)?)
    } else {
        None
    };
    if let Some(room) = rooms(path)?
        .into_iter()
        .find(|r| r.url == url && r.live.is_some())
    {
        let live = room.live.unwrap();
        let value = match action {
            "role" | "transfer" | "remove" => live
                .rows
                .iter()
                .find(|r| r.ip == value)
                .map(|r| r.node.as_str())
                .ok_or_else(|| io::Error::other("Participant is unavailable"))?,
            "name" => validated_name(value, 64)?,
            "avatar" if live.avatar_revision.is_some() => value,
            "password" if !value.is_empty() && value.len() <= 512 => value,
            "password-mode" if live.methods.as_ref().is_some_and(|m| m.password) => "off",
            "link" => {
                if room.policy.link_disabled {
                    "on"
                } else {
                    "off"
                }
            }
            "qr" => {
                if room.policy.qr {
                    "off"
                } else {
                    "on"
                }
            }
            _ => {
                return Err(io::Error::other(
                    "This setting is unavailable on this instance",
                ));
            }
        };
        let sealed = if action == "password" {
            Some(super::password::seal(url, value)?)
        } else {
            None
        };
        let reply = live.manage(action, value)?;
        if password_change {
            store_network_password(path, url, sealed, reply["password_revision"].as_str())?;
        }
        drop(guard);
        refresh_transport(path)?;
        return Ok(());
    }
    modify(path, url, |room| {
        let owner = room.creator_ip == room.own_ip;
        let p = &mut room.policy;
        match action {
            "link" => p.link_disabled = !p.link_disabled,
            "qr" => p.qr = !p.qr,
            "password-mode" => {
                p.password = if p.password.is_some() {
                    None
                } else {
                    Some(uuid::Uuid::new_v4().simple().to_string()[..14].to_owned())
                }
            }
            "password" if p.password.is_some() && !value.is_empty() => {
                p.password = Some(value.into())
            }
            "name" => p.name = Some(validated_name(value, 64)?.into()),
            "avatar" => p.avatar = value.into(),
            "role" | "transfer" | "remove" => {
                if value == room.own_ip
                    || value == room.creator_ip
                    || p.removed.iter().any(|ip| ip == value)
                    || !crate::peers::room_members(url, "")
                        .iter()
                        .any(|m| m.ip == value)
                {
                    return Err(io::Error::other("Participant is unavailable"));
                }
                let admin = p.admins.iter().any(|ip| ip == value);
                if !owner && (action != "remove" || admin) {
                    return Err(io::Error::other("Network management permission required"));
                }
                p.admins.retain(|ip| ip != value);
                match action {
                    "role" if !admin => p.admins.push(value.into()),
                    "transfer" => {
                        p.owner = Some(value.into());
                        p.admins.retain(|ip| ip != &room.own_ip);
                    }
                    "remove" => p.removed.push(value.into()),
                    _ => {}
                }
            }
            _ => return Err(io::Error::other("Invalid network setting")),
        }
        Ok(())
    })
}

pub fn join_credentials(path: &Path, login: &str, password: &str) -> io::Result<()> {
    update(path, |prefs| {
        let url = crate::invite::NETWORKS
            .into_iter()
            .find(|&url| {
                let r = room_info(prefs, url);
                r.server == login.trim()
                    && !password.is_empty()
                    && r.policy.password.as_deref() == Some(password)
            })
            .ok_or_else(|| io::Error::other("Invalid network credentials"))?;
        select_url(prefs, url, true)
    })
}

pub fn join_on_server(path: &Path, server: &str, name: &str, password: &str) -> io::Result<()> {
    join_on_server_with_progress(path, server, name, password, &|_| {})
}

#[test]
fn failed_password_join_records_the_operation_and_windows_error_without_credentials() {
    let dir = tempfile::tempdir().unwrap();
    let profile = dir.path().join("prefs.json");
    std::fs::create_dir(&profile).unwrap();
    let error = join_on_server(
        &profile,
        "private-host.example",
        "private-name",
        "private-password",
    )
    .unwrap_err();
    let log = std::fs::read_to_string(dir.path().join("last-join.log")).unwrap();
    assert!(log.contains("method=password"));
    assert!(log.contains("stage=access"));
    assert!(log.contains("result=failed operation=read_settings"));
    assert!(log.contains(&format!("win32={}", error.raw_os_error().unwrap())));
    for secret in ["private-host", "private-name", "private-password"] {
        assert!(!log.contains(secret));
    }
}

pub fn join_on_server_with_progress(
    path: &Path,
    server: &str,
    name: &str,
    password: &str,
    progress: &dyn Fn(super::JoinStage),
) -> io::Result<()> {
    super::join_log::run(path, "password", progress, |notify| {
        join_with_password(path, server, name, password, notify)
    })
}

fn join_with_password(
    path: &Path,
    server: &str,
    name: &str,
    password: &str,
    progress: &dyn Fn(super::JoinStage),
) -> io::Result<()> {
    progress(super::JoinStage::Access);
    super::join_log::operation("read_settings");
    if !options(path)?.enabled {
        return Err(io::Error::other("Uplink is disabled"));
    }
    let host = server.trim().to_ascii_lowercase();
    let prefs = read(path)?;
    for url in crate::invite::NETWORKS {
        let room = room_info(&prefs, url);
        if room.server == host {
            if room.name.trim().to_lowercase() != name.trim().to_lowercase() {
                return Err(io::Error::other("Invalid network credentials"));
            }
            super::join_log::operation("check_credentials");
            return join_credentials(path, &host, password);
        }
    }
    super::join_log::operation("password_login");
    let link = crate::transport::password_invite(&host, name.trim(), password)?;
    super::join_link(path, &link, progress)
}
