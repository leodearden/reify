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

/// §6 row 4 — a bare `fillet` radius is rejected with a UNITS diagnostic that REPLACES
/// the span-less kernel failure the PRD's §2 probe recorded.
///
/// The negative half is the row's real claim. Before the gate, a bare radius reached
/// OCCT and surfaced as `BRepFilletAPI_MakeFillet failed` — unattributable, with no span
/// and no argument name. Asserting only the new message would leave a regression that
/// merely PREPENDED a units line to the old kernel failure indistinguishable from the
/// fix, so the old failure's absence is asserted too.
#[test]
fn a_bare_fillet_radius_replaces_the_span_less_occt_failure() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let path = dir.path().join("bare_fillet_radius.ri");
    std::fs::write(
        &path,
        r#"module bare_fillet_radius

structure def S {
    let b = fillet(box(10mm, 10mm, 10mm), 1)
    param geometry : Solid = b
}
"#,
    )
    .expect("failed to write temp module");

    let (status, stdout, stderr) =
        common::run_with_args(&["eval", path.to_str().expect("temp path is UTF-8")]);

    assert!(
        !status.success(),
        "a bare fillet radius must exit nonzero;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains(
            "fillet: radius argument expects Length, got Int; \
             pass a dimensioned length such as `5mm`"
        ),
        "stderr should carry the units rejection naming `radius`; got: {stderr}"
    );
    assert!(
        !stderr.contains("BRepFilletAPI_MakeFillet"),
        "the bare radius must never reach OCCT: the span-less kernel failure is \
         REPLACED by the units diagnostic, not accompanied by it; got: {stderr}"
    );
}

/// §6 row 5a — the PLANE-VALUE form. The plane's own offset is rejected, and `mirror`
/// then fails attributably on the resulting `undef` rather than silently building a
/// mirror plane 10 METRES out.
///
/// Split from the scalar form below because the two travel different routes: this one
/// rejects inside `plane_yz` and propagates `undef` into `mirror`, while the scalar form
/// rejects at `mirror`'s own origin slots. One test asserting a loose `contains("mirror")`
/// would pass on either and so pin neither.
#[test]
fn a_bare_plane_offset_is_rejected_and_mirror_fails_attributably() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let path = dir.path().join("bare_mirror_plane_offset.ri");
    std::fs::write(
        &path,
        r#"module bare_mirror_plane_offset

structure def S {
    let b = box(20mm, 20mm, 10mm)
    let m = mirror(b, plane_yz(10))
    param geometry : Solid = m
}
"#,
    )
    .expect("failed to write temp module");

    let (status, stdout, stderr) =
        common::run_with_args(&["eval", path.to_str().expect("temp path is UTF-8")]);

    assert!(
        !status.success(),
        "a bare plane offset must exit nonzero;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains(
            "plane_yz: offset argument expects Length, got Int; \
             pass a dimensioned length such as `5mm`"
        ),
        "stderr should name the PLANE's own rejected offset; got: {stderr}"
    );
    assert!(
        stderr.contains("mirror: expected a Plane value, got undef"),
        "the consuming `mirror` must fail attributably on the resulting undef rather \
         than silently mirroring about a 10-metre plane; got: {stderr}"
    );
}

/// §6 row 5b — the SCALAR form keeps its pre-existing per-component wording. The
/// no-regression half of row 5.
///
/// `ox` is asserted by name: a loose `contains("mirror")` would also be satisfied by the
/// plane-value route's `expected a Plane value` message, so it could not tell a surviving
/// per-component gate from a collapsed one.
#[test]
fn the_scalar_mirror_origin_keeps_its_per_component_wording() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let path = dir.path().join("bare_mirror_scalar_origin.ri");
    std::fs::write(
        &path,
        r#"module bare_mirror_scalar_origin

structure def S {
    let b = box(20mm, 20mm, 10mm)
    let m = mirror(b, 10, 0, 0, 1, 0, 0)
    param geometry : Solid = m
}
"#,
    )
    .expect("failed to write temp module");

    let (status, stdout, stderr) =
        common::run_with_args(&["eval", path.to_str().expect("temp path is UTF-8")]);

    assert!(
        !status.success(),
        "the scalar mirror origin must still exit nonzero;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains(
            "mirror: ox argument expects Length, got Int; \
             pass a dimensioned length such as `5mm`"
        ),
        "stderr should carry the per-COMPONENT rejection naming `ox`; got: {stderr}"
    );
}

/// §6 row 6 (control) — the dimensioned plane offset still exits 0 and still mirrors
/// about the same place.
///
/// WHY THE COMPARAND IS `0.02 m` AND NOT THE PRD's LITERAL `plane_yz(0.01)`: that
/// spelling is now itself rejected (measured: `plane_yz: offset argument expects Length,
/// got Real`), so it cannot be run as a live comparand. The identity the PRD row was
/// always about survives intact — `10mm` IS `0.01 m`, and mirroring a box spanning
/// x ∈ [0, 0.02] about the plane x = 0.01 leaves its centroid at x = 0.02.
///
/// A tolerance rather than a byte-equal match: the printed x is
/// `0.019999999999999997 m`, and pinning that spelling would make the row fail on any
/// benign change to the value printer's float formatting instead of on a geometry change.
#[test]
fn a_dimensioned_plane_offset_mirrors_to_the_same_si_position() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let path = dir.path().join("dimensioned_mirror_plane.ri");
    std::fs::write(
        &path,
        r#"module dimensioned_mirror_plane

structure def S {
    let b = box(20mm, 20mm, 10mm)
    let m = mirror(b, plane_yz(10mm))
    let c = centroid(m)
    param geometry : Solid = m
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
        "a dimensioned plane offset must not be rejected; got: {stderr}"
    );

    let x = centroid_x_metres(&stdout);
    assert!(
        (x - 0.02).abs() < 1e-9,
        "mirroring about x = 10mm must leave the centroid at x = 0.02 m — the SI \
         identity the PRD's `plane_yz(0.01)` comparand stood for; got {x} m from: {stdout}"
    );
}

/// Read the x component, in metres, out of the `S.c = point(<x> m, <y> m, <z> m)` line
/// `reify eval` prints for a `centroid(...)` binding.
///
/// Deliberately narrow: the CLI's only output is text, so SOME extraction is unavoidable
/// for a tolerance comparison, and the narrowest one that cannot silently succeed on the
/// wrong line is better than a looser scan. Every failure path panics with the stdout
/// that produced it, so a printer change surfaces as a readable diagnostic rather than a
/// wrong number.
fn centroid_x_metres(stdout: &str) -> f64 {
    let point = stdout
        .lines()
        .find_map(|line| line.strip_prefix("S.c = point("))
        .unwrap_or_else(|| panic!("no `S.c = point(...)` line in stdout: {stdout}"));
    let x = point
        .split_once(" m,")
        .unwrap_or_else(|| panic!("no `<x> m,` component in `{point}` (stdout: {stdout})"))
        .0;
    x.parse()
        .unwrap_or_else(|e| panic!("centroid x `{x}` is not a number ({e}); stdout: {stdout}"))
}
