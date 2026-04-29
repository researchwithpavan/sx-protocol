//! SX schema parser, validator, and compatibility tools.

use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use sx_core::{SxError, SxErrorCode, SxPath, SxResult, SxTypedArrayType, SxValue};

/// Supported schema type references.
#[derive(Debug, Clone, PartialEq)]
pub enum SchemaType {
    Primitive(String),
    Array(Box<SchemaType>),
    TypedArray(SxTypedArrayType),
    Object(String),
    Enum(Vec<String>),
    Table(BTreeMap<String, SchemaType>),
}

/// One schema field definition.
#[derive(Debug, Clone, PartialEq)]
pub struct SchemaField {
    pub id: u32,
    pub name: String,
    pub ty: SchemaType,
    pub optional: bool,
    pub default: Option<SxValue>,
    pub hints: BTreeSet<String>,
    pub renamed_from: Option<String>,
}

/// Parsed schema document.
#[derive(Debug, Clone, PartialEq)]
pub struct Schema {
    pub name: String,
    pub version: u32,
    pub fields: Vec<SchemaField>,
}

/// Compatibility result with migration plan.
#[derive(Debug, Clone, PartialEq)]
pub struct CompatibilityReport {
    pub compatible: bool,
    pub steps: Vec<MigrationStep>,
}

/// Migration steps accepted by zero-copy-compatible path.
#[derive(Debug, Clone, PartialEq)]
pub enum MigrationStep {
    CopyField {
        id: u32,
        name: String,
    },
    RenameField {
        from: String,
        to: String,
    },
    AddOptionalField {
        id: u32,
        name: String,
    },
    AddDefaultField {
        id: u32,
        name: String,
        default: SxValue,
    },
    IncompatibleChange {
        reason: String,
    },
}

/// Parses schema text.
pub fn parse_schema(input: &str) -> SxResult<Schema> {
    let normalized = input.replace("\r\n", "\n");
    let mut lines = normalized.lines().map(str::trim).filter(|l| !l.is_empty());

    let header = lines
        .next()
        .ok_or_else(|| SxError::new(SxErrorCode::ParseError, "schema text is empty"))?;
    if !header.starts_with("schema ") {
        return Err(SxError::new(
            SxErrorCode::ParseError,
            "schema must start with 'schema'",
        ));
    }

    let header_clean = header.trim_end_matches('{').trim();
    let parts: Vec<&str> = header_clean.split_whitespace().collect();
    if parts.len() < 2 {
        return Err(SxError::new(
            SxErrorCode::ParseError,
            "schema header missing name",
        ));
    }
    let name = parts[1].to_string();
    let version = if parts.len() >= 3 && parts[2].starts_with('v') {
        parts[2][1..].parse::<u32>().unwrap_or(1)
    } else {
        1
    };

    let mut fields = Vec::new();
    for line in lines {
        if line == "}" {
            break;
        }
        let line = line.trim_end_matches(',').trim();
        let (hints, rest) = parse_hints(line);
        let field = parse_field_line(rest, hints)?;
        fields.push(field);
    }

    if fields.is_empty() {
        return Err(SxError::new(
            SxErrorCode::ParseError,
            "schema has no fields",
        ));
    }

    Ok(Schema {
        name,
        version,
        fields,
    })
}

fn parse_hints(line: &str) -> (BTreeSet<String>, &str) {
    let mut rest = line;
    let mut hints = BTreeSet::new();
    loop {
        let r = rest.trim_start();
        if let Some(stripped) = r.strip_prefix('@') {
            let mut parts = stripped.splitn(2, char::is_whitespace);
            let hint = parts.next().unwrap_or_default().to_string();
            hints.insert(hint);
            rest = parts.next().unwrap_or_default();
        } else {
            rest = r;
            break;
        }
    }
    (hints, rest)
}

fn parse_field_line(line: &str, hints: BTreeSet<String>) -> SxResult<SchemaField> {
    let line = line.trim();
    if !line.starts_with('#') {
        return Err(SxError::new(
            SxErrorCode::ParseError,
            format!("field line must start with '#': {line}"),
        ));
    }
    let mut after_id = &line[1..];
    let id_end = after_id
        .find(char::is_whitespace)
        .ok_or_else(|| SxError::new(SxErrorCode::ParseError, "field id missing"))?;
    let id = after_id[..id_end]
        .parse::<u32>()
        .map_err(|e| SxError::new(SxErrorCode::ParseError, format!("invalid field id: {e}")))?;
    after_id = after_id[id_end..].trim();

    let (name_part, right) = after_id
        .split_once(':')
        .ok_or_else(|| SxError::new(SxErrorCode::ParseError, "field missing ':'"))?;
    let optional = name_part.trim_end().ends_with('?');
    let name = name_part.trim_end_matches('?').trim().to_string();

    let (ty_text, default_text) = if let Some((l, r)) = right.split_once('=') {
        (l.trim(), Some(r.trim()))
    } else {
        (right.trim(), None)
    };

    let ty = parse_type(ty_text)?;
    let default = default_text
        .map(|text| sx_text::parse_sx_text(text))
        .transpose()?;

    Ok(SchemaField {
        id,
        name,
        ty,
        optional,
        default,
        hints,
        renamed_from: None,
    })
}

fn parse_type(ty: &str) -> SxResult<SchemaType> {
    let ty = ty.trim();
    if let Some(inner) = ty.strip_prefix("typed<").and_then(|x| x.strip_suffix('>')) {
        let t = match inner {
            "u8" => SxTypedArrayType::U8,
            "i32" => SxTypedArrayType::I32,
            "f32" => SxTypedArrayType::F32,
            "f64" => SxTypedArrayType::F64,
            "bool" => SxTypedArrayType::Bool,
            _ => {
                return Err(SxError::new(
                    SxErrorCode::ParseError,
                    format!("unsupported typed array element '{inner}'"),
                ))
            }
        };
        return Ok(SchemaType::TypedArray(t));
    }
    if let Some(inner) = ty.strip_suffix("[]") {
        return Ok(SchemaType::Array(Box::new(parse_type(inner)?)));
    }
    if let Some(inner) = ty.strip_prefix("enum[").and_then(|x| x.strip_suffix(']')) {
        let vals = inner
            .split(',')
            .map(|s| s.trim().trim_matches('"').to_string())
            .collect();
        return Ok(SchemaType::Enum(vals));
    }
    Ok(match ty {
        "null" | "bool" | "u16" | "u32" | "u64" | "i8" | "i16" | "i64" | "string" | "bytes"
        | "uuid" | "timestamp" | "date" | "duration" | "decimal" | "money" | "object" | "table" => {
            SchemaType::Primitive(ty.to_string())
        }
        other => SchemaType::Object(other.to_string()),
    })
}

/// Deterministic schema hash (SHA-256 over canonical schema text).
pub fn schema_hash(schema: &Schema) -> [u8; 32] {
    let mut lines = Vec::new();
    lines.push(format!("schema {} v{}", schema.name, schema.version));
    let mut fields = schema.fields.clone();
    fields.sort_by_key(|f| f.id);
    for f in fields {
        let mut hints = f.hints.iter().cloned().collect::<Vec<_>>();
        hints.sort();
        lines.push(format!(
            "#{} {}{}: {:?} default:{:?} hints:{}",
            f.id,
            f.name,
            if f.optional { "?" } else { "" },
            f.ty,
            f.default,
            hints.join("|")
        ));
    }
    let canon = lines.join("\n");
    let digest = Sha256::digest(canon.as_bytes());
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

/// Validates value against schema.
pub fn validate(schema: &Schema, value: &SxValue) -> SxResult<()> {
    let root = match value {
        SxValue::Object(obj) => obj,
        _ => {
            return Err(SxError::new(
                SxErrorCode::TypeMismatch,
                "root value must be object for schema validation",
            ))
        }
    };

    for field in &schema.fields {
        match root.get(&field.name) {
            Some(v) => validate_type(&field.ty, v, &SxPath::root().key(&field.name))?,
            None if field.optional || field.default.is_some() => {}
            None => {
                return Err(SxError::new(
                    SxErrorCode::RequiredFieldMissing,
                    format!("missing required field '{}'", field.name),
                )
                .with_path(SxPath::root().key(&field.name)))
            }
        }
    }
    Ok(())
}

fn validate_type(ty: &SchemaType, value: &SxValue, path: &SxPath) -> SxResult<()> {
    match ty {
        SchemaType::Primitive(p) => {
            let ok = match p.as_str() {
                "null" => matches!(value, SxValue::Null),
                "bool" => matches!(value, SxValue::Bool(_)),
                "u8" => {
                    matches!(value, SxValue::U8(_))
                        || matches!(value, SxValue::U64(v) if *v <= u8::MAX as u64)
                }
                "u16" => {
                    matches!(value, SxValue::U16(_))
                        || matches!(value, SxValue::U64(v) if *v <= u16::MAX as u64)
                }
                "u32" => {
                    matches!(value, SxValue::U32(_))
                        || matches!(value, SxValue::U64(v) if *v <= u32::MAX as u64)
                }
                "u64" => matches!(value, SxValue::U64(_)),
                "i8" => {
                    matches!(value, SxValue::I8(_))
                        || matches!(value, SxValue::I64(v) if *v >= i8::MIN as i64 && *v <= i8::MAX as i64)
                }
                "i16" => {
                    matches!(value, SxValue::I16(_))
                        || matches!(value, SxValue::I64(v) if *v >= i16::MIN as i64 && *v <= i16::MAX as i64)
                }
                "i32" => {
                    matches!(value, SxValue::I32(_))
                        || matches!(value, SxValue::I64(v) if *v >= i32::MIN as i64 && *v <= i32::MAX as i64)
                }
                "i64" => matches!(value, SxValue::I64(_)),
                "string" => matches!(value, SxValue::String(_)),
                "bytes" => matches!(value, SxValue::Bytes(_)),
                "uuid" => matches!(value, SxValue::Uuid(_)),
                "timestamp" => matches!(value, SxValue::Timestamp(_)),
                "date" => matches!(value, SxValue::Date(_)),
                "duration" => matches!(value, SxValue::Duration(_)),
                "decimal" => matches!(value, SxValue::Decimal(_)),
                "money" => matches!(value, SxValue::Money(_)),
                "object" => matches!(value, SxValue::Object(_)),
                "table" => matches!(value, SxValue::Table(_)),
                _ => true,
            };
            if ok {
                Ok(())
            } else {
                Err(SxError::new(
                    SxErrorCode::TypeMismatch,
                    format!("type mismatch at {} expected {}", path, p),
                )
                .with_path(path.clone()))
            }
        }
        SchemaType::Array(inner) => {
            let SxValue::Array(items) = value else {
                return Err(SxError::new(SxErrorCode::TypeMismatch, "expected array")
                    .with_path(path.clone()));
            };
            for (i, item) in items.iter().enumerate() {
                validate_type(inner, item, &path.index(i))?;
            }
            Ok(())
        }
        SchemaType::TypedArray(t) => {
            let ok = matches!(
                (t, value),
                (
                    SxTypedArrayType::U8,
                    SxValue::TypedArray(sx_core::SxTypedArray::U8(_))
                ) | (
                    SxTypedArrayType::I32,
                    SxValue::TypedArray(sx_core::SxTypedArray::I32(_))
                ) | (
                    SxTypedArrayType::F32,
                    SxValue::TypedArray(sx_core::SxTypedArray::F32(_))
                ) | (
                    SxTypedArrayType::F64,
                    SxValue::TypedArray(sx_core::SxTypedArray::F64(_))
                ) | (
                    SxTypedArrayType::Bool,
                    SxValue::TypedArray(sx_core::SxTypedArray::Bool(_))
                )
            );
            if ok {
                Ok(())
            } else {
                Err(
                    SxError::new(SxErrorCode::TypeMismatch, "typed-array mismatch")
                        .with_path(path.clone()),
                )
            }
        }
        SchemaType::Object(_) => {
            if matches!(value, SxValue::Object(_)) {
                Ok(())
            } else {
                Err(SxError::new(SxErrorCode::TypeMismatch, "expected object")
                    .with_path(path.clone()))
            }
        }
        SchemaType::Enum(vals) => {
            let ok = match value {
                SxValue::Enum(s) | SxValue::String(s) => vals.contains(s),
                _ => false,
            };
            if ok {
                Ok(())
            } else {
                Err(
                    SxError::new(SxErrorCode::ValidationError, "enum value out of range")
                        .with_path(path.clone()),
                )
            }
        }
        SchemaType::Table(_) => {
            if matches!(value, SxValue::Table(_)) {
                Ok(())
            } else {
                Err(SxError::new(SxErrorCode::TypeMismatch, "expected table")
                    .with_path(path.clone()))
            }
        }
    }
}

/// Computes compatibility report and migration plan between schema versions.
pub fn check_compatibility(old: &Schema, new: &Schema) -> CompatibilityReport {
    let mut steps = Vec::new();
    let mut compatible = true;

    let old_by_id: BTreeMap<u32, &SchemaField> = old.fields.iter().map(|f| (f.id, f)).collect();
    let new_by_id: BTreeMap<u32, &SchemaField> = new.fields.iter().map(|f| (f.id, f)).collect();

    for (id, oldf) in &old_by_id {
        if let Some(newf) = new_by_id.get(id) {
            if oldf.ty == newf.ty {
                if oldf.name == newf.name {
                    steps.push(MigrationStep::CopyField {
                        id: *id,
                        name: newf.name.clone(),
                    });
                } else {
                    steps.push(MigrationStep::RenameField {
                        from: oldf.name.clone(),
                        to: newf.name.clone(),
                    });
                }
            } else {
                compatible = false;
                steps.push(MigrationStep::IncompatibleChange {
                    reason: format!("field {} type changed", id),
                });
            }
        } else {
            compatible = false;
            steps.push(MigrationStep::IncompatibleChange {
                reason: format!("field {} removed", id),
            });
        }
    }

    for (id, newf) in &new_by_id {
        if !old_by_id.contains_key(id) {
            if newf.optional {
                steps.push(MigrationStep::AddOptionalField {
                    id: *id,
                    name: newf.name.clone(),
                });
            } else if let Some(default) = &newf.default {
                steps.push(MigrationStep::AddDefaultField {
                    id: *id,
                    name: newf.name.clone(),
                    default: default.clone(),
                });
            } else {
                compatible = false;
                steps.push(MigrationStep::IncompatibleChange {
                    reason: format!("new required field {} without default", id),
                });
            }
        }
    }

    CompatibilityReport { compatible, steps }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn parse_and_validate() {
        let schema = parse_schema(
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
        validate(&schema, &SxValue::Object(obj)).unwrap();
    }

    #[test]
    fn compatibility_report() {
        let old = parse_schema(
            r#"
            schema U v1 {
              #1 id: uuid
              #2 name: string
            }
            "#,
        )
        .unwrap();
        let new = parse_schema(
            r#"
            schema U v2 {
              #1 id: uuid
              #2 full_name: string
              #3 active?: bool
            }
            "#,
        )
        .unwrap();
        let report = check_compatibility(&old, &new);
        assert!(report.compatible);
        assert!(report
            .steps
            .iter()
            .any(|s| matches!(s, MigrationStep::RenameField { .. })));
    }

    #[test]
    fn validation_rejects_wrong_type() {
        let schema = parse_schema(
            r#"
            schema U v1 {
              #1 id: uuid
              #2 age: i64
            }
            "#,
        )
        .unwrap();
        let mut obj = BTreeMap::new();
        obj.insert(
            "id".to_string(),
            SxValue::Uuid(*uuid::Uuid::new_v4().as_bytes()),
        );
        obj.insert("age".to_string(), SxValue::String("x".to_string()));
        let err = validate(&schema, &SxValue::Object(obj)).unwrap_err();
        assert_eq!(err.code, SxErrorCode::TypeMismatch);
    }
}
