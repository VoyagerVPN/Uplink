use crate::{RoomWindow, VpsUi, apply_room_change, avatar};
use slint::{ComponentHandle, SharedString};
use std::path::Path;
use uplink::{i18n, settings, vps};

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
