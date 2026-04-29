/// Supported typed-array element kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SxTypedArrayType {
    U8,
    I32,
    F32,
    F64,
    Bool,
}

/// Typed array value.
#[derive(Debug, Clone, PartialEq)]
pub enum SxTypedArray {
    U8(Vec<u8>),
    I32(Vec<i32>),
    F32(Vec<f32>),
    F64(Vec<f64>),
    Bool(Vec<bool>),
}

impl SxTypedArray {
    /// Returns the element type of the typed array.
    pub fn element_type(&self) -> SxTypedArrayType {
        match self {
            Self::U8(_) => SxTypedArrayType::U8,
            Self::I32(_) => SxTypedArrayType::I32,
            Self::F32(_) => SxTypedArrayType::F32,
            Self::F64(_) => SxTypedArrayType::F64,
            Self::Bool(_) => SxTypedArrayType::Bool,
        }
    }

    /// Returns number of elements.
    pub fn len(&self) -> usize {
        match self {
            Self::U8(v) => v.len(),
            Self::I32(v) => v.len(),
            Self::F32(v) => v.len(),
            Self::F64(v) => v.len(),
            Self::Bool(v) => v.len(),
        }
    }

    /// Returns true when empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
