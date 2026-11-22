use super::{RoomWindow, UpdatePhase, UpdateUi};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{io, os::windows::io::AsRawHandle, path::Path};
use uplink::{
    changed, control,
    updates::{Phase, Status},
};
use windows_sys::Win32::System::Threading::{INFINITE, WaitForSingleObject};
fn apply(ui: &RoomWindow, state: Status) {
    let update = ui.global::<UpdateUi>();
    update.set_phase(match state.phase {
        Phase::Idle => UpdatePhase::Idle,
        Phase::Checking => UpdatePhase::Checking,
        Phase::Current => UpdatePhase::Current,
        Phase::Found => UpdatePhase::Found,
        Phase::Loading => UpdatePhase::Loading,
        Phase::Verifying => UpdatePhase::Verifying,
        Phase::Ready => UpdatePhase::Ready,
        Phase::Restarting => UpdatePhase::Restarting,
        Phase::Updated => UpdatePhase::Updated,
        Phase::Offline => UpdatePhase::Offline,
        Phase::Invalid => UpdatePhase::Invalid,
        Phase::InstallFailed => UpdatePhase::InstallFailed,
    });
    if update.get_target().as_str() != state.target
        || update.get_notes().row_count() == 0 && !state.notes.is_empty()
    {
        let rows: Vec<_> = state
            .notes
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|line| {
                let text = line
                    .strip_prefix("## ")
                    .or_else(|| line.strip_prefix("- "))
                    .unwrap_or(line);
                super::ReleaseNote {
                    text: text.replace("**", "").into(),
                    heading: line.starts_with("## "),
                    bullet: line.starts_with("- "),
                }
            })
            .collect();
        update.set_notes(ModelRc::new(VecModel::from(rows)));
    }
    update.set_target(state.target.into());
    update.set_percent(state.percent.into());
}
use slint::Model;
pub fn bind(window: &RoomWindow, profile: &Path) -> io::Result<()> {
    let update = window.global::<UpdateUi>();
    update.set_channel_available(true);
    let event = changed::listen_update()?;
    let profile = profile.to_owned();
    if !control::tray_window().is_null()
        && let Ok(state) = Status::load(&profile)
    {
        apply(window, state);
    }
    let send = |cmd| {
        let weak = window.as_weak();
        move || {
            if control::send(cmd).is_err()
                && let Some(ui) = weak.upgrade()
            {
                ui.global::<UpdateUi>().set_phase(UpdatePhase::Offline);
            }
        }
    };
    update.on_check(send(control::UPDATE_CHECK));
    update.on_download(send(control::UPDATE_DOWNLOAD));
    update.on_install(send(control::UPDATE_INSTALL));
    let weak = window.as_weak();
    std::thread::spawn(move || {
        while unsafe { WaitForSingleObject(event.as_raw_handle(), INFINITE) } == 0 {
            if let Ok(state) = Status::load(&profile)
                && weak
                    .upgrade_in_event_loop(move |ui| apply(&ui, state))
                    .is_err()
            {
                break;
            }
        }
    });
    Ok(())
}
