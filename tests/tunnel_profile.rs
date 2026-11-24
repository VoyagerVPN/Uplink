use uplink::tunnel_profile::Profile;

#[test]
fn the_service_accepts_only_member_profiles_with_a_private_virtual_ip() {
    let mut grant = serde_json::json!({
        "id":"a1".repeat(16),"ip":"10.7.9.2","endpoint":"192.0.2.1:9993",
        "credential":"AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE="
    });
    let config = serde_json::from_value::<Profile>(grant.clone())
        .unwrap()
        .config()
        .unwrap();
    assert!(config.contains("dev_name = \"Uplink\""));
    assert!(config.contains("ipv4 = \"10.7.9.2/24\""));
    grant["ip"] = "8.8.8.8".into();
    assert!(
        serde_json::from_value::<Profile>(grant.clone())
            .unwrap()
            .config()
            .is_err()
    );
    grant["ip"] = "10.7.9.2".into();
    grant["shell_hook"] = "ignored?".into();
    assert!(serde_json::from_value::<Profile>(grant).is_err());
}
