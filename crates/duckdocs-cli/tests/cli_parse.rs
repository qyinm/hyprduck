use std::process::Command;

#[test]
fn doctor_reports_engine_resolution() {
    let output = Command::new(env!("CARGO_BIN_EXE_duckdocs-cli"))
        .arg("doctor")
        .output()
        .expect("doctor command should run");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("DuckDocs CLI is available."));
}
