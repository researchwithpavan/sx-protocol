# Java Binding

Java binding via JNA.

## Requirements

- JDK 17+
- Maven 3.9+
- `SX_FFI_LIB` must point to built `sx-ffi` shared library

## Smoke Test

```bash
export SX_FFI_LIB=target/release/libsx_ffi.so
mvn test -f bindings/java/pom.xml
```
