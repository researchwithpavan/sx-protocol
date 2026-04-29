use sx_core::{json, SxErrorCode, SxValue};

#[test]
fn duplicate_keys_rejected() {
    let err = SxValue::object_from_pairs(vec![
        ("a".to_string(), SxValue::I64(1)),
        ("a".to_string(), SxValue::I64(2)),
    ])
    .unwrap_err();
    assert_eq!(err.code, SxErrorCode::DuplicateKey);
}

#[test]
fn json_tagged_bytes_roundtrip() {
    let value = SxValue::Bytes(vec![1, 2, 3]);
    let j = json::sx_to_json(&value);
    let back = json::json_to_sx(&j).unwrap();
    assert_eq!(value, back);
}
