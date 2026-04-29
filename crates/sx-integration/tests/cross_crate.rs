use std::collections::BTreeMap;
use sx_core::{DeltaDocument, DeltaOp, DeltaOpKind, SxPath, SxValue};

#[test]
fn text_binary_hash_delta_flow() {
    let text = "{name:\"Asha\",count:1,active:false}";
    let parsed = sx_text::parse_sx_text(text).unwrap();
    let bin = sx_binary::encode_binary(&parsed, None, None).unwrap();
    let decoded = sx_binary::decode_binary(&bin).unwrap();
    assert_eq!(parsed, decoded);

    let h1 = sx_crypto::logical_hash(&parsed).unwrap();
    let h2 = sx_crypto::logical_hash(&decoded).unwrap();
    assert_eq!(h1, h2);

    let delta = DeltaDocument {
        from_hash: None,
        ops: vec![DeltaOp {
            kind: DeltaOpKind::Increment,
            path: SxPath::parse("/count").unwrap(),
            value: Some(SxValue::I64(2)),
            from: None,
            index: None,
        }],
    };
    let patched = sx_delta::apply_delta(&decoded, &delta).unwrap();
    let SxValue::Object(map) = patched else {
        panic!("object expected")
    };
    assert_eq!(map.get("count"), Some(&SxValue::I64(3)));
}

#[test]
fn schema_validation_flow() {
    let schema = sx_schema::parse_schema(
        r#"
        schema User v1 {
          #1 id: uuid
          #2 name: string
          #3 active?: bool = true
        }
        "#,
    )
    .unwrap();

    let mut obj = BTreeMap::new();
    obj.insert(
        "id".to_string(),
        SxValue::Uuid(*uuid::Uuid::new_v4().as_bytes()),
    );
    obj.insert("name".to_string(), SxValue::String("Asha".to_string()));
    sx_schema::validate(&schema, &SxValue::Object(obj)).unwrap();
}
