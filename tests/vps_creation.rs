use serde_json::json;
use uplink::vps::Creation;

#[test]
fn ssh_port_accepts_a_pasted_five_digit_value() {
    use uplink::vps::field_error;
    assert_eq!(field_error("port", "22141\r\n"), "");
    for invalid in ["22\n141", "0", "65536"] {
        assert!(!field_error("port", invalid).is_empty());
    }
}

#[test]
fn validates_fields_and_accepts_first_use_without_a_manual_fingerprint() {
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
        ("fingerprint", "bad"),
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
    v.as_object_mut().unwrap().remove("fingerprint");
    assert!(serde_json::from_value::<Creation>(v.clone()).unwrap().validate().is_err());
    v["trust_on_first_use"] = true.into();
    assert!(serde_json::from_value::<Creation>(v).unwrap().validate().is_ok());
}
