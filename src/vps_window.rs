use crate::{RoomWindow, VpsUi, apply_room_change, avatar};
use slint::{ComponentHandle, SharedString};
use std::path::Path;
use uplink::{i18n, settings, vps};

pub fn bind(ui: &RoomWindow, path: &Path) {
    let api = ui.global::<VpsUi>();
    let prefs = path.to_owned();
    api.on_known_fingerprint(move |ip, port| {
        settings::vps_fingerprint(&prefs, &format!("{ip}:{port}"))
            .unwrap_or_default()
            .into()
    });
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
        move |name, ip, port, user, password, identity, fingerprint| {
            let weak = weak.clone();
            let path = path.clone();
            let config = vps::Creation {
                name: name.into(),
                ip: ip.into(),
                port: port.parse().unwrap_or(0),
                user: user.into(),
                password: password.into(),
                identity: identity.into(),
                fingerprint: fingerprint.trim().into(),
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
                        Ok(url) => {
                            if let Ok(rooms) = rooms {
                                apply_room_change(&ui, rooms);
                            }
                            api.set_invitation(url.into());
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
