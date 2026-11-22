use uplink::{invite, settings};
#[test]
fn disabled_link_is_rejected_even_after_leaving() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("prefs.json");
    settings::join(&path, invite::INVITATION).unwrap();
    settings::network_action(&path, invite::INVITATION, "link", "").unwrap();
    settings::leave(&path, None).unwrap();
    assert!(settings::join(&path, invite::INVITATION).is_err());
}

#[test]
fn credentials_and_qr_are_independent_of_link_membership() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("prefs.json");
    let url = invite::INVITATION;
    settings::join(&path, url).unwrap();
    for action in ["link", "qr", "password-mode"] {
        settings::network_action(&path, url, action, "").unwrap();
    }
    settings::network_action(&path, url, "password", "test-pass").unwrap();
    settings::leave(&path, None).unwrap();
    assert!(settings::join_credentials(&path, "after-hours.example", "wrong").is_err());
    let title = "Я".repeat(64);
    assert!(
        settings::join_on_server(&path, "after-hours.example", "incorrect", "test-pass").is_err()
    );
    settings::join_on_server(
        &path,
        " AFTER-HOURS.EXAMPLE ",
        " После работы ",
        "test-pass",
    )
    .unwrap();
    settings::network_action(&path, url, "name", &title).unwrap();
    assert!(settings::network_action(&path, url, "name", &"Я".repeat(65)).is_err());
    assert!(settings::join(&path, url).is_err());
    settings::join(&path, &format!("{url}?via=qr")).unwrap();
    settings::network_action(&path, url, "qr", "").unwrap();
    assert!(settings::join(&path, &format!("{url}?via=qr")).is_err());
    assert_eq!(settings::rooms(&path).unwrap().len(), 1);
}

#[test]
fn transfer_revokes_owner_access_and_admins_cannot_change_roles() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("prefs.json");
    let url = invite::INVITATION;
    settings::join(&path, url).unwrap();
    let apply = |action, value| settings::network_action(&path, url, action, value);
    apply("role", "192.0.2.10").unwrap();
    apply("transfer", "192.0.2.10").unwrap();
    assert!(apply("name", "new").is_err());
    let mut prefs: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    prefs["network_settings"][url]["admins"] = serde_json::json!(["192.0.2.11", "192.0.2.12"]);
    std::fs::write(&path, serde_json::to_vec(&prefs).unwrap()).unwrap();
    apply("name", "new").unwrap();
    apply("qr", "").unwrap();
    apply("remove", "192.0.2.15").unwrap();
    assert!(apply("remove", "192.0.2.12").is_err());
    assert!(apply("role", "192.0.2.13").is_err());
    assert!(apply("transfer", "192.0.2.13").is_err());
    assert!(apply("remove", "192.0.2.10").is_err());
    let room = settings::rooms(&path).unwrap().remove(0);
    assert!(room.manages() && room.policy.qr);
    assert_eq!(room.name, "new");
    assert_eq!(room.policy.removed, ["192.0.2.15"]);
}
