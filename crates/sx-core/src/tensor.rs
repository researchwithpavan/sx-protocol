use crate::typed_array::SxTypedArray;

/// Logical tensor metadata and data payload.
#[derive(Debug, Clone, PartialEq)]
pub struct SxTensor {
    pub shape: Vec<usize>,
    pub data: SxTypedArray,
    pub layout: Option<String>,
}
