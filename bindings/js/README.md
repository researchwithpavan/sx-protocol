# JavaScript Binding

Node.js binding via `ffi-napi` to the C ABI.

## Requirements

- Node.js 20+ with npm
- Native addon toolchain for `ffi-napi` (`python3`, `make`, `g++` on Linux/macOS; MSVC Build Tools on Windows)
- `npm install --prefix bindings/js` before running tests
- Build path without spaces is recommended for `node-gyp` include-path reliability

## Smoke Test

```bash
export SX_FFI_LIB=target/release/libsx_ffi.so
npm install --prefix bindings/js
npm test --prefix bindings/js
```
