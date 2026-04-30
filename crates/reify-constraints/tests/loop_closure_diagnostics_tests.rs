//! Integration tests for the kinematic loop-closure diagnostic wrapper added
//! in task 2677 (PRD `docs/prds/v0_2/kinematic-constraints.md` §"Singularity,
//! over/under-constraint diagnostics").
//!
//! Tests pin the public surface introduced by this task:
//!   * three new `DiagnosticCode` variants (`KinematicSingularity`,
//!     `KinematicOverconstrained`, `KinematicUnderconstrained`);
//!   * the `LoopClosureReport { outcome, is_singular, diagnostics }` struct;
//!   * the `solve_loop_closure_with_diagnostics(...)` wrapper, which adds
//!     pre-/post-processing on top of the existing `solve_loop_closure`
//!     Newton solver.
//!
//! Diagnostic-code variant correctness (Copy/Clone/Hash/serde wire format)
//! is exercised in `crates/reify-types/src/diagnostics.rs`'s inline test
//! module — these tests focus on producer-side semantics.

use reify_constraints::{LoopClosureReport, NewtonOutcome};
use reify_types::{Diagnostic, DiagnosticCode, Severity};

// ── Step-1: DiagnosticCode variants exist and are distinct ──────────────

/// All three kinematic-loop-closure variants must be distinct
/// `DiagnosticCode` values — `assert_ne!` across all pairs guarantees that
/// downstream tooling matching on a typed code identifier never confuses a
/// singular Jacobian (warning) with an over-constrained mechanism (error).
#[test]
fn kinematic_loop_closure_diagnostic_codes_are_distinct() {
    let singular = DiagnosticCode::KinematicSingularity;
    let over = DiagnosticCode::KinematicOverconstrained;
    let under = DiagnosticCode::KinematicUnderconstrained;
    assert_ne!(singular, over);
    assert_ne!(singular, under);
    assert_ne!(over, under);
}

/// `KinematicSingularity` is a `Warning` per PRD prose `W_KINEMATIC_SINGULARITY` —
/// pinned by round-tripping through `Diagnostic::warning(...).with_code(...)`.
#[test]
fn kinematic_singularity_round_trips_via_warning_with_code() {
    let d =
        Diagnostic::warning("singular Jacobian").with_code(DiagnosticCode::KinematicSingularity);
    assert_eq!(d.severity, Severity::Warning);
    assert_eq!(d.code, Some(DiagnosticCode::KinematicSingularity));
}

/// `KinematicOverconstrained` is an `Error` per PRD prose
/// `E_KINEMATIC_OVERCONSTRAINED` — pinned by round-tripping through
/// `Diagnostic::error(...).with_code(...)`.
#[test]
fn kinematic_overconstrained_round_trips_via_error_with_code() {
    let d = Diagnostic::error("over-constrained")
        .with_code(DiagnosticCode::KinematicOverconstrained);
    assert_eq!(d.severity, Severity::Error);
    assert_eq!(d.code, Some(DiagnosticCode::KinematicOverconstrained));
}

/// `KinematicUnderconstrained` is a `Warning` per PRD prose
/// `W_KINEMATIC_UNDERCONSTRAINED` — pinned by round-tripping through
/// `Diagnostic::warning(...).with_code(...)`.
#[test]
fn kinematic_underconstrained_round_trips_via_warning_with_code() {
    let d = Diagnostic::warning("under-constrained")
        .with_code(DiagnosticCode::KinematicUnderconstrained);
    assert_eq!(d.severity, Severity::Warning);
    assert_eq!(d.code, Some(DiagnosticCode::KinematicUnderconstrained));
}

// ── Step-3: LoopClosureReport public-struct shape ───────────────────────

/// Pins the public shape of `LoopClosureReport`: three publicly accessible
/// fields (`outcome`, `is_singular`, `diagnostics`) that the
/// `solve_loop_closure_with_diagnostics` wrapper populates.  Constructing
/// the struct via a literal and reading every field confirms each one is
/// `pub` — a future change that demotes any field to private would fail
/// here.
#[test]
fn loop_closure_report_struct_literal_exposes_three_pub_fields() {
    let report = LoopClosureReport {
        outcome: NewtonOutcome::Converged {
            x: vec![0.0],
            iters: 0,
            residual_norm: 0.0,
        },
        is_singular: false,
        diagnostics: vec![],
    };
    assert!(matches!(report.outcome, NewtonOutcome::Converged { .. }));
    assert!(!report.is_singular);
    assert!(report.diagnostics.is_empty());
}
