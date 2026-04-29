use base64::{engine::general_purpose::STANDARD, Engine as _};
use sx_core::{SxColumn, SxTypedArray, SxValue};

/// Formats an SX value using deterministic pretty rules.
pub fn format_value(value: &SxValue) -> String {
    fmt(value, 0)
}

/// Formats an SX value in canonical text form.
pub fn format_canonical(value: &SxValue) -> String {
    format_value(value)
}

fn fmt(v: &SxValue, indent: usize) -> String {
    match v {
        SxValue::Null => "null".to_string(),
        SxValue::Bool(b) => b.to_string(),
        SxValue::U8(n) => n.to_string(),
        SxValue::U16(n) => n.to_string(),
        SxValue::U32(n) => n.to_string(),
        SxValue::U64(n) => n.to_string(),
        SxValue::I8(n) => n.to_string(),
        SxValue::I16(n) => n.to_string(),
        SxValue::I32(n) => n.to_string(),
        SxValue::I64(n) => n.to_string(),
        SxValue::F32(n) => n.to_string(),
        SxValue::F64(n) => n.to_string(),
        SxValue::Decimal(d) => format!("decimal(\"{}\", scale: {})", d.scaled, d.scale),
        SxValue::Money(m) => format!(
            "money(\"{}\", {}, scale: {})",
            m.currency, m.scaled, m.scale
        ),
        SxValue::String(s) => format!("\"{}\"", escape(s)),
        SxValue::Bytes(b) => format!("bytes(\"base64:{}\")", STANDARD.encode(b)),
        SxValue::Enum(e) => format!("\"{}\"", escape(e)),
        SxValue::Uuid(u) => format!("uuid(\"{}\")", uuid::Uuid::from_bytes(*u)),
        SxValue::Timestamp(s) => format!("timestamp(\"{}\")", escape(s)),
        SxValue::Date(s) => format!("date(\"{}\")", escape(s)),
        SxValue::Duration(s) => format!("duration(\"{}\")", escape(s)),
        SxValue::Url(s) => format!("url(\"{}\")", escape(s)),
        SxValue::Email(s) => format!("email(\"{}\")", escape(s)),
        SxValue::Array(items) => {
            if items.is_empty() {
                return "[]".to_string();
            }
            let pad = " ".repeat(indent + 2);
            let mut out = String::from("[\n");
            for (i, item) in items.iter().enumerate() {
                out.push_str(&pad);
                out.push_str(&fmt(item, indent + 2));
                if i + 1 != items.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str(&" ".repeat(indent));
            out.push(']');
            out
        }
        SxValue::Object(map) => {
            if map.is_empty() {
                return "{}".to_string();
            }
            let pad = " ".repeat(indent + 2);
            let mut out = String::from("{\n");
            let len = map.len();
            for (idx, (k, v)) in map.iter().enumerate() {
                out.push_str(&pad);
                out.push_str(k);
                out.push_str(": ");
                out.push_str(&fmt(v, indent + 2));
                if idx + 1 != len {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str(&" ".repeat(indent));
            out.push('}');
            out
        }
        SxValue::TypedArray(t) => match t {
            SxTypedArray::U8(v) => format!("u8[{}]", join(v.iter().map(|x| x.to_string()))),
            SxTypedArray::I32(v) => format!("i32[{}]", join(v.iter().map(|x| x.to_string()))),
            SxTypedArray::F32(v) => format!("f32[{}]", join(v.iter().map(|x| x.to_string()))),
            SxTypedArray::F64(v) => format!("f64[{}]", join(v.iter().map(|x| x.to_string()))),
            SxTypedArray::Bool(v) => format!("bool[{}]", join(v.iter().map(|x| x.to_string()))),
        },
        SxValue::Table(table) => {
            let mut out = String::from("table {\n");
            let len = table.columns.len();
            for (idx, (k, c)) in table.columns.iter().enumerate() {
                out.push_str("  ");
                out.push_str(k);
                out.push_str(": ");
                match c {
                    SxColumn::Typed(t) => {
                        out.push_str(&fmt(&SxValue::TypedArray(t.clone()), indent + 2))
                    }
                    SxColumn::Values(v) => {
                        out.push_str(&fmt(&SxValue::Array(v.clone()), indent + 2))
                    }
                }
                if idx + 1 != len {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push('}');
            out
        }
        SxValue::Map(entries) => {
            let mut out = String::from("map {\n");
            for (k, v) in entries {
                out.push_str("  ");
                out.push_str(&fmt(k, indent + 2));
                out.push_str(": ");
                out.push_str(&fmt(v, indent + 2));
                out.push_str("\n");
            }
            out.push('}');
            out
        }
        SxValue::Tensor(t) => format!(
            "tensor<{}, {:?}> {}",
            format!("{:?}", t.data.element_type()).to_lowercase(),
            t.shape,
            fmt(&SxValue::TypedArray(t.data.clone()), indent)
        ),
        SxValue::Reference(r) => format!("ref(\"{}\")", escape(&r.target)),
        SxValue::BlobRef(b) => {
            let mut fields = Vec::new();
            fields.push(format!("uri: \"{}\"", escape(&b.uri)));
            if let Some(m) = &b.media_type {
                fields.push(format!("media_type: \"{}\"", escape(m)));
            }
            if let Some(s) = b.size {
                fields.push(format!("size: {}", s));
            }
            if let Some(h) = &b.hash {
                fields.push(format!("hash: \"{}\"", hex::encode(h)));
            }
            format!("blob_ref {{ {} }}", fields.join(", "))
        }
        SxValue::Delta(d) => {
            let mut out = String::new();
            out.push_str("delta");
            if let Some(h) = &d.from_hash {
                out.push_str(&format!(" from hash(\"{}\")", escape(h)));
            }
            out.push_str(" {\n");
            for op in &d.ops {
                out.push_str("  ");
                out.push_str(&format!("{:?}", op.kind).to_lowercase());
                out.push(' ');
                out.push_str(&op.path.to_string());
                if let Some(v) = &op.value {
                    out.push_str(" = ");
                    out.push_str(&fmt(v, indent + 2));
                }
                out.push('\n');
            }
            out.push('}');
            out
        }
        SxValue::Message(m) => {
            let payload = m
                .payload
                .as_ref()
                .map(|p| fmt(p, indent + 2))
                .unwrap_or_else(|| "null".to_string());
            format!(
                "message {{\n  sx_version: {},\n  payload: {}\n}}",
                m.sx_version, payload
            )
        }
    }
}

fn join<I>(iter: I) -> String
where
    I: Iterator<Item = String>,
{
    iter.collect::<Vec<_>>().join(", ")
}

fn escape(input: &str) -> String {
    input.replace('\\', "\\\\").replace('"', "\\\"")
}
