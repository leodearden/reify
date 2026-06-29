mod common;

use std::process::Command;

/// δ (task #4891) B8 integration gate: a Physical + SurfaceTreated body carrying
/// a functional Coating(process: Anodize, color: Color(named:"RAL9005", ...))
/// exports the anodize-derived RGB into the 3MF <basematerials>, overriding the
/// bare-material grey, with NO W_3MF_NO_MATERIALS warning.
///
/// Stored/uncompressed 3MF: part names and model XML appear literally in raw
/// bytes — no zip reader needed, substring search suffices.
///
/// Colour derivation: see `examples/surface_finish_3mf.ri` (authoritative).
/// Expected result: displaycolor="#0E0E10FF" (RAL9005 → Rgb8{14,14,16}).
///
/// TDD history: the test was initially written RED (step-1) when
/// `examples/surface_finish_3mf.ri` did not yet exist — `reify build <missing>`
/// exited non-zero and assert (a) failed.  The example was added in step-2
/// (same commit, this file), turning the test GREEN.
#[test]
fn build_anodized_box_3mf_reflects_coating_color() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let output_path = dir.path().join("out.3mf");

    let example = common::example_path("surface_finish_3mf.ri");

    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args([
            "build",
            &example,
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

    // (a) Build must succeed.
    assert!(
        output.status.success(),
        "reify build should exit 0 for surface_finish_3mf.ri\nstdout: {stdout}\nstderr: {stderr}"
    );

    // (a-ii) Positive diagnostic channel check: stdout contains "Wrote" on success.
    // This makes the negative W_3MF_NO_MATERIALS assertion (d) below non-vacuous —
    // if stdout stops being used for diagnostics entirely, this assertion catches it
    // before (d) silently becomes a no-op.
    assert!(
        stdout.contains("Wrote"),
        "stdout should contain 'Wrote' on a successful build\nstdout: {stdout}\nstderr: {stderr}"
    );

    // (b) Output file must exist.
    assert!(
        output_path.exists(),
        "output file should be written on success"
    );

    let bytes = std::fs::read(&output_path).expect("failed to read output 3MF file");

    // (c-i) Stored/uncompressed 3MF package structure present.
    assert!(
        bytes
            .windows(b"3D/3dmodel.model".len())
            .any(|w| w == b"3D/3dmodel.model"),
        "output bytes must contain '3D/3dmodel.model' (not a valid 3MF package?)\n\
         stdout: {stdout}\nstderr: {stderr}"
    );

    // (c-ii) <basematerials element present — coating colour reached the file.
    assert!(
        bytes
            .windows(b"<basematerials".len())
            .any(|w| w == b"<basematerials"),
        "output must contain a <basematerials> element — coating colour did not reach the 3MF\n\
         stdout: {stdout}\nstderr: {stderr}"
    );

    // (c-iii) <base element present inside <basematerials.
    assert!(
        bytes
            .windows(b"<base ".len())
            .any(|w| w == b"<base "),
        "output must contain a <base ...> element with the colour attribute\n\
         stdout: {stdout}\nstderr: {stderr}"
    );

    // (c-iv) Exact anodize-derived colour: RAL9005 → Rgb8{14,14,16} → #0E0E10FF.
    // Presence of this exact string proves the coating override beat the bare-material grey.
    let needle = b"displaycolor=\"#0E0E10FF\"";
    assert!(
        bytes.windows(needle.len()).any(|w| w == needle),
        "output must contain displaycolor=\"#0E0E10FF\" (RAL9005 anodize colour) — \
         coating override did not propagate to the exported 3MF\n\
         stdout: {stdout}\nstderr: {stderr}"
    );

    // (d) No W_3MF_NO_MATERIALS diagnostic — the warning must be suppressed when a
    //     colour is present (write_3mf contract).
    //     (a-ii) above proves stdout is in use, so this negative check is non-vacuous.
    //     Note: (c-iv) is the load-bearing gate — displaycolor="#0E0E10FF" in the
    //     file directly proves the coating override propagated; (d) is belt-and-
    //     suspenders confirmation that the diagnostic channel is silent.
    assert!(
        !stdout.contains("W_3MF_NO_MATERIALS"),
        "stdout must NOT contain W_3MF_NO_MATERIALS when a coating colour is present\n\
         stdout: {stdout}"
    );
    assert!(
        !stderr.contains("W_3MF_NO_MATERIALS"),
        "stderr must NOT contain W_3MF_NO_MATERIALS when a coating colour is present\n\
         stderr: {stderr}"
    );
}
