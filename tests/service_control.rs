#[test]
fn missing_service_is_reported_by_windows() {
    let error = uplink::service_control::set_running("Uplink.Test.Missing.Service", true)
        .expect_err("an absent service cannot accept a power command");
    assert_eq!(error.raw_os_error(), Some(1060));
}
