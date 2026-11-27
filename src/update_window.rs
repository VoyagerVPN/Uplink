use super::{RoomWindow, UpdateFailure, UpdatePhase, UpdateUi, update_notes};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::{io, os::windows::io::AsRawHandle, path::Path};
use uplink::{
    changed, control,
    updates::{Failure, Phase, Status},
};
use windows_sys::Win32::System::Threading::{INFINITE, WaitForSingleObject};
fn apply(ui: &RoomWindow, state: Status) {
    let update = ui.global::<UpdateUi>();
    update.set_failure(match state.failure {
        None => UpdateFailure::None,
        Some(Failure::Check) => UpdateFailure::Check,
        Some(Failure::Release) => UpdateFailure::Release,
        Some(Failure::Download) => UpdateFailure::Download,
        Some(Failure::Storage) => UpdateFailure::Storage,
        Some(Failure::Package) => UpdateFailure::Package,
        Some(Failure::Permission) => UpdateFailure::Permission,
        Some(Failure::Busy) => UpdateFailure::Busy,
        Some(Failure::InstallFiles) => UpdateFailure::InstallFiles,
        Some(Failure::Installer) => UpdateFailure::Installer,
        Some(Failure::ClientFiles) => UpdateFailure::ClientFiles,
        Some(Failure::ClientStart) => UpdateFailure::ClientStart,
    });
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
        update.set_notes(ModelRc::new(VecModel::from(update_notes::parse(&state.notes))));
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
                let update = ui.global::<UpdateUi>();
                update.set_failure(UpdateFailure::Updater);
                update.set_phase(UpdatePhase::Offline);
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
