use rand::{rngs::StdRng, Rng, SeedableRng};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use sx_core::{SxErrorCode, SxValue};

#[derive(Clone, Copy, Debug)]
struct Entry {
    kind: u8,
    offset: usize,
    length: usize,
    checksum_pos: usize,
}

#[test]
fn malformed_segment_offset_out_of_bounds_rejected() {
    let value = SxValue::Object(BTreeMap::from([("x".to_string(), SxValue::I64(1))]));
    let mut bytes = sx_binary::encode_binary(&value, None, None).unwrap();
    let entries = parse_entries(&bytes);
    assert!(!entries.is_empty());
    let first_offset_pos = 11 + 4;
    bytes[first_offset_pos..first_offset_pos + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    let err = sx_binary::decode_binary(&bytes).unwrap_err();
    assert_eq!(err.code, SxErrorCode::SegmentOutOfBounds);
}

#[test]
fn malformed_invalid_total_length_rejected() {
    let value = SxValue::Object(BTreeMap::from([("x".to_string(), SxValue::I64(1))]));
    let mut bytes = sx_binary::encode_binary(&value, None, None).unwrap();
    let bad_len = (bytes.len() as u32).saturating_add(17);
    bytes[5..9].copy_from_slice(&bad_len.to_le_bytes());
    let err = sx_binary::decode_binary(&bytes).unwrap_err();
    assert_eq!(err.code, SxErrorCode::InvalidLength);
}

#[test]
fn malformed_invalid_type_tag_rejected() {
    let value = SxValue::Object(BTreeMap::from([("x".to_string(), SxValue::I64(1))]));
    let mut bytes = sx_binary::encode_binary(&value, None, None).unwrap();
    let entries = parse_entries(&bytes);
    let value_entry = entries.iter().find(|e| e.kind == 5).unwrap();
    bytes[value_entry.offset] = 0xFE;
    patch_checksum(&mut bytes, *value_entry);
    let err = sx_binary::decode_binary(&bytes).unwrap_err();
    assert_eq!(err.code, SxErrorCode::ValidationError);
}

#[test]
fn malformed_invalid_dictionary_ref_rejected() {
    let batch = build_columnar_event_batch();
    let mut bytes = sx_binary::encode_binary(&batch, None, None).unwrap();
    let entries = parse_entries(&bytes);
    let col_entry = entries.iter().find(|e| e.kind == 6).unwrap();
    let payload = &mut bytes[col_entry.offset..col_entry.offset + col_entry.length];
    let first_id_pos = locate_first_column_dict_id(payload).unwrap();
    payload[first_id_pos] = 2;
    patch_checksum(&mut bytes, *col_entry);
    let err = sx_binary::decode_binary(&bytes).unwrap_err();
    assert_eq!(err.code, SxErrorCode::InvalidDictionaryRef);
}

#[test]
fn malformed_invalid_shape_ref_rejected() {
    let value = SxValue::Object(BTreeMap::from([("x".to_string(), SxValue::I64(1))]));
    let mut bytes = sx_binary::encode_binary(&value, None, None).unwrap();
    let entries = parse_entries(&bytes);
    let shape_entry = entries.iter().find(|e| e.kind == 3).unwrap();
    let shape_start = shape_entry.offset;
    bytes[shape_start + 2] = 0xFF;
    patch_checksum(&mut bytes, *shape_entry);
    let err = sx_binary::decode_binary(&bytes).unwrap_err();
    assert_eq!(err.code, SxErrorCode::InvalidShapeRef);
}

#[test]
fn malformed_truncated_buffers_rejected() {
    let value = build_columnar_event_batch();
    let bytes = sx_binary::encode_binary(&value, None, None).unwrap();
    let mut rng = StdRng::seed_from_u64(1234);
    for _ in 0..64 {
        let keep = rng.gen_range(0..bytes.len());
        let truncated = &bytes[..keep];
        let err = sx_binary::decode_binary(truncated).unwrap_err();
        assert!(
            matches!(
                err.code,
                SxErrorCode::InvalidLength
                    | SxErrorCode::SegmentOutOfBounds
                    | SxErrorCode::UnexpectedEof
                    | SxErrorCode::InvalidMagic
            ),
            "unexpected error code {:?} for truncated length {}",
            err.code,
            keep
        );
    }
}

#[test]
fn malformed_integer_overflow_inputs_rejected() {
    let value = SxValue::Object(BTreeMap::from([("x".to_string(), SxValue::I64(1))]));
    let mut bytes = sx_binary::encode_binary(&value, None, None).unwrap();
    let table_offset = 11;
    bytes[table_offset + 8..table_offset + 12].copy_from_slice(&u32::MAX.to_le_bytes());
    let err = sx_binary::decode_binary(&bytes).unwrap_err();
    assert!(matches!(
        err.code,
        SxErrorCode::SegmentOutOfBounds | SxErrorCode::InvalidLength
    ));

    // Build a minimal single-segment container where value segment is TAG_U64 + overflowing varint.
    let overflow_payload = vec![
        0x10, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    ];
    let digest = Sha256::digest(&overflow_payload);
    let checksum = [digest[0], digest[1], digest[2], digest[3]];
    let total_len = (27 + overflow_payload.len()) as u32;
    let mut bytes2 = Vec::new();
    bytes2.extend_from_slice(b"SX\0");
    bytes2.push(1); // version
    bytes2.push(0); // flags
    bytes2.extend_from_slice(&total_len.to_le_bytes());
    bytes2.extend_from_slice(&1u16.to_le_bytes()); // one segment
    bytes2.extend_from_slice(&1u16.to_le_bytes()); // id
    bytes2.push(5); // ValueData
    bytes2.push(0); // codec
    bytes2.extend_from_slice(&27u32.to_le_bytes()); // offset
    bytes2.extend_from_slice(&(overflow_payload.len() as u32).to_le_bytes());
    bytes2.extend_from_slice(&checksum);
    bytes2.extend_from_slice(&overflow_payload);
    let err2 = sx_binary::decode_binary(&bytes2).unwrap_err();
    assert_eq!(err2.code, SxErrorCode::InvalidNumber);
}

fn parse_entries(bytes: &[u8]) -> Vec<Entry> {
    let mut cursor = 9usize;
    let segment_count = u16::from_le_bytes([bytes[cursor], bytes[cursor + 1]]) as usize;
    cursor += 2;
    let mut entries = Vec::with_capacity(segment_count);
    for _ in 0..segment_count {
        let kind = bytes[cursor + 2];
        let offset = u32::from_le_bytes([
            bytes[cursor + 4],
            bytes[cursor + 5],
            bytes[cursor + 6],
            bytes[cursor + 7],
        ]) as usize;
        let length = u32::from_le_bytes([
            bytes[cursor + 8],
            bytes[cursor + 9],
            bytes[cursor + 10],
            bytes[cursor + 11],
        ]) as usize;
        entries.push(Entry {
            kind,
            offset,
            length,
            checksum_pos: cursor + 12,
        });
        cursor += 16;
    }
    entries
}

fn patch_checksum(bytes: &mut [u8], entry: Entry) {
    let digest = Sha256::digest(&bytes[entry.offset..entry.offset + entry.length]);
    let checksum = [digest[0], digest[1], digest[2], digest[3]];
    bytes[entry.checksum_pos..entry.checksum_pos + 4].copy_from_slice(&checksum);
}

fn build_columnar_event_batch() -> SxValue {
    let mut rows = Vec::new();
    for i in 0..128 {
        let mut row = BTreeMap::new();
        row.insert("tenant".to_string(), SxValue::String("acme".to_string()));
        row.insert(
            "type".to_string(),
            SxValue::String(if i % 2 == 0 { "click" } else { "view" }.to_string()),
        );
        row.insert("timestamp".to_string(), SxValue::I64(1_700_000_000_000 + i));
        rows.push(SxValue::Object(row));
    }
    SxValue::Array(rows)
}

fn locate_first_column_dict_id(column_payload: &[u8]) -> Option<usize> {
    let mut cursor = 0usize;
    cursor += 1;
    let rows = read_varint(column_payload, &mut cursor)? as usize;
    let cols = read_varint(column_payload, &mut cursor)? as usize;
    for _ in 0..cols {
        let _field_id = read_varint(column_payload, &mut cursor)?;
        let name_len = read_varint(column_payload, &mut cursor)? as usize;
        cursor += name_len;
        let kind = *column_payload.get(cursor)?;
        cursor += 1;
        if kind == 1 {
            let dict_len = read_varint(column_payload, &mut cursor)? as usize;
            for _ in 0..dict_len {
                let l = read_varint(column_payload, &mut cursor)? as usize;
                cursor += l;
            }
            let ids_len = read_varint(column_payload, &mut cursor)? as usize;
            if ids_len != rows {
                return None;
            }
            return Some(cursor);
        }
        skip_column(column_payload, &mut cursor, kind)?;
    }
    None
}

fn skip_column(bytes: &[u8], cursor: &mut usize, kind: u8) -> Option<()> {
    match kind {
        1 => {
            let dict_len = read_varint(bytes, cursor)? as usize;
            for _ in 0..dict_len {
                let l = read_varint(bytes, cursor)? as usize;
                *cursor += l;
            }
            let ids_len = read_varint(bytes, cursor)? as usize;
            for _ in 0..ids_len {
                let _ = read_varint(bytes, cursor)?;
            }
        }
        2 | 4 => {
            let len = read_varint(bytes, cursor)? as usize;
            *cursor += len * 8;
        }
        3 => {
            let _len = read_varint(bytes, cursor)? as usize;
            let bytes_len = read_varint(bytes, cursor)? as usize;
            *cursor += bytes_len;
        }
        _ => return None,
    }
    Some(())
}

fn read_varint(bytes: &[u8], cursor: &mut usize) -> Option<u64> {
    let mut shift = 0u32;
    let mut out = 0u64;
    loop {
        let b = *bytes.get(*cursor)?;
        *cursor += 1;
        out |= ((b & 0x7F) as u64) << shift;
        if b & 0x80 == 0 {
            return Some(out);
        }
        shift += 7;
        if shift > 63 {
            return None;
        }
    }
}
