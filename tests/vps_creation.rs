use serde_json::json;
use uplink::vps::Creation;

#[test]
fn requires_host_identity_and_validates_fields_before_opening_ssh() {
    let mut v = json!({"name":"Friday","ip":"192.0.2.10","port":22,"user":"root","password":"test",
                       "fingerprint":"SHA256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"});
    assert!(
        serde_json::from_value::<Creation>(v.clone())
            .unwrap()
            .validate()
            .is_ok()
    );
    for (field, invalid) in [
        ("name", " "),
        ("ip", "one.example"),
        ("user", "-F"),
        ("fingerprint", ""),
        ("password", ""),
    ] {
        let old = v[field].take();
        v[field] = invalid.into();
        assert!(
            serde_json::from_value::<Creation>(v.clone())
                .unwrap()
                .validate()
                .is_err(),
            "{field}"
        );
        v[field] = old;
    }
}
