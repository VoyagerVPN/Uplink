mod input;

use crate::{TrayChoice, TrayMenu};
use slint::{
    ComponentHandle,
    winit_030::{WinitWindowAccessor, winit},
};
use std::{
    cell::{Cell, RefCell},
    io,
    mem::size_of,
    path::Path,
    rc::Rc,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};
use uplink::{changed, control, settings};
use windows_sys::Win32::{
    Foundation::POINT,
    Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint},
    UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI},
};
use winit::{
    dpi::PhysicalPosition,
    platform::windows::WindowAttributesExtWindows,
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
    window::WindowLevel,
};

fn menu_position(at: POINT) -> PhysicalPosition<i32> {
    let monitor = unsafe { MonitorFromPoint(at, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
        return PhysicalPosition::new(at.x, at.y);
    }
    let mut dpi = 96;
    let mut dpi_y = 96;
    unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut dpi_y) };
    let px = |dp: i32| (dp * dpi as i32 + 48) / 96;
    let work = info.rcWork;
    let gap = px(8);
    let width = px(228);
    let height = px(248);
    let above = at.y - gap - height;
    let top = if above >= work.top { above } else { at.y + gap };
    PhysicalPosition::new(
        (at.x - width / 2).clamp(work.left, (work.right - width).max(work.left)),
        top.clamp(work.top, (work.bottom - height).max(work.top)),
    )
}

pub fn run(x: i32, y: i32, light_taskbar: bool) -> Result<(), Box<dyn std::error::Error>> {
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .with_winit_window_attributes_hook(move |attributes| {
            attributes
                .with_skip_taskbar(true)
                .with_active(false)
                .with_window_level(WindowLevel::AlwaysOnTop)
                .with_position(menu_position(POINT { x, y }))
        })
        .select()?;

    let path = Rc::new(settings::path()?);
    let window = TrayMenu::new()?;
    slint::select_bundled_translation(&settings::language(&path)?)?;
    window.set_light_taskbar(light_taskbar);
    sync_preferences(&window, &path)?;

    let input = Rc::new(RefCell::new(None));
    let active_input = input.clone();
    let weak = window.as_weak();
    slint::spawn_local(async move {
        let Some(ui) = weak.upgrade() else { return };
        let result = async {
            let native = ui.window().winit_window().await.map_err(io::Error::other)?;
            let RawWindowHandle::Win32(handle) =
                native.window_handle().map_err(io::Error::other)?.as_raw()
            else {
                return Err(io::Error::other("Cannot initialize the Uplink tray menu"));
            };
            input::MenuInput::install(handle.hwnd.get() as _)
        }
        .await;
        if result.is_err() {
            let _ = slint::quit_event_loop();
        }
        active_input.replace(Some(result));
    })?;

    let sync = slint::Timer::default();
    let weak = window.as_weak();
    let current = path.clone();
    sync.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(350),
        move || {
            if let Some(ui) = weak.upgrade() {
                let _ = sync_preferences(&ui, &current);
            }
        },
    );

    let choice = Rc::new(Cell::new(None));
    let output = choice.clone();
    let error = Arc::new(Mutex::new(None));
    let failed = error.clone();
    let power = Rc::new(RefCell::new(None));
    let inflight = power.clone();
    let weak = window.as_weak();
    let prefs = path.clone();
    window.on_choose(move |action| match action {
        TrayChoice::Open | TrayChoice::Quit => {
            if action == TrayChoice::Open {
                control::allow_tray_focus();
            }
            output.set(Some(action));
            let _ = slint::quit_event_loop();
        }
        TrayChoice::PowerOn | TrayChoice::PowerOff => {
            let Some(ui) = weak.upgrade() else { return };
            if ui.get_power_busy() {
                return;
            }
            ui.set_power_busy(true);
            let prefs = prefs.as_ref().clone();
            let failed = failed.clone();
            let target = ui.as_weak();
            inflight.replace(Some(thread::spawn(move || {
                let result = apply_choice(&prefs, action).and(changed::notify());
                if let Err(error) = result {
                    *failed.lock().unwrap() = Some(error);
                }
                let _ = target.upgrade_in_event_loop(move |ui| {
                    if failed.lock().unwrap().is_some() {
                        let _ = slint::quit_event_loop();
                    } else {
                        let _ = sync_preferences(&ui, &prefs);
                        ui.set_power_busy(false);
                    }
                });
            })));
        }
        TrayChoice::Green | TrayChoice::Ping | TrayChoice::System => {
            let Some(ui) = weak.upgrade() else { return };
            if let Err(err) =
                apply_choice(&prefs, action).and_then(|()| sync_preferences(&ui, &prefs))
            {
                *failed.lock().unwrap() = Some(err);
                let _ = slint::quit_event_loop();
            } else {
                control::refresh_tray_icon();
            }
        }
    });
    window.run()?;

    if let Some(result) = input.take() {
        drop(result?);
    }
    if let Some(worker) = power.take() {
        worker
            .join()
            .map_err(|_| io::Error::other("Uplink power operation failed"))?;
    }
    if let Some(err) = error.lock().unwrap().take() {
        return Err(err.into());
    }
    if let Some(choice) = choice.get() {
        apply_choice(&path, choice)?;
    }
    Ok(())
}

fn sync_preferences(ui: &TrayMenu, path: &Path) -> io::Result<()> {
    ui.set_power_on(settings::options(path)?.enabled);
    ui.set_mode(match settings::tray_mode(path)?.as_str() {
        "ping" => TrayChoice::Ping,
        "system" => TrayChoice::System,
        _ => TrayChoice::Green,
    });
    Ok(())
}

fn apply_choice(path: &Path, choice: TrayChoice) -> io::Result<()> {
    let mode = match choice {
        TrayChoice::Open => return control::send(control::SHOW).map(|_| ()),
        TrayChoice::Quit => return control::send(control::QUIT).map(|_| ()),
        TrayChoice::PowerOn | TrayChoice::PowerOff => {
            return settings::set_option(path, "power", choice == TrayChoice::PowerOn);
        }
        TrayChoice::Green => "green",
        TrayChoice::Ping => "ping",
        TrayChoice::System => "system",
    };
    settings::save_tray_mode(path, mode)
}

#[cfg(test)]
mod tests {
    use uplink::{invite, settings};

    #[test]
    fn tray_power_preserves_the_network_and_icon_mode() {
        let directory = tempfile::tempdir().unwrap();
        let prefs = directory.path().join("prefs.json");
        settings::join(&prefs, invite::INVITATION).unwrap();
        super::apply_choice(&prefs, crate::TrayChoice::Ping).unwrap();
        super::apply_choice(&prefs, crate::TrayChoice::PowerOff).unwrap();
        assert!(!settings::options(&prefs).unwrap().enabled);
        assert_eq!(settings::tray_mode(&prefs).unwrap(), "ping");
        assert_eq!(settings::rooms(&prefs).unwrap()[0].url, invite::INVITATION);
        super::apply_choice(&prefs, crate::TrayChoice::PowerOn).unwrap();
        assert!(settings::options(&prefs).unwrap().enabled);
        assert_eq!(settings::tray_mode(&prefs).unwrap(), "ping");
        assert_eq!(settings::rooms(&prefs).unwrap()[0].url, invite::INVITATION);
    }
}
