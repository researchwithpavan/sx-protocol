use crate::delta_types::DeltaDocument;
use crate::envelope::MessageEnvelope;
use crate::error::{SxError, SxErrorCode, SxResult};
use crate::table::SxTable;
use crate::tensor::SxTensor;
use crate::typed_array::SxTypedArray;
use crate::types::SxType;
use std::collections::BTreeMap;

/// Deterministic decimal value representation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecimalValue {
    pub scaled: i128,
    pub scale: u32,
}

/// Money as deterministic scaled integer + currency code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoneyValue {
    pub currency: String,
    pub scaled: i64,
    pub scale: u32,
}

/// Internal reference value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceValue {
    pub target: String,
}

/// External blob reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobRef {
    pub uri: String,
    pub media_type: Option<String>,
    pub size: Option<u64>,
    pub hash: Option<Vec<u8>>,
}

/// Canonical logical SX value.
#[derive(Debug, Clone, PartialEq)]
pub enum SxValue {
    Null,
    Bool(bool),
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
    I8(i8),
    I16(i16),
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    Decimal(DecimalValue),
    Money(MoneyValue),
    String(String),
    Bytes(Vec<u8>),
    Array(Vec<SxValue>),
    Object(BTreeMap<String, SxValue>),
    Map(Vec<(SxValue, SxValue)>),
    Enum(String),
    Uuid([u8; 16]),
    Timestamp(String),
    Date(String),
    Duration(String),
    Url(String),
    Email(String),
    TypedArray(SxTypedArray),
    Table(SxTable),
    Tensor(SxTensor),
    Reference(ReferenceValue),
    BlobRef(BlobRef),
    Delta(DeltaDocument),
    Message(MessageEnvelope),
}

impl SxValue {
    /// Returns logical type.
    pub fn sx_type(&self) -> SxType {
        match self {
            Self::Null => SxType::Null,
            Self::Bool(_) => SxType::Bool,
            Self::U8(_) => SxType::U8,
            Self::U16(_) => SxType::U16,
            Self::U32(_) => SxType::U32,
            Self::U64(_) => SxType::U64,
            Self::I8(_) => SxType::I8,
            Self::I16(_) => SxType::I16,
            Self::I32(_) => SxType::I32,
            Self::I64(_) => SxType::I64,
            Self::F32(_) => SxType::F32,
            Self::F64(_) => SxType::F64,
            Self::Decimal(_) | Self::Money(_) => SxType::Decimal,
            Self::String(_) | Self::Url(_) | Self::Email(_) => SxType::String,
            Self::Bytes(_) => SxType::Bytes,
            Self::Array(_) => SxType::Array,
            Self::Object(_) => SxType::Object,
            Self::Map(_) => SxType::Map,
            Self::Enum(_) => SxType::Enum,
            Self::Uuid(_) => SxType::Uuid,
            Self::Timestamp(_) => SxType::Timestamp,
            Self::Date(_) => SxType::Date,
            Self::Duration(_) => SxType::Duration,
            Self::TypedArray(_) => SxType::TypedArray,
            Self::Table(_) => SxType::Table,
            Self::Tensor(_) => SxType::Tensor,
            Self::Reference(_) => SxType::Reference,
            Self::BlobRef(_) => SxType::BlobRef,
            Self::Delta(_) => SxType::Delta,
            Self::Message(_) => SxType::Message,
        }
    }

    /// Builds an object while rejecting duplicate keys.
    pub fn object_from_pairs(pairs: Vec<(String, SxValue)>) -> SxResult<Self> {
        let mut out = BTreeMap::new();
        for (k, v) in pairs {
            if out.insert(k.clone(), v).is_some() {
                return Err(SxError::new(
                    SxErrorCode::DuplicateKey,
                    format!("duplicate object key '{k}'"),
                ));
            }
        }
        Ok(Self::Object(out))
    }

    /// Gets field from object.
    pub fn get_field(&self, key: &str) -> Option<&SxValue> {
        match self {
            Self::Object(map) => map.get(key),
            _ => None,
        }
    }

    /// Canonical object entries sorted by key.
    pub fn canonical_object_entries(&self) -> Option<Vec<(&String, &SxValue)>> {
        let Self::Object(map) = self else { return None };
        Some(map.iter().collect())
    }
}

impl From<bool> for SxValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<String> for SxValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<&str> for SxValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_string())
    }
}

impl From<i64> for SxValue {
    fn from(value: i64) -> Self {
        Self::I64(value)
    }
}

impl From<u64> for SxValue {
    fn from(value: u64) -> Self {
        Self::U64(value)
    }
}

impl From<f64> for SxValue {
    fn from(value: f64) -> Self {
        Self::F64(value)
    }
}
