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
