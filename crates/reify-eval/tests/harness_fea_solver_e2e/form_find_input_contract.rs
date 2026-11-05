//! The force-density input contract (solver-crates-layering η₂, #9271) seen
//! through the `solver::form_find` and `solver::form_find_free` trampolines: an
//! input the kernel rejects surfaces as a located `E_FormFindInfeasible`
//! diagnostic, never a completed solve.
//!
//! Deliberately not named `tensegrity_*`, so the `-- tensegrity` filter keeps
//! selecting exactly the tensegrity modules.

use reify_eval::{CancellationHandle, ComputeOutcome, RealizationReadHandle};
use reify_ir::{OpaqueState, PersistentMap, StructureInstanceData, StructureTypeId, Value};
use reify_test_support::point3;

/// A line-only Tensegrity of `nodes` with no struts and `cables` as index pairs.
fn cable_net(nodes: Vec<Value>, cables: &[(i64, i64)]) -> Value {
    let cables = cables
        .iter()
        .map(|&(j, k)| Value::List(vec![Value::Int(j), Value::Int(k)]))
        .collect();
    let fields: PersistentMap<String, Value> = [
        ("nodes".to_string(), Value::List(nodes)),
        ("struts".to_string(), Value::List(vec![])),
        ("cables".to_string(), Value::List(cables)),
    ]
    .into_iter()
    .collect();
    Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(0),
        type_name: "Tensegrity".to_string(),
        version: 1,
        fields,
    }))
}

/// Invoke the anchored trampoline with no realization and no warm state.
fn call_form_find(value_inputs: &[Value]) -> ComputeOutcome {
    let no_realization: &[RealizationReadHandle] = &[];
    let no_warm_state: Option<&OpaqueState> = None;
    reify_eval::compute_targets::form_find::solve_form_find_trampoline(
        value_inputs,
        no_realization,
        &Value::Undef,
        no_warm_state,
        &CancellationHandle::new(),
    )
}

/// Invoke the free-standing trampoline with no realization and no warm state.
fn call_form_find_free(value_inputs: &[Value]) -> ComputeOutcome {
    let no_realization: &[RealizationReadHandle] = &[];
    let no_warm_state: Option<&OpaqueState> = None;
    reify_eval::compute_targets::form_find::solve_form_find_free_trampoline(
        value_inputs,
        no_realization,
        &Value::Undef,
        no_warm_state,
        &CancellationHandle::new(),
    )
}

/// The free trampoline's four inputs: `structure`, one group id per member,
/// then `seed_ratios` and `reference_group`.
fn free_inputs(
    structure: Value,
    group_ids: &[i64],
    seed_ratios: &[f64],
    reference_group: i64,
) -> Vec<Value> {
    vec![
        structure,
        Value::List(group_ids.iter().copied().map(Value::Int).collect()),
        Value::List(seed_ratios.iter().copied().map(Value::Real).collect()),
        Value::Int(reference_group),
    ]
}

/// The outcome is `Failed` with an `E_FormFindInfeasible` diagnostic naming
/// every one of `needles`.
fn assert_failed_infeasible(outcome: ComputeOutcome, needles: &[&str]) {
    let ComputeOutcome::Failed { diagnostics, .. } = outcome else {
        panic!("expected ComputeOutcome::Failed, got {outcome:?}");
    };
    let joined = diagnostics
        .iter()
        .map(|d| d.message.as_str())
        .collect::<Vec<_>>()
        .join(" | ");
    for needle in ["E_FormFindInfeasible"].iter().chain(needles) {
        assert!(
            joined.contains(needle),
            "expected the diagnostic to mention {needle:?}, got: {joined}"
        );
    }
}

/// The T1a cable net with its free node seeded at NaN: the line-only solve
/// never reads a free node's guess, so only the input contract can catch it.
#[test]
fn trampoline_non_finite_node_coordinate_is_failed_naming_the_node() {
    let nodes = vec![
        point3(f64::NAN, 0.2, 0.4),
        point3(1.0, 0.0, 0.0),
        point3(-1.0, 0.0, 0.0),
        point3(0.0, 1.0, 1.0),
        point3(0.0, -1.0, 1.0),
    ];
    let structure = cable_net(nodes, &[(0, 1), (0, 2), (0, 3), (0, 4)]);
    let value_inputs = vec![
        structure,
        Value::List(vec![Value::Real(1.0); 4]),
        Value::List((1..=4).map(Value::Int).collect()),
    ];
    assert_failed_infeasible(
        call_form_find(&value_inputs),
        &["non-finite node coordinate", "nodes[0]"],
    );
}

/// A free-standing cable net whose node 2 is seeded at NaN, under a well-formed
/// one-group spec: the free trampoline reports the node, not a solve.
#[test]
fn free_trampoline_non_finite_node_coordinate_is_failed_naming_the_node() {
    let nodes = vec![
        point3(0.0, 0.2, 0.4),
        point3(1.0, 0.0, 0.0),
        point3(-1.0, f64::NAN, 0.0),
        point3(0.0, 1.0, 1.0),
        point3(0.0, -1.0, 1.0),
    ];
    let structure = cable_net(nodes, &[(0, 1), (0, 2), (0, 3), (0, 4)]);
    assert_failed_infeasible(
        call_form_find_free(&free_inputs(structure, &[0, 0, 0, 0], &[1.0], 0)),
        &["non-finite node coordinate", "nodes[2]"],
    );
}

/// A `GroupRatios` spec with a zero seed is a malformed group spec, worded as
/// its own cause rather than as a members-vs-kinds dimension mismatch.
#[test]
fn free_trampoline_zero_seed_ratio_is_failed_as_a_malformed_group_spec() {
    let nodes = vec![
        point3(0.0, 0.2, 0.4),
        point3(1.0, 0.0, 0.0),
        point3(-1.0, 0.0, 0.0),
        point3(0.0, 1.0, 1.0),
        point3(0.0, -1.0, 1.0),
    ];
    let structure = cable_net(nodes, &[(0, 1), (0, 2), (0, 3), (0, 4)]);
    assert_failed_infeasible(
        call_form_find_free(&free_inputs(structure, &[0, 0, 1, 1], &[1.0, 0.0], 0)),
        &["zero seed ratio"],
    );
}
