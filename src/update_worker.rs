use std::{
    io,
    path::PathBuf,
    sync::mpsc::{self, RecvTimeoutError, SyncSender},
    time::{Duration, Instant},
};
use uplink::{
    control, settings,
    updates::{self, Phase, Status},
};
use windows_sys::Win32::UI::WindowsAndMessaging::PostThreadMessageW;

pub fn start(profile: PathBuf, updated: bool) -> SyncSender<u32> {
    let (send, recv) = mpsc::sync_channel(3);
    let owner = unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() };
    std::thread::spawn(move || {
        let staging = profile.with_file_name("update-cache");
        updates::clean_staging(&staging);
        let auto = || settings::options(&profile).is_ok_and(|o| o.autoupdate);
        let mut state = Status::default();
        if updated {
            state.phase = Phase::Updated;
        }
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
                    publish(&mut state, Phase::Restarting);
                    if updates::Release::install(r, p).is_ok() {
                        unsafe { PostThreadMessageW(owner, crate::tray::QUIT, 0, 0) };
                        break;
                    }
                    publish(&mut state, Phase::InstallFailed);
                }
                continue;
            }
            if cmd == control::UPDATE_CHECK || cmd == control::UPDATE_AUTO {
                if cmd == control::UPDATE_AUTO && (!auto() || state.phase == Phase::Ready) {
                    next = Instant::now() + Duration::from_secs(6 * 3600);
                    continue;
                }
                next = Instant::now() + Duration::from_secs(6 * 3600);
                package = None;
                release = None;
                state = Status::default();
                publish(&mut state, Phase::Checking);
                match updates::latest(env!("CARGO_PKG_VERSION")) {
                    Ok(Some(r)) => {
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
