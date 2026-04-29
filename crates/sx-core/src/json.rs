use crate::delta_types::DeltaDocument;
use crate::error::{SxError, SxErrorCode, SxResult};
use crate::table::{SxColumn, SxTable};
use crate::typed_array::SxTypedArray;
use crate::value::{BlobRef, DecimalValue, MoneyValue, ReferenceValue, SxValue};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{Map, Number, Value};
use std::collections::BTreeMap;

/// Converts SX value to JSON using tagged objects for non-native values.
pub fn sx_to_json(value: &SxValue) -> Value {
    match value {
        SxValue::Null => Value::Null,
        SxValue::Bool(v) => Value::Bool(*v),
        SxValue::U8(v) => Value::Number(Number::from(*v)),
        SxValue::U16(v) => Value::Number(Number::from(*v)),
        SxValue::U32(v) => Value::Number(Number::from(*v)),
        SxValue::U64(v) => Value::Number(Number::from(*v)),
        SxValue::I8(v) => Value::Number(Number::from(*v)),
        SxValue::I16(v) => Value::Number(Number::from(*v)),
        SxValue::I32(v) => Value::Number(Number::from(*v)),
        SxValue::I64(v) => Value::Number(Number::from(*v)),
        SxValue::F32(v) => Number::from_f64(*v as f64).map_or(Value::Null, Value::Number),
        SxValue::F64(v) => Number::from_f64(*v).map_or(Value::Null, Value::Number),
        SxValue::String(s) | SxValue::Enum(s) | SxValue::Url(s) | SxValue::Email(s) => {
            Value::String(s.clone())
        }
        SxValue::Bytes(bytes) => tagged("bytes", Value::String(STANDARD.encode(bytes))),
        SxValue::Array(arr) => Value::Array(arr.iter().map(sx_to_json).collect()),
        SxValue::Object(map) => {
            let mut out = Map::new();
            for (k, v) in map {
                out.insert(k.clone(), sx_to_json(v));
            }
            Value::Object(out)
        }
        SxValue::Map(entries) => tagged(
            "map",
            Value::Array(
                entries
                    .iter()
                    .map(|(k, v)| Value::Array(vec![sx_to_json(k), sx_to_json(v)]))
                    .collect(),
            ),
        ),
        SxValue::Uuid(bytes) => tagged(
            "uuid",
            Value::String(uuid::Uuid::from_bytes(*bytes).to_string()),
        ),
        SxValue::Timestamp(ts) => tagged("timestamp", Value::String(ts.clone())),
        SxValue::Date(d) => tagged("date", Value::String(d.clone())),
        SxValue::Duration(d) => tagged("duration", Value::String(d.clone())),
        SxValue::Decimal(DecimalValue { scaled, scale }) => {
            let mut obj = Map::new();
            obj.insert("scaled".to_string(), Value::String(scaled.to_string()));
            obj.insert("scale".to_string(), Value::Number(Number::from(*scale)));
            tagged("decimal", Value::Object(obj))
        }
        SxValue::Money(MoneyValue {
            currency,
            scaled,
            scale,
        }) => {
            let mut obj = Map::new();
            obj.insert("currency".to_string(), Value::String(currency.clone()));
            obj.insert("scaled".to_string(), Value::Number(Number::from(*scaled)));
            obj.insert("scale".to_string(), Value::Number(Number::from(*scale)));
            tagged("money", Value::Object(obj))
        }
        SxValue::TypedArray(a) => {
            let (ty, val) = match a {
                SxTypedArray::U8(v) => (
                    "typed_array:u8",
                    Value::Array(v.iter().map(|x| Value::Number(Number::from(*x))).collect()),
                ),
                SxTypedArray::I32(v) => (
                    "typed_array:i32",
                    Value::Array(v.iter().map(|x| Value::Number(Number::from(*x))).collect()),
                ),
                SxTypedArray::F32(v) => (
                    "typed_array:f32",
                    Value::Array(
                        v.iter()
                            .map(|x| Number::from_f64(*x as f64).map_or(Value::Null, Value::Number))
                            .collect(),
                    ),
                ),
                SxTypedArray::F64(v) => (
                    "typed_array:f64",
                    Value::Array(
                        v.iter()
                            .map(|x| Number::from_f64(*x).map_or(Value::Null, Value::Number))
                            .collect(),
                    ),
                ),
                SxTypedArray::Bool(v) => (
                    "typed_array:bool",
                    Value::Array(v.iter().map(|x| Value::Bool(*x)).collect()),
                ),
            };
            tagged(ty, val)
        }
        SxValue::Table(table) => {
            let mut obj = Map::new();
            for (k, c) in &table.columns {
                let col = match c {
                    SxColumn::Typed(t) => sx_to_json(&SxValue::TypedArray(t.clone())),
                    SxColumn::Values(v) => Value::Array(v.iter().map(sx_to_json).collect()),
                };
                obj.insert(k.clone(), col);
            }
            tagged("table", Value::Object(obj))
        }
        SxValue::Tensor(t) => {
            let mut obj = Map::new();
            obj.insert(
                "shape".to_string(),
                Value::Array(
                    t.shape
                        .iter()
                        .map(|x| Value::Number(Number::from(*x)))
                        .collect(),
                ),
            );
            obj.insert(
                "data".to_string(),
                sx_to_json(&SxValue::TypedArray(t.data.clone())),
            );
            if let Some(layout) = &t.layout {
                obj.insert("layout".to_string(), Value::String(layout.clone()));
            }
            tagged("tensor", Value::Object(obj))
        }
        SxValue::Reference(ReferenceValue { target }) => {
            tagged("ref", Value::String(target.clone()))
        }
        SxValue::BlobRef(BlobRef {
            uri,
            media_type,
            size,
            hash,
        }) => {
            let mut obj = Map::new();
            obj.insert("uri".to_string(), Value::String(uri.clone()));
            if let Some(m) = media_type {
                obj.insert("media_type".to_string(), Value::String(m.clone()));
            }
            if let Some(s) = size {
                obj.insert("size".to_string(), Value::Number(Number::from(*s)));
            }
            if let Some(h) = hash {
                obj.insert("hash".to_string(), Value::String(hex::encode(h)));
            }
            tagged("blob_ref", Value::Object(obj))
        }
        SxValue::Delta(DeltaDocument { from_hash, ops }) => {
            let mut obj = Map::new();
            if let Some(h) = from_hash {
                obj.insert("from_hash".to_string(), Value::String(h.clone()));
            }
            let list = ops
                .iter()
                .map(|op| {
                    let mut m = Map::new();
                    m.insert(
                        "kind".to_string(),
                        Value::String(format!("{:?}", op.kind).to_lowercase()),
                    );
                    m.insert("path".to_string(), Value::String(op.path.to_string()));
                    if let Some(v) = &op.value {
                        m.insert("value".to_string(), sx_to_json(v));
                    }
                    if let Some(f) = &op.from {
                        m.insert("from".to_string(), Value::String(f.to_string()));
                    }
                    if let Some(idx) = op.index {
                        m.insert("index".to_string(), Value::Number(Number::from(idx)));
                    }
                    Value::Object(m)
                })
                .collect();
            obj.insert("ops".to_string(), Value::Array(list));
            tagged("delta", Value::Object(obj))
        }
        SxValue::Message(msg) => {
            let mut obj = Map::new();
            obj.insert(
                "sx_version".to_string(),
                Value::Number(Number::from(msg.sx_version)),
            );
            if let Some(v) = &msg.message_id {
                obj.insert("message_id".to_string(), Value::String(v.clone()));
            }
            if let Some(v) = &msg.message_type {
                obj.insert("type".to_string(), Value::String(v.clone()));
            }
            if let Some(v) = &msg.schema {
                obj.insert("schema".to_string(), Value::String(v.clone()));
            }
            if let Some(v) = &msg.timestamp {
                obj.insert("timestamp".to_string(), Value::String(v.clone()));
            }
            if let Some(v) = &msg.logical_hash {
                obj.insert("logical_hash".to_string(), Value::String(hex::encode(v)));
            }
            let mut fields = Map::new();
            for (k, v) in &msg.fields {
                fields.insert(k.clone(), sx_to_json(v));
            }
            obj.insert("fields".to_string(), Value::Object(fields));
            if let Some(payload) = &msg.payload {
                obj.insert("payload".to_string(), sx_to_json(payload));
            }
            tagged("message", Value::Object(obj))
        }
    }
}

fn tagged(tag: &str, value: Value) -> Value {
    let mut out = Map::new();
    out.insert("$type".to_string(), Value::String(tag.to_string()));
    out.insert("$value".to_string(), value);
    Value::Object(out)
}

/// Converts JSON to SX value, supporting tagged-object convention.
pub fn json_to_sx(value: &Value) -> SxResult<SxValue> {
    match value {
        Value::Null => Ok(SxValue::Null),
        Value::Bool(b) => Ok(SxValue::Bool(*b)),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(SxValue::I64(i))
            } else if let Some(u) = n.as_u64() {
                Ok(SxValue::U64(u))
            } else if let Some(f) = n.as_f64() {
                Ok(SxValue::F64(f))
            } else {
                Err(SxError::new(
                    SxErrorCode::InvalidNumber,
                    "unsupported JSON number",
                ))
            }
        }
        Value::String(s) => Ok(SxValue::String(s.clone())),
        Value::Array(arr) => {
            let mut out = Vec::with_capacity(arr.len());
            for item in arr {
                out.push(json_to_sx(item)?);
            }
            Ok(SxValue::Array(out))
        }
        Value::Object(obj) => {
            if let (Some(Value::String(t)), Some(v)) = (obj.get("$type"), obj.get("$value")) {
                return parse_tagged(t, v);
            }
            let mut out = BTreeMap::new();
            for (k, v) in obj {
                if out.contains_key(k) {
                    return Err(SxError::new(
                        SxErrorCode::DuplicateKey,
                        format!("duplicate key '{k}'"),
                    ));
                }
                out.insert(k.clone(), json_to_sx(v)?);
            }
            Ok(SxValue::Object(out))
        }
    }
}

fn parse_tagged(tag: &str, value: &Value) -> SxResult<SxValue> {
    match tag {
        "uuid" => {
            let s = value
                .as_str()
                .ok_or_else(|| SxError::new(SxErrorCode::ParseError, "uuid must be string"))?;
            let id = uuid::Uuid::parse_str(s)
                .map_err(|e| SxError::new(SxErrorCode::ParseError, format!("invalid uuid: {e}")))?;
            Ok(SxValue::Uuid(*id.as_bytes()))
        }
        "timestamp" => Ok(SxValue::Timestamp(
            value.as_str().unwrap_or_default().to_string(),
        )),
        "date" => Ok(SxValue::Date(
            value.as_str().unwrap_or_default().to_string(),
        )),
        "duration" => Ok(SxValue::Duration(
            value.as_str().unwrap_or_default().to_string(),
        )),
        "bytes" => {
            let s = value.as_str().ok_or_else(|| {
                SxError::new(SxErrorCode::ParseError, "bytes must be base64 string")
            })?;
            let bytes = STANDARD.decode(s).map_err(|e| {
                SxError::new(
                    SxErrorCode::ParseError,
                    format!("invalid base64 bytes: {e}"),
                )
            })?;
            Ok(SxValue::Bytes(bytes))
        }
        "decimal" => {
            let obj = value
                .as_object()
                .ok_or_else(|| SxError::new(SxErrorCode::ParseError, "decimal must be object"))?;
            let scaled = obj
                .get("scaled")
                .and_then(Value::as_str)
                .ok_or_else(|| SxError::new(SxErrorCode::ParseError, "decimal.scaled missing"))?
                .parse::<i128>()
                .map_err(|e| {
                    SxError::new(
                        SxErrorCode::ParseError,
                        format!("invalid decimal scaled: {e}"),
                    )
                })?;
            let scale = obj
                .get("scale")
                .and_then(Value::as_u64)
                .ok_or_else(|| SxError::new(SxErrorCode::ParseError, "decimal.scale missing"))?
                as u32;
            Ok(SxValue::Decimal(DecimalValue { scaled, scale }))
        }
        "money" => {
            let obj = value
                .as_object()
                .ok_or_else(|| SxError::new(SxErrorCode::ParseError, "money must be object"))?;
            let currency = obj
                .get("currency")
                .and_then(Value::as_str)
                .ok_or_else(|| SxError::new(SxErrorCode::ParseError, "money.currency missing"))?
                .to_string();
            let scaled = obj
                .get("scaled")
                .and_then(Value::as_i64)
                .ok_or_else(|| SxError::new(SxErrorCode::ParseError, "money.scaled missing"))?;
            let scale = obj
                .get("scale")
                .and_then(Value::as_u64)
                .ok_or_else(|| SxError::new(SxErrorCode::ParseError, "money.scale missing"))?
                as u32;
            Ok(SxValue::Money(MoneyValue {
                currency,
                scaled,
                scale,
            }))
        }
        other if other.starts_with("typed_array:") => {
            let arr = value.as_array().ok_or_else(|| {
                SxError::new(SxErrorCode::ParseError, "typed array value must be array")
            })?;
            let out = match other {
                "typed_array:u8" => {
                    SxTypedArray::U8(arr.iter().map(|x| x.as_u64().unwrap_or(0) as u8).collect())
                }
                "typed_array:i32" => {
                    SxTypedArray::I32(arr.iter().map(|x| x.as_i64().unwrap_or(0) as i32).collect())
                }
                "typed_array:f32" => SxTypedArray::F32(
                    arr.iter()
                        .map(|x| x.as_f64().unwrap_or(0.0) as f32)
                        .collect(),
                ),
                "typed_array:f64" => {
                    SxTypedArray::F64(arr.iter().map(|x| x.as_f64().unwrap_or(0.0)).collect())
                }
                "typed_array:bool" => {
                    SxTypedArray::Bool(arr.iter().map(|x| x.as_bool().unwrap_or(false)).collect())
                }
                _ => {
                    return Err(SxError::new(
                        SxErrorCode::UnsupportedFeature,
                        format!("unsupported typed array tag: {other}"),
                    ))
                }
            };
            Ok(SxValue::TypedArray(out))
        }
        "ref" => Ok(SxValue::Reference(ReferenceValue {
            target: value.as_str().unwrap_or_default().to_string(),
        })),
        "blob_ref" => {
            let obj = value
                .as_object()
                .ok_or_else(|| SxError::new(SxErrorCode::ParseError, "blob_ref must be object"))?;
            let uri = obj
                .get("uri")
                .and_then(Value::as_str)
                .ok_or_else(|| SxError::new(SxErrorCode::ParseError, "blob_ref.uri missing"))?
                .to_string();
            Ok(SxValue::BlobRef(BlobRef {
                uri,
                media_type: obj
                    .get("media_type")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                size: obj.get("size").and_then(Value::as_u64),
                hash: obj
                    .get("hash")
                    .and_then(Value::as_str)
                    .map(|h| hex::decode(h).unwrap_or_default()),
            }))
        }
        "table" => {
            let obj = value
                .as_object()
                .ok_or_else(|| SxError::new(SxErrorCode::ParseError, "table must be object"))?;
            let mut cols = BTreeMap::new();
            for (k, v) in obj {
                let parsed = json_to_sx(v)?;
                let col = match parsed {
                    SxValue::TypedArray(t) => SxColumn::Typed(t),
                    SxValue::Array(items) => SxColumn::Values(items),
                    _ => {
                        return Err(SxError::new(
                            SxErrorCode::TypeMismatch,
                            format!("table column '{k}' must be typed array or array"),
                        ))
                    }
                };
                cols.insert(k.clone(), col);
            }
            Ok(SxValue::Table(SxTable { columns: cols }))
        }
        "map" => {
            let arr = value
                .as_array()
                .ok_or_else(|| SxError::new(SxErrorCode::ParseError, "map value must be array"))?;
            let mut out = Vec::new();
            for item in arr {
                let pair = item.as_array().ok_or_else(|| {
                    SxError::new(SxErrorCode::ParseError, "map entry must be 2-item array")
                })?;
                if pair.len() != 2 {
                    return Err(SxError::new(
                        SxErrorCode::ParseError,
                        "map entry must have exactly 2 elements",
                    ));
                }
                out.push((json_to_sx(&pair[0])?, json_to_sx(&pair[1])?));
            }
            Ok(SxValue::Map(out))
        }
        "tensor" => {
            let obj = value
                .as_object()
                .ok_or_else(|| SxError::new(SxErrorCode::ParseError, "tensor must be object"))?;
            let shape = obj
                .get("shape")
                .and_then(Value::as_array)
                .ok_or_else(|| SxError::new(SxErrorCode::ParseError, "tensor.shape missing"))?
                .iter()
                .map(|x| x.as_u64().unwrap_or(0) as usize)
                .collect::<Vec<_>>();
            let data_value = obj
                .get("data")
                .ok_or_else(|| SxError::new(SxErrorCode::ParseError, "tensor.data missing"))?;
            let data = match json_to_sx(data_value)? {
                SxValue::TypedArray(t) => t,
                _ => {
                    return Err(SxError::new(
                        SxErrorCode::TypeMismatch,
                        "tensor data must be typed array",
                    ))
                }
            };
            Ok(SxValue::Tensor(crate::SxTensor {
                shape,
                data,
                layout: obj
                    .get("layout")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            }))
        }
        "delta" => {
            let obj = value
                .as_object()
                .ok_or_else(|| SxError::new(SxErrorCode::ParseError, "delta must be object"))?;
            let from_hash = obj
                .get("from_hash")
                .and_then(Value::as_str)
                .map(str::to_string);
            let mut ops = Vec::new();
            for item in obj
                .get("ops")
                .and_then(Value::as_array)
                .ok_or_else(|| SxError::new(SxErrorCode::ParseError, "delta.ops missing"))?
            {
                let m = item.as_object().ok_or_else(|| {
                    SxError::new(SxErrorCode::ParseError, "delta op must be object")
                })?;
                let kind_str = m.get("kind").and_then(Value::as_str).ok_or_else(|| {
                    SxError::new(SxErrorCode::ParseError, "delta op kind missing")
                })?;
                let kind = match kind_str {
                    "set" => crate::DeltaOpKind::Set,
                    "replace" => crate::DeltaOpKind::Replace,
                    "remove" => crate::DeltaOpKind::Remove,
                    "append" => crate::DeltaOpKind::Append,
                    "prepend" => crate::DeltaOpKind::Prepend,
                    "insert" => crate::DeltaOpKind::Insert,
                    "increment" => crate::DeltaOpKind::Increment,
                    "decrement" => crate::DeltaOpKind::Decrement,
                    "merge" => crate::DeltaOpKind::Merge,
                    "move" => crate::DeltaOpKind::Move,
                    "copy" => crate::DeltaOpKind::Copy,
                    "clear" => crate::DeltaOpKind::Clear,
                    _ => {
                        return Err(SxError::new(
                            SxErrorCode::ParseError,
                            format!("unknown delta op '{kind_str}'"),
                        ))
                    }
                };
                let path =
                    crate::SxPath::parse(m.get("path").and_then(Value::as_str).ok_or_else(
                        || SxError::new(SxErrorCode::ParseError, "delta op path missing"),
                    )?)?;
                let from = m
                    .get("from")
                    .and_then(Value::as_str)
                    .map(crate::SxPath::parse)
                    .transpose()?;
                let value = m.get("value").map(json_to_sx).transpose()?;
                let index = m.get("index").and_then(Value::as_u64).map(|x| x as usize);
                ops.push(crate::DeltaOp {
                    kind,
                    path,
                    value,
                    from,
                    index,
                });
            }
            Ok(SxValue::Delta(DeltaDocument { from_hash, ops }))
        }
        "message" => {
            let obj = value
                .as_object()
                .ok_or_else(|| SxError::new(SxErrorCode::ParseError, "message must be object"))?;
            let mut fields = BTreeMap::new();
            if let Some(fobj) = obj.get("fields").and_then(Value::as_object) {
                for (k, v) in fobj {
                    fields.insert(k.clone(), json_to_sx(v)?);
                }
            }
            let payload = obj
                .get("payload")
                .map(json_to_sx)
                .transpose()?
                .map(Box::new);
            Ok(SxValue::Message(crate::MessageEnvelope {
                sx_version: obj.get("sx_version").and_then(Value::as_u64).unwrap_or(1) as u32,
                message_id: obj
                    .get("message_id")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                message_type: obj.get("type").and_then(Value::as_str).map(str::to_string),
                schema: obj
                    .get("schema")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                timestamp: obj
                    .get("timestamp")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                logical_hash: obj
                    .get("logical_hash")
                    .and_then(Value::as_str)
                    .map(|s| hex::decode(s).unwrap_or_default()),
                fields,
                payload,
            }))
        }
        _ => Err(SxError::new(
            SxErrorCode::UnsupportedFeature,
            format!("unknown tagged type '{tag}'"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tagged_roundtrip_uuid() {
        let v = SxValue::Uuid(*uuid::Uuid::new_v4().as_bytes());
        let j = sx_to_json(&v);
        let back = json_to_sx(&j).unwrap();
        assert_eq!(v, back);
    }
}
