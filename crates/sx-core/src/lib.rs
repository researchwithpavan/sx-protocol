//! Core logical model for SX Protocol.

pub mod delta_types;
pub mod envelope;
pub mod error;
pub mod json;
pub mod path;
pub mod table;
pub mod tensor;
pub mod typed_array;
pub mod types;
pub mod value;

pub use delta_types::{DeltaDocument, DeltaOp, DeltaOpKind};
pub use envelope::MessageEnvelope;
pub use error::{SxError, SxErrorCode, SxResult};
pub use path::{SxPath, SxPathSegment};
pub use table::{SxColumn, SxTable};
pub use tensor::SxTensor;
pub use typed_array::{SxTypedArray, SxTypedArrayType};
pub use types::SxType;
pub use value::{BlobRef, DecimalValue, MoneyValue, ReferenceValue, SxValue};
