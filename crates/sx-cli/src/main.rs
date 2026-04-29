use clap::{Args, Parser, Subcommand, ValueEnum};
use rand::{rngs::StdRng, Rng, SeedableRng};
use serde_json::json;
use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use sx_core::{DeltaDocument, DeltaOp, DeltaOpKind, SxPath, SxValue};
use sx_runtime::{NumericOp, SxMessageView, SxTableView};

struct CountingAllocator;

static CURRENT_ALLOC: AtomicUsize = AtomicUsize::new(0);
static PEAK_ALLOC: AtomicUsize = AtomicUsize::new(0);

#[global_allocator]
static GLOBAL_ALLOCATOR: CountingAllocator = CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            let cur = CURRENT_ALLOC.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            let mut peak = PEAK_ALLOC.load(Ordering::Relaxed);
            while cur > peak {
                match PEAK_ALLOC.compare_exchange_weak(
                    peak,
                    cur,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => break,
                    Err(actual) => peak = actual,
                }
            }
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        let size = layout.size();
        let _ = CURRENT_ALLOC.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
            Some(v.saturating_sub(size))
        });
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = unsafe { System.realloc(ptr, layout, new_size) };
        if !new_ptr.is_null() {
            let old = layout.size();
            if new_size >= old {
                let diff = new_size - old;
                let cur = CURRENT_ALLOC.fetch_add(diff, Ordering::Relaxed) + diff;
                let mut peak = PEAK_ALLOC.load(Ordering::Relaxed);
                while cur > peak {
                    match PEAK_ALLOC.compare_exchange_weak(
                        peak,
                        cur,
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                    ) {
                        Ok(_) => break,
                        Err(actual) => peak = actual,
                    }
                }
            } else {
                let diff = old - new_size;
                let _ = CURRENT_ALLOC.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
                    Some(v.saturating_sub(diff))
                });
            }
        }
        new_ptr
    }
}

fn reset_alloc_stats() {
    CURRENT_ALLOC.store(0, Ordering::Relaxed);
    PEAK_ALLOC.store(0, Ordering::Relaxed);
}

fn peak_alloc_bytes() -> usize {
    PEAK_ALLOC.load(Ordering::Relaxed)
}

#[derive(Parser, Debug)]
#[command(name = "sx", version = "0.1.0", about = "SX Protocol CLI")]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Validate(FileArg),
    Fmt(FileArg),
    Convert(ConvertArgs),
    Inspect(FileArg),
    Hash(FileArg),
    Diff(DiffArgs),
    Patch(PatchArgs),
    Schema(SchemaCommand),
    Benchmark(BenchmarkArgs),
}

#[derive(Args, Debug)]
struct FileArg {
    file: String,
}

#[derive(Args, Debug)]
struct ConvertArgs {
    input: String,
    #[arg(long)]
    to: ConvertTarget,
    #[arg(long)]
    out: String,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum ConvertTarget {
    Text,
    Binary,
    Json,
}

#[derive(Args, Debug)]
struct DiffArgs {
    base: String,
    target: String,
    #[arg(long)]
    out: String,
}

#[derive(Args, Debug)]
struct PatchArgs {
    base: String,
    delta: String,
    #[arg(long)]
    out: String,
}

#[derive(Subcommand, Debug)]
enum SchemaSub {
    Check { schema: String },
}

#[derive(Args, Debug)]
struct SchemaCommand {
    #[command(subcommand)]
    sub: SchemaSub,
}

#[derive(Args, Debug)]
struct BenchmarkArgs {
    #[arg(long)]
    out: String,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {}", err);
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.cmd {
        Command::Validate(FileArg { file }) => {
            let _ = load_value(&file)?;
            println!("ok");
        }
        Command::Fmt(FileArg { file }) => {
            let value = load_value(&file)?;
            let formatted = sx_text::format_canonical(&value);
            println!("{formatted}");
        }
        Command::Convert(args) => convert(args)?,
        Command::Inspect(FileArg { file }) => inspect(&file)?,
        Command::Hash(FileArg { file }) => {
            let value = load_value(&file)?;
            let hash = sx_crypto::logical_hash(&value)?;
            println!("{}", hex::encode(hash));
        }
        Command::Diff(args) => diff_cmd(args)?,
        Command::Patch(args) => patch_cmd(args)?,
        Command::Schema(SchemaCommand { sub }) => match sub {
            SchemaSub::Check { schema } => {
                let text = fs::read_to_string(schema)?;
                let parsed = sx_schema::parse_schema(&text)?;
                let hash = sx_schema::schema_hash(&parsed);
                println!("schema: {} v{}", parsed.name, parsed.version);
                println!("hash: {}", hex::encode(hash));
            }
        },
        Command::Benchmark(args) => benchmark_cmd(&args.out)?,
    }
    Ok(())
}

fn convert(args: ConvertArgs) -> Result<(), Box<dyn std::error::Error>> {
    let value = load_value(&args.input)?;
    match args.to {
        ConvertTarget::Text => {
            fs::write(args.out, sx_text::format_canonical(&value))?;
        }
        ConvertTarget::Binary => {
            fs::write(args.out, sx_binary::encode_binary(&value, None, None)?)?;
        }
        ConvertTarget::Json => {
            let j = sx_core::json::sx_to_json(&value);
            fs::write(args.out, serde_json::to_string_pretty(&j)?)?;
        }
    }
    Ok(())
}

fn inspect(file: &str) -> Result<(), Box<dyn std::error::Error>> {
    let value = load_value(file)?;
    println!("type: {:?}", value.sx_type());
    match &value {
        SxValue::Object(map) => println!("fields: {}", map.len()),
        SxValue::Array(items) => println!("items: {}", items.len()),
        SxValue::Table(table) => println!(
            "table rows: {}, columns: {}",
            table.row_count(),
            table.columns.len()
        ),
        _ => {}
    }
    Ok(())
}

fn diff_cmd(args: DiffArgs) -> Result<(), Box<dyn std::error::Error>> {
    let base = load_value(&args.base)?;
    let target = load_value(&args.target)?;
    let delta = sx_delta::diff(&base, &target);
    let out = sx_text::format_canonical(&SxValue::Delta(delta));
    fs::write(args.out, out)?;
    Ok(())
}

fn patch_cmd(args: PatchArgs) -> Result<(), Box<dyn std::error::Error>> {
    let base = load_value(&args.base)?;
    let delta_val = load_value(&args.delta)?;
    let SxValue::Delta(delta) = delta_val else {
        return Err("delta file must contain delta value".into());
    };
    let patched = sx_delta::apply_delta(&base, &delta)?;
    write_value(&args.out, &patched)?;
    Ok(())
}

fn load_value(path: &str) -> Result<SxValue, Box<dyn std::error::Error>> {
    if path == "-" {
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf)?;
        return Ok(sx_text::parse_sx_text(&buf)?);
    }
    let data = fs::read(path)?;
    if is_binary(path, &data) {
        Ok(sx_binary::decode_binary(&data)?)
    } else {
        let text = String::from_utf8(data)?;
        Ok(sx_text::parse_sx_text(&text)?)
    }
}

fn write_value(path: &str, value: &SxValue) -> Result<(), Box<dyn std::error::Error>> {
    if path == "-" {
        let txt = sx_text::format_canonical(value);
        io::stdout().write_all(txt.as_bytes())?;
        return Ok(());
    }
    if path.ends_with(".sxb") {
        fs::write(path, sx_binary::encode_binary(value, None, None)?)?;
    } else if path.ends_with(".json") {
        fs::write(
            path,
            serde_json::to_string_pretty(&sx_core::json::sx_to_json(value))?,
        )?;
    } else {
        fs::write(path, sx_text::format_canonical(value))?;
    }
    Ok(())
}

fn is_binary(path: &str, data: &[u8]) -> bool {
    path.ends_with(".sxb") || data.starts_with(b"SX\0")
}

fn benchmark_cmd(out_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let datasets = benchmark_datasets();
    let mut rows = Vec::new();

    rows.push(bench_text_parse(
        "sx_text_parse_small",
        &datasets.small_user,
        2000,
    ));
    rows.push(bench_text_parse(
        "sx_text_parse_medium",
        &datasets.order_created,
        1500,
    ));
    rows.push(bench_binary_encode(
        "sx_binary_encode_small",
        &datasets.small_user_value,
        4000,
    ));
    rows.push(bench_binary_decode(
        "sx_binary_decode_small",
        &datasets.small_user_value,
        4000,
    ));
    rows.push(bench_binary_encode(
        "sx_binary_encode_event_batch_1k",
        &datasets.event_batch_1k_value,
        200,
    ));
    rows.push(bench_binary_decode(
        "sx_binary_decode_event_batch_1k",
        &datasets.event_batch_1k_value,
        200,
    ));
    rows.push(bench_binary_decode(
        "sx_binary_decode_event_batch_1k_full_materialization",
        &datasets.event_batch_1k_value,
        200,
    ));
    rows.push(bench_hot_fields(
        "sx_binary_decode_hot_fields_1k",
        &datasets.event_batch_1k_value,
        300,
    ));
    rows.push(bench_hot_fields_full_decode(
        "sx_binary_decode_hot_fields_1k_full_materialization",
        &datasets.event_batch_1k_value,
        300,
    ));
    rows.push(bench_table_scan(
        "sx_binary_table_scan_10k",
        &datasets.event_batch_10k_table,
        80,
    ));
    rows.push(bench_table_scan_full_materialization(
        "sx_binary_table_scan_10k_full_materialization",
        &datasets.event_batch_10k_table,
        80,
    ));
    rows.push(bench_hash(
        "sx_logical_hash_small",
        &datasets.small_user_value,
        3000,
    ));
    rows.push(bench_delta_apply(
        "sx_delta_apply_small",
        &datasets.small_user_value,
        2000,
    ));
    rows.push(bench_json_parse(
        "json_parse_small_baseline",
        &datasets.small_user_json,
        3000,
    ));
    rows.push(bench_json_parse(
        "json_parse_event_batch_1k_baseline",
        &datasets.event_batch_1k_json,
        200,
    ));
    rows.push(bench_json_size(
        "json_size_event_batch_1k_baseline",
        &datasets.event_batch_1k_json,
    ));
    rows.push(bench_sx_size(
        "sx_size_event_batch_1k",
        &datasets.event_batch_1k_value,
    ));

    let mut csv = String::from("implementation,operation,dataset,records,bytes_input,bytes_output,iterations,total_ms,ops_per_sec,mb_per_sec,peak_memory_bytes,notes\n");
    for row in rows {
        csv.push_str(&format!(
            "{},{},{},{},{},{},{},{:.3},{:.3},{:.3},{},{}\n",
            row.implementation,
            row.operation,
            row.dataset,
            row.records,
            row.bytes_input,
            row.bytes_output,
            row.iterations,
            row.total_ms,
            row.ops_per_sec,
            row.mb_per_sec,
            row.peak_memory_bytes,
            row.notes
        ));
    }

    fs::write(out_path, csv)?;
    write_benchmark_metadata(out_path)?;
    println!("wrote {}", out_path);
    Ok(())
}

fn write_benchmark_metadata(out_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let out = std::path::Path::new(out_path);
    let file_name = out
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("benchmarks.csv");
    let meta_name = if file_name.ends_with(".csv") {
        format!("{}.meta.json", file_name.trim_end_matches(".csv"))
    } else {
        format!("{file_name}.meta.json")
    };
    let meta_path = out
        .parent()
        .map(|p| p.join(meta_name))
        .unwrap_or_else(|| std::path::PathBuf::from("benchmarks.meta.json"));

    let rustc = std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let rustc_verbose = std::process::Command::new("rustc")
        .args(["--version", "--verbose"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_else(|| "unknown".to_string());
    let cpu = detect_cpu_model();

    let meta = json!({
        "generated_at_utc": chrono_like_now_utc(),
        "benchmark_csv": out_path,
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "family": std::env::consts::FAMILY,
        "cpu_model": cpu,
        "rustc_version": rustc,
        "rustc_verbose": rustc_verbose,
        "notes": "Benchmark results are sensitive to CPU frequency scaling, thermal state, and background load. Run on an isolated machine with fixed governor for reproducibility."
    });
    fs::write(meta_path, serde_json::to_string_pretty(&meta)?)?;
    Ok(())
}

fn detect_cpu_model() -> String {
    if let Ok(v) = std::env::var("PROCESSOR_IDENTIFIER") {
        if !v.trim().is_empty() {
            return v;
        }
    }
    if let Ok(text) = fs::read_to_string("/proc/cpuinfo") {
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("model name\t: ") {
                return rest.to_string();
            }
        }
    }
    "unknown".to_string()
}

fn chrono_like_now_utc() -> String {
    let now = std::time::SystemTime::now();
    let secs = now
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

struct BenchData {
    small_user: String,
    order_created: String,
    small_user_value: SxValue,
    event_batch_1k_value: SxValue,
    event_batch_10k_table: SxValue,
    small_user_json: String,
    event_batch_1k_json: String,
}

fn benchmark_datasets() -> BenchData {
    let small_user = r#"{id: uuid("018f4b5e-7a24-7c8a-b28d-3f951a1b7f13"), name: "Asha", active: true, created_at: timestamp("2026-04-29T12:00:00Z")}"#.to_string();
    let small_user_value = sx_text::parse_sx_text(&small_user).unwrap();

    let order_created = r#"message OrderCreated { order_id: uuid("018f4b5e-7a24-7c8a-b28d-3f951a1b7001"), customer_id: uuid("018f4b5e-7a24-7c8a-b28d-3f951a1b7002"), total: money("INR", 4999, scale: 2), created_at: timestamp("2026-04-29T12:00:00Z") }"#.to_string();

    let mut rng = StdRng::seed_from_u64(42);
    let event_batch_1k_value = generate_event_batch(1_000, &mut rng);
    let event_batch_10k_table = generate_table(10_000, &mut rng);

    let small_user_json =
        serde_json::to_string(&sx_core::json::sx_to_json(&small_user_value)).unwrap();
    let event_batch_1k_json =
        serde_json::to_string(&sx_core::json::sx_to_json(&event_batch_1k_value)).unwrap();

    BenchData {
        small_user,
        order_created,
        small_user_value,
        event_batch_1k_value,
        event_batch_10k_table,
        small_user_json,
        event_batch_1k_json,
    }
}

fn generate_event_batch(n: usize, rng: &mut StdRng) -> SxValue {
    let mut events = Vec::with_capacity(n);
    for i in 0..n {
        let mut obj = BTreeMap::new();
        obj.insert("tenant".to_string(), SxValue::String("acme".to_string()));
        obj.insert(
            "type".to_string(),
            SxValue::String(if i % 3 == 0 { "click" } else { "view" }.to_string()),
        );
        obj.insert(
            "timestamp".to_string(),
            SxValue::I64(1_714_392_000_000 + i as i64),
        );
        obj.insert(
            "user_id".to_string(),
            SxValue::String(format!("u-{}", rng.gen::<u32>())),
        );
        obj.insert("active".to_string(), SxValue::Bool(i % 2 == 0));
        events.push(SxValue::Object(obj));
    }
    SxValue::Array(events)
}

fn generate_table(n: usize, rng: &mut StdRng) -> SxValue {
    let mut cols = BTreeMap::new();
    let mut id_col = Vec::with_capacity(n);
    let mut temp_col = Vec::with_capacity(n);
    let mut active_col = Vec::with_capacity(n);
    for i in 0..n {
        id_col.push(SxValue::I64(i as i64));
        temp_col.push(SxValue::F64(20.0 + (rng.gen::<f64>() * 10.0)));
        active_col.push(SxValue::Bool(i % 2 == 0));
    }
    cols.insert("id".to_string(), sx_core::SxColumn::Values(id_col));
    cols.insert("temp".to_string(), sx_core::SxColumn::Values(temp_col));
    cols.insert("active".to_string(), sx_core::SxColumn::Values(active_col));
    SxValue::Table(sx_core::SxTable { columns: cols })
}

struct BenchRow {
    implementation: String,
    operation: String,
    dataset: String,
    records: usize,
    bytes_input: usize,
    bytes_output: usize,
    iterations: usize,
    total_ms: f64,
    ops_per_sec: f64,
    mb_per_sec: f64,
    peak_memory_bytes: usize,
    notes: String,
}

fn bench_text_parse(name: &str, text: &str, iters: usize) -> BenchRow {
    reset_alloc_stats();
    let start = Instant::now();
    for _ in 0..iters {
        let _ = sx_text::parse_sx_text(text).unwrap();
    }
    let total_ms = start.elapsed().as_secs_f64() * 1000.0;
    make_row(
        "rust",
        name,
        "text",
        text.len(),
        1,
        text.len(),
        0,
        iters,
        total_ms,
        peak_alloc_bytes(),
        "measured with counting allocator",
    )
}

fn bench_binary_encode(name: &str, value: &SxValue, iters: usize) -> BenchRow {
    let input_text = sx_text::format_canonical(value);
    let mut bytes_out = 0usize;
    reset_alloc_stats();
    let start = Instant::now();
    for _ in 0..iters {
        let b = sx_binary::encode_binary(value, None, None).unwrap();
        bytes_out = b.len();
    }
    let total_ms = start.elapsed().as_secs_f64() * 1000.0;
    make_row(
        "rust",
        name,
        "value",
        input_text.len(),
        1,
        input_text.len(),
        bytes_out,
        iters,
        total_ms,
        peak_alloc_bytes(),
        "measured with counting allocator",
    )
}

fn bench_binary_decode(name: &str, value: &SxValue, iters: usize) -> BenchRow {
    let encoded = sx_binary::encode_binary(value, None, None).unwrap();
    reset_alloc_stats();
    let start = Instant::now();
    for _ in 0..iters {
        let _ = sx_binary::decode_binary(&encoded).unwrap();
    }
    let total_ms = start.elapsed().as_secs_f64() * 1000.0;
    make_row(
        "rust",
        name,
        "value",
        encoded.len(),
        1,
        encoded.len(),
        encoded.len(),
        iters,
        total_ms,
        peak_alloc_bytes(),
        "measured with counting allocator",
    )
}

fn bench_hot_fields(name: &str, value: &SxValue, iters: usize) -> BenchRow {
    let encoded = sx_binary::encode_binary(value, None, None).unwrap();
    let sample = sx_binary::decode_hot_field_values(&encoded, "tenant").unwrap();
    let bytes_out = sample
        .iter()
        .map(|v| match v {
            SxValue::String(s) => s.len(),
            _ => 8,
        })
        .sum::<usize>()
        .max(1);
    reset_alloc_stats();
    sx_binary::reset_decode_stats();
    let start = Instant::now();
    for _ in 0..iters {
        let _ = sx_binary::decode_hot_field_values(&encoded, "tenant").unwrap();
    }
    let total_ms = start.elapsed().as_secs_f64() * 1000.0;
    let stats = sx_binary::current_decode_stats();
    make_row(
        "rust",
        name,
        "event_batch_1k",
        encoded.len(),
        1000,
        encoded.len(),
        bytes_out,
        iters,
        total_ms,
        peak_alloc_bytes(),
        &format!(
            "hot field fast path; full_decode_calls={}; rows_materialized={}",
            stats.full_decode_calls, stats.rows_materialized
        ),
    )
}

fn bench_hot_fields_full_decode(name: &str, value: &SxValue, iters: usize) -> BenchRow {
    let encoded = sx_binary::encode_binary(value, None, None).unwrap();
    let view = SxMessageView::from_binary(&encoded);
    let sample = view.materialize().unwrap();
    let bytes_out = sx_text::format_canonical(&sample).len().max(1);
    reset_alloc_stats();
    sx_binary::reset_decode_stats();
    let start = Instant::now();
    for _ in 0..iters {
        let _ = view.materialize().unwrap();
    }
    let total_ms = start.elapsed().as_secs_f64() * 1000.0;
    let stats = sx_binary::current_decode_stats();
    make_row(
        "rust",
        name,
        "event_batch_1k",
        encoded.len(),
        1000,
        encoded.len(),
        bytes_out,
        iters,
        total_ms,
        peak_alloc_bytes(),
        &format!(
            "full decode baseline; full_decode_calls={}; rows_materialized={}",
            stats.full_decode_calls, stats.rows_materialized
        ),
    )
}

fn bench_table_scan(name: &str, value: &SxValue, iters: usize) -> BenchRow {
    let encoded = sx_binary::encode_binary(value, None, None).unwrap();
    let sample_count = sx_binary::scan_table_numeric_gt(&encoded, "temp", 25.0).unwrap();
    let bytes_out = (sample_count * std::mem::size_of::<i64>()).max(1);
    reset_alloc_stats();
    sx_binary::reset_decode_stats();
    let start = Instant::now();
    for _ in 0..iters {
        let _ = sx_binary::scan_table_numeric_gt(&encoded, "temp", 25.0).unwrap();
    }
    let total_ms = start.elapsed().as_secs_f64() * 1000.0;
    let stats = sx_binary::current_decode_stats();
    make_row(
        "rust",
        name,
        "event_batch_10k",
        encoded.len(),
        10_000,
        encoded.len(),
        bytes_out,
        iters,
        total_ms,
        peak_alloc_bytes(),
        &format!(
            "encoded column scan; full_decode_calls={}; rows_materialized={}",
            stats.full_decode_calls, stats.rows_materialized
        ),
    )
}

fn bench_table_scan_full_materialization(name: &str, value: &SxValue, iters: usize) -> BenchRow {
    let table = SxTableView::new(value).unwrap();
    let input = sx_text::format_canonical(value);
    let sample = table.filter_numeric("temp", NumericOp::Gt, 25.0).unwrap();
    let bytes_out = sx_text::format_canonical(&SxValue::Table(sample))
        .len()
        .max(1);
    reset_alloc_stats();
    let start = Instant::now();
    for _ in 0..iters {
        let _ = table.filter_numeric("temp", NumericOp::Gt, 25.0).unwrap();
        let _ = table.filter_bool("active", true).unwrap();
    }
    let total_ms = start.elapsed().as_secs_f64() * 1000.0;
    make_row(
        "rust",
        name,
        "event_batch_10k",
        input.len(),
        10_000,
        input.len(),
        bytes_out,
        iters,
        total_ms,
        peak_alloc_bytes(),
        "materialized table baseline",
    )
}

fn bench_hash(name: &str, value: &SxValue, iters: usize) -> BenchRow {
    let input = sx_text::format_canonical(value);
    reset_alloc_stats();
    let start = Instant::now();
    for _ in 0..iters {
        let _ = sx_crypto::logical_hash(value).unwrap();
    }
    let total_ms = start.elapsed().as_secs_f64() * 1000.0;
    make_row(
        "rust",
        name,
        "small",
        input.len(),
        1,
        input.len(),
        32,
        iters,
        total_ms,
        peak_alloc_bytes(),
        "measured with counting allocator",
    )
}

fn bench_delta_apply(name: &str, value: &SxValue, iters: usize) -> BenchRow {
    let delta = DeltaDocument {
        from_hash: None,
        ops: vec![
            DeltaOp {
                kind: DeltaOpKind::Set,
                path: SxPath::parse("/active").unwrap_or_else(|_| SxPath::root()),
                value: Some(SxValue::Bool(true)),
                from: None,
                index: None,
            },
            DeltaOp {
                kind: DeltaOpKind::Set,
                path: SxPath::parse("/name").unwrap_or_else(|_| SxPath::root()),
                value: Some(SxValue::String("bench".to_string())),
                from: None,
                index: None,
            },
        ],
    };
    let input = sx_text::format_canonical(value);
    reset_alloc_stats();
    let start = Instant::now();
    for _ in 0..iters {
        let _ = sx_delta::apply_delta(value, &delta).unwrap();
    }
    let total_ms = start.elapsed().as_secs_f64() * 1000.0;
    make_row(
        "rust",
        name,
        "small",
        input.len(),
        1,
        input.len(),
        input.len(),
        iters,
        total_ms,
        peak_alloc_bytes(),
        "measured with counting allocator",
    )
}

fn bench_json_parse(name: &str, json: &str, iters: usize) -> BenchRow {
    reset_alloc_stats();
    let start = Instant::now();
    for _ in 0..iters {
        let _: serde_json::Value = serde_json::from_str(json).unwrap();
    }
    let total_ms = start.elapsed().as_secs_f64() * 1000.0;
    make_row(
        "serde_json",
        name,
        "json",
        json.len(),
        1,
        json.len(),
        json.len(),
        iters,
        total_ms,
        peak_alloc_bytes(),
        "measured with counting allocator",
    )
}

fn bench_json_size(name: &str, json: &str) -> BenchRow {
    let iterations = 50_000usize;
    reset_alloc_stats();
    let start = Instant::now();
    let mut measured = 0usize;
    for _ in 0..iterations {
        measured = json.len();
    }
    let total_ms = start.elapsed().as_secs_f64() * 1000.0;
    make_row(
        "serde_json",
        name,
        "event_batch_1k",
        json.len(),
        1000,
        measured,
        measured,
        iterations,
        total_ms,
        peak_alloc_bytes(),
        "size baseline",
    )
}

fn bench_sx_size(name: &str, value: &SxValue) -> BenchRow {
    let input_len = sx_text::format_canonical(value).len();
    let iterations = 200usize;
    reset_alloc_stats();
    let start = Instant::now();
    let mut out_len = 0usize;
    for _ in 0..iterations {
        out_len = sx_binary::encode_binary(value, None, None).unwrap().len();
    }
    let total_ms = start.elapsed().as_secs_f64() * 1000.0;
    make_row(
        "rust",
        name,
        "event_batch_1k",
        input_len,
        1000,
        input_len,
        out_len,
        iterations,
        total_ms,
        peak_alloc_bytes(),
        "size from encoded binary",
    )
}

fn make_row(
    implementation: &str,
    operation: &str,
    dataset: &str,
    bytes_input: usize,
    records: usize,
    bytes_in_for_rate: usize,
    bytes_output: usize,
    iters: usize,
    total_ms: f64,
    peak_memory_bytes: usize,
    notes: &str,
) -> BenchRow {
    let secs = (total_ms / 1000.0).max(1e-9);
    let ops_per_sec = iters as f64 / secs;
    let mb_per_sec = ((bytes_in_for_rate as f64 * iters as f64) / (1024.0 * 1024.0)) / secs;
    BenchRow {
        implementation: implementation.to_string(),
        operation: operation.to_string(),
        dataset: dataset.to_string(),
        records,
        bytes_input,
        bytes_output,
        iterations: iters,
        total_ms,
        ops_per_sec,
        mb_per_sec,
        peak_memory_bytes,
        notes: notes.to_string(),
    }
}
