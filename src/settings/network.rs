use super::*;
use serde::{Deserialize, Serialize};

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

pub fn network_action(path: &Path, url: &str, action: &str, value: &str) -> io::Result<()> {
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
        live.manage(action, value)?;
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
            return join_credentials(path, &host, password);
        }
    }
    let link = crate::transport::password_invite(&host, name.trim(), password)?;
    join(path, &link)
}
