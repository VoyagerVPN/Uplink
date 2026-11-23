#![windows_subsystem = "windows"]
mod avatar;
mod network_window;
mod vps_window;
mod placement;
mod setup_window;
mod tray_menu;
#[cfg(debug_assertions)]
mod update_preview;
mod update_window;
use network_window::bind_network;
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{io, path::Path, rc::Rc};
use uplink::{changed, invite, peers, settings};

slint::include_modules!();

fn ping_reading(ms: Option<u32>) -> PingReading {
    PingReading {
        bars: peers::signal_bars(ms),
        ms: ms.unwrap_or_default() as i32,
    }
}

fn compare_names(left: &str, right: &str) -> std::cmp::Ordering {
    use windows_sys::Win32::Globalization::{CSTR_EQUAL, CompareStringEx, NORM_IGNORECASE};
    let a: Vec<_> = left.encode_utf16().collect();
    let b: Vec<_> = right.encode_utf16().collect();
    let order = unsafe {
        CompareStringEx(
            windows_sys::core::w!("ru-RU"),
            NORM_IGNORECASE,
            a.as_ptr(),
            a.len() as i32,
            b.as_ptr(),
            b.len() as i32,
            std::ptr::null(),
            std::ptr::null(),
            0,
        )
    };
    if order == 0 {
        left.cmp(right)
    } else {
        order.cmp(&CSTR_EQUAL)
    }
}

fn members(
    mut participants: Vec<peers::Peer<'_>>,
    creator: &str,
    policy: &settings::Policy,
    by_ping: bool,
    descending: bool,
) -> ModelRc<Member> {
    participants.retain(|p| !policy.removed.iter().any(|ip| ip == p.ip));
    participants.sort_by(|a, b| {
        let direction =
            |order: std::cmp::Ordering| if descending { order.reverse() } else { order };
        if by_ping {
            match (a.ping_ms, b.ping_ms) {
                (Some(x), Some(y)) if x != y => return direction(x.cmp(&y)),
                (None, Some(_)) => return std::cmp::Ordering::Greater,
                (Some(_), None) => return std::cmp::Ordering::Less,
                _ => {}
            }
        }
        let order = compare_names(a.nick, b.nick).then_with(|| a.ip.cmp(b.ip));
        if by_ping { order } else { direction(order) }
    });
    let rows = participants
        .into_iter()
        .map(|p| Member {
            nick: p.nick.into(),
            ip: p.ip.into(),
            creator: p.ip == creator,
            admin: policy.admins.iter().any(|ip| ip == p.ip),
            own: p.own,
            ping: ping_reading(p.ping_ms),
        })
        .collect::<Vec<_>>();
    ModelRc::new(VecModel::from(rows))
}

fn refresh_members(ui: &RoomWindow, room: &settings::Room) {
    ui.set_members(members(
        room.participants(&ui.get_nickname()),
        &ui.get_creator_ip(),
        &room.policy,
        ui.get_by_ping(),
        ui.get_descending(),
    ));
}

fn apply_rooms(ui: &RoomWindow, rooms: Vec<settings::Room>) {
    let net = ui.global::<NetworkUi>();
    if rooms.first().map(|r| r.url.as_str()) != Some(ui.get_invite_url().as_str())
        || !rooms.first().is_some_and(|r| r.manages()) && !net.get_panel().is_empty()
    {
        net.invoke_close_modal();
        net.set_panel("".into());
        net.set_dragging_file(false);
    }
    ui.set_joined(!rooms.is_empty());
    if rooms.is_empty() {
        net.set_server("".into());
        net.set_password("".into());
        net.set_password_on(false);
    }
    net.set_live(rooms.first().is_some_and(|r| r.live.is_some()));
    if rooms
        .first()
        .is_none_or(|r| r.live.is_none() || r.url != ui.get_invite_url().as_str() || !r.manages())
    {
        ui.set_editing_room(false);
    }
    if let Some(room) = rooms.first() {
        ui.set_creator_ip(room.creator_ip.as_str().into());
        ui.set_is_creator(!room.creator_ip.is_empty() && room.creator_ip == room.own_ip);
        ui.set_room_name(room.name.as_str().into());
        ui.set_room_server(room.server.as_str().into());
        ui.set_invite_url(room.url.as_str().into());
        net.set_name(room.name.as_str().into());
        net.set_server(room.server.as_str().into());
        if room.live.is_none() {
            net.set_link(room.url.as_str().into());
        } else if net.get_modal() != "invite" {
            net.set_link("".into());
        }
        net.set_manages(room.manages());
        net.set_avatar_editable(
            room.live
                .as_ref()
                .is_none_or(|r| r.avatar_revision.is_some()),
        );
        if !net.get_avatar_editable() && net.get_modal() == "crop" {
            net.invoke_close_modal();
        }
        net.set_owner(!room.creator_ip.is_empty() && room.creator_ip == room.own_ip);
        net.set_link_on(!room.policy.link_disabled);
        net.set_qr_on(room.policy.qr);
        net.set_password_on(room.policy.password.is_some());
        if net.get_live() && net.get_panel() == "methods" {
            net.invoke_load_password();
        } else {
            net.set_password(room.policy.password.clone().unwrap_or_default().into());
        }
        if net.get_modal() == "invite" {
            let method = net.get_method();
            if !match method.as_str() {
                "link" => net.get_link_on(),
                "qr" => net.get_qr_on(),
                "password" => net.get_password_on(),
                _ => false,
            } {
                net.invoke_open_invite();
            }
        }
        if room.live.is_none() {
            net.set_qr_image(avatar::qr(&room.url));
        } else if net.get_modal() != "invite" {
            net.set_qr_image(Default::default());
        }
        refresh_members(ui, room);
        net.set_people(ModelRc::new(VecModel::from(
            room.participants(&ui.get_nickname())
                .into_iter()
                .filter(|p| !room.policy.removed.iter().any(|ip| ip == p.ip))
                .map(|p| Person {
                    name: p.nick.into(),
                    ip: p.ip.into(),
                    owner: p.ip == room.creator_ip,
                    admin: room.policy.admins.iter().any(|ip| ip == p.ip),
                    own: p.own,
                })
                .collect::<Vec<_>>(),
        )));
    } else {
        ui.set_invite_url("".into());
        ui.set_room_name("".into());
        ui.set_members(ModelRc::default());
    }
    let prefs = settings::path().ok();
    let mut rows: Vec<_> = rooms
        .into_iter()
        .map(|r| {
            let image = if let Some(live) = &r.live {
                prefs
                    .as_deref()
                    .and_then(|p| live.avatar_file(p))
                    .and_then(|path| slint::Image::load_from_path(&path).ok())
            } else {
                avatar::stored(&r.policy.avatar)
            };
            SavedRoom {
                has_avatar: image.is_some(),
                avatar: image.unwrap_or_default(),
                initials: if r.url == invite::PINE && r.name == "Еловый бор" {
                    "Ел".into()
                } else {
                    r.name
                        .split_whitespace()
                        .take(2)
                        .filter_map(|word| word.chars().next())
                        .flat_map(char::to_uppercase)
                        .collect::<String>()
                        .into()
                },
                name: r.name.into(),
                url: r.url.into(),
                server: r.server.into(),
            }
        })
        .collect();
    if let Some(first) = rows.first() {
        net.set_initials(first.initials.clone());
        net.set_avatar(first.avatar.clone());
        net.set_has_avatar(first.has_avatar);
    }
    rows.sort_by_key(|room| {
        invite::NETWORKS
            .iter()
            .position(|&url| url == room.url.as_str())
    });
    ui.set_rooms(ModelRc::new(VecModel::from(rows)));
}

fn open_pending_invite(ui: &RoomWindow, path: &Path) {
    let net = ui.global::<NetworkUi>();
    if ui.get_creating_network() || net.get_joining() || (ui.get_adding_network() && !net.get_join_failure().is_empty()) {
        return;
    }
    match settings::take_invite(path) {
        Ok(Some(link)) => {
            ui.set_settings_open(false);
            ui.global::<NetworkUi>().set_panel("".into());
            ui.global::<NetworkUi>().invoke_close_modal();
            ui.global::<NetworkUi>().set_credentials(false);
            ui.set_pending_link(link.as_str().into());
            ui.set_adding_network(true);
            ui.invoke_join_room(link.into());
        }
        Err(e) => show_error(ui, e),
        Ok(None) => {}
    }
}

fn start_join(
    ui: &RoomWindow,
    path: &Path,
    join: impl FnOnce(&Path, &dyn Fn(settings::JoinStage)) -> io::Result<()> + Send + 'static,
) {
    let net = ui.global::<NetworkUi>();
    if net.get_joining() {
        return;
    }
    ui.set_error("".into());
    net.set_error("".into());
    ui.set_adding_network(true);
    net.set_join_failure("".into());
    net.set_join_step(settings::JoinStage::Access as i32);
    net.set_joining(true);
    let weak = ui.as_weak();
    let path = path.to_owned();
    std::thread::spawn(move || {
        let progress = |step: settings::JoinStage| {
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.global::<NetworkUi>().set_join_step(step as i32)
            });
        };
        let result = join(&path, &progress).and_then(|()| settings::rooms(&path));
        let _ = weak.upgrade_in_event_loop(move |ui| {
            let net = ui.global::<NetworkUi>();
            if let Err(error) = &result {
                let message =
                    uplink::i18n::error_message(&ui.get_language_code(), &error.to_string())
                        .to_owned();
                net.set_join_failure(message.into());
            }
            net.set_joining(false);
            if let Ok(rooms) = result {
                apply_room_change(&ui, rooms);
                open_pending_invite(&ui, &path);
            }
        });
    });
}

fn finish_room_change(ui: &RoomWindow, prefs: &Path, result: io::Result<()>) {
    match result.and_then(|()| settings::rooms(prefs)) {
        Ok(rooms) => apply_room_change(ui, rooms),
        Err(e) => show_error(ui, e),
    }
}

fn apply_room_change(ui: &RoomWindow, rooms: Vec<settings::Room>) {
    apply_rooms(ui, rooms);
    ui.set_adding_network(false);
    ui.set_pending_link("".into());
    ui.set_error("".into());
    uplink::control::refresh_tray_icon();
}

fn apply_options(ui: &RoomWindow, options: settings::Options, autostart: bool) {
    ui.set_power_on(options.enabled);
    ui.set_autoconnect(options.autoconnect);
    ui.set_autoupdate(options.autoupdate);
    ui.set_autostart(autostart);
    ui.set_health(options.health.into());
}

fn apply_language(ui: &RoomWindow, code: &str) {
    if ui.get_language_code() == code {
        return;
    }
    if let Some(&(_, name)) = uplink::i18n::LANGUAGES.iter().find(|&&(id, _)| id == code) {
        slint::select_bundled_translation(code).expect("bundled Uplink language");
        ui.set_language_code(code.into());
        ui.set_language_name(name.into());
    }
}

fn show_error(ui: &RoomWindow, error: io::Error) {
    ui.set_error(uplink::i18n::error_message(&ui.get_language_code(), &error.to_string()).into());
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "--setup-ui") {
        placement::configure([388, 252])?;
        std::process::exit(setup_window::run(&args[1..])?);
    }
    if let [flag, x, y, light] = args.as_slice()
        && flag == "--tray-menu"
    {
        return tray_menu::run(x.parse()?, y.parse()?, light.parse()?);
    }
    placement::configure([388, 540])?;
    let window = RoomWindow::new()?;
    let update = window.global::<UpdateUi>();
    update.set_version(env!("CARGO_PKG_VERSION").into());
    window.set_languages(ModelRc::new(VecModel::from(
        uplink::i18n::LANGUAGES
            .iter()
            .map(|&(code, name)| LanguageChoice {
                code: code.into(),
                name: name.into(),
            })
            .collect::<Vec<_>>(),
    )));
    #[cfg(debug_assertions)]
    if let [flag, scene] = args.as_slice()
        && flag == "--preview-update"
    {
        return update_preview::run(&window, scene);
    }
    if !(args.is_empty() || args.len() == 1 && invite::accepts_invite(&args[0])) {
        return Err(io::Error::other("Unknown invitation").into());
    }
    let path = Rc::new(settings::path()?);
    vps_window::bind(&window, &path);
    update_window::bind(&window, &path)?;
    apply_language(&window, &settings::language(&path)?);
    let weak = window.as_weak();
    let prefs = path.clone();
    window.on_change_language(move |code| {
        let Some(ui) = weak.upgrade() else { return };
        match settings::save_language(&prefs, &code) {
            Ok(()) => {
                apply_language(&ui, &code);
                ui.set_error("".into());
            }
            Err(e) => show_error(&ui, e),
        }
    });
    bind_network(&window, path.clone());
    let event = changed::listen()?;
    let nickname = settings::nickname(&path, &peers::windows_name())?;
    window.set_nickname(nickname.as_str().into());
    if let Some(url) = args.first() {
        settings::queue_invite(&path, url)?;
    }
    apply_rooms(&window, settings::rooms(&path)?);
    let weak = window.as_weak();
    let prefs = path.clone();
    window.on_sort_members(move |by_ping| {
        if let Some(ui) = weak.upgrade() {
            ui.set_descending(ui.get_by_ping() == by_ping && !ui.get_descending());
            ui.set_by_ping(by_ping);
            if let Ok(rooms) = settings::rooms(&prefs) {
                if let Some(room) = rooms.first() {
                    refresh_members(&ui, room);
                }
            }
        }
    });
    apply_options(
        &window,
        settings::options(&path)?,
        uplink::startup::enabled()?,
    );
    let weak = window.as_weak();
    let prefs = path.clone();
    window.on_change_option(move |key, value| {
        let Some(ui) = weak.upgrade() else { return };
        let result = settings::set_option(&prefs, &key, value).and_then(|()| {
            apply_options(&ui, settings::options(&prefs)?, uplink::startup::enabled()?);
            if key == "autoupdate" && value {
                let _ = uplink::control::send(uplink::control::UPDATE_AUTO);
            }
            uplink::control::refresh_tray_icon();
            Ok(())
        });
        match result {
            Ok(()) => ui.set_error("".into()),
            Err(e) => {
                if let (Ok(options), Ok(autostart)) =
                    (settings::options(&prefs), uplink::startup::enabled())
                {
                    apply_options(&ui, options, autostart);
                }
                show_error(&ui, e);
            }
        }
    });
    let weak = window.as_weak();
    let prefs = path.clone();
    window.on_rename(move |name| {
        let Some(ui) = weak.upgrade() else { return };
        match settings::save_nickname(&prefs, name.as_str()) {
            Ok(()) => {
                ui.set_nickname(name.trim().into());
                if let Ok(rooms) = settings::rooms(&prefs) {
                    apply_rooms(&ui, rooms);
                }
                ui.set_editing(false);
                ui.set_error("".into());
            }
            Err(e) => show_error(&ui, e),
        }
    });
    let weak = window.as_weak();
    let prefs = path.clone();
    window.on_rename_room(move |name| {
        let Some(ui) = weak.upgrade() else { return };
        finish_room_change(
            &ui,
            &prefs,
            settings::rename_selected(&prefs, Some(&ui.get_invite_url()), name.as_str()),
        );
    });
    let weak = window.as_weak();
    let prefs = path.clone();
    window.on_join_dialog_closed(move || {
        if let Some(ui) = weak.upgrade() {
            open_pending_invite(&ui, &prefs);
        }
    });
    let weak = window.as_weak();
    let prefs = path.clone();
    window.on_join_room(move |url| {
        let Some(ui) = weak.upgrade() else { return };
        let url = url.to_string();
        start_join(&ui, &prefs, move |path, progress| {
            settings::join_with_progress(path, &url, true, progress)
        });
    });
    let weak = window.as_weak();
    let prefs = path.clone();
    window.on_select_room(move |url| {
        if let Some(ui) = weak.upgrade() {
            finish_room_change(&ui, &prefs, settings::select(&prefs, &url));
        }
    });
    let refresh_path = path.as_ref().clone();
    let startup_path = path.clone();
    let weak = window.as_weak();
    window.on_leave_room(move || {
        if let Some(ui) = weak.upgrade() {
            finish_room_change(
                &ui,
                &path,
                settings::leave(&path, Some(&ui.get_invite_url())),
            );
        }
    });
    window.on_copy(|value| clipboard_win::set_clipboard_string(value.as_str()).is_ok());
    let closing = window.as_weak();
    window.window().on_close_requested(move || {
        if let Some(ui) = closing.upgrade()
            && ui.global::<NetworkUi>().get_joining()
        {
            ui.window().set_minimized(true);
            return slint::CloseRequestResponse::KeepWindowShown;
        }
        slint::CloseRequestResponse::HideWindow
    });
    window.on_dismiss(|| {
        let _ = slint::quit_event_loop();
    });
    let weak = window.as_weak();
    std::thread::spawn(move || {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::Threading::{INFINITE, WaitForSingleObject};
        while unsafe { WaitForSingleObject(event.as_raw_handle(), INFINITE) } == 0 {
            let result = (|| -> io::Result<_> {
                Ok((
                    settings::nickname(&refresh_path, &peers::windows_name())?,
                    settings::rooms(&refresh_path)?,
                    settings::options(&refresh_path)?,
                    settings::language(&refresh_path)?,
                    uplink::startup::enabled()?,
                ))
            })();
            let inbox = refresh_path.clone();
            if weak
                .upgrade_in_event_loop(move |ui| match result {
                    Ok((nick, rooms, options, language, autostart)) => {
                        open_pending_invite(&ui, &inbox);
                        apply_language(&ui, &language);
                        apply_options(&ui, options, autostart);
                        if ui.get_editing()
                            && ui.get_nickname().as_str() == nick
                            && rooms.first().is_some_and(|r| {
                                r.live.is_some() && r.url == ui.get_invite_url().as_str()
                            })
                        {
                            refresh_members(&ui, &rooms[0]);
                            return;
                        }
                        ui.set_editing(false);
                        ui.set_nickname(nick.into());
                        apply_rooms(&ui, rooms);
                    }
                    Err(e) => show_error(&ui, e),
                })
                .is_err()
            {
                break;
            }
        }
    });
    open_pending_invite(&window, &startup_path);
    window.run()?;
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        let msg = uplink::i18n::launch_error(&e);
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
                std::ptr::null_mut(),
                msg.as_ptr(),
                windows_sys::core::w!("Uplink"),
                0x10,
            );
        }
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_sorts_names_and_numeric_latency_in_both_directions() {
        use slint::Model;
        let names = |by_ping, descending| {
            members(
                peers::room_members(invite::INVITATION, "Аня"),
                peers::SELF_IP,
                &settings::Policy::default(),
                by_ping,
                descending,
            )
            .iter()
            .map(|row| row.nick.to_string())
            .collect::<Vec<_>>()
        };
        let asc = names(false, false);
        let desc = names(false, true);
        assert_eq!(
            asc.iter().rev().collect::<Vec<_>>(),
            desc.iter().collect::<Vec<_>>()
        );
        let russian: Vec<_> = asc
            .iter()
            .map(String::as_str)
            .filter(|name| name.chars().next().unwrap() > 'z')
            .collect();
        assert_eq!(russian, ["Аня", "Базальт", "Грай", "Еловый"]);
        assert_eq!(
            &names(true, false)[..4],
            ["Mira", "Аня", "Orbit", "Базальт"]
        );
        assert_eq!(&names(true, true)[..4], ["Базальт", "Orbit", "Аня", "Mira"]);
    }
}
