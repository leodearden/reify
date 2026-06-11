mod common;

use std::process::Command;

/// Build a box fixture to a `.3mf` output path and assert valid OPC package
/// structure in the resulting bytes (Stored=uncompressed, no zip reader needed).
///
/// RED before step-10: main.rs maps unknown extensions to STEP, so out.3mf
/// contains STEP bytes — the 3D/3dmodel.model and <triangle assertions fail.
#[test]
fn build_box_to_3mf_writes_valid_package() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let output_path = dir.path().join("out.3mf");

    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args([
            "build",
            &common::fixture_path("box_3mf.ri"),
            "-o",
            output_path.to_str().expect("temp path is not valid UTF-8"),
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("failed to execute reify binary");

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

    assert!(
        output.status.success(),
        "reify build should exit 0 for valid box_3mf.ri\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("Wrote"),
        "stdout should contain 'Wrote', got: {stdout}"
    );
    assert!(
        output_path.exists(),
        "output file should be written on success"
    );

    let bytes = std::fs::read(&output_path).expect("failed to read output file");

    // Stored/uncompressed: OPC part names and model XML appear literally in raw bytes.
    assert!(
        bytes
            .windows(b"3D/3dmodel.model".len())
            .any(|w| w == b"3D/3dmodel.model"),
        "output bytes must contain '3D/3dmodel.model' (got STEP or other format?)"
    );

    let tri_needle = b"<triangle ";
    let tri_count = bytes
        .windows(tri_needle.len())
        .filter(|w| *w == tri_needle)
        .count();
    assert!(
        tri_count > 0,
        "3MF output must contain at least one <triangle> element"
    );
}
