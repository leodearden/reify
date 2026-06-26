// SPDX-License-Identifier: AGPL-3.0-or-later

//! E2e regression suite for the CG warm-start heuristic (task #4869).
//!
//! The heuristic COLD-starts when ‖f − K·u_warm‖ ≥ ‖f‖ (warm guess no better
//! than zero) and WARM-starts otherwise. The closed-form isotropic stiffness-
//! scaling fixtures guarantee unambiguous regimes without relying on the
//! marginal real R-fast→R0 transition.

use reify_eval::compute_targets::elastic_static::solve_elastic_static_trampoline;
use reify_eval::{CancellationHandle, ComputeOutcome};
use reify_ir::{OpaqueState, Value};

mod common;
use common::as_printed::{
    FEA_H, FEA_L, FEA_W, elastic_options, isotropic_material, length, point_load_list, support_list,
};

/// Run a single FEA solve and return `(result_value, donated_warm_state)`.
fn solve(material: Value, prior: Option<OpaqueState>) -> (Value, OpaqueState) {
    let value_inputs = [
        material,
        length(FEA_L),
        length(FEA_W),
        length(FEA_H),
        point_load_list(1000.0),
        support_list(),
        elastic_options(false),
    ];
    let cancel = CancellationHandle::new();
    match solve_elastic_static_trampoline(
        &value_inputs,
        &[],
        &Value::Undef,
        prior.as_ref(),
        &cancel,
    ) {
        ComputeOutcome::Completed { result, new_warm_state, .. } => {
            let warm = new_warm_state.expect("solve_elastic_static_trampoline must donate warm state");
            (result, warm)
        }
        other => panic!("expected Completed, got {other:?}"),
    }
}

fn get_field(result: &Value, key: &str) -> Value {
    match result {
        Value::StructureInstance(d) => d
            .fields
            .get(key)
            .unwrap_or_else(|| panic!("ElasticResult missing field '{key}'"))
            .clone(),
        other => panic!("expected ElasticResult StructureInstance, got {other:?}"),
    }
}

fn warm_started(result: &Value) -> bool {
    match get_field(result, "warm_started") {
        Value::Bool(b) => b,
        other => panic!("warm_started expected Bool, got {other:?}"),
    }
}

fn converged(result: &Value) -> bool {
    match get_field(result, "converged") {
        Value::Bool(b) => b,
        other => panic!("converged expected Bool, got {other:?}"),
    }
}

fn iters(result: &Value) -> i64 {
    match get_field(result, "iterations") {
        Value::Int(n) => n,
        other => panic!("iterations expected Int, got {other:?}"),
    }
}

// step-3 RED: ElasticResult Value has no `warm_started` field yet →
// `fields.get("warm_started")` returns None → `get_field` panics.

/// Smoke-test that `warm_started` field is present and correctly reflects the
/// cold/warm distinction:
///   (a) cold solve (prior=None) → warm_started == false.
///   (b) re-solve SAME material with exact prior warm-state → warm_started == true.
#[test]
fn warm_started_field_present_and_correct() {
    let mat = isotropic_material(2.0e9);

    // (a) Cold solve — no prior warm state.
    let (result_cold, warm_state) = solve(mat.clone(), None);
    assert!(converged(&result_cold), "cold solve must converge");
    assert!(
        !warm_started(&result_cold),
        "cold solve (prior=None) must report warm_started=false"
    );

    // (b) Re-solve SAME operator from the exact prior solution — beneficial.
    // The prior u solves K(E)·u=f exactly (to CG rel-tol 1e-6), so
    // ‖K(E)·u − f‖ ≈ 1e-6·‖f‖ ≪ ‖f‖ → heuristic keeps warm.
    let (result_warm, _) = solve(mat, Some(warm_state));
    assert!(converged(&result_warm), "warm re-solve must converge");
    assert!(
        warm_started(&result_warm),
        "re-solve from exact prior must report warm_started=true (residual ≈ 0 ≪ ‖f‖)"
    );
}

// step-5 RED: without the heuristic filter (post step-4), `warm_start` is
// unconditionally `prior_cg.as_ref()`, so Regime A picks warm → warm_started==true
// but we assert warm_started==false → RED.

/// Verify the heuristic selects COLD for a large operator delta (α=100, warm
/// residual ≈ 99‖f‖ ≫ ‖f‖) and WARM for a small nudge (α=1.02, warm residual
/// ≈ 0.02‖f‖ ≪ ‖f‖).
///
/// Closed-form basis: for isotropic elasticity K(αE) = α·K(E) exactly.
/// If u_E solves K(E)·u = f, then ‖K(αE)·u_E − f‖ = |α−1|·‖f‖.
///
///   α=100  → 99‖f‖ ≥ ‖f‖  → heuristic picks COLD  → iters_chosen == iters_cold (identical)
///   α=1.02 → 0.02‖f‖ ≪ ‖f‖ → heuristic picks WARM → iters_chosen ≤ iters_cold
#[test]
fn heuristic_never_exceeds_cold_iters_across_both_regimes() {
    let base_e = 2.0e9_f64;

    // Solve the base operator K(E) twice — once per regime.
    // OpaqueState is not Clone (it wraps Box<dyn Any>), so we need two
    // independent warm states from two cold solves of the same operator.
    let (_, warm_base_a) = solve(isotropic_material(base_e), None);
    let (_, warm_base_b) = solve(isotropic_material(base_e), None);

    // ── REGIME A: large delta (α = 100) ──────────────────────────────────────
    let m_big = isotropic_material(base_e * 100.0);

    // Cold baseline for K(100E).
    let (result_cold_big, _) = solve(m_big.clone(), None);
    assert!(converged(&result_cold_big), "Regime A cold solve must converge");
    let iters_cold_big = iters(&result_cold_big);

    // Warm-probe K(100E) with u_E as prior.
    // ‖K(100E)·u_E − f‖ ≈ 99‖f‖ ≥ ‖f‖ → heuristic must reject → warm_started=false.
    let (result_warm_big, _) = solve(m_big, Some(warm_base_a));
    assert!(converged(&result_warm_big), "Regime A warm probe must converge");
    assert!(
        !warm_started(&result_warm_big),
        "Regime A: ‖K(100E)·u_E−f‖≈99‖f‖ ≥ ‖f‖ → heuristic must pick COLD (warm_started=false)"
    );
    assert_eq!(
        iters(&result_warm_big),
        iters_cold_big,
        "Regime A: heuristic-cold is the same code path as plain-cold → identical iterations"
    );

    // ── REGIME B: small nudge (α = 1.02) ─────────────────────────────────────
    let m_nudge = isotropic_material(base_e * 1.02);

    // Cold baseline for K(1.02E).
    let (result_cold_nudge, _) = solve(m_nudge.clone(), None);
    assert!(converged(&result_cold_nudge), "Regime B cold solve must converge");
    let iters_cold_nudge = iters(&result_cold_nudge);

    // Warm-probe K(1.02E) with u_E as prior.
    // ‖K(1.02E)·u_E − f‖ ≈ 0.02‖f‖ ≪ ‖f‖ → heuristic must accept → warm_started=true.
    let (result_warm_nudge, _) = solve(m_nudge, Some(warm_base_b));
    assert!(converged(&result_warm_nudge), "Regime B warm probe must converge");
    assert!(
        warm_started(&result_warm_nudge),
        "Regime B: ‖K(1.02E)·u_E−f‖≈0.02‖f‖ ≪ ‖f‖ → heuristic must pick WARM (warm_started=true)"
    );
    assert!(
        iters(&result_warm_nudge) <= iters_cold_nudge,
        "Regime B: warm start (50× smaller initial residual) must not exceed cold iters \
         (warm={}, cold={})",
        iters(&result_warm_nudge),
        iters_cold_nudge,
    );
}
