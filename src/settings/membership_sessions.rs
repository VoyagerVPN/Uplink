use super::password;
use serde_json::{Map, Value};
use std::{collections::BTreeMap, io};

#[derive(Default)]
pub(super) struct Sessions(BTreeMap<(&'static str, String), (String, String)>);

const SECTIONS: [&str; 2] = ["live", "pending"];

fn memberships<'a>(
    prefs: &'a mut Value,
    section: &str,
) -> impl Iterator<Item = (&'a String, &'a mut Value)> {
    prefs
        .get_mut(section)
        .and_then(Value::as_object_mut)
        .into_iter()
        .flat_map(Map::iter_mut)
}

pub(super) fn needs_migration(prefs: &Value) -> bool {
    SECTIONS.iter().any(|section| {
        prefs[*section].as_object().is_some_and(|rooms| {
            rooms.values().any(|room| {
                room["session"].is_object()
                    || room["session"].as_str().is_some_and(|s| !s.is_empty())
            })
        })
    })
}

impl Sessions {
    pub(super) fn decrypt(prefs: &mut Value) -> io::Result<Self> {
        let mut sessions = Self::default();
        for section in SECTIONS {
            for (key, room) in memberships(prefs, section) {
                let Some(room) = room.as_object_mut() else {
                    continue;
                };
                let sealed = room.remove("session_dpapi");
                let session = &room.get("session").unwrap_or(&Value::Null);
                let encoded = match session {
                    Value::Object(envelope) => envelope
                        .get("dpapi")
                        .and_then(Value::as_str)
                        .ok_or(io::ErrorKind::InvalidData)?,
                    Value::String(token) if token.is_empty() && sealed.is_some() => sealed
                        .as_ref()
                        .and_then(Value::as_str)
                        .ok_or(io::ErrorKind::InvalidData)?,
                    _ => continue,
                };
                let token = password::unprotect(&format!("uplink-membership:{key}"), encoded)?;
                sessions
                    .0
                    .insert((section, key.clone()), (token.clone(), encoded.into()));
                room.insert("session".into(), Value::String(token));
            }
        }
        Ok(sessions)
    }

    pub(super) fn encrypt(mut self, prefs: &mut Value) -> io::Result<()> {
        for section in SECTIONS {
            for (key, room) in memberships(prefs, section) {
                let Some(token) = room["session"].as_str().filter(|s| !s.is_empty()) else {
                    continue;
                };
                let sealed = match self.0.remove(&(section, key.clone())) {
                    Some((previous, sealed)) if previous == token => sealed,
                    _ => password::protect(&format!("uplink-membership:{key}"), token)?,
                };
                // Старый клиент читает строковое поле, но не получает токен или его шифротекст.
                room["session"] = "".into();
                room["session_dpapi"] = sealed.into();
            }
        }
        Ok(())
    }
}
