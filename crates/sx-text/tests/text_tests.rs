#[test]
fn parse_comments_trailing_unquoted() {
    let input = r#"
    {
      // comment
      name: "Asha",
      active: true,
    }
    "#;
    let v = sx_text::parse_sx_text(input).unwrap();
    let formatted = sx_text::format_value(&v);
    assert!(formatted.contains("name"));
}

#[test]
fn parse_table_and_message() {
    let t = sx_text::parse_sx_text("table T { temp: f32[1.0, 2.0], active: bool[true, false] }")
        .unwrap();
    assert!(matches!(t, sx_core::SxValue::Table(_)));
    let m = sx_text::parse_sx_text("message M { a: 1 }").unwrap();
    assert!(matches!(m, sx_core::SxValue::Message(_)));
}
