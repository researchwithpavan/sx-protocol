# SX Protocol (State eXchange Protocol)

SX is a schema-aware state exchange protocol with human-readable text (`.sx`) and compact binary (`.sxb`) representations.

## Quick Start

```bash
cargo build --release
cargo run -p sx-cli -- validate examples/basic.sx
cargo run -p sx-cli -- convert examples/basic.sx --to binary --out /tmp/basic.sxb
cargo run -p sx-cli -- convert /tmp/basic.sxb --to text --out /tmp/basic_roundtrip.sx
cargo run -p sx-cli -- hash examples/basic.sx
cargo run -p sx-cli -- benchmark --out benchmarks.csv
```

## Static Website

- Open `site/index.html` directly in a browser, or host `site/` on GitHub Pages/static hosting.

## Build

```bash
cargo fmt --check
cargo test
cargo build --release
```

## CI

Cross-platform CI is defined in `.github/workflows/ci.yml` and runs on Linux, macOS, and Windows:

- `cargo fmt --check`
- `cargo test`
- `cargo build --release`
- `cargo run -p sx-cli -- benchmark --out benchmarks.csv`
- C and C++ smoke compile/run against `sx-ffi`
- Python binding smoke test
- JS and Java smoke tests on Linux with provisioned toolchains

## CLI

```bash
sx validate <file>
sx fmt <file>
sx convert <input> --to text|binary|json --out <file>
sx inspect <file>
sx hash <file>
sx diff <base> <target> --out <delta>
sx patch <base> <delta> --out <target>
sx schema check <schema>
sx benchmark --out benchmarks.csv
```

## Bindings

- C ABI: `crates/sx-ffi` with header `bindings/c/sx.h`
- C++ RAII wrapper: `bindings/cpp/sx.hpp`
- Python (`ctypes`): `bindings/python`
- JavaScript (`ffi-napi`): `bindings/js`
- Java (`JNA`): `bindings/java`

## Testing Bindings

```bash
SX_FFI_LIB=target/release/libsx_ffi.so PYTHONPATH=bindings/python python3 -m pytest bindings/python/tests
npm test --prefix bindings/js
mvn test -f bindings/java/pom.xml
```

If a toolchain is unavailable, see `PROJECT_STATUS.md` for current environment limits.

## Alpha 2 Benchmarks

Current benchmark artifacts:

- `benchmarks.csv`
- `benchmarks.meta.json` (OS/CPU/rustc + reproducibility note)

Benchmark Results Are Single-Run Measurements.

Current run highlights:

- `sx_binary_decode_event_batch_1k`: `308.748 ops/s`
- `json_parse_event_batch_1k_baseline`: `307.920 ops/s`
- `sx_binary_decode_hot_fields_1k`: `1488.284 ops/s` (`full_decode_calls=0`)
- `sx_binary_table_scan_10k`: `338.754 ops/s` (`rows_materialized=0`)
