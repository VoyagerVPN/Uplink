pub const INVITATION: &str = "uplink://join/after-hours";

pub const COOP: &str = "uplink://coop.example/rooms/coop";
pub const ARENA: &str = "uplink://join/arena";
pub const PINE: &str = "uplink://join/pine";
pub const NIGHT: &str = "uplink://join/night";
pub const NETWORKS: [&str; 5] = [INVITATION, COOP, ARENA, PINE, NIGHT];

pub fn canonical(value: &str) -> Option<&'static str> {
    let value = value.trim();
    NETWORKS
        .into_iter()
        .find(|&url| url == value.strip_suffix("?via=qr").unwrap_or(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_instance_invites_require_a_host_and_a_single_secret() {
        let token = "a".repeat(64);
        assert!(accepts_invite(&format!(
            "uplink://lan.example/invite/{token}"
        )));
        for authority in [
            "user@lan.example",
            "lan.example:80",
            "-bad.example",
            "..",
            "",
        ] {
            assert!(!accepts_invite(&format!(
                "uplink://{authority}/invite/{token}"
            )));
        }
        assert!(!accepts_invite(&format!(
            "uplink://lan.example/invite/{token}/extra"
        )));
    }

    #[test]
    fn opens_only_the_known_room_link() {
        assert!(accepts_invite("  uplink://join/after-hours  "));
        assert!(!accepts_invite("uplink://join/unknown"));
        assert!(!accepts_invite(
            "https://example.org/?url=uplink://join/after-hours"
        ));
    }
}

pub fn accepts_invite(value: &str) -> bool {
    canonical(value).is_some() || instance_link(value).is_some()
}

pub fn instance_link(value: &str) -> Option<(&str, &str)> {
    let (host, secret) = value
        .trim()
        .strip_prefix("uplink://")?
        .split_once("/invite/")?;
    (valid_host(host)
        && secret.len() == 64
        && secret
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
    .then_some((host, secret))
}

pub fn valid_host(host: &str) -> bool {
    let label = |s: &str| {
        !s.is_empty()
            && s.len() <= 63
            && !s.starts_with('-')
            && !s.ends_with('-')
            && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    };
    !host.is_empty() && host.len() <= 253 && host.split('.').all(label)
}
