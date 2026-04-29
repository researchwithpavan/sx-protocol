//! SX streaming frame reader/writer.

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use sx_core::{SxError, SxErrorCode, SxResult};

const STREAM_MAGIC: &[u8; 4] = b"SXS1";

/// Stream frame kind.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameKind {
    Message = 1,
    Chunk = 2,
    Schema = 3,
    Dictionary = 4,
    Delta = 5,
    Checkpoint = 6,
}

impl FrameKind {
    fn from_u8(v: u8) -> SxResult<Self> {
        match v {
            1 => Ok(Self::Message),
            2 => Ok(Self::Chunk),
            3 => Ok(Self::Schema),
            4 => Ok(Self::Dictionary),
            5 => Ok(Self::Delta),
            6 => Ok(Self::Checkpoint),
            _ => Err(SxError::new(
                SxErrorCode::ValidationError,
                format!("unknown frame kind {v}"),
            )),
        }
    }
}

/// Chunk metadata for chunk frames.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkMetadata {
    pub message_id: String,
    pub chunk_index: u32,
    pub chunk_count: u32,
    pub segment_id: u32,
    pub offset: u64,
    pub length: u32,
    pub checksum: u32,
}

/// A stream frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub kind: FrameKind,
    pub metadata: Option<ChunkMetadata>,
    pub payload: Vec<u8>,
}

/// Writes frames to bytes.
pub struct FrameWriter {
    frames: Vec<Frame>,
}

impl FrameWriter {
    pub fn new() -> Self {
        Self { frames: Vec::new() }
    }

    pub fn push(&mut self, frame: Frame) {
        self.frames.push(frame);
    }

    pub fn finish(self) -> SxResult<Vec<u8>> {
        let mut out = Vec::new();
        out.extend_from_slice(STREAM_MAGIC);
        out.extend_from_slice(&(self.frames.len() as u32).to_le_bytes());
        for frame in self.frames {
            out.push(frame.kind as u8);
            match &frame.metadata {
                Some(meta) => {
                    out.push(1);
                    write_string(&mut out, &meta.message_id);
                    out.extend_from_slice(&meta.chunk_index.to_le_bytes());
                    out.extend_from_slice(&meta.chunk_count.to_le_bytes());
                    out.extend_from_slice(&meta.segment_id.to_le_bytes());
                    out.extend_from_slice(&meta.offset.to_le_bytes());
                    out.extend_from_slice(&meta.length.to_le_bytes());
                    out.extend_from_slice(&meta.checksum.to_le_bytes());
                }
                None => out.push(0),
            }
            out.extend_from_slice(&(frame.payload.len() as u32).to_le_bytes());
            out.extend_from_slice(&checksum32(&frame.payload).to_le_bytes());
            out.extend_from_slice(&frame.payload);
        }
        Ok(out)
    }
}

/// Reads frames from bytes.
pub struct FrameReader<'a> {
    bytes: &'a [u8],
    cursor: usize,
    remaining: u32,
}

impl<'a> FrameReader<'a> {
    pub fn new(bytes: &'a [u8]) -> SxResult<Self> {
        if bytes.len() < 8 || &bytes[0..4] != STREAM_MAGIC {
            return Err(SxError::new(
                SxErrorCode::InvalidMagic,
                "invalid stream magic",
            ));
        }
        let remaining = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        Ok(Self {
            bytes,
            cursor: 8,
            remaining,
        })
    }

    pub fn next_frame(&mut self) -> SxResult<Option<Frame>> {
        if self.remaining == 0 {
            return Ok(None);
        }
        let kind = FrameKind::from_u8(self.read_u8()?)?;
        let has_meta = self.read_u8()? != 0;
        let metadata = if has_meta {
            let message_id = self.read_string()?;
            let chunk_index = self.read_u32()?;
            let chunk_count = self.read_u32()?;
            let segment_id = self.read_u32()?;
            let offset = self.read_u64()?;
            let length = self.read_u32()?;
            let checksum = self.read_u32()?;
            Some(ChunkMetadata {
                message_id,
                chunk_index,
                chunk_count,
                segment_id,
                offset,
                length,
                checksum,
            })
        } else {
            None
        };
        let payload_len = self.read_u32()? as usize;
        let payload_checksum = self.read_u32()?;
        let payload = self.read_bytes(payload_len)?;
        if checksum32(&payload) != payload_checksum {
            return Err(SxError::new(
                SxErrorCode::ChecksumFailed,
                "frame payload checksum mismatch",
            ));
        }
        if let Some(meta) = &metadata {
            if meta.length as usize != payload.len() {
                return Err(SxError::new(
                    SxErrorCode::ValidationError,
                    "chunk length metadata mismatch",
                ));
            }
            if checksum32(&payload) != meta.checksum {
                return Err(SxError::new(
                    SxErrorCode::ChecksumFailed,
                    "chunk metadata checksum mismatch",
                ));
            }
        }
        self.remaining -= 1;
        Ok(Some(Frame {
            kind,
            metadata,
            payload,
        }))
    }

    fn read_u8(&mut self) -> SxResult<u8> {
        let b = *self
            .bytes
            .get(self.cursor)
            .ok_or_else(|| SxError::new(SxErrorCode::UnexpectedEof, "stream eof"))?;
        self.cursor += 1;
        Ok(b)
    }

    fn read_u32(&mut self) -> SxResult<u32> {
        let raw = self.read_bytes(4)?;
        Ok(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
    }

    fn read_u64(&mut self) -> SxResult<u64> {
        let raw = self.read_bytes(8)?;
        Ok(u64::from_le_bytes([
            raw[0], raw[1], raw[2], raw[3], raw[4], raw[5], raw[6], raw[7],
        ]))
    }

    fn read_string(&mut self) -> SxResult<String> {
        let len = self.read_u32()? as usize;
        let raw = self.read_bytes(len)?;
        String::from_utf8(raw).map_err(|e| SxError::new(SxErrorCode::InvalidUtf8, e.to_string()))
    }

    fn read_bytes(&mut self, n: usize) -> SxResult<Vec<u8>> {
        let end = self
            .cursor
            .checked_add(n)
            .ok_or_else(|| SxError::new(SxErrorCode::InvalidLength, "overflow"))?;
        let slice = self
            .bytes
            .get(self.cursor..end)
            .ok_or_else(|| SxError::new(SxErrorCode::UnexpectedEof, "stream eof"))?;
        self.cursor = end;
        Ok(slice.to_vec())
    }
}

/// Reassembles chunk frames into complete message bytes keyed by message_id.
pub fn reconstruct_chunks(frames: &[Frame]) -> SxResult<BTreeMap<String, Vec<u8>>> {
    #[derive(Default)]
    struct Acc {
        chunks: BTreeMap<u32, Vec<u8>>,
        count: u32,
    }

    let mut map: BTreeMap<String, Acc> = BTreeMap::new();
    for frame in frames {
        if frame.kind != FrameKind::Chunk {
            continue;
        }
        let meta = frame.metadata.as_ref().ok_or_else(|| {
            SxError::new(SxErrorCode::ValidationError, "chunk frame missing metadata")
        })?;

        let acc = map.entry(meta.message_id.clone()).or_default();
        if acc.count == 0 {
            acc.count = meta.chunk_count;
        } else if acc.count != meta.chunk_count {
            return Err(SxError::new(
                SxErrorCode::ValidationError,
                "inconsistent chunk_count for message",
            ));
        }

        if acc
            .chunks
            .insert(meta.chunk_index, frame.payload.clone())
            .is_some()
        {
            return Err(SxError::new(
                SxErrorCode::ValidationError,
                "duplicate chunk index",
            ));
        }
    }

    let mut out = BTreeMap::new();
    for (id, acc) in map {
        if acc.chunks.len() as u32 != acc.count {
            return Err(SxError::new(
                SxErrorCode::ValidationError,
                format!("incomplete chunk set for message '{id}'"),
            ));
        }
        let mut payload = Vec::new();
        for idx in 0..acc.count {
            let chunk = acc.chunks.get(&idx).ok_or_else(|| {
                SxError::new(
                    SxErrorCode::ValidationError,
                    format!("missing chunk {idx} for message '{id}'"),
                )
            })?;
            payload.extend_from_slice(chunk);
        }
        out.insert(id, payload);
    }
    Ok(out)
}

fn checksum32(data: &[u8]) -> u32 {
    let digest = Sha256::digest(data);
    u32::from_le_bytes([digest[0], digest[1], digest[2], digest[3]])
}

fn write_string(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(&(s.len() as u32).to_le_bytes());
    out.extend_from_slice(s.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_roundtrip_and_chunk_reconstruction() {
        let mut writer = FrameWriter::new();
        let c1 = b"hello ".to_vec();
        let c2 = b"world".to_vec();
        writer.push(Frame {
            kind: FrameKind::Chunk,
            metadata: Some(ChunkMetadata {
                message_id: "msg-1".to_string(),
                chunk_index: 0,
                chunk_count: 2,
                segment_id: 0,
                offset: 0,
                length: c1.len() as u32,
                checksum: checksum32(&c1),
            }),
            payload: c1,
        });
        writer.push(Frame {
            kind: FrameKind::Chunk,
            metadata: Some(ChunkMetadata {
                message_id: "msg-1".to_string(),
                chunk_index: 1,
                chunk_count: 2,
                segment_id: 0,
                offset: 6,
                length: c2.len() as u32,
                checksum: checksum32(&c2),
            }),
            payload: c2,
        });
        let bytes = writer.finish().unwrap();
        let mut reader = FrameReader::new(&bytes).unwrap();
        let mut frames = Vec::new();
        while let Some(frame) = reader.next_frame().unwrap() {
            frames.push(frame);
        }
        let rebuilt = reconstruct_chunks(&frames).unwrap();
        assert_eq!(rebuilt.get("msg-1").unwrap(), b"hello world");
    }
}
