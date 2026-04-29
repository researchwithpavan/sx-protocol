use crate::value::SxValue;
use std::collections::BTreeMap;

/// Message envelope carrying metadata and payload.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MessageEnvelope {
    pub sx_version: u32,
    pub message_id: Option<String>,
    pub message_type: Option<String>,
    pub schema: Option<String>,
    pub timestamp: Option<String>,
    pub logical_hash: Option<Vec<u8>>,
    pub fields: BTreeMap<String, SxValue>,
    pub payload: Option<Box<SxValue>>,
}
