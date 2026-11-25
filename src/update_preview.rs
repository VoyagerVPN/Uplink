use super::{RoomWindow, UpdatePhase, UpdateUi, apply_language, apply_rooms};
use slint::{ComponentHandle, ModelRc, Timer, TimerMode, VecModel};
use std::{io, rc::Rc, time::Duration};
use uplink::{invite, settings};

fn start(ui: &RoomWindow, phase: UpdatePhase, timer: &Rc<Timer>) {
    let update = ui.global::<UpdateUi>();
    update.set_phase(phase);
    if phase == UpdatePhase::Loading {
        update.set_percent(0);
    }
    let weak = ui.as_weak();
    let clock = Rc::downgrade(timer);
    let interval = if phase == UpdatePhase::Loading {
        220
    } else {
        700
    };
    timer.start(
        TimerMode::Repeated,
        Duration::from_millis(interval),
        move || {
            let (Some(ui), Some(clock)) = (weak.upgrade(), clock.upgrade()) else {
                return;
            };
            let update = ui.global::<UpdateUi>();
            clock.stop();
            match update.get_phase() {
                UpdatePhase::Checking if update.get_version() == update.get_target() => {
                    update.set_phase(UpdatePhase::Current)
                }
                UpdatePhase::Checking if ui.get_autoupdate() => {
                    start(&ui, UpdatePhase::Loading, &clock)
                }
                UpdatePhase::Checking => update.set_phase(UpdatePhase::Found),
                UpdatePhase::Loading => {
                    let percent = (update.get_percent() + 7).min(100);
                    update.set_percent(percent);
                    if percent == 100 {
                        start(&ui, UpdatePhase::Verifying, &clock);
                    } else {
                        clock.restart();
                    }
                }
                UpdatePhase::Verifying => update.set_phase(UpdatePhase::Ready),
                UpdatePhase::Restarting => {
                    update.set_version(update.get_target());
                    update.set_phase(UpdatePhase::Updated);
                }
                _ => {}
            }
        },
    );
}

pub fn run(ui: &RoomWindow, scene: &str) -> Result<(), Box<dyn std::error::Error>> {
    let phase = match scene {
        "idle" => UpdatePhase::Idle,
        "checking" => UpdatePhase::Checking,
        "current" => UpdatePhase::Current,
        "found" => UpdatePhase::Found,
        "loading" => UpdatePhase::Loading,
        "verifying" => UpdatePhase::Verifying,
        "ready" => UpdatePhase::Ready,
        "restarting" => UpdatePhase::Restarting,
        "updated" => UpdatePhase::Updated,
        "offline" => UpdatePhase::Offline,
        "invalid" => UpdatePhase::Invalid,
        _ => return Err(io::Error::other("Unknown update preview").into()),
    };
    let profile = tempfile::tempdir()?;
    let prefs = profile.path().join("prefs.json");
    std::fs::write(
        &prefs,
        serde_json::to_vec(&serde_json::json!({"rooms": invite::NETWORKS}))?,
    )?;
    ui.set_nickname("Ranazy".into());
    apply_rooms(ui, settings::rooms(&prefs)?);
    apply_language(ui, "ru");
    let update = ui.global::<UpdateUi>();
    update.set_target(
        format!(
            "{}.{}.{}",
            env!("CARGO_PKG_VERSION_MAJOR"),
            env!("CARGO_PKG_VERSION_MINOR"),
            env!("CARGO_PKG_VERSION_PATCH").parse::<u32>()? + 1
        )
        .into(),
    );
    update.set_phase(phase);
    update.set_channel_available(true);
    update.set_percent(58);
    if phase == UpdatePhase::Updated {
        update.set_version(update.get_target());
    }
    ui.set_autoupdate(matches!(
        phase,
        UpdatePhase::Loading
            | UpdatePhase::Verifying
            | UpdatePhase::Ready
            | UpdatePhase::Restarting
            | UpdatePhase::Updated
    ));
    fn notes(ui: &RoomWindow) {
        let lang = ui.get_language_code();
        ui.global::<UpdateUi>()
            .set_notes(ModelRc::new(VecModel::from(
                [
                    "Click the version to check for updates.",
                    "A ring next to the version shows download progress.",
                    "Once the download finishes, you choose when to restart.",
                ]
                .map(|s| super::ReleaseNote {
                    text: uplink::i18n::message(&lang, s).into(),
                    bullet: true,
                    ..Default::default()
                })
                .to_vec(),
            )));
    }
    notes(ui);
    let weak = ui.as_weak();
    ui.on_change_language(move |lang| {
        if let Some(ui) = weak.upgrade() {
            apply_language(&ui, &lang);
            notes(&ui);
        }
    });
    let timer = Rc::new(Timer::default());
    let trigger = |phase| {
        let weak = ui.as_weak();
        let timer = timer.clone();
        move || {
            if let Some(ui) = weak.upgrade() {
                start(&ui, phase, &timer);
            }
        }
    };
    update.on_check(trigger(UpdatePhase::Checking));
    update.on_download(trigger(UpdatePhase::Loading));
    update.on_install(trigger(UpdatePhase::Restarting));
    let weak = ui.as_weak();
    ui.on_change_option(move |key, value| {
        if let Some(ui) = weak.upgrade() {
            match key.as_str() {
                "autoupdate" => {
                    ui.set_autoupdate(value);
                    if value
                        && matches!(
                            ui.global::<UpdateUi>().get_phase(),
                            UpdatePhase::Idle | UpdatePhase::Found
                        )
                    {
                        start(&ui, UpdatePhase::Checking, &timer);
                    }
                }
                "power" => ui.set_power_on(value),
                "autostart" => ui.set_autostart(value),
                "autoconnect" => ui.set_autoconnect(value),
                _ => {}
            }
        }
    });
    ui.on_dismiss(|| {
        let _ = slint::quit_event_loop();
    });
    ui.run()?;
    Ok(())
}
