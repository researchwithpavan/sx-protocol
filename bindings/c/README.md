# C Binding

## Ownership Rules

- `SxMessage*` from SX APIs must be released with `sx_message_free`.
- `SxErrorInfo*` must be released with `sx_error_free`.
- strings from `sx_message_to_text` / `sx_value_get_string` must use `sx_string_free`.
- byte buffers from `sx_message_encode_binary` / `sx_value_get_bytes` / `sx_hash_logical` must use `sx_bytes_free`.

## Example Build (Linux/macOS)

```bash
cc -Ibindings/c bindings/c/examples/basic.c -Ltarget/release -lsx_ffi -o /tmp/sx_c_example
```

## Example Build (Windows MSVC)

```powershell
cl /I bindings\c bindings\c\examples\basic.c /link /LIBPATH:target\release sx_ffi.lib
```
