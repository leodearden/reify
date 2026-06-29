//! Integration gate for task 4790 (cost-min β): Money-objective robustness floor
//! wired end-to-end, exercised via the SHIPPED `examples/continuous_cost_min.ri`.
//!
//! This test reads the actual shipped example from disk (not a test-fixture copy).
//! Compile-level regressions in the shipped file are caught first by the bulk gate
//! `crates/reify-compiler/tests/examples_smoke.rs` (auto-discovers every `examples/*.ri`);
//! this test's compile check is a fast-fail precondition for the eval path below, not the
//! primary regression detector for compile errors. The unique value of this test is the
//! eval-layer diagnostic/off-boundary assertions that `examples_smoke` cannot cover.
//!
//! # What is tested
//!
//! The `CostMinBracket` structure in `examples/continuous_cost_min.ri` has:
//! - `param thickness : Length = auto(free)` — the free param cost-min drives to bound
//! - `constraint thickness > 2mm` — stress/clearance lower bound
//! - `minimize unit_cost * (thickness / 1mm)` — Money objective → floor synthesised
//!
//! With α's robustness floor:
//! - `thickness` resolves **strictly above** 2mm (off boundary; seed fallback ~10mm
//!   satisfies `> 2mm` trivially — see α's eval test doc for why no upper-bound assert)
//! - eval emits exactly one `RobustnessFloorApplied` (Info) naming `cost_robustness_tradeoff`
//! - no `RobustnessFloorInfeasible` and no Error-severity diagnostics
//!
//! # Reuse
//!
//! Import set and compile→eval→assert skeleton mirror
//! `crates/reify-eval/tests/robustness_floor_signal.rs::money_floor_resolves_off_boundary_and_emits_info`
//! (α's eval-level signal test). The only differences: disk path via CARGO_MANIFEST_DIR
//! instead of `include_str!`, and the ValueCellId entity name (`CostMinBracket`).

use reify_constraints::DimensionalSolver;
use reify_core::{DiagnosticCode, Severity, ValueCellId};
use reify_eval::Engine;
use reify_ir::Value;
use reify_test_support::{MockConstraintChecker, collect_errors, compile_source_with_stdlib};

/// Path to the shipped example, resolved relative to this crate's manifest directory.
const EXAMPLE_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/continuous_cost_min.ri");

/// Integration gate (β §8.2 headline): `CostMinBracket` resolves off the 2mm boundary
/// and emits exactly one `RobustnessFloorApplied` Info naming `cost_robustness_tradeoff`.
///
/// **Why no upper-bound assertion on thickness**: with default Length bounds `[1µm, 10m]`
/// and seed ~10mm, Nelder-Mead drifts sub-boundary (penalty-free region below 2mm has lower
/// objective), falls back to the initially-feasible seed (~10mm), and returns that as the
/// solution. The value (~10mm) satisfies `> 2mm` but is NOT near 2mm+margin ≈ 2.04mm.
/// Upper-bound / precise-convergence assertions belong in the solver-level test
/// (`crates/reify-constraints/tests/robustness_floor.rs`) which injects explicit bounds
/// `[1mm,1.5mm]`. The eval-layer primary purpose here is diagnostic-emission verification.
#[test]
fn continuous_cost_min_example_resolves_off_boundary_and_emits_info() {
    let src = std::fs::read_to_string(EXAMPLE_PATH).unwrap_or_else(|e| {
        panic!(
            "Could not read {}: {} — run step-2 impl to create the example file",
            EXAMPLE_PATH, e
        )
    });

    // ── compile ────────────────────────────────────────────────────────────────
    let compiled = compile_source_with_stdlib(&src);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "examples/continuous_cost_min.ri should compile without errors: {:#?}",
        errors
    );

    // ── eval ───────────────────────────────────────────────────────────────────
    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None)
        .with_solver(Box::new(DimensionalSolver));
    let result = engine.eval(&compiled);

    // ── OFF-BOUNDARY (structural, NOT precise-convergence) ─────────────────────
    let thickness_id = ValueCellId::new("CostMinBracket", "thickness");
    let thickness_si = match result.values.get(&thickness_id) {
        Some(Value::Scalar { si_value, .. }) => *si_value,
        other => panic!(
            "expected Scalar for CostMinBracket.thickness, got {:?}",
            other
        ),
    };
    // Sanity guard: the resolved value must be strictly above the 2mm boundary (0.002 m).
    // NOTE: this assertion does NOT prove the floor moved the value — the seed fallback
    // (~10mm) already satisfies `> 0.002` regardless of the floor. The real floor signal
    // is the `RobustnessFloorApplied` diagnostic checked below. The precise-convergence
    // assertion (value ≈ 2.04mm) lives in the solver-level test with injected bounds
    // (`crates/reify-constraints/tests/robustness_floor.rs`), not here.
    // Pure cost-min without the floor would park at exactly 0.002 m.
    assert!(
        thickness_si > 0.002,
        "thickness should be strictly above the 2mm boundary (0.002 m); got {:.6} m",
        thickness_si
    );

    // ── EXACTLY ONE RobustnessFloorApplied Info ────────────────────────────────
    let floor_applied: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::RobustnessFloorApplied))
        .collect();
    assert_eq!(
        floor_applied.len(),
        1,
        "expected exactly one RobustnessFloorApplied diagnostic; got {}: {:#?}",
        floor_applied.len(),
        result.diagnostics,
    );

    // ── NAMES THE OVERRIDE ─────────────────────────────────────────────────────
    let info_msg = &floor_applied[0].message;
    assert!(
        info_msg.contains("cost_robustness_tradeoff"),
        "RobustnessFloorApplied message must contain 'cost_robustness_tradeoff'; got: {:?}",
        info_msg,
    );

    // ── NO RobustnessFloorInfeasible ───────────────────────────────────────────
    let floor_infeasible: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::RobustnessFloorInfeasible))
        .collect();
    assert!(
        floor_infeasible.is_empty(),
        "must emit no RobustnessFloorInfeasible; got: {:#?}",
        floor_infeasible,
    );

    // ── NO Error-severity eval diagnostics ────────────────────────────────────
    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        eval_errors.is_empty(),
        "must emit no Error-severity eval diagnostics; got: {:#?}",
        eval_errors,
    );
}
