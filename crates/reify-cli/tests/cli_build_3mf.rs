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

/// δ (task #4763) B7/B8 capstone: a Physical body with a Material color reaches
/// the exported 3MF file as a `<basematerials>` element (B7); a colorless box
/// body does NOT emit `<basematerials>` (B8 geometry-only back-compat).
///
/// Stored/uncompressed 3MF: part names and model XML appear literally in raw
/// bytes — no zip reader needed, substring search suffices.
#[test]
fn build_colored_box_to_3mf_writes_basematerials() {
    // --- B7: colored box → displaycolor= present ---
    {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let output_path = dir.path().join("out.3mf");

        let output = Command::new(env!("CARGO_BIN_EXE_reify"))
            .args([
                "build",
                &common::fixture_path("box_3mf_colored.ri"),
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
            "B7: reify build should exit 0 for box_3mf_colored.ri\nstdout: {stdout}\nstderr: {stderr}"
        );
        assert!(output_path.exists(), "B7: output file must be written");

        let bytes = std::fs::read(&output_path).expect("B7: failed to read output file");

        let has_displaycolor = bytes
            .windows(b"displaycolor=".len())
            .any(|w| w == b"displaycolor=");
        assert!(
            has_displaycolor,
            "B7: colored box must produce a <basematerials> element with displaycolor= \
             in the 3MF — body color did not reach the file"
        );
        let has_base_elem = bytes
            .windows(b"<base ".len())
            .any(|w| w == b"<base ");
        assert!(
            has_base_elem,
            "B7: <base displaycolor=...> element must be present in the model XML"
        );
    }

    // --- B8: colorless box → no displaycolor ---
    {
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
            "B8: reify build should exit 0 for box_3mf.ri\nstdout: {stdout}\nstderr: {stderr}"
        );

        let bytes = std::fs::read(&output_path).expect("B8: failed to read output file");

        assert!(
            bytes
                .windows(b"<triangle ".len())
                .any(|w| w == b"<triangle "),
            "B8: geometry-only 3MF must still contain <triangle> elements"
        );
        let has_displaycolor = bytes
            .windows(b"displaycolor=".len())
            .any(|w| w == b"displaycolor=");
        assert!(
            !has_displaycolor,
            "B8: colorless box_3mf.ri must NOT produce a displaycolor= attribute — \
             geometry-only back-compat broken"
        );
    }
}
