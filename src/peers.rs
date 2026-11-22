pub const ROOM_NAME: &str = "После работы";
pub const SELF_IP: &str = "192.0.2.11";

pub struct Peer<'a> {
    pub nick: &'a str,
    pub ip: &'a str,
    pub online: bool,
    pub own: bool,
    pub ping_ms: Option<u32>,
}

pub fn members(nick: &str) -> Vec<Peer<'_>> {
    // Здесь появятся данные транспорта. Ноль не заменяет отсутствующее измерение задержки.
    [
        (nick, SELF_IP, true, server_ping_ms(true)),
        ("Mira", "192.0.2.10", true, Some(24)),
        ("Orbit", "192.0.2.12", true, Some(93)),
        ("Базальт", "192.0.2.13", true, Some(186)),
        ("Еловый", "192.0.2.14", false, None),
        ("Kaiser", "192.0.2.15", false, None),
        ("Грай", "192.0.2.16", false, None),
        ("Bishop", "192.0.2.17", false, None),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (nick, ip, online, ping_ms))| Peer {
        nick,
        ip,
        online,
        own: i == 0,
        ping_ms,
    })
    .collect()
}

pub fn room_members<'a>(url: &str, nick: &'a str) -> Vec<Peer<'a>> {
    use crate::invite::{ARENA, COOP, NIGHT, PINE};
    let rows: &[(&str, &'static str, Option<u32>)] = match url {
        COOP => &[
            ("", "198.51.100.11", Some(54)),
            ("Каррамба", "198.51.100.20", Some(32)),
        ],
        ARENA => &[
            ("", "10.42.0.11", Some(42)),
            ("Грай", "10.42.0.20", Some(19)),
            ("Базальт", "10.42.0.15", Some(52)),
            ("Mira", "10.42.0.16", Some(35)),
        ],
        PINE => &[
            ("", "10.64.0.11", Some(31)),
            ("Еловый", "10.64.0.20", Some(18)),
            ("Kaiser", "10.64.0.14", None),
        ],
        NIGHT => &[
            ("", "10.85.0.11", Some(67)),
            ("Orbit", "10.85.0.20", Some(72)),
            ("Bishop", "10.85.0.18", Some(35)),
            ("Mira", "10.85.0.16", Some(85)),
            ("Грай", "10.85.0.14", None),
            ("Монарх", "10.85.0.12", Some(132)),
        ],
        _ => return members(nick),
    };
    rows.iter()
        .map(|&(name, ip, ping_ms)| Peer {
            nick: if name.is_empty() { nick } else { name },
            ip,
            online: ping_ms.is_some(),
            own: name.is_empty(),
            ping_ms,
        })
        .collect()
}

pub fn server_ping_ms(joined: bool) -> Option<u32> {
    joined.then_some(38)
}

pub fn signal_bars(ms: Option<u32>) -> i32 {
    match ms {
        None => 0,
        Some(0..=70) => 3,
        Some(71..=150) => 2,
        Some(_) => 1,
    }
}

pub fn windows_name() -> String {
    std::env::var("USERNAME")
        .unwrap_or_else(|_| crate::i18n::message(&crate::i18n::current(), "Member").into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_exercises_strong_medium_and_weak_connections() {
        let participants = members("Я");
        assert_eq!(
            participants.iter().find(|p| p.own).unwrap().ping_ms,
            Some(38)
        );
        let levels: Vec<_> = participants
            .iter()
            .filter_map(|p| p.ping_ms.map(|ms| signal_bars(Some(ms))))
            .collect();
        for level in [1, 2, 3] {
            assert!(levels.contains(&level));
        }
        assert!(
            participants
                .iter()
                .filter(|p| !p.online)
                .all(|p| p.ping_ms.is_none())
        );
        assert_eq!(signal_bars(None), 0);
        assert_eq!(server_ping_ms(false), None);
        assert!(server_ping_ms(true).is_some());
    }
}
