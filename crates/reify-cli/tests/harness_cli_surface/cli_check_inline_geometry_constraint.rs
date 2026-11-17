//! `reify check` decides a constraint over an inline geometry query (task 10033).
//!
//! `constraint volume(box(w, w, w)) > 1mm^3` must report OK, as its let-bound
//! twin does, instead of `INDETERMINATE … operator undefined for these operand
//! kinds: Scalar<m>` with exit 0. Drives the binary directly: this harness's seam
//! rule forbids `tests/common` helpers.

use std::process::{Command, Output};

/// Write `module <stem>` + `body` to `<stem>.ri` (the stem must match the module,
/// or E_MODULE_PATH_MISMATCH fires) and run `reify check` on it.
fn check_module(stem: &str, body: &str) -> Output {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let path = dir.path().join(format!("{stem}.ri"));
    std::fs::write(&path, format!("module {stem}\n\n{body}\n")).expect("failed to write module");
    Command::new(env!("CARGO_BIN_EXE_reify"))
        .arg("check")
        .arg(&path)
        .output()
        .expect("failed to spawn reify")
}

#[test]
fn check_decides_a_constraint_over_an_inline_geometry_query() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT unavailable, so no volume() can be realized");
        return;
    }

    let ok = check_module(
        "geo2",
        "structure def Part2 {\n    param w : Length = 10mm\n    \
         constraint volume(box(w, w, w)) > 1mm^3\n}",
    );
    let ok_stdout = String::from_utf8_lossy(&ok.stdout);
    let ok_stderr = String::from_utf8_lossy(&ok.stderr);
    assert!(
        ok_stdout.contains("OK Part2#constraint[0]") && !ok_stdout.contains("INDETERMINATE"),
        "expected `OK Part2#constraint[0]`;\nstdout:\n{ok_stdout}\nstderr:\n{ok_stderr}"
    );
    assert!(
        ok.status.success(),
        "a satisfied check must exit 0: {:?}",
        ok.status
    );

    let big = check_module(
        "geo3",
        "structure def Part3 {\n    param w : Length = 10mm\n    \
         constraint volume(box(w, w, w)) > 1m^3\n}",
    );
    let big_stdout = String::from_utf8_lossy(&big.stdout);
    let big_stderr = String::from_utf8_lossy(&big.stderr);
    assert!(
        big_stdout.contains("VIOLATED Part3#constraint[0]"),
        "expected `VIOLATED Part3#constraint[0]`;\nstdout:\n{big_stdout}\nstderr:\n{big_stderr}"
    );
    assert!(!big.status.success(), "a violated check must exit non-zero");
}
