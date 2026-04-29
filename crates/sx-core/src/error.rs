use crate::path::SxPath;
use std::fmt;
use thiserror::Error;

/// Result alias for SX operations.
pub type SxResult<T> = Result<T, SxError>;

/// Stable SX error codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SxErrorCode {
    InvalidMagic,
    UnsupportedVersion,
    InvalidLength,
    SegmentOutOfBounds,
    ChecksumFailed,
    SchemaNotFound,
    TypeMismatch,
    RequiredFieldMissing,
    InvalidFieldId,
    InvalidDictionaryRef,
    InvalidShapeRef,
    MessageTooLarge,
    UnsupportedFeature,
    InvalidPath,
    DuplicateKey,
    ParseError,
    ValidationError,
    InvalidUtf8,
    InvalidNumber,
    UnexpectedEof,
    Internal,
}

impl fmt::Display for SxErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = match self {
            Self::InvalidMagic => "SX_INVALID_MAGIC",
            Self::UnsupportedVersion => "SX_UNSUPPORTED_VERSION",
            Self::InvalidLength => "SX_INVALID_LENGTH",
            Self::SegmentOutOfBounds => "SX_SEGMENT_OUT_OF_BOUNDS",
            Self::ChecksumFailed => "SX_CHECKSUM_FAILED",
            Self::SchemaNotFound => "SX_SCHEMA_NOT_FOUND",
            Self::TypeMismatch => "SX_TYPE_MISMATCH",
            Self::RequiredFieldMissing => "SX_REQUIRED_FIELD_MISSING",
            Self::InvalidFieldId => "SX_INVALID_FIELD_ID",
            Self::InvalidDictionaryRef => "SX_INVALID_DICTIONARY_REF",
            Self::InvalidShapeRef => "SX_INVALID_SHAPE_REF",
            Self::MessageTooLarge => "SX_MESSAGE_TOO_LARGE",
            Self::UnsupportedFeature => "SX_UNSUPPORTED",
            Self::InvalidPath => "SX_INVALID_PATH",
            Self::DuplicateKey => "SX_DUPLICATE_KEY",
            Self::ParseError => "SX_PARSE_ERROR",
            Self::ValidationError => "SX_VALIDATION_ERROR",
            Self::InvalidUtf8 => "SX_INVALID_UTF8",
            Self::InvalidNumber => "SX_INVALID_NUMBER",
            Self::UnexpectedEof => "SX_UNEXPECTED_EOF",
            Self::Internal => "SX_INTERNAL",
        };
        write!(f, "{code}")
    }
}

/// Structured SX error with optional path and context.
#[derive(Debug, Error, Clone)]
#[error("{code}: {message}")]
pub struct SxError {
    pub code: SxErrorCode,
    pub message: String,
    pub path: Option<SxPath>,
}

impl SxError {
    /// Creates a new error.
    pub fn new(code: SxErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            path: None,
        }
    }

    /// Adds path context to an error.
    pub fn with_path(mut self, path: SxPath) -> Self {
        self.path = Some(path);
        self
    }
}
