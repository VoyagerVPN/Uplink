use super::{diagnostics, read, room_info, saved_urls, ssh_keys, transport_lock, update};
use crate::transport::{Membership, SshRequest};
use std::{io, path::Path};

pub enum Action<'a> {
    Request(&'a str),
    Cancel(&'a str),
    Reject(&'a str),
}

pub fn apply(path: &Path, key: &str, action: Action<'_>) -> io::Result<SshRequest> {
    let _guard = transport_lock(path)?;
    let prefs = read(path)?;
    if !saved_urls(&prefs)?.contains(&key) || prefs["power"] == false {
        return Err(io::Error::other("The selected network has changed"));
    }
    let live = room_info(&prefs, key)
        .live
        .filter(|live| live.ssh_supported)
        .ok_or_else(|| io::Error::other("This server does not support SSH requests."))?;
    let request = match action {
        Action::Request(ip) => {
            if !live.ssh_key_requests {
                return Err(io::Error::other(
                    "Update this Uplink server to send SSH requests.",
                ));
            }
            let peer = live
                .rows
                .iter()
                .find(|peer| peer.ip == ip && peer.node != live.node)
                .ok_or_else(|| io::Error::other("Participant is unavailable"))?;
            if let Some(previous) = live.ssh_requests.iter().find(|request| {
                request.requester == live.node
                    && request.recipient == peer.node
                    && request.state != "pending"
            }) {
                clear_key(path, &live, previous);
            }
            let public_key = ssh_keys::for_request(key, &live.node, &peer.node)?;
            let reply = live.request_ssh(&peer.node, &public_key)?;
            if reply.requester != live.node
                || reply.recipient != peer.node
                || reply.state != "pending"
                || reply.public_key.as_deref() != Some(&public_key)
            {
                return Err(io::Error::other(
                    "The Uplink server returned an unexpected SSH request.",
                ));
            }
            reply
        }
        Action::Cancel(id) => live.respond_ssh(id, "cancel")?,
        Action::Reject(id) => live.respond_ssh(id, "reject")?,
    };
    clear_key(path, &live, &request);
    update(path, |prefs| {
        let record = &mut prefs["live"][key];
        if record["session"] != live.session {
            return Ok(());
        }
        let mut requests = live.ssh_requests;
        requests.retain(|existing| {
            existing.requester != request.requester || existing.recipient != request.recipient
        });
        requests.push(request.clone());
        record["ssh_requests"] = serde_json::to_value(requests)?;
        Ok(())
    })?;
    Ok(request)
}

fn clear_key(path: &Path, live: &Membership, request: &SshRequest) {
    if request.requester == live.node
        && matches!(
            request.state.as_str(),
            "cancelled" | "rejected" | "revoked" | "expired"
        )
        && let Some(public_key) = &request.public_key
        && let Err(error) =
            ssh_keys::forget(&live.key(), &live.node, &request.recipient, public_key)
    {
        diagnostics::failure(path, "remove_ssh_request_key", format_args!(""), &error);
    }
}

pub(super) fn clear_closed_keys(path: &Path, live: &Membership) {
    for request in &live.ssh_requests {
        clear_key(path, live, request);
    }
}
