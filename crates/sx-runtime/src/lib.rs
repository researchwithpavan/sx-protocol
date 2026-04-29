//! SX runtime access model with lazy message views.

use sx_core::{SxColumn, SxError, SxErrorCode, SxPath, SxPathSegment, SxResult, SxTable, SxValue};

/// Lazy message view over SX binary bytes.
pub struct SxMessageView<'a> {
    bytes: &'a [u8],
}

/// Object view.
pub struct SxObjectView<'a> {
    object: &'a std::collections::BTreeMap<String, SxValue>,
}

/// Array view.
pub struct SxArrayView<'a> {
    array: &'a [SxValue],
}

/// String view.
pub struct SxStringView<'a> {
    value: &'a str,
}

/// Typed array view.
pub struct SxTypedArrayView<'a> {
    value: &'a sx_core::SxTypedArray,
}

/// Table view.
pub struct SxTableView<'a> {
    table: &'a SxTable,
}

impl<'a> SxMessageView<'a> {
    /// Creates view from encoded SX binary bytes.
    pub fn from_binary(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }

    /// Decodes full value lazily when requested.
    pub fn materialize(&self) -> SxResult<SxValue> {
        sx_binary::decode_binary(self.bytes)
    }

    /// Reads top-level field by name (hot path helper).
    pub fn hot_field(&self, name: &str) -> SxResult<Option<SxValue>> {
        let value = self.materialize()?;
        Ok(match value {
            SxValue::Object(map) => map.get(name).cloned(),
            SxValue::Message(msg) => msg.fields.get(name).cloned(),
            _ => None,
        })
    }

    /// Projects selected paths into an object.
    pub fn project(&self, paths: &[SxPath]) -> SxResult<SxValue> {
        let value = self.materialize()?;
        let mut out = std::collections::BTreeMap::new();
        for path in paths {
            if let Some(v) = get_at_path(&value, path) {
                out.insert(path.to_string(), v.clone());
            }
        }
        Ok(SxValue::Object(out))
    }
}

impl<'a> SxObjectView<'a> {
    pub fn new(value: &'a SxValue) -> Option<Self> {
        if let SxValue::Object(obj) = value {
            Some(Self { object: obj })
        } else {
            None
        }
    }

    pub fn get(&self, key: &str) -> Option<&'a SxValue> {
        self.object.get(key)
    }
}

impl<'a> SxArrayView<'a> {
    pub fn new(value: &'a SxValue) -> Option<Self> {
        if let SxValue::Array(array) = value {
            Some(Self { array })
        } else {
            None
        }
    }

    pub fn len(&self) -> usize {
        self.array.len()
    }

    pub fn get(&self, idx: usize) -> Option<&'a SxValue> {
        self.array.get(idx)
    }
}

impl<'a> SxStringView<'a> {
    pub fn new(value: &'a SxValue) -> Option<Self> {
        match value {
            SxValue::String(s) => Some(Self { value: s }),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'a str {
        self.value
    }
}

impl<'a> SxTypedArrayView<'a> {
    pub fn new(value: &'a SxValue) -> Option<Self> {
        match value {
            SxValue::TypedArray(v) => Some(Self { value: v }),
            _ => None,
        }
    }

    pub fn value(&self) -> &'a sx_core::SxTypedArray {
        self.value
    }
}

impl<'a> SxTableView<'a> {
    pub fn new(value: &'a SxValue) -> Option<Self> {
        match value {
            SxValue::Table(t) => Some(Self { table: t }),
            _ => None,
        }
    }

    /// Selects a subset of columns.
    pub fn select_columns(&self, names: &[&str]) -> SxTable {
        let mut out = std::collections::BTreeMap::new();
        for name in names {
            if let Some(col) = self.table.columns.get(*name) {
                out.insert((*name).to_string(), col.clone());
            }
        }
        SxTable { columns: out }
    }

    /// Filters rows by equality on a column.
    pub fn filter_eq(&self, column: &str, value: &SxValue) -> SxResult<SxTable> {
        let mask = self.row_filter(column, |v| v == value)?;
        Ok(apply_row_mask(self.table, &mask))
    }

    /// Filters rows by numeric comparison (>, >=, <, <=).
    pub fn filter_numeric(&self, column: &str, op: NumericOp, rhs: f64) -> SxResult<SxTable> {
        let mask = self.row_filter(column, |v| match as_f64(v) {
            Some(x) => op.apply(x, rhs),
            None => false,
        })?;
        Ok(apply_row_mask(self.table, &mask))
    }

    /// Filters rows by boolean value.
    pub fn filter_bool(&self, column: &str, wanted: bool) -> SxResult<SxTable> {
        let mask = self.row_filter(column, |v| matches!(v, SxValue::Bool(b) if *b == wanted))?;
        Ok(apply_row_mask(self.table, &mask))
    }

    fn row_filter<F>(&self, column: &str, pred: F) -> SxResult<Vec<bool>>
    where
        F: Fn(&SxValue) -> bool,
    {
        let col = self.table.columns.get(column).ok_or_else(|| {
            SxError::new(
                SxErrorCode::InvalidPath,
                format!("unknown column '{column}'"),
            )
        })?;
        let rows = self.table.row_count();
        let mut mask = vec![false; rows];
        for i in 0..rows {
            let value = column_value_owned(col, i)?;
            if pred(&value) {
                mask[i] = true;
            }
        }
        Ok(mask)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum NumericOp {
    Gt,
    Ge,
    Lt,
    Le,
    Eq,
}

impl NumericOp {
    fn apply(self, lhs: f64, rhs: f64) -> bool {
        match self {
            Self::Gt => lhs > rhs,
            Self::Ge => lhs >= rhs,
            Self::Lt => lhs < rhs,
            Self::Le => lhs <= rhs,
            Self::Eq => (lhs - rhs).abs() <= f64::EPSILON,
        }
    }
}

fn as_f64(value: &SxValue) -> Option<f64> {
    match value {
        SxValue::I64(v) => Some(*v as f64),
        SxValue::U64(v) => Some(*v as f64),
        SxValue::F32(v) => Some(*v as f64),
        SxValue::F64(v) => Some(*v),
        _ => None,
    }
}

fn column_value_owned(col: &SxColumn, idx: usize) -> SxResult<SxValue> {
    match col {
        SxColumn::Values(v) => v.get(idx).cloned().ok_or_else(|| {
            SxError::new(SxErrorCode::InvalidLength, "column row index out of bounds")
        }),
        SxColumn::Typed(sx_core::SxTypedArray::U8(v)) => {
            v.get(idx).copied().map(SxValue::U8).ok_or_else(|| {
                SxError::new(SxErrorCode::InvalidLength, "column row index out of bounds")
            })
        }
        SxColumn::Typed(sx_core::SxTypedArray::I32(v)) => {
            v.get(idx).copied().map(SxValue::I32).ok_or_else(|| {
                SxError::new(SxErrorCode::InvalidLength, "column row index out of bounds")
            })
        }
        SxColumn::Typed(sx_core::SxTypedArray::F32(v)) => {
            v.get(idx).copied().map(SxValue::F32).ok_or_else(|| {
                SxError::new(SxErrorCode::InvalidLength, "column row index out of bounds")
            })
        }
        SxColumn::Typed(sx_core::SxTypedArray::F64(v)) => {
            v.get(idx).copied().map(SxValue::F64).ok_or_else(|| {
                SxError::new(SxErrorCode::InvalidLength, "column row index out of bounds")
            })
        }
        SxColumn::Typed(sx_core::SxTypedArray::Bool(v)) => {
            v.get(idx).copied().map(SxValue::Bool).ok_or_else(|| {
                SxError::new(SxErrorCode::InvalidLength, "column row index out of bounds")
            })
        }
    }
}

fn apply_row_mask(table: &SxTable, mask: &[bool]) -> SxTable {
    let mut columns = std::collections::BTreeMap::new();
    for (name, col) in &table.columns {
        let filtered = match col {
            SxColumn::Values(v) => {
                let mut out = Vec::new();
                for (i, item) in v.iter().enumerate() {
                    if mask.get(i).copied().unwrap_or(false) {
                        out.push(item.clone());
                    }
                }
                SxColumn::Values(out)
            }
            SxColumn::Typed(t) => SxColumn::Typed(t.clone()),
        };
        columns.insert(name.clone(), filtered);
    }
    SxTable { columns }
}

fn get_at_path<'a>(root: &'a SxValue, path: &SxPath) -> Option<&'a SxValue> {
    let mut cur = root;
    for seg in &path.segments {
        cur = match (cur, seg) {
            (SxValue::Object(obj), SxPathSegment::Key(k)) => obj.get(k)?,
            (SxValue::Array(arr), SxPathSegment::Index(i)) => arr.get(*i)?,
            _ => return None,
        };
    }
    Some(cur)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn view_materialize_and_hot() {
        let mut obj = BTreeMap::new();
        obj.insert("tenant".to_string(), SxValue::String("acme".to_string()));
        let value = SxValue::Object(obj);
        let bin = sx_binary::encode_binary(&value, None, None).unwrap();
        let view = SxMessageView::from_binary(&bin);
        let hot = view.hot_field("tenant").unwrap();
        assert_eq!(hot, Some(SxValue::String("acme".to_string())));
    }
}
