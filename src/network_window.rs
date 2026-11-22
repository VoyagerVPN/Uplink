use super::*;
use slint::winit_030::{EventResult, WinitWindowAccessor, winit::event::WindowEvent};
use std::{cell::RefCell, path::PathBuf};
type Editor = Rc<RefCell<Option<avatar::Draft>>>;

fn problem(ui: &RoomWindow, e: io::Error) {
    ui.global::<NetworkUi>()
        .set_error(uplink::i18n::message(&ui.get_language_code(), &e.to_string()).into());
}

fn load_file(ui: &RoomWindow, draft: &Editor, path: PathBuf) {
    let net = ui.global::<NetworkUi>();
    if !net.get_manages() || net.get_panel() != "general" {
        return;
    }
    match avatar::Draft::open(&path) {
        Ok(photo) => {
            net.set_preview(photo.preview());
            net.set_zoom(photo.zoom);
            *draft.borrow_mut() = Some(photo);
            net.set_error("".into());
            net.set_modal("crop".into());
        }
        Err(e) => problem(ui, e),
    }
}
fn adjust(ui: &RoomWindow, draft: &Editor, x: f32, y: f32, zoom: f32) {
    if let Some(d) = draft.borrow_mut().as_mut() {
        d.adjust(x, y, zoom);
        let net = ui.global::<NetworkUi>();
        net.set_zoom(d.zoom);
        net.set_preview(d.preview());
    }
}

pub(super) fn bind_network(window: &RoomWindow, prefs: Rc<PathBuf>) {
    let draft: Editor = Default::default();
    let net = window.global::<NetworkUi>();
    let weak = window.as_weak();
    let profile = prefs.clone();
    let generation = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    net.on_request_link(move || {
        let Some(ui) = weak.upgrade() else { return };
        let Ok(rooms) = settings::rooms(&profile) else {
            return;
        };
        let Some(live) = rooms.into_iter().next().and_then(|r| r.live) else {
            return;
        };
        let net = ui.global::<NetworkUi>();
        net.set_link("".into());
        net.set_qr_image(Default::default());
        let generation = generation.clone();
        let request = generation.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        if !net.get_link_on() && !net.get_qr_on() {
            return;
        }
        let weak = ui.as_weak();
        std::thread::spawn(move || {
            let result = live.share();
            let _ = weak.upgrade_in_event_loop(move |ui| {
                let net = ui.global::<NetworkUi>();
                if generation.load(std::sync::atomic::Ordering::Relaxed) != request
                    || net.get_modal() != "invite"
                    || ui.get_invite_url().as_str() != live.key()
                {
                    return;
                }
                match result {
                    Ok(link) => {
                        net.set_qr_image(avatar::invitation_qr(&link));
                        net.set_link(link.into());
                    }
                    Err(e) => problem(&ui, e),
                }
            });
        });
    });
    let weak = window.as_weak();
    let d = draft.clone();
    let profile = prefs.clone();
    net.on_action(move |action, value| {
        let Some(ui) = weak.upgrade() else { return };
        let net = ui.global::<NetworkUi>();
        if !net.get_manages() || net.get_saving() {
            return;
        }
        if action == "password-mode" && net.get_live() && !net.get_password_on() {
            net.set_setting_password(!net.get_setting_password());
            return;
        }
        if action == "pick" {
            if let Some(path) = avatar::pick() {
                load_file(&ui, &d, path);
            }
            return;
        }
        let url = ui.get_invite_url();
        if net.get_live() && action != "save-avatar" {
            net.set_saving(true);
            let (action, value, url) = (action.to_string(), value.to_string(), url.to_string());
            let path = profile.as_ref().clone();
            let weak = ui.as_weak();
            std::thread::spawn(move || {
                let result = settings::network_action(&path, &url, &action, &value);
                let _ = weak.upgrade_in_event_loop(move |ui| {
                    let net = ui.global::<NetworkUi>();
                    net.set_saving(false);
                    if ui.get_invite_url().as_str() != url {
                        return;
                    }
                    match result.and_then(|_| settings::rooms(&path)) {
                        Ok(rooms) => {
                            if matches!(action.as_str(), "transfer" | "remove") {
                                net.invoke_close_modal();
                            }
                            if action == "password" {
                                net.set_password_revision(
                                    net.get_password_revision().wrapping_add(1),
                                );
                            }
                            net.set_setting_password(false);
                            net.set_error("".into());
                            apply_rooms(&ui, rooms);
                        }
                        Err(error) => problem(&ui, error),
                    }
                });
            });
            return;
        }
        let result = if action == "save-avatar" {
            d.borrow()
                .as_ref()
                .ok_or_else(|| io::Error::other("Select an image"))
                .and_then(|p| p.encode())
                .and_then(|encoded| settings::network_action(&profile, &url, "avatar", &encoded))
        } else {
            settings::network_action(&profile, &url, &action, &value)
        };
        match result.and_then(|()| settings::rooms(&profile)) {
            Ok(rooms) => {
                if matches!(action.as_str(), "save-avatar" | "transfer" | "remove") {
                    net.invoke_close_modal();
                }
                net.set_setting_password(false);
                net.set_error("".into());
                apply_rooms(&ui, rooms);
            }
            Err(e) => problem(&ui, e),
        }
    });
    let weak = window.as_weak();
    net.on_login(move |host, name, pass| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        let net = ui.global::<NetworkUi>();
        if net.get_joining() || host.trim().is_empty() || name.trim().is_empty() || pass.is_empty()
        {
            return;
        }
        net.set_joining(true);
        let weak = ui.as_weak();
        let path = prefs.as_ref().clone();
        let (host, name, pass) = (host.to_string(), name.to_string(), pass.to_string());
        std::thread::spawn(move || {
            let result = settings::join_on_server(&path, &host, &name, &pass);
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.global::<NetworkUi>().set_joining(false);
                finish_room_change(&ui, &path, result)
            });
        });
    });
    net.on_clipboard(|s| clipboard_win::set_clipboard_string(&s).is_ok());
    let d = draft.clone();
    let weak = window.as_weak();
    net.on_clear_draft(move || {
        d.borrow_mut().take();
        if let Some(ui) = weak.upgrade() {
            ui.global::<NetworkUi>().set_preview(Default::default());
        }
    });
    let d = draft.clone();
    let weak = window.as_weak();
    net.on_pan(move |x, y| {
        if let Some(ui) = weak.upgrade() {
            let z = ui.global::<NetworkUi>().get_zoom();
            adjust(&ui, &d, x, y, z);
        }
    });
    let d = draft.clone();
    let weak = window.as_weak();
    net.on_scale(move |z| {
        if let Some(ui) = weak.upgrade() {
            adjust(&ui, &d, 0., 0., z);
        }
    });
    let weak = window.as_weak();
    window.window().on_winit_window_event(move |win, event| {
        if let Some(ui) = weak.upgrade() {
            let net = ui.global::<NetworkUi>();
            if net.get_manages() && net.get_panel() == "general" && net.get_modal().is_empty() {
                match event {
                    WindowEvent::HoveredFile(_) => net.set_dragging_file(true),
                    WindowEvent::HoveredFileCancelled => net.set_dragging_file(false),
                    WindowEvent::DroppedFile(path) => {
                        net.set_dragging_file(false);
                        let on_tile = win
                            .with_winit_window(|w| {
                                let mut pt = windows_sys::Win32::Foundation::POINT::default();
                                if unsafe {
                                    windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos(
                                        &mut pt,
                                    )
                                } == 0
                                {
                                    return false;
                                }
                                let Ok(origin) = w.inner_position() else {
                                    return false;
                                };
                                let x = (pt.x - origin.x) as f32 / win.scale_factor();
                                let y = (pt.y - origin.y) as f32 / win.scale_factor();
                                let bounds = net.get_drop_bounds();
                                (bounds.x..bounds.x + bounds.w).contains(&x)
                                    && (bounds.y..bounds.y + bounds.h).contains(&y)
                            })
                            .unwrap_or(false);
                        if on_tile {
                            load_file(&ui, &draft, path.clone());
                        }
                    }
                    _ => {}
                }
            }
        }
        EventResult::Propagate
    });
}
