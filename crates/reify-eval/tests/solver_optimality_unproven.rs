//! Engine-level tests for W_SOLVER_OPTIMALITY_UNPROVEN (task #4804, γ):
//! verify that the eval engine surfaces `DiagnosticCode::SolverOptimalityUnproven`
//! when an objective solve hits the iteration limit (B4), and does NOT surface it
//! for a converged solve (B6 — no false-positive).
//!
//! Two cases via inline `.ri` sources (no geometry → stub-mode safe):
//!
//!   (B4) LARGE-SI-magnitude `minimize` — MaxIters fired, warning expected.
//!        Uses `constraint x > 8m` (within default Length bounds of [1µm, 10m]).
//!        Initial midpoint ~5m is infeasible → max_iters = 5000 (MAX_ITERS).
//!        At the constraint boundary x ≈ 8m, ULP(8)² ≈ 3.1e-30 > NM_SD_TOLERANCE=1e-30,
//!        so the Nelder-Mead cost SD floor stays above the threshold → MaxItersReached
//!        → iter_limited=true → BestFound{reason~"iteration limit"} → warning.
//!
//!        Also asserts (I1 guard): resolved value satisfies constraint x ≥ 8 m.
//!
//!   (B6) Small mm-scale `minimize` — converges, NO warning expected.
//!        SD floor (~mm²·1e-32) is far below NM_SD_TOLERANCE=1e-30 → early exit
//!        → iter_limited=false → BestFound{reason~"converged within iteration budget"}
//!        → no warning.
//!
//! RED until step-4 reroutes the main eval objective path to `solve_ranked` and
//! pushes the warning.

use reify_constraints::DimensionalSolver;
use reify_core::{DiagnosticCode, Severity, ValueCellId};
use reify_eval::Engine;
use reify_ir::Value;
use reify_test_support::{MockConstraintChecker, compile_source_with_stdlib};

/// B4 source: LARGE-SI-magnitude param, pure constraint/objective solve, no geometry.
///
/// `x` is a `Length = auto` param with default solver bounds (1µm, 10m).
/// The initial midpoint is ~5m; `constraint x > 8m` makes the initial point
/// infeasible (5m < 8m) → `max_iters = MAX_ITERS = 5000`.
///
/// `minimize x` drives the solver to find x just above 8m.  At the constraint
/// boundary (x ≈ 8m), the ULP of 8.0 in f64 is ~1.76e-15 m, so the squared
/// residual floor (~ULP²) ≈ 3.1e-30 > NM_SD_TOLERANCE = 1e-30.  The SD of
/// Nelder-Mead costs stays above the convergence threshold → MaxItersReached
/// → iter_limited=true → BestFound reason "iteration limit reached; ..." → warning.
const LARGE_SI_SOURCE: &str = r#"
structure LargeSiObjective {
    param x: Length = auto
    constraint x > 8m
    minimize x
}
"#;

/// B6 source: small mm-scale param — converges, no warning expected.
///
/// `y` is a `Length = auto` param minimized within a tight mm-scale box.
/// SD floor at mm scale is far below NM_SD_TOLERANCE → early convergence
/// → iter_limited=false → no warning.
const SMALL_MM_SOURCE: &str = r#"
structure SmallMmObjective {
    param y: Length = auto
    constraint y > 1mm
    constraint y < 50mm
    minimize y
}
"#;

/// [B4] Eval of a LARGE-SI-magnitude objective emits
/// `DiagnosticCode::SolverOptimalityUnproven` as a `Severity::Warning`.
///
/// Also asserts the message contains "W_SOLVER_OPTIMALITY_UNPROVEN" (user-observable
/// signal) and that the resolved value respects the constraint (I1 guard:
/// linear objective → optimum at constraint boundary x ≥ 8 m = 8.0 SI).
#[test]
fn large_si_objective_emits_solver_optimality_unproven_warning() {
    let compiled = compile_source_with_stdlib(LARGE_SI_SOURCE);

    // Fixture should compile without errors.
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "LargeSiObjective fixture should compile without errors: {:#?}",
        compile_errors
    );

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(DimensionalSolver));

    let result = engine.eval(&compiled);

    // B4: a W_SOLVER_OPTIMALITY_UNPROVEN warning must be present.
    let optimality_warnings: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::SolverOptimalityUnproven))
        .collect();
    assert!(
        !optimality_warnings.is_empty(),
        "expected a DiagnosticCode::SolverOptimalityUnproven warning for large-SI objective, \
         got diagnostics: {:#?}",
        result.diagnostics
    );
    let w = optimality_warnings[0];
    assert_eq!(
        w.severity,
        Severity::Warning,
        "SolverOptimalityUnproven must be Severity::Warning, got {:?}",
        w.severity
    );
    // Message must carry the user-observable mnemonic (B4 leaf signal).
    assert!(
        w.message.contains("W_SOLVER_OPTIMALITY_UNPROVEN"),
        "warning message must contain 'W_SOLVER_OPTIMALITY_UNPROVEN', got: {:?}",
        w.message
    );

    // I1 guard: resolved value respects the constraint (x > 8 m = 8.0 m in SI).
    let x_id = ValueCellId::new("LargeSiObjective", "x");
    let x_si = match result.values.get(&x_id) {
        Some(Value::Scalar { si_value, .. }) => *si_value,
        other => panic!(
            "expected Scalar for LargeSiObjective.x, got {:?}",
            other
        ),
    };
    assert!(
        x_si >= 8.0,
        "x should respect constraint x > 8 m = 8.0 m, got {:.2} m",
        x_si
    );
}

/// [B6] Eval of a small mm-scale objective does NOT emit
/// `DiagnosticCode::SolverOptimalityUnproven` (no false-positive).
///
/// The solve converges early (SD floor at mm scale < NM_SD_TOLERANCE),
/// so iter_limited=false → BestFound reason = "converged within iteration budget"
/// → the "iteration limit" gate does NOT fire.
#[test]
fn small_mm_objective_does_not_emit_solver_optimality_unproven() {
    let compiled = compile_source_with_stdlib(SMALL_MM_SOURCE);

    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "SmallMmObjective fixture should compile without errors: {:#?}",
        compile_errors
    );

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(DimensionalSolver));

    let result = engine.eval(&compiled);

    // B6: no SolverOptimalityUnproven warning must appear.
    let optimality_warnings: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::SolverOptimalityUnproven))
        .collect();
    assert!(
        optimality_warnings.is_empty(),
        "unexpected SolverOptimalityUnproven warning for converged mm-scale objective: {:#?}",
        optimality_warnings
    );

    // Basic sanity: resolved value respects constraints (y > 1 mm).
    let y_id = ValueCellId::new("SmallMmObjective", "y");
    let y_si = match result.values.get(&y_id) {
        Some(Value::Scalar { si_value, .. }) => *si_value,
        other => panic!(
            "expected Scalar for SmallMmObjective.y, got {:?}",
            other
        ),
    };
    assert!(
        y_si > 0.001,
        "y should respect constraint y > 1 mm = 0.001 m, got {:.6} m",
        y_si
    );
}
