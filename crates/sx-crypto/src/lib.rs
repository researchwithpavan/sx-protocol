//! Canonical semantic hashing for SX values.

use sha2::{Digest, Sha256};
use sx_core::{DecimalValue, SxError, SxErrorCode, SxResult, SxValue};

/// Computes canonical SHA-256 logical hash for SX value.
pub fn logical_hash(value: &SxValue) -> SxResult<[u8; 32]> {
    let canonical = canonicalize(value)?;
    let digest = Sha256::digest(canonical.as_bytes());
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    Ok(out)
}

/// Verifies two values have identical logical hash.
pub fn logically_equal(a: &SxValue, b: &SxValue) -> SxResult<bool> {
    Ok(logical_hash(a)? == logical_hash(b)?)
}

/// Signature API surface, currently unsupported in v1.
pub fn sign_value(_value: &SxValue, _algorithm: &str, _private_key: &[u8]) -> SxResult<Vec<u8>> {
    Err(SxError::new(
        SxErrorCode::UnsupportedFeature,
        "signature algorithms are unsupported in v1; hash-only mode implemented",
    ))
}

/// Signature verify API surface, currently unsupported in v1.
pub fn verify_signature(
    _value: &SxValue,
    _algorithm: &str,
    _public_key: &[u8],
    _signature: &[u8],
) -> SxResult<bool> {
    Err(SxError::new(
        SxErrorCode::UnsupportedFeature,
        "signature algorithms are unsupported in v1; hash-only mode implemented",
    ))
}

fn canonicalize(value: &SxValue) -> SxResult<String> {
    match value {
        SxValue::Null => Ok("null".to_string()),
        SxValue::Bool(v) => Ok(if *v { "true" } else { "false" }.to_string()),
        SxValue::U8(v) => Ok(format!("u:{v}")),
        SxValue::U16(v) => Ok(format!("u:{v}")),
        SxValue::U32(v) => Ok(format!("u:{v}")),
        SxValue::U64(v) => Ok(format!("u:{v}")),
        SxValue::I8(v) => Ok(format!("i:{v}")),
        SxValue::I16(v) => Ok(format!("i:{v}")),
        SxValue::I32(v) => Ok(format!("i:{v}")),
        SxValue::I64(v) => Ok(format!("i:{v}")),
        SxValue::F32(v) => Ok(format!("f:{}", normalize_float(*v as f64))),
        SxValue::F64(v) => Ok(format!("f:{}", normalize_float(*v))),
        SxValue::Decimal(DecimalValue { scaled, scale }) => {
            let (norm_scaled, norm_scale) = normalize_decimal(*scaled, *scale);
            Ok(format!("d:{norm_scaled}@{norm_scale}"))
        }
        SxValue::Money(m) => {
            let (norm_scaled, norm_scale) = normalize_decimal(m.scaled as i128, m.scale);
            Ok(format!("money:{}:{norm_scaled}@{norm_scale}", m.currency))
        }
        SxValue::String(s) => Ok(format!("s:{}", escape(s))),
        SxValue::Bytes(b) => Ok(format!("b:{}", hex::encode(b))),
        SxValue::Array(items) => {
            let mut out = String::from("[");
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&canonicalize(item)?);
            }
            out.push(']');
            Ok(out)
        }
        SxValue::Object(map) => {
            let mut out = String::from("{");
            for (i, (k, v)) in map.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&escape(k));
                out.push(':');
                out.push_str(&canonicalize(v)?);
            }
            out.push('}');
            Ok(out)
        }
        SxValue::Map(entries) => {
            let mut pairs = Vec::new();
            for (k, v) in entries {
                pairs.push((canonicalize(k)?, canonicalize(v)?));
            }
            pairs.sort_by(|a, b| a.0.cmp(&b.0));
            let mut out = String::from("map{");
            for (i, (k, v)) in pairs.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(k);
                out.push(':');
                out.push_str(v);
            }
            out.push('}');
            Ok(out)
        }
        SxValue::Enum(s) => Ok(format!("enum:{}", escape(s))),
        SxValue::Uuid(v) => Ok(format!("uuid:{}", uuid::Uuid::from_bytes(*v))),
        SxValue::Timestamp(ts) => Ok(format!("ts:{}", normalize_timestamp(ts)?)),
        SxValue::Date(s) => Ok(format!("date:{}", s)),
        SxValue::Duration(s) => Ok(format!("dur:{}", s)),
        SxValue::Url(s) => Ok(format!("url:{}", s)),
        SxValue::Email(s) => Ok(format!("email:{}", s.to_lowercase())),
        SxValue::TypedArray(t) => Ok(format!("typed:{t:?}")),
        SxValue::Table(t) => Ok(format!("table:{t:?}")),
        SxValue::Tensor(t) => Ok(format!("tensor:{t:?}")),
        SxValue::Reference(r) => Ok(format!("ref:{}", r.target)),
        SxValue::BlobRef(b) => Ok(format!("blob:{}:{:?}:{:?}", b.uri, b.size, b.hash)),
        SxValue::Delta(d) => Ok(format!("delta:{d:?}")),
        SxValue::Message(m) => canonicalize(&SxValue::Object(m.fields.clone())),
    }
}

fn normalize_float(v: f64) -> String {
    if v == 0.0 {
        return "0".to_string();
    }
    let s = format!("{v:.15}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

fn normalize_decimal(mut scaled: i128, mut scale: u32) -> (i128, u32) {
    while scale > 0 && scaled % 10 == 0 {
        scaled /= 10;
        scale -= 1;
    }
    (scaled, scale)
}

fn normalize_timestamp(input: &str) -> SxResult<String> {
    let parsed = chrono::DateTime::parse_from_rfc3339(input).map_err(|e| {
        SxError::new(
            SxErrorCode::ValidationError,
            format!("invalid timestamp '{input}': {e}"),
        )
    })?;
    Ok(parsed
        .to_utc()
        .to_rfc3339_opts(chrono::SecondsFormat::Nanos, true))
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace(':', "\\:")
        .replace(',', "\\,")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn same_logical_hash_for_object_ordering() {
        let mut a = BTreeMap::new();
        a.insert("id".to_string(), SxValue::I64(1));
        a.insert("name".to_string(), SxValue::String("Asha".to_string()));

        let mut b = BTreeMap::new();
        b.insert("name".to_string(), SxValue::String("Asha".to_string()));
        b.insert("id".to_string(), SxValue::I64(1));

        assert!(logically_equal(&SxValue::Object(a), &SxValue::Object(b)).unwrap());
    }

    #[test]
    fn different_logical_data_different_hash() {
        let a = SxValue::String("a".to_string());
        let b = SxValue::String("b".to_string());
        assert_ne!(logical_hash(&a).unwrap(), logical_hash(&b).unwrap());
    }
}
