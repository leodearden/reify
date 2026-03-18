//! Tests for ConcurrentEvalAdapter and edit_param_concurrent.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use reify_compiler::ValueCellKind;
use reify_eval::cache::{EvalOutcome, NodeId};
use reify_eval::deps::{DependencyTrace, ReverseDependencyIndex};
use reify_eval::graph::{EvaluationGraph, ValueCellNode};
use reify_eval::{ConcurrentEditSetup, ConcurrentEditResult};
use reify_runtime::concurrent::{AsyncNodeEvaluator, CancellationToken, ConcurrentScheduler};
use reify_runtime::concurrent_eval::ConcurrentEvalAdapter;
use reify_test_support::TopologyTemplateBuilder;
use reify_test_support::builders::{binop, literal, value_ref_typed};
use reify_types::{
    BinOp, ContentHash, DeterminacyState, PersistentMap, SnapshotId, Type, Value,
    ValueCellId, ValueMap, VersionId,
};

/// Helper: build a simple topology (param a, let b = a * 2) and return
/// a ConcurrentEditSetup as if a was changed from 5 to 10.
fn simple_setup() -> ConcurrentEditSetup {
    let e = "T";

    // Build graph from template
    let a_ref = reify_types::CompiledExpr::value_ref(ValueCellId::new(e, "a"), Type::Real);
    let two = reify_types::CompiledExpr::literal(Value::Real(2.0), Type::Real);
    let b_expr = reify_types::CompiledExpr::binop(BinOp::Mul, a_ref, two, Type::Real);

    let template = TopologyTemplateBuilder::new(e)
        .param(e, "a", Type::Real, Some(reify_types::CompiledExpr::literal(Value::Real(5.0), Type::Real)))
        .let_binding(e, "b", Type::Real, b_expr)
        .build();

    let graph = EvaluationGraph::from_templates(&[template]);

    // Build values: a=10 (the new value)
    let mut values = ValueMap::new();
    values.insert(ValueCellId::new(e, "a"), Value::Real(10.0));
    values.insert(ValueCellId::new(e, "b"), Value::Real(10.0)); // old value

    // Build snapshot_values
    let mut snapshot_values = PersistentMap::new();
    snapshot_values.insert(
        ValueCellId::new(e, "a"),
        (Value::Real(10.0), DeterminacyState::Determined),
    );
    snapshot_values.insert(
        ValueCellId::new(e, "b"),
        (Value::Real(10.0), DeterminacyState::Determined),
    );

    // Build traces
    let mut traces = HashMap::new();
    traces.insert(
        NodeId::Value(ValueCellId::new(e, "a")),
        DependencyTrace::default(),
    );
    traces.insert(
        NodeId::Value(ValueCellId::new(e, "b")),
        DependencyTrace {
            reads: vec![ValueCellId::new(e, "a")],
        },
    );

    // Build reverse index
    let reverse_index = ReverseDependencyIndex::build_from_graph(&graph);

    // Previous hashes: b had hash for old value (Real(10.0))
    let old_hash = reify_eval::cache::CachedResult::Value(
        Value::Real(10.0),
        DeterminacyState::Determined,
    ).content_hash();
    let mut previous_hashes = HashMap::new();
    previous_hashes.insert(NodeId::Value(ValueCellId::new(e, "b")), old_hash);

    // Eval set: only b is dirty (a is the changed param, not in dirty cone)
    let eval_set = vec![NodeId::Value(ValueCellId::new(e, "b"))];

    let mut changed_cells = HashSet::new();
    changed_cells.insert(ValueCellId::new(e, "a"));

    ConcurrentEditSetup {
        eval_set,
        graph,
        values,
        snapshot_values,
        traces,
        reverse_index,
        previous_hashes,
        version: VersionId(1),
        snapshot_id: SnapshotId(1),
        parent_snapshot_id: SnapshotId(0),
        changed_cells,
    }
}

/// step-3: ConcurrentEvalAdapter correctly evaluates a single value node.
#[tokio::test]
async fn adapter_evaluates_single_value_node() {
    let setup = simple_setup();
    let adapter = ConcurrentEvalAdapter::from_setup(&setup);

    let b_node = NodeId::Value(ValueCellId::new("T", "b"));

    // b should be dirty
    assert!(adapter.is_dirty(&b_node), "b should be dirty");

    // Evaluate b: should compute a * 2 = 10 * 2 = 20
    let outcome = adapter.evaluate(b_node.clone()).await;

    // (1) Outcome should be Changed (20.0 != 10.0)
    assert_eq!(outcome, EvalOutcome::Changed, "b should have changed");

    // (2) Verify adapter's values map now has b=20
    let adapter_values = adapter.values();
    assert_eq!(
        adapter_values.get(&ValueCellId::new("T", "b")),
        Some(&Value::Real(20.0)),
        "b should be 20.0"
    );

    // (3) Verify results contain an entry
    let results = adapter.take_results();
    assert_eq!(results.len(), 1, "should have 1 result");
    assert_eq!(results[0].outcome, EvalOutcome::Changed);
}
