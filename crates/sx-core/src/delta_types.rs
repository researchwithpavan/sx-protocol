use crate::path::SxPath;
use crate::value::SxValue;

/// Delta operation kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeltaOpKind {
    Set,
    Replace,
    Remove,
    Append,
    Prepend,
    Insert,
    Increment,
    Decrement,
    Merge,
    Move,
    Copy,
    Clear,
}

/// One delta operation.
#[derive(Debug, Clone, PartialEq)]
pub struct DeltaOp {
    pub kind: DeltaOpKind,
    pub path: SxPath,
    pub value: Option<SxValue>,
    pub from: Option<SxPath>,
    pub index: Option<usize>,
}

/// Delta document.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DeltaDocument {
    pub from_hash: Option<String>,
    pub ops: Vec<DeltaOp>,
}
