//! End-to-end `reify check` enforcement tests for the definition-time joint DOF
//! self-check — geometric-joints β (task 4396), the §7.1 self-checking law.
//!
//! A `joint NAME(datums) with <declared free DOF> = <relation body>` declaration
//! asserts, at definition time (before any solve), that its **declared** free DOF
//! matches the **geometric residual** the relation body leaves — by COUNT and by
//! KIND. On mismatch the compiler emits `DiagnosticCode::JointDofMismatch` (PRD
//! mnemonic `E_JOINT_DOF_MISMATCH`) with a geometric explanation.
//!
//! These cases pin the four PRD boundary tests (docs/prds/v0_6/geometric-joints.md
//! §7.1, B1–B4) end-to-end, using the REAL landed relation vocabulary
//! (`concentric`/`coincident` over `Axis` = 4 DOF = 2 rot + 2 trans; `on(Point,
//! Plane)` = 1 trans) — never the PRD-illustrative `coaxial`, which is not a
//! landed relation:
//!   - B1 revolute (`concentric` + `on`)        → residual (1 rot, 0 trans), declares
//!     `angle: Angle` = (1, 0)                   → CLEAN;
//!   - B4 cylindrical (`concentric`)            → residual (1 rot, 1 trans), declares
//!     `{ angle: Angle, travel: Length }` = (1, 1) → CLEAN;
//!   - B2 count fail (`concentric` only)        → residual (1, 1), declares
//!     `angle: Angle` = (1, 0)                   → ONE `JointDofMismatch`;
//!   - B3 kind fail (`concentric` + `on`)       → residual (1, 0), declares
//!     `travel: Length` = (0, 1)                 → ONE `JointDofMismatch`.
//!
//! RED until step-12 replaces the no-op `Declaration::Joint(_)` arm in
//! `compile_builder/entities_phase.rs` with the self-check: while the arm is a
//! no-op the joint body is never analysed, so B2/B3 emit nothing and their
//! "exactly one mismatch" assertions fail. (B1/B4 hold trivially before and
//! after — a clean joint never draws a mismatch.)

use reify_core::{Diagnostic, DiagnosticCode, Severity};
use reify_test_support::compile_source_with_stdlib;

/// The error-severity `JointDofMismatch` diagnostics emitted while compiling
/// `module` — the β joint-DOF self-check signal (mirrors δ's `relate_errors`).
fn joint_dof_errors(module: &reify_compiler::CompiledModule) -> Vec<&Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| {
            d.code == Some(DiagnosticCode::JointDofMismatch) && d.severity == Severity::Error
        })
        .collect()
}

// ── B1 / B4: clean joints (no mismatch) ──────────────────────────────────────

/// B1 — a revolute pair: `concentric(a, b)` (2 rot, 2 trans) + `on(p, stop)`
/// (0 rot, 1 trans) leaves residual (1 rot, 0 trans), and the declaration
/// `with angle: Angle` is exactly (1, 0). COUNT and KIND match → NO
/// `JointDofMismatch`. Holds both before and after step-12.
#[test]
fn b1_revolute_concentric_plus_on_is_clean() {
    let module = compile_source_with_stdlib(
        "joint revolute(a: Axis, b: Axis, p: Point3<Length>, stop: Plane) \
         with angle: Angle = { concentric(a, b)  on(p, stop) }",
    );
    let errs = joint_dof_errors(&module);
    assert!(
        errs.is_empty(),
        "B1 revolute (residual 1 rotational, declares `angle: Angle`) must NOT draw \
         E_JOINT_DOF_MISMATCH, got: {errs:#?}",
    );
}

/// B4 — a cylindrical pair: `concentric(a, b)` (2 rot, 2 trans) leaves residual
/// (1 rot, 1 trans), and the record declaration `with { angle: Angle, travel:
/// Length }` is exactly (1, 1). Match → NO `JointDofMismatch`. Holds before and
/// after step-12.
#[test]
fn b4_cylindrical_record_is_clean() {
    let module = compile_source_with_stdlib(
        "joint cylindrical(a: Axis, b: Axis) \
         with { angle: Angle, travel: Length } = concentric(a, b)",
    );
    let errs = joint_dof_errors(&module);
    assert!(
        errs.is_empty(),
        "B4 cylindrical (residual 1 rot + 1 trans, declares `{{ angle, travel }}`) must NOT \
         draw E_JOINT_DOF_MISMATCH, got: {errs:#?}",
    );
}

// ── B2: COUNT mismatch ───────────────────────────────────────────────────────

/// B2 — COUNT fail: `concentric(a, b)` alone leaves residual (1 rot, 1 trans),
/// but the declaration `with angle: Angle` is only (1, 0). The uncovered
/// translational freedom must surface exactly one `JointDofMismatch` whose
/// message names the residual `1 rot + 1 trans`.
///
/// RED: the no-op Joint arm never analyses the body, so zero mismatches are
/// emitted and `errs.len() == 1` fails.
#[test]
fn b2_count_mismatch_concentric_only() {
    let module = compile_source_with_stdlib(
        "joint bad(a: Axis, b: Axis) with angle: Angle = concentric(a, b)",
    );
    let errs = joint_dof_errors(&module);
    assert_eq!(
        errs.len(),
        1,
        "B2 (residual 1 rot + 1 trans, declares only `angle: Angle`) must draw exactly one \
         E_JOINT_DOF_MISMATCH.\nAll diagnostics: {:#?}",
        module.diagnostics
    );
    assert!(
        errs[0].message.contains("1 rot + 1 trans"),
        "B2 message must state the geometric residual `1 rot + 1 trans`: {}",
        errs[0].message
    );
}

// ── B3: KIND mismatch ────────────────────────────────────────────────────────

/// B3 — KIND fail: `concentric(a, b)` + `on(p, stop)` leaves residual (1 rot,
/// 0 trans), but the declaration `with travel: Length` is (0, 1). The COUNTS
/// agree (1 == 1) yet the KINDS disagree — a translational declaration cannot
/// absorb a rotational residual — so exactly one `JointDofMismatch` is emitted
/// naming the translational-vs-rotational disagreement.
///
/// RED: the no-op Joint arm emits nothing, so `errs.len() == 1` fails.
#[test]
fn b3_kind_mismatch_travel_vs_rotational_residual() {
    let module = compile_source_with_stdlib(
        "joint kindbad(a: Axis, b: Axis, p: Point3<Length>, stop: Plane) \
         with travel: Length = { concentric(a, b)  on(p, stop) }",
    );
    let errs = joint_dof_errors(&module);
    assert_eq!(
        errs.len(),
        1,
        "B3 (residual 1 rotational, declares `travel: Length`) must draw exactly one \
         E_JOINT_DOF_MISMATCH for the kind disagreement.\nAll diagnostics: {:#?}",
        module.diagnostics
    );
    assert!(
        errs[0].message.contains("translational"),
        "B3 message must name the declared translational kind that disagrees with the \
         rotational residual: {}",
        errs[0].message
    );
}

// ── CI example: examples/joint_dof_self_check.ri ─────────────────────────────

/// The committed CI example must compile CLEAN — zero Error-severity diagnostics,
/// and in particular zero `JointDofMismatch`. The example carries only PASSING
/// joints (a revolute single form + a cylindrical record form); the fail modes
/// (B2/B3 above) live in this test module, since one must-pass `.ri` cannot also
/// fail.
///
/// This is also covered by `examples_smoke::all_examples_parse_and_compile_with_stdlib`,
/// which walks every `examples/*.ri`; this dedicated test pins the joint example
/// by name with a sharper assertion that explicitly names the `JointDofMismatch`
/// code.
///
/// RED: `examples/joint_dof_self_check.ri` does not exist yet, so the read fails.
#[test]
fn ci_example_compiles_clean() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/joint_dof_self_check.ri"
    );
    let source = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read CI example `{path}`: {e}"));
    let module = compile_source_with_stdlib(&source);

    let errors: Vec<&Diagnostic> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "examples/joint_dof_self_check.ri must compile with zero Error diagnostics, got: {errors:#?}",
    );
    assert!(
        joint_dof_errors(&module).is_empty(),
        "examples/joint_dof_self_check.ri must emit zero E_JOINT_DOF_MISMATCH",
    );
}

// ── DOF `in <range>` dimensional consistency ─────────────────────────────────

/// The error-severity range/DOF dimension-mismatch diagnostics — β's `in <range>`
/// dimensional consistency check (the compile-time analog of the runtime
/// `validate_range` dimensional guard). Coded `ArgTypeMismatch`, message names
/// the `range`.
fn range_dim_errors(module: &reify_compiler::CompiledModule) -> Vec<&Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| {
            d.code == Some(DiagnosticCode::ArgTypeMismatch)
                && d.severity == Severity::Error
                && d.message.contains("range")
        })
        .collect()
}

/// A `with angle: Angle in 0mm..10mm` DOF declares an ANGLE freedom but bounds
/// its travel with a LENGTH range — a dimensional contradiction the §7.1 range
/// check must reject (the compile-time analog of the runtime `validate_range`
/// dimensional guard). Exactly one range-dimension error; the DOF COUNT/KIND
/// itself is fine (residual (1,0) == declared (1,0)), so no `JointDofMismatch`.
///
/// RED: β does not yet validate the range, so the mismatch emits nothing.
#[test]
fn range_dimension_mismatch_angle_dof_length_range() {
    let module = compile_source_with_stdlib(
        "joint r(a: Axis, b: Axis, p: Point3<Length>, stop: Plane) \
         with angle: Angle in 0mm..10mm = { concentric(a, b)  on(p, stop) }",
    );
    let errs = range_dim_errors(&module);
    assert_eq!(
        errs.len(),
        1,
        "an `angle: Angle` DOF bounded by a `0mm..10mm` (Length) range must draw exactly one \
         range-dimension error.\nAll diagnostics: {:#?}",
        module.diagnostics
    );
    // The COUNT/KIND law is satisfied — only the range dimension is wrong.
    assert!(
        joint_dof_errors(&module).is_empty(),
        "the range-dimension mismatch must NOT also draw a JointDofMismatch (residual matches): {:#?}",
        joint_dof_errors(&module)
    );
}

/// CONTROL: the same joint with a dimensionally-matching `in 0deg..120deg`
/// (Angle) range over the `angle: Angle` DOF draws NO range-dimension error.
/// Holds both before and after step-16 (this is B1 with an explicit range).
#[test]
fn range_dimension_match_angle_dof_angle_range_is_clean() {
    let module = compile_source_with_stdlib(
        "joint r(a: Axis, b: Axis, p: Point3<Length>, stop: Plane) \
         with angle: Angle in 0deg..120deg = { concentric(a, b)  on(p, stop) }",
    );
    assert!(
        range_dim_errors(&module).is_empty(),
        "a dimensionally-matching `0deg..120deg` range over an `angle: Angle` DOF must NOT draw \
         a range-dimension error, got: {:#?}",
        range_dim_errors(&module)
    );
}

// ── Unclassifiable DOF field type ────────────────────────────────────────────

/// The `E_ARG_TYPE_MISMATCH` diagnostics naming a DOF field whose declared type
/// has no geometric joint-DOF kind (the per-field surfacing — distinct from the
/// `in <range>` dimension check, which also uses `ArgTypeMismatch` but whose
/// message mentions `range`).
fn dof_kind_errors(module: &reify_compiler::CompiledModule) -> Vec<&Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| {
            d.code == Some(DiagnosticCode::ArgTypeMismatch)
                && d.severity == Severity::Error
                && d.message.contains("not a valid joint DOF kind")
        })
        .collect()
}

/// A DOF declared with a type that is neither `Angle`, `Length`, nor
/// `Orientation` (here a datum `Axis`) has no geometric kind to match against the
/// residual. β surfaces this per-field with a targeted `E_ARG_TYPE_MISMATCH`
/// naming the offending field — and SUPPRESSES the count/kind verdict, since a
/// `(0, 0)` contribution would otherwise emit a confusing `E_JOINT_DOF_MISMATCH`
/// that never names the real problem.
#[test]
fn unclassifiable_dof_type_is_surfaced_per_field_and_suppresses_the_verdict() {
    let module =
        compile_source_with_stdlib("joint j(a: Axis, b: Axis) with foo: Axis = concentric(a, b)");
    let errs = dof_kind_errors(&module);
    assert_eq!(
        errs.len(),
        1,
        "a DOF declared `foo: Axis` must draw exactly one targeted \
         not-a-valid-DOF-kind error.\nAll diagnostics: {:#?}",
        module.diagnostics
    );
    assert!(
        errs[0].message.contains("foo"),
        "the diagnostic must name the offending DOF field `foo`: {}",
        errs[0].message
    );
    assert!(
        joint_dof_errors(&module).is_empty(),
        "the unclassifiable-DOF diagnostic must REPLACE (not accompany) a confusing \
         JointDofMismatch: {:#?}",
        joint_dof_errors(&module)
    );
}

// ── Gradualism: body member with a curated count but no kind split ────────────

/// `tangent` has a curated DOF COUNT (`relation_delta_dof` = 2) but no
/// rotational/translational split (`relation_delta_dof_kinds` = `None`, because
/// it is surface-conditional). `residual_kinds` omits it, INFLATING the residual
/// above the true geometry — so a count/kind verdict computed from that residual
/// would draw a SPURIOUS `E_JOINT_DOF_MISMATCH`.
///
/// PRD §7.1 gradualism requires the verdict be suppressed when the body contains
/// any such count-known/kind-unknown member. Here `concentric(a, b)` (residual
/// the kind table reads as (1, 1)) plus `tangent(a, b)` would mismatch the
/// `angle: Angle` = (1, 0) declaration under the naive computation, but must NOT:
/// the residual cannot be fully attributed, so no mismatch is emitted.
#[test]
fn body_with_undecidable_kind_split_suppresses_spurious_mismatch() {
    let module = compile_source_with_stdlib(
        "joint j(a: Axis, b: Axis) with angle: Angle = { concentric(a, b)  tangent(a, b) }",
    );
    assert!(
        joint_dof_errors(&module).is_empty(),
        "a body with a count-known/kind-unknown member (`tangent`) must suppress the \
         count/kind verdict (gradualism), drawing zero E_JOINT_DOF_MISMATCH: {:#?}",
        joint_dof_errors(&module)
    );
}
