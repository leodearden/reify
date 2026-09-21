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
use reify_core::units::LENGTH_MIGRATION_HINT;
use std::process::ExitStatus;

/// Run `reify eval` over `source`.
fn eval_source(stem: &str, source: &str) -> (ExitStatus, String, String) {
    run_source("eval", stem, source)
}

/// Run `reify check` over `source` — the twin row 9 needs.
///
/// Kept as a named twin rather than folded into a `run_source("check", …)` call so that
/// row 9's two calls read as the same bytes through two subcommands, which is that row's
/// entire claim.
fn check_source(stem: &str, source: &str) -> (ExitStatus, String, String) {
    run_source("check", stem, source)
}

/// Write `source` as `<stem>.ri` into a fresh temp dir, run `reify <subcommand>` over it
/// and return `(status, stdout, stderr)`.
///
/// The single spawn site the two helpers above share — the same shape
/// `tests/common/mod.rs` uses for its own `spawn_reify`. `source` must declare
/// `module <stem>`; see this module's header for why.
fn run_source(subcommand: &str, stem: &str, source: &str) -> (ExitStatus, String, String) {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let path = dir.path().join(format!("{stem}.ri"));
    std::fs::write(&path, source).expect("failed to write temp module");
    common::run_with_args(&[subcommand, path.to_str().expect("temp path is UTF-8")])
}

/// Assert that `stderr` carries the ONE units-rejection line `ArgRejection::message`
/// (`crates/reify-ir/src/arg_acceptance.rs:211-222`) produces for `builtin`'s `arg`.
///
/// Built from the template rather than hand-spelled per row, and from the real
/// [`LENGTH_MIGRATION_HINT`] const rather than a copy of its text. Hand-spelling the hint
/// once per row would be exactly the lockstep duplication D9 exists to prevent: a reword
/// of the hint must break this suite in ONE place, not in every row that quotes it.
fn expect_length_rejection(stderr: &str, builtin: &str, arg: &str, got: &str) {
    let expected =
        format!("{builtin}: {arg} argument expects Length, got {got}; {LENGTH_MIGRATION_HINT}");
    assert!(
        stderr.contains(&expected),
        "stderr should carry the units rejection `{expected}`; got: {stderr}"
    );
}

/// §6 row 1 — a bare-`Int` primitive dimension is rejected at the process boundary,
/// naming EVERY offending argument rather than only the first.
///
/// All three of `width`, `height` and `depth` are asserted: a gate that fired on one
/// position and silently passed the other two would still exit 1, so the exit code alone
/// cannot distinguish a whole gate from a third of one.
#[test]
fn bare_box_dimensions_exit_1_naming_every_rejected_argument() {
    let (status, stdout, stderr) = eval_source(
        "bare_box_dimensions",
        r#"module bare_box_dimensions

structure def S {
    let b = box(20, 20, 10)
    param geometry : Solid = b
}
"#,
    );

    assert!(
        !status.success(),
        "`reify eval` on bare box dimensions must exit nonzero — this IS the §1 \
         headline signal;\nstdout: {stdout}\nstderr: {stderr}"
    );
    for arg in ["width", "height", "depth"] {
        expect_length_rejection(&stderr, "box", arg, "Int");
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
    let (status, stdout, stderr) = eval_source(
        "dimensioned_box",
        r#"module dimensioned_box

structure def S {
    let b = box(20mm, 20mm, 10mm)
    let v = volume(b)
    param geometry : Solid = b
}
"#,
    );

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
    let (status, stdout, stderr) = eval_source(
        "bare_zero_box",
        r#"module bare_zero_box

structure def S {
    let b = box(0, 0, 0)
    param geometry : Solid = b
}
"#,
    );

    assert!(
        !status.success(),
        "D1: a bare zero must be rejected, not exempted;\nstdout: {stdout}\nstderr: {stderr}"
    );
    for arg in ["width", "height", "depth"] {
        expect_length_rejection(&stderr, "box", arg, "Int");
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
    let (status, stdout, stderr) = eval_source(
        "bare_fillet_radius",
        r#"module bare_fillet_radius

structure def S {
    let b = fillet(box(10mm, 10mm, 10mm), 1)
    param geometry : Solid = b
}
"#,
    );

    assert!(
        !status.success(),
        "a bare fillet radius must exit nonzero;\nstdout: {stdout}\nstderr: {stderr}"
    );
    expect_length_rejection(&stderr, "fillet", "radius", "Int");
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
    let (status, stdout, stderr) = eval_source(
        "bare_mirror_plane_offset",
        r#"module bare_mirror_plane_offset

structure def S {
    let b = box(20mm, 20mm, 10mm)
    let m = mirror(b, plane_yz(10))
    param geometry : Solid = m
}
"#,
    );

    assert!(
        !status.success(),
        "a bare plane offset must exit nonzero;\nstdout: {stdout}\nstderr: {stderr}"
    );
    expect_length_rejection(&stderr, "plane_yz", "offset", "Int");
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
    let (status, stdout, stderr) = eval_source(
        "bare_mirror_scalar_origin",
        r#"module bare_mirror_scalar_origin

structure def S {
    let b = box(20mm, 20mm, 10mm)
    let m = mirror(b, 10, 0, 0, 1, 0, 0)
    param geometry : Solid = m
}
"#,
    );

    assert!(
        !status.success(),
        "the scalar mirror origin must still exit nonzero;\nstdout: {stdout}\nstderr: {stderr}"
    );
    expect_length_rejection(&stderr, "mirror", "ox", "Int");
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
    let (status, stdout, stderr) = eval_source(
        "dimensioned_mirror_plane",
        r#"module dimensioned_mirror_plane

structure def S {
    let b = box(20mm, 20mm, 10mm)
    let m = mirror(b, plane_yz(10mm))
    let c = centroid(m)
    param geometry : Solid = m
}
"#,
    );

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

/// §6 row 7 — a bare translation component is rejected by NAME, replacing the generic
/// shape message that named nothing.
///
/// Both halves matter. Before the gate, `vec3(5, 0, 0)` failed as
/// `not a valid Transform<3>` — a whole-argument complaint that told the user neither
/// which component was wrong nor that units were the reason. The row's claim is that the
/// generic message was REPLACED, so its absence is asserted alongside the new one.
///
/// ARITY TRAP: `transform3` takes `(orientation, translation)`. A 4-arg spelling never
/// reaches the units gate at all — it fails the generic shape check first, and a fixture
/// written that way would read as a units failure while measuring something else.
///
/// The `Scalar{DIMENSIONLESS}` twin §6 row 7 also names gets NO fixture here, and that
/// is deliberate rather than an omission: ζ probed it and recorded the result at
/// `crates/reify-eval/tests/harness_geometry/transform_translation_length_units_e2e.rs:19-29`
/// — `5mm / 1mm` collapses to `Value::Real`, so no `.ri` source can express the twin, and
/// writing one anyway would exercise the `Real` path twice while claiming to cover it.
/// Its home is the unit row
/// `geometry_ops/tests.rs::compile_geometry_op_apply_transform_translation_follows_the_three_state_contract`,
/// which constructs the value directly; it enters this suite through the ledger.
#[test]
fn a_bare_transform_translation_names_the_component_not_the_shape() {
    let (status, stdout, stderr) = eval_source(
        "bare_transform_translation",
        r#"module bare_transform_translation

structure def S {
    let b = box(20mm, 20mm, 10mm)
    let moved = apply_transform(b, transform3(orient_identity(), vec3(5, 0, 0)))
    param geometry : Solid = moved
}
"#,
    );

    assert!(
        !status.success(),
        "a bare translation component must exit nonzero;\nstdout: {stdout}\nstderr: {stderr}"
    );
    expect_length_rejection(&stderr, "apply_transform", "translation.x", "Int");
    assert!(
        !stderr.contains("not a valid Transform<3>"),
        "the generic shape message must be REPLACED by the per-component units \
         diagnostic, not accompanied by it; got: {stderr}"
    );
}

/// §6 row 9 — D8: `reify check` and `reify eval` agree on the SAME bytes.
///
/// One source, two subcommands, one assertion block. `check` is the cheap pre-flight a
/// user reaches for first; if it passed what `eval` rejects, the gate would be advisory
/// rather than binding, and a bare-number habit would survive contact with it.
///
/// This holds today on η's compile slots alone, with no dependency on PRD 2's `reify
/// check` semantics — verified live. Should this row ever come to need those semantics,
/// that is a real dependency edge onto PRD 2's task, never a weakening of the assertion.
#[test]
fn check_and_eval_agree_on_a_bare_primitive_dimension() {
    let stem = "bare_box_both_subcommands";
    let source = r#"module bare_box_both_subcommands

structure def S {
    let b = box(20, 20, 10)
    param geometry : Solid = b
}
"#;

    for (subcommand, (status, stdout, stderr)) in [
        ("check", check_source(stem, source)),
        ("eval", eval_source(stem, source)),
    ] {
        assert!(
            !status.success(),
            "`reify {subcommand}` must exit nonzero on a bare primitive dimension;\n\
             stdout: {stdout}\nstderr: {stderr}"
        );
        expect_length_rejection(&stderr, "box", "width", "Int");
    }
}

/// §6 row 9b — the arity-6 `linear_pattern_2d` site is resolved as an ARITY error, and
/// was deliberately NOT given an arity-6 LENGTH slot.
///
/// §6 left this row open ("resolved either way"); the decompose-time correction settled
/// it as the malformed-fixture branch, so the row's postcondition is a PAIR: the arity
/// diagnostic is present AND no spacing slot was minted at that arity. Asserting only the
/// arity message would stay green if someone later added the slot, which is the outcome
/// the row exists to rule out — an arity-6 call has `count2` at index 5, so a slot there
/// would emit a FALSE units rejection on valid code (the reason
/// `builtin_signatures.rs` guards the slots with `arg_count == 11`).
///
/// Compile-layer twin:
/// `harness_compilation_surface/compile_api_tests.rs::compile_linear_pattern_2d_wrong_arity_produces_diagnostic`.
#[test]
fn the_arity_6_linear_pattern_2d_site_is_an_arity_error_not_a_length_slot() {
    let (status, stdout, stderr) = eval_source(
        "arity_6_linear_pattern_2d",
        r#"module arity_6_linear_pattern_2d

structure def S {
    let w = box(5mm, 5mm, 5mm)
    let p = linear_pattern_2d(w, 1, 0, 0, 3, 20)
    param geometry : Solid = p
}
"#,
    );

    assert!(
        !status.success(),
        "an arity-6 linear_pattern_2d call must exit nonzero;\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stderr.contains("linear_pattern_2d() expects 11 arguments, got 6"),
        "the site must resolve as an ARITY error; got: {stderr}"
    );
    for slot in ["spacing1", "spacing2"] {
        assert!(
            !stderr.contains(slot),
            "no `{slot}` LENGTH slot exists at arity 6 — index 5 is `count2` there, so a \
             slot would emit a FALSE units rejection on valid code; got: {stderr}"
        );
    }
}
