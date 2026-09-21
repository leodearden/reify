//! The PROCESS-BOUNDARY half of PRD `docs/prds/v0_6/units-length-gate-completion.md` §6.
//!
//! Every leaf of that PRD tested its own gate IN-PROCESS, at `Engine::build` against a
//! `MockGeometryKernel`. Nothing pinned the signal the PRD's §1 headline promises the
//! user: `$ reify eval bare.ri` → a units diagnostic, `$ echo $?` → 1. A severity
//! downgrade, a swallowed build result at `main.rs`, or a diagnostic-renderer regression
//! would leave every leaf test green while that user-observable signal vanished. This
//! file exists so that cannot happen: each test spawns the real `reify` binary and
//! asserts the exit code and the stderr a user would actually see.
//!
//! Each source is written to a `tempfile::tempdir()` rather than to the flat
//! `tests/fixtures/` tree, so a bare-number source sits beside the assertion it feeds and
//! this file can be read in isolation. The file stem MUST match the source's `module`
//! declaration: on a mismatch the CLI reports `E_MODULE_PATH_MISMATCH` and the test
//! measures that instead of the units gate (the trap documented at
//! `harness_cli/cli_check.rs:796-797`).

use crate::common;

/// §6 row 1 — a bare-`Int` primitive dimension is rejected at the process boundary,
/// naming EVERY offending argument rather than only the first.
///
/// All three of `width`, `height` and `depth` are asserted: a gate that fired on one
/// position and silently passed the other two would still exit 1, so the exit code alone
/// cannot distinguish a whole gate from a third of one.
#[test]
fn bare_box_dimensions_exit_1_naming_every_rejected_argument() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let path = dir.path().join("bare_box_dimensions.ri");
    std::fs::write(
        &path,
        r#"module bare_box_dimensions

structure def S {
    let b = box(20, 20, 10)
    param geometry : Solid = b
}
"#,
    )
    .expect("failed to write temp module");

    let (status, stdout, stderr) =
        common::run_with_args(&["eval", path.to_str().expect("temp path is UTF-8")]);

    assert!(
        !status.success(),
        "`reify eval` on bare box dimensions must exit nonzero — this IS the §1 \
         headline signal;\nstdout: {stdout}\nstderr: {stderr}"
    );
    for arg in ["width", "height", "depth"] {
        assert!(
            stderr.contains(&format!(
                "box: {arg} argument expects Length, got Int; \
                 pass a dimensioned length such as `5mm`"
            )),
            "stderr should carry the units rejection for `{arg}`; got: {stderr}"
        );
    }
}

/// §6 row 2 (control) — the dimensioned spelling still exits 0 AND still realizes the
/// same SI volume the pre-gate build produced.
///
/// The volume literal is the load-bearing half. Exit 0 alone would also be produced by a
/// gate that accepted the LENGTH and then re-scaled it; `20mm × 20mm × 10mm` is
/// 4·10⁻⁶ m³ before the gate and must remain so after it.
#[test]
fn dimensioned_box_exits_0_with_the_pre_gate_si_volume() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let path = dir.path().join("dimensioned_box.ri");
    std::fs::write(
        &path,
        r#"module dimensioned_box

structure def S {
    let b = box(20mm, 20mm, 10mm)
    let v = volume(b)
    param geometry : Solid = b
}
"#,
    )
    .expect("failed to write temp module");

    let (status, stdout, stderr) =
        common::run_with_args(&["eval", path.to_str().expect("temp path is UTF-8")]);

    assert!(
        status.success(),
        "the dimensioned control must still exit 0;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stderr.contains("expects Length"),
        "a dimensioned length must not be rejected; got: {stderr}"
    );
    assert!(
        stdout.contains("S.v = 0.000004 m^3"),
        "the gate must not have re-scaled an ACCEPTED length: 20mm × 20mm × 10mm is \
         0.000004 m³, the pre-gate SI baseline; got: {stdout}"
    );
}

/// §6 row 3 — D1: a bare `0` is NOT special-cased, so it is rejected with exactly the
/// wording any other bare number gets.
///
/// `0` is the tempting exemption (it is dimensionally harmless), and an exemption is the
/// one shape that would let a bare-number habit survive the gate.
#[test]
fn a_bare_zero_is_rejected_like_any_other_bare_number() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let path = dir.path().join("bare_zero_box.ri");
    std::fs::write(
        &path,
        r#"module bare_zero_box

structure def S {
    let b = box(0, 0, 0)
    param geometry : Solid = b
}
"#,
    )
    .expect("failed to write temp module");

    let (status, stdout, stderr) =
        common::run_with_args(&["eval", path.to_str().expect("temp path is UTF-8")]);

    assert!(
        !status.success(),
        "D1: a bare zero must be rejected, not exempted;\nstdout: {stdout}\nstderr: {stderr}"
    );
    for arg in ["width", "height", "depth"] {
        assert!(
            stderr.contains(&format!(
                "box: {arg} argument expects Length, got Int; \
                 pass a dimensioned length such as `5mm`"
            )),
            "a bare zero must draw the SAME wording as any other bare number for \
             `{arg}`; got: {stderr}"
        );
    }
}
