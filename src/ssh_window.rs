use super::*;
use slint::Model;
use std::{cell::RefCell, collections::HashSet, path::PathBuf};
use uplink::{settings::ssh_requests, transport::SshRequest};

fn key(room: &settings::Room, request: &SshRequest) -> String {
    format!("{}#{}", room.url, request.id)
}

fn name(room: &settings::Room, node: &str) -> String {
    room.live.as_ref()
        .and_then(|live| live.rows.iter().find(|row| row.node == node))
        .map(|row| row.nick.clone())
        .unwrap_or_else(|| node.to_owned())
}

fn local_time(timestamp: i64, language: &str) -> slint::SharedString {
    use chrono::{Local, TimeZone};
    Local.timestamp_opt(timestamp, 0).single().map(|date| {
        date.format(if language == "ru" { "%d.%m.%Y %H:%M:%S" } else { "%Y-%m-%d %H:%M:%S" })
            .to_string().into()
    }).unwrap_or_default()
}

pub(super) fn refresh(ui: &RoomWindow, rooms: &[settings::Room]) {
    let notices = ui.global::<NoticesUi>();
    let mut requests = Vec::new();
    let mut history = Vec::new();
    for room in rooms {
        let Some(live) = &room.live else { continue };
        for request in live.ssh_requests.iter().filter(|r| r.state == "pending" && r.recipient == live.node) {
            requests.push((request.occurred_at, SshNotice {
                id: key(room, request).into(),
                caption: name(room, &request.requester).into(),
                outcome: request.state.as_str().into(),
                local_time: "".into(),
            }));
        }
        for event in &live.ssh_history {
            let peer = if event.requester == live.node { &event.recipient } else { &event.requester };
            history.push((event.occurred_at, SshNotice {
                id: key(room, event).into(),
                caption: name(room, peer).into(),
                outcome: event.state.as_str().into(),
                local_time: local_time(event.occurred_at, ui.get_language_code().as_str()),
            }));
        }
    }
    requests.sort_by_key(|(time, _)| std::cmp::Reverse(*time));
    history.sort_by_key(|(time, _)| std::cmp::Reverse(*time));
    let known: HashSet<_> = notices.get_history().iter().map(|event| event.id).collect();
    let actionable: HashSet<_> = notices.get_requests().iter().map(|request| request.id).collect();
    if history.iter().any(|(_, event)| !known.contains(&event.id))
        || requests.iter().any(|(_, request)| !actionable.contains(&request.id))
    {
        notices.set_unread(true);
    }
    let pending: Vec<_> = requests.into_iter().map(|(_, request)| request).collect();
    let ssh = ui.global::<SshUi>();
    if !ssh.get_active_id().is_empty() && !pending.iter().any(|event| event.id == ssh.get_active_id()) {
        ssh.set_active_id("".into());
    }
    notices.set_requests(ModelRc::new(VecModel::from(pending)));
    notices.set_history(ModelRc::new(VecModel::from(history.into_iter().map(|(_, event)| event).collect::<Vec<_>>())));
    if ui.get_can_prompt_ssh() {
        ui.invoke_prompt_ssh();
    }
}

fn inspect(ui: &RoomWindow, path: &Path, reference: &str) -> io::Result<()> {
    let rooms = settings::rooms(path)?;
    let (network, id) = reference.split_once('#').ok_or(io::ErrorKind::InvalidInput)?;
    let Some(room) = rooms.iter().find(|r| r.url == network) else { return Ok(()) };
    let Some(live) = &room.live else { return Ok(()) };
    let Some(request) = live.ssh_requests.iter().find(|r| r.id == id && r.recipient == live.node && r.state == "pending")
        else { return Ok(()) };
    let ssh = ui.global::<SshUi>();
    let avatar = live.avatar_file(path).and_then(|p| slint::Image::load_from_path(&p).ok());
    ssh.set_requester(name(room, &request.requester).into());
    ssh.set_network_name(room.name.as_str().into());
    ssh.set_initials(room.name.split_whitespace().take(2).filter_map(|word| word.chars().next())
        .flat_map(char::to_uppercase).collect::<String>().into());
    ssh.set_has_avatar(avatar.is_some());
    ssh.set_avatar(avatar.unwrap_or_default());
    ssh.set_error("".into());
    ssh.set_active_id(reference.into());
    Ok(())
}

pub(super) fn bind(ui: &RoomWindow, path: &Rc<PathBuf>) {
    let shown = Rc::new(RefCell::new(HashSet::new()));
    let notices = ui.global::<NoticesUi>();
    let weak = ui.as_weak();
    notices.on_read(move || {
        if let Some(ui) = weak.upgrade() { ui.global::<NoticesUi>().set_unread(false); }
    });
    let weak = ui.as_weak();
    let prefs = path.clone();
    let manually_shown = shown.clone();
    notices.on_inspect(move |id| {
        if let Some(ui) = weak.upgrade() {
            manually_shown.borrow_mut().insert(id.clone());
            if let Err(e) = inspect(&ui, &prefs, &id) { show_error(&ui, e); }
        }
    });
    let weak = ui.as_weak();
    ui.on_prompt_ssh(move || {
        let Some(ui) = weak.upgrade() else { return };
        let ssh = ui.global::<SshUi>();
        if ssh.get_working() || !ssh.get_active_id().is_empty() || !ui.get_can_prompt_ssh() { return; }
        let requests = ui.global::<NoticesUi>().get_requests();
        if let Some(next) = requests.iter().find(|r| !shown.borrow().contains(&r.id)) {
            ui.global::<NoticesUi>().invoke_inspect(next.id);
        }
    });
    let weak = ui.as_weak();
    let prefs = path.clone();
    ui.on_ssh_member(move |action, ip| {
        let Some(ui) = weak.upgrade() else { return };
        let selected = ui.get_invite_url().to_string();
        if action == "request" {
            run_action(&ui, &prefs, selected, action.to_string(), ip.to_string());
            return;
        }
        let result = settings::rooms(&prefs);
        let Ok(rooms) = result else { return };
        let Some(room) = rooms.iter().find(|r| r.url == selected) else { return };
        let Some(live) = &room.live else { return };
        let Some(peer) = live.rows.iter().find(|row| row.ip == ip.as_str()) else { return };
        let Some(request) = live.ssh_requests.iter().find(|r| r.state == "pending" && if action == "review" {
            r.requester == peer.node && r.recipient == live.node
        } else {
            r.recipient == peer.node && r.requester == live.node
        }) else { return };
        if action == "review" { ui.global::<NoticesUi>().invoke_inspect(key(room, request).into()); }
        else { run_action(&ui, &prefs, selected, action.to_string(), request.id.clone()); }
    });
    let weak = ui.as_weak();
    let prefs = path.clone();
    ui.global::<SshUi>().on_decide(move |decision| {
        let Some(ui) = weak.upgrade() else { return };
        let ssh = ui.global::<SshUi>();
        match decision.as_str() {
            "dismiss" => {
                ssh.set_active_id("".into());
                ui.invoke_prompt_ssh();
            }
            "approve" => ssh.set_error(uplink::i18n::error_message(
                ui.get_language_code().as_str(), "SSH access cannot be opened in this build."
            ).into()),
            "reject" => if let Some((network, id)) = ssh.get_active_id().split_once('#') {
                run_action(&ui, &prefs, network.to_owned(), "reject".to_owned(), id.to_owned());
            },
            _ => {}
        }
    });
}

fn run_action(ui: &RoomWindow, prefs: &Path, network: String, action: String, target: String) {
    let ssh = ui.global::<SshUi>();
    if ssh.get_working() { return; }
    ssh.set_working(true);
    ssh.set_error("".into());
    ui.set_error("".into());
    let weak = ui.as_weak();
    let prefs = prefs.to_owned();
    std::thread::spawn(move || {
        let command = match action.as_str() {
            "request" => ssh_requests::Action::Request(&target),
            "cancel" => ssh_requests::Action::Cancel(&target),
            _ => ssh_requests::Action::Reject(&target),
        };
        let outcome = ssh_requests::apply(&prefs, &network, command).and_then(|_| settings::rooms(&prefs));
        let _ = weak.upgrade_in_event_loop(move |ui| {
            let ssh = ui.global::<SshUi>();
            ssh.set_working(false);
            match outcome {
                Ok(rooms) => {
                    if action == "request" && ui.get_invite_url() == network {
                        ssh.set_receipt_ip(target.into());
                        ssh.set_receipt_network(network.into());
                        ssh.set_receipt(ssh.get_receipt().wrapping_add(1));
                    }
                    apply_rooms(&ui, rooms);
                }
                Err(e) if action == "reject" => ssh.set_error(uplink::i18n::error_message(
                    ui.get_language_code().as_str(), &e.to_string()
                ).into()),
                Err(e) => show_error(&ui, e),
            }
        });
    });
}
