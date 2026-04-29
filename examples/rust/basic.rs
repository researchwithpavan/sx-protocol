fn main() -> Result<(), Box<dyn std::error::Error>> {
    let text = "{name:\"Asha\",active:true}";
    let value = sx_text::parse_sx_text(text)?;
    let binary = sx_binary::encode_binary(&value, None, None)?;
    let decoded = sx_binary::decode_binary(&binary)?;
    println!("{}", sx_text::format_canonical(&decoded));
    Ok(())
}
