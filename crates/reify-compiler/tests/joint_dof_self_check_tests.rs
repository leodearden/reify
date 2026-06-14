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
