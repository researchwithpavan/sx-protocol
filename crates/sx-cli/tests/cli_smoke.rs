use std::fs;
use std::process::Command;

#[test]
fn cli_validate_and_hash() {
    let tmp = std::env::temp_dir().join("sx_cli_smoke.sx");
    fs::write(&tmp, "{name:\"Asha\",active:true}").unwrap();

    let bin = env!("CARGO_BIN_EXE_sx");
    let status = Command::new(bin)
        .arg("validate")
        .arg(tmp.to_string_lossy().to_string())
        .status()
        .unwrap();
    assert!(status.success());

    let output = Command::new(bin)
        .arg("hash")
        .arg(tmp.to_string_lossy().to_string())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(!output.stdout.is_empty());
}
