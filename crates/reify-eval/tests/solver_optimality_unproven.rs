//! Engine-level tests for W_SOLVER_OPTIMALITY_UNPROVEN (task #4804, γ):
//! verify that the eval engine surfaces `DiagnosticCode::SolverOptimalityUnproven`
//! when an objective solve hits the iteration limit (B4), and does NOT surface it
//! for a converged solve (B6 — no false-positive).
//!
//! Two cases via inline `.ri` sources (no geometry → stub-mode safe):
//!
//!   (B4) 12-param `minimize` over tight 2mm windows — MaxIters fired, warning expected.
//!        Mirrors `warm_start_budget_exhaustion_stays_feasible` in solver_integration.rs:
//!        each of 12 Length params is constrained to (9mm, 11mm) with initial = 10mm
//!        (feasible), and the objective `minimize a + b + ... + l` drives all params
//!        toward the 9mm lower boundary.
//!
//!        With 12 auto params, max_iters = min(500 × 13, 5000) = 5000.  Nelder-Mead in
//!        12D with a 2mm window cannot converge in 5000 iterations (verified by the
//!        solver_integration test).  The NM either:
//!          (a) hits MaxIters at a feasible point → Solved{values} + iter_limited=true, OR
//!          (b) hits MaxIters at an infeasible point → solver falls back to initial (10mm)
//!              via the initially-feasible fallback + iter_limited=true.
//!        Either way: BestFound{reason~"iteration limit reached; ..."} → warning fires.
//!
//!        I1 guard: all 12 resolved param values satisfy the feasibility window
//!        [9mm, 11mm] (whether via the NM's best-feasible point or the fallback initial).
//!
//!   (B6) Small 1-param `minimize` that converges — NO warning expected.
//!        1D NM with a linear objective converges at the infeasible optimum (y ≈ 1mm−500nm),
//!        triggers the feasibility fallback to initial y=10mm, but iter_limited=false
//!        → BestFound{reason~"converged within iteration budget"} → no warning.
//!
//! RED until step-4 reroutes the main eval objective path to `solve_ranked` and
//! pushes the warning.

use reify_constraints::DimensionalSolver;
use reify_core::{DiagnosticCode, Severity, ValueCellId};
use reify_eval::Engine;
use reify_ir::{
    ConstraintSolver, OptimalityStatus, RankedSolveResult, ResolutionProblem, SolveResult, Value,
};
use reify_test_support::{MockConstraintChecker, compile_source_with_stdlib};
use std::collections::HashMap;

// ── S3 mock: an I2-violating solver that returns an empty Ranked result ────────
//
// Used to drive the debug_assert guards at the engine seam (step-4/5) and
// registry seam (step-6/7).

/// Minimal solver that intentionally violates I2: `solve_ranked` returns
/// `Ranked { candidates: vec![], ... }`, which should trigger the debug_assert
/// guard before `candidates.swap_remove(0)` is called.
struct EmptyRankedSolver;

impl ConstraintSolver for EmptyRankedSolver {
    fn solve(&self, _problem: &ResolutionProblem) -> SolveResult {
        SolveResult::Solved {
            values: HashMap::new(),
            unique: false,
        }
    }

    fn solve_ranked(&self, _problem: &ResolutionProblem) -> RankedSolveResult {
        // Deliberately empty candidates — violates I2.
        RankedSolveResult::Ranked {
            candidates: vec![],
            optimality: OptimalityStatus::FeasibilityOnly,
        }
    }
}

/// B4 source: 12 auto Length params in a tight 2mm window (9mm–11mm), initial = 10mm.
///
/// Mirrors `warm_start_budget_exhaustion_stays_feasible` in solver_integration.rs:
/// 12D Nelder-Mead with a 2mm feasibility window cannot converge in
/// max_iters = min(500 × 13, 5000) = 5000 iterations → MaxItersReached →
/// iter_limited=true → BestFound{reason~"iteration limit reached"} → warning.
///
/// Whether the NM's best_param at MaxIters is feasible (returned as-is) or
/// infeasible (solver falls back to initial 10mm via the initially-feasible fallback),
/// the meta.iter_limited=true flag persists through the path.
const MULTI_PARAM_SOURCE: &str = r#"
structure ObjectiveMaxIters {
    param a: Length = auto
    param b: Length = auto
    param c: Length = auto
    param d: Length = auto
    param e: Length = auto
    param f: Length = auto
    param g: Length = auto
    param h: Length = auto
    param i: Length = auto
    param j: Length = auto
    param k: Length = auto
    param l: Length = auto

    constraint a > 9mm
    constraint a < 11mm
    constraint b > 9mm
    constraint b < 11mm
    constraint c > 9mm
    constraint c < 11mm
    constraint d > 9mm
    constraint d < 11mm
    constraint e > 9mm
    constraint e < 11mm
    constraint f > 9mm
    constraint f < 11mm
    constraint g > 9mm
    constraint g < 11mm
    constraint h > 9mm
    constraint h < 11mm
    constraint i > 9mm
    constraint i < 11mm
    constraint j > 9mm
    constraint j < 11mm
    constraint k > 9mm
    constraint k < 11mm
    constraint l > 9mm
    constraint l < 11mm

    minimize a + b + c + d + e + f + g + h + i + j + k + l
}
"#;

/// B6 source: small 1-param `minimize` — converges, NO warning expected.
///
/// 1D NM with a linear objective (`minimize y`) converges at the infeasible minimum
/// (y ≈ 1mm − 500nm), triggers the initially-feasible fallback to y=10mm,
/// but iter_limited=false → BestFound{reason~"converged within iteration budget"} → no warning.
const SMALL_MM_SOURCE: &str = r#"
structure SmallMmObjective {
    param y: Length = auto
    constraint y > 1mm
    constraint y < 50mm
    minimize y
}
"#;

/// [B4] Eval of a 12-param objective that hits MaxIters emits
/// `DiagnosticCode::SolverOptimalityUnproven` as a `Severity::Warning`.
///
/// Also asserts the message contains "W_SOLVER_OPTIMALITY_UNPROVEN" (user-observable
/// signal) and that resolved param `a` satisfies the feasibility window (I1 guard).
#[test]
fn multi_param_objective_emits_solver_optimality_unproven_warning() {
    let compiled = compile_source_with_stdlib(MULTI_PARAM_SOURCE);

    // Fixture should compile without errors.
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "ObjectiveMaxIters fixture should compile without errors: {:#?}",
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
        "expected a DiagnosticCode::SolverOptimalityUnproven warning for 12-param objective, \
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

    // I1 guard: param `a` must be within the feasibility window [9mm, 11mm] = [0.009, 0.011].
    // Whether via the NM's best-feasible point or the initially-feasible fallback (10mm = 0.01m),
    // the resolved value must satisfy all constraints.
    let a_id = ValueCellId::new("ObjectiveMaxIters", "a");
    let a_si = match result.values.get(&a_id) {
        Some(Value::Scalar { si_value, .. }) => *si_value,
        other => panic!(
            "expected Scalar for ObjectiveMaxIters.a, got {:?}",
            other
        ),
    };
    assert!(
        a_si >= 0.009 - 1e-9,
        "a must be >= 9mm = 0.009 m (got {:.8} m)",
        a_si
    );
    assert!(
        a_si <= 0.011 + 1e-9,
        "a must be <= 11mm = 0.011 m (got {:.8} m)",
        a_si
    );
}

/// [S1] Rot-guard: the actual `examples/solver_optimality_unproven.ri` file still
/// hits the iteration limit and emits `DiagnosticCode::SolverOptimalityUnproven`.
///
/// Embeds the file via `include_str!` (compile-time existence guarantee; canonical
/// pattern — see `buckling_smoke.rs:22`). The diagnostic CODE is mechanism-precise:
/// the engine_eval.rs iteration-limit gate is its sole emitter, so code-present ⟺
/// example still hits MaxIters. Guards both rot vectors: silent NM convergence AND
/// deletion/renaming of the example artifact.
#[test]
fn example_file_solver_optimality_unproven_emits_warning() {
    const EXAMPLE_SRC: &str =
        include_str!("../../../examples/solver_optimality_unproven.ri");

    let compiled = compile_source_with_stdlib(EXAMPLE_SRC);

    // Fixture should compile without errors.
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "solver_optimality_unproven.ri should compile without errors: {:#?}",
        compile_errors
    );

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(DimensionalSolver));

    let result = engine.eval(&compiled);

    // S1: at least one W_SOLVER_OPTIMALITY_UNPROVEN warning must be present.
    let optimality_warnings: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::SolverOptimalityUnproven))
        .collect();
    assert!(
        !optimality_warnings.is_empty(),
        "expected DiagnosticCode::SolverOptimalityUnproven warning from example file, \
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

    // I1 guard (optional): param `a` must be within [9mm, 11mm].
    let a_id = ValueCellId::new("ObjectiveMaxIters", "a");
    let a_si = match result.values.get(&a_id) {
        Some(Value::Scalar { si_value, .. }) => *si_value,
        other => panic!(
            "expected Scalar for ObjectiveMaxIters.a, got {:?}",
            other
        ),
    };
    assert!(
        a_si >= 0.009 - 1e-9,
        "a must be >= 9mm = 0.009 m (got {:.8} m)",
        a_si
    );
    assert!(
        a_si <= 0.011 + 1e-9,
        "a must be <= 11mm = 0.011 m (got {:.8} m)",
        a_si
    );
}

/// [B6] Eval of a small 1-param objective does NOT emit
/// `DiagnosticCode::SolverOptimalityUnproven` (no false-positive).
///
/// 1D NM converges at the infeasible optimum → feasibility fallback to initial y=10mm
/// → iter_limited=false → BestFound{reason~"converged within iteration budget"} → no warning.
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
        "unexpected SolverOptimalityUnproven warning for converged 1D objective: {:#?}",
        optimality_warnings
    );

    // Basic sanity: resolved value respects constraints (y must be within [1mm, 50mm]).
    let y_id = ValueCellId::new("SmallMmObjective", "y");
    let y_si = match result.values.get(&y_id) {
        Some(Value::Scalar { si_value, .. }) => *si_value,
        other => panic!(
            "expected Scalar for SmallMmObjective.y, got {:?}",
            other
        ),
    };
    assert!(
        y_si >= 0.001 - 1e-9,
        "y must respect constraint y > 1mm (got {:.8} m)",
        y_si
    );
    assert!(
        y_si <= 0.050 + 1e-9,
        "y must respect constraint y < 50mm (got {:.8} m)",
        y_si
    );
}

// ── S3 engine seam (task #4871) ────────────────────────────────────────────────

/// Tiny objective source for S3 tests: one auto Length param with a `minimize`
/// directive.  `objective.is_some()` → the engine calls `solve_ranked` → the
/// EmptyRankedSolver returns `Ranked { candidates: vec![], ... }` → `swap_remove(0)`
/// panics (before the guard) or `debug_assert!` fires (after the guard).
const S3_OBJECTIVE_SOURCE: &str = r#"
structure S {
    param x: Length = auto
    constraint x > 1mm
    constraint x < 50mm
    minimize x
}
"#;

/// [S3-engine] A solver that violates I2 (empty Ranked candidates) panics at the
/// engine seam with the I2 assert message, not the opaque vec index message.
///
/// Uses `assert!` (always-on, all build profiles) so the clear I2 diagnostic is
/// present in both debug and release builds. The seam-specific suffix "(engine seam)"
/// in the expected string uniquely pins this test to the engine_eval.rs guard and
/// cannot be satisfied by the registry guard (which emits "(registry seam)").
///
/// RED before step-5 adds the guard: `candidates.swap_remove(0)` panics with
/// "removal index (is 0) should be < len (is 0)", which does NOT contain the
/// expected substring → `should_panic` mismatch FAILS.
/// GREEN after step-5: assert fires first with the seam-specific I2 message.
#[test]
#[should_panic(expected = "RankedSolveResult::Ranked must carry >=1 candidate (I2) (engine seam)")]
fn empty_ranked_candidates_trips_i2_assert_engine_seam() {
    let compiled = compile_source_with_stdlib(S3_OBJECTIVE_SOURCE);
    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(EmptyRankedSolver));
    let _ = engine.eval(&compiled);
}

// ── S3 registry seam (task #4871) ─────────────────────────────────────────────

/// [S3-registry] A solver wrapped in SolverRegistry that violates I2 panics at the
/// REGISTRY seam (registry.rs) with the I2 assert message.
///
/// The registry routes the objective-bearing Dimensional component to
/// EmptyRankedSolver.solve_ranked → returns Ranked { candidates: vec![], ... } →
/// registry.rs panics internally BEFORE returning to the engine (distinct seam from
/// the engine guard, so this exercises registry.rs independently).
///
/// Uses `assert!` (always-on, all build profiles) so the clear I2 diagnostic is
/// present in both debug and release builds. The seam-specific suffix "(registry seam)"
/// in the expected string uniquely pins this test to the registry.rs guard and cannot
/// be satisfied by the engine guard (which emits "(engine seam)").
///
/// RED before step-7 adds the guard: registry.rs `candidates.swap_remove(0)` panics
/// with "removal index (is 0) should be < len (is 0)" → should_panic mismatch FAILS.
/// GREEN after step-7: assert fires first with the seam-specific I2 message.
#[test]
#[should_panic(expected = "RankedSolveResult::Ranked must carry >=1 candidate (I2) (registry seam)")]
fn empty_ranked_candidates_trips_i2_assert_registry_seam() {
    let compiled = compile_source_with_stdlib(S3_OBJECTIVE_SOURCE);
    // Wrap the I2-violating solver in a SolverRegistry so the registry's
    // solve_inner dispatches to EmptyRankedSolver and panics at registry.rs.
    let registry = reify_constraints::SolverRegistry::new(Box::new(EmptyRankedSolver));
    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(registry));
    let _ = engine.eval(&compiled);
}
