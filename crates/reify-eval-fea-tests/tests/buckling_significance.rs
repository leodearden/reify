//! Integration tests for significance-filter BucklingResult support.
//!
//! PRD reference: `docs/prds/v0_5/buckling-eigensolver.md §13`
//!
//! These tests exercise the public
//! `reify_eval::significance_filter::{significance_filter, is_opted_in, FilterOutcome}`
//! with hand-built BucklingResult StructureInstance fixtures that mirror the
//! trampoline's exact field layout.
//!
//! TDD plan: steps 1–8 of task θ (#3457).
//! Steps come in RED/GREEN pairs: each test step deliberately fails (RED) until
//! its paired impl step makes it pass (GREEN).

// Value has interior mutability (SampledField → AtomicBool) so BTreeMap<Value, Value>
// triggers the mutable_key_type lint.  These tests build Value::Map fixtures which
// inherently require BTreeMap<Value, Value> — suppress the lint for the whole file.
#![allow(clippy::mutable_key_type)]

use reify_core::Type;
use reify_eval::significance_filter::{FilterOutcome, is_opted_in, significance_filter};
use reify_ir::{
    FieldSourceKind, InterpolationKind, PersistentMap, SampledField, SampledGridKind,
    StructureInstanceData, StructureTypeId, Value,
};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

// ── Fixture helpers ───────────────────────────────────────────────────────────

/// Build a minimal `Value::Field { source: Sampled }` wrapping the given data.
/// Matches the `make_sampled_field` shape used in significance_filter unit tests.
fn make_sampled_field(name: &str, data: &[f64]) -> Value {
    Value::Field {
        domain_type: Type::dimensionless_scalar(),
        codomain_type: Type::dimensionless_scalar(),
        source: FieldSourceKind::Sampled,
        lambda: Arc::new(Value::SampledField(SampledField {
            name: name.to_string(),
            kind: SampledGridKind::Regular1D,
            bounds_min: vec![0.0],
            bounds_max: vec![1.0],
            spacing: vec![0.5],
            axis_grids: vec![(0..data.len()).map(|i| i as f64).collect()],
            interpolation: InterpolationKind::Linear,
            data: data.to_vec(),
            oob_emitted: AtomicBool::new(false),
        })),
    }
}

/// Build a minimal pre_stress `ElasticResult` StructureInstance.
///
/// Carries a Sampled displacement field (using `pre_stress_disp` data) plus
/// `converged: true` and `iterations: Int(0)`. This is the minimal
/// "structural-presence" shape the significance filter inspects.
fn make_pre_stress(pre_stress_disp: &[f64]) -> Value {
    let fields: PersistentMap<String, Value> = [
        (
            "displacement".to_string(),
            make_sampled_field("displacement", pre_stress_disp),
        ),
        ("converged".to_string(), Value::Bool(true)),
        ("iterations".to_string(), Value::Int(0)),
    ]
    .into_iter()
    .collect();

    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "ElasticResult".to_string(),
        version: 1,
        fields,
    }))
}

/// Build a Mode StructureInstance with the given eigenvalue and displaced
/// positions list.
///
/// Exact field layout:
///   `eigenvalue: Value::Real(λ)`
///   `mode_shape: Value::Map { "displaced_positions": Value::List<Value::Real> }`
fn make_mode(eigenvalue: f64, displaced_positions: &[f64]) -> Value {
    let mode_shape_map: BTreeMap<Value, Value> = [(
        Value::String("displaced_positions".to_string()),
        Value::List(
            displaced_positions
                .iter()
                .map(|&x| Value::Real(x))
                .collect(),
        ),
    )]
    .into_iter()
    .collect();

    let fields: PersistentMap<String, Value> = [
        ("eigenvalue".to_string(), Value::Real(eigenvalue)),
        ("mode_shape".to_string(), Value::Map(mode_shape_map)),
    ]
    .into_iter()
    .collect();

    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "Mode".to_string(),
        version: 1,
        fields,
    }))
}

/// Build a BucklingResult StructureInstance exactly matching the trampoline
/// field layout (task ε, `compute_targets/buckling.rs`).
///
/// Field layout:
/// ```text
/// BucklingResult {
///   modes: List<Mode{eigenvalue:Real, mode_shape:Map{"displaced_positions":List<Real>}}>,
///   converged: Bool,
///   iterations: Int(0),
///   pre_stress: ElasticResult StructureInstance,
///   base_node_positions: List<Real>,
/// }
/// ```
///
/// # Arguments
/// - `eigenvalues`: one eigenvalue per mode (determines mode count)
/// - `converged`: BucklingResult.converged flag
/// - `displaced_positions`: one `&[f64]` slice of displaced positions per mode
/// - `pre_stress_disp`: displacement data for the pre_stress Sampled field
///
/// # Panics
/// Panics if `eigenvalues.len() != displaced_positions.len()`.
fn make_buckling_result(
    eigenvalues: &[f64],
    converged: bool,
    displaced_positions: &[&[f64]],
    pre_stress_disp: &[f64],
) -> Value {
    assert_eq!(
        eigenvalues.len(),
        displaced_positions.len(),
        "eigenvalues.len() must equal displaced_positions.len()"
    );

    let modes: Vec<Value> = eigenvalues
        .iter()
        .zip(displaced_positions.iter())
        .map(|(&ev, &dp)| make_mode(ev, dp))
        .collect();

    // Minimal base_node_positions (one node at origin).
    let base_node_positions: Vec<Value> =
        vec![Value::Real(0.0), Value::Real(0.0), Value::Real(0.0)];

    let fields: PersistentMap<String, Value> = [
        ("modes".to_string(), Value::List(modes)),
        ("converged".to_string(), Value::Bool(converged)),
        ("iterations".to_string(), Value::Int(0)),
        ("pre_stress".to_string(), make_pre_stress(pre_stress_disp)),
        (
            "base_node_positions".to_string(),
            Value::List(base_node_positions),
        ),
    ]
    .into_iter()
    .collect();

    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "BucklingResult".to_string(),
        version: 1,
        fields,
    }))
}

// ── Step-3: significance_filter eigenvalue tests ──────────────────────────────
//
// RED until step-4 adds the buckling branch.
//
// Before step-4, `significance_filter("solver::buckling", ...)` falls through
// to the elastic `Value::Map` path after the tolerance guard.  A
// `Value::StructureInstance` is not a `Value::Map`, so the Map-shape guard
// returns `Different` — even when eigenvalues are nearly identical.
// Every `Equivalent` assertion below is therefore RED until step-4 lands.

/// RED driver (a): eigenvalue within a tiny relative delta → Equivalent.
///
/// prev and new have eigenvalue 1000.0 vs 1000.0*(1+1e-9) — a relative delta
/// of 1e-9, far below any reasonable engineering threshold.  All other fields
/// are identical (mode_shape bit-equal, converged/iterations identical,
/// pre_stress identical).  The fixtures are explicitly non-bit-equal so the
/// bit-equality shortcut is NOT taken, and the eigenvalue comparison must fire.
#[test]
fn buckling_equivalent_for_tiny_eigenvalue_delta() {
    let disp: &[f64] = &[0.1, 0.2, 0.3, 0.4, 0.5, 0.6];
    let pre = &[0.0, 0.0, 0.0];
    let tol = 1e-3_f64;

    let ev_a = 1000.0_f64;
    let ev_b = 1000.0 * (1.0 + 1e-9); // relative delta = 1e-9

    let prev = make_buckling_result(&[ev_a], true, &[disp], pre);
    let new = make_buckling_result(&[ev_b], true, &[disp], pre);

    assert_ne!(
        prev, new,
        "fixture: prev and new must be distinct (not bit-equal)"
    );
    assert_eq!(
        significance_filter("solver::buckling", &prev, &new, Some(tol)),
        FilterOutcome::Equivalent,
        "eigenvalue relative delta 1e-9 << tolerance must yield Equivalent (RED until step-4)"
    );
}

/// Guard (b): bit-identical fixtures with Some(tol) → Equivalent (bit-equality shortcut).
/// Already GREEN after step-2 (opted-in + bit-equal shortcut fires).
#[test]
fn buckling_bit_identical_is_equivalent() {
    let disp: &[f64] = &[0.1, 0.2, 0.3];
    let pre = &[0.0];
    let r = make_buckling_result(&[4000.0], true, &[disp], pre);
    assert_eq!(
        significance_filter("solver::buckling", &r, &r.clone(), Some(1e-3)),
        FilterOutcome::Equivalent,
        "bit-identical BucklingResult must yield Equivalent via shortcut"
    );
}

/// Guard (c): grossly different eigenvalue (relative ~1e-2) → Different.
/// GREEN after step-4: the eigenvalue comparison fires and detects the delta.
/// Also passes before step-4 via the Map-shape guard returning Different.
#[test]
fn buckling_different_for_large_eigenvalue_delta() {
    let disp: &[f64] = &[0.1, 0.2, 0.3];
    let pre = &[0.0];
    let tol = 1e-3_f64;

    let prev = make_buckling_result(&[1000.0], true, &[disp], pre);
    let new = make_buckling_result(&[1010.0], true, &[disp], pre); // 1% difference

    assert_eq!(
        significance_filter("solver::buckling", &prev, &new, Some(tol)),
        FilterOutcome::Different,
        "eigenvalue relative delta ~1e-2 must yield Different"
    );
}

/// Guard (d): None tolerance with a non-bit-equal fixture → Different
/// (conservative fallback — missing tolerance gate).
#[test]
fn buckling_different_for_none_tolerance() {
    let disp: &[f64] = &[0.1, 0.2, 0.3];
    let pre = &[0.0];

    let prev = make_buckling_result(&[1000.0], true, &[disp], pre);
    let new = make_buckling_result(&[1000.001], true, &[disp], pre);

    assert_ne!(prev, new, "fixture: must be non-bit-equal");
    assert_eq!(
        significance_filter("solver::buckling", &prev, &new, None),
        FilterOutcome::Different,
        "None tolerance must produce Different (conservative fallback)"
    );
}

/// Guard (e): prev is not a StructureInstance → Different (malformed shape).
#[test]
fn buckling_different_for_non_structure_instance_prev() {
    let disp: &[f64] = &[0.1, 0.2, 0.3];
    let pre = &[0.0];
    let new = make_buckling_result(&[1000.0], true, &[disp], pre);
    let not_si = Value::Real(0.0);
    assert_eq!(
        significance_filter("solver::buckling", &not_si, &new, Some(1e-3)),
        FilterOutcome::Different,
        "non-StructureInstance prev must yield Different"
    );
}

/// Guard (f): wrong type_name ("NotBucklingResult") → Different.
#[test]
fn buckling_different_for_wrong_type_name() {
    let disp: &[f64] = &[0.1, 0.2, 0.3];
    let pre = &[0.0];
    let good = make_buckling_result(&[1000.0], true, &[disp], pre);

    // Build a StructureInstance with the wrong type_name.
    let fields: PersistentMap<String, Value> = [
        ("modes".to_string(), Value::List(vec![])),
        ("converged".to_string(), Value::Bool(true)),
        ("iterations".to_string(), Value::Int(0)),
        ("pre_stress".to_string(), make_pre_stress(pre)),
        ("base_node_positions".to_string(), Value::List(vec![])),
    ]
    .into_iter()
    .collect();
    let wrong_name = Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "NotBucklingResult".to_string(),
        version: 1,
        fields,
    }));

    assert_eq!(
        significance_filter("solver::buckling", &good, &wrong_name, Some(1e-3)),
        FilterOutcome::Different,
        "wrong type_name must yield Different"
    );
}

/// Guard (g): `modes` field missing from new → Different.
#[test]
fn buckling_different_for_missing_modes_field() {
    let disp: &[f64] = &[0.1, 0.2, 0.3];
    let pre = &[0.0];
    let good = make_buckling_result(&[1000.0], true, &[disp], pre);

    // Build a StructureInstance without the modes field.
    let fields: PersistentMap<String, Value> = [
        ("converged".to_string(), Value::Bool(true)),
        ("iterations".to_string(), Value::Int(0)),
        ("pre_stress".to_string(), make_pre_stress(pre)),
        ("base_node_positions".to_string(), Value::List(vec![])),
    ]
    .into_iter()
    .collect();
    let no_modes = Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "BucklingResult".to_string(),
        version: 1,
        fields,
    }));

    assert_eq!(
        significance_filter("solver::buckling", &good, &no_modes, Some(1e-3)),
        FilterOutcome::Different,
        "missing modes field must yield Different"
    );
}

/// Guard (h): `converged` field missing from new → Different.
#[test]
fn buckling_different_for_missing_converged_field() {
    let disp: &[f64] = &[0.1, 0.2, 0.3];
    let pre = &[0.0];
    let good = make_buckling_result(&[1000.0], true, &[disp], pre);

    // Build a StructureInstance without the converged field.
    let fields: PersistentMap<String, Value> = [
        ("modes".to_string(), Value::List(vec![])),
        ("iterations".to_string(), Value::Int(0)),
        ("pre_stress".to_string(), make_pre_stress(pre)),
        ("base_node_positions".to_string(), Value::List(vec![])),
    ]
    .into_iter()
    .collect();
    let no_converged = Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "BucklingResult".to_string(),
        version: 1,
        fields,
    }));

    assert_eq!(
        significance_filter("solver::buckling", &good, &no_converged, Some(1e-3)),
        FilterOutcome::Different,
        "missing converged field must yield Different"
    );
}

/// Guard (i): a mode entry that is not a StructureInstance → Different.
#[test]
fn buckling_different_for_non_structure_instance_mode_entry() {
    let pre = &[0.0];
    let tol = 1e-3_f64;

    let good = make_buckling_result(&[1000.0], true, &[&[0.1, 0.2, 0.3]], pre);

    // Build a modes list where one entry is Value::Real instead of a StructureInstance.
    let bad_mode = Value::Real(999.0);
    let fields: PersistentMap<String, Value> = [
        ("modes".to_string(), Value::List(vec![bad_mode])),
        ("converged".to_string(), Value::Bool(true)),
        ("iterations".to_string(), Value::Int(0)),
        ("pre_stress".to_string(), make_pre_stress(pre)),
        ("base_node_positions".to_string(), Value::List(vec![])),
    ]
    .into_iter()
    .collect();
    let bad_modes = Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "BucklingResult".to_string(),
        version: 1,
        fields,
    }));

    assert_eq!(
        significance_filter("solver::buckling", &good, &bad_modes, Some(tol)),
        FilterOutcome::Different,
        "non-StructureInstance mode entry must yield Different"
    );
}

/// Guard (j): a mode missing its `eigenvalue` field → Different.
#[test]
fn buckling_different_for_mode_missing_eigenvalue() {
    let pre = &[0.0];
    let tol = 1e-3_f64;

    let good = make_buckling_result(&[1000.0], true, &[&[0.1, 0.2, 0.3]], pre);

    // Build a Mode StructureInstance without the eigenvalue field.
    let mode_fields: PersistentMap<String, Value> =
        [("mode_shape".to_string(), Value::Map(BTreeMap::new()))]
            .into_iter()
            .collect();
    let mode_no_ev = Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "Mode".to_string(),
        version: 1,
        fields: mode_fields,
    }));
    let fields: PersistentMap<String, Value> = [
        ("modes".to_string(), Value::List(vec![mode_no_ev])),
        ("converged".to_string(), Value::Bool(true)),
        ("iterations".to_string(), Value::Int(0)),
        ("pre_stress".to_string(), make_pre_stress(pre)),
        ("base_node_positions".to_string(), Value::List(vec![])),
    ]
    .into_iter()
    .collect();
    let bad = Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "BucklingResult".to_string(),
        version: 1,
        fields,
    }));

    assert_eq!(
        significance_filter("solver::buckling", &good, &bad, Some(tol)),
        FilterOutcome::Different,
        "mode missing eigenvalue must yield Different"
    );
}

/// Guard (k): eigenvalue NaN → Different.
#[test]
fn buckling_different_for_nan_eigenvalue() {
    let disp: &[f64] = &[0.1, 0.2, 0.3];
    let pre = &[0.0];
    let tol = 1e-3_f64;

    let good = make_buckling_result(&[1000.0], true, &[disp], pre);
    let nan_ev = make_buckling_result(&[f64::NAN], true, &[disp], pre);

    assert_eq!(
        significance_filter("solver::buckling", &good, &nan_ev, Some(tol)),
        FilterOutcome::Different,
        "NaN eigenvalue must yield Different"
    );
}

/// Guard (l): eigenvalue ±Inf → Different.
#[test]
fn buckling_different_for_inf_eigenvalue() {
    let disp: &[f64] = &[0.1, 0.2, 0.3];
    let pre = &[0.0];
    let tol = 1e-3_f64;

    let good = make_buckling_result(&[1000.0], true, &[disp], pre);
    let inf_ev = make_buckling_result(&[f64::INFINITY], true, &[disp], pre);

    assert_eq!(
        significance_filter("solver::buckling", &good, &inf_ev, Some(tol)),
        FilterOutcome::Different,
        "+Inf eigenvalue must yield Different"
    );

    let neg_inf_ev = make_buckling_result(&[f64::NEG_INFINITY], true, &[disp], pre);
    assert_eq!(
        significance_filter("solver::buckling", &good, &neg_inf_ev, Some(tol)),
        FilterOutcome::Different,
        "-Inf eigenvalue must yield Different"
    );
}

/// Guard (m): modes length mismatch between prev and new → Different.
#[test]
fn buckling_different_for_modes_length_mismatch() {
    let disp: &[f64] = &[0.1, 0.2, 0.3];
    let pre = &[0.0];
    let tol = 1e-3_f64;

    let one_mode = make_buckling_result(&[1000.0], true, &[disp], pre);
    let two_modes = make_buckling_result(&[1000.0, 2000.0], true, &[disp, disp], pre);

    assert_eq!(
        significance_filter("solver::buckling", &one_mode, &two_modes, Some(tol)),
        FilterOutcome::Different,
        "modes length mismatch must yield Different"
    );
}

// ── Amendment: converged / iterations exact-equality and eigenvalue floor guards ─

/// Guard: `converged` true vs false → Different.
///
/// Exercises the exact Bool equality branch directly (matching `Some(Bool(p))` +
/// `Some(Bool(n))` with guard `p == n`).  A regression that matched
/// `Some(Bool(_))` instead of `p == n` would allow mismatched converged flags
/// through as Equivalent.
#[test]
fn buckling_different_for_converged_mismatch() {
    let disp: &[f64] = &[0.1, 0.2, 0.3];
    let pre = &[0.0];
    let tol = 1e-3_f64;

    let converged_true = make_buckling_result(&[1000.0], true, &[disp], pre);
    let converged_false = make_buckling_result(&[1000.0], false, &[disp], pre);

    assert_ne!(
        converged_true, converged_false,
        "fixture: must be non-bit-equal (converged differs)"
    );
    assert_eq!(
        significance_filter(
            "solver::buckling",
            &converged_true,
            &converged_false,
            Some(tol)
        ),
        FilterOutcome::Different,
        "converged true vs false must yield Different"
    );
}

/// Guard: `iterations` Int(0) vs Int(1) → Different.
///
/// Exercises the exact Int equality branch.  The trampoline currently always
/// emits Int(0); this guard ensures the equality check is not effectively dead
/// weight — a regression that matched `Some(Int(_))` instead of `p == n` would
/// silently allow any iteration count through as Equivalent.
#[test]
fn buckling_different_for_iterations_mismatch() {
    let ev = 1000.0_f64;
    let disp: &[f64] = &[0.1, 0.2, 0.3];
    let pre = &[0.0];
    let tol = 1e-3_f64;

    let prev = make_buckling_result(&[ev], true, &[disp], pre);

    // Hand-build a BucklingResult with iterations = Int(1).
    // make_buckling_result always sets iterations to Int(0), so we build manually.
    let fields: PersistentMap<String, Value> = [
        ("modes".to_string(), Value::List(vec![make_mode(ev, disp)])),
        ("converged".to_string(), Value::Bool(true)),
        ("iterations".to_string(), Value::Int(1)), // differs from prev's Int(0)
        ("pre_stress".to_string(), make_pre_stress(pre)),
        ("base_node_positions".to_string(), Value::List(vec![])),
    ]
    .into_iter()
    .collect();
    let new_iter_1 = Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "BucklingResult".to_string(),
        version: 1,
        fields,
    }));

    assert_ne!(
        prev, new_iter_1,
        "fixture: must be non-bit-equal (iterations differs)"
    );
    assert_eq!(
        significance_filter("solver::buckling", &prev, &new_iter_1, Some(tol)),
        FilterOutcome::Different,
        "iterations Int(0) vs Int(1) must yield Different"
    );
}

/// Guard (a): near-zero eigenvalue delta → Different.
///
/// Both eigenvalues are well below EIGENVALUE_MIN_DENOM (1e-12), but differ by
/// a factor of 2×.  The floor activates (denom = 1e-12); the absolute diff
/// (1e-13) is far above EIGENVALUE_REL_TOL * 1e-12 = 1e-18, so Different.
///
/// Verifies that near-zero eigenvalues do not receive a false Equivalent due to
/// the denominator floor compressing the effective threshold.
#[test]
fn buckling_different_for_near_zero_eigenvalue_delta() {
    let disp: &[f64] = &[0.1, 0.2, 0.3];
    let pre = &[0.0];
    let tol = 1e-3_f64;

    let prev = make_buckling_result(&[1e-13], true, &[disp], pre);
    let new = make_buckling_result(&[2e-13], true, &[disp], pre);

    assert_ne!(prev, new, "fixture: must be non-bit-equal");
    assert_eq!(
        significance_filter("solver::buckling", &prev, &new, Some(tol)),
        FilterOutcome::Different,
        "near-zero eigenvalue 1e-13 vs 2e-13 must yield Different"
    );
}

/// Guard (b): zero eigenvalues → Equivalent (EIGENVALUE_MIN_DENOM floor active).
///
/// With λ = 0.0 for both prev and new, max(|p|, |n|) = 0.0.  Without the
/// EIGENVALUE_MIN_DENOM floor the effective threshold would collapse to
/// EIGENVALUE_REL_TOL * 0.0 = 0.0, turning ANY non-zero delta into Different.
/// With the floor (denom = 1e-12) the threshold is 1e-18, and diff = 0.0 is
/// well within tolerance → Equivalent.
///
/// The displaced_positions differ by 0.5*tol to make fixtures non-bit-equal so
/// the bit-equality shortcut is not taken and the eigenvalue path is exercised.
#[test]
fn buckling_equivalent_for_zero_eigenvalues() {
    let tol = 1e-3_f64;
    let disp_prev: &[f64] = &[0.1, 0.2, 0.3];
    let disp_new: &[f64] = &[0.1 + 0.5 * tol, 0.2, 0.3]; // sub-tol mode_shape delta
    let pre = &[0.0];

    let prev = make_buckling_result(&[0.0], true, &[disp_prev], pre);
    let new = make_buckling_result(&[0.0], true, &[disp_new], pre);

    assert_ne!(prev, new, "fixture: must be non-bit-equal");
    assert_eq!(
        significance_filter("solver::buckling", &prev, &new, Some(tol)),
        FilterOutcome::Equivalent,
        "zero eigenvalues + sub-tol mode_shape delta must yield Equivalent (floor active)"
    );
}

// ── Step-5: mode_shape displaced_positions tests ──────────────────────────────
//
// RED until step-6 adds mode_shape comparison to `buckling_result_significance`.
//
// Before step-6, mode_shapes are ignored: two fixtures with identical eigenvalues
// but different displaced_positions are indistinguishable → Equivalent.
// The RED driver exploits this: a shape delta > tol should be Different but
// currently returns Equivalent.

/// RED driver (a): displaced-position delta over tol → Different.
///
/// Both modes have the same eigenvalue (bit-equal). Only mode_shape differs:
/// `x vs x + 2*tol` — strictly over-tolerance.  Before step-6 the mode_shape
/// is ignored and the function returns Equivalent (from eigenvalue equality).
#[test]
fn buckling_different_for_over_tol_displaced_position() {
    let tol = 1e-3_f64;
    let ev = 4000.0_f64;
    let pre = &[0.0];

    // Identical base positions; new has first coordinate shifted by 2*tol.
    let pos_prev: &[f64] = &[0.1, 0.2, 0.3];
    let pos_new: &[f64] = &[0.1 + 2.0 * tol, 0.2, 0.3]; // delta = 2*tol > tol

    let prev = make_buckling_result(&[ev], true, &[pos_prev], pre);
    let new = make_buckling_result(&[ev], true, &[pos_new], pre);

    assert_ne!(prev, new, "fixture: must be non-bit-equal");
    assert_eq!(
        significance_filter("solver::buckling", &prev, &new, Some(tol)),
        FilterOutcome::Different,
        "displaced_positions delta 2*tol > tol must yield Different (RED until step-6)"
    );
}

/// Guard (b): sub-tolerance displaced-position delta → Equivalent.
///
/// Delta = 0.5*tol < tol, eigenvalues equal → should be Equivalent.
/// Also GREEN before step-6 (eigenvalue equality already covers this case).
#[test]
fn buckling_equivalent_for_sub_tol_displaced_position() {
    let tol = 1e-3_f64;
    let ev = 4000.0_f64;
    let pre = &[0.0];

    let pos_prev: &[f64] = &[0.1, 0.2, 0.3];
    let pos_new: &[f64] = &[0.1 + 0.5 * tol, 0.2, 0.3]; // delta = 0.5*tol < tol

    let prev = make_buckling_result(&[ev], true, &[pos_prev], pre);
    let new = make_buckling_result(&[ev], true, &[pos_new], pre);

    assert_ne!(prev, new, "fixture: must be non-bit-equal");
    assert_eq!(
        significance_filter("solver::buckling", &prev, &new, Some(tol)),
        FilterOutcome::Equivalent,
        "displaced_positions delta 0.5*tol < tol must yield Equivalent"
    );
}

/// Guard (c): displaced_positions length mismatch between modes → Different.
#[test]
fn buckling_different_for_displaced_positions_length_mismatch() {
    let tol = 1e-3_f64;
    let ev = 4000.0_f64;
    let pre = &[0.0];

    // prev mode: 3 positions; new mode: 6 positions (different node count).
    let prev = make_buckling_result(&[ev], true, &[&[0.1, 0.2, 0.3]], pre);
    let new = make_buckling_result(&[ev], true, &[&[0.1, 0.2, 0.3, 0.4, 0.5, 0.6]], pre);

    assert_eq!(
        significance_filter("solver::buckling", &prev, &new, Some(tol)),
        FilterOutcome::Different,
        "displaced_positions length mismatch must yield Different"
    );
}

/// Guard (d): NaN in displaced_positions → Different.
#[test]
fn buckling_different_for_nan_in_displaced_position() {
    let tol = 1e-3_f64;
    let ev = 4000.0_f64;
    let pre = &[0.0];

    let prev = make_buckling_result(&[ev], true, &[&[0.1, 0.2, 0.3]], pre);
    let new_nan = make_buckling_result(&[ev], true, &[&[0.1, f64::NAN, 0.3]], pre);
    assert_eq!(
        significance_filter("solver::buckling", &prev, &new_nan, Some(tol)),
        FilterOutcome::Different,
        "NaN in displaced_positions must yield Different"
    );
}

/// Guard (e): mode_shape is not a Map → Different.
#[test]
fn buckling_different_for_mode_shape_not_a_map() {
    let tol = 1e-3_f64;
    let ev = 4000.0_f64;
    let pre = &[0.0];

    let good = make_buckling_result(&[ev], true, &[&[0.1, 0.2, 0.3]], pre);

    // Build a Mode with mode_shape = Value::Real instead of Map.
    let mode_fields: PersistentMap<String, Value> = [
        ("eigenvalue".to_string(), Value::Real(ev)),
        ("mode_shape".to_string(), Value::Real(99.0)), // wrong type
    ]
    .into_iter()
    .collect();
    let bad_mode = Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "Mode".to_string(),
        version: 1,
        fields: mode_fields,
    }));
    let fields: PersistentMap<String, Value> = [
        ("modes".to_string(), Value::List(vec![bad_mode])),
        ("converged".to_string(), Value::Bool(true)),
        ("iterations".to_string(), Value::Int(0)),
        ("pre_stress".to_string(), make_pre_stress(pre)),
        ("base_node_positions".to_string(), Value::List(vec![])),
    ]
    .into_iter()
    .collect();
    let bad = Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "BucklingResult".to_string(),
        version: 1,
        fields,
    }));

    assert_eq!(
        significance_filter("solver::buckling", &good, &bad, Some(tol)),
        FilterOutcome::Different,
        "mode_shape = Real (not Map) must yield Different"
    );
}

/// Guard (f): mode_shape Map missing displaced_positions key → Different.
#[test]
fn buckling_different_for_mode_shape_missing_displaced_positions() {
    let tol = 1e-3_f64;
    let ev = 4000.0_f64;
    let pre = &[0.0];

    let good = make_buckling_result(&[ev], true, &[&[0.1, 0.2, 0.3]], pre);

    // Build a Mode where mode_shape Map lacks "displaced_positions".
    let empty_map: BTreeMap<Value, Value> = BTreeMap::new();
    let mode_fields: PersistentMap<String, Value> = [
        ("eigenvalue".to_string(), Value::Real(ev)),
        ("mode_shape".to_string(), Value::Map(empty_map)),
    ]
    .into_iter()
    .collect();
    let bad_mode = Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "Mode".to_string(),
        version: 1,
        fields: mode_fields,
    }));
    let fields: PersistentMap<String, Value> = [
        ("modes".to_string(), Value::List(vec![bad_mode])),
        ("converged".to_string(), Value::Bool(true)),
        ("iterations".to_string(), Value::Int(0)),
        ("pre_stress".to_string(), make_pre_stress(pre)),
        ("base_node_positions".to_string(), Value::List(vec![])),
    ]
    .into_iter()
    .collect();
    let bad = Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "BucklingResult".to_string(),
        version: 1,
        fields,
    }));

    assert_eq!(
        significance_filter("solver::buckling", &good, &bad, Some(tol)),
        FilterOutcome::Different,
        "mode_shape Map missing displaced_positions key must yield Different"
    );
}

// ── Step-7: pre_stress structural-conservatism tests ─────────────────────────
//
// RED until step-8 adds the pre_stress structural check.
//
// Before step-8, `buckling_result_significance` does NOT inspect the
// `pre_stress` field.  Two fixtures that are otherwise Equivalent (matching
// eigenvalues + mode_shapes) but differ in `pre_stress` structural presence
// would return Equivalent instead of Different.

/// RED driver (a): new BucklingResult omits pre_stress field → Different.
///
/// Both fixtures have equal eigenvalues and equal mode_shapes.  Only `new`
/// omits the `pre_stress` field entirely.  Before step-8 the pre_stress field
/// is ignored and the function returns Equivalent.
#[test]
fn buckling_different_for_missing_pre_stress_field() {
    let tol = 1e-3_f64;
    let ev = 4000.0_f64;
    let disp: &[f64] = &[0.1, 0.2, 0.3];
    let pre = &[0.0_f64];

    let prev = make_buckling_result(&[ev], true, &[disp], pre);

    // Build a BucklingResult without the pre_stress field.
    let fields: PersistentMap<String, Value> = [
        ("modes".to_string(), Value::List(vec![make_mode(ev, disp)])),
        ("converged".to_string(), Value::Bool(true)),
        ("iterations".to_string(), Value::Int(0)),
        // pre_stress intentionally omitted
        ("base_node_positions".to_string(), Value::List(vec![])),
    ]
    .into_iter()
    .collect();
    let new_no_pre = Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "BucklingResult".to_string(),
        version: 1,
        fields,
    }));

    assert_ne!(prev, new_no_pre, "fixture: must be non-bit-equal");
    assert_eq!(
        significance_filter("solver::buckling", &prev, &new_no_pre, Some(tol)),
        FilterOutcome::Different,
        "missing pre_stress field must yield Different (RED until step-8)"
    );
}

/// RED driver (b): new BucklingResult has pre_stress = Value::Real → Different.
///
/// pre_stress is present but is a Value::Real instead of a StructureInstance.
/// Before step-8 this passes through as Equivalent.
#[test]
fn buckling_different_for_non_structure_instance_pre_stress() {
    let tol = 1e-3_f64;
    let ev = 4000.0_f64;
    let disp: &[f64] = &[0.1, 0.2, 0.3];
    let pre = &[0.0_f64];

    let prev = make_buckling_result(&[ev], true, &[disp], pre);

    // Build a BucklingResult where pre_stress = Value::Real (not SI).
    let fields: PersistentMap<String, Value> = [
        ("modes".to_string(), Value::List(vec![make_mode(ev, disp)])),
        ("converged".to_string(), Value::Bool(true)),
        ("iterations".to_string(), Value::Int(0)),
        ("pre_stress".to_string(), Value::Real(0.0)), // wrong type
        ("base_node_positions".to_string(), Value::List(vec![])),
    ]
    .into_iter()
    .collect();
    let new_wrong_pre = Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "BucklingResult".to_string(),
        version: 1,
        fields,
    }));

    assert_ne!(prev, new_wrong_pre, "fixture: must be non-bit-equal");
    assert_eq!(
        significance_filter("solver::buckling", &prev, &new_wrong_pre, Some(tol)),
        FilterOutcome::Different,
        "pre_stress = Real (not StructureInstance) must yield Different (RED until step-8)"
    );
}

/// Guard (c): both fixtures carry a well-formed pre_stress StructureInstance → Equivalent.
///
/// Uses a tiny eigenvalue delta (1e-9 relative) to make the fixtures non-bit-equal
/// while staying within EIGENVALUE_REL_TOL.  Mode_shapes are identical.
/// Both carry a valid pre_stress StructureInstance.
/// Already GREEN after step-6 (eigenvalue + mode_shape comparison passes).
#[test]
fn buckling_equivalent_with_well_formed_pre_stress() {
    let tol = 1e-3_f64;
    let ev_a = 4000.0_f64;
    let ev_b = 4000.0 * (1.0 + 1e-9); // relative delta 1e-9 << EIGENVALUE_REL_TOL
    let disp: &[f64] = &[0.1, 0.2, 0.3];
    let pre = &[0.0_f64];

    let prev = make_buckling_result(&[ev_a], true, &[disp], pre);
    let new = make_buckling_result(&[ev_b], true, &[disp], pre);

    assert_ne!(
        prev, new,
        "fixture: must be non-bit-equal (eigenvalues differ at 1e-9 relative)"
    );
    assert_eq!(
        significance_filter("solver::buckling", &prev, &new, Some(tol)),
        FilterOutcome::Equivalent,
        "within-tolerance eigenvalues + equal mode_shapes + valid pre_stress must yield Equivalent"
    );
}

// ── Step-1: is_opted_in allowlist tests ──────────────────────────────────────
//
// RED until step-2 adds "solver::buckling" to the is_opted_in match.

/// RED driver: `is_opted_in("solver::buckling")` must return true.
/// Before step-2 the allowlist contains only "solver::elastic_static", so this
/// fails with: assertion failed.
#[test]
fn is_opted_in_returns_true_for_buckling() {
    assert!(
        is_opted_in("solver::buckling"),
        "\"solver::buckling\" must be in the v1 opt-in allowlist"
    );
}

/// Regression guard: elastic_static must remain opted in after buckling is added.
#[test]
fn is_opted_in_elastic_static_stays_true() {
    assert!(
        is_opted_in("solver::elastic_static"),
        "\"solver::elastic_static\" must remain in the opt-in allowlist"
    );
}

/// Regression guards: modal and arbitrary strings must NOT be opted in.
#[test]
fn is_opted_in_returns_false_for_modal_and_arbitrary() {
    assert!(
        !is_opted_in("solver::modal"),
        "\"solver::modal\" must NOT be in the opt-in allowlist"
    );
    assert!(
        !is_opted_in("foo::bar"),
        "arbitrary strings must NOT be in the opt-in allowlist"
    );
}
