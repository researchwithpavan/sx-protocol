use crate::typed_array::SxTypedArray;
use crate::value::SxValue;
use std::collections::BTreeMap;

/// A table column can be typed or generic values.
#[derive(Debug, Clone, PartialEq)]
pub enum SxColumn {
    Typed(SxTypedArray),
    Values(Vec<SxValue>),
}

impl SxColumn {
    /// Number of rows in this column.
    pub fn len(&self) -> usize {
        match self {
            Self::Typed(t) => t.len(),
            Self::Values(v) => v.len(),
        }
    }
}

/// Logical table representation.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SxTable {
    pub columns: BTreeMap<String, SxColumn>,
}

impl SxTable {
    /// Returns row count inferred from first column or zero.
    pub fn row_count(&self) -> usize {
        self.columns.values().next().map(SxColumn::len).unwrap_or(0)
    }
}
