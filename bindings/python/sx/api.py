import ctypes
import os
from pathlib import Path


class SxError(Exception):
    pass


class _SxErrorInfo(ctypes.Structure):
    _fields_ = [("code", ctypes.c_int), ("message", ctypes.c_char_p)]


class _SxByteBuffer(ctypes.Structure):
    _fields_ = [("data", ctypes.POINTER(ctypes.c_uint8)), ("len", ctypes.c_size_t)]


def _load_lib():
    env = os.environ.get("SX_FFI_LIB")
    if env:
        return ctypes.CDLL(env)
    cwd = Path.cwd()
    candidates = []
    for root in [cwd, cwd / "..", cwd / "../.."]:
        base = root.resolve()
        candidates.extend(
            [
                base / "target/release/libsx_ffi.so",
                base / "target/release/libsx_ffi.dylib",
                base / "target/release/sx_ffi.dll",
            ]
        )
    for c in candidates:
        if c.exists():
            return ctypes.CDLL(str(c))
    raise RuntimeError("SX FFI library not found; set SX_FFI_LIB")


_lib = None


def _get_lib():
    global _lib
    if _lib is None:
        _lib = _load_lib()
        _configure_lib(_lib)
    return _lib


def _configure_lib(lib):
    lib.sx_message_parse_text.argtypes = [ctypes.c_char_p, ctypes.POINTER(ctypes.c_void_p), ctypes.POINTER(ctypes.c_void_p)]
    lib.sx_message_parse_text.restype = ctypes.c_int
    lib.sx_message_to_text.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_char_p), ctypes.POINTER(ctypes.c_void_p)]
    lib.sx_message_to_text.restype = ctypes.c_int
    lib.sx_message_encode_binary.argtypes = [ctypes.c_void_p, ctypes.POINTER(_SxByteBuffer), ctypes.POINTER(ctypes.c_void_p)]
    lib.sx_message_encode_binary.restype = ctypes.c_int
    lib.sx_message_decode_binary.argtypes = [ctypes.POINTER(ctypes.c_uint8), ctypes.c_size_t, ctypes.POINTER(ctypes.c_void_p), ctypes.POINTER(ctypes.c_void_p)]
    lib.sx_message_decode_binary.restype = ctypes.c_int
    lib.sx_hash_logical.argtypes = [ctypes.c_void_p, ctypes.POINTER(_SxByteBuffer), ctypes.POINTER(ctypes.c_void_p)]
    lib.sx_hash_logical.restype = ctypes.c_int
    lib.sx_value_get_field.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.POINTER(ctypes.c_void_p), ctypes.POINTER(ctypes.c_void_p)]
    lib.sx_value_get_field.restype = ctypes.c_int
    lib.sx_value_get_string.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_char_p), ctypes.POINTER(ctypes.c_void_p)]
    lib.sx_value_get_string.restype = ctypes.c_int
    lib.sx_message_free.argtypes = [ctypes.c_void_p]
    lib.sx_error_free.argtypes = [ctypes.c_void_p]
    lib.sx_string_free.argtypes = [ctypes.c_char_p]
    lib.sx_bytes_free.argtypes = [_SxByteBuffer]
    lib.sx_version.restype = ctypes.c_char_p


def sx_library_available() -> bool:
    try:
        _get_lib()
        return True
    except Exception:
        return False


class SxMessage:
    def __init__(self, handle):
        self._handle = handle

    @classmethod
    def parse_text(cls, text: str) -> "SxMessage":
        lib = _get_lib()
        out = ctypes.c_void_p()
        err = ctypes.c_void_p()
        status = lib.sx_message_parse_text(text.encode("utf-8"), ctypes.byref(out), ctypes.byref(err))
        if status != 0:
            _raise_error(lib, err)
        return cls(out)

    @classmethod
    def from_binary(cls, data: bytes) -> "SxMessage":
        lib = _get_lib()
        out = ctypes.c_void_p()
        err = ctypes.c_void_p()
        buf = (ctypes.c_uint8 * len(data)).from_buffer_copy(data)
        status = lib.sx_message_decode_binary(buf, len(data), ctypes.byref(out), ctypes.byref(err))
        if status != 0:
            _raise_error(lib, err)
        return cls(out)

    def to_text(self) -> str:
        lib = _get_lib()
        out = ctypes.c_char_p()
        err = ctypes.c_void_p()
        status = lib.sx_message_to_text(self._handle, ctypes.byref(out), ctypes.byref(err))
        if status != 0:
            _raise_error(lib, err)
        s = ctypes.string_at(out).decode("utf-8")
        lib.sx_string_free(out)
        return s

    def to_binary(self) -> bytes:
        lib = _get_lib()
        out = _SxByteBuffer()
        err = ctypes.c_void_p()
        status = lib.sx_message_encode_binary(self._handle, ctypes.byref(out), ctypes.byref(err))
        if status != 0:
            _raise_error(lib, err)
        data = bytes(ctypes.cast(out.data, ctypes.POINTER(ctypes.c_ubyte * out.len)).contents)
        lib.sx_bytes_free(out)
        return data

    def logical_hash(self) -> bytes:
        lib = _get_lib()
        out = _SxByteBuffer()
        err = ctypes.c_void_p()
        status = lib.sx_hash_logical(self._handle, ctypes.byref(out), ctypes.byref(err))
        if status != 0:
            _raise_error(lib, err)
        data = bytes(ctypes.cast(out.data, ctypes.POINTER(ctypes.c_ubyte * out.len)).contents)
        lib.sx_bytes_free(out)
        return data

    def field(self, name: str) -> "SxMessage":
        lib = _get_lib()
        out = ctypes.c_void_p()
        err = ctypes.c_void_p()
        status = lib.sx_value_get_field(self._handle, name.encode("utf-8"), ctypes.byref(out), ctypes.byref(err))
        if status != 0:
            _raise_error(lib, err)
        return SxMessage(out)

    def as_string(self) -> str:
        lib = _get_lib()
        out = ctypes.c_char_p()
        err = ctypes.c_void_p()
        status = lib.sx_value_get_string(self._handle, ctypes.byref(out), ctypes.byref(err))
        if status != 0:
            _raise_error(lib, err)
        s = ctypes.string_at(out).decode("utf-8")
        lib.sx_string_free(out)
        return s

    def close(self):
        if self._handle:
            _get_lib().sx_message_free(self._handle)
            self._handle = None

    def __del__(self):
        self.close()


def sx_version() -> str:
    return _get_lib().sx_version().decode("utf-8")


def _raise_error(lib, err_ptr):
    if not err_ptr:
        raise SxError("unknown SX error")
    info = ctypes.cast(err_ptr, ctypes.POINTER(_SxErrorInfo)).contents
    msg = info.message.decode("utf-8") if info.message else ""
    lib.sx_error_free(err_ptr)
    raise SxError(f"SX[{info.code}] {msg}")
