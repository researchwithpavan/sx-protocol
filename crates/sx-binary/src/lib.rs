//! SX Binary container encoder/decoder.

use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::collections::HashMap;
use sx_core::{SxColumn, SxError, SxErrorCode, SxResult, SxTable, SxTensor, SxTypedArray, SxValue};

const MAGIC: &[u8; 3] = b"SX\0";
pub const SX_BINARY_VERSION: u8 = 1;
const FLAG_SCHEMA_HASH: u8 = 0x01;
const FLAG_LOGICAL_HASH: u8 = 0x02;
const CODEC_COLUMNAR_EVENT_BATCH_V1: u8 = 1;
const CODEC_COLUMNAR_TABLE_V1: u8 = 2;

/// Decode instrumentation counters for proof tests and benchmarks.
#[derive(Debug, Clone, Copy, Default)]
pub struct DecodeStats {
    pub full_decode_calls: usize,
    pub values_decoded: usize,
    pub objects_decoded: usize,
    pub rows_materialized: usize,
    pub strings_decoded: usize,
    pub cold_values_decoded: usize,
    pub hot_field_reads: usize,
    pub columnar_scans: usize,
}

thread_local! {
    static DECODE_STATS: RefCell<DecodeStats> = RefCell::new(DecodeStats::default());
}

/// Resets decode statistics for the current thread.
pub fn reset_decode_stats() {
    DECODE_STATS.with(|s| *s.borrow_mut() = DecodeStats::default());
}

/// Returns current decode statistics for the current thread.
pub fn current_decode_stats() -> DecodeStats {
    DECODE_STATS.with(|s| *s.borrow())
}

fn with_decode_stats<F: FnOnce(&mut DecodeStats)>(f: F) {
    DECODE_STATS.with(|s| f(&mut s.borrow_mut()));
}

/// Extracts a single field from an encoded event batch without full decode when columnar data is present.
pub fn decode_hot_field_values(bytes: &[u8], field: &str) -> SxResult<Vec<SxValue>> {
    with_decode_stats(|s| s.hot_field_reads += 1);
    let parsed = parse_container(bytes)?;
    if let Some(col_seg) = parsed
        .entries
        .iter()
        .find(|e| e.kind == SegmentKind::ColumnData && e.codec == CODEC_COLUMNAR_EVENT_BATCH_V1)
    {
        let data = segment_slice(bytes, col_seg)?;
        return extract_event_batch_field_column(data, field);
    }

    // Fallback for non-columnar payloads.
    let value = decode_binary(bytes)?;
    let mut out = Vec::new();
    match value {
        SxValue::Object(mut obj) => {
            if let Some(v) = obj.remove(field) {
                out.push(v);
            }
        }
        SxValue::Array(items) => {
            for item in items {
                if let SxValue::Object(mut obj) = item {
                    if let Some(v) = obj.remove(field) {
                        out.push(v);
                    }
                }
            }
        }
        _ => {}
    }
    Ok(out)
}

/// Scans an encoded table column without materializing rows when columnar data is present.
pub fn scan_table_numeric_gt(bytes: &[u8], column: &str, rhs: f64) -> SxResult<usize> {
    with_decode_stats(|s| s.columnar_scans += 1);
    let parsed = parse_container(bytes)?;
    if let Some(col_seg) = parsed
        .entries
        .iter()
        .find(|e| e.kind == SegmentKind::ColumnData && e.codec == CODEC_COLUMNAR_TABLE_V1)
    {
        let data = segment_slice(bytes, col_seg)?;
        return scan_table_column_numeric_gt(data, column, rhs);
    }

    // Fallback path for non-columnar payloads.
    let value = decode_binary(bytes)?;
    let SxValue::Table(table) = value else {
        return Err(SxError::new(
            SxErrorCode::TypeMismatch,
            "value is not a table payload",
        ));
    };
    let col = table
        .columns
        .get(column)
        .ok_or_else(|| SxError::new(SxErrorCode::InvalidPath, "unknown table column"))?;
    let mut count = 0usize;
    match col {
        SxColumn::Values(values) => {
            with_decode_stats(|s| s.rows_materialized += values.len());
            for v in values {
                if numeric_value_gt(v, rhs) {
                    count += 1;
                }
            }
        }
        SxColumn::Typed(SxTypedArray::I32(values)) => {
            for v in values {
                if (*v as f64) > rhs {
                    count += 1;
                }
            }
        }
        SxColumn::Typed(SxTypedArray::F32(values)) => {
            for v in values {
                if (*v as f64) > rhs {
                    count += 1;
                }
            }
        }
        SxColumn::Typed(SxTypedArray::F64(values)) => {
            for v in values {
                if *v > rhs {
                    count += 1;
                }
            }
        }
        SxColumn::Typed(SxTypedArray::U8(values)) => {
            for v in values {
                if (*v as f64) > rhs {
                    count += 1;
                }
            }
        }
        SxColumn::Typed(SxTypedArray::Bool(_)) => {}
    }
    Ok(count)
}

/// Supported segment kinds in the SX binary container.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentKind {
    Envelope = 1,
    HotIndex = 2,
    ShapeTable = 3,
    Dictionary = 4,
    ValueData = 5,
    ColumnData = 6,
    TensorData = 7,
    BlobData = 8,
    DeltaData = 9,
    Extension = 10,
    PresenceMap = 11,
}

impl SegmentKind {
    fn from_u8(v: u8) -> SxResult<Self> {
        match v {
            1 => Ok(Self::Envelope),
            2 => Ok(Self::HotIndex),
            3 => Ok(Self::ShapeTable),
            4 => Ok(Self::Dictionary),
            5 => Ok(Self::ValueData),
            6 => Ok(Self::ColumnData),
            7 => Ok(Self::TensorData),
            8 => Ok(Self::BlobData),
            9 => Ok(Self::DeltaData),
            10 => Ok(Self::Extension),
            11 => Ok(Self::PresenceMap),
            _ => Err(SxError::new(
                SxErrorCode::ValidationError,
                format!("unknown segment kind {v}"),
            )),
        }
    }
}

#[derive(Debug, Clone)]
struct Segment {
    id: u16,
    kind: SegmentKind,
    codec: u8,
    data: Vec<u8>,
    required_extension: bool,
}

#[derive(Debug, Clone)]
struct SegmentEntry {
    id: u16,
    kind: SegmentKind,
    codec: u8,
    offset: u32,
    length: u32,
    checksum: u32,
}

#[derive(Debug, Clone)]
struct ParsedContainer {
    entries: Vec<SegmentEntry>,
}

/// Encodes value into SX binary with optional schema/logical hashes.
pub fn encode_binary(
    value: &SxValue,
    schema_hash: Option<[u8; 32]>,
    logical_hash: Option<[u8; 32]>,
) -> SxResult<Vec<u8>> {
    let mut columnar_segment = None;
    let value_data = if let Some(col) = encode_columnar_event_batch(value)? {
        columnar_segment = Some(col);
        encode_value(&SxValue::String("$sx.columnar_event_batch.v1".to_string()))?
    } else if let Some(col) = encode_columnar_table(value)? {
        columnar_segment = Some(col);
        encode_value(&SxValue::String("$sx.columnar_table.v1".to_string()))?
    } else {
        encode_value(value)?
    };
    let shape_data = encode_shape_table(value);
    let dict_data = encode_dictionary(value);
    let hot_data = encode_hot_index(value);
    let presence_data = encode_presence_maps(value);

    let mut segments = Vec::new();
    segments.push(Segment {
        id: 1,
        kind: SegmentKind::ValueData,
        codec: 0,
        data: value_data,
        required_extension: false,
    });
    if !shape_data.is_empty() {
        segments.push(Segment {
            id: 2,
            kind: SegmentKind::ShapeTable,
            codec: 0,
            data: shape_data,
            required_extension: false,
        });
    }
    if !dict_data.is_empty() {
        segments.push(Segment {
            id: 3,
            kind: SegmentKind::Dictionary,
            codec: 0,
            data: dict_data,
            required_extension: false,
        });
    }
    if !hot_data.is_empty() {
        segments.push(Segment {
            id: 4,
            kind: SegmentKind::HotIndex,
            codec: 0,
            data: hot_data,
            required_extension: false,
        });
    }
    if !presence_data.is_empty() {
        segments.push(Segment {
            id: 5,
            kind: SegmentKind::PresenceMap,
            codec: 0,
            data: presence_data,
            required_extension: false,
        });
    }
    if let Some(data) = columnar_segment {
        let codec = if matches!(value, SxValue::Table(_)) {
            CODEC_COLUMNAR_TABLE_V1
        } else {
            CODEC_COLUMNAR_EVENT_BATCH_V1
        };
        segments.push(Segment {
            id: 6,
            kind: SegmentKind::ColumnData,
            codec,
            data,
            required_extension: false,
        });
    }

    let mut flags = 0u8;
    if schema_hash.is_some() {
        flags |= FLAG_SCHEMA_HASH;
    }
    if logical_hash.is_some() {
        flags |= FLAG_LOGICAL_HASH;
    }

    let segment_table_len = segments.len() * (2 + 1 + 1 + 4 + 4 + 4);
    let header_len = 3
        + 1
        + 1
        + 4
        + 2
        + if schema_hash.is_some() { 32 } else { 0 }
        + if logical_hash.is_some() { 32 } else { 0 };
    let payload_len: usize = segments.iter().map(|s| s.data.len()).sum();
    let total_len = header_len + segment_table_len + payload_len;

    let mut out = Vec::with_capacity(total_len);
    out.extend_from_slice(MAGIC);
    out.push(SX_BINARY_VERSION);
    out.push(flags);
    out.extend_from_slice(&(total_len as u32).to_le_bytes());
    out.extend_from_slice(&(segments.len() as u16).to_le_bytes());
    if let Some(h) = schema_hash {
        out.extend_from_slice(&h);
    }
    if let Some(h) = logical_hash {
        out.extend_from_slice(&h);
    }

    let mut offset = (header_len + segment_table_len) as u32;
    let mut entries = Vec::with_capacity(segments.len());
    for seg in &segments {
        let checksum = checksum32(&seg.data);
        entries.push(SegmentEntry {
            id: seg.id,
            kind: seg.kind,
            codec: if seg.required_extension {
                seg.codec | 0x80
            } else {
                seg.codec
            },
            offset,
            length: seg.data.len() as u32,
            checksum,
        });
        offset += seg.data.len() as u32;
    }

    for e in &entries {
        out.extend_from_slice(&e.id.to_le_bytes());
        out.push(e.kind as u8);
        out.push(e.codec);
        out.extend_from_slice(&e.offset.to_le_bytes());
        out.extend_from_slice(&e.length.to_le_bytes());
        out.extend_from_slice(&e.checksum.to_le_bytes());
    }

    for seg in segments {
        out.extend_from_slice(&seg.data);
    }

    Ok(out)
}

/// Decodes SX binary into logical value.
pub fn decode_binary(bytes: &[u8]) -> SxResult<SxValue> {
    with_decode_stats(|s| s.full_decode_calls += 1);
    let parsed = parse_container(bytes)?;
    validate_metadata_segments(bytes, &parsed.entries)?;
    if let Some(col_seg) = parsed
        .entries
        .iter()
        .find(|e| e.kind == SegmentKind::ColumnData)
    {
        let data = segment_slice(bytes, col_seg)?;
        return match col_seg.codec {
            CODEC_COLUMNAR_EVENT_BATCH_V1 => decode_columnar_event_batch(data),
            CODEC_COLUMNAR_TABLE_V1 => decode_columnar_table(data),
            codec => Err(SxError::new(
                SxErrorCode::UnsupportedFeature,
                format!("unsupported columnar codec {codec}"),
            )),
        };
    }

    let value_seg = parsed
        .entries
        .iter()
        .find(|e| e.kind == SegmentKind::ValueData)
        .ok_or_else(|| SxError::new(SxErrorCode::ValidationError, "missing value segment"))?;
    let payload = segment_slice(bytes, value_seg)?;
    decode_value(payload)
}

fn validate_metadata_segments(bytes: &[u8], entries: &[SegmentEntry]) -> SxResult<()> {
    for entry in entries {
        match entry.kind {
            SegmentKind::Dictionary => validate_dictionary_segment(segment_slice(bytes, entry)?)?,
            SegmentKind::ShapeTable => validate_shape_table_segment(segment_slice(bytes, entry)?)?,
            _ => {}
        }
    }
    Ok(())
}

fn validate_dictionary_segment(bytes: &[u8]) -> SxResult<()> {
    let mut cursor = 0usize;
    let n = decode_varint(bytes, &mut cursor).map_err(|_| {
        SxError::new(
            SxErrorCode::InvalidDictionaryRef,
            "invalid dictionary count encoding",
        )
    })? as usize;
    for _ in 0..n {
        let raw = read_bytes(bytes, &mut cursor).map_err(|_| {
            SxError::new(
                SxErrorCode::InvalidDictionaryRef,
                "invalid dictionary entry bytes",
            )
        })?;
        std::str::from_utf8(&raw).map_err(|_| {
            SxError::new(
                SxErrorCode::InvalidDictionaryRef,
                "dictionary entry is not UTF-8",
            )
        })?;
    }
    if cursor != bytes.len() {
        return Err(SxError::new(
            SxErrorCode::InvalidDictionaryRef,
            "trailing bytes in dictionary segment",
        ));
    }
    Ok(())
}

fn validate_shape_table_segment(bytes: &[u8]) -> SxResult<()> {
    let mut cursor = 0usize;
    let n = decode_varint(bytes, &mut cursor)
        .map_err(|_| SxError::new(SxErrorCode::InvalidShapeRef, "invalid shape count encoding"))?
        as usize;
    for _ in 0..n {
        let raw = read_bytes(bytes, &mut cursor)
            .map_err(|_| SxError::new(SxErrorCode::InvalidShapeRef, "invalid shape entry bytes"))?;
        std::str::from_utf8(&raw)
            .map_err(|_| SxError::new(SxErrorCode::InvalidShapeRef, "shape entry is not UTF-8"))?;
    }
    if cursor != bytes.len() {
        return Err(SxError::new(
            SxErrorCode::InvalidShapeRef,
            "trailing bytes in shape segment",
        ));
    }
    Ok(())
}

fn parse_container(bytes: &[u8]) -> SxResult<ParsedContainer> {
    if bytes.len() < 11 {
        return Err(SxError::new(
            SxErrorCode::InvalidLength,
            "message too short",
        ));
    }
    if &bytes[0..3] != MAGIC {
        return Err(SxError::new(SxErrorCode::InvalidMagic, "invalid magic"));
    }
    let version = bytes[3];
    if version != SX_BINARY_VERSION {
        return Err(SxError::new(
            SxErrorCode::UnsupportedVersion,
            format!("unsupported version {version}"),
        ));
    }
    let flags = bytes[4];
    let total_len = u32::from_le_bytes(read_fixed::<4>(bytes, 5)?) as usize;
    if total_len != bytes.len() {
        return Err(SxError::new(
            SxErrorCode::InvalidLength,
            format!("declared length {total_len} does not match {}", bytes.len()),
        ));
    }

    let mut cursor = 9usize;
    let segment_count = u16::from_le_bytes(read_fixed::<2>(bytes, cursor)?) as usize;
    cursor += 2;

    if flags & FLAG_SCHEMA_HASH != 0 {
        cursor = checked_add(cursor, 32)?;
    }
    if flags & FLAG_LOGICAL_HASH != 0 {
        cursor = checked_add(cursor, 32)?;
    }
    if cursor > bytes.len() {
        return Err(SxError::new(
            SxErrorCode::InvalidLength,
            "header out of bounds",
        ));
    }

    let table_bytes = segment_count
        .checked_mul(16)
        .ok_or_else(|| SxError::new(SxErrorCode::InvalidLength, "segment table overflow"))?;
    if checked_add(cursor, table_bytes)? > bytes.len() {
        return Err(SxError::new(
            SxErrorCode::SegmentOutOfBounds,
            "segment table out of bounds",
        ));
    }

    let mut entries = Vec::with_capacity(segment_count);
    for _ in 0..segment_count {
        let id = u16::from_le_bytes(read_fixed::<2>(bytes, cursor)?);
        cursor += 2;
        let kind = SegmentKind::from_u8(bytes[cursor])?;
        cursor += 1;
        let codec = bytes[cursor];
        cursor += 1;
        let offset = u32::from_le_bytes(read_fixed::<4>(bytes, cursor)?);
        cursor += 4;
        let length = u32::from_le_bytes(read_fixed::<4>(bytes, cursor)?);
        cursor += 4;
        let checksum = u32::from_le_bytes(read_fixed::<4>(bytes, cursor)?);
        cursor += 4;

        let start = offset as usize;
        let end = checked_add(start, length as usize)?;
        if end > bytes.len() {
            return Err(SxError::new(
                SxErrorCode::SegmentOutOfBounds,
                format!("segment {id} out of bounds"),
            ));
        }

        let required = (codec & 0x80) != 0;
        let codec = codec & 0x7F;
        if kind == SegmentKind::Extension && required {
            return Err(SxError::new(
                SxErrorCode::UnsupportedFeature,
                "required extension segment not supported",
            ));
        }
        if kind == SegmentKind::Extension && !required {
            continue;
        }

        let got = checksum32(&bytes[start..end]);
        if got != checksum {
            return Err(SxError::new(
                SxErrorCode::ChecksumFailed,
                format!("segment {id} checksum mismatch"),
            ));
        }
        entries.push(SegmentEntry {
            id,
            kind,
            codec,
            offset,
            length,
            checksum,
        });
    }

    Ok(ParsedContainer { entries })
}

fn segment_slice<'a>(bytes: &'a [u8], segment: &SegmentEntry) -> SxResult<&'a [u8]> {
    let start = segment.offset as usize;
    let end = checked_add(start, segment.length as usize)?;
    bytes
        .get(start..end)
        .ok_or_else(|| SxError::new(SxErrorCode::SegmentOutOfBounds, "segment out of bounds"))
}

/// Encodes unsigned integer using varint.
pub fn encode_varint(mut value: u64, out: &mut Vec<u8>) {
    while value >= 0x80 {
        out.push(((value & 0x7F) as u8) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

/// Decodes varint.
pub fn decode_varint(bytes: &[u8], cursor: &mut usize) -> SxResult<u64> {
    let mut shift = 0u32;
    let mut out = 0u64;
    loop {
        let b = *bytes
            .get(*cursor)
            .ok_or_else(|| SxError::new(SxErrorCode::UnexpectedEof, "varint eof"))?;
        *cursor += 1;
        out |= ((b & 0x7F) as u64) << shift;
        if b & 0x80 == 0 {
            return Ok(out);
        }
        shift += 7;
        if shift > 63 {
            return Err(SxError::new(SxErrorCode::InvalidNumber, "varint too large"));
        }
    }
}

/// ZigZag encodes signed integer.
pub fn zigzag_encode(v: i64) -> u64 {
    ((v << 1) ^ (v >> 63)) as u64
}

/// ZigZag decodes signed integer.
pub fn zigzag_decode(v: u64) -> i64 {
    ((v >> 1) as i64) ^ (-((v & 1) as i64))
}

fn encode_columnar_event_batch(value: &SxValue) -> SxResult<Option<Vec<u8>>> {
    let SxValue::Array(rows) = value else {
        return Ok(None);
    };
    if rows.len() < 64 {
        return Ok(None);
    }

    let mut keys = Vec::<String>::new();
    let mut first = true;
    for row in rows {
        let SxValue::Object(obj) = row else {
            return Ok(None);
        };
        let mut row_keys = obj.keys().cloned().collect::<Vec<_>>();
        row_keys.sort();
        if first {
            keys = row_keys;
            first = false;
        } else if keys != row_keys {
            return Ok(None);
        }
    }
    if keys.is_empty() {
        return Ok(None);
    }

    let mut out = Vec::new();
    // version
    out.push(1);
    encode_varint(rows.len() as u64, &mut out);
    encode_varint(keys.len() as u64, &mut out);

    for (field_id, key) in keys.iter().enumerate() {
        encode_varint((field_id + 1) as u64, &mut out);
        encode_bytes(key.as_bytes(), &mut out);

        let sample = match &rows[0] {
            SxValue::Object(obj) => obj.get(key).unwrap(),
            _ => unreachable!(),
        };

        match sample {
            SxValue::String(_) => {
                out.push(1); // dictionary string column
                let mut dict = Vec::<String>::new();
                let mut dict_index = HashMap::<String, u32>::new();
                let mut ids = Vec::<u32>::with_capacity(rows.len());
                for row in rows {
                    let SxValue::Object(obj) = row else {
                        unreachable!()
                    };
                    let value = match obj.get(key) {
                        Some(SxValue::String(s)) => s.clone(),
                        _ => return Ok(None),
                    };
                    let id = if let Some(id) = dict_index.get(&value) {
                        *id
                    } else {
                        let next = dict.len() as u32;
                        dict_index.insert(value.clone(), next);
                        dict.push(value);
                        next
                    };
                    ids.push(id);
                }
                encode_varint(dict.len() as u64, &mut out);
                for s in &dict {
                    encode_bytes(s.as_bytes(), &mut out);
                }
                encode_varint(ids.len() as u64, &mut out);
                for id in ids {
                    encode_varint(id as u64, &mut out);
                }
            }
            SxValue::I64(_) => {
                out.push(2); // i64 column
                encode_varint(rows.len() as u64, &mut out);
                for row in rows {
                    let SxValue::Object(obj) = row else {
                        unreachable!()
                    };
                    let val = match obj.get(key) {
                        Some(SxValue::I64(v)) => *v,
                        _ => return Ok(None),
                    };
                    out.extend_from_slice(&val.to_le_bytes());
                }
            }
            SxValue::Bool(_) => {
                out.push(3); // bool packed
                let mut bits = Vec::with_capacity(rows.len());
                for row in rows {
                    let SxValue::Object(obj) = row else {
                        unreachable!()
                    };
                    let val = match obj.get(key) {
                        Some(SxValue::Bool(v)) => *v,
                        _ => return Ok(None),
                    };
                    bits.push(val);
                }
                encode_varint(bits.len() as u64, &mut out);
                let packed = pack_bits(&bits);
                encode_bytes(&packed, &mut out);
            }
            _ => return Ok(None),
        }
    }
    Ok(Some(out))
}

fn encode_columnar_table(value: &SxValue) -> SxResult<Option<Vec<u8>>> {
    let SxValue::Table(table) = value else {
        return Ok(None);
    };
    if table.columns.is_empty() {
        return Ok(None);
    }

    let rows = table.row_count();
    if rows < 64 {
        return Ok(None);
    }

    let mut out = Vec::new();
    out.push(1); // version
    encode_varint(rows as u64, &mut out);
    encode_varint(table.columns.len() as u64, &mut out);

    for (field_id, (name, col)) in table.columns.iter().enumerate() {
        encode_varint((field_id + 1) as u64, &mut out);
        encode_bytes(name.as_bytes(), &mut out);
        match col {
            SxColumn::Values(values) => {
                if values.len() != rows {
                    return Ok(None);
                }
                if values.iter().all(|v| matches!(v, SxValue::I64(_))) {
                    out.push(2); // i64
                    encode_varint(rows as u64, &mut out);
                    for v in values {
                        let SxValue::I64(n) = v else { unreachable!() };
                        out.extend_from_slice(&n.to_le_bytes());
                    }
                } else if values.iter().all(|v| matches!(v, SxValue::F64(_))) {
                    out.push(4); // f64
                    encode_varint(rows as u64, &mut out);
                    for v in values {
                        let SxValue::F64(n) = v else { unreachable!() };
                        out.extend_from_slice(&n.to_le_bytes());
                    }
                } else if values.iter().all(|v| matches!(v, SxValue::Bool(_))) {
                    out.push(3); // bool packed
                    let bits: Vec<bool> = values
                        .iter()
                        .map(|v| matches!(v, SxValue::Bool(true)))
                        .collect();
                    encode_varint(bits.len() as u64, &mut out);
                    encode_bytes(&pack_bits(&bits), &mut out);
                } else if values.iter().all(|v| matches!(v, SxValue::String(_))) {
                    out.push(1); // dictionary string
                    let mut dict = Vec::<String>::new();
                    let mut dict_index = HashMap::<String, u32>::new();
                    let mut ids = Vec::<u32>::with_capacity(rows);
                    for v in values {
                        let SxValue::String(s) = v else {
                            unreachable!()
                        };
                        let id = if let Some(id) = dict_index.get(s) {
                            *id
                        } else {
                            let next = dict.len() as u32;
                            dict_index.insert(s.clone(), next);
                            dict.push(s.clone());
                            next
                        };
                        ids.push(id);
                    }
                    encode_varint(dict.len() as u64, &mut out);
                    for s in &dict {
                        encode_bytes(s.as_bytes(), &mut out);
                    }
                    encode_varint(ids.len() as u64, &mut out);
                    for id in ids {
                        encode_varint(id as u64, &mut out);
                    }
                } else {
                    return Ok(None);
                }
            }
            SxColumn::Typed(SxTypedArray::I32(values)) => {
                if values.len() != rows {
                    return Ok(None);
                }
                out.push(2); // i64
                encode_varint(rows as u64, &mut out);
                for n in values {
                    out.extend_from_slice(&(*n as i64).to_le_bytes());
                }
            }
            SxColumn::Typed(SxTypedArray::F64(values)) => {
                if values.len() != rows {
                    return Ok(None);
                }
                out.push(4); // f64
                encode_varint(rows as u64, &mut out);
                for n in values {
                    out.extend_from_slice(&n.to_le_bytes());
                }
            }
            SxColumn::Typed(SxTypedArray::F32(values)) => {
                if values.len() != rows {
                    return Ok(None);
                }
                out.push(4); // f64
                encode_varint(rows as u64, &mut out);
                for n in values {
                    out.extend_from_slice(&(*n as f64).to_le_bytes());
                }
            }
            SxColumn::Typed(SxTypedArray::U8(values)) => {
                if values.len() != rows {
                    return Ok(None);
                }
                out.push(2); // i64
                encode_varint(rows as u64, &mut out);
                for n in values {
                    out.extend_from_slice(&(*n as i64).to_le_bytes());
                }
            }
            SxColumn::Typed(SxTypedArray::Bool(values)) => {
                if values.len() != rows {
                    return Ok(None);
                }
                out.push(3); // bool packed
                encode_varint(rows as u64, &mut out);
                encode_bytes(&pack_bits(values), &mut out);
            }
        }
    }

    Ok(Some(out))
}

fn decode_columnar_event_batch(data: &[u8]) -> SxResult<SxValue> {
    let parsed = parse_columnar_payload(data)?;
    let mut rows = Vec::with_capacity(parsed.rows);
    with_decode_stats(|s| s.rows_materialized += parsed.rows);
    for row_idx in 0..parsed.rows {
        let mut obj = std::collections::BTreeMap::new();
        for col in &parsed.columns {
            let value = match col {
                ColumnarColumn::DictString { name, dict, ids } => {
                    let idx = *ids.get(row_idx).ok_or_else(|| {
                        SxError::new(SxErrorCode::InvalidLength, "column row index out of bounds")
                    })? as usize;
                    let s = dict.get(idx).ok_or_else(|| {
                        SxError::new(
                            SxErrorCode::InvalidDictionaryRef,
                            "dictionary ref out of bounds",
                        )
                    })?;
                    obj.insert(name.clone(), SxValue::String(s.clone()));
                    continue;
                }
                ColumnarColumn::I64 { values, .. } => {
                    SxValue::I64(*values.get(row_idx).ok_or_else(|| {
                        SxError::new(SxErrorCode::InvalidLength, "column row index out of bounds")
                    })?)
                }
                ColumnarColumn::F64 { values, .. } => {
                    SxValue::F64(*values.get(row_idx).ok_or_else(|| {
                        SxError::new(SxErrorCode::InvalidLength, "column row index out of bounds")
                    })?)
                }
                ColumnarColumn::Bool { bytes, len, .. } => {
                    if row_idx >= *len {
                        return Err(SxError::new(
                            SxErrorCode::InvalidLength,
                            "bool column row index out of bounds",
                        ));
                    }
                    SxValue::Bool((bytes[row_idx / 8] & (1u8 << (row_idx % 8))) != 0)
                }
            };
            obj.insert(col.name().to_string(), value);
        }
        rows.push(SxValue::Object(obj));
    }
    Ok(SxValue::Array(rows))
}

fn decode_columnar_table(data: &[u8]) -> SxResult<SxValue> {
    let parsed = parse_columnar_payload(data)?;
    let mut columns = std::collections::BTreeMap::new();
    for col in parsed.columns {
        let name = col.name().to_string();
        let value = match col {
            ColumnarColumn::DictString { dict, ids, .. } => {
                let mut out = Vec::with_capacity(ids.len());
                for id in ids {
                    let s = dict.get(id as usize).ok_or_else(|| {
                        SxError::new(
                            SxErrorCode::InvalidDictionaryRef,
                            "dictionary ref out of bounds",
                        )
                    })?;
                    out.push(SxValue::String(s.clone()));
                }
                with_decode_stats(|s| s.rows_materialized += out.len());
                SxColumn::Values(out)
            }
            ColumnarColumn::I64 { values, .. } => SxColumn::Values(
                {
                    with_decode_stats(|s| s.rows_materialized += values.len());
                    values
                }
                .into_iter()
                .map(SxValue::I64)
                .collect::<Vec<SxValue>>(),
            ),
            ColumnarColumn::F64 { values, .. } => SxColumn::Values(
                {
                    with_decode_stats(|s| s.rows_materialized += values.len());
                    values
                }
                .into_iter()
                .map(SxValue::F64)
                .collect::<Vec<SxValue>>(),
            ),
            ColumnarColumn::Bool { bytes, len, .. } => {
                let mut values = Vec::with_capacity(len);
                for i in 0..len {
                    values.push(SxValue::Bool((bytes[i / 8] & (1u8 << (i % 8))) != 0));
                }
                with_decode_stats(|s| s.rows_materialized += values.len());
                SxColumn::Values(values)
            }
        };
        columns.insert(name, value);
    }
    Ok(SxValue::Table(SxTable { columns }))
}

fn extract_event_batch_field_column(data: &[u8], field: &str) -> SxResult<Vec<SxValue>> {
    let mut cursor = 0usize;
    let version = *data
        .get(cursor)
        .ok_or_else(|| SxError::new(SxErrorCode::UnexpectedEof, "columnar version eof"))?;
    cursor += 1;
    if version != 1 {
        return Err(SxError::new(
            SxErrorCode::UnsupportedVersion,
            format!("unsupported columnar payload version {version}"),
        ));
    }
    let rows = decode_varint(data, &mut cursor)? as usize;
    let cols = decode_varint(data, &mut cursor)? as usize;
    let mut out = Vec::new();
    for _ in 0..cols {
        let _field_id = decode_varint(data, &mut cursor)?;
        let name = read_string(data, &mut cursor)?;
        let kind = *data
            .get(cursor)
            .ok_or_else(|| SxError::new(SxErrorCode::UnexpectedEof, "column kind eof"))?;
        cursor += 1;
        if name == field {
            match kind {
                1 => {
                    let dict_len = decode_varint(data, &mut cursor)? as usize;
                    let mut dict = Vec::with_capacity(dict_len);
                    for _ in 0..dict_len {
                        dict.push(read_string(data, &mut cursor)?);
                    }
                    let ids_len = decode_varint(data, &mut cursor)? as usize;
                    if ids_len != rows {
                        return Err(SxError::new(
                            SxErrorCode::InvalidLength,
                            "dictionary id column length mismatch",
                        ));
                    }
                    out.reserve(ids_len);
                    for _ in 0..ids_len {
                        let idx = decode_varint(data, &mut cursor)? as usize;
                        let s = dict.get(idx).ok_or_else(|| {
                            SxError::new(
                                SxErrorCode::InvalidDictionaryRef,
                                "dictionary ref out of bounds",
                            )
                        })?;
                        out.push(SxValue::String(s.clone()));
                    }
                }
                2 => {
                    let len = decode_varint(data, &mut cursor)? as usize;
                    if len != rows {
                        return Err(SxError::new(
                            SxErrorCode::InvalidLength,
                            "i64 column length mismatch",
                        ));
                    }
                    out.reserve(len);
                    for _ in 0..len {
                        out.push(SxValue::I64(i64::from_le_bytes(read_fixed::<8>(
                            data, cursor,
                        )?)));
                        cursor += 8;
                    }
                }
                3 => {
                    let len = decode_varint(data, &mut cursor)? as usize;
                    if len != rows {
                        return Err(SxError::new(
                            SxErrorCode::InvalidLength,
                            "bool column length mismatch",
                        ));
                    }
                    let bits = read_bytes(data, &mut cursor)?;
                    out.reserve(len);
                    for i in 0..len {
                        out.push(SxValue::Bool((bits[i / 8] & (1u8 << (i % 8))) != 0));
                    }
                }
                4 => {
                    let len = decode_varint(data, &mut cursor)? as usize;
                    if len != rows {
                        return Err(SxError::new(
                            SxErrorCode::InvalidLength,
                            "f64 column length mismatch",
                        ));
                    }
                    out.reserve(len);
                    for _ in 0..len {
                        out.push(SxValue::F64(f64::from_le_bytes(read_fixed::<8>(
                            data, cursor,
                        )?)));
                        cursor += 8;
                    }
                }
                _ => {
                    return Err(SxError::new(
                        SxErrorCode::ValidationError,
                        "unknown column kind",
                    ))
                }
            }
            return Ok(out);
        }

        skip_column_payload(data, &mut cursor, kind)?;
    }
    Ok(Vec::new())
}

fn scan_table_column_numeric_gt(data: &[u8], column: &str, rhs: f64) -> SxResult<usize> {
    let mut cursor = 0usize;
    let version = *data
        .get(cursor)
        .ok_or_else(|| SxError::new(SxErrorCode::UnexpectedEof, "columnar version eof"))?;
    cursor += 1;
    if version != 1 {
        return Err(SxError::new(
            SxErrorCode::UnsupportedVersion,
            format!("unsupported columnar payload version {version}"),
        ));
    }
    let rows = decode_varint(data, &mut cursor)? as usize;
    let cols = decode_varint(data, &mut cursor)? as usize;

    for _ in 0..cols {
        let _field_id = decode_varint(data, &mut cursor)?;
        let name = read_string(data, &mut cursor)?;
        let kind = *data
            .get(cursor)
            .ok_or_else(|| SxError::new(SxErrorCode::UnexpectedEof, "column kind eof"))?;
        cursor += 1;
        if name == column {
            return match kind {
                2 => {
                    let len = decode_varint(data, &mut cursor)? as usize;
                    if len != rows {
                        return Err(SxError::new(
                            SxErrorCode::InvalidLength,
                            "i64 column length mismatch",
                        ));
                    }
                    let mut count = 0usize;
                    for _ in 0..len {
                        let v = i64::from_le_bytes(read_fixed::<8>(data, cursor)?);
                        cursor += 8;
                        if (v as f64) > rhs {
                            count += 1;
                        }
                    }
                    Ok(count)
                }
                4 => {
                    let len = decode_varint(data, &mut cursor)? as usize;
                    if len != rows {
                        return Err(SxError::new(
                            SxErrorCode::InvalidLength,
                            "f64 column length mismatch",
                        ));
                    }
                    let mut count = 0usize;
                    for _ in 0..len {
                        let v = f64::from_le_bytes(read_fixed::<8>(data, cursor)?);
                        cursor += 8;
                        if v > rhs {
                            count += 1;
                        }
                    }
                    Ok(count)
                }
                _ => Err(SxError::new(
                    SxErrorCode::TypeMismatch,
                    "requested column is not numeric",
                )),
            };
        }
        skip_column_payload(data, &mut cursor, kind)?;
    }
    Err(SxError::new(
        SxErrorCode::InvalidPath,
        "table column not found in encoded payload",
    ))
}

fn skip_column_payload(data: &[u8], cursor: &mut usize, kind: u8) -> SxResult<()> {
    match kind {
        1 => {
            let dict_len = decode_varint(data, cursor)? as usize;
            for _ in 0..dict_len {
                let _ = read_string(data, cursor)?;
            }
            let ids_len = decode_varint(data, cursor)? as usize;
            for _ in 0..ids_len {
                let _ = decode_varint(data, cursor)?;
            }
        }
        2 | 4 => {
            let len = decode_varint(data, cursor)? as usize;
            *cursor = checked_add(*cursor, len * 8)?;
            if *cursor > data.len() {
                return Err(SxError::new(
                    SxErrorCode::UnexpectedEof,
                    "numeric column payload eof",
                ));
            }
        }
        3 => {
            let _len = decode_varint(data, cursor)? as usize;
            let _ = read_bytes(data, cursor)?;
        }
        _ => {
            return Err(SxError::new(
                SxErrorCode::ValidationError,
                format!("unknown column kind {kind}"),
            ))
        }
    }
    Ok(())
}

#[derive(Debug)]
enum ColumnarColumn {
    DictString {
        name: String,
        dict: Vec<String>,
        ids: Vec<u32>,
    },
    I64 {
        name: String,
        values: Vec<i64>,
    },
    F64 {
        name: String,
        values: Vec<f64>,
    },
    Bool {
        name: String,
        bytes: Vec<u8>,
        len: usize,
    },
}

impl ColumnarColumn {
    fn name(&self) -> &str {
        match self {
            Self::DictString { name, .. }
            | Self::I64 { name, .. }
            | Self::F64 { name, .. }
            | Self::Bool { name, .. } => name,
        }
    }
}

#[derive(Debug)]
struct ParsedColumnar {
    rows: usize,
    columns: Vec<ColumnarColumn>,
}

fn parse_columnar_payload(data: &[u8]) -> SxResult<ParsedColumnar> {
    let mut cursor = 0usize;
    let version = *data
        .get(cursor)
        .ok_or_else(|| SxError::new(SxErrorCode::UnexpectedEof, "columnar version eof"))?;
    cursor += 1;
    if version != 1 {
        return Err(SxError::new(
            SxErrorCode::UnsupportedVersion,
            format!("unsupported columnar payload version {version}"),
        ));
    }
    let rows = decode_varint(data, &mut cursor)? as usize;
    let cols = decode_varint(data, &mut cursor)? as usize;
    let mut columns = Vec::with_capacity(cols);
    for _ in 0..cols {
        let _field_id = decode_varint(data, &mut cursor)?;
        let name = read_string(data, &mut cursor)?;
        let kind = *data
            .get(cursor)
            .ok_or_else(|| SxError::new(SxErrorCode::UnexpectedEof, "column kind eof"))?;
        cursor += 1;
        match kind {
            1 => {
                let dict_len = decode_varint(data, &mut cursor)? as usize;
                let mut dict = Vec::with_capacity(dict_len);
                for _ in 0..dict_len {
                    dict.push(read_string(data, &mut cursor)?);
                }
                let ids_len = decode_varint(data, &mut cursor)? as usize;
                if ids_len != rows {
                    return Err(SxError::new(
                        SxErrorCode::InvalidLength,
                        "dictionary id column length mismatch",
                    ));
                }
                let mut ids = Vec::with_capacity(ids_len);
                for _ in 0..ids_len {
                    ids.push(decode_varint(data, &mut cursor)? as u32);
                }
                columns.push(ColumnarColumn::DictString { name, dict, ids });
            }
            2 => {
                let len = decode_varint(data, &mut cursor)? as usize;
                if len != rows {
                    return Err(SxError::new(
                        SxErrorCode::InvalidLength,
                        "i64 column length mismatch",
                    ));
                }
                let mut values = Vec::with_capacity(len);
                for _ in 0..len {
                    values.push(i64::from_le_bytes(read_fixed::<8>(data, cursor)?));
                    cursor += 8;
                }
                columns.push(ColumnarColumn::I64 { name, values });
            }
            3 => {
                let len = decode_varint(data, &mut cursor)? as usize;
                if len != rows {
                    return Err(SxError::new(
                        SxErrorCode::InvalidLength,
                        "bool column length mismatch",
                    ));
                }
                let packed = read_bytes(data, &mut cursor)?;
                columns.push(ColumnarColumn::Bool {
                    name,
                    bytes: packed,
                    len,
                });
            }
            4 => {
                let len = decode_varint(data, &mut cursor)? as usize;
                if len != rows {
                    return Err(SxError::new(
                        SxErrorCode::InvalidLength,
                        "f64 column length mismatch",
                    ));
                }
                let mut values = Vec::with_capacity(len);
                for _ in 0..len {
                    values.push(f64::from_le_bytes(read_fixed::<8>(data, cursor)?));
                    cursor += 8;
                }
                columns.push(ColumnarColumn::F64 { name, values });
            }
            _ => {
                return Err(SxError::new(
                    SxErrorCode::ValidationError,
                    format!("unknown column kind {kind}"),
                ))
            }
        }
    }
    if cursor != data.len() {
        return Err(SxError::new(
            SxErrorCode::ValidationError,
            "trailing bytes in columnar payload",
        ));
    }
    Ok(ParsedColumnar { rows, columns })
}

fn numeric_value_gt(v: &SxValue, rhs: f64) -> bool {
    match v {
        SxValue::I8(x) => (*x as f64) > rhs,
        SxValue::I16(x) => (*x as f64) > rhs,
        SxValue::I32(x) => (*x as f64) > rhs,
        SxValue::I64(x) => (*x as f64) > rhs,
        SxValue::U8(x) => (*x as f64) > rhs,
        SxValue::U16(x) => (*x as f64) > rhs,
        SxValue::U32(x) => (*x as f64) > rhs,
        SxValue::U64(x) => (*x as f64) > rhs,
        SxValue::F32(x) => (*x as f64) > rhs,
        SxValue::F64(x) => *x > rhs,
        _ => false,
    }
}

fn encode_shape_table(value: &SxValue) -> Vec<u8> {
    let mut shapes = Vec::<Vec<String>>::new();
    collect_shapes(value, &mut shapes);
    shapes.sort();
    shapes.dedup();
    let mut out = Vec::new();
    encode_varint(shapes.len() as u64, &mut out);
    for s in shapes {
        let text = s.join(",");
        encode_bytes(text.as_bytes(), &mut out);
    }
    out
}

fn collect_shapes(value: &SxValue, out: &mut Vec<Vec<String>>) {
    match value {
        SxValue::Object(map) => {
            out.push(map.keys().cloned().collect());
            for v in map.values() {
                collect_shapes(v, out);
            }
        }
        SxValue::Array(items) => {
            for v in items {
                collect_shapes(v, out);
            }
        }
        SxValue::Table(table) => {
            out.push(table.columns.keys().cloned().collect());
        }
        _ => {}
    }
}

fn encode_dictionary(value: &SxValue) -> Vec<u8> {
    let mut strings = Vec::new();
    collect_strings(value, &mut strings);
    strings.sort();
    strings.dedup();
    let mut out = Vec::new();
    encode_varint(strings.len() as u64, &mut out);
    for s in strings {
        encode_bytes(s.as_bytes(), &mut out);
    }
    out
}

fn collect_strings(value: &SxValue, out: &mut Vec<String>) {
    match value {
        SxValue::String(s)
        | SxValue::Enum(s)
        | SxValue::Timestamp(s)
        | SxValue::Date(s)
        | SxValue::Duration(s)
        | SxValue::Url(s)
        | SxValue::Email(s) => out.push(s.clone()),
        SxValue::Object(map) => {
            for (k, v) in map {
                out.push(k.clone());
                collect_strings(v, out);
            }
        }
        SxValue::Array(items) => {
            for v in items {
                collect_strings(v, out);
            }
        }
        SxValue::Table(table) => {
            for k in table.columns.keys() {
                out.push(k.clone());
            }
        }
        _ => {}
    }
}

fn encode_hot_index(value: &SxValue) -> Vec<u8> {
    let mut out = Vec::new();
    if let SxValue::Object(map) = value {
        encode_varint(map.len() as u64, &mut out);
        for (idx, key) in map.keys().enumerate() {
            encode_bytes(key.as_bytes(), &mut out);
            encode_varint(idx as u64, &mut out);
            encode_varint(0, &mut out);
        }
    }
    out
}

fn encode_presence_maps(value: &SxValue) -> Vec<u8> {
    let mut out = Vec::new();
    if let SxValue::Table(table) = value {
        encode_varint(table.columns.len() as u64, &mut out);
        for (name, col) in &table.columns {
            encode_bytes(name.as_bytes(), &mut out);
            let bitmap = match col {
                SxColumn::Values(values) => {
                    values.iter().map(|v| !matches!(v, SxValue::Null)).collect()
                }
                SxColumn::Typed(t) => vec![true; t.len()],
            };
            encode_varint(bitmap.len() as u64, &mut out);
            let bytes = pack_bits(&bitmap);
            encode_bytes(&bytes, &mut out);
        }
    }
    out
}

fn pack_bits(bits: &[bool]) -> Vec<u8> {
    let mut out = vec![0u8; (bits.len() + 7) / 8];
    for (i, bit) in bits.iter().enumerate() {
        if *bit {
            out[i / 8] |= 1u8 << (i % 8);
        }
    }
    out
}

fn encode_bytes(bytes: &[u8], out: &mut Vec<u8>) {
    encode_varint(bytes.len() as u64, out);
    out.extend_from_slice(bytes);
}

const TAG_NULL: u8 = 0x00;
const TAG_FALSE: u8 = 0x01;
const TAG_TRUE: u8 = 0x02;
const TAG_U64: u8 = 0x10;
const TAG_I64: u8 = 0x11;
const TAG_F64: u8 = 0x12;
const TAG_DECIMAL: u8 = 0x13;
const TAG_STRING: u8 = 0x20;
const TAG_BYTES: u8 = 0x22;
const TAG_ARRAY: u8 = 0x30;
const TAG_TYPED_ARRAY: u8 = 0x31;
const TAG_TABLE: u8 = 0x32;
const TAG_TENSOR: u8 = 0x33;
const TAG_OBJECT: u8 = 0x40;
const TAG_TIMESTAMP: u8 = 0x50;
const TAG_DATE: u8 = 0x51;
const TAG_DURATION: u8 = 0x52;
const TAG_UUID: u8 = 0x53;
const TAG_DELTA: u8 = 0x60;
const TAG_REFERENCE: u8 = 0x70;
const TAG_BLOB_REF: u8 = 0x71;

fn encode_value(value: &SxValue) -> SxResult<Vec<u8>> {
    let mut out = Vec::new();
    write_value(value, &mut out)?;
    Ok(out)
}

fn write_value(value: &SxValue, out: &mut Vec<u8>) -> SxResult<()> {
    match value {
        SxValue::Null => out.push(TAG_NULL),
        SxValue::Bool(false) => out.push(TAG_FALSE),
        SxValue::Bool(true) => out.push(TAG_TRUE),
        SxValue::U8(v) => {
            out.push(TAG_U64);
            encode_varint(*v as u64, out);
        }
        SxValue::U16(v) => {
            out.push(TAG_U64);
            encode_varint(*v as u64, out);
        }
        SxValue::U32(v) => {
            out.push(TAG_U64);
            encode_varint(*v as u64, out);
        }
        SxValue::U64(v) => {
            out.push(TAG_U64);
            encode_varint(*v, out);
        }
        SxValue::I8(v) => {
            out.push(TAG_I64);
            encode_varint(zigzag_encode(*v as i64), out);
        }
        SxValue::I16(v) => {
            out.push(TAG_I64);
            encode_varint(zigzag_encode(*v as i64), out);
        }
        SxValue::I32(v) => {
            out.push(TAG_I64);
            encode_varint(zigzag_encode(*v as i64), out);
        }
        SxValue::I64(v) => {
            out.push(TAG_I64);
            encode_varint(zigzag_encode(*v), out);
        }
        SxValue::F32(v) => {
            out.push(TAG_F64);
            out.extend_from_slice(&(*v as f64).to_le_bytes());
        }
        SxValue::F64(v) => {
            out.push(TAG_F64);
            out.extend_from_slice(&v.to_le_bytes());
        }
        SxValue::Decimal(d) => {
            out.push(TAG_DECIMAL);
            out.push(0);
            out.extend_from_slice(&d.scaled.to_le_bytes());
            out.extend_from_slice(&d.scale.to_le_bytes());
        }
        SxValue::Money(m) => {
            out.push(TAG_DECIMAL);
            out.push(1);
            out.extend_from_slice(&(m.scaled as i128).to_le_bytes());
            out.extend_from_slice(&m.scale.to_le_bytes());
            encode_bytes(m.currency.as_bytes(), out);
        }
        SxValue::String(s) | SxValue::Enum(s) | SxValue::Url(s) | SxValue::Email(s) => {
            out.push(TAG_STRING);
            encode_bytes(s.as_bytes(), out);
        }
        SxValue::Bytes(b) => {
            out.push(TAG_BYTES);
            encode_bytes(b, out);
        }
        SxValue::Array(items) => {
            out.push(TAG_ARRAY);
            encode_varint(items.len() as u64, out);
            for item in items {
                write_value(item, out)?;
            }
        }
        SxValue::Object(map) => {
            out.push(TAG_OBJECT);
            encode_varint(map.len() as u64, out);
            for (k, v) in map {
                encode_bytes(k.as_bytes(), out);
                write_value(v, out)?;
            }
        }
        SxValue::Map(entries) => {
            out.push(TAG_OBJECT);
            encode_varint(entries.len() as u64, out);
            for (k, v) in entries {
                let key_text = format!("{:?}", k);
                encode_bytes(key_text.as_bytes(), out);
                write_value(v, out)?;
            }
        }
        SxValue::Uuid(u) => {
            out.push(TAG_UUID);
            out.extend_from_slice(u);
        }
        SxValue::Timestamp(s) => {
            out.push(TAG_TIMESTAMP);
            encode_bytes(s.as_bytes(), out);
        }
        SxValue::Date(s) => {
            out.push(TAG_DATE);
            encode_bytes(s.as_bytes(), out);
        }
        SxValue::Duration(s) => {
            out.push(TAG_DURATION);
            encode_bytes(s.as_bytes(), out);
        }
        SxValue::TypedArray(a) => {
            out.push(TAG_TYPED_ARRAY);
            match a {
                SxTypedArray::U8(v) => {
                    out.push(1);
                    encode_varint(v.len() as u64, out);
                    out.extend_from_slice(v);
                }
                SxTypedArray::I32(v) => {
                    out.push(2);
                    encode_varint(v.len() as u64, out);
                    for n in v {
                        out.extend_from_slice(&n.to_le_bytes());
                    }
                }
                SxTypedArray::F32(v) => {
                    out.push(3);
                    encode_varint(v.len() as u64, out);
                    for n in v {
                        out.extend_from_slice(&n.to_le_bytes());
                    }
                }
                SxTypedArray::F64(v) => {
                    out.push(4);
                    encode_varint(v.len() as u64, out);
                    for n in v {
                        out.extend_from_slice(&n.to_le_bytes());
                    }
                }
                SxTypedArray::Bool(v) => {
                    out.push(5);
                    encode_varint(v.len() as u64, out);
                    out.extend_from_slice(&pack_bits(v));
                }
            }
        }
        SxValue::Table(SxTable { columns }) => {
            out.push(TAG_TABLE);
            encode_varint(columns.len() as u64, out);
            for (name, col) in columns {
                encode_bytes(name.as_bytes(), out);
                match col {
                    SxColumn::Typed(t) => {
                        out.push(1);
                        write_value(&SxValue::TypedArray(t.clone()), out)?;
                    }
                    SxColumn::Values(v) => {
                        out.push(2);
                        write_value(&SxValue::Array(v.clone()), out)?;
                    }
                }
            }
        }
        SxValue::Tensor(SxTensor {
            shape,
            data,
            layout,
        }) => {
            out.push(TAG_TENSOR);
            encode_varint(shape.len() as u64, out);
            for d in shape {
                encode_varint(*d as u64, out);
            }
            match layout {
                Some(l) => {
                    out.push(1);
                    encode_bytes(l.as_bytes(), out);
                }
                None => out.push(0),
            }
            write_value(&SxValue::TypedArray(data.clone()), out)?;
        }
        SxValue::Reference(r) => {
            out.push(TAG_REFERENCE);
            encode_bytes(r.target.as_bytes(), out);
        }
        SxValue::BlobRef(b) => {
            out.push(TAG_BLOB_REF);
            encode_bytes(b.uri.as_bytes(), out);
            if let Some(m) = &b.media_type {
                out.push(1);
                encode_bytes(m.as_bytes(), out);
            } else {
                out.push(0);
            }
            if let Some(sz) = b.size {
                out.push(1);
                encode_varint(sz, out);
            } else {
                out.push(0);
            }
            if let Some(h) = &b.hash {
                out.push(1);
                encode_bytes(h, out);
            } else {
                out.push(0);
            }
        }
        SxValue::Delta(d) => {
            out.push(TAG_DELTA);
            let text = sx_text::format_canonical(&SxValue::Delta(d.clone()));
            let serialized = text.into_bytes();
            encode_bytes(&serialized, out);
        }
        SxValue::Message(m) => {
            write_value(
                &SxValue::Object(
                    m.fields
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect(),
                ),
                out,
            )?;
        }
    }
    Ok(())
}

fn decode_value(bytes: &[u8]) -> SxResult<SxValue> {
    let mut cursor = 0usize;
    let v = read_value(bytes, &mut cursor)?;
    if cursor != bytes.len() {
        return Err(SxError::new(
            SxErrorCode::ValidationError,
            "trailing bytes in value segment",
        ));
    }
    Ok(v)
}

fn read_value(bytes: &[u8], cursor: &mut usize) -> SxResult<SxValue> {
    with_decode_stats(|s| s.values_decoded += 1);
    let tag = *bytes
        .get(*cursor)
        .ok_or_else(|| SxError::new(SxErrorCode::UnexpectedEof, "value tag eof"))?;
    *cursor += 1;
    match tag {
        TAG_NULL => Ok(SxValue::Null),
        TAG_FALSE => Ok(SxValue::Bool(false)),
        TAG_TRUE => Ok(SxValue::Bool(true)),
        TAG_U64 => Ok(SxValue::U64(decode_varint(bytes, cursor)?)),
        TAG_I64 => Ok(SxValue::I64(zigzag_decode(decode_varint(bytes, cursor)?))),
        TAG_F64 => {
            let raw = read_fixed::<8>(bytes, *cursor)?;
            *cursor += 8;
            Ok(SxValue::F64(f64::from_le_bytes(raw)))
        }
        TAG_DECIMAL => {
            let flavor = *bytes
                .get(*cursor)
                .ok_or_else(|| SxError::new(SxErrorCode::UnexpectedEof, "decimal flavor eof"))?;
            *cursor += 1;
            let scaled = i128::from_le_bytes(read_fixed::<16>(bytes, *cursor)?);
            *cursor += 16;
            let scale = u32::from_le_bytes(read_fixed::<4>(bytes, *cursor)?);
            *cursor += 4;
            if flavor == 1 {
                let s = read_string(bytes, cursor)?;
                return Ok(SxValue::Money(sx_core::MoneyValue {
                    currency: s,
                    scaled: scaled as i64,
                    scale,
                }));
            }
            Ok(SxValue::Decimal(sx_core::DecimalValue { scaled, scale }))
        }
        TAG_STRING => {
            let s = read_string(bytes, cursor)?;
            Ok(SxValue::String(s))
        }
        TAG_BYTES => {
            let b = read_bytes(bytes, cursor)?;
            Ok(SxValue::Bytes(b))
        }
        TAG_ARRAY => {
            let n = decode_varint(bytes, cursor)? as usize;
            let mut out = Vec::with_capacity(n);
            for _ in 0..n {
                out.push(read_value(bytes, cursor)?);
            }
            Ok(SxValue::Array(out))
        }
        TAG_OBJECT => {
            with_decode_stats(|s| s.objects_decoded += 1);
            let n = decode_varint(bytes, cursor)? as usize;
            let mut out = std::collections::BTreeMap::new();
            for _ in 0..n {
                let k = read_string(bytes, cursor)?;
                let v = read_value(bytes, cursor)?;
                if k == "payload" || k == "cold_payload" || k == "cold_blob" {
                    with_decode_stats(|s| s.cold_values_decoded += 1);
                }
                if out.insert(k.clone(), v).is_some() {
                    return Err(SxError::new(
                        SxErrorCode::DuplicateKey,
                        format!("duplicate key '{k}' in object"),
                    ));
                }
            }
            Ok(SxValue::Object(out))
        }
        TAG_UUID => {
            let raw = read_fixed::<16>(bytes, *cursor)?;
            *cursor += 16;
            Ok(SxValue::Uuid(raw))
        }
        TAG_TIMESTAMP => Ok(SxValue::Timestamp(read_string(bytes, cursor)?)),
        TAG_DATE => Ok(SxValue::Date(read_string(bytes, cursor)?)),
        TAG_DURATION => Ok(SxValue::Duration(read_string(bytes, cursor)?)),
        TAG_TYPED_ARRAY => read_typed_array(bytes, cursor),
        TAG_TABLE => read_table(bytes, cursor),
        TAG_TENSOR => read_tensor(bytes, cursor),
        TAG_REFERENCE => Ok(SxValue::Reference(sx_core::ReferenceValue {
            target: read_string(bytes, cursor)?,
        })),
        TAG_BLOB_REF => {
            let uri = read_string(bytes, cursor)?;
            let media_type = if read_flag(bytes, cursor)? {
                Some(read_string(bytes, cursor)?)
            } else {
                None
            };
            let size = if read_flag(bytes, cursor)? {
                Some(decode_varint(bytes, cursor)?)
            } else {
                None
            };
            let hash = if read_flag(bytes, cursor)? {
                Some(read_bytes(bytes, cursor)?)
            } else {
                None
            };
            Ok(SxValue::BlobRef(sx_core::BlobRef {
                uri,
                media_type,
                size,
                hash,
            }))
        }
        TAG_DELTA => {
            let raw = read_string(bytes, cursor)?;
            match sx_text::parse_sx_text(&raw)? {
                SxValue::Delta(d) => Ok(SxValue::Delta(d)),
                _ => Err(SxError::new(
                    SxErrorCode::ValidationError,
                    "delta payload did not decode to delta value",
                )),
            }
        }
        _ => Err(SxError::new(
            SxErrorCode::ValidationError,
            format!("unknown type tag 0x{tag:02X}"),
        )),
    }
}

fn read_typed_array(bytes: &[u8], cursor: &mut usize) -> SxResult<SxValue> {
    let kind = *bytes
        .get(*cursor)
        .ok_or_else(|| SxError::new(SxErrorCode::UnexpectedEof, "typed-array kind eof"))?;
    *cursor += 1;
    let len = decode_varint(bytes, cursor)? as usize;
    let value = match kind {
        1 => {
            let end = checked_add(*cursor, len)?;
            let slice = bytes
                .get(*cursor..end)
                .ok_or_else(|| SxError::new(SxErrorCode::UnexpectedEof, "u8 typed array eof"))?;
            *cursor = end;
            SxTypedArray::U8(slice.to_vec())
        }
        2 => {
            let mut out = Vec::with_capacity(len);
            for _ in 0..len {
                let raw = read_fixed::<4>(bytes, *cursor)?;
                *cursor += 4;
                out.push(i32::from_le_bytes(raw));
            }
            SxTypedArray::I32(out)
        }
        3 => {
            let mut out = Vec::with_capacity(len);
            for _ in 0..len {
                let raw = read_fixed::<4>(bytes, *cursor)?;
                *cursor += 4;
                out.push(f32::from_le_bytes(raw));
            }
            SxTypedArray::F32(out)
        }
        4 => {
            let mut out = Vec::with_capacity(len);
            for _ in 0..len {
                let raw = read_fixed::<8>(bytes, *cursor)?;
                *cursor += 8;
                out.push(f64::from_le_bytes(raw));
            }
            SxTypedArray::F64(out)
        }
        5 => {
            let byte_len = (len + 7) / 8;
            let end = checked_add(*cursor, byte_len)?;
            let bits = bytes
                .get(*cursor..end)
                .ok_or_else(|| SxError::new(SxErrorCode::UnexpectedEof, "bool typed array eof"))?;
            *cursor = end;
            let mut out = Vec::with_capacity(len);
            for i in 0..len {
                out.push((bits[i / 8] & (1u8 << (i % 8))) != 0);
            }
            SxTypedArray::Bool(out)
        }
        _ => {
            return Err(SxError::new(
                SxErrorCode::ValidationError,
                format!("unknown typed-array kind {kind}"),
            ))
        }
    };
    Ok(SxValue::TypedArray(value))
}

fn read_table(bytes: &[u8], cursor: &mut usize) -> SxResult<SxValue> {
    let n = decode_varint(bytes, cursor)? as usize;
    let mut cols = std::collections::BTreeMap::new();
    for _ in 0..n {
        let name = read_string(bytes, cursor)?;
        let kind = *bytes
            .get(*cursor)
            .ok_or_else(|| SxError::new(SxErrorCode::UnexpectedEof, "table kind eof"))?;
        *cursor += 1;
        let value = read_value(bytes, cursor)?;
        let col = match (kind, value) {
            (1, SxValue::TypedArray(t)) => SxColumn::Typed(t),
            (2, SxValue::Array(v)) => SxColumn::Values(v),
            _ => {
                return Err(SxError::new(
                    SxErrorCode::TypeMismatch,
                    "invalid table column payload",
                ))
            }
        };
        with_decode_stats(|s| s.rows_materialized += col.len());
        cols.insert(name, col);
    }
    Ok(SxValue::Table(SxTable { columns: cols }))
}

fn read_tensor(bytes: &[u8], cursor: &mut usize) -> SxResult<SxValue> {
    let n = decode_varint(bytes, cursor)? as usize;
    let mut shape = Vec::with_capacity(n);
    for _ in 0..n {
        shape.push(decode_varint(bytes, cursor)? as usize);
    }
    let has_layout = read_flag(bytes, cursor)?;
    let layout = if has_layout {
        Some(read_string(bytes, cursor)?)
    } else {
        None
    };
    let data = match read_value(bytes, cursor)? {
        SxValue::TypedArray(t) => t,
        _ => {
            return Err(SxError::new(
                SxErrorCode::TypeMismatch,
                "tensor data must be typed array",
            ))
        }
    };
    Ok(SxValue::Tensor(SxTensor {
        shape,
        data,
        layout,
    }))
}

fn read_flag(bytes: &[u8], cursor: &mut usize) -> SxResult<bool> {
    let v = *bytes
        .get(*cursor)
        .ok_or_else(|| SxError::new(SxErrorCode::UnexpectedEof, "flag eof"))?;
    *cursor += 1;
    Ok(v != 0)
}

fn read_string(bytes: &[u8], cursor: &mut usize) -> SxResult<String> {
    with_decode_stats(|s| s.strings_decoded += 1);
    let raw = read_bytes(bytes, cursor)?;
    String::from_utf8(raw).map_err(|e| SxError::new(SxErrorCode::InvalidUtf8, e.to_string()))
}

fn read_bytes(bytes: &[u8], cursor: &mut usize) -> SxResult<Vec<u8>> {
    let len = decode_varint(bytes, cursor)? as usize;
    let end = checked_add(*cursor, len)?;
    let slice = bytes
        .get(*cursor..end)
        .ok_or_else(|| SxError::new(SxErrorCode::UnexpectedEof, "bytes eof"))?;
    *cursor = end;
    Ok(slice.to_vec())
}

fn checksum32(data: &[u8]) -> u32 {
    let digest = Sha256::digest(data);
    u32::from_le_bytes([digest[0], digest[1], digest[2], digest[3]])
}

fn read_fixed<const N: usize>(bytes: &[u8], offset: usize) -> SxResult<[u8; N]> {
    let end = checked_add(offset, N)?;
    let slice = bytes
        .get(offset..end)
        .ok_or_else(|| SxError::new(SxErrorCode::UnexpectedEof, "fixed-width eof"))?;
    let mut out = [0u8; N];
    out.copy_from_slice(slice);
    Ok(out)
}

fn checked_add(a: usize, b: usize) -> SxResult<usize> {
    a.checked_add(b)
        .ok_or_else(|| SxError::new(SxErrorCode::InvalidLength, "integer overflow"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn varint_roundtrip() {
        for v in [0, 1, 127, 128, 16384, u32::MAX as u64] {
            let mut out = Vec::new();
            encode_varint(v, &mut out);
            let mut c = 0;
            let back = decode_varint(&out, &mut c).unwrap();
            assert_eq!(v, back);
            assert_eq!(c, out.len());
        }
    }

    #[test]
    fn zigzag_roundtrip() {
        for v in [-100, -1, 0, 1, 7, i32::MAX as i64] {
            assert_eq!(zigzag_decode(zigzag_encode(v)), v);
        }
    }

    #[test]
    fn encode_decode_roundtrip() {
        let mut obj = BTreeMap::new();
        obj.insert("a".to_string(), SxValue::I64(1));
        obj.insert("b".to_string(), SxValue::String("x".to_string()));
        let v = SxValue::Object(obj);
        let b = encode_binary(&v, None, None).unwrap();
        let back = decode_binary(&b).unwrap();
        assert_eq!(v, back);
    }

    #[test]
    fn malformed_rejected() {
        let bad = b"SX\0\x01\x00\x05\x00\x00\x00".to_vec();
        let err = decode_binary(&bad).unwrap_err();
        assert!(matches!(err.code, SxErrorCode::InvalidLength));
    }

    #[test]
    fn columnar_event_batch_roundtrip() {
        let mut rows = Vec::new();
        for i in 0..128 {
            let mut obj = BTreeMap::new();
            obj.insert("tenant".to_string(), SxValue::String("acme".to_string()));
            obj.insert("type".to_string(), SxValue::String("click".to_string()));
            obj.insert("timestamp".to_string(), SxValue::I64(1_700_000_000_000 + i));
            obj.insert(
                "cold_payload".to_string(),
                SxValue::String(format!("blob-{}-{}", i, "x".repeat(64))),
            );
            rows.push(SxValue::Object(obj));
        }
        let value = SxValue::Array(rows);
        let encoded = encode_binary(&value, None, None).unwrap();
        let decoded = decode_binary(&encoded).unwrap();
        assert_eq!(decoded, value);
    }

    #[test]
    fn hot_field_fast_path_avoids_full_decode_and_cold_values() {
        let mut rows = Vec::new();
        for i in 0..128 {
            let mut obj = BTreeMap::new();
            obj.insert("tenant".to_string(), SxValue::String("acme".to_string()));
            obj.insert("timestamp".to_string(), SxValue::I64(1_700_000_000_000 + i));
            obj.insert(
                "cold_payload".to_string(),
                SxValue::String(format!("payload-{}-{}", i, "z".repeat(256))),
            );
            rows.push(SxValue::Object(obj));
        }
        let value = SxValue::Array(rows);
        let encoded = encode_binary(&value, None, None).unwrap();

        reset_decode_stats();
        let tenants = decode_hot_field_values(&encoded, "tenant").unwrap();
        let stats = current_decode_stats();
        assert_eq!(tenants.len(), 128);
        assert!(tenants
            .iter()
            .all(|v| matches!(v, SxValue::String(s) if s == "acme")));
        assert_eq!(stats.full_decode_calls, 0);
        assert_eq!(stats.rows_materialized, 0);
        assert_eq!(stats.cold_values_decoded, 0);
    }

    #[test]
    fn table_scan_fast_path_avoids_row_materialization() {
        let mut columns = BTreeMap::new();
        let mut id = Vec::new();
        let mut temp = Vec::new();
        let mut active = Vec::new();
        for i in 0..10_000 {
            id.push(SxValue::I64(i as i64));
            temp.push(SxValue::F64(20.0 + (i as f64 * 0.001)));
            active.push(SxValue::Bool(i % 2 == 0));
        }
        columns.insert("id".to_string(), SxColumn::Values(id));
        columns.insert("temp".to_string(), SxColumn::Values(temp));
        columns.insert("active".to_string(), SxColumn::Values(active));
        let table = SxValue::Table(SxTable { columns });
        let encoded = encode_binary(&table, None, None).unwrap();

        reset_decode_stats();
        let matched = scan_table_numeric_gt(&encoded, "temp", 25.0).unwrap();
        let stats = current_decode_stats();
        assert!(matched > 0);
        assert_eq!(stats.full_decode_calls, 0);
        assert_eq!(stats.rows_materialized, 0);
    }
}
