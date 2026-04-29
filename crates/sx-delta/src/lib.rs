//! SX delta application and diff.

use sx_core::{
    DeltaDocument, DeltaOp, DeltaOpKind, SxError, SxErrorCode, SxPath, SxPathSegment, SxResult,
    SxValue,
};

/// Applies a delta document to base value.
pub fn apply_delta(base: &SxValue, delta: &DeltaDocument) -> SxResult<SxValue> {
    let mut current = base.clone();
    for op in &delta.ops {
        apply_op(&mut current, op)?;
    }
    Ok(current)
}

/// Computes deterministic diff for common changes.
pub fn diff(base: &SxValue, target: &SxValue) -> DeltaDocument {
    let mut ops = Vec::new();
    diff_at(&SxPath::root(), base, target, &mut ops);
    DeltaDocument {
        from_hash: None,
        ops,
    }
}

fn diff_at(path: &SxPath, base: &SxValue, target: &SxValue, ops: &mut Vec<DeltaOp>) {
    match (base, target) {
        (SxValue::Object(a), SxValue::Object(b)) => {
            for key in a.keys() {
                if !b.contains_key(key) {
                    ops.push(DeltaOp {
                        kind: DeltaOpKind::Remove,
                        path: path.key(key),
                        value: None,
                        from: None,
                        index: None,
                    });
                }
            }
            for (k, v2) in b {
                let p = path.key(k);
                if let Some(v1) = a.get(k) {
                    diff_at(&p, v1, v2, ops);
                } else {
                    ops.push(DeltaOp {
                        kind: DeltaOpKind::Set,
                        path: p,
                        value: Some(v2.clone()),
                        from: None,
                        index: None,
                    });
                }
            }
        }
        (SxValue::Array(a), SxValue::Array(b)) => {
            let min_len = a.len().min(b.len());
            for i in 0..min_len {
                diff_at(&path.index(i), &a[i], &b[i], ops);
            }
            if b.len() > a.len() {
                for item in b.iter().skip(a.len()) {
                    ops.push(DeltaOp {
                        kind: DeltaOpKind::Append,
                        path: path.clone(),
                        value: Some(item.clone()),
                        from: None,
                        index: None,
                    });
                }
            } else if a.len() > b.len() {
                for i in (b.len()..a.len()).rev() {
                    ops.push(DeltaOp {
                        kind: DeltaOpKind::Remove,
                        path: path.index(i),
                        value: None,
                        from: None,
                        index: None,
                    });
                }
            }
        }
        (SxValue::I64(x), SxValue::I64(y)) if y > x => {
            ops.push(DeltaOp {
                kind: DeltaOpKind::Increment,
                path: path.clone(),
                value: Some(SxValue::I64(y - x)),
                from: None,
                index: None,
            });
        }
        (SxValue::I64(x), SxValue::I64(y)) if y < x => {
            ops.push(DeltaOp {
                kind: DeltaOpKind::Decrement,
                path: path.clone(),
                value: Some(SxValue::I64(x - y)),
                from: None,
                index: None,
            });
        }
        _ if base != target => {
            ops.push(DeltaOp {
                kind: DeltaOpKind::Replace,
                path: path.clone(),
                value: Some(target.clone()),
                from: None,
                index: None,
            });
        }
        _ => {}
    }
}

fn apply_op(root: &mut SxValue, op: &DeltaOp) -> SxResult<()> {
    match op.kind {
        DeltaOpKind::Set | DeltaOpKind::Replace => {
            let value = op.value.clone().ok_or_else(|| {
                SxError::new(SxErrorCode::ValidationError, "set/replace requires value")
            })?;
            set_path(root, &op.path, value)
        }
        DeltaOpKind::Remove => remove_path(root, &op.path),
        DeltaOpKind::Append => {
            let value = op.value.clone().ok_or_else(|| {
                SxError::new(SxErrorCode::ValidationError, "append requires value")
            })?;
            let target = get_mut(root, &op.path)?;
            match target {
                SxValue::Array(arr) => {
                    arr.push(value);
                    Ok(())
                }
                _ => Err(SxError::new(
                    SxErrorCode::TypeMismatch,
                    "append target must be array",
                )),
            }
        }
        DeltaOpKind::Prepend => {
            let value = op.value.clone().ok_or_else(|| {
                SxError::new(SxErrorCode::ValidationError, "prepend requires value")
            })?;
            let target = get_mut(root, &op.path)?;
            match target {
                SxValue::Array(arr) => {
                    arr.insert(0, value);
                    Ok(())
                }
                _ => Err(SxError::new(
                    SxErrorCode::TypeMismatch,
                    "prepend target must be array",
                )),
            }
        }
        DeltaOpKind::Insert => {
            let value = op.value.clone().ok_or_else(|| {
                SxError::new(SxErrorCode::ValidationError, "insert requires value")
            })?;
            let idx = op.index.ok_or_else(|| {
                SxError::new(SxErrorCode::ValidationError, "insert requires index")
            })?;
            let target = get_mut(root, &op.path)?;
            match target {
                SxValue::Array(arr) => {
                    if idx > arr.len() {
                        return Err(SxError::new(
                            SxErrorCode::InvalidPath,
                            "insert index out of range",
                        ));
                    }
                    arr.insert(idx, value);
                    Ok(())
                }
                _ => Err(SxError::new(
                    SxErrorCode::TypeMismatch,
                    "insert target must be array",
                )),
            }
        }
        DeltaOpKind::Increment | DeltaOpKind::Decrement => {
            let by = op.value.as_ref().and_then(extract_i64).ok_or_else(|| {
                SxError::new(
                    SxErrorCode::TypeMismatch,
                    "increment/decrement expects integer value",
                )
            })?;
            let sign = if op.kind == DeltaOpKind::Increment {
                1
            } else {
                -1
            };
            let target = get_mut(root, &op.path)?;
            match target {
                SxValue::I64(v) => {
                    *v += sign * by;
                    Ok(())
                }
                SxValue::U64(v) => {
                    let next = (*v as i128) + (sign as i128) * (by as i128);
                    if next < 0 {
                        return Err(SxError::new(
                            SxErrorCode::ValidationError,
                            "u64 underflow in delta op",
                        ));
                    }
                    *v = next as u64;
                    Ok(())
                }
                _ => Err(SxError::new(
                    SxErrorCode::TypeMismatch,
                    "increment/decrement target must be integer",
                )),
            }
        }
        DeltaOpKind::Merge => {
            let value = op.value.clone().ok_or_else(|| {
                SxError::new(SxErrorCode::ValidationError, "merge requires value")
            })?;
            let target = get_mut(root, &op.path)?;
            match (target, value) {
                (SxValue::Object(dst), SxValue::Object(src)) => {
                    for (k, v) in src {
                        dst.insert(k, v);
                    }
                    Ok(())
                }
                _ => Err(SxError::new(
                    SxErrorCode::TypeMismatch,
                    "merge requires object target and value",
                )),
            }
        }
        DeltaOpKind::Move => {
            let from = op
                .from
                .as_ref()
                .ok_or_else(|| SxError::new(SxErrorCode::ValidationError, "move requires from"))?;
            let val = get(root, from)?.clone();
            remove_path(root, from)?;
            set_path(root, &op.path, val)
        }
        DeltaOpKind::Copy => {
            let from = op
                .from
                .as_ref()
                .ok_or_else(|| SxError::new(SxErrorCode::ValidationError, "copy requires from"))?;
            let val = get(root, from)?.clone();
            set_path(root, &op.path, val)
        }
        DeltaOpKind::Clear => {
            let target = get_mut(root, &op.path)?;
            match target {
                SxValue::Array(arr) => arr.clear(),
                SxValue::Object(map) => map.clear(),
                SxValue::Bytes(b) => b.clear(),
                SxValue::String(s) => s.clear(),
                _ => {
                    return Err(SxError::new(
                        SxErrorCode::TypeMismatch,
                        "clear supports array/object/bytes/string",
                    ))
                }
            }
            Ok(())
        }
    }
}

fn extract_i64(v: &SxValue) -> Option<i64> {
    match v {
        SxValue::I64(n) => Some(*n),
        SxValue::U64(n) => i64::try_from(*n).ok(),
        _ => None,
    }
}

fn get<'a>(root: &'a SxValue, path: &SxPath) -> SxResult<&'a SxValue> {
    let mut cur = root;
    for seg in &path.segments {
        cur = match (cur, seg) {
            (SxValue::Object(map), SxPathSegment::Key(k)) => map.get(k).ok_or_else(|| {
                SxError::new(SxErrorCode::InvalidPath, format!("missing key '{k}'"))
            })?,
            (SxValue::Array(arr), SxPathSegment::Index(i)) => arr.get(*i).ok_or_else(|| {
                SxError::new(SxErrorCode::InvalidPath, format!("missing index {i}"))
            })?,
            _ => {
                return Err(SxError::new(
                    SxErrorCode::InvalidPath,
                    "path segment does not match value type",
                ))
            }
        };
    }
    Ok(cur)
}

fn get_mut<'a>(root: &'a mut SxValue, path: &SxPath) -> SxResult<&'a mut SxValue> {
    let mut cur = root;
    for seg in &path.segments {
        cur = match (cur, seg) {
            (SxValue::Object(map), SxPathSegment::Key(k)) => map.get_mut(k).ok_or_else(|| {
                SxError::new(SxErrorCode::InvalidPath, format!("missing key '{k}'"))
            })?,
            (SxValue::Array(arr), SxPathSegment::Index(i)) => arr.get_mut(*i).ok_or_else(|| {
                SxError::new(SxErrorCode::InvalidPath, format!("missing index {i}"))
            })?,
            _ => {
                return Err(SxError::new(
                    SxErrorCode::InvalidPath,
                    "path segment does not match value type",
                ))
            }
        };
    }
    Ok(cur)
}

fn set_path(root: &mut SxValue, path: &SxPath, value: SxValue) -> SxResult<()> {
    if path.segments.is_empty() {
        *root = value;
        return Ok(());
    }

    let (parent, last) = split_parent(path)?;
    let parent = get_mut(root, &parent)?;
    match (parent, last) {
        (SxValue::Object(map), SxPathSegment::Key(k)) => {
            map.insert(k, value);
            Ok(())
        }
        (SxValue::Array(arr), SxPathSegment::Index(i)) => {
            if i < arr.len() {
                arr[i] = value;
            } else if i == arr.len() {
                arr.push(value);
            } else {
                return Err(SxError::new(
                    SxErrorCode::InvalidPath,
                    "array set index out of range",
                ));
            }
            Ok(())
        }
        _ => Err(SxError::new(
            SxErrorCode::InvalidPath,
            "cannot set path on non-container",
        )),
    }
}

fn remove_path(root: &mut SxValue, path: &SxPath) -> SxResult<()> {
    if path.segments.is_empty() {
        *root = SxValue::Null;
        return Ok(());
    }
    let (parent, last) = split_parent(path)?;
    let parent = get_mut(root, &parent)?;
    match (parent, last) {
        (SxValue::Object(map), SxPathSegment::Key(k)) => {
            map.remove(&k);
            Ok(())
        }
        (SxValue::Array(arr), SxPathSegment::Index(i)) => {
            if i >= arr.len() {
                return Err(SxError::new(
                    SxErrorCode::InvalidPath,
                    "array remove index out of range",
                ));
            }
            arr.remove(i);
            Ok(())
        }
        _ => Err(SxError::new(
            SxErrorCode::InvalidPath,
            "cannot remove path on non-container",
        )),
    }
}

fn split_parent(path: &SxPath) -> SxResult<(SxPath, SxPathSegment)> {
    let mut parent = path.clone();
    let last = parent
        .segments
        .pop()
        .ok_or_else(|| SxError::new(SxErrorCode::InvalidPath, "empty path"))?;
    Ok((parent, last))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn apply_basic_set() {
        let mut base_obj = BTreeMap::new();
        base_obj.insert("count".to_string(), SxValue::I64(1));
        let base = SxValue::Object(base_obj);
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
        let out = apply_delta(&base, &delta).unwrap();
        let SxValue::Object(map) = out else {
            panic!("object")
        };
        assert_eq!(map.get("count"), Some(&SxValue::I64(3)));
    }

    #[test]
    fn diff_replace() {
        let a = SxValue::I64(1);
        let b = SxValue::I64(9);
        let d = diff(&a, &b);
        assert_eq!(d.ops.len(), 1);
        assert_eq!(d.ops[0].kind, DeltaOpKind::Increment);
    }

    #[test]
    fn all_delta_ops_smoke() {
        let mut base_obj = BTreeMap::new();
        base_obj.insert("n".to_string(), SxValue::I64(10));
        base_obj.insert(
            "arr".to_string(),
            SxValue::Array(vec![SxValue::I64(1), SxValue::I64(2)]),
        );
        base_obj.insert("obj".to_string(), SxValue::Object(BTreeMap::new()));
        let base = SxValue::Object(base_obj);

        let delta = DeltaDocument {
            from_hash: None,
            ops: vec![
                DeltaOp {
                    kind: DeltaOpKind::Set,
                    path: SxPath::parse("/x").unwrap(),
                    value: Some(SxValue::I64(1)),
                    from: None,
                    index: None,
                },
                DeltaOp {
                    kind: DeltaOpKind::Replace,
                    path: SxPath::parse("/x").unwrap(),
                    value: Some(SxValue::I64(2)),
                    from: None,
                    index: None,
                },
                DeltaOp {
                    kind: DeltaOpKind::Append,
                    path: SxPath::parse("/arr").unwrap(),
                    value: Some(SxValue::I64(3)),
                    from: None,
                    index: None,
                },
                DeltaOp {
                    kind: DeltaOpKind::Prepend,
                    path: SxPath::parse("/arr").unwrap(),
                    value: Some(SxValue::I64(0)),
                    from: None,
                    index: None,
                },
                DeltaOp {
                    kind: DeltaOpKind::Insert,
                    path: SxPath::parse("/arr").unwrap(),
                    value: Some(SxValue::I64(9)),
                    from: None,
                    index: Some(2),
                },
                DeltaOp {
                    kind: DeltaOpKind::Increment,
                    path: SxPath::parse("/n").unwrap(),
                    value: Some(SxValue::I64(2)),
                    from: None,
                    index: None,
                },
                DeltaOp {
                    kind: DeltaOpKind::Decrement,
                    path: SxPath::parse("/n").unwrap(),
                    value: Some(SxValue::I64(1)),
                    from: None,
                    index: None,
                },
                DeltaOp {
                    kind: DeltaOpKind::Merge,
                    path: SxPath::parse("/obj").unwrap(),
                    value: Some(SxValue::Object(BTreeMap::from([(
                        "a".to_string(),
                        SxValue::Bool(true),
                    )]))),
                    from: None,
                    index: None,
                },
                DeltaOp {
                    kind: DeltaOpKind::Copy,
                    path: SxPath::parse("/copied").unwrap(),
                    value: None,
                    from: Some(SxPath::parse("/x").unwrap()),
                    index: None,
                },
                DeltaOp {
                    kind: DeltaOpKind::Move,
                    path: SxPath::parse("/moved").unwrap(),
                    value: None,
                    from: Some(SxPath::parse("/copied").unwrap()),
                    index: None,
                },
                DeltaOp {
                    kind: DeltaOpKind::Clear,
                    path: SxPath::parse("/obj").unwrap(),
                    value: None,
                    from: None,
                    index: None,
                },
                DeltaOp {
                    kind: DeltaOpKind::Remove,
                    path: SxPath::parse("/x").unwrap(),
                    value: None,
                    from: None,
                    index: None,
                },
            ],
        };

        let out = apply_delta(&base, &delta).unwrap();
        let SxValue::Object(map) = out else {
            panic!("object")
        };
        assert_eq!(map.get("n"), Some(&SxValue::I64(11)));
        assert!(map.get("x").is_none());
        assert!(matches!(map.get("moved"), Some(SxValue::I64(2))));
        assert!(matches!(map.get("obj"), Some(SxValue::Object(o)) if o.is_empty()));
    }
}
