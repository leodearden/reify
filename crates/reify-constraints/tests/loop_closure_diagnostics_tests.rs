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

use reify_constraints::{
    LoopClosureReport, NewtonConfig, NewtonOutcome, StartStrategy,
    solve_loop_closure_with_diagnostics,
};
use reify_stdlib::eval_builtin;
use reify_types::{Diagnostic, DiagnosticCode, Severity, Value};

// ── Test fixtures (mirrors the inline helpers in loop_closure.rs) ──────

fn axis_x() -> Value {
    Value::Vector(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)])
}

fn axis_y() -> Value {
    Value::Vector(vec![Value::Real(0.0), Value::Real(1.0), Value::Real(0.0)])
}

fn axis_z() -> Value {
    Value::Vector(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(1.0)])
}

fn length_range(lo: f64, up: f64) -> Value {
    Value::Range {
        lower: Some(Box::new(Value::length(lo))),
        upper: Some(Box::new(Value::length(up))),
        lower_inclusive: true,
        upper_inclusive: true,
    }
}

fn angle_range(lo: f64, up: f64) -> Value {
    Value::Range {
        lower: Some(Box::new(Value::angle(lo))),
        upper: Some(Box::new(Value::angle(up))),
        lower_inclusive: true,
        upper_inclusive: true,
    }
}

fn prismatic_x_0_to_1() -> Value {
    eval_builtin("prismatic", &[axis_x(), length_range(0.0, 1.0)])
}

fn prismatic_y_0_to_1() -> Value {
    eval_builtin("prismatic", &[axis_y(), length_range(0.0, 1.0)])
}

fn prismatic_z_0_to_1() -> Value {
    eval_builtin("prismatic", &[axis_z(), length_range(0.0, 1.0)])
}

fn revolute_x_0_to_pi() -> Value {
    eval_builtin(
        "revolute",
        &[axis_x(), angle_range(0.0, std::f64::consts::PI)],
    )
}

fn revolute_y_0_to_pi() -> Value {
    eval_builtin(
        "revolute",
        &[axis_y(), angle_range(0.0, std::f64::consts::PI)],
    )
}

fn revolute_z_0_to_pi() -> Value {
    eval_builtin(
        "revolute",
        &[axis_z(), angle_range(0.0, std::f64::consts::PI)],
    )
}

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

// ── Step-5: over-constrained pre-check (free_b.len() < 6) ───────────────

/// 1 free DOF against a 6-component twist residual is over-constrained
/// (free_b.len() = 1 < 6).  The wrapper must:
///   * short-circuit the Newton solve (NotConverged outcome, no plausible
///     config — the diagnostic IS the user-facing signal of structural
///     infeasibility per PRD prose);
///   * emit exactly one Error-severity diagnostic with code
///     `KinematicOverconstrained`;
///   * keep `is_singular` false (no Newton run → no singularity check).
#[test]
fn solve_loop_closure_with_diagnostics_emits_overconstrained_for_one_dof() {
    let chain_a = vec![prismatic_x_0_to_1()];
    let vals_a = vec![0.5];
    let chain_b = vec![prismatic_x_0_to_1()];
    let vals_b_initial = vec![0.0];
    let free_b = vec![0]; // 1 < 6 → over-constrained
    let strategy = StartStrategy::WarmStart(vec![0.0]);
    let cfg = NewtonConfig::default();

    let report = solve_loop_closure_with_diagnostics(
        &chain_a,
        &vals_a,
        &chain_b,
        &vals_b_initial,
        &free_b,
        &strategy,
        &cfg,
    );

    assert_eq!(
        report.diagnostics.len(),
        1,
        "expected exactly one over-constrained diagnostic, got {:?}",
        report.diagnostics
    );
    let d = &report.diagnostics[0];
    assert_eq!(d.severity, Severity::Error);
    assert_eq!(d.code, Some(DiagnosticCode::KinematicOverconstrained));
    assert!(
        matches!(report.outcome, NewtonOutcome::NotConverged { .. }),
        "over-constrained short-circuit must return NotConverged, got {:?}",
        report.outcome
    );
    assert!(!report.is_singular, "no Newton run → is_singular must stay false");
}

// ── Step-7: under-constrained pre-check (free_b.len() > 6) ──────────────

/// 7 prismatic-x joints all on the +X axis is under-constrained
/// (free_b.len() = 7 > 6) AND structurally singular (all 7 free vars
/// contribute to the same +X translation, so the Jacobian is rank-1).
///
/// With both the under-constrained pre-check (step-8) and the singularity
/// post-process (step-10) wired, this single problem co-emits BOTH
/// warnings:
///   * `KinematicUnderconstrained` (Warning) — DOF-balance pre-check;
///   * `KinematicSingularity` (Warning) — rank-deficient Jacobian
///     post-process.
///
/// Outcome MUST be `NewtonOutcome::Singular` — that is the LDLᵀ pivot
/// guard's signal, and it is also the only outcome that produces the
/// second (singularity) diagnostic this test expects.  Asserting
/// `Singular` directly proves both that the under-constrained branch
/// DELEGATES to the solver (rather than short-circuiting) AND that the
/// singularity post-process fires; a regression that made this rank-1
/// chain converge would surface as a clear "expected Singular" panic
/// here, instead of a confusing diagnostic-count mismatch.
#[test]
fn solve_loop_closure_with_diagnostics_emits_underconstrained_for_seven_dofs() {
    let chain_a = vec![prismatic_x_0_to_1()];
    let vals_a = vec![0.5];
    let chain_b: Vec<Value> = (0..7).map(|_| prismatic_x_0_to_1()).collect();
    let vals_b_initial = vec![0.0; 7];
    let free_b: Vec<usize> = (0..7).collect();
    let strategy = StartStrategy::WarmStart(vec![0.0; 7]);
    let cfg = NewtonConfig::default();

    let report = solve_loop_closure_with_diagnostics(
        &chain_a,
        &vals_a,
        &chain_b,
        &vals_b_initial,
        &free_b,
        &strategy,
        &cfg,
    );

    // Two diagnostics: under-constrained pre-check + singular post-process.
    // Both are Warning severity per PRD prose (W_*).
    assert_eq!(
        report.diagnostics.len(),
        2,
        "expected under-constrained AND singularity warnings on rank-deficient 7-prismatic-x chain, got {:?}",
        report.diagnostics
    );
    assert!(
        report.diagnostics.iter().any(|d| {
            d.severity == Severity::Warning
                && d.code == Some(DiagnosticCode::KinematicUnderconstrained)
        }),
        "missing KinematicUnderconstrained warning in {:?}",
        report.diagnostics
    );
    assert!(
        report.diagnostics.iter().any(|d| {
            d.severity == Severity::Warning
                && d.code == Some(DiagnosticCode::KinematicSingularity)
        }),
        "missing KinematicSingularity warning in {:?}",
        report.diagnostics
    );

    // Outcome must be Singular — required by the LDLᵀ pivot guard on a
    // rank-1 Jacobian and the only variant that triggers the singularity
    // post-process (which the diagnostic-count assertion above pins).
    assert!(
        matches!(report.outcome, NewtonOutcome::Singular { .. }),
        "expected NewtonOutcome::Singular from rank-deficient 7-prismatic-x chain, got {:?}",
        report.outcome
    );
}

// ── Step-9: singularity post-process (rank-deficient Jacobian) ──────────

/// 6 prismatic joints all on +X axis is balanced (free_b.len() = 6 == 6,
/// no DOF imbalance) but structurally singular: the Jacobian is rank-1
/// (all 6 columns project onto the same +X linear contribution), so the
/// LDLᵀ pivot guard inside `newton_solve` returns
/// `NewtonOutcome::Singular`.
///
/// The wrapper's job is to translate that signal into the PRD's
/// `W_KINEMATIC_SINGULARITY` warning class while preserving the
/// last-converged config in the `Singular` variant's `x` payload.
///
/// Pinning that no `KinematicOverconstrained` / `KinematicUnderconstrained`
/// entry leaks in confirms the singularity branch is independent of the
/// DOF-balance pre-checks.
#[test]
fn solve_loop_closure_with_diagnostics_emits_singularity_for_rank_one_chain() {
    let chain_a = vec![prismatic_x_0_to_1()];
    let vals_a = vec![0.5];
    let chain_b: Vec<Value> = (0..6).map(|_| prismatic_x_0_to_1()).collect();
    let vals_b_initial = vec![0.5; 6];
    let free_b: Vec<usize> = (0..6).collect();
    let strategy = StartStrategy::WarmStart(vec![0.0; 6]);
    let cfg = NewtonConfig::default();

    let report = solve_loop_closure_with_diagnostics(
        &chain_a,
        &vals_a,
        &chain_b,
        &vals_b_initial,
        &free_b,
        &strategy,
        &cfg,
    );

    assert!(
        report.is_singular,
        "expected is_singular=true on rank-deficient Jacobian, got is_singular={} \
         (outcome={:?})",
        report.is_singular,
        report.outcome,
    );
    match &report.outcome {
        NewtonOutcome::Singular { x, .. } => {
            assert_eq!(
                x.len(),
                6,
                "Singular outcome must preserve the last-converged config (6 free vars), \
                 got x.len()={}",
                x.len()
            );
        }
        other => panic!(
            "expected NewtonOutcome::Singular from rank-deficient Jacobian, got {other:?}"
        ),
    }

    // Exactly one diagnostic, and it MUST be the singularity warning — no
    // bleed-through from the over/under-constrained pre-checks (free_b.len()=6).
    assert_eq!(
        report.diagnostics.len(),
        1,
        "expected exactly one singularity diagnostic, got {:?}",
        report.diagnostics
    );
    let d = &report.diagnostics[0];
    assert_eq!(d.severity, Severity::Warning);
    assert_eq!(d.code, Some(DiagnosticCode::KinematicSingularity));
    assert!(
        !report.diagnostics.iter().any(|x| matches!(
            x.code,
            Some(DiagnosticCode::KinematicOverconstrained)
                | Some(DiagnosticCode::KinematicUnderconstrained)
        )),
        "balanced free-DOF count (6) must not emit over/under-constrained diagnostics, got {:?}",
        report.diagnostics
    );
}

// ── Coverage gaps surfaced in the amendment review ──────────────────────

/// The over-constrained branch of the wrapper has special-case handling
/// for `StartStrategy::Midpoint` (post-amendment: query each free joint's
/// midpoint to populate the returned `x`).  Pinning that the Midpoint
/// strategy actually drives `x` (not just the WarmStart fallback) means a
/// regression to the old `filter_map(vals_b_initial[i])` shortcut would
/// break this assertion: prismatic_x_0_to_1's midpoint is 0.5, which is
/// distinct from `vals_b_initial[0] = 0.0` here.
#[test]
fn solve_loop_closure_with_diagnostics_overconstrained_midpoint_uses_joint_midpoint() {
    let chain_a = vec![prismatic_x_0_to_1()];
    let vals_a = vec![0.5];
    let chain_b = vec![prismatic_x_0_to_1()];
    let vals_b_initial = vec![0.0];
    let free_b = vec![0]; // 1 < 6 → over-constrained
    let strategy = StartStrategy::Midpoint;
    let cfg = NewtonConfig::default();

    let report = solve_loop_closure_with_diagnostics(
        &chain_a,
        &vals_a,
        &chain_b,
        &vals_b_initial,
        &free_b,
        &strategy,
        &cfg,
    );

    assert_eq!(
        report.diagnostics.len(),
        1,
        "expected exactly one over-constrained diagnostic, got {:?}",
        report.diagnostics
    );
    let d = &report.diagnostics[0];
    assert_eq!(d.severity, Severity::Error);
    assert_eq!(d.code, Some(DiagnosticCode::KinematicOverconstrained));
    assert!(!report.is_singular, "no Newton run → is_singular must stay false");
    match &report.outcome {
        NewtonOutcome::NotConverged { x, residual_norm } => {
            assert_eq!(x.len(), 1, "x must align positionally with free_b");
            assert!(
                (x[0] - 0.5).abs() < 1e-12,
                "Midpoint strategy must populate x with joint midpoint (0.5 for 0..1m \
                 prismatic_x), got {:?}",
                x[0]
            );
            assert!(
                !residual_norm.is_finite(),
                "over-constrained short-circuit must signal infeasibility via \
                 INFINITY residual_norm, got {residual_norm}"
            );
        }
        other => panic!("over-constrained short-circuit must return NotConverged, got {other:?}"),
    }
}

/// Balanced (free_b.len() == 6) AND non-singular happy path: three
/// prismatic plus three revolute joints on three orthogonal axes span
/// all 6 components of the closure-residual twist, so the Jacobian is
/// full-rank.  With `chain_a == chain_b` at `vals_a == vals_b_initial`,
/// the residual is identically zero from the first iteration → the
/// solver converges in 0 iters with no DOF imbalance and no
/// rank-deficiency.
///
/// Pinning that the wrapper emits NO diagnostics on the balanced
/// non-singular path complements the singular-balanced test above: a
/// regression that emitted spurious diagnostics on a healthy mechanism
/// (e.g. a stale check that fired on every `free_b.len() == 6` call)
/// would surface here.
#[test]
fn solve_loop_closure_with_diagnostics_balanced_full_rank_emits_no_diagnostics() {
    let chain: Vec<Value> = vec![
        prismatic_x_0_to_1(),
        prismatic_y_0_to_1(),
        prismatic_z_0_to_1(),
        revolute_x_0_to_pi(),
        revolute_y_0_to_pi(),
        revolute_z_0_to_pi(),
    ];
    let vals = vec![0.5, 0.5, 0.5, 0.0, 0.0, 0.0];
    let chain_a = chain.clone();
    let vals_a = vals.clone();
    let chain_b = chain;
    let vals_b_initial = vals.clone();
    let free_b: Vec<usize> = (0..6).collect();
    let strategy = StartStrategy::WarmStart(vals.clone());
    let cfg = NewtonConfig::default();

    let report = solve_loop_closure_with_diagnostics(
        &chain_a,
        &vals_a,
        &chain_b,
        &vals_b_initial,
        &free_b,
        &strategy,
        &cfg,
    );

    assert!(
        report.diagnostics.is_empty(),
        "balanced non-singular path must emit no diagnostics, got {:?}",
        report.diagnostics
    );
    assert!(
        !report.is_singular,
        "balanced non-singular path must NOT lift is_singular, got is_singular={} (outcome={:?})",
        report.is_singular,
        report.outcome,
    );
    // Outcome must be Converged (zero initial residual) — anything else
    // would mean the solver failed on the trivial identity case.
    assert!(
        matches!(report.outcome, NewtonOutcome::Converged { .. }),
        "expected Converged on identity residual, got {:?}",
        report.outcome
    );
}

/// Fills the (under-constrained × full-rank) cell of the 2×2 (pre-check ×
/// singularity) coverage matrix:
///
/// |                      | full-rank Jacobian                              | singular Jacobian                                      |
/// |----------------------|-------------------------------------------------|--------------------------------------------------------|
/// | balanced (len==6)    | `balanced_full_rank_emits_no_diagnostics` ✓     | `emits_singularity_for_rank_one_chain` ✓               |
/// | under-constrained    | **this test** ✓                                 | `emits_underconstrained_for_seven_dofs` ✓              |
///
/// ## Construction
///
/// The chain is `[prismatic_x, prismatic_y, prismatic_z, revolute_x,
/// revolute_y, revolute_z, prismatic_x]` — 7 joints.  The first 6 span all
/// 6 components of the closure-residual twist (proven in
/// `balanced_full_rank_emits_no_diagnostics`); the 7th redundant
/// `prismatic_x` pushes `free_b.len()` to 7, triggering the
/// under-constrained pre-check (`free_b.len() > SINGLE_LOOP_RESIDUAL_COUNT`).
///
/// `chain_a == chain_b` at `vals_a == vals_b_initial` produces an
/// identically-zero residual at iteration 0.  `newton_solve`'s convergence
/// check fires BEFORE `JᵀJ` is built or `LDLᵀ` is invoked, so the solver
/// returns `Converged` in 0
/// iters.  With `LDLᵀ` never running, `NewtonOutcome::Singular` is
/// **structurally unreachable** → the wrapper's singularity post-process
/// cannot fire.
///
/// ## What this pins
///
/// A regression that broke ONLY the under-constrained branch (without also
/// breaking the singularity branch) would still pass
/// `emits_underconstrained_for_seven_dofs` (because the singularity warning
/// would still appear), but it would FAIL HERE because the under-constrained
/// warning would be absent and the diagnostic count would be 0 instead of 1.
#[test]
fn solve_loop_closure_with_diagnostics_emits_underconstrained_with_full_rank_jacobian() {
    let chain: Vec<Value> = vec![
        prismatic_x_0_to_1(),
        prismatic_y_0_to_1(),
        prismatic_z_0_to_1(),
        revolute_x_0_to_pi(),
        revolute_y_0_to_pi(),
        revolute_z_0_to_pi(),
        prismatic_x_0_to_1(), // 7th joint: redundant → free_b.len()=7 > 6
    ];
    let vals = vec![0.5, 0.5, 0.5, 0.0, 0.0, 0.0, 0.5];
    let chain_a = chain.clone();
    let vals_a = vals.clone();
    let chain_b = chain;
    let vals_b_initial = vals.clone(); // identity residual: chain_a == chain_b at same vals
    let free_b: Vec<usize> = (0..7).collect();
    let strategy = StartStrategy::WarmStart(vals.clone());
    let cfg = NewtonConfig::default();

    let report = solve_loop_closure_with_diagnostics(
        &chain_a,
        &vals_a,
        &chain_b,
        &vals_b_initial,
        &free_b,
        &strategy,
        &cfg,
    );

    // At least one diagnostic must exist: the under-constrained pre-check warning.
    // Using >=1 (rather than ==1) keeps this check independent of the
    // KinematicSingularity bleed-through assertion below — both carry real weight.
    assert!(
        report.diagnostics.len() >= 1,
        "expected at least one under-constrained diagnostic, got {:?}",
        report.diagnostics
    );
    let d = &report.diagnostics[0];
    assert_eq!(
        d.severity,
        Severity::Warning,
        "KinematicUnderconstrained must be Warning severity, got {:?}",
        d.severity
    );
    assert_eq!(
        d.code,
        Some(DiagnosticCode::KinematicUnderconstrained),
        "diagnostic code must be KinematicUnderconstrained, got {:?}",
        d.code
    );

    // Decoupling guarantee: no singularity bleed-through.
    // Independent of the >=1 length check above — a regression that caused
    // KinematicSingularity to co-emit (e.g. if the LDLᵀ path fired) would
    // fail here even if the under-constrained diagnostic is also present.
    assert!(
        !report
            .diagnostics
            .iter()
            .any(|d| d.code == Some(DiagnosticCode::KinematicSingularity)),
        "full-rank construction must NOT emit KinematicSingularity, got {:?}",
        report.diagnostics
    );

    // is_singular must stay false — the LDLᵀ path was never taken.
    assert!(
        !report.is_singular,
        "full-rank construction must NOT lift is_singular, got is_singular={} (outcome={:?})",
        report.is_singular,
        report.outcome,
    );

    // Zero initial residual → Converged in 0 iters.
    assert!(
        matches!(report.outcome, NewtonOutcome::Converged { .. }),
        "expected Converged on identity residual, got {:?}",
        report.outcome
    );
}
