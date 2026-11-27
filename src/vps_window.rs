use crate::{NoticesUi, RoomWindow, ServerUpdateNotice, VpsUi, apply_room_change, avatar};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::{collections::BTreeMap, path::Path};
use uplink::{i18n, settings, vps};

pub fn refresh_notices(ui: &RoomWindow, rooms: &[settings::Room]) {
    let mut servers = BTreeMap::<String, (&settings::Room, u16)>::new();
    for room in rooms {
        if let Some(live) = &room.live {
            if !room.creator_ip.is_empty() && room.creator_ip == room.own_ip && !live.availability.membership_gone() {
                let revision = live.server_revision.unwrap_or(0);
                servers.entry(live.host.to_ascii_lowercase())
                    .and_modify(|(_, known)| *known = (*known).max(revision))
                    .or_insert((room, revision));
            }
        }
    }
    let notices = ui.global::<NoticesUi>();
    let previous = notices.get_server_updates();
    let bundled = vps::server_revision();
    let updates: Vec<_> = servers.into_iter().filter(|(_, (_, revision))| *revision < bundled)
        .map(|(server, (room, revision))| ServerUpdateNotice {
            server: server.into(), network: room.url.as_str().into(), name: room.name.as_str().into(),
            revision: i32::from(revision),
        }).collect();
    if updates.is_empty() {
        notices.set_server_unread(false);
    } else if updates.iter().any(|update| !previous.iter().any(|old| old.server == update.server)) {
        notices.set_server_unread(true);
    }
    notices.set_server_updates(ModelRc::new(VecModel::from(updates)));
}

pub fn bind(ui: &RoomWindow, path: &Path) {
    let api = ui.global::<VpsUi>();
    let weak = ui.as_weak();
    api.on_check(move |field, value| {
        let Some(ui) = weak.upgrade() else {
            return SharedString::default();
        };
        let message = vps::field_error(field.as_str(), value.as_str());
        i18n::message(ui.get_language_code().as_str(), message).into()
    });
    api.on_choose_key(move |old| {
        avatar::pick_file("SSH\0*.*\0\0")
            .map(|p| p.to_string_lossy().as_ref().into())
            .unwrap_or(old)
    });
    let saved_path = path.to_owned();
    api.on_has_upgrade_access(move |server| vps::has_saved_access(&saved_path, &server));
    let weak = ui.as_weak();
    let upgrade_path = path.to_owned();
    let run_upgrade = move |server: String, access: Option<vps::Creation>| {
        let progress = weak.clone();
        let complete = weak.clone();
        let path = upgrade_path.clone();
        std::thread::spawn(move || {
            let on_step = |step| {
                let _ = progress.upgrade_in_event_loop(move |ui| ui.global::<VpsUi>().set_step(step));
            };
            let result = match access {
                Some(access) => vps::upgrade(vps::Upgrade { server, access }, &path, on_step),
                None => vps::upgrade_saved(server, &path, on_step),
            };
            let _ = complete.upgrade_in_event_loop(move |ui| {
                let api = ui.global::<VpsUi>();
                match result {
                    Ok(report) => {
                        api.set_upgrade_log(report.log_text.into());
                        let lang = ui.get_language_code();
                        api.set_failure(if report.detail.is_empty() { "".into() } else { i18n::error_message(&lang, &report.detail).into() });
                        api.set_upgrade_result(report.result.into());
                    }
                    Err(error) => {
                        api.set_failure(i18n::error_message(&ui.get_language_code(), &error.to_string()).into());
                        api.set_upgrade_result("failed".into());
                    }
                }
            });
        });
    };
    let run_saved = run_upgrade.clone();
    api.on_upgrade(move |server| run_saved(server.into(), None));
    api.on_upgrade_with_access(move |server, ip, port, user, password, identity| {
        run_upgrade(server.into(), Some(vps::Creation {
            ip: ip.into(),
            port: port.trim().parse().unwrap_or(0),
            user: user.into(),
            password: password.into(),
            identity: identity.into(),
            trust_on_first_use: true,
            ..Default::default()
        }));
    });
    let weak = ui.as_weak();
    let path = path.to_owned();
    api.on_install(
        move |name, ip, port, user, password, identity, host, bind, udp_port| {
            let weak = weak.clone();
            let path = path.clone();
            let config = vps::Creation {
                name: name.into(),
                ip: ip.into(),
                port: port.trim().parse().unwrap_or(0),
                user: user.into(),
                password: password.into(),
                identity: identity.into(),
                trust_on_first_use: true,
                public_host: host.into(),
                proxy_bind: bind.into(),
                udp_port: udp_port.trim().parse().ok(),
                ..Default::default()
            };
            std::thread::spawn(move || {
                let result = vps::create(config, &path, true, |step| {
                    let _ =
                        weak.upgrade_in_event_loop(move |ui| ui.global::<VpsUi>().set_step(step));
                });
                let rooms = settings::rooms(&path);
                let _ = weak.upgrade_in_event_loop(move |ui| {
                    let api = ui.global::<VpsUi>();
                    match result {
                        Ok(network) => {
                            if let Ok(rooms) = rooms {
                                apply_room_change(&ui, rooms);
                            }
                            api.set_network_key(network.key.into());
                            api.set_invitation(network.invitation.into());
                        }
                        Err(error) => {
                            let source = error.to_string();
                            let lang = ui.get_language_code();
                            let msg = i18n::error_message(&lang, &source);
                            api.set_failure(msg.into());
                        }
                    }
                });
            });
        },
    );
}
