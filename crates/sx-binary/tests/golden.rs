#[test]
fn golden_binary_fixture_is_stable() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden");
    let text = std::fs::read_to_string(root.join("basic_expected.sx")).unwrap();
    let value = sx_text::parse_sx_text(&text).unwrap();
    let encoded = sx_binary::encode_binary(&value, None, None).unwrap();
    let fixture = std::fs::read(root.join("basic_expected.sxb")).unwrap();
    assert_eq!(
        encoded, fixture,
        "golden binary changed; regenerate with: cargo run -p sx-cli -- convert tests/golden/basic_expected.sx --to binary --out tests/golden/basic_expected.sxb"
    );
    let decoded = sx_binary::decode_binary(&fixture).unwrap();
    assert_eq!(decoded, value);
}

#[test]
fn malformed_binary_rejected() {
    let malformed = vec![0, 1, 2, 3];
    assert!(sx_binary::decode_binary(&malformed).is_err());
}
