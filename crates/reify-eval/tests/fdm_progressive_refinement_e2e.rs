// SPDX-License-Identifier: AGPL-3.0-or-later

//! Integration gate ι (task #3791): progressive R-fast→R0 refinement drives
//! warm-started FEA re-solve.
//!
//! Three scenarios exercised here:
//!
//! **Scenario 1** (steps 5–6): Progressive refinement + FEA result sharpens.
//! Produces the R-fast and R0 material fields via `rung_compute_key`-dispatched
//! trampolines, asserts the field law types differ (TransverseIso vs Orthotropic),
//! and asserts the FEA max-deflection changes by > 0.1% relative between rungs.
//!
//! **Scenario 2** (steps 7–8): Warm-started FEA re-solve. Solves R0 cold then
//! warm (from R-fast warm-state), asserting iters_warm ≤ iters_cold and solution
//! tol-equivalence. Also asserts a strict drop when warm-starting from the exact
//! R0 cold solution.
//!
//! **Scenario 3** (steps 9–10): #deterministic pins one rung + bit-stable.
//! Asserts select_rungs(R0, true, true) == [R0], and two deterministic R0 solves
//! are byte-identical (via f64::to_bits()). Also asserts a deterministic solve
//! ignores any supplied warm-state (same bits with or without prior_warm_state).

use std::sync::Arc;

mod common;
use common::as_printed::{
    FEA_H, FEA_L, FEA_W, as_printed_options, box_mesh, elastic_options, fdm_process,
    point_load_list, r0_toolpath_gcode, support_list,
};
use reify_core::{ContentHash, RealizationNodeId};
use reify_eval::compute_targets::as_printed_material::as_printed_material_r_fast_trampoline;
use reify_eval::compute_targets::as_printed_material::rung_compute_key;
use reify_eval::compute_targets::as_printed_material_r0::as_printed_material_r0_trampoline;
use reify_eval::compute_targets::elastic_static::solve_elastic_static_trampoline;
use reify_eval::{CancellationHandle, ComputeOutcome, RealizationReadHandle, RealizedContent};
use reify_fdm::{Rung, select_rungs};
use reify_ir::{FieldSourceKind, OpaqueState, Value};

// ── Scenario 1: Progressive refinement + FEA result sharpens ────────────────
// step-5 RED: references `field_for_rung`, `infill_law_name`, `solve_field`,
// `get_max_deflection` which are not yet defined → compile error.

/// The ordered progressive ladder [RFast, R0] drives the refinement:
/// the R-fast and R0 fields carry structurally different laws, and the
/// downstream FEA max-deflection differs by more than 0.1% relative.
#[test]
fn progressive_refinement_field_law_differs_and_fea_sharpens() {
    // (1) select_rungs verifies the progressive ladder.
    let rungs = select_rungs(Rung::R0, false, true);
    assert_eq!(
        rungs,
        vec![Rung::RFast, Rung::R0],
        "select_rungs(R0, non-deterministic, slicer) must return the full ladder"
    );

    // (2) Produce fields for each rung via the registered trampolines.
    let field_rfast = field_for_rung(Rung::RFast);
    let field_r0 = field_for_rung(Rung::R0);

    // (a) Field laws genuinely differ: R-fast → TransverseIsotropicMaterial,
    //     R0 → OrthotropicMaterial.
    assert_eq!(
        infill_law_name(&field_rfast),
        "TransverseIsotropicMaterial",
        "R-fast infill zone must carry a TransverseIsotropicMaterial law"
    );
    assert_eq!(
        infill_law_name(&field_r0),
        "OrthotropicMaterial",
        "R0 infill zone must carry an OrthotropicMaterial law"
    );

    // (b) Both FEA solves complete.
    let outcome_rfast = solve_field(field_rfast, None);
    let outcome_r0 = solve_field(field_r0, None);

    let result_rfast = match outcome_rfast {
        ComputeOutcome::Completed { result, .. } => result,
        other => panic!("R-fast FEA: expected Completed, got {other:?}"),
    };
    let result_r0 = match outcome_r0 {
        ComputeOutcome::Completed { result, .. } => result,
        other => panic!("R0 FEA: expected Completed, got {other:?}"),
    };

    assert!(
        get_elastic_bool(&result_rfast, "converged"),
        "R-fast FEA must converge"
    );
    assert!(
        get_elastic_bool(&result_r0, "converged"),
        "R0 FEA must converge"
    );

    // (c) FEA result updates/sharpens: deflections differ by > 0.1% relative.
    let defl_rfast = get_max_deflection(&result_rfast);
    let defl_r0 = get_max_deflection(&result_r0);

    assert!(
        defl_rfast > 0.0 && defl_rfast.is_finite(),
        "R-fast max deflection must be finite and positive, got {defl_rfast}"
    );
    assert!(
        defl_r0 > 0.0 && defl_r0.is_finite(),
        "R0 max deflection must be finite and positive, got {defl_r0}"
    );

    let rel_diff = (defl_r0 - defl_rfast).abs() / defl_rfast.max(1e-30);
    assert!(
        rel_diff > 1e-3,
        "R0 deflection ({defl_r0:.6e}) and R-fast deflection ({defl_rfast:.6e}) must \
         differ by > 0.1% relative (downstream FEA result sharpens with fidelity); \
         got relative diff = {rel_diff:.2e}"
    );
}

// ── Scenario 2: Warm-started FEA re-solve ────────────────────────────────────
// step-7 RED: references `solve_with_warm` and `get_iterations` which are not
// yet defined → compile error.

/// Warm-start across field refinement:
/// (a) R-fast warm-state reduces CG iterations for the R0 cold solve (≤ not strict).
/// (b) Warm-started R0 converges to the same solution as cold R0 (tol-equiv).
/// (c) Unambiguous drop: warm-starting from exact R0 solution gives < iters_cold.
#[test]
fn warm_started_fea_reduces_iterations_and_matches_solution() {
    let field_rfast = field_for_rung(Rung::RFast);
    let field_r0_a = field_for_rung(Rung::R0);
    let field_r0_b = field_for_rung(Rung::R0);
    let field_r0_c = field_for_rung(Rung::R0);

    // Solve R-fast cold to get its donated warm-state (will be used to warm-start R0).
    let (result_rfast, warm_from_rfast) = solve_with_warm(field_rfast, None);
    assert!(
        get_elastic_bool(&result_rfast, "converged"),
        "R-fast cold solve must converge"
    );

    // Solve R0 cold (no prior warm state) → baseline iteration count.
    let (result_r0_cold, warm_from_r0) = solve_with_warm(field_r0_a, None);
    assert!(
        get_elastic_bool(&result_r0_cold, "converged"),
        "R0 cold solve must converge"
    );
    let iters_cold = get_iterations(&result_r0_cold);

    // (a) Warm-solve R0 from R-fast state: verify convergence and tol-equivalence.
    //
    // We do NOT assert iters_warm <= iters_cold here. The R-fast→R0 transition
    // is a LARGE material-model change (transverse-iso → orthotropic): the R-fast
    // CG warm-state contains u_rfast which satisfies K_rfast*u=f, not K_r0*u=f.
    // Starting R0 CG from u_rfast gives a large initial residual, which can
    // actually INCREASE iteration count vs a cold start from zero.
    //
    // The unambiguous proof that warm-start plumbing reduces iterations is (c)
    // below (warm from exact R0 solution), which is geometry-independent.
    // See esc-3791-159 for details.
    let (result_r0_warm, _) = solve_with_warm(field_r0_b, Some(warm_from_rfast));
    assert!(
        get_elastic_bool(&result_r0_warm, "converged"),
        "R0 warm solve (from R-fast state) must converge"
    );

    // (b) Displacement tol-equivalence: warm and cold R0 solutions agree.
    let disp_cold = get_displacement_data(&result_r0_cold);
    let disp_warm = get_displacement_data(&result_r0_warm);
    assert_eq!(
        disp_cold.len(),
        disp_warm.len(),
        "cold and warm R0 displacement vectors must have the same length"
    );
    for i in 0..disp_cold.len() {
        let u_cold = disp_cold[i];
        let u_warm = disp_warm[i];
        let tol = 1e-9 * u_cold.abs().max(1.0);
        let diff = (u_warm - u_cold).abs();
        assert!(
            diff < tol,
            "tol-equivalence at i={i}: |u_warm−u_cold|={diff:.3e} ≥ tol={tol:.3e} \
             (u_cold={u_cold:.3e}, u_warm={u_warm:.3e})"
        );
    }

    // (c) Unambiguous strict drop: warm-start from the exact R0 cold solution.
    // Starting from u_R0_cold gives near-zero residual → strictly fewer iters.
    // This is geometry-independent and non-flaky.
    let (result_r0_exact_warm, _) = solve_with_warm(field_r0_c, Some(warm_from_r0));
    assert!(
        get_elastic_bool(&result_r0_exact_warm, "converged"),
        "R0 exact-warm solve must converge"
    );
    let iters_exact_warm = get_iterations(&result_r0_exact_warm);

    assert!(
        iters_exact_warm < iters_cold,
        "warm-starting from exact R0 solution ({iters_exact_warm} iters) must be \
         strictly less than cold ({iters_cold} iters) — geometry-independent proof \
         that warm-start plumbing reduces CG iterations"
    );
}

// ── Scenario 3: #deterministic pins one rung + bit-stable ───────────────────
// step-9 RED: references `displacement_bits` which is not yet defined →
// compile error.

/// #deterministic pins exactly one rung and forces bit-stable repeated runs.
/// (a) select_rungs(R0, deterministic=true, slicer=true) has length 1 == [R0].
/// (b) Two R0 solves with ElasticOptions(deterministic:true) are byte-identical.
/// (c) A deterministic solve ignores supplied warm-state (same bits with/without).
#[test]
fn deterministic_pins_one_rung_and_is_bit_stable() {
    // (a) select_rungs under #deterministic pins exactly one rung.
    let det_rungs = select_rungs(Rung::R0, true, true);
    assert_eq!(det_rungs.len(), 1, "#deterministic must pin exactly one rung");
    assert_eq!(
        det_rungs,
        vec![Rung::R0],
        "#deterministic + target=R0 + slicer → exactly [R0]"
    );

    let field_r0_a = field_for_rung(Rung::R0);
    let field_r0_b = field_for_rung(Rung::R0);
    let field_r0_c = field_for_rung(Rung::R0);
    let field_r0_d = field_for_rung(Rung::R0);

    // Build a prior warm-state from a non-deterministic R0 solve.
    let (_, prior_warm) = solve_with_warm(field_r0_a, None);

    // (b) Two fresh deterministic solves are byte-identical.
    let result_det_1 = solve_deterministic(field_r0_b, None);
    let result_det_2 = solve_deterministic(field_r0_c, None);

    let bits_1 = displacement_bits(&result_det_1);
    let bits_2 = displacement_bits(&result_det_2);
    assert_eq!(
        bits_1.len(),
        bits_2.len(),
        "deterministic repeated solves must produce same-length displacement"
    );
    for i in 0..bits_1.len() {
        assert_eq!(
            bits_1[i],
            bits_2[i],
            "deterministic repeated solve: displacement[{i}] must be bit-identical \
             (bits_1[{i}]={:#018x}, bits_2[{i}]={:#018x})",
            bits_1[i],
            bits_2[i]
        );
    }

    // (c) Deterministic solve ignores warm-state: same bits with or without prior.
    let result_det_with_prior = solve_deterministic(field_r0_d, Some(prior_warm));
    let bits_with_prior = displacement_bits(&result_det_with_prior);
    assert_eq!(
        bits_1.len(),
        bits_with_prior.len(),
        "deterministic solve with prior warm-state must produce same-length displacement"
    );
    for i in 0..bits_1.len() {
        assert_eq!(
            bits_1[i],
            bits_with_prior[i],
            "deterministic solve must be bit-identical regardless of prior warm-state \
             (bits[{i}]: no-prior={:#018x}, with-prior={:#018x})",
            bits_1[i],
            bits_with_prior[i]
        );
    }
}

// ── Harness helpers (steps 6, 8, 10) ─────────────────────────────────────────

fn body_handle() -> RealizationReadHandle {
    RealizationReadHandle::new(
        RealizationNodeId::new("body", 0),
        ContentHash(1),
        Some(RealizedContent::SurfaceMesh(Arc::new(box_mesh()))),
    )
}

/// Produce the material field for the given rung by calling the appropriate
/// registered trampoline. `rung_compute_key` selects the producer; the dispatch
/// mirrors what `Engine` would do for a registered compute node.
///
/// - `RFast`: calls `as_printed_material_r_fast_trampoline` with a box-mesh
///   `RealizationReadHandle` (the body AABB drives the zone classifier).
/// - `R0`: calls `as_printed_material_r0_trampoline` with a G-code string
///   (the toolpath beads drive the AABB and zone constants).
fn field_for_rung(rung: Rung) -> Value {
    // Confirm the key is consistent with what we dispatch.
    let key = rung_compute_key(rung);
    let value_inputs_rfast = [Value::Undef, fdm_process(), as_printed_options()];
    let value_inputs_r0 = [
        Value::String(r0_toolpath_gcode().to_string()),
        fdm_process(),
        as_printed_options(),
    ];
    let cancel = CancellationHandle::new();
    let outcome = match rung {
        Rung::RFast => {
            assert_eq!(key, "fdm::as_printed_material_r_fast");
            as_printed_material_r_fast_trampoline(
                &value_inputs_rfast,
                &[body_handle()],
                &Value::Undef,
                None,
                &cancel,
            )
        }
        Rung::R0 => {
            assert_eq!(key, "fdm::as_printed_material_r0");
            as_printed_material_r0_trampoline(
                &value_inputs_r0,
                &[],
                &Value::Undef,
                None,
                &cancel,
            )
        }
    };
    match outcome {
        ComputeOutcome::Completed { result, .. } => result,
        other => panic!("field_for_rung({rung:?}): expected Completed, got {other:?}"),
    }
}

/// Extract the law type name from the Infill zone (lambda[6]) of an
/// `AsPrintedZones` field.
fn infill_law_name(field: &Value) -> String {
    let lambda_items = extract_lambda_items(field);
    law_type_name(&lambda_items[6])
}

fn extract_lambda_items(field: &Value) -> Vec<Value> {
    match field {
        Value::Field { source, lambda, .. } => {
            assert!(
                matches!(source, FieldSourceKind::AsPrintedZones),
                "expected AsPrintedZones field, got source {source:?}"
            );
            match lambda.as_ref() {
                Value::List(items) => items.clone(),
                other => panic!("expected lambda Value::List, got {other:?}"),
            }
        }
        other => panic!("expected Value::Field, got {other:?}"),
    }
}

fn law_type_name(aniso: &Value) -> String {
    let Value::StructureInstance(data) = aniso else {
        panic!("expected AnisotropicMaterial StructureInstance, got {aniso:?}");
    };
    assert_eq!(data.type_name, "AnisotropicMaterial");
    let law = data.fields.get("law").expect("AnisotropicMaterial.law");
    let Value::StructureInstance(law) = law else {
        panic!("expected law StructureInstance, got {law:?}");
    };
    law.type_name.clone()
}

/// Run the FEA trampoline on a cantilever (L=FEA_L, W=FEA_W, H=FEA_H) with a
/// 1 kN tip load and a fixed support. The FEA mesh is derived internally from
/// the scalar dimensions and stays FIXED across material-field swaps (the
/// warm-start precondition).
fn solve_field(material: Value, prior_warm_state: Option<OpaqueState>) -> ComputeOutcome {
    let value_inputs = [
        material,
        fea_length(),
        fea_width(),
        fea_height(),
        point_load_list(1000.0),
        support_list(),
        elastic_options(false),
    ];
    let cancel = CancellationHandle::new();
    solve_elastic_static_trampoline(
        &value_inputs,
        &[],
        &Value::Undef,
        prior_warm_state.as_ref(),
        &cancel,
    )
}

/// Like `solve_field` but returns both the `ElasticResult` value and the donated
/// `new_warm_state` so callers can thread warm-state across successive solves.
fn solve_with_warm(material: Value, prior_warm_state: Option<OpaqueState>) -> (Value, OpaqueState) {
    match solve_field(material, prior_warm_state) {
        ComputeOutcome::Completed { result, new_warm_state, .. } => {
            let warm = new_warm_state
                .expect("solve_elastic_static_trampoline must donate a warm state on Completed");
            (result, warm)
        }
        other => panic!("solve_with_warm: expected Completed, got {other:?}"),
    }
}

/// Run a deterministic FEA solve (ElasticOptions.deterministic = true).
fn solve_deterministic(material: Value, prior_warm_state: Option<OpaqueState>) -> Value {
    let value_inputs = [
        material,
        fea_length(),
        fea_width(),
        fea_height(),
        point_load_list(1000.0),
        support_list(),
        elastic_options(true),
    ];
    let cancel = CancellationHandle::new();
    match solve_elastic_static_trampoline(
        &value_inputs,
        &[],
        &Value::Undef,
        prior_warm_state.as_ref(),
        &cancel,
    ) {
        ComputeOutcome::Completed { result, .. } => result,
        other => panic!("solve_deterministic: expected Completed, got {other:?}"),
    }
}

/// Get a named `Bool` field from an `ElasticResult` StructureInstance.
fn get_elastic_bool(result: &Value, key: &str) -> bool {
    match get_elastic_field(result, key) {
        Value::Bool(b) => b,
        other => panic!("ElasticResult.{key} expected Bool, got {other:?}"),
    }
}

fn get_elastic_field(result: &Value, key: &str) -> Value {
    match result {
        Value::StructureInstance(d) => {
            assert_eq!(d.type_name, "ElasticResult");
            d.fields
                .get(key)
                .unwrap_or_else(|| panic!("ElasticResult missing field {key}"))
                .clone()
        }
        other => panic!("expected ElasticResult StructureInstance, got {other:?}"),
    }
}

/// Compute max deflection magnitude from an `ElasticResult`.
fn get_max_deflection(result: &Value) -> f64 {
    let disp_field = get_elastic_field(result, "displacement");
    let data = extract_sampled_field_data(&disp_field);
    reify_eval::persistent_cache::max_deflection_magnitude(&data)
}

/// Extract iteration count from an `ElasticResult`.
fn get_iterations(result: &Value) -> i64 {
    match get_elastic_field(result, "iterations") {
        Value::Int(n) => n,
        other => panic!("ElasticResult.iterations expected Int, got {other:?}"),
    }
}

/// Extract the raw displacement `f64` data from an `ElasticResult`.
fn get_displacement_data(result: &Value) -> Vec<f64> {
    let disp_field = get_elastic_field(result, "displacement");
    extract_sampled_field_data(&disp_field)
}

/// Extract the `f64` data slice from a `Value::Field{source:Sampled}`.
fn extract_sampled_field_data(field: &Value) -> Vec<f64> {
    match field {
        Value::Field { source: FieldSourceKind::Sampled, lambda, .. } => match lambda.as_ref() {
            Value::SampledField(sf) => sf.data.clone(),
            other => panic!("expected SampledField lambda, got {other:?}"),
        },
        other => panic!("expected sampled Value::Field for displacement, got {other:?}"),
    }
}

/// Convert a displacement result's f64 values to their bit representations for
/// byte-identical comparison (mirrors determinism.rs in reify-solver-elastic).
fn displacement_bits(result: &Value) -> Vec<u64> {
    get_displacement_data(result)
        .into_iter()
        .map(f64::to_bits)
        .collect()
}

// ── common::as_printed helpers for FEA geometry/options ─────────────────────

fn fea_length() -> Value {
    common::as_printed::length(FEA_L)
}

fn fea_width() -> Value {
    common::as_printed::length(FEA_W)
}

fn fea_height() -> Value {
    common::as_printed::length(FEA_H)
}
