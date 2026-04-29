# Python Binding

`ctypes` wrapper over `sx-ffi`.

Set `SX_FFI_LIB` to the compiled shared library path.

```bash
export SX_FFI_LIB=target/release/libsx_ffi.so
PYTHONPATH=bindings/python python3 -m pytest bindings/python/tests
```
