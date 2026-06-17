//! Integration tests for the io-export δ **declarative output driver**:
//! `reify build <file.ri>` with **NO `-o`** lets the DSL `Output` occurrences
//! drive the export format(s) and path(s), each resolved relative to the design
//! file's directory.
//!
//! Models on `cli_build_3mf.rs` (`CARGO_BIN_EXE_reify` + `tempfile` +
//! `common::fixture_path`). The `box(...)` primitive realizes via the link-time
//! kernel, exactly as `cli_build_3mf.rs` assumes (no extra OCCT gate).
//!
//! Signals (PRD §7.5):
//! - **B5** — driver format+path: no `-o`, the `STLOutput` occurrence drives the
//!   STL format and the `"o.stl"` path.
//! - **B6** — multi-output: every `Output` occurrence emits its own file.
//! - **B7** — design-file-relative path: a relative occurrence path resolves
//!   against the `.ri` file's directory, NOT the process cwd.
//! - **B10** — back-compat: `-o` present keeps the imperative single-output
//!   path byte-for-byte (the declarative driver does NOT fire).
//!
//! RED until step-16 wires the no-`-o` declarative mode into `cmd_build`: today
//! `reify build f.ri` without `-o` exits non-zero with a usage error, so
//! B5/B6/B7 fail. B10 (the `-o` imperative path) is a pure regression guard and
//! is green before and after.

mod common;

use std::path::Path;
use std::process::Command;

/// Assert `bytes` is a well-formed binary STL: an 80-byte header, a 4-byte
/// little-endian triangle count `N > 0`, then exactly `50·N` triangle bytes —
/// i.e. `len == 84 + 50·N`. This byte identity proves the kernel serialized
/// **STL** specifically (not STEP or another format), so it is the strongest
/// evidence that the DSL `STLOutput` occurrence — not a CLI flag — drove the
/// format.
fn assert_valid_binary_stl(bytes: &[u8]) {
    assert!(
        bytes.len() >= 84,
        "binary STL must be at least 84 bytes (80-byte header + 4-byte count); got {}",
        bytes.len()
    );
    let n = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
    assert!(n > 0, "binary STL triangle count must be > 0; got {n}");
    assert_eq!(
        bytes.len(),
        84 + 50 * n,
        "binary STL must be exactly 84 + 50·N bytes (N = {n}); got {}",
        bytes.len()
    );
}

/// Run `reify <args...>` with the child process's working directory set to
/// `cwd`, returning `(success, stdout, stderr)`. Pinning the child cwd lets the
/// B7 test prove design-relative resolution does not fall back to the cwd.
fn run_in(cwd: &Path, args: &[&str]) -> (bool, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_reify"))
        .args(args)
        .current_dir(cwd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("failed to execute reify binary");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// B5: `reify build <temp>/foo.ri` with NO `-o` must let the single `STLOutput`
/// occurrence drive both the format (STL) and the path (`"o.stl"`, resolved into
/// the design-file directory). Exit 0, stdout says "Wrote", and `<temp>/o.stl`
/// is a valid binary STL.
#[test]
fn build_no_output_flag_drives_format_and_path() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let ri = dir.path().join("foo.ri");
    std::fs::copy(common::fixture_path("output_driver_single.ri"), &ri)
        .expect("failed to copy single-output fixture");

    let (ok, stdout, stderr) = run_in(
        dir.path(),
        &["build", ri.to_str().expect("temp path is not valid UTF-8")],
    );

    assert!(
        ok,
        "reify build (no -o) must exit 0 when the design declares an Output occurrence\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("Wrote"),
        "stdout should report the written artifact with 'Wrote'; got:\n{stdout}"
    );

    let out = dir.path().join("o.stl");
    assert!(
        out.exists(),
        "the STLOutput occurrence (path \"o.stl\") must write <design_dir>/o.stl"
    );
    assert_valid_binary_stl(&std::fs::read(&out).expect("failed to read o.stl"));
}

/// B7: a relative occurrence path resolves against the **design file's**
/// directory, not the process cwd. With the `.ri` at `<root>/sub/foo.ri` and the
/// child cwd pinned to `<root>` (NOT `sub`), `o.stl` must land at
/// `<root>/sub/o.stl` and NOT at `<root>/o.stl`.
#[test]
fn build_no_output_flag_resolves_path_relative_to_design_file() {
    let root = tempfile::tempdir().expect("failed to create temp dir");
    let sub = root.path().join("sub");
    std::fs::create_dir_all(&sub).expect("failed to create sub dir");
    let ri = sub.join("foo.ri");
    std::fs::copy(common::fixture_path("output_driver_single.ri"), &ri)
        .expect("failed to copy single-output fixture");

    // cwd = root (NOT sub): a cwd-relative resolver would write root/o.stl.
    let (ok, stdout, stderr) = run_in(
        root.path(),
        &["build", ri.to_str().expect("temp path is not valid UTF-8")],
    );

    assert!(
        ok,
        "reify build (no -o) must exit 0\nstdout: {stdout}\nstderr: {stderr}"
    );

    let design_relative = sub.join("o.stl");
    assert!(
        design_relative.exists(),
        "o.stl must be written relative to the design file's dir (<root>/sub/o.stl)"
    );
    let cwd_relative = root.path().join("o.stl");
    assert!(
        !cwd_relative.exists(),
        "o.stl must NOT be written relative to the process cwd (<root>/o.stl) — \
         the path is design-file-relative (B7)"
    );
    assert_valid_binary_stl(&std::fs::read(&design_relative).expect("failed to read o.stl"));
}

/// B6: every `Output` occurrence emits its own file. `output_driver_multi.ri`
/// declares an `STLOutput` ("o.stl") and a `STEPOutput` ("o2.step"); a no-`-o`
/// build must write both.
#[test]
fn build_no_output_flag_emits_all_occurrences() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let ri = dir.path().join("foo.ri");
    std::fs::copy(common::fixture_path("output_driver_multi.ri"), &ri)
        .expect("failed to copy multi-output fixture");

    let (ok, stdout, stderr) = run_in(
        dir.path(),
        &["build", ri.to_str().expect("temp path is not valid UTF-8")],
    );

    assert!(
        ok,
        "reify build (no -o) must exit 0 for a multi-output design\n\
         stdout: {stdout}\nstderr: {stderr}"
    );

    let stl = dir.path().join("o.stl");
    let step = dir.path().join("o2.step");
    assert!(stl.exists(), "the STLOutput occurrence must write o.stl");
    assert!(step.exists(), "the STEPOutput occurrence must write o2.step");
    assert_valid_binary_stl(&std::fs::read(&stl).expect("failed to read o.stl"));
    assert!(
        std::fs::metadata(&step).map(|m| m.len()).unwrap_or(0) > 0,
        "o2.step must be non-empty"
    );
}

/// B10 (back-compat regression guard): `-o` present keeps the imperative
/// single-output path. `reify build <temp>/foo.ri -o <temp>/x.stl` writes the
/// `-o` target and does NOT run the declarative driver — so the occurrence's own
/// path ("o.stl") is never emitted. This test is green both before and after
/// step-16.
#[test]
fn build_with_output_flag_keeps_imperative_path() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let ri = dir.path().join("foo.ri");
    std::fs::copy(common::fixture_path("output_driver_single.ri"), &ri)
        .expect("failed to copy single-output fixture");
    let x = dir.path().join("x.stl");

    let (ok, stdout, stderr) = run_in(
        dir.path(),
        &[
            "build",
            ri.to_str().expect("temp path is not valid UTF-8"),
            "-o",
            x.to_str().expect("temp path is not valid UTF-8"),
        ],
    );

    assert!(
        ok,
        "reify build -o must exit 0 (imperative back-compat)\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("Wrote"),
        "stdout should report the written artifact with 'Wrote'; got:\n{stdout}"
    );
    assert!(x.exists(), "the -o target x.stl must be written");
    assert_valid_binary_stl(&std::fs::read(&x).expect("failed to read x.stl"));

    // With -o present, the declarative driver MUST NOT fire: the occurrence's
    // own path ("o.stl") must not be emitted alongside the imperative output.
    assert!(
        !dir.path().join("o.stl").exists(),
        "with -o present the imperative path must not also run the Output driver"
    );
}
