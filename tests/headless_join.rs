use std::process::Command;

#[test]
fn headless_join_saves_the_network_without_starting_a_window() {
    let profile = tempfile::tempdir().unwrap();
    let ctl = |args: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_uplinkctl"))
            .args(args)
            .env("UPLINK_PREFS_DIR", profile.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
    };
    let joined = ctl(&["join", "--headless", "uplink://join/after-hours"]);
    assert_eq!(joined["data"]["window"], "stopped");
    let rooms = ctl(&["rooms"]);
    assert_eq!(
        rooms["data"]["rooms"][0]["url"],
        "uplink://join/after-hours"
    );
}

#[test]
fn headless_join_reports_https_failure_for_an_unreachable_server() {
    let profile = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_uplinkctl"))
        .args([
            "join",
            "--headless",
            &format!("uplink://127.0.0.1/invite/{}", "a".repeat(64)),
        ])
        .env("UPLINK_PREFS_DIR", profile.path())
        .env("ProgramData", profile.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert!(error["message"].is_string());
    let log = std::fs::read_to_string(profile.path().join("Uplink/last-join.log")).unwrap();
    assert!(log.contains("operation=http_bootstrap"));
}
