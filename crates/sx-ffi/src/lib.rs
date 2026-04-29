//! Stable C ABI for SX protocol operations.

use libc::{c_char, c_int, size_t};
use std::ffi::{CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};

use sx_core::{SxError, SxErrorCode, SxType, SxValue};

#[repr(C)]
pub struct SxMessage {
    value: SxValue,
}

#[repr(C)]
pub struct SxErrorInfo {
    pub code: c_int,
    pub message: *mut c_char,
}

#[repr(C)]
pub struct SxByteBuffer {
    pub data: *mut u8,
    pub len: size_t,
}

const SX_OK: c_int = 0;
const SX_ERR: c_int = 1;

fn code_to_int(code: &SxErrorCode) -> c_int {
    match code {
        SxErrorCode::InvalidMagic => 1001,
        SxErrorCode::UnsupportedVersion => 1002,
        SxErrorCode::InvalidLength => 1003,
        SxErrorCode::SegmentOutOfBounds => 1004,
        SxErrorCode::ChecksumFailed => 1005,
        SxErrorCode::SchemaNotFound => 1101,
        SxErrorCode::TypeMismatch => 1102,
        SxErrorCode::RequiredFieldMissing => 1103,
        SxErrorCode::InvalidFieldId => 1104,
        SxErrorCode::InvalidDictionaryRef => 1105,
        SxErrorCode::InvalidShapeRef => 1106,
        SxErrorCode::MessageTooLarge => 1201,
        SxErrorCode::UnsupportedFeature => 1202,
        SxErrorCode::InvalidPath => 1203,
        SxErrorCode::DuplicateKey => 1204,
        SxErrorCode::ParseError => 1301,
        SxErrorCode::ValidationError => 1302,
        SxErrorCode::InvalidUtf8 => 1303,
        SxErrorCode::InvalidNumber => 1304,
        SxErrorCode::UnexpectedEof => 1305,
        SxErrorCode::Internal => 1999,
    }
}

fn set_error(out_err: *mut *mut SxErrorInfo, err: SxError) {
    if out_err.is_null() {
        return;
    }
    unsafe {
        let msg = match CString::new(err.message) {
            Ok(s) => s,
            Err(_) => CString::from_vec_with_nul(vec![b'e', b'r', b'r', b'o', b'r', 0])
                .expect("valid fallback c string"),
        };
        let e = Box::new(SxErrorInfo {
            code: code_to_int(&err.code),
            message: msg.into_raw(),
        });
        *out_err = Box::into_raw(e);
    }
}

fn set_internal_error(out_err: *mut *mut SxErrorInfo, msg: &str) {
    set_error(
        out_err,
        SxError {
            code: SxErrorCode::Internal,
            message: msg.to_string(),
            path: None,
        },
    );
}

fn run_ffi<F>(out_err: *mut *mut SxErrorInfo, f: F) -> c_int
where
    F: FnOnce() -> Result<(), SxError>,
{
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(())) => SX_OK,
        Ok(Err(err)) => {
            set_error(out_err, err);
            SX_ERR
        }
        Err(_) => {
            set_internal_error(out_err, "panic across FFI boundary prevented");
            SX_ERR
        }
    }
}

fn cstr_to_string(ptr: *const c_char) -> Result<String, SxError> {
    if ptr.is_null() {
        return Err(SxError::new(
            SxErrorCode::ValidationError,
            "null string pointer",
        ));
    }
    let s = unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .map_err(|e| SxError::new(SxErrorCode::InvalidUtf8, e.to_string()))?;
    Ok(s.to_string())
}

fn message_ref<'a>(msg: *const SxMessage) -> Result<&'a SxMessage, SxError> {
    if msg.is_null() {
        return Err(SxError::new(
            SxErrorCode::ValidationError,
            "null message pointer",
        ));
    }
    Ok(unsafe { &*msg })
}

/// Parses SX text into a message handle.
#[no_mangle]
pub extern "C" fn sx_message_parse_text(
    input: *const c_char,
    out_msg: *mut *mut SxMessage,
    out_err: *mut *mut SxErrorInfo,
) -> c_int {
    run_ffi(out_err, || {
        if out_msg.is_null() {
            return Err(SxError::new(
                SxErrorCode::ValidationError,
                "out_msg is null",
            ));
        }
        let input = cstr_to_string(input)?;
        let value = sx_text::parse_sx_text(&input)?;
        unsafe {
            *out_msg = Box::into_raw(Box::new(SxMessage { value }));
        }
        Ok(())
    })
}

/// Converts message value to SX text.
#[no_mangle]
pub extern "C" fn sx_message_to_text(
    msg: *const SxMessage,
    out_text: *mut *mut c_char,
    out_err: *mut *mut SxErrorInfo,
) -> c_int {
    run_ffi(out_err, || {
        if out_text.is_null() {
            return Err(SxError::new(
                SxErrorCode::ValidationError,
                "out_text is null",
            ));
        }
        let msg = message_ref(msg)?;
        let text = sx_text::format_value(&msg.value);
        let c = CString::new(text)
            .map_err(|_| SxError::new(SxErrorCode::Internal, "text contains NUL"))?;
        unsafe {
            *out_text = c.into_raw();
        }
        Ok(())
    })
}

/// Encodes message to SX binary.
#[no_mangle]
pub extern "C" fn sx_message_encode_binary(
    msg: *const SxMessage,
    out_buf: *mut SxByteBuffer,
    out_err: *mut *mut SxErrorInfo,
) -> c_int {
    run_ffi(out_err, || {
        if out_buf.is_null() {
            return Err(SxError::new(
                SxErrorCode::ValidationError,
                "out_buf is null",
            ));
        }
        let msg = message_ref(msg)?;
        let encoded = sx_binary::encode_binary(&msg.value, None, None)?;
        let mut boxed = encoded.into_boxed_slice();
        let ptr = boxed.as_mut_ptr();
        let len = boxed.len();
        std::mem::forget(boxed);
        unsafe {
            (*out_buf).data = ptr;
            (*out_buf).len = len;
        }
        Ok(())
    })
}

/// Decodes SX binary into a message handle.
#[no_mangle]
pub extern "C" fn sx_message_decode_binary(
    data: *const u8,
    len: size_t,
    out_msg: *mut *mut SxMessage,
    out_err: *mut *mut SxErrorInfo,
) -> c_int {
    run_ffi(out_err, || {
        if out_msg.is_null() {
            return Err(SxError::new(
                SxErrorCode::ValidationError,
                "out_msg is null",
            ));
        }
        if data.is_null() {
            return Err(SxError::new(
                SxErrorCode::ValidationError,
                "data pointer is null",
            ));
        }
        let bytes = unsafe { std::slice::from_raw_parts(data, len) };
        let value = sx_binary::decode_binary(bytes)?;
        unsafe {
            *out_msg = Box::into_raw(Box::new(SxMessage { value }));
        }
        Ok(())
    })
}

/// Frees message handle.
#[no_mangle]
pub extern "C" fn sx_message_free(msg: *mut SxMessage) {
    if !msg.is_null() {
        unsafe {
            drop(Box::from_raw(msg));
        }
    }
}

/// Frees error object.
#[no_mangle]
pub extern "C" fn sx_error_free(err: *mut SxErrorInfo) {
    if err.is_null() {
        return;
    }
    unsafe {
        let err_box = Box::from_raw(err);
        if !err_box.message.is_null() {
            drop(CString::from_raw(err_box.message));
        }
    }
}

/// Frees C string returned by SX APIs.
#[no_mangle]
pub extern "C" fn sx_string_free(text: *mut c_char) {
    if !text.is_null() {
        unsafe {
            drop(CString::from_raw(text));
        }
    }
}

/// Frees binary buffer returned by SX APIs.
#[no_mangle]
pub extern "C" fn sx_bytes_free(buf: SxByteBuffer) {
    if !buf.data.is_null() && buf.len > 0 {
        unsafe {
            let _ = Box::from_raw(std::slice::from_raw_parts_mut(buf.data, buf.len));
        }
    }
}

/// Returns type code for root value.
#[no_mangle]
pub extern "C" fn sx_value_get_type(msg: *const SxMessage) -> c_int {
    let Ok(msg) = message_ref(msg) else {
        return -1;
    };
    sx_type_code(msg.value.sx_type())
}

fn sx_type_code(ty: SxType) -> c_int {
    match ty {
        SxType::Null => 0,
        SxType::Bool => 1,
        SxType::U8 | SxType::U16 | SxType::U32 | SxType::U64 => 2,
        SxType::I8 | SxType::I16 | SxType::I32 | SxType::I64 => 3,
        SxType::F32 | SxType::F64 => 4,
        SxType::Decimal => 5,
        SxType::String => 6,
        SxType::Bytes => 7,
        SxType::Array => 8,
        SxType::Object => 9,
        SxType::Map => 10,
        SxType::Enum => 11,
        SxType::Uuid => 12,
        SxType::Timestamp => 13,
        SxType::Date => 14,
        SxType::Duration => 15,
        SxType::TypedArray => 16,
        SxType::Table => 17,
        SxType::Tensor => 18,
        SxType::Reference => 19,
        SxType::BlobRef => 20,
        SxType::Delta => 21,
        SxType::Message => 22,
    }
}

/// Gets object field by name as a new message handle.
#[no_mangle]
pub extern "C" fn sx_value_get_field(
    msg: *const SxMessage,
    field: *const c_char,
    out_msg: *mut *mut SxMessage,
    out_err: *mut *mut SxErrorInfo,
) -> c_int {
    run_ffi(out_err, || {
        if out_msg.is_null() {
            return Err(SxError::new(
                SxErrorCode::ValidationError,
                "out_msg is null",
            ));
        }
        let key = cstr_to_string(field)?;
        let msg = message_ref(msg)?;
        let val = msg
            .value
            .get_field(&key)
            .ok_or_else(|| {
                SxError::new(SxErrorCode::InvalidPath, format!("field '{key}' not found"))
            })?
            .clone();
        unsafe {
            *out_msg = Box::into_raw(Box::new(SxMessage { value: val }));
        }
        Ok(())
    })
}

/// Gets string value.
#[no_mangle]
pub extern "C" fn sx_value_get_string(
    msg: *const SxMessage,
    out_text: *mut *mut c_char,
    out_err: *mut *mut SxErrorInfo,
) -> c_int {
    run_ffi(out_err, || {
        if out_text.is_null() {
            return Err(SxError::new(
                SxErrorCode::ValidationError,
                "out_text is null",
            ));
        }
        let msg = message_ref(msg)?;
        let s = match &msg.value {
            SxValue::String(v)
            | SxValue::Enum(v)
            | SxValue::Timestamp(v)
            | SxValue::Date(v)
            | SxValue::Duration(v) => v,
            SxValue::Url(v) | SxValue::Email(v) => v,
            _ => {
                return Err(SxError::new(
                    SxErrorCode::TypeMismatch,
                    "value is not string-like",
                ))
            }
        };
        let c = CString::new(s.clone())
            .map_err(|_| SxError::new(SxErrorCode::Internal, "string contains NUL"))?;
        unsafe {
            *out_text = c.into_raw();
        }
        Ok(())
    })
}

/// Gets i64 value.
#[no_mangle]
pub extern "C" fn sx_value_get_i64(msg: *const SxMessage, out_value: *mut i64) -> c_int {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if out_value.is_null() {
            return Err(SxError::new(
                SxErrorCode::ValidationError,
                "out_value is null",
            ));
        }
        let msg = message_ref(msg)?;
        let v = match &msg.value {
            SxValue::I64(v) => *v,
            SxValue::I32(v) => *v as i64,
            SxValue::I16(v) => *v as i64,
            SxValue::I8(v) => *v as i64,
            _ => {
                return Err(SxError::new(
                    SxErrorCode::TypeMismatch,
                    "value is not signed integer",
                ))
            }
        };
        unsafe { *out_value = v };
        Ok(())
    }));
    if matches!(result, Ok(Ok(()))) {
        SX_OK
    } else {
        SX_ERR
    }
}

/// Gets u64 value.
#[no_mangle]
pub extern "C" fn sx_value_get_u64(msg: *const SxMessage, out_value: *mut u64) -> c_int {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if out_value.is_null() {
            return Err(SxError::new(
                SxErrorCode::ValidationError,
                "out_value is null",
            ));
        }
        let msg = message_ref(msg)?;
        let v = match &msg.value {
            SxValue::U64(v) => *v,
            SxValue::U32(v) => *v as u64,
            SxValue::U16(v) => *v as u64,
            SxValue::U8(v) => *v as u64,
            _ => {
                return Err(SxError::new(
                    SxErrorCode::TypeMismatch,
                    "value is not unsigned integer",
                ))
            }
        };
        unsafe { *out_value = v };
        Ok(())
    }));
    if matches!(result, Ok(Ok(()))) {
        SX_OK
    } else {
        SX_ERR
    }
}

/// Gets bool value.
#[no_mangle]
pub extern "C" fn sx_value_get_bool(msg: *const SxMessage, out_value: *mut bool) -> c_int {
    let result = catch_unwind(AssertUnwindSafe(|| {
        if out_value.is_null() {
            return Err(SxError::new(
                SxErrorCode::ValidationError,
                "out_value is null",
            ));
        }
        let msg = message_ref(msg)?;
        let SxValue::Bool(v) = &msg.value else {
            return Err(SxError::new(SxErrorCode::TypeMismatch, "value is not bool"));
        };
        unsafe { *out_value = *v };
        Ok(())
    }));
    if matches!(result, Ok(Ok(()))) {
        SX_OK
    } else {
        SX_ERR
    }
}

/// Gets bytes value.
#[no_mangle]
pub extern "C" fn sx_value_get_bytes(
    msg: *const SxMessage,
    out_buf: *mut SxByteBuffer,
    out_err: *mut *mut SxErrorInfo,
) -> c_int {
    run_ffi(out_err, || {
        if out_buf.is_null() {
            return Err(SxError::new(
                SxErrorCode::ValidationError,
                "out_buf is null",
            ));
        }
        let msg = message_ref(msg)?;
        let bytes = match &msg.value {
            SxValue::Bytes(b) => b.clone(),
            _ => {
                return Err(SxError::new(
                    SxErrorCode::TypeMismatch,
                    "value is not bytes",
                ))
            }
        };
        let mut boxed = bytes.into_boxed_slice();
        let ptr = boxed.as_mut_ptr();
        let len = boxed.len();
        std::mem::forget(boxed);
        unsafe {
            (*out_buf).data = ptr;
            (*out_buf).len = len;
        }
        Ok(())
    })
}

/// Computes canonical logical hash.
#[no_mangle]
pub extern "C" fn sx_hash_logical(
    msg: *const SxMessage,
    out_buf: *mut SxByteBuffer,
    out_err: *mut *mut SxErrorInfo,
) -> c_int {
    run_ffi(out_err, || {
        if out_buf.is_null() {
            return Err(SxError::new(
                SxErrorCode::ValidationError,
                "out_buf is null",
            ));
        }
        let msg = message_ref(msg)?;
        let hash = sx_crypto::logical_hash(&msg.value)?;
        let mut boxed = hash.to_vec().into_boxed_slice();
        let ptr = boxed.as_mut_ptr();
        let len = boxed.len();
        std::mem::forget(boxed);
        unsafe {
            (*out_buf).data = ptr;
            (*out_buf).len = len;
        }
        Ok(())
    })
}

/// Applies delta document to base message.
#[no_mangle]
pub extern "C" fn sx_apply_delta(
    base_msg: *const SxMessage,
    delta_msg: *const SxMessage,
    out_msg: *mut *mut SxMessage,
    out_err: *mut *mut SxErrorInfo,
) -> c_int {
    run_ffi(out_err, || {
        if out_msg.is_null() {
            return Err(SxError::new(
                SxErrorCode::ValidationError,
                "out_msg is null",
            ));
        }
        let base = message_ref(base_msg)?;
        let delta = message_ref(delta_msg)?;
        let SxValue::Delta(delta_doc) = &delta.value else {
            return Err(SxError::new(
                SxErrorCode::TypeMismatch,
                "delta message must contain delta value",
            ));
        };
        let value = sx_delta::apply_delta(&base.value, delta_doc)?;
        unsafe {
            *out_msg = Box::into_raw(Box::new(SxMessage { value }));
        }
        Ok(())
    })
}

/// Returns protocol version string.
#[no_mangle]
pub extern "C" fn sx_version() -> *const c_char {
    static VERSION: &[u8] = b"SX Protocol v1\0";
    VERSION.as_ptr() as *const c_char
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr;

    #[test]
    fn ffi_parse_encode_decode() {
        let input = CString::new("{a:1}").unwrap();
        let mut msg: *mut SxMessage = ptr::null_mut();
        let mut err: *mut SxErrorInfo = ptr::null_mut();
        assert_eq!(
            sx_message_parse_text(input.as_ptr(), &mut msg, &mut err),
            SX_OK
        );
        assert!(!msg.is_null());
        let mut bin = SxByteBuffer {
            data: ptr::null_mut(),
            len: 0,
        };
        assert_eq!(sx_message_encode_binary(msg, &mut bin, &mut err), SX_OK);
        assert!(bin.len > 0);
        let mut decoded: *mut SxMessage = ptr::null_mut();
        assert_eq!(
            sx_message_decode_binary(bin.data, bin.len, &mut decoded, &mut err),
            SX_OK
        );
        sx_message_free(msg);
        sx_message_free(decoded);
        sx_bytes_free(bin);
        if !err.is_null() {
            sx_error_free(err);
        }
    }
}
