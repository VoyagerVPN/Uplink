use std::{
    io,
    path::PathBuf,
    sync::mpsc::{self, RecvTimeoutError, SyncSender},
    time::{Duration, Instant},
};
use uplink::{
    control, settings,
    updates::{self, Failure, Phase, Status},
};
use windows_sys::Win32::UI::WindowsAndMessaging::PostThreadMessageW;

pub fn start(profile: PathBuf, updated: bool) -> SyncSender<u32> {
    let (send, recv) = mpsc::sync_channel(3);
    let owner = unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() };
    std::thread::spawn(move || {
        let staging = profile.with_file_name("update-cache");
        updates::clean_staging(&staging);
        let auto = || settings::options(&profile).is_ok_and(|o| o.autoupdate);
        let current = env!("CARGO_PKG_VERSION");
        let mut state = Status::load(&profile).unwrap_or_default().after_restart(current, updated);
        let _ = state.save(&profile);
        let mut release = None;
        let mut package = None;
        let mut next = Instant::now();
        loop {
            let cmd = match recv.recv_timeout(next.saturating_duration_since(Instant::now())) {
                Ok(cmd) => cmd,
                Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => control::UPDATE_AUTO,
            };
            let publish = |state: &mut Status, phase| {
                state.phase = phase;
                let _ = state.save(&profile);
            };
            if cmd == control::UPDATE_INSTALL {
                if !matches!(state.phase, Phase::Ready | Phase::InstallFailed) {
                    continue;
                }
                if let (Some(r), Some(p)) = (&release, &package) {
                    state.failure = None;
                    publish(&mut state, Phase::Restarting);
                    match updates::Release::install(r, p, &profile) {
                        Ok(()) => {
                            unsafe { PostThreadMessageW(owner, crate::tray::QUIT, 0, 0) };
                            break;
                        }
                        Err(failure) => {
                            state.failure = Some(failure);
                            publish(&mut state, Phase::InstallFailed);
                        }
                    }
                }
                continue;
            }
            if cmd == control::UPDATE_CHECK || cmd == control::UPDATE_AUTO {
                if cmd == control::UPDATE_AUTO && (!auto() || state.phase == Phase::Ready) {
                    next = Instant::now() + Duration::from_secs(6 * 3600);
                    continue;
                }
                next = Instant::now() + Duration::from_secs(6 * 3600);
                // После перезапуска сохраняем объяснение сбоя, пока не вышла замена
                // или пользователь сам не запустил повторную проверку.
                let latest = if cmd == control::UPDATE_AUTO && state.phase.failed() && !state.target.is_empty() {
                    match updates::latest(&state.target) {
                        Ok(Some(newer)) => Ok(Some(newer)),
                        _ => continue,
                    }
                } else {
                    state = Status::default();
                    publish(&mut state, Phase::Checking);
                    updates::latest(current)
                };
                package = None;
                release = None;
                match latest {
                    Ok(Some(r)) => {
                        state = Status::default();
                        state.target = r.version.clone();
                        state.notes = r.notes.clone();
                        release = Some(r);
                        publish(&mut state, Phase::Found);
                    }
                    Ok(None) => {
                        state = Status::default();
                        publish(&mut state, Phase::Current);
                        continue;
                    }
                    Err(e) => {
                        state.failure = Some(if e.kind() == io::ErrorKind::InvalidData {
                            Failure::Release
                        } else {
                            Failure::Check
                        });
                        publish(
                            &mut state,
                            if e.kind() == io::ErrorKind::InvalidData {
                                Phase::Invalid
                            } else {
                                Phase::Offline
                            },
                        );
                        next = Instant::now() + Duration::from_secs(15 * 60);
                        continue;
                    }
                }
                if !auto() {
                    continue;
                }
            }
            if !matches!(state.phase, Phase::Found | Phase::Invalid | Phase::Offline) {
                continue;
            }
            let Some(r) = release.as_ref() else { continue };
            state.percent = 0;
            state.failure = None;
            publish(&mut state, Phase::Loading);
            let mut progress = state.clone();
            let mut verification = state.clone();
            match r.download(
                &staging,
                |percent| {
                    progress.percent = percent;
                    let _ = progress.save(&profile);
                },
                || publish(&mut verification, Phase::Verifying),
            ) {
                Ok(p) => {
                    package = Some(p);
                    state.percent = 100;
                    publish(&mut state, Phase::Ready);
                }
                Err(e) => {
                    package = None;
                    state.failure = Some(match e.kind() {
                        io::ErrorKind::InvalidData => Failure::Package,
                        io::ErrorKind::PermissionDenied | io::ErrorKind::StorageFull => Failure::Storage,
                        _ => Failure::Download,
                    });
                    publish(
                        &mut state,
                        if e.kind() == io::ErrorKind::InvalidData {
                            Phase::Invalid
                        } else {
                            Phase::Offline
                        },
                    );
                }
            }
        }
    });
    send
}
