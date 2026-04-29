const ffi = require('ffi-napi');
const ref = require('ref-napi');
const StructDi = require('ref-struct-di');
const Struct = StructDi(ref);

const SxByteBuffer = Struct({
  data: ref.refType(ref.types.uint8),
  len: ref.types.size_t,
});

function loadLib() {
  const defaults = {
    win32: 'target/release/sx_ffi.dll',
    darwin: 'target/release/libsx_ffi.dylib',
    linux: 'target/release/libsx_ffi.so',
  };
  const lib = process.env.SX_FFI_LIB || defaults[process.platform] || defaults.linux;
  return ffi.Library(lib, {
    sx_message_parse_text: ['int', ['string', 'pointer', 'pointer']],
    sx_message_to_text: ['int', ['pointer', 'pointer', 'pointer']],
    sx_message_encode_binary: ['int', ['pointer', ref.refType(SxByteBuffer), 'pointer']],
    sx_hash_logical: ['int', ['pointer', ref.refType(SxByteBuffer), 'pointer']],
    sx_message_free: ['void', ['pointer']],
    sx_string_free: ['void', ['pointer']],
    sx_bytes_free: ['void', [SxByteBuffer]],
    sx_version: ['string', []],
  });
}

const lib = loadLib();

function sxVersion() {
  return lib.sx_version();
}

module.exports = { sxVersion, lib, SxByteBuffer };
