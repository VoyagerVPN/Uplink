use uplink::updates::newer_stable;

#[test]
fn only_offers_newer_stable_windows_versions() {
    assert!(newer_stable("0.1.2", "v0.1.10"));
    for tag in [
        "v0.1.2",
        "v0.1.1",
        "v0.2.0-beta.1",
        "../setup",
        "v0.1.02",
        "v70000.0.0",
    ] {
        assert!(!newer_stable("0.1.2", tag), "{tag}");
    }
}
