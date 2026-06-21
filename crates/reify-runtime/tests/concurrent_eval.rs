//! Tests for ConcurrentEvalAdapter and edit_param_concurrent.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use reify_eval::cache::{CachedResult, EvalOutcome, NodeId};
use reify_eval::deps::{DependencyTrace, ReverseDependencyIndex};
use reify_eval::graph::EvaluationGraph;
use reify_eval::{ConcurrentEditSetup, Engine};
use reify_runtime::concurrent::{AsyncNodeEvaluator, CancellationToken, ConcurrentScheduler};
#[cfg(feature = "test-utils")]
use reify_runtime::concurrent_eval::poison_fields;
use reify_runtime::concurrent_eval::{ConcurrentEvalAdapter, edit_param_concurrent};
use reify_test_support::TopologyTemplateBuilder;
use reify_test_support::mocks::MockConstraintChecker;
use reify_core::{SnapshotId, Type, ValueCellId, VersionId};
use reify_ir::{BinOp, DeterminacyState, PersistentMap, Value, ValueMap};

/// Helper: build a simple topology (param a, let b = a * 2) and return
/// a ConcurrentEditSetup as if a was changed from 5 to 10.
fn simple_setup() -> ConcurrentEditSetup {
    let e = "T";

    // Build graph from template
    let a_ref = reify_ir::CompiledExpr::value_ref(ValueCellId::new(e, "a"), Type::dimensionless_scalar());
    let two = reify_ir::CompiledExpr::literal(Value::Real(2.0), Type::dimensionless_scalar());
    let b_expr = reify_ir::CompiledExpr::binop(BinOp::Mul, a_ref, two, Type::dimensionless_scalar());

    let template = TopologyTemplateBuilder::new(e)
        .param(
            e,
            "a",
            Type::dimensionless_scalar(),
            Some(reify_ir::CompiledExpr::literal(
                Value::Real(5.0),
                Type::dimensionless_scalar(),
            )),
        )
        .let_binding(e, "b", Type::dimensionless_scalar(), b_expr)
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
        DependencyTrace { realization_reads: Vec::new(),
            reads: vec![ValueCellId::new(e, "a")],
        },
    );

    // Build reverse index
    let reverse_index = ReverseDependencyIndex::build_from_graph(&graph);

    // Previous hashes: b had hash for old value (Real(10.0))
    let old_hash =
        CachedResult::Value(Value::Real(10.0), DeterminacyState::Determined).content_hash();
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
        functions: vec![].into(),
        meta_map: Arc::new(HashMap::new()),
        objective: None,
    }
}

/// Helper to build a compiled module from a template for Engine tests.
fn build_module(template: reify_compiler::TopologyTemplate) -> reify_compiler::CompiledModule {
    reify_test_support::CompiledModuleBuilder::new(reify_core::ModulePath::single("test"))
        .template(template)
        .build()
}

/// Test helper: evaluator that panics on any node.
/// Used by rollback_on_task_panicked, repeated_error_then_success_cycle,
/// test_cleanup_on_task_panic, and test_cleanup_on_task_cancelled.
struct PanickingEvaluator;
impl AsyncNodeEvaluator for PanickingEvaluator {
    async fn evaluate(&self, _node: NodeId) -> EvalOutcome {
        panic!("intentional panic in evaluator");
    }
}

/// step-3: ConcurrentEvalAdapter correctly evaluates a single value node.
#[tokio::test]
async fn adapter_evaluates_single_value_node() {
    let setup = simple_setup();
    let adapter = ConcurrentEvalAdapter::from_setup(&setup);

    let b_node = NodeId::Value(ValueCellId::new("T", "b"));

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

/// step-5: edit_param_concurrent on a linear chain produces correct values.
#[tokio::test]
async fn edit_param_concurrent_linear_chain() {
    let e = "T";
    let a_ref = reify_ir::CompiledExpr::value_ref(ValueCellId::new(e, "a"), Type::dimensionless_scalar());
    let two = reify_ir::CompiledExpr::literal(Value::Real(2.0), Type::dimensionless_scalar());
    let b_expr = reify_ir::CompiledExpr::binop(BinOp::Mul, a_ref, two, Type::dimensionless_scalar());

    let template = TopologyTemplateBuilder::new(e)
        .param(
            e,
            "a",
            Type::dimensionless_scalar(),
            Some(reify_ir::CompiledExpr::literal(
                Value::Real(5.0),
                Type::dimensionless_scalar(),
            )),
        )
        .let_binding(e, "b", Type::dimensionless_scalar(), b_expr)
        .build();

    let module = build_module(template);
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    // Cold-start eval
    let _initial = engine.eval(&module);

    let a_id = ValueCellId::new(e, "a");
    let b_id = ValueCellId::new(e, "b");
    let cancel = CancellationToken::new();

    // Concurrent edit: change a from 5 to 50
    let (_setup, result) =
        edit_param_concurrent(&mut engine, a_id.clone(), Value::Real(50.0), &cancel)
            .await
            .unwrap();

    // Verify values: a=50, b=100 (50*2)
    assert_eq!(result.values.get(&a_id), Some(&Value::Real(50.0)));
    assert_eq!(result.values.get(&b_id), Some(&Value::Real(100.0)));

    // Verify actual_eval_set contains b
    assert!(
        result.actual_eval_set.contains(&NodeId::Value(b_id)),
        "actual_eval_set should contain b"
    );
}

/// step-7: concurrent eval of 3 independent let bindings at the same level.
#[tokio::test]
async fn concurrent_three_independent_lets() {
    let e = "T";

    // param a, let x = a+1, let y = a+2, let z = a+3
    let a_ref = || reify_ir::CompiledExpr::value_ref(ValueCellId::new(e, "a"), Type::dimensionless_scalar());
    let x_expr = reify_ir::CompiledExpr::binop(
        BinOp::Add,
        a_ref(),
        reify_ir::CompiledExpr::literal(Value::Real(1.0), Type::dimensionless_scalar()),
        Type::dimensionless_scalar(),
    );
    let y_expr = reify_ir::CompiledExpr::binop(
        BinOp::Add,
        a_ref(),
        reify_ir::CompiledExpr::literal(Value::Real(2.0), Type::dimensionless_scalar()),
        Type::dimensionless_scalar(),
    );
    let z_expr = reify_ir::CompiledExpr::binop(
        BinOp::Add,
        a_ref(),
        reify_ir::CompiledExpr::literal(Value::Real(3.0), Type::dimensionless_scalar()),
        Type::dimensionless_scalar(),
    );

    let template = TopologyTemplateBuilder::new(e)
        .param(
            e,
            "a",
            Type::dimensionless_scalar(),
            Some(reify_ir::CompiledExpr::literal(
                Value::Real(5.0),
                Type::dimensionless_scalar(),
            )),
        )
        .let_binding(e, "x", Type::dimensionless_scalar(), x_expr)
        .let_binding(e, "y", Type::dimensionless_scalar(), y_expr)
        .let_binding(e, "z", Type::dimensionless_scalar(), z_expr)
        .build();

    let module = build_module(template);
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let _initial = engine.eval(&module);

    let a_id = ValueCellId::new(e, "a");
    let cancel = CancellationToken::new();

    // Change a from 5 to 10
    let (_setup, result) =
        edit_param_concurrent(&mut engine, a_id.clone(), Value::Real(10.0), &cancel)
            .await
            .unwrap();

    // All three should be correct: x=11, y=12, z=13
    assert_eq!(
        result.values.get(&ValueCellId::new(e, "x")),
        Some(&Value::Real(11.0))
    );
    assert_eq!(
        result.values.get(&ValueCellId::new(e, "y")),
        Some(&Value::Real(12.0))
    );
    assert_eq!(
        result.values.get(&ValueCellId::new(e, "z")),
        Some(&Value::Real(13.0))
    );

    // All three should appear in actual_eval_set and node_results
    assert_eq!(
        result.actual_eval_set.len(),
        3,
        "actual_eval_set: {:?}",
        result.actual_eval_set
    );
    assert_eq!(
        result.node_results.len(),
        3,
        "node_results: {:?}",
        result.node_results
    );
}

/// step-9: multi-level diamond dependency.
#[tokio::test]
async fn concurrent_diamond_dependency() {
    let e = "T";

    // param a, let b = a * 2, let c = a + 1, let d = b + c
    let a_ref = || reify_ir::CompiledExpr::value_ref(ValueCellId::new(e, "a"), Type::dimensionless_scalar());
    let b_ref = || reify_ir::CompiledExpr::value_ref(ValueCellId::new(e, "b"), Type::dimensionless_scalar());
    let c_ref = || reify_ir::CompiledExpr::value_ref(ValueCellId::new(e, "c"), Type::dimensionless_scalar());

    let b_expr = reify_ir::CompiledExpr::binop(
        BinOp::Mul,
        a_ref(),
        reify_ir::CompiledExpr::literal(Value::Real(2.0), Type::dimensionless_scalar()),
        Type::dimensionless_scalar(),
    );
    let c_expr = reify_ir::CompiledExpr::binop(
        BinOp::Add,
        a_ref(),
        reify_ir::CompiledExpr::literal(Value::Real(1.0), Type::dimensionless_scalar()),
        Type::dimensionless_scalar(),
    );
    let d_expr = reify_ir::CompiledExpr::binop(BinOp::Add, b_ref(), c_ref(), Type::dimensionless_scalar());

    let template = TopologyTemplateBuilder::new(e)
        .param(
            e,
            "a",
            Type::dimensionless_scalar(),
            Some(reify_ir::CompiledExpr::literal(
                Value::Real(5.0),
                Type::dimensionless_scalar(),
            )),
        )
        .let_binding(e, "b", Type::dimensionless_scalar(), b_expr)
        .let_binding(e, "c", Type::dimensionless_scalar(), c_expr)
        .let_binding(e, "d", Type::dimensionless_scalar(), d_expr)
        .build();

    let module = build_module(template);
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let _initial = engine.eval(&module);

    let a_id = ValueCellId::new(e, "a");
    let cancel = CancellationToken::new();

    // Change a from 5 to 10
    let (_setup, result) =
        edit_param_concurrent(&mut engine, a_id.clone(), Value::Real(10.0), &cancel)
            .await
            .unwrap();

    // b = 10 * 2 = 20, c = 10 + 1 = 11, d = 20 + 11 = 31
    assert_eq!(
        result.values.get(&ValueCellId::new(e, "b")),
        Some(&Value::Real(20.0))
    );
    assert_eq!(
        result.values.get(&ValueCellId::new(e, "c")),
        Some(&Value::Real(11.0))
    );
    assert_eq!(
        result.values.get(&ValueCellId::new(e, "d")),
        Some(&Value::Real(31.0)),
        "d should be 20 + 11 = 31 (not stale values)"
    );
}

/// step-11: early cutoff in concurrent mode.
#[tokio::test]
async fn concurrent_early_cutoff() {
    let e = "T";

    // param a, let x = a - a (always 0), let y = x + 1
    let a_ref = || reify_ir::CompiledExpr::value_ref(ValueCellId::new(e, "a"), Type::dimensionless_scalar());
    let x_ref = || reify_ir::CompiledExpr::value_ref(ValueCellId::new(e, "x"), Type::dimensionless_scalar());

    let x_expr = reify_ir::CompiledExpr::binop(BinOp::Sub, a_ref(), a_ref(), Type::dimensionless_scalar());
    let y_expr = reify_ir::CompiledExpr::binop(
        BinOp::Add,
        x_ref(),
        reify_ir::CompiledExpr::literal(Value::Real(1.0), Type::dimensionless_scalar()),
        Type::dimensionless_scalar(),
    );

    let template = TopologyTemplateBuilder::new(e)
        .param(
            e,
            "a",
            Type::dimensionless_scalar(),
            Some(reify_ir::CompiledExpr::literal(
                Value::Real(5.0),
                Type::dimensionless_scalar(),
            )),
        )
        .let_binding(e, "x", Type::dimensionless_scalar(), x_expr)
        .let_binding(e, "y", Type::dimensionless_scalar(), y_expr)
        .build();

    let module = build_module(template);
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let _initial = engine.eval(&module);

    let a_id = ValueCellId::new(e, "a");
    let cancel = CancellationToken::new();

    // Change a from 5 to 7 — x = a - a = 0 (same as before) → Unchanged
    let (_setup, result) =
        edit_param_concurrent(&mut engine, a_id.clone(), Value::Real(7.0), &cancel)
            .await
            .unwrap();

    // (1) x should appear in actual_eval_set with outcome Unchanged
    let x_node = NodeId::Value(ValueCellId::new(e, "x"));
    let y_node = NodeId::Value(ValueCellId::new(e, "y"));

    assert!(
        result.actual_eval_set.contains(&x_node),
        "x should be in actual_eval_set: {:?}",
        result.actual_eval_set
    );
    let x_result = result
        .node_results
        .iter()
        .find(|r| r.node == x_node)
        .unwrap();
    assert_eq!(
        x_result.outcome,
        EvalOutcome::Unchanged,
        "x should be Unchanged"
    );

    // (2) y should be in skipped set
    assert!(
        result.skipped.contains(&y_node),
        "y should be in skipped set: {:?}",
        result.skipped
    );

    // (3) y should NOT appear in node_results
    assert!(
        !result.node_results.iter().any(|r| r.node == y_node),
        "y should NOT appear in node_results"
    );

    // (4) y should retain its value of 1.0 (0 + 1)
    assert_eq!(
        result.values.get(&ValueCellId::new(e, "y")),
        Some(&Value::Real(1.0)),
        "y should retain value of 1.0"
    );
}

/// step-15: cancellation stops concurrent evaluation between levels.
#[tokio::test]
async fn concurrent_cancellation_between_levels() {
    let e = "T";

    // param a → let b = a * 2 (level 0), b → let c = b + 1 (level 1)
    let a_ref = || reify_ir::CompiledExpr::value_ref(ValueCellId::new(e, "a"), Type::dimensionless_scalar());
    let b_ref = || reify_ir::CompiledExpr::value_ref(ValueCellId::new(e, "b"), Type::dimensionless_scalar());

    let b_expr = reify_ir::CompiledExpr::binop(
        BinOp::Mul,
        a_ref(),
        reify_ir::CompiledExpr::literal(Value::Real(2.0), Type::dimensionless_scalar()),
        Type::dimensionless_scalar(),
    );
    let c_expr = reify_ir::CompiledExpr::binop(
        BinOp::Add,
        b_ref(),
        reify_ir::CompiledExpr::literal(Value::Real(1.0), Type::dimensionless_scalar()),
        Type::dimensionless_scalar(),
    );

    let template = TopologyTemplateBuilder::new(e)
        .param(
            e,
            "a",
            Type::dimensionless_scalar(),
            Some(reify_ir::CompiledExpr::literal(
                Value::Real(5.0),
                Type::dimensionless_scalar(),
            )),
        )
        .let_binding(e, "b", Type::dimensionless_scalar(), b_expr)
        .let_binding(e, "c", Type::dimensionless_scalar(), c_expr)
        .build();

    let module = build_module(template);
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let _initial = engine.eval(&module);

    // Set up a cancel token that we'll trigger after level 0
    let cancel = CancellationToken::new();

    // Use the lower-level API to control cancellation timing
    let a_id = ValueCellId::new(e, "a");
    let setup = engine
        .prepare_concurrent_edit(a_id, Value::Real(10.0))
        .unwrap();
    let eval_set = setup.eval_set.clone();
    let traces = setup.traces.clone();

    // Create a custom evaluator that cancels after first evaluation
    struct CancellingAdapter {
        inner: ConcurrentEvalAdapter,
        cancel: CancellationToken,
    }

    impl AsyncNodeEvaluator for CancellingAdapter {
        async fn evaluate(&self, node: NodeId) -> EvalOutcome {
            let outcome = self.inner.evaluate(node).await;
            // Cancel after evaluating (first node triggers cancellation)
            self.cancel.cancel();
            outcome
        }
    }

    let adapter = ConcurrentEvalAdapter::from_setup(&setup);
    let cancelling = Arc::new(CancellingAdapter {
        inner: adapter,
        cancel: cancel.clone(),
    });

    let scheduler = ConcurrentScheduler;
    let _result = scheduler
        .execute(
            eval_set.clone(),
            cancelling.clone(),
            &traces,
            &cancel,
            &setup.changed_cells,
        )
        .await
        .unwrap();

    let b_node = NodeId::Value(ValueCellId::new(e, "b"));
    let c_node = NodeId::Value(ValueCellId::new(e, "c"));

    // (1) b should appear in results (level 0 completed)
    let results = cancelling.inner.take_results();
    let b_evaluated = results.iter().any(|r| r.node == b_node);
    assert!(b_evaluated, "b should have been evaluated");

    // (2) c should NOT appear in results (level 1 was cancelled)
    let c_evaluated = results.iter().any(|r| r.node == c_node);
    assert!(
        !c_evaluated,
        "c should NOT have been evaluated (cancelled between levels)"
    );

    // (3) Function returned Ok (cooperative cancellation)
    // (verified by the .unwrap() above)
}

/// step-17: bracket topology concurrent edit matches sequential.
#[tokio::test]
async fn bracket_concurrent_matches_sequential() {
    use reify_test_support::bracket_compiled_module;

    let module = bracket_compiled_module();
    let e = "Bracket";

    // Sequential engine
    let checker_seq = MockConstraintChecker::new();
    let mut engine_seq = Engine::new(Box::new(checker_seq), None);
    engine_seq.eval(&module);

    // Concurrent engine
    let checker_con = MockConstraintChecker::new();
    let mut engine_con = Engine::new(Box::new(checker_con), None);
    engine_con.eval(&module);

    let width_id = ValueCellId::new(e, "width");

    // Sequential edit
    let seq_result = engine_seq
        .edit_param(width_id.clone(), Value::length(0.1))
        .unwrap();

    // Concurrent edit
    let cancel = CancellationToken::new();
    let (_setup, con_result) = edit_param_concurrent(
        &mut engine_con,
        width_id.clone(),
        Value::length(0.1),
        &cancel,
    )
    .await
    .unwrap();

    // (1) All values should match exactly
    for (id, seq_val) in seq_result.values.iter() {
        let con_val = con_result.values.get(id);
        assert_eq!(Some(seq_val), con_val, "values should match for {:?}", id);
    }

    // (2) Both should report the same evaluated nodes
    // Sequential: volume is the Value node in eval set for width change
    let seq_eval_set = engine_seq.last_eval_set();
    let volume_node = NodeId::Value(ValueCellId::new(e, "volume"));

    // Both should have volume in their eval sets
    assert!(
        seq_eval_set.contains(&volume_node),
        "sequential eval set should contain volume: {:?}",
        seq_eval_set
    );
    assert!(
        con_result.actual_eval_set.contains(&volume_node),
        "concurrent eval set should contain volume: {:?}",
        con_result.actual_eval_set
    );
}

/// step-21: rollback Pending state when scheduler returns Err(TaskPanicked).
#[tokio::test]
async fn rollback_on_task_panicked_restores_engine_state() {
    use reify_runtime::concurrent::SchedulerError;
    use reify_ir::Freshness;

    let e = "T";
    let a_ref = reify_ir::CompiledExpr::value_ref(ValueCellId::new(e, "a"), Type::dimensionless_scalar());
    let two = reify_ir::CompiledExpr::literal(Value::Real(2.0), Type::dimensionless_scalar());
    let b_expr = reify_ir::CompiledExpr::binop(BinOp::Mul, a_ref, two, Type::dimensionless_scalar());

    let template = TopologyTemplateBuilder::new(e)
        .param(
            e,
            "a",
            Type::dimensionless_scalar(),
            Some(reify_ir::CompiledExpr::literal(
                Value::Real(5.0),
                Type::dimensionless_scalar(),
            )),
        )
        .let_binding(e, "b", Type::dimensionless_scalar(), b_expr)
        .build();

    let module = build_module(template);
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let _initial = engine.eval(&module);

    let a_id = ValueCellId::new(e, "a");
    let b_node = NodeId::Value(ValueCellId::new(e, "b"));

    // Prepare concurrent edit — marks b as Pending
    let setup = engine
        .prepare_concurrent_edit(a_id.clone(), Value::Real(50.0))
        .unwrap();

    // Verify b is Pending
    let entry = engine.cache_store().get(&b_node).unwrap();
    assert!(
        matches!(entry.freshness, Freshness::Pending { .. }),
        "b should be Pending after prepare"
    );

    let panicking = Arc::new(PanickingEvaluator);
    let cancel = CancellationToken::new();
    let scheduler = ConcurrentScheduler;

    // Execute — should return Err(TaskPanicked)
    let result = scheduler
        .execute(
            setup.eval_set.clone(),
            panicking,
            &setup.traces,
            &cancel,
            &setup.changed_cells,
        )
        .await;

    assert!(result.is_err(), "scheduler should return error on panic");
    match result.unwrap_err() {
        SchedulerError::TaskPanicked(_) => {} // expected
        other => panic!("Expected TaskPanicked, got {:?}", other),
    }

    // Rollback
    engine.rollback_concurrent_edit(&setup);

    // (1) All nodes in eval_set should have freshness=Final (not stuck in Pending)
    let entry = engine.cache_store().get(&b_node).unwrap();
    assert_eq!(
        entry.freshness,
        Freshness::Final,
        "b should be Final after rollback, got: {:?}",
        entry.freshness
    );

    // (2) Sequential edit_param should succeed with correct values
    let seq_result = engine.edit_param(a_id.clone(), Value::Real(50.0)).unwrap();
    assert_eq!(
        seq_result.values.get(&ValueCellId::new(e, "b")),
        Some(&Value::Real(100.0)),
        "b should be 50 * 2 = 100 after sequential edit"
    );
}

/// step-23: Repeated error-then-success cycle validates full recovery.
#[tokio::test]
async fn repeated_error_then_success_cycle() {
    use reify_runtime::concurrent::SchedulerError;
    use reify_ir::Freshness;

    let e = "T";

    // param a, let b = a * 2, let c = b + 1
    let a_ref = || reify_ir::CompiledExpr::value_ref(ValueCellId::new(e, "a"), Type::dimensionless_scalar());
    let b_ref = || reify_ir::CompiledExpr::value_ref(ValueCellId::new(e, "b"), Type::dimensionless_scalar());

    let b_expr = reify_ir::CompiledExpr::binop(
        BinOp::Mul,
        a_ref(),
        reify_ir::CompiledExpr::literal(Value::Real(2.0), Type::dimensionless_scalar()),
        Type::dimensionless_scalar(),
    );
    let c_expr = reify_ir::CompiledExpr::binop(
        BinOp::Add,
        b_ref(),
        reify_ir::CompiledExpr::literal(Value::Real(1.0), Type::dimensionless_scalar()),
        Type::dimensionless_scalar(),
    );

    let template = TopologyTemplateBuilder::new(e)
        .param(
            e,
            "a",
            Type::dimensionless_scalar(),
            Some(reify_ir::CompiledExpr::literal(
                Value::Real(5.0),
                Type::dimensionless_scalar(),
            )),
        )
        .let_binding(e, "b", Type::dimensionless_scalar(), b_expr)
        .let_binding(e, "c", Type::dimensionless_scalar(), c_expr)
        .build();

    let module = build_module(template);
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let _initial = engine.eval(&module);

    let a_id = ValueCellId::new(e, "a");
    let b_node = NodeId::Value(ValueCellId::new(e, "b"));
    let c_node = NodeId::Value(ValueCellId::new(e, "c"));

    // === First cycle: prepare → panicking scheduler → rollback ===
    let setup1 = engine
        .prepare_concurrent_edit(a_id.clone(), Value::Real(20.0))
        .unwrap();

    let cancel = CancellationToken::new();
    let scheduler = ConcurrentScheduler;
    let err_result = scheduler
        .execute(
            setup1.eval_set.clone(),
            Arc::new(PanickingEvaluator),
            &setup1.traces,
            &cancel,
            &setup1.changed_cells,
        )
        .await;
    assert!(matches!(err_result, Err(SchedulerError::TaskPanicked(_))));

    // Rollback
    engine.rollback_concurrent_edit(&setup1);

    // Verify engine is in clean state
    let entry_b = engine.cache_store().get(&b_node).unwrap();
    assert_eq!(
        entry_b.freshness,
        Freshness::Final,
        "b should be Final after rollback"
    );
    let entry_c = engine.cache_store().get(&c_node).unwrap();
    assert_eq!(
        entry_c.freshness,
        Freshness::Final,
        "c should be Final after rollback"
    );

    // === Second cycle: edit_param_concurrent should succeed normally ===
    let (setup2, result2) =
        edit_param_concurrent(&mut engine, a_id.clone(), Value::Real(20.0), &cancel)
            .await
            .unwrap();

    // Values should be correct: a=20, b=40, c=41
    assert_eq!(
        result2.values.get(&ValueCellId::new(e, "a")),
        Some(&Value::Real(20.0))
    );
    assert_eq!(
        result2.values.get(&ValueCellId::new(e, "b")),
        Some(&Value::Real(40.0))
    );
    assert_eq!(
        result2.values.get(&ValueCellId::new(e, "c")),
        Some(&Value::Real(41.0))
    );

    // === Third: apply and verify engine state is fully correct ===
    engine.apply_concurrent_edit(&setup2, result2);

    // Cache freshness should be Final for all evaluated nodes
    let entry_b = engine.cache_store().get(&b_node).unwrap();
    assert_eq!(
        entry_b.freshness,
        Freshness::Final,
        "b should be Final after apply"
    );
    let entry_c = engine.cache_store().get(&c_node).unwrap();
    assert_eq!(
        entry_c.freshness,
        Freshness::Final,
        "c should be Final after apply"
    );

    // Snapshot should have correct version
    let snapshot = engine.snapshot().unwrap();
    assert_eq!(
        snapshot.version, setup2.version,
        "version should match setup2"
    );

    // Values in snapshot should be correct
    let (b_val, _) = snapshot.values.get(&ValueCellId::new(e, "b")).unwrap();
    assert_eq!(*b_val, Value::Real(40.0), "snapshot b should be 40");
    let (c_val, _) = snapshot.values.get(&ValueCellId::new(e, "c")).unwrap();
    assert_eq!(*c_val, Value::Real(41.0), "snapshot c should be 41");
}

/// Mixed fan-in in concurrent mode: when an unchanged intermediary's
/// dependents ALSO read the changed param directly, early cutoff must
/// NOT skip them.
///
/// Graph:
///   param a (Int, default 5)
///   let x = if a > 0 then 1 else 1  (reads a, always 1 → Unchanged)
///   let y = a + x                    (reads BOTH a and x → mixed fan-in)
///
/// Edit a → 10: x re-evals to 1 (Unchanged), y MUST re-eval to 11.
#[tokio::test]
async fn mixed_fan_in_concurrent_unchanged_upstream_does_not_skip_shared_downstream() {
    use reify_core::ContentHash;
    use reify_ir::{CompiledExpr, CompiledExprKind};

    let e = "T";

    // Build conditional: if a > 0 then 1 else 1 (always 1, reads a)
    let condition = CompiledExpr::binop(
        BinOp::Gt,
        CompiledExpr::value_ref(ValueCellId::new(e, "a"), Type::Int),
        CompiledExpr::literal(Value::Int(0), Type::Int),
        Type::Bool,
    );
    let conditional = CompiledExpr {
        kind: CompiledExprKind::Conditional {
            condition: Box::new(condition),
            then_branch: Box::new(CompiledExpr::literal(Value::Int(1), Type::Int)),
            else_branch: Box::new(CompiledExpr::literal(Value::Int(1), Type::Int)),
        },
        result_type: Type::Int,
        content_hash: ContentHash::of_str("if_a_gt_0_then_1_else_1"),
    };

    // let y = a + x (reads both a and x)
    let y_expr = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::value_ref(ValueCellId::new(e, "a"), Type::Int),
        CompiledExpr::value_ref(ValueCellId::new(e, "x"), Type::Int),
        Type::Int,
    );

    let template = TopologyTemplateBuilder::new(e)
        .param(
            e,
            "a",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(5), Type::Int)),
        )
        .let_binding(e, "x", Type::Int, conditional)
        .let_binding(e, "y", Type::Int, y_expr)
        .build();

    let module = build_module(template);
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let _initial = engine.eval(&module);

    let a_id = ValueCellId::new(e, "a");
    let cancel = CancellationToken::new();

    // Concurrent edit: change a from 5 to 10
    let (_setup, result) =
        edit_param_concurrent(&mut engine, a_id.clone(), Value::Int(10), &cancel)
            .await
            .unwrap();

    let y_node = NodeId::Value(ValueCellId::new(e, "y"));

    // y MUST be in actual_eval_set (not skipped)
    assert!(
        result.actual_eval_set.contains(&y_node),
        "y should be in actual_eval_set (reads changed param a directly, \
         even though x is Unchanged). actual_eval_set: {:?}",
        result.actual_eval_set
    );

    // y must have the correct re-evaluated value: 10 + 1 = 11
    assert_eq!(
        result.values.get(&ValueCellId::new(e, "y")),
        Some(&Value::Int(11)),
        "y should be 10 + 1 = 11, NOT stale 6"
    );
}

/// Triple fan-in in concurrent mode: two unchanged intermediaries plus a
/// changed param all feed into the same downstream node.
///
/// Graph:
///   param a (Int, 5)
///   let x = if a > 0 then 1 else 1  (reads a, always 1 → Unchanged)
///   let z = if a > 0 then 2 else 2  (reads a, always 2 → Unchanged)
///   let y = a + x + z               (reads a, x, AND z → triple fan-in)
///
/// Edit a → 10: x=1, z=2 (both Unchanged), y MUST re-eval to 13.
#[tokio::test]
async fn concurrent_triple_fan_in_mixed_early_cutoff() {
    use reify_core::ContentHash;
    use reify_ir::{CompiledExpr, CompiledExprKind};

    let e = "T";

    // Build conditional: if a > 0 then 1 else 1 (always 1)
    let x_cond = CompiledExpr::binop(
        BinOp::Gt,
        CompiledExpr::value_ref(ValueCellId::new(e, "a"), Type::Int),
        CompiledExpr::literal(Value::Int(0), Type::Int),
        Type::Bool,
    );
    let x_expr = CompiledExpr {
        kind: CompiledExprKind::Conditional {
            condition: Box::new(x_cond),
            then_branch: Box::new(CompiledExpr::literal(Value::Int(1), Type::Int)),
            else_branch: Box::new(CompiledExpr::literal(Value::Int(1), Type::Int)),
        },
        result_type: Type::Int,
        content_hash: ContentHash::of_str("if_a_gt_0_then_1_else_1"),
    };

    // Build conditional: if a > 0 then 2 else 2 (always 2)
    let z_cond = CompiledExpr::binop(
        BinOp::Gt,
        CompiledExpr::value_ref(ValueCellId::new(e, "a"), Type::Int),
        CompiledExpr::literal(Value::Int(0), Type::Int),
        Type::Bool,
    );
    let z_expr = CompiledExpr {
        kind: CompiledExprKind::Conditional {
            condition: Box::new(z_cond),
            then_branch: Box::new(CompiledExpr::literal(Value::Int(2), Type::Int)),
            else_branch: Box::new(CompiledExpr::literal(Value::Int(2), Type::Int)),
        },
        result_type: Type::Int,
        content_hash: ContentHash::of_str("if_a_gt_0_then_2_else_2"),
    };

    // let y = (a + x) + z
    let a_plus_x = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::value_ref(ValueCellId::new(e, "a"), Type::Int),
        CompiledExpr::value_ref(ValueCellId::new(e, "x"), Type::Int),
        Type::Int,
    );
    let y_expr = CompiledExpr::binop(
        BinOp::Add,
        a_plus_x,
        CompiledExpr::value_ref(ValueCellId::new(e, "z"), Type::Int),
        Type::Int,
    );

    let template = TopologyTemplateBuilder::new(e)
        .param(
            e,
            "a",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(5), Type::Int)),
        )
        .let_binding(e, "x", Type::Int, x_expr)
        .let_binding(e, "z", Type::Int, z_expr)
        .let_binding(e, "y", Type::Int, y_expr)
        .build();

    let module = build_module(template);
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let _initial = engine.eval(&module);

    let a_id = ValueCellId::new(e, "a");
    let cancel = CancellationToken::new();

    // Concurrent edit: change a from 5 to 10
    let (_setup, result) =
        edit_param_concurrent(&mut engine, a_id.clone(), Value::Int(10), &cancel)
            .await
            .unwrap();

    let y_node = NodeId::Value(ValueCellId::new(e, "y"));

    // y MUST be in actual_eval_set (not skipped)
    assert!(
        result.actual_eval_set.contains(&y_node),
        "y should be in actual_eval_set despite both x and z being Unchanged. \
         actual_eval_set: {:?}",
        result.actual_eval_set
    );

    // y must have the correct value: 10 + 1 + 2 = 13
    assert_eq!(
        result.values.get(&ValueCellId::new(e, "y")),
        Some(&Value::Int(13)),
        "y should be 10 + 1 + 2 = 13, NOT stale 8"
    );
}

/// After concurrent edit of a param that affects constraints governing auto params,
/// the solver should re-run and resolved values should be propagated.
///
/// Module: param `a` (default mm(3.0)), auto `x`, let `y = x * 2.0`,
/// constraint `x > a`. SequencedMockSolver: 1st call x=mm(5.0), 2nd x=mm(20.0).
///
/// Cold eval → x=mm(5.0)=0.005 SI, y = 0.01 SI.
/// Concurrent edit a→mm(8.0) → solver re-resolves x to mm(20.0)=0.02 SI.
/// After apply: y should be 0.04 SI, x should be 0.02 SI.
#[tokio::test]
async fn edit_param_concurrent_re_resolves_auto_params() {
    use reify_test_support::builders::{binop, gt, literal, value_ref};
    use reify_test_support::mm;
    use reify_test_support::mocks::SequencedMockConstraintSolver;
    use reify_ir::SolveResult;

    let a_id = ValueCellId::new("S", "a");
    let x_id = ValueCellId::new("S", "x");
    let y_id = ValueCellId::new("S", "y");

    // Sequenced solver: first x=mm(5.0), second x=mm(20.0)
    let mut solved1 = HashMap::new();
    solved1.insert(x_id.clone(), mm(5.0));
    let mut solved2 = HashMap::new();
    solved2.insert(x_id.clone(), mm(20.0));

    let solver = SequencedMockConstraintSolver::new(vec![
        SolveResult::Solved {
            values: solved1,
            unique: true,
        },
        SolveResult::Solved {
            values: solved2,
            unique: true,
        },
    ]);

    let template = TopologyTemplateBuilder::new("S")
        .param("S", "a", Type::length(), Some(literal(mm(3.0))))
        .auto_param("S", "x", Type::length())
        .let_binding(
            "S",
            "y",
            Type::length(),
            binop(BinOp::Mul, value_ref("S", "x"), literal(Value::Real(2.0))),
        )
        .constraint("S", 0, None, gt(value_ref("S", "x"), value_ref("S", "a")))
        .build();

    let module = build_module(template);
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None).with_solver(Box::new(solver));

    // Cold eval: x resolved to mm(5.0), y = 0.005*2 = 0.01
    let cold = engine.eval(&module);
    assert!(
        !cold.resolved_params.is_empty(),
        "cold eval should resolve auto params"
    );

    let cancel = CancellationToken::new();

    // Concurrent edit: change a from mm(3.0) to mm(8.0)
    let (setup, result) = edit_param_concurrent(&mut engine, a_id.clone(), mm(8.0), &cancel)
        .await
        .unwrap();

    // Apply the concurrent edit
    engine.apply_concurrent_edit(&setup, result);

    // After apply, check engine snapshot values
    let snap = engine.snapshot().expect("snapshot should exist");

    // x should be re-resolved to mm(20.0) = 0.02 SI
    let (x_val, _) = snap.values.get(&x_id).expect("x should be in snapshot");
    assert!(
        matches!(x_val, Value::Scalar { si_value, .. } if (*si_value - 0.02).abs() < 1e-10),
        "expected x = mm(20.0) = 0.02 SI after concurrent edit, got {:?}",
        x_val
    );

    // y should be re-evaluated: mm(20.0)*2 = 0.04 SI
    let (y_val, _) = snap.values.get(&y_id).expect("y should be in snapshot");
    assert!(
        matches!(y_val, Value::Scalar { si_value, .. } if (*si_value - 0.04).abs() < 1e-10),
        "expected y = 0.04 SI after concurrent edit with re-resolution, got {:?}",
        y_val
    );
}

/// After a concurrent edit that triggers auto-resolution, the returned
/// `ConcurrentEditResult` should include `resolved_params` with the solver's
/// resolved values.
///
/// Module: param `a` (default mm(3.0)), auto `x`, let `y = x * 2.0`,
/// constraint `x > a`. SequencedMockSolver: 1st call x=mm(5.0), 2nd x=mm(20.0).
///
/// Cold eval → x=mm(5.0). Concurrent edit a→mm(8.0) → solver re-resolves x to mm(20.0).
/// Assert result.resolved_params contains x→mm(20.0).
#[tokio::test]
async fn concurrent_edit_result_includes_resolved_params() {
    use reify_test_support::builders::{binop, gt, literal, value_ref};
    use reify_test_support::mm;
    use reify_test_support::mocks::SequencedMockConstraintSolver;
    use reify_ir::SolveResult;

    let a_id = ValueCellId::new("S", "a");
    let x_id = ValueCellId::new("S", "x");

    // Sequenced solver: first x=mm(5.0), second x=mm(20.0)
    let mut solved1 = HashMap::new();
    solved1.insert(x_id.clone(), mm(5.0));
    let mut solved2 = HashMap::new();
    solved2.insert(x_id.clone(), mm(20.0));

    let solver = SequencedMockConstraintSolver::new(vec![
        SolveResult::Solved {
            values: solved1,
            unique: true,
        },
        SolveResult::Solved {
            values: solved2,
            unique: true,
        },
    ]);

    let template = TopologyTemplateBuilder::new("S")
        .param("S", "a", Type::length(), Some(literal(mm(3.0))))
        .auto_param("S", "x", Type::length())
        .let_binding(
            "S",
            "y",
            Type::length(),
            binop(BinOp::Mul, value_ref("S", "x"), literal(Value::Real(2.0))),
        )
        .constraint("S", 0, None, gt(value_ref("S", "x"), value_ref("S", "a")))
        .build();

    let module = build_module(template);
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None).with_solver(Box::new(solver));

    // Cold eval: x resolved to mm(5.0)
    let cold = engine.eval(&module);
    assert!(
        !cold.resolved_params.is_empty(),
        "cold eval should resolve auto params"
    );

    let cancel = CancellationToken::new();

    // Concurrent edit: change a from mm(3.0) to mm(8.0)
    let (_setup, result) = edit_param_concurrent(&mut engine, a_id.clone(), mm(8.0), &cancel)
        .await
        .unwrap();

    // The ConcurrentEditResult should carry resolved_params
    assert!(
        !result.resolved_params.is_empty(),
        "ConcurrentEditResult should include resolved_params after re-resolution"
    );
    let resolved_x = result
        .resolved_params
        .get(&x_id)
        .expect("resolved_params should contain x");
    assert!(
        matches!(resolved_x, Value::Scalar { si_value, .. } if (*si_value - 0.02).abs() < 1e-10),
        "expected resolved x = mm(20.0) = 0.02 SI, got {:?}",
        resolved_x
    );

    // diagnostics should be empty (successful solve)
    assert!(
        result.diagnostics.is_empty(),
        "diagnostics should be empty on successful solve, got {:?}",
        result.diagnostics
    );
}

/// When the solver returns Infeasible during a concurrent edit, the
/// `ConcurrentEditResult` should carry the solver's diagnostic messages.
///
/// Module: param `a` (default mm(1.0)), auto `x`, constraint `x > a`.
/// SequencedMockSolver: 1st call Solved x=mm(5.0), 2nd call Infeasible
/// with a diagnostic message.
///
/// Cold eval → x=mm(5.0). Concurrent edit a→mm(10.0) → solver infeasible.
/// Assert result.diagnostics is non-empty and contains the infeasibility message.
#[tokio::test]
async fn concurrent_edit_result_includes_diagnostics_on_infeasible() {
    use reify_test_support::builders::{gt, literal, value_ref};
    use reify_test_support::mm;
    use reify_test_support::mocks::SequencedMockConstraintSolver;
    use reify_core::Diagnostic;
    use reify_ir::SolveResult;

    let a_id = ValueCellId::new("S", "a");
    let x_id = ValueCellId::new("S", "x");

    // Sequenced solver: first Solved, second Infeasible
    let mut solved1 = HashMap::new();
    solved1.insert(x_id.clone(), mm(5.0));

    let solver = SequencedMockConstraintSolver::new(vec![
        SolveResult::Solved {
            values: solved1,
            unique: true,
        },
        SolveResult::Infeasible {
            diagnostics: vec![Diagnostic::error("constraint x > a is infeasible")],
        },
    ]);

    let template = TopologyTemplateBuilder::new("S")
        .param("S", "a", Type::length(), Some(literal(mm(1.0))))
        .auto_param("S", "x", Type::length())
        .constraint("S", 0, None, gt(value_ref("S", "x"), value_ref("S", "a")))
        .build();

    let module = build_module(template);
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None).with_solver(Box::new(solver));

    // Cold eval: x resolved to mm(5.0)
    let cold = engine.eval(&module);
    assert!(
        !cold.resolved_params.is_empty(),
        "cold eval should resolve auto params"
    );

    let cancel = CancellationToken::new();

    // Concurrent edit: change a from mm(1.0) to mm(10.0) — triggers infeasible solve
    let (_setup, result) = edit_param_concurrent(&mut engine, a_id.clone(), mm(10.0), &cancel)
        .await
        .unwrap();

    // resolved_params should be empty (infeasible solve doesn't produce resolved values)
    assert!(
        result.resolved_params.is_empty(),
        "resolved_params should be empty on infeasible solve, got {:?}",
        result.resolved_params
    );

    // diagnostics should contain the infeasibility message
    assert!(
        !result.diagnostics.is_empty(),
        "diagnostics should be non-empty on infeasible solve"
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("infeasible")),
        "diagnostics should contain infeasibility message, got {:?}",
        result.diagnostics
    );
}

/// After a concurrent edit that violates a constraint, `edit_check_concurrent`
/// should return a CheckResult with the constraint marked as Violated.
///
/// Module: param `width` (default mm(10.0)), constraint `width > mm(5.0)`.
/// Cold check → Satisfied. edit_check_concurrent(width, mm(2.0)) → Violated.
/// Also verifies values and empty resolved_params (no auto params).
#[tokio::test]
async fn edit_check_concurrent_reports_constraint_satisfaction() {
    use reify_runtime::concurrent_eval::edit_check_concurrent;
    use reify_test_support::builders::{gt, literal, value_ref};
    use reify_test_support::mm;
    use reify_ir::Satisfaction;

    let width_id = ValueCellId::new("S", "width");

    let template = TopologyTemplateBuilder::new("S")
        .param("S", "width", Type::length(), Some(literal(mm(10.0))))
        .constraint("S", 0, None, gt(value_ref("S", "width"), literal(mm(5.0))))
        .build();

    let module = build_module(template);
    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = Engine::new(Box::new(checker), None);

    // Cold check: width=mm(10.0) > mm(5.0) → Satisfied
    let cold_result = engine.check(&module);
    assert_eq!(
        cold_result.constraint_results[0].satisfaction,
        Satisfaction::Satisfied,
        "cold check should be Satisfied"
    );

    let cancel = CancellationToken::new();

    // Concurrent edit: width=mm(2.0) < mm(5.0) → Violated
    let check_result = edit_check_concurrent(&mut engine, width_id.clone(), mm(2.0), &cancel)
        .await
        .unwrap();

    assert_eq!(
        check_result.constraint_results[0].satisfaction,
        Satisfaction::Violated,
        "constraint should be Violated when width=mm(2.0) < mm(5.0)"
    );

    // values should reflect the new width
    let width_val = check_result
        .values
        .get(&width_id)
        .expect("values should contain width");
    assert!(
        matches!(width_val, Value::Scalar { si_value, .. } if (*si_value - 0.002).abs() < 1e-10),
        "expected width = mm(2.0) = 0.002 SI, got {:?}",
        width_val
    );

    // No auto params → resolved_params should be empty
    assert!(
        check_result.resolved_params.is_empty(),
        "resolved_params should be empty (no auto params)"
    );
}

/// Verify constraint transitions work correctly through the concurrent path
/// across multiple consecutive edits.
///
/// Module: param `width` (default mm(10.0)), constraint `width > mm(5.0)`.
/// Cold check → Satisfied. edit_check_concurrent(width, mm(2.0)) → Violated.
/// edit_check_concurrent(width, mm(8.0)) → Satisfied.
#[tokio::test]
async fn edit_check_concurrent_constraint_transitions() {
    use reify_runtime::concurrent_eval::edit_check_concurrent;
    use reify_test_support::builders::{gt, literal, value_ref};
    use reify_test_support::mm;
    use reify_ir::Satisfaction;

    let width_id = ValueCellId::new("S", "width");

    let template = TopologyTemplateBuilder::new("S")
        .param("S", "width", Type::length(), Some(literal(mm(10.0))))
        .constraint("S", 0, None, gt(value_ref("S", "width"), literal(mm(5.0))))
        .build();

    let module = build_module(template);
    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = Engine::new(Box::new(checker), None);

    // Cold check: width=mm(10.0) > mm(5.0) → Satisfied
    let cold_result = engine.check(&module);
    assert_eq!(
        cold_result.constraint_results[0].satisfaction,
        Satisfaction::Satisfied,
        "cold check should be Satisfied"
    );

    let cancel = CancellationToken::new();

    // First concurrent edit: width=mm(2.0) < mm(5.0) → Violated
    let result1 = edit_check_concurrent(&mut engine, width_id.clone(), mm(2.0), &cancel)
        .await
        .unwrap();
    assert_eq!(
        result1.constraint_results[0].satisfaction,
        Satisfaction::Violated,
        "constraint should be Violated when width=mm(2.0) < mm(5.0)"
    );

    // Second concurrent edit: width=mm(8.0) > mm(5.0) → Satisfied
    let result2 = edit_check_concurrent(&mut engine, width_id.clone(), mm(8.0), &cancel)
        .await
        .unwrap();
    assert_eq!(
        result2.constraint_results[0].satisfaction,
        Satisfaction::Satisfied,
        "constraint should be Satisfied when width=mm(8.0) > mm(5.0)"
    );
}

/// End-to-end concurrent interactive loop: edit → resolution → let-binding
/// propagation → constraint checking, all in one interactive update.
///
/// Module: param `a` (default mm(3.0)), auto `x`, let `y = x * 2.0`,
/// constraint `x > a` (index 0), constraint `y < mm(100.0)` (index 1).
/// SequencedMockSolver: 1st call x=mm(5.0), 2nd call x=mm(20.0).
///
/// Cold check → x=mm(5.0), y=0.01 SI, both constraints satisfied.
/// edit_check_concurrent(a, mm(8.0)) → x re-resolved to mm(20.0), y=0.04 SI.
/// Assert: (1) resolved_params contains x→mm(20.0),
///         (2) values[y] = 0.04 SI,
///         (3) constraint `x > a` Satisfied,
///         (4) constraint `y < mm(100)` Satisfied.
#[tokio::test]
async fn edit_check_concurrent_with_resolution_and_constraints() {
    use reify_runtime::concurrent_eval::edit_check_concurrent;
    use reify_test_support::builders::{binop, gt, literal, lt, value_ref};
    use reify_test_support::mm;
    use reify_test_support::mocks::SequencedMockConstraintSolver;
    use reify_ir::{Satisfaction, SolveResult};

    let a_id = ValueCellId::new("S", "a");
    let x_id = ValueCellId::new("S", "x");
    let y_id = ValueCellId::new("S", "y");

    // Sequenced solver: first x=mm(5.0), second x=mm(20.0)
    let mut solved1 = HashMap::new();
    solved1.insert(x_id.clone(), mm(5.0));
    let mut solved2 = HashMap::new();
    solved2.insert(x_id.clone(), mm(20.0));

    let solver = SequencedMockConstraintSolver::new(vec![
        SolveResult::Solved {
            values: solved1,
            unique: true,
        },
        SolveResult::Solved {
            values: solved2,
            unique: true,
        },
    ]);

    let template = TopologyTemplateBuilder::new("S")
        .param("S", "a", Type::length(), Some(literal(mm(3.0))))
        .auto_param("S", "x", Type::length())
        .let_binding(
            "S",
            "y",
            Type::length(),
            binop(BinOp::Mul, value_ref("S", "x"), literal(Value::Real(2.0))),
        )
        // constraint 0: x > a
        .constraint("S", 0, None, gt(value_ref("S", "x"), value_ref("S", "a")))
        // constraint 1: y < mm(100.0)
        .constraint("S", 1, None, lt(value_ref("S", "y"), literal(mm(100.0))))
        .build();

    let module = build_module(template);
    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = Engine::new(Box::new(checker), None).with_solver(Box::new(solver));

    // Cold check: x=mm(5.0), y=mm(10.0), both constraints satisfied
    let cold = engine.check(&module);
    assert_eq!(cold.constraint_results.len(), 2);
    assert_eq!(
        cold.constraint_results[0].satisfaction,
        Satisfaction::Satisfied
    );
    assert_eq!(
        cold.constraint_results[1].satisfaction,
        Satisfaction::Satisfied
    );

    let cancel = CancellationToken::new();

    // edit_check_concurrent: a → mm(8.0) → solver re-resolves x to mm(20.0)
    let result = edit_check_concurrent(&mut engine, a_id.clone(), mm(8.0), &cancel)
        .await
        .unwrap();

    // (1) resolved_params contains x→mm(20.0)
    assert!(
        !result.resolved_params.is_empty(),
        "resolved_params should contain re-resolved auto params"
    );
    let resolved_x = result
        .resolved_params
        .get(&x_id)
        .expect("resolved_params should contain x");
    assert!(
        matches!(resolved_x, Value::Scalar { si_value, .. } if (*si_value - 0.02).abs() < 1e-10),
        "expected resolved x = mm(20.0) = 0.02 SI, got {:?}",
        resolved_x
    );

    // (2) y re-evaluated from resolved x: mm(20.0) * 2.0 = 0.04 SI
    let y_val = result.values.get(&y_id).expect("values should contain y");
    assert!(
        matches!(y_val, Value::Scalar { si_value, .. } if (*si_value - 0.04).abs() < 1e-10),
        "expected y = 0.04 SI after re-resolution, got {:?}",
        y_val
    );

    // (3) constraint x > a: mm(20.0) > mm(8.0) → Satisfied
    assert_eq!(
        result.constraint_results[0].satisfaction,
        Satisfaction::Satisfied,
        "constraint x > a should be Satisfied after re-resolution"
    );

    // (4) constraint y < mm(100.0): mm(40.0) < mm(100.0) → Satisfied
    assert_eq!(
        result.constraint_results[1].satisfaction,
        Satisfaction::Satisfied,
        "constraint y < mm(100.0) should be Satisfied"
    );
}

/// Verify that constraint labels are preserved through the concurrent
/// constraint-checking path (edit_check_concurrent).
///
/// Module: param `width` (default mm(10.0)), constraint with label "min_width":
/// `width > mm(5.0)`. Cold check → label present. edit_check_concurrent(width,
/// mm(2.0)) → Violated, label must still be "min_width".
///
/// Should pass immediately since edit_check_concurrent routes through
/// check_constraints_with_values which was fixed to use cnode.label.clone().
#[tokio::test]
async fn edit_check_concurrent_preserves_constraint_labels() {
    use reify_runtime::concurrent_eval::edit_check_concurrent;
    use reify_test_support::builders::{gt, literal, value_ref};
    use reify_test_support::mm;
    use reify_core::ConstraintNodeId;
    use reify_ir::Satisfaction;

    let width_id = ValueCellId::new("S", "width");

    let template = TopologyTemplateBuilder::new("S")
        .param("S", "width", Type::length(), Some(literal(mm(10.0))))
        // Labeled constraint: "min_width", width > mm(5.0)
        .constraint(
            "S",
            0,
            Some("min_width"),
            gt(value_ref("S", "width"), literal(mm(5.0))),
        )
        .build();

    let module = build_module(template);
    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = Engine::new(Box::new(checker), None);

    // Cold check: width=mm(10.0) > mm(5.0) → Satisfied, label present
    let cold_result = engine.check(&module);
    assert_eq!(cold_result.constraint_results.len(), 1);
    assert_eq!(
        cold_result.constraint_results[0].label,
        Some("min_width".to_string()),
        "cold check: label should be 'min_width'"
    );
    assert_eq!(
        cold_result.constraint_results[0].satisfaction,
        Satisfaction::Satisfied,
    );

    let cancel = CancellationToken::new();

    // edit_check_concurrent: width=mm(2.0) → Violated, label preserved
    let result = edit_check_concurrent(&mut engine, width_id.clone(), mm(2.0), &cancel)
        .await
        .unwrap();

    assert_eq!(result.constraint_results.len(), 1);

    let c0 = &result.constraint_results[0];
    assert_eq!(c0.id, ConstraintNodeId::new("S", 0));

    // Label must be preserved through the concurrent path
    assert_eq!(
        c0.label,
        Some("min_width".to_string()),
        "edit_check_concurrent: label should be preserved as 'min_width'"
    );

    // Satisfaction must be Violated
    assert_eq!(
        c0.satisfaction,
        Satisfaction::Violated,
        "constraint should be Violated when width=mm(2.0) < mm(5.0)"
    );
}

/// MetaAccess expressions evaluate correctly through the concurrent path.
///
/// Module: template 'S' with meta {"grade": "A2"}, param `width` (default mm(10.0)),
/// let `grade_label` = if width > mm(15.0) then meta_access("S", "grade") else "",
/// constraint `grade_label == "A2"`.
///
/// `grade_label` depends on both `width` (ValueRef) and meta_access. Editing
/// `width` places `grade_label` in the dirty cone, so the concurrent evaluator
/// must re-evaluate it — exercising MetaAccess through the concurrent path and
/// requiring meta_map to be present in the concurrent EvalContext.
///
/// Cold check at width=mm(10.0): grade_label="" → constraint Violated.
/// edit_check_concurrent(width, mm(20.0)):
///   - grade_label re-evaluates via meta_access to "A2"
///   - constraint grade_label == "A2" flips to Satisfied
#[tokio::test]
async fn edit_check_concurrent_with_meta_access() {
    use reify_runtime::concurrent_eval::edit_check_concurrent;
    use reify_test_support::builders::{conditional_expr, eq, gt, literal, value_ref};
    use reify_test_support::mm;
    use reify_ir::{CompiledExpr, Satisfaction};

    let width_id = ValueCellId::new("S", "width");
    let grade_label_id = ValueCellId::new("S", "grade_label");

    // grade_label = if width > mm(15.0) then meta_access("S", "grade") else ""
    let width_ref = value_ref("S", "width");
    let threshold = literal(mm(15.0));
    let cond = gt(width_ref, threshold);
    let meta_expr = CompiledExpr::meta_access("S".to_string(), "grade".to_string());
    let empty_str = CompiledExpr::literal(Value::String(String::new()), Type::String);
    let grade_label_expr = conditional_expr(cond, meta_expr, empty_str);

    // constraint: grade_label == "A2"
    let grade_label_ref = value_ref("S", "grade_label");
    let a2_literal = CompiledExpr::literal(Value::String("A2".to_string()), Type::String);
    let constraint_expr = eq(grade_label_ref, a2_literal);

    let template = TopologyTemplateBuilder::new("S")
        .meta(
            [("grade".to_string(), "A2".to_string())]
                .into_iter()
                .collect(),
        )
        .param("S", "width", Type::length(), Some(literal(mm(10.0))))
        .let_binding("S", "grade_label", Type::String, grade_label_expr)
        .constraint("S", 0, None, constraint_expr)
        .build();

    let module = build_module(template);
    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = Engine::new(Box::new(checker), None);

    // Cold check at width=mm(10.0): 10.0 !> 15.0 → grade_label == "" → Violated
    let cold_result = engine.check(&module);
    assert_eq!(
        cold_result.values.get(&grade_label_id),
        Some(&Value::String(String::new())),
        "cold check: grade_label should be empty when width <= threshold"
    );
    assert_eq!(
        cold_result.constraint_results[0].satisfaction,
        Satisfaction::Violated,
        "cold check: grade_label == 'A2' should be Violated"
    );

    let cancel = CancellationToken::new();

    // Concurrent edit: width=mm(20.0) places grade_label in the dirty cone.
    // The concurrent evaluator must take the then-branch (meta_access) and
    // resolve it against meta_map to produce "A2".
    let check_result = edit_check_concurrent(&mut engine, width_id.clone(), mm(20.0), &cancel)
        .await
        .unwrap();

    // grade_label was re-evaluated by the concurrent path: meta_access → "A2"
    assert_eq!(
        check_result.values.get(&grade_label_id),
        Some(&Value::String("A2".to_string())),
        "grade_label should be re-evaluated to 'A2' via meta_access through concurrent path"
    );

    // Constraint flips to Satisfied
    assert_eq!(
        check_result.constraint_results[0].satisfaction,
        Satisfaction::Satisfied,
        "grade_label == 'A2' must become Satisfied after concurrent width edit"
    );
}

// --- Poison recovery tests ---
//
// These tests verify that poisoned locks are recovered gracefully via
// unwrap_or_else(|e| e.into_inner()) + tracing::warn!, preventing cascading
// panics when one evaluation task panics mid-computation.
//
// The original C4 design called for panic-on-poison (propagating PoisonError),
// but this was revised in favor of graceful recovery because in concurrent
// evaluation, one panicking task would cascade to all tasks sharing the adapter
// via poisoned locks, taking down the entire evaluation batch instead of just
// the faulting node.

#[cfg(feature = "test-utils")]
use reify_test_support::warn_capturing_subscriber;

/// Runs `action` under a `warn_capturing_subscriber`, asserts that it does not
/// panic, that exactly `expected_warns` WARN events were emitted, and that at
/// least one event contains every `(key, value)` pair in `expected_fields` AND
/// an `"error"` key on that same event.
/// Returns the value produced by `action` for downstream assertions.
///
/// # Contract
///
/// In addition to the count and field-value checks, this helper enforces a
/// **co-location invariant**: at least one warn event must carry BOTH the
/// canonical message `poison_fields::MSG_LOCK_POISONED` AND the structured
/// `error` field on the *same* event.  This prevents a future refactor from
/// accidentally emitting the message on one event and the error field on a
/// separate unrelated warn — a split that would silently pass count/field-value
/// checks while breaking the structured-error contract.
///
/// # Coverage scope
///
/// **Covered (9):** the sync-accessible poison-recovery `tracing::warn!` sites in
/// [`ConcurrentEvalAdapter`]:
///
/// * 3 helper-method read/exclusive sites (`values/read`, `snapshot_values/read`,
///   `results/exclusive`) emitting `lock` + `access` + `error` fields.
/// * 6 `into_result` / `build_result_shared` sites emitting `lock` + `path` + `error` fields.
///
/// **Not covered (2):** `write_values` (`access=write`) and `write_snapshot_values`
/// (`access=write`) are only reachable through `async evaluate()`.  Because
/// `evaluate()` cannot be wrapped in the sync `catch_unwind` pattern this helper
/// uses, those sites are covered separately by the `poison_evaluate` module tests
/// (`evaluate_emits_access_write_for_poisoned_values` and
/// `evaluate_emits_access_write_for_poisoned_snapshot_values`).
///
/// # Panics
///
/// Panics if `action` panics (i.e., `catch_unwind` returns `Err`), or if the
/// WARN count, field-value, co-location, or error-key checks fail.
#[cfg(feature = "test-utils")]
fn assert_poison_recovers<T: Send + 'static>(
    action: impl FnOnce() -> T + std::panic::UnwindSafe,
    expected_warns: usize,
    expected_fields: &[(&str, &str)],
) -> T {
    use std::panic::catch_unwind;
    let (subscriber, capture) = warn_capturing_subscriber();
    let result = tracing::subscriber::with_default(subscriber, || catch_unwind(action));
    assert!(
        result.is_ok(),
        "action panicked when poison recovery was expected — catch_unwind returned Err"
    );
    capture.assert_count(expected_warns);
    // Validates exact field VALUES (e.g. lock="values", access="read") for the
    // structured-field schema contract.
    capture.assert_any_event_has_fields(expected_fields);
    // Co-location (fields): the same event that satisfies expected_fields must
    // also carry the "error" key.  All 9 poison-recovery warn sites emit
    // `error = %e`; if the error field appears on a different event (or is
    // absent entirely) this check catches the regression.
    let all_fields = capture.fields_by_event();
    let error_colocated = all_fields.iter().any(|event_fields| {
        expected_fields
            .iter()
            .all(|(k, v)| event_fields.get(*k).map(|s| s.as_str()) == Some(*v))
            && event_fields.contains_key("error")
    });
    assert!(
        error_colocated,
        "no single WARN event had all expected fields AND an \"error\" key;\n  \
         expected_fields: {expected_fields:?}\n  \
         fields_by_event: {all_fields:?}"
    );

    // Co-location (message): MSG_LOCK_POISONED and the `error` field must
    // appear on the same event.  The two parallel vecs are always equal-length
    // (WarnCapturingSubscriber pushes to both in the same event() call); the
    // length assertion is a safety-net for that internal invariant.
    let msgs = capture.messages();
    assert_eq!(
        msgs.len(),
        all_fields.len(),
        "messages and fields_by_event must have equal length (internal invariant of WarnCapture)"
    );
    let has_msg_colocation = msgs.iter().zip(all_fields.iter()).any(|(msg, fields)| {
        msg == poison_fields::MSG_LOCK_POISONED && fields.contains_key("error")
    });
    assert!(
        has_msg_colocation,
        "expected at least one warn event with BOTH message == {:?} AND field 'error'; \
         messages: {:?}, fields: {:?}",
        poison_fields::MSG_LOCK_POISONED,
        msgs,
        all_fields,
    );

    result.unwrap()
}

#[cfg(feature = "test-utils")]
mod poison_recovery {
    use super::*;

    /// values() recovers gracefully from a poisoned values RwLock: no panic,
    /// returned slice contains both T.a and T.b with correct values, and exactly
    /// one tracing::warn! is emitted with the "values RwLock poisoned" message.
    #[test]
    fn values_recovers_from_poisoned_values_lock_with_warn() {
        let setup = simple_setup();
        let adapter = ConcurrentEvalAdapter::from_setup(&setup);

        adapter.poison_values();

        // values() acquires 1 lock: values RwLock. Only that lock is poisoned,
        // so exactly 1 WARN fires with structured fields naming the lock.
        let values = assert_poison_recovers(
            || adapter.values(),
            1,
            &[
                ("lock", poison_fields::LOCK_VALUES),
                ("access", poison_fields::ACCESS_READ),
            ],
        );
        // Verify exact values from simple_setup: T.a=Real(10.0), T.b=Real(10.0)
        assert_eq!(
            values.get(&ValueCellId::new("T", "a")),
            Some(&Value::Real(10.0)),
            "T.a should be Real(10.0) after values() poison recovery"
        );
        assert_eq!(
            values.get(&ValueCellId::new("T", "b")),
            Some(&Value::Real(10.0)),
            "T.b should be Real(10.0) after values() poison recovery"
        );
    }

    /// take_results() recovers gracefully from a poisoned results Mutex: no panic,
    /// returned results are empty, and exactly one tracing::warn! is emitted with
    /// the "results Mutex poisoned" message.
    #[test]
    fn take_results_recovers_from_poisoned_results_lock_with_warn() {
        let setup = simple_setup();
        let adapter = ConcurrentEvalAdapter::from_setup(&setup);

        adapter.poison_results();

        // take_results() acquires 1 lock: results Mutex. Only that lock is poisoned,
        // so exactly 1 WARN fires with structured fields naming the lock.
        let results = assert_poison_recovers(
            || adapter.take_results(),
            1,
            &[
                ("lock", poison_fields::LOCK_RESULTS),
                ("access", poison_fields::ACCESS_EXCLUSIVE),
            ],
        );
        assert_eq!(
            results.len(),
            0,
            "results should be empty after poison recovery"
        );
    }

    /// snapshot_values() recovers gracefully from a poisoned snapshot_values RwLock:
    /// no panic, returned map contains both T.a and T.b with correct (Value, DeterminacyState)
    /// tuples, and exactly one tracing::warn! is emitted with the "snapshot_values RwLock
    /// poisoned" message.
    #[test]
    fn snapshot_values_recovers_from_poisoned_lock_with_warn() {
        let setup = simple_setup();
        let adapter = ConcurrentEvalAdapter::from_setup(&setup);

        adapter.poison_snapshot_values();

        // snapshot_values() acquires 1 lock: snapshot_values RwLock. Only that lock is
        // poisoned, so exactly 1 WARN fires with structured fields naming the lock.
        let sv = assert_poison_recovers(
            || adapter.snapshot_values(),
            1,
            &[
                ("lock", poison_fields::LOCK_SNAPSHOT_VALUES),
                ("access", poison_fields::ACCESS_READ),
            ],
        );
        // Verify exact (Value, DeterminacyState) tuples from simple_setup
        assert_eq!(
            sv.get(&ValueCellId::new("T", "a")),
            Some(&(Value::Real(10.0), DeterminacyState::Determined)),
            "T.a snapshot should be (Real(10.0), Determined) after poison recovery"
        );
        assert_eq!(
            sv.get(&ValueCellId::new("T", "b")),
            Some(&(Value::Real(10.0), DeterminacyState::Determined)),
            "T.b snapshot should be (Real(10.0), Determined) after poison recovery"
        );
    }

    /// Verify that tracing::warn! is emitted when build_result_shared() recovers from
    /// a poisoned snapshot_values lock.  Also asserts the action succeeds (is_ok),
    /// fixing the previously discarded `_result`.
    #[test]
    fn tracing_warn_emitted_on_poison_snapshot_values_read() {
        let setup = simple_setup();
        let adapter = ConcurrentEvalAdapter::from_setup(&setup);
        let eval_set = vec![NodeId::Value(ValueCellId::new("T", "b"))];

        adapter.poison_snapshot_values();

        // Only snapshot_values is poisoned; values and results locks are healthy.
        // build_result_shared() acquires all three (values RwLock, snapshot_values RwLock,
        // results Mutex), so exactly 1 of 3 lock acquisitions triggers a recovery WARN.
        assert_poison_recovers(
            || adapter.build_result_shared(&eval_set, HashSet::new()),
            1,
            &[
                ("lock", poison_fields::LOCK_SNAPSHOT_VALUES),
                ("access", poison_fields::ACCESS_READ),
            ],
        );
    }

    /// Co-location contract: the canonical message `MSG_LOCK_POISONED` and the
    /// structured `error` field must appear on the **same** warn event.
    ///
    /// This standalone test captures events directly (without going through
    /// `assert_poison_recovers`) so the co-location invariant is explicit and not
    /// hidden behind helper abstraction.  It uses `values()` after `poison_values()`
    /// as the simplest single-warn exercise path.
    #[test]
    fn error_field_colocated_with_canonical_message() {
        let setup = simple_setup();
        let adapter = ConcurrentEvalAdapter::from_setup(&setup);
        adapter.poison_values();

        let (subscriber, capture) = warn_capturing_subscriber();
        let _values = tracing::subscriber::with_default(subscriber, || adapter.values());

        let msgs = capture.messages();
        let fbe = capture.fields_by_event();

        // Safety invariant: WarnCapturingSubscriber pushes to both vecs in the same
        // event() call, so they are always equal-length.
        assert_eq!(
            msgs.len(),
            fbe.len(),
            "messages and fields_by_event must have equal length (internal invariant of WarnCapture)"
        );

        // At least one warn event must have BOTH the canonical message AND the
        // structured error field — they must be co-located on the same event.
        let has_colocation = msgs.iter().zip(fbe.iter()).any(|(msg, fields)| {
            msg == poison_fields::MSG_LOCK_POISONED && fields.contains_key("error")
        });
        assert!(
            has_colocation,
            "expected at least one warn event with BOTH message == {:?} AND field 'error'; \
             messages: {:?}, fields: {:?}",
            poison_fields::MSG_LOCK_POISONED,
            msgs,
            fbe,
        );
    }
}

/// Critical test: 3+ parent mixed fan-in where downstream reads ONLY
/// intermediaries — NOT the changed param a directly.
///
/// Graph:
///   param a (Int, default 5)
///   let p1 = a * 2              (reads a, Changed: 10→20)
///   let p2 = if a > 0 then 1 else 1  (reads a, Unchanged: always 1)
///   let p3 = if a > 0 then 2 else 2  (reads a, Unchanged: always 2)
///   let d = p1 + p2 + p3        (reads ONLY p1, p2, p3 — NOT a directly)
///
/// Edit a: 5→10. d's dirtiness depends entirely on p1 being Changed
/// propagating through changed_vcids. d must be evaluated with value
/// 20 + 1 + 2 = 23.
#[tokio::test]
async fn three_plus_parent_mixed_fan_in_no_direct_param_read() {
    use reify_core::ContentHash;
    use reify_ir::{CompiledExpr, CompiledExprKind};

    let e = "T";

    // p1 = a * 2 (will change: 5*2=10 → 10*2=20)
    let p1_expr = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(ValueCellId::new(e, "a"), Type::Int),
        CompiledExpr::literal(Value::Int(2), Type::Int),
        Type::Int,
    );

    // p2 = if a > 0 then 1 else 1 (always 1 → Unchanged)
    let p2_cond = CompiledExpr::binop(
        BinOp::Gt,
        CompiledExpr::value_ref(ValueCellId::new(e, "a"), Type::Int),
        CompiledExpr::literal(Value::Int(0), Type::Int),
        Type::Bool,
    );
    let p2_expr = CompiledExpr {
        kind: CompiledExprKind::Conditional {
            condition: Box::new(p2_cond),
            then_branch: Box::new(CompiledExpr::literal(Value::Int(1), Type::Int)),
            else_branch: Box::new(CompiledExpr::literal(Value::Int(1), Type::Int)),
        },
        result_type: Type::Int,
        content_hash: ContentHash::of_str("if_a_gt_0_then_1_else_1"),
    };

    // p3 = if a > 0 then 2 else 2 (always 2 → Unchanged)
    let p3_cond = CompiledExpr::binop(
        BinOp::Gt,
        CompiledExpr::value_ref(ValueCellId::new(e, "a"), Type::Int),
        CompiledExpr::literal(Value::Int(0), Type::Int),
        Type::Bool,
    );
    let p3_expr = CompiledExpr {
        kind: CompiledExprKind::Conditional {
            condition: Box::new(p3_cond),
            then_branch: Box::new(CompiledExpr::literal(Value::Int(2), Type::Int)),
            else_branch: Box::new(CompiledExpr::literal(Value::Int(2), Type::Int)),
        },
        result_type: Type::Int,
        content_hash: ContentHash::of_str("if_a_gt_0_then_2_else_2"),
    };

    // d = (p1 + p2) + p3 — reads ONLY p1, p2, p3, NOT a
    let p1_plus_p2 = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::value_ref(ValueCellId::new(e, "p1"), Type::Int),
        CompiledExpr::value_ref(ValueCellId::new(e, "p2"), Type::Int),
        Type::Int,
    );
    let d_expr = CompiledExpr::binop(
        BinOp::Add,
        p1_plus_p2,
        CompiledExpr::value_ref(ValueCellId::new(e, "p3"), Type::Int),
        Type::Int,
    );

    let template = TopologyTemplateBuilder::new(e)
        .param(
            e,
            "a",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(5), Type::Int)),
        )
        .let_binding(e, "p1", Type::Int, p1_expr)
        .let_binding(e, "p2", Type::Int, p2_expr)
        .let_binding(e, "p3", Type::Int, p3_expr)
        .let_binding(e, "d", Type::Int, d_expr)
        .build();

    let module = build_module(template);
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let _initial = engine.eval(&module);

    let a_id = ValueCellId::new(e, "a");
    let cancel = CancellationToken::new();

    // Edit a: 5 → 10
    let (_setup, result) =
        edit_param_concurrent(&mut engine, a_id.clone(), Value::Int(10), &cancel)
            .await
            .unwrap();

    let d_node = NodeId::Value(ValueCellId::new(e, "d"));

    // d MUST be in actual_eval_set (not skipped) — its dirtiness depends
    // entirely on p1 being Changed, propagated via changed_vcids
    assert!(
        result.actual_eval_set.contains(&d_node),
        "d should be in actual_eval_set (p1 is Changed, making d dirty \
         even though p2 and p3 are Unchanged). actual_eval_set: {:?}",
        result.actual_eval_set
    );

    // d must have the correct value: p1=20, p2=1, p3=2 → d=23
    assert_eq!(
        result.values.get(&ValueCellId::new(e, "d")),
        Some(&Value::Int(23)),
        "d should be 20 + 1 + 2 = 23"
    );
}

/// Wide fan-in: 5 parents, only p1 Changed, others Unchanged.
///
/// Graph:
///   param a (Int, default 5)
///   let p1 = a * 2              (Changed: 10→20)
///   let p2 = if a>0 then 1 else 1  (Unchanged: always 1)
///   let p3 = if a>0 then 2 else 2  (Unchanged: always 2)
///   let p4 = if a>0 then 3 else 3  (Unchanged: always 3)
///   let p5 = if a>0 then 4 else 4  (Unchanged: always 4)
///   let d = ((p1+p2)+(p3+p4))+p5  (reads ONLY p1-p5, NOT a)
///
/// Edit a: 5→10. Assert d is in actual_eval_set with value 20+1+2+3+4=30.
#[tokio::test]
async fn five_parent_fan_in_one_changed() {
    use reify_core::ContentHash;
    use reify_ir::{CompiledExpr, CompiledExprKind};

    let e = "T";

    // p1 = a * 2 (will change)
    let p1_expr = CompiledExpr::binop(
        BinOp::Mul,
        CompiledExpr::value_ref(ValueCellId::new(e, "a"), Type::Int),
        CompiledExpr::literal(Value::Int(2), Type::Int),
        Type::Int,
    );

    // Helper to build conditional: if a > 0 then K else K
    let make_unchanged = |k: i64, label: &str| -> CompiledExpr {
        let cond = CompiledExpr::binop(
            BinOp::Gt,
            CompiledExpr::value_ref(ValueCellId::new(e, "a"), Type::Int),
            CompiledExpr::literal(Value::Int(0), Type::Int),
            Type::Bool,
        );
        CompiledExpr {
            kind: CompiledExprKind::Conditional {
                condition: Box::new(cond),
                then_branch: Box::new(CompiledExpr::literal(Value::Int(k), Type::Int)),
                else_branch: Box::new(CompiledExpr::literal(Value::Int(k), Type::Int)),
            },
            result_type: Type::Int,
            content_hash: ContentHash::of_str(label),
        }
    };

    let p2_expr = make_unchanged(1, "if_a_gt_0_then_1_else_1");
    let p3_expr = make_unchanged(2, "if_a_gt_0_then_2_else_2");
    let p4_expr = make_unchanged(3, "if_a_gt_0_then_3_else_3");
    let p5_expr = make_unchanged(4, "if_a_gt_0_then_4_else_4");

    // d = ((p1+p2) + (p3+p4)) + p5
    let p1_plus_p2 = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::value_ref(ValueCellId::new(e, "p1"), Type::Int),
        CompiledExpr::value_ref(ValueCellId::new(e, "p2"), Type::Int),
        Type::Int,
    );
    let p3_plus_p4 = CompiledExpr::binop(
        BinOp::Add,
        CompiledExpr::value_ref(ValueCellId::new(e, "p3"), Type::Int),
        CompiledExpr::value_ref(ValueCellId::new(e, "p4"), Type::Int),
        Type::Int,
    );
    let sum_4 = CompiledExpr::binop(BinOp::Add, p1_plus_p2, p3_plus_p4, Type::Int);
    let d_expr = CompiledExpr::binop(
        BinOp::Add,
        sum_4,
        CompiledExpr::value_ref(ValueCellId::new(e, "p5"), Type::Int),
        Type::Int,
    );

    let template = TopologyTemplateBuilder::new(e)
        .param(
            e,
            "a",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(5), Type::Int)),
        )
        .let_binding(e, "p1", Type::Int, p1_expr)
        .let_binding(e, "p2", Type::Int, p2_expr)
        .let_binding(e, "p3", Type::Int, p3_expr)
        .let_binding(e, "p4", Type::Int, p4_expr)
        .let_binding(e, "p5", Type::Int, p5_expr)
        .let_binding(e, "d", Type::Int, d_expr)
        .build();

    let module = build_module(template);
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let _initial = engine.eval(&module);

    let a_id = ValueCellId::new(e, "a");
    let cancel = CancellationToken::new();

    // Edit a: 5 → 10
    let (_setup, result) =
        edit_param_concurrent(&mut engine, a_id.clone(), Value::Int(10), &cancel)
            .await
            .unwrap();

    let d_node = NodeId::Value(ValueCellId::new(e, "d"));

    // d MUST be in actual_eval_set
    assert!(
        result.actual_eval_set.contains(&d_node),
        "d should be in actual_eval_set (p1 is Changed). actual_eval_set: {:?}",
        result.actual_eval_set
    );

    // d = 20 + 1 + 2 + 3 + 4 = 30
    assert_eq!(
        result.values.get(&ValueCellId::new(e, "d")),
        Some(&Value::Int(30)),
        "d should be 20 + 1 + 2 + 3 + 4 = 30"
    );
}

// --- Extended poison recovery tests (build_result_shared / into_result) ---
// These tests verify that poisoned locks are recovered gracefully in
// build_result_shared() and into_result(), extending the basic
// poison_recovery module's coverage of values()/take_results().
// Gated behind feature = "test-utils" because the poison_*() helpers
// are only available with that feature.

#[cfg(feature = "test-utils")]
mod poison_recovery_extended {
    use super::*;

    /// Poisons a lock via `$poison_method` (sole-owner path — no Arc guard held),
    /// then delegates to [`assert_poison_recovers`] calling `$action_method` on
    /// `into_result` / `build_result_shared`.  Returns the [`ConcurrentEditResult`]
    /// for optional downstream data assertions.
    macro_rules! poison_and_recover {
        ($poison_method:ident, $action_method:ident, $fields:expr) => {{
            let setup = simple_setup();
            let adapter = ConcurrentEvalAdapter::from_setup(&setup);
            let eval_set = vec![NodeId::Value(ValueCellId::new("T", "b"))];
            adapter.$poison_method();
            assert_poison_recovers(
                || adapter.$action_method(&eval_set, HashSet::new()),
                1,
                $fields,
            )
        }};
    }

    /// build_result_shared() recovers from poisoned values RwLock.
    /// Verifies exact values for T.a and T.b from simple_setup.
    #[test]
    fn build_result_shared_recovers_from_poisoned_values_lock() {
        let edit_result = poison_and_recover!(
            poison_values,
            build_result_shared,
            &[
                ("lock", poison_fields::LOCK_VALUES),
                ("access", poison_fields::ACCESS_READ)
            ]
        );
        // Verify both T.a and T.b are present with exact values from simple_setup
        assert_eq!(
            edit_result.values.get(&ValueCellId::new("T", "a")),
            Some(&Value::Real(10.0)),
            "T.a should be Real(10.0) after build_result_shared values poison recovery"
        );
        assert_eq!(
            edit_result.values.get(&ValueCellId::new("T", "b")),
            Some(&Value::Real(10.0)),
            "T.b should be Real(10.0) after build_result_shared values poison recovery"
        );
        assert_eq!(
            edit_result.values.len(),
            2,
            "values should have exactly T.a and T.b entries"
        );
    }

    /// build_result_shared() recovers from poisoned snapshot_values RwLock.
    /// Verifies exact (Value, DeterminacyState) tuples for T.a and T.b.
    #[test]
    fn build_result_shared_recovers_from_poisoned_snapshot_values_lock() {
        let edit_result = poison_and_recover!(
            poison_snapshot_values,
            build_result_shared,
            &[
                ("lock", poison_fields::LOCK_SNAPSHOT_VALUES),
                ("access", poison_fields::ACCESS_READ)
            ]
        );
        // Verify exact (Value, DeterminacyState) tuples from simple_setup
        assert_eq!(
            edit_result.snapshot_values.get(&ValueCellId::new("T", "a")),
            Some(&(Value::Real(10.0), DeterminacyState::Determined)),
            "T.a snapshot should be (Real(10.0), Determined) after poison recovery"
        );
        assert_eq!(
            edit_result.snapshot_values.get(&ValueCellId::new("T", "b")),
            Some(&(Value::Real(10.0), DeterminacyState::Determined)),
            "T.b snapshot should be (Real(10.0), Determined) after poison recovery"
        );
    }

    /// build_result_shared() recovers from poisoned results Mutex.
    /// node_results should be empty because no evaluations occurred.
    #[test]
    fn build_result_shared_recovers_from_poisoned_results_lock() {
        let edit_result = poison_and_recover!(
            poison_results,
            build_result_shared,
            &[
                ("lock", poison_fields::LOCK_RESULTS),
                ("access", poison_fields::ACCESS_EXCLUSIVE)
            ]
        );
        assert!(
            edit_result.node_results.is_empty(),
            "node_results should be empty (no evaluations occurred) after poison recovery"
        );
    }

    /// into_result() recovers from poisoned values RwLock.
    /// Verifies exact values for T.a and T.b from simple_setup.
    #[test]
    fn into_result_recovers_from_poisoned_values_lock() {
        let edit_result = poison_and_recover!(
            poison_values,
            into_result,
            &[
                ("lock", poison_fields::LOCK_VALUES),
                ("path", poison_fields::PATH_INTO_INNER)
            ]
        );
        assert_eq!(
            edit_result.values.get(&ValueCellId::new("T", "a")),
            Some(&Value::Real(10.0)),
            "T.a should be Real(10.0) after into_result values poison recovery"
        );
        assert_eq!(
            edit_result.values.get(&ValueCellId::new("T", "b")),
            Some(&Value::Real(10.0)),
            "T.b should be Real(10.0) after into_result values poison recovery"
        );
    }

    /// into_result() recovers from poisoned snapshot_values RwLock.
    /// Verifies exact (Value, DeterminacyState) tuples for T.a.
    #[test]
    fn into_result_recovers_from_poisoned_snapshot_values_lock() {
        let edit_result = poison_and_recover!(
            poison_snapshot_values,
            into_result,
            &[
                ("lock", poison_fields::LOCK_SNAPSHOT_VALUES),
                ("path", poison_fields::PATH_INTO_INNER)
            ]
        );
        assert_eq!(
            edit_result.snapshot_values.get(&ValueCellId::new("T", "a")),
            Some(&(Value::Real(10.0), DeterminacyState::Determined)),
            "T.a snapshot should be (Real(10.0), Determined) after into_result snapshot poison recovery"
        );
    }

    /// into_result() recovers from poisoned results Mutex.
    /// node_results should be empty because no evaluations occurred.
    #[test]
    fn into_result_recovers_from_poisoned_results_lock() {
        let edit_result = poison_and_recover!(
            poison_results,
            into_result,
            &[
                ("lock", poison_fields::LOCK_RESULTS),
                ("path", poison_fields::PATH_INTO_INNER)
            ]
        );
        assert!(
            edit_result.node_results.is_empty(),
            "node_results should be empty (no evaluations occurred) after poison recovery"
        );
    }
}

// Tests for into_result() recovering via the shared-fallback (Err(arc)) branch.
//
// These tests hold a second Arc clone alive when into_result() is called, so
// Arc::try_unwrap fails (refcount == 2) and execution falls into the
// Err(arc) → read()/lock() → unwrap_or_else path for each of the three locks.
//
// Each lock has two tests:
//   1. warn-counting — confirms tracing::warn! is emitted on poison recovery
//   2. recovery — confirms no panic and the returned data is usable

#[cfg(feature = "test-utils")]
mod poison_shared_fallback {
    use super::*;

    /// Sets up a shared-fallback scenario: creates the adapter, holds a second
    /// Arc for `$arc_method` (forcing `Arc::try_unwrap` to return `Err`), poisons
    /// the lock via `$poison_method`, then delegates to [`assert_poison_recovers`]
    /// with `into_result`.  Returns the [`ConcurrentEditResult`] for optional
    /// downstream data assertions.
    macro_rules! shared_fallback_recover {
        ($arc_method:ident, $poison_method:ident, $fields:expr) => {{
            let setup = simple_setup();
            let adapter = ConcurrentEvalAdapter::from_setup(&setup);
            let eval_set = vec![NodeId::Value(ValueCellId::new("T", "b"))];
            let _guard = adapter.$arc_method();
            adapter.$poison_method();
            assert_poison_recovers(
                || adapter.into_result(&eval_set, HashSet::new()),
                1,
                $fields,
            )
        }};
    }

    // -----------------------------------------------------------------------
    // values shared-fallback
    // -----------------------------------------------------------------------

    /// Verify that tracing::warn! is emitted when into_result() recovers from a
    /// poisoned values lock via the shared-fallback (Err(arc) → read()) path.
    ///
    /// A second Arc clone is held alive so Arc::try_unwrap returns Err,
    /// exercising the shared-fallback branch in concurrent_eval.rs.
    /// Event must have lock=values, path=shared_fallback.
    #[test]
    fn tracing_warn_emitted_on_poison_into_result_shared_fallback_values() {
        shared_fallback_recover!(
            values_arc,
            poison_values,
            &[
                ("lock", poison_fields::LOCK_VALUES),
                ("path", poison_fields::PATH_SHARED_FALLBACK)
            ]
        );
    }

    /// Verify that into_result() does not panic and returns usable data when
    /// recovering from a poisoned values lock via the shared-fallback path.
    /// Verifies T.a = Real(10.0) from simple_setup.
    #[test]
    fn into_result_shared_fallback_recovers_from_poisoned_values() {
        let edit_result = shared_fallback_recover!(
            values_arc,
            poison_values,
            &[
                ("lock", poison_fields::LOCK_VALUES),
                ("path", poison_fields::PATH_SHARED_FALLBACK)
            ]
        );
        assert_eq!(
            edit_result.values.get(&ValueCellId::new("T", "a")),
            Some(&Value::Real(10.0)),
            "T.a should be Real(10.0) after shared-fallback values poison recovery"
        );
    }

    // -----------------------------------------------------------------------
    // snapshot_values shared-fallback
    // -----------------------------------------------------------------------

    /// Verify that tracing::warn! is emitted when into_result() recovers from a
    /// poisoned snapshot_values lock via the shared-fallback (Err(arc) → read()) path.
    ///
    /// A second Arc clone is held alive so Arc::try_unwrap returns Err.
    /// Event must have lock=snapshot_values, path=shared_fallback.
    #[test]
    fn tracing_warn_emitted_on_poison_into_result_shared_fallback_snapshot_values() {
        shared_fallback_recover!(
            snapshot_values_arc,
            poison_snapshot_values,
            &[
                ("lock", poison_fields::LOCK_SNAPSHOT_VALUES),
                ("path", poison_fields::PATH_SHARED_FALLBACK)
            ]
        );
    }

    /// Verify that into_result() does not panic and returns usable data when
    /// recovering from a poisoned snapshot_values lock via the shared-fallback path.
    /// Verifies T.a and T.b tuples from simple_setup.
    #[test]
    fn into_result_shared_fallback_recovers_from_poisoned_snapshot_values() {
        let edit_result = shared_fallback_recover!(
            snapshot_values_arc,
            poison_snapshot_values,
            &[
                ("lock", poison_fields::LOCK_SNAPSHOT_VALUES),
                ("path", poison_fields::PATH_SHARED_FALLBACK)
            ]
        );
        assert_eq!(
            edit_result.snapshot_values.get(&ValueCellId::new("T", "a")),
            Some(&(Value::Real(10.0), DeterminacyState::Determined)),
            "T.a snapshot should be (Real(10.0), Determined) after shared-fallback poison recovery"
        );
        assert_eq!(
            edit_result.snapshot_values.get(&ValueCellId::new("T", "b")),
            Some(&(Value::Real(10.0), DeterminacyState::Determined)),
            "T.b snapshot should be (Real(10.0), Determined) after shared-fallback poison recovery"
        );
    }

    // -----------------------------------------------------------------------
    // results shared-fallback
    // -----------------------------------------------------------------------

    /// Verify that tracing::warn! is emitted when into_result() recovers from a
    /// poisoned results lock via the shared-fallback (Err(arc) → lock()) path.
    ///
    /// A second Arc clone is held alive so Arc::try_unwrap returns Err.
    /// Event must have lock=results, path=shared_fallback.
    #[test]
    fn tracing_warn_emitted_on_poison_into_result_shared_fallback_results() {
        shared_fallback_recover!(
            results_arc,
            poison_results,
            &[
                ("lock", poison_fields::LOCK_RESULTS),
                ("path", poison_fields::PATH_SHARED_FALLBACK)
            ]
        );
    }

    /// Verify that into_result() does not panic and returns usable data when
    /// recovering from a poisoned results lock via the shared-fallback path.
    /// node_results should be empty because no evaluations occurred.
    #[test]
    fn into_result_shared_fallback_recovers_from_poisoned_results() {
        let edit_result = shared_fallback_recover!(
            results_arc,
            poison_results,
            &[
                ("lock", poison_fields::LOCK_RESULTS),
                ("path", poison_fields::PATH_SHARED_FALLBACK)
            ]
        );
        assert!(
            edit_result.node_results.is_empty(),
            "node_results should be empty (no evaluations occurred) after shared-fallback poison recovery"
        );
    }

    // -----------------------------------------------------------------------
    // all-three shared-fallback: multi-lock simultaneous poison
    // -----------------------------------------------------------------------

    /// Verify that all three locks being poisoned simultaneously via the
    /// shared-fallback path recovers gracefully: exactly 3 WARN events are
    /// emitted with distinct messages, no panic occurs, and all result fields
    /// contain usable data.
    ///
    /// Holds all 3 Arc guards so that `Arc::try_unwrap` fails for each lock,
    /// forcing the shared-fallback (`Err(arc) → read()/lock()`) path for all three.
    #[test]
    fn all_three_locks_poisoned_shared_fallback_recovers_with_three_warns() {
        use std::panic::{AssertUnwindSafe, catch_unwind};
        let setup = simple_setup();
        let adapter = ConcurrentEvalAdapter::from_setup(&setup);
        let eval_set = vec![NodeId::Value(ValueCellId::new("T", "b"))];

        // Hold all 3 Arc guards — each Arc::try_unwrap will return Err, forcing
        // all three locks through the shared-fallback (Err(arc) → read()/lock()) path.
        let _values_guard = adapter.values_arc();
        let _snapshot_guard = adapter.snapshot_values_arc();
        let _results_guard = adapter.results_arc();

        // Poison all 3 locks via intentional panics in separate threads.
        adapter.poison_values();
        adapter.poison_snapshot_values();
        adapter.poison_results();

        let (subscriber, capture) = warn_capturing_subscriber();
        let result = tracing::subscriber::with_default(subscriber, || {
            catch_unwind(AssertUnwindSafe(|| {
                adapter.into_result(&eval_set, HashSet::new())
            }))
        });

        // (1) No panic — into_result() must recover from all 3 poisoned locks.
        assert!(
            result.is_ok(),
            "into_result() panicked when triple-lock poison recovery was expected"
        );
        let edit_result = result.unwrap();

        // (2) Exactly 3 WARN events — one per poisoned lock.
        capture.assert_count(3);

        // (3) All 3 distinct shared-fallback lock events present (by structured fields).
        capture.assert_any_event_has_fields(&[
            ("lock", poison_fields::LOCK_VALUES),
            ("path", poison_fields::PATH_SHARED_FALLBACK),
        ]);
        capture.assert_any_event_has_fields(&[
            ("lock", poison_fields::LOCK_SNAPSHOT_VALUES),
            ("path", poison_fields::PATH_SHARED_FALLBACK),
        ]);
        capture.assert_any_event_has_fields(&[
            ("lock", poison_fields::LOCK_RESULTS),
            ("path", poison_fields::PATH_SHARED_FALLBACK),
        ]);

        // (4) T.a = Real(10.0) from simple_setup.
        assert_eq!(
            edit_result.values.get(&ValueCellId::new("T", "a")),
            Some(&Value::Real(10.0)),
            "T.a should be Real(10.0) after triple shared-fallback poison recovery"
        );

        // (5) T.a snapshot = (Real(10.0), Determined) from simple_setup.
        assert_eq!(
            edit_result.snapshot_values.get(&ValueCellId::new("T", "a")),
            Some(&(Value::Real(10.0), DeterminacyState::Determined)),
            "T.a snapshot should be (Real(10.0), Determined) after triple shared-fallback poison recovery"
        );

        // (6) node_results is empty (no evaluations occurred).
        assert!(
            edit_result.node_results.is_empty(),
            "node_results should be empty (no evaluations occurred) \
             after triple shared-fallback poison recovery"
        );
    }
}

// Tests for evaluate() recovering from poisoned locks. The evaluate method
// acquires 4 locks: values read, values write, snapshot_values write, and
// results lock. These tests verify each lock site recovers gracefully.
//
// NOTE: evaluate_recovers_poisoned_values_write covers BOTH the read path
// (read_values(), which clones the current values map) and the write path
// (write_values(), which inserts the computed value) because poison_values()
// poisons the single RwLock shared by both operations.

#[cfg(feature = "test-utils")]
mod poison_evaluate {
    use super::*;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    /// Evaluates a node with explicit `catch_unwind` verification that no panic
    /// occurs. Uses `block_in_place` + `Handle::current().block_on()` to bridge
    /// the sync `catch_unwind` boundary with the async `evaluate()` method.
    fn evaluate_with_recovery(
        adapter: &ConcurrentEvalAdapter,
        node: NodeId,
    ) -> Result<EvalOutcome, Box<dyn std::any::Any + Send>> {
        catch_unwind(AssertUnwindSafe(|| {
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(adapter.evaluate(node))
            })
        }))
    }

    /// evaluate() recovers from poisoned values RwLock (both read and write paths).
    /// The write lock is the same RwLock as the read lock — this test verifies both:
    /// - the read path (read_values() clones the current values map for evaluation input)
    /// - the write path (write_values() inserts the computed result after evaluation)
    ///
    /// Both paths acquire the same RwLock, so poisoning via poison_values() exercises
    /// both recovery sites in a single test.
    ///
    /// If evaluate() panics on poison instead of recovering, this test will fail
    /// with the panic rather than completing.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn evaluate_recovers_poisoned_values_write() {
        let setup = simple_setup();
        let adapter = ConcurrentEvalAdapter::from_setup(&setup);
        let node = NodeId::Value(ValueCellId::new("T", "b"));

        // Poison the values lock — both read and write acquisitions must recover
        adapter.poison_values();

        let outcome = evaluate_with_recovery(&adapter, node)
            .expect("evaluate() should recover from poisoned values lock, not panic");
        assert_eq!(outcome, EvalOutcome::Changed);

        // Verify the value was written despite poisoning
        let values = adapter.values();
        assert_eq!(
            values.get(&ValueCellId::new("T", "b")),
            Some(&Value::Real(20.0))
        );
    }

    /// evaluate() recovers from poisoned snapshot_values RwLock.
    ///
    /// If evaluate() panics on poison instead of recovering, this test will fail
    /// with the panic rather than completing.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn evaluate_recovers_poisoned_snapshot_values() {
        let setup = simple_setup();
        let adapter = ConcurrentEvalAdapter::from_setup(&setup);
        let node = NodeId::Value(ValueCellId::new("T", "b"));

        // Poison only snapshot_values
        adapter.poison_snapshot_values();

        let outcome = evaluate_with_recovery(&adapter, node)
            .expect("evaluate() should recover from poisoned snapshot_values lock");
        assert_eq!(outcome, EvalOutcome::Changed);

        // Verify snapshot_values were actually written despite poisoning
        let snap = adapter.snapshot_values();
        assert!(
            snap.contains_key(&ValueCellId::new("T", "b")),
            "snapshot_values should contain T.b after poison recovery"
        );
        assert_eq!(
            snap.get(&ValueCellId::new("T", "b")),
            Some(&(Value::Real(20.0), DeterminacyState::Determined)),
            "T.b snapshot should be (20.0, Determined) after evaluate"
        );

        // Verify the primary values map was also written (write_values runs before
        // write_snapshot_values, so it should succeed even when only snapshot_values
        // was poisoned).
        let values = adapter.values();
        assert_eq!(
            values.get(&ValueCellId::new("T", "b")),
            Some(&Value::Real(20.0)),
            "T.b value should be 20.0 after evaluate (values lock was not poisoned)"
        );
    }

    /// evaluate() recovers from poisoned results Mutex.
    ///
    /// If evaluate() panics on poison instead of recovering, this test will fail
    /// with the panic rather than completing.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn evaluate_recovers_poisoned_results() {
        let setup = simple_setup();
        let adapter = ConcurrentEvalAdapter::from_setup(&setup);
        let node = NodeId::Value(ValueCellId::new("T", "b"));

        // Poison only results
        adapter.poison_results();

        let outcome = evaluate_with_recovery(&adapter, node)
            .expect("evaluate() should recover from poisoned results lock");
        assert_eq!(outcome, EvalOutcome::Changed);

        // Verify results were actually pushed despite poisoning
        let results = adapter.take_results();
        assert_eq!(
            results.len(),
            1,
            "evaluate() should push exactly one result"
        );
        assert_eq!(results[0].node, NodeId::Value(ValueCellId::new("T", "b")));
        assert_eq!(results[0].outcome, EvalOutcome::Changed);
        assert_eq!(
            results[0].value,
            Value::Real(20.0),
            "T.b should be a*2 = 10*2 = 20.0 after evaluate"
        );
        assert_eq!(
            results[0].determinacy,
            DeterminacyState::Determined,
            "T.b should be Determined after successful evaluate"
        );
    }

    /// Verify that tracing::warn! is emitted when evaluate() recovers from poisoned locks.
    /// evaluate() touches read_values, write_values, write_snapshot_values, and lock_results,
    /// so poisoning values should produce multiple WARN events.  At least one event must
    /// have lock=values to confirm the correct lock triggered the warning.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn tracing_warn_emitted_on_poison_evaluate() {
        let setup = simple_setup();
        let adapter = ConcurrentEvalAdapter::from_setup(&setup);
        let node = NodeId::Value(ValueCellId::new("T", "b"));

        // Poison values lock — affects both read and write paths in evaluate()
        adapter.poison_values();

        let (subscriber, capture) = warn_capturing_subscriber();
        let outcome = tracing::subscriber::with_default(subscriber, || {
            evaluate_with_recovery(&adapter, node)
        });
        let outcome =
            outcome.expect("evaluate() should recover from poisoned values lock, not panic");
        assert_eq!(outcome, EvalOutcome::Changed);

        let count = capture.count();
        assert!(
            count >= 2,
            "expected at least 2 WARN events (read_values + write_values recovery), got {count}"
        );
        // Verify at least one event names the correct lock AND access=write via structured fields
        capture.assert_any_event_has_fields(&[
            ("lock", poison_fields::LOCK_VALUES),
            ("access", poison_fields::ACCESS_WRITE),
        ]);
    }

    /// Verify that tracing::warn! is emitted when evaluate() recovers from a poisoned
    /// snapshot_values lock.  evaluate() calls write_snapshot_values after write_values,
    /// so poisoning only snapshot_values isolates the write_snapshot_values warn site.
    /// The values lock remains clean, so read_values and write_values succeed silently;
    /// exactly the snapshot_values write path fires a WARN event.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn tracing_warn_emitted_on_poison_evaluate_snapshot_values() {
        let setup = simple_setup();
        let adapter = ConcurrentEvalAdapter::from_setup(&setup);
        let node = NodeId::Value(ValueCellId::new("T", "b"));

        // Poison only snapshot_values — values lock stays clean
        adapter.poison_snapshot_values();

        let (subscriber, capture) = warn_capturing_subscriber();
        let outcome = tracing::subscriber::with_default(subscriber, || {
            evaluate_with_recovery(&adapter, node)
        });
        let outcome = outcome
            .expect("evaluate() should recover from poisoned snapshot_values lock, not panic");
        assert_eq!(outcome, EvalOutcome::Changed);

        let count = capture.count();
        assert!(
            count >= 1,
            "expected at least 1 WARN event (write_snapshot_values recovery), got {count}"
        );
        // Verify at least one event names lock=snapshot_values AND access=write
        capture.assert_any_event_has_fields(&[
            ("lock", poison_fields::LOCK_SNAPSHOT_VALUES),
            ("access", poison_fields::ACCESS_WRITE),
        ]);
    }

    /// evaluate() emits access=write for the write_values() warn site when
    /// the values RwLock is poisoned.
    ///
    /// Restores independent write-path coverage for write_values() that does not
    /// depend on the structured_field_emission module surviving future consolidation.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn evaluate_emits_access_write_for_poisoned_values() {
        let setup = simple_setup();
        let adapter = ConcurrentEvalAdapter::from_setup(&setup);
        let node = NodeId::Value(ValueCellId::new("T", "b"));

        adapter.poison_values();

        let (subscriber, capture) = warn_capturing_subscriber();
        tracing::subscriber::with_default(subscriber, || {
            evaluate_with_recovery(&adapter, node)
                .expect("evaluate() should recover from poisoned values lock");
        });

        capture.assert_any_event_has_fields(&[
            ("lock", poison_fields::LOCK_VALUES),
            ("access", poison_fields::ACCESS_WRITE),
        ]);
    }

    /// evaluate() emits access=write for the write_snapshot_values() warn site
    /// when the snapshot_values RwLock is poisoned.
    ///
    /// Restores independent write-path coverage for write_snapshot_values() that
    /// does not depend on the structured_field_emission module surviving future
    /// consolidation.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn evaluate_emits_access_write_for_poisoned_snapshot_values() {
        let setup = simple_setup();
        let adapter = ConcurrentEvalAdapter::from_setup(&setup);
        let node = NodeId::Value(ValueCellId::new("T", "b"));

        adapter.poison_snapshot_values();

        let (subscriber, capture) = warn_capturing_subscriber();
        tracing::subscriber::with_default(subscriber, || {
            evaluate_with_recovery(&adapter, node)
                .expect("evaluate() should recover from poisoned snapshot_values lock");
        });

        capture.assert_any_event_has_fields(&[
            ("lock", poison_fields::LOCK_SNAPSHOT_VALUES),
            ("access", poison_fields::ACCESS_WRITE),
        ]);
    }
}

mod execute_with_config_tests {
    //! Tests for execute_with_config: priority, commitment, overrides.
    use super::*;
    use reify_runtime::Priority;
    use reify_runtime::commitment::{
        CommitmentPolicy, CommitmentTracker, NodeCommitmentOverride, NodeKind, NodePolicyOverrides,
    };
    use reify_runtime::concurrent::{SchedulerConfig, SchedulerError};
    use reify_runtime::priority_promotion::SharedPriorityPromoter;
    use std::sync::Mutex;
    use std::time::Duration;

    /// Test helper: evaluator that returns EvalOutcome::Changed for all nodes.
    struct AllChangedAsync;
    impl AsyncNodeEvaluator for AllChangedAsync {
        async fn evaluate(&self, _node: NodeId) -> EvalOutcome {
            EvalOutcome::Changed
        }
    }

    /// Test that nodes within a level are spawned in priority order.
    /// Three nodes at the same level with priorities P0Interactive, P1Slow, P3Speculative.
    /// Uses TrackingAsyncEvaluator to record evaluation order.
    /// With #[tokio::test] (current_thread runtime), spawn order == eval order for
    /// synchronous evaluators.
    #[tokio::test]
    async fn test_priority_ordering_within_level() {
        // TrackingAsyncEvaluator: records NodeId on each evaluate call
        struct TrackingAsyncEvaluator {
            eval_order: Arc<Mutex<Vec<NodeId>>>,
        }
        impl AsyncNodeEvaluator for TrackingAsyncEvaluator {
            async fn evaluate(&self, node: NodeId) -> EvalOutcome {
                self.eval_order.lock().unwrap().push(node);
                EvalOutcome::Changed
            }
        }

        let e = "PRI";
        // Use names whose hash order differs from priority order.
        // "zz_high" (P0) should be evaluated first despite hashing differently than "aa_low" (P3).
        let node_p0 = NodeId::Value(ValueCellId::new(e, "zz_high"));
        let node_p1slow = NodeId::Value(ValueCellId::new(e, "mm_mid"));
        let node_p3 = NodeId::Value(ValueCellId::new(e, "aa_low"));

        // All at same level (no inter-dependencies, empty traces → dirty by default)
        let mut traces = HashMap::new();
        traces.insert(node_p0.clone(), DependencyTrace::default());
        traces.insert(node_p1slow.clone(), DependencyTrace::default());
        traces.insert(node_p3.clone(), DependencyTrace::default());

        let eval_order = Arc::new(Mutex::new(Vec::new()));
        let evaluator = Arc::new(TrackingAsyncEvaluator {
            eval_order: Arc::clone(&eval_order),
        });

        // Set up priorities
        let mut node_priorities = HashMap::new();
        node_priorities.insert(node_p0.clone(), Priority::P0Interactive);
        node_priorities.insert(node_p1slow.clone(), Priority::P1Slow);
        node_priorities.insert(node_p3.clone(), Priority::P3Speculative);

        let promoter = Arc::new(SharedPriorityPromoter::new());

        let config = SchedulerConfig {
            priority_promoter: Some(Arc::clone(&promoter)),
            node_priorities,
            ..SchedulerConfig::default()
        };

        let cancel = CancellationToken::new();
        let scheduler = ConcurrentScheduler;
        // Reverse order in eval_set to ensure sorting actually reorders
        let eval_set = vec![node_p3.clone(), node_p1slow.clone(), node_p0.clone()];

        let result = scheduler
            .execute_with_config(
                eval_set,
                evaluator,
                &traces,
                &cancel,
                &HashSet::new(),
                config,
            )
            .await
            .unwrap();

        assert_eq!(result.changed.len(), 3);

        // With priority sorting, spawn order should be: P0, P1Slow, P3
        let order = eval_order.lock().unwrap();
        assert_eq!(order.len(), 3);
        assert_eq!(order[0], node_p0, "P0Interactive should be evaluated first");
        assert_eq!(order[1], node_p1slow, "P1Slow should be evaluated second");
        assert_eq!(order[2], node_p3, "P3Speculative should be evaluated last");
    }

    /// Test that OnlyRunOnFinalInputs nodes with intermediate inputs are skipped.
    /// Two nodes at same level: node_a (default CommitIfSlow) and node_b
    /// (OnlyRunOnFinalInputs override). node_b's cache entry reads an upstream
    /// cell at Freshness::Intermediate → has_non_final_inputs returns true for node_b.
    /// node_b should be in result.skipped and NOT in result.changed.
    /// node_a is absent from the cache → has_non_final_inputs returns false
    /// (vacuously runnable), so node_a should be in result.changed.
    #[tokio::test]
    async fn test_only_run_on_final_inputs_skipped() {
        use reify_eval::cache::{CacheStore, CachedResult, NodeCache};
        use reify_core::VersionId;
        use reify_ir::{DeterminacyState, Freshness, Value};

        let e = "SKIP";
        let node_a = NodeId::Value(ValueCellId::new(e, "a"));
        let node_b = NodeId::Value(ValueCellId::new(e, "b"));
        let upstream = ValueCellId::new(e, "upstream");

        // Build CacheStore: upstream at Intermediate, node_b reads it.
        // node_a is absent → has_non_final_inputs returns false (vacuously runnable).
        let mut cs = CacheStore::new();
        cs.put(
            NodeId::Value(upstream.clone()),
            NodeCache::new(
                CachedResult::Value(Value::Real(0.0), DeterminacyState::Determined),
                Freshness::Intermediate { generation: 1 },
                DependencyTrace { realization_reads: Vec::new(), reads: vec![] },
                VersionId(1),
            ),
        );
        cs.put(
            node_b.clone(),
            NodeCache::new(
                CachedResult::Value(Value::Real(0.0), DeterminacyState::Determined),
                Freshness::Final,
                DependencyTrace { realization_reads: Vec::new(),
                    reads: vec![upstream.clone()],
                },
                VersionId(1),
            ),
        );

        // Both at same level, empty traces → dirty by default
        let mut traces = HashMap::new();
        traces.insert(node_a.clone(), DependencyTrace::default());
        traces.insert(node_b.clone(), DependencyTrace::default());

        // node_b has OnlyRunOnFinalInputs override
        let mut node_overrides = NodePolicyOverrides::new();
        node_overrides.set_instance(node_b.clone(), NodeCommitmentOverride::OnlyRunOnFinalInputs);

        let config = SchedulerConfig {
            node_overrides,
            cache: Some(&cs),
            ..SchedulerConfig::default()
        };

        let cancel = CancellationToken::new();
        let scheduler = ConcurrentScheduler;
        let evaluator = Arc::new(AllChangedAsync);
        let eval_set = vec![node_a.clone(), node_b.clone()];

        let result = scheduler
            .execute_with_config(
                eval_set,
                evaluator,
                &traces,
                &cancel,
                &HashSet::new(),
                config,
            )
            .await
            .unwrap();

        // node_b should be in skipped (OnlyRunOnFinalInputs with intermediate inputs)
        assert!(
            result.skipped.contains(&node_b),
            "node_b should be in skipped set (OnlyRunOnFinalInputs with intermediate inputs)"
        );
        assert!(
            !result.changed.contains(&node_b),
            "node_b should NOT be in changed set"
        );

        // node_a should be evaluated and in changed
        assert!(
            result.changed.contains(&node_a),
            "node_a should be in changed set (default CommitIfSlow)"
        );
    }

    /// Test that a committed node's result survives cancellation.
    /// Two nodes at same level. CommitmentPolicy with always_commit_after=10ms.
    /// fast_node takes <1ms and fires cancel; slow_node sleeps 50ms (accumulating
    /// elapsed > 10ms → committed). After execute_with_config:
    /// - slow_node IS in result.changed (committed, survived cancel)
    /// - fast_node NOT in result.changed (uncommitted, cancelled)
    #[tokio::test]
    async fn test_committed_node_survives_cancellation() {
        let e = "CMT";
        let fast_node = NodeId::Value(ValueCellId::new(e, "fast"));
        let slow_node = NodeId::Value(ValueCellId::new(e, "slow"));

        // Both at same level, empty traces → dirty by default
        let mut traces = HashMap::new();
        traces.insert(fast_node.clone(), DependencyTrace::default());
        traces.insert(slow_node.clone(), DependencyTrace::default());

        // Evaluator: fast_node fires cancel instantly, slow_node sleeps 50ms
        let cancel = CancellationToken::new();

        struct CommitmentTestEvaluator {
            cancel: CancellationToken,
            fast_node: NodeId,
        }
        impl AsyncNodeEvaluator for CommitmentTestEvaluator {
            async fn evaluate(&self, node: NodeId) -> EvalOutcome {
                if node == self.fast_node {
                    // Fast node: cancel immediately
                    self.cancel.cancel();
                    EvalOutcome::Changed
                } else {
                    // Slow node: sleep long enough to exceed always_commit_after
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    EvalOutcome::Changed
                }
            }
        }

        let evaluator = Arc::new(CommitmentTestEvaluator {
            cancel: cancel.clone(),
            fast_node: fast_node.clone(),
        });

        // CommitmentPolicy: commit after 10ms (slow_node's 50ms will exceed this)
        let policy = CommitmentPolicy {
            always_commit_after: Duration::from_millis(10),
            commit_when_proportion_done: 0.5,
        };
        let tracker = Arc::new(Mutex::new(CommitmentTracker::new(policy)));

        // Pin fast_node to P1Slow so B4 guard is skipped and the
        // uncommitted-cancellation logic is exercised (fast_node never commits).
        let mut node_priorities = HashMap::new();
        node_priorities.insert(fast_node.clone(), Priority::P1Slow);

        let config = SchedulerConfig {
            commitment_tracker: Some(Arc::clone(&tracker)),
            node_priorities,
            ..SchedulerConfig::default()
        };

        let scheduler = ConcurrentScheduler;
        let eval_set = vec![fast_node.clone(), slow_node.clone()];

        let result = scheduler
            .execute_with_config(
                eval_set,
                evaluator,
                &traces,
                &cancel,
                &HashSet::new(),
                config,
            )
            .await
            .unwrap();

        // Verify cancellation actually occurred (fast_node fires cancel.cancel())
        assert!(
            cancel.is_cancelled(),
            "cancel token should be fired by fast_node during evaluation"
        );

        // Exactly one node survives: slow_node (committed) — fast_node dropped (uncommitted)
        assert_eq!(
            result.changed.len(),
            1,
            "exactly one node should survive cancellation (the committed slow_node)"
        );
        // slow_node should survive cancellation because it's committed (elapsed > 10ms)
        assert!(
            result.changed.contains(&slow_node),
            "slow_node should be in changed (committed, survived cancel)"
        );
        // fast_node should be dropped because it's uncommitted when cancel fires
        assert!(
            !result.changed.contains(&fast_node),
            "fast_node should NOT be in changed (uncommitted, cancelled)"
        );
    }

    /// Test that uncommitted nodes in dirty cone are cancelled.
    /// Two nodes at same level. CommitmentPolicy with always_commit_after=5s (long).
    /// One node fires cancel during eval. Both are fast (<1ms, well below threshold).
    /// Assert neither node is in result.changed (both uncommitted when cancel fires).
    #[tokio::test]
    async fn test_uncommitted_in_dirty_cone_cancelled() {
        let e = "UNC";
        let node_a = NodeId::Value(ValueCellId::new(e, "a"));
        let node_b = NodeId::Value(ValueCellId::new(e, "b"));

        let mut traces = HashMap::new();
        traces.insert(node_a.clone(), DependencyTrace::default());
        traces.insert(node_b.clone(), DependencyTrace::default());

        let cancel = CancellationToken::new();

        // Evaluator: node_a fires cancel, both are fast
        struct CancellingEvaluator {
            cancel: CancellationToken,
            trigger_node: NodeId,
        }
        impl AsyncNodeEvaluator for CancellingEvaluator {
            async fn evaluate(&self, node: NodeId) -> EvalOutcome {
                if node == self.trigger_node {
                    self.cancel.cancel();
                }
                EvalOutcome::Changed
            }
        }

        let evaluator = Arc::new(CancellingEvaluator {
            cancel: cancel.clone(),
            trigger_node: node_a.clone(),
        });

        // CommitmentPolicy: 5s threshold — neither node will reach this
        let policy = CommitmentPolicy {
            always_commit_after: Duration::from_secs(5),
            commit_when_proportion_done: 0.99,
        };
        let tracker = Arc::new(Mutex::new(CommitmentTracker::new(policy)));

        // Pin both nodes to P1Slow so B4 guard is skipped and the
        // uncommitted-cancellation logic is exercised (neither node commits).
        let mut node_priorities = HashMap::new();
        node_priorities.insert(node_a.clone(), Priority::P1Slow);
        node_priorities.insert(node_b.clone(), Priority::P1Slow);

        let config = SchedulerConfig {
            commitment_tracker: Some(Arc::clone(&tracker)),
            node_priorities,
            ..SchedulerConfig::default()
        };

        let scheduler = ConcurrentScheduler;
        let eval_set = vec![node_a.clone(), node_b.clone()];

        let result = scheduler
            .execute_with_config(
                eval_set,
                evaluator,
                &traces,
                &cancel,
                &HashSet::new(),
                config,
            )
            .await
            .unwrap();

        // Both nodes are uncommitted (fast, below 5s threshold) and cancel is fired
        // → both should be dropped
        assert!(
            !result.changed.contains(&node_a),
            "node_a should NOT be in changed (uncommitted, cancelled)"
        );
        assert!(
            !result.changed.contains(&node_b),
            "node_b should NOT be in changed (uncommitted, cancelled)"
        );
    }

    /// Test that commitment tracker and priority promoter are cleaned up after execution.
    /// Two nodes, normal execution (no cancel). After execute_with_config completes,
    /// tracker.task_count() == 0 and promoter.count() == 0.
    #[tokio::test]
    async fn test_cleanup_on_completion() {
        let e = "CLN";
        let node_a = NodeId::Value(ValueCellId::new(e, "a"));
        let node_b = NodeId::Value(ValueCellId::new(e, "b"));

        let mut traces = HashMap::new();
        traces.insert(node_a.clone(), DependencyTrace::default());
        traces.insert(node_b.clone(), DependencyTrace::default());

        let tracker = Arc::new(Mutex::new(CommitmentTracker::new(
            CommitmentPolicy::default(),
        )));
        let promoter = Arc::new(SharedPriorityPromoter::new());

        let mut node_priorities = HashMap::new();
        node_priorities.insert(node_a.clone(), Priority::P0Interactive);
        node_priorities.insert(node_b.clone(), Priority::P1Slow);

        let config = SchedulerConfig {
            commitment_tracker: Some(Arc::clone(&tracker)),
            priority_promoter: Some(Arc::clone(&promoter)),
            node_priorities,
            ..SchedulerConfig::default()
        };

        let cancel = CancellationToken::new();
        let scheduler = ConcurrentScheduler;
        let evaluator = Arc::new(AllChangedAsync);
        let eval_set = vec![node_a.clone(), node_b.clone()];

        let result = scheduler
            .execute_with_config(
                eval_set,
                evaluator,
                &traces,
                &cancel,
                &HashSet::new(),
                config,
            )
            .await
            .unwrap();

        assert_eq!(result.changed.len(), 2);

        // After execution, all nodes should be cleaned up from tracker and promoter
        let tracker_count = tracker.lock().unwrap().task_count();
        assert_eq!(
            tracker_count, 0,
            "tracker should have 0 tasks after completion, got {tracker_count}"
        );
        let promoter_count = promoter.count();
        assert_eq!(
            promoter_count, 0,
            "promoter should have 0 nodes after completion, got {promoter_count}"
        );
    }

    /// Test that commitment tracker and priority promoter are cleaned up even when a task panics.
    /// The `return Err(TaskPanicked)` at line 353 bypasses the cleanup block, so this test
    /// should FAIL until the cleanup-on-error-path fix is implemented.
    #[tokio::test]
    async fn test_cleanup_on_task_panic() {
        let e = "PNC";
        let node_a = NodeId::Value(ValueCellId::new(e, "a"));
        let node_b = NodeId::Value(ValueCellId::new(e, "b"));

        let mut traces = HashMap::new();
        traces.insert(node_a.clone(), DependencyTrace::default());
        traces.insert(node_b.clone(), DependencyTrace::default());

        let tracker = Arc::new(Mutex::new(CommitmentTracker::new(
            CommitmentPolicy::default(),
        )));
        let promoter = Arc::new(SharedPriorityPromoter::new());

        let mut node_priorities = HashMap::new();
        node_priorities.insert(node_a.clone(), Priority::P0Interactive);
        node_priorities.insert(node_b.clone(), Priority::P1Slow);

        let config = SchedulerConfig {
            commitment_tracker: Some(Arc::clone(&tracker)),
            priority_promoter: Some(Arc::clone(&promoter)),
            node_priorities,
            ..SchedulerConfig::default()
        };

        let cancel = CancellationToken::new();
        let scheduler = ConcurrentScheduler;
        let evaluator = Arc::new(PanickingEvaluator);
        let eval_set = vec![node_a.clone(), node_b.clone()];

        let result = scheduler
            .execute_with_config(
                eval_set,
                evaluator,
                &traces,
                &cancel,
                &HashSet::new(),
                config,
            )
            .await;

        // Should return TaskPanicked error
        assert!(result.is_err(), "scheduler should return error on panic");
        match result.unwrap_err() {
            SchedulerError::TaskPanicked(_) => {} // expected
            other => panic!("Expected TaskPanicked, got {:?}", other),
        }

        // After error, all nodes should still be cleaned up from tracker and promoter
        let tracker_count = tracker.lock().unwrap().task_count();
        assert_eq!(
            tracker_count, 0,
            "tracker should have 0 tasks after panic, got {tracker_count}"
        );
        let promoter_count = promoter.count();
        assert_eq!(
            promoter_count, 0,
            "promoter should have 0 nodes after panic, got {promoter_count}"
        );
    }

    /// Test that commitment tracker and priority promoter are cleaned up on task cancellation.
    ///
    /// The `TaskCancelled` error path (`Err(_)` non-panic in `handle.await`) shares the same
    /// cleanup closure as `TaskPanicked` (both call `cleanup_level` before returning).
    /// Since triggering a true `JoinError::Cancelled` through `execute_with_config`'s public API
    /// requires externally aborting internally-held JoinHandles (not accessible), this test
    /// exercises the error-path cleanup through `execute_with_config` using a `PanickingEvaluator`
    /// (TaskPanicked path) with three nodes. The cleanup closure handles all dirty_nodes for the
    /// level, so verifying cleanup on the panic path also validates the cancellation path's
    /// identical cleanup behavior.
    ///
    /// This test FAILS because the early `return Err(...)` bypasses the cleanup block,
    /// leaving stale entries in both structures.
    #[tokio::test]
    async fn test_cleanup_on_task_cancelled() {
        let e = "CXL";
        let node_a = NodeId::Value(ValueCellId::new(e, "a"));
        let node_b = NodeId::Value(ValueCellId::new(e, "b"));
        let node_c = NodeId::Value(ValueCellId::new(e, "c"));

        let mut traces = HashMap::new();
        traces.insert(node_a.clone(), DependencyTrace::default());
        traces.insert(node_b.clone(), DependencyTrace::default());
        traces.insert(node_c.clone(), DependencyTrace::default());

        let tracker = Arc::new(Mutex::new(CommitmentTracker::new(
            CommitmentPolicy::default(),
        )));
        let promoter = Arc::new(SharedPriorityPromoter::new());

        let mut node_priorities = HashMap::new();
        node_priorities.insert(node_a.clone(), Priority::P0Interactive);
        node_priorities.insert(node_b.clone(), Priority::P1Slow);
        node_priorities.insert(node_c.clone(), Priority::P3Speculative);

        let config = SchedulerConfig {
            commitment_tracker: Some(Arc::clone(&tracker)),
            priority_promoter: Some(Arc::clone(&promoter)),
            node_priorities,
            ..SchedulerConfig::default()
        };

        let cancel = CancellationToken::new();
        let scheduler = ConcurrentScheduler;
        let evaluator = Arc::new(PanickingEvaluator);
        let eval_set = vec![node_a.clone(), node_b.clone(), node_c.clone()];

        let result = scheduler
            .execute_with_config(
                eval_set,
                evaluator,
                &traces,
                &cancel,
                &HashSet::new(),
                config,
            )
            .await;

        // Should return an error (TaskPanicked since that's what we can trigger)
        assert!(result.is_err(), "scheduler should return error on panic");
        match result.unwrap_err() {
            SchedulerError::TaskPanicked(_) => {} // expected — exercises same cleanup path as TaskCancelled
            other => panic!("Expected TaskPanicked, got {:?}", other),
        }

        // After error, ALL three nodes should be cleaned up from tracker and promoter.
        // This verifies cleanup_level handles the full dirty_nodes list, not just the
        // node that caused the error — same behavior needed for TaskCancelled path.
        let tracker_count = tracker.lock().unwrap().task_count();
        assert_eq!(
            tracker_count, 0,
            "tracker should have 0 tasks after error, got {tracker_count}"
        );
        let promoter_count = promoter.count();
        assert_eq!(
            promoter_count, 0,
            "promoter should have 0 nodes after error, got {promoter_count}"
        );
    }

    /// Test that node_overrides are correctly threaded from the dirty-check to
    /// the commitment tracker registration. Three nodes at same level:
    /// - trigger: fires cancel immediately (fast, < always_commit_after → cancelled)
    /// - node_a: default CommitIfSlow, sleeps 50ms (> 10ms always_commit_after → committed, survives cancel)
    /// - node_b: AlwaysCancelWhenStale override, sleeps 50ms (NeverCommit regardless of time → cancelled)
    ///
    /// This validates that the override looked up during the dirty/skip pre-computation
    /// is the same override used during commitment tracker registration — if they diverge,
    /// node_b could incorrectly commit (CommitIfSlow default) instead of being cancelled.
    #[tokio::test]
    async fn test_node_override_threaded_to_commitment_tracker() {
        let e = "THREAD";
        let trigger = NodeId::Value(ValueCellId::new(e, "trigger"));
        let node_a = NodeId::Value(ValueCellId::new(e, "a"));
        let node_b = NodeId::Value(ValueCellId::new(e, "b"));

        // All at same level, empty traces → dirty by default
        let mut traces = HashMap::new();
        traces.insert(trigger.clone(), DependencyTrace::default());
        traces.insert(node_a.clone(), DependencyTrace::default());
        traces.insert(node_b.clone(), DependencyTrace::default());

        let cancel = CancellationToken::new();

        struct OverrideThreadingEvaluator {
            cancel: CancellationToken,
            trigger: NodeId,
        }
        impl AsyncNodeEvaluator for OverrideThreadingEvaluator {
            async fn evaluate(&self, node: NodeId) -> EvalOutcome {
                if node == self.trigger {
                    // Trigger: fire cancel immediately
                    self.cancel.cancel();
                    EvalOutcome::Changed
                } else {
                    // Slow nodes: sleep long enough to exceed always_commit_after
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    EvalOutcome::Changed
                }
            }
        }

        let evaluator = Arc::new(OverrideThreadingEvaluator {
            cancel: cancel.clone(),
            trigger: trigger.clone(),
        });

        // CommitmentPolicy: commit after 10ms (slow nodes' 50ms will exceed this)
        let policy = CommitmentPolicy {
            always_commit_after: Duration::from_millis(10),
            commit_when_proportion_done: 0.5,
        };
        let tracker = Arc::new(Mutex::new(CommitmentTracker::new(policy)));

        // node_b has AlwaysCancelWhenStale override
        let mut node_overrides = NodePolicyOverrides::new();
        node_overrides.set_instance(
            node_b.clone(),
            NodeCommitmentOverride::AlwaysCancelWhenStale,
        );

        // Pin node_b to P1Slow so B4 guard is skipped and AlwaysCancelWhenStale
        // is exercised (node_b must still be cancelled by its override).
        let mut node_priorities = HashMap::new();
        node_priorities.insert(node_b.clone(), Priority::P1Slow);

        let config = SchedulerConfig {
            commitment_tracker: Some(Arc::clone(&tracker)),
            node_overrides,
            node_priorities,
            ..SchedulerConfig::default()
        };

        let scheduler = ConcurrentScheduler;
        let eval_set = vec![trigger.clone(), node_a.clone(), node_b.clone()];

        let result = scheduler
            .execute_with_config(
                eval_set,
                evaluator,
                &traces,
                &cancel,
                &HashSet::new(),
                config,
            )
            .await
            .unwrap();

        // node_a (CommitIfSlow default): elapsed 50ms > 10ms → committed → survives cancel
        assert!(
            result.changed.contains(&node_a),
            "node_a should be in changed (CommitIfSlow, elapsed > always_commit_after → committed)"
        );

        // node_b (AlwaysCancelWhenStale): NeverCommit regardless of elapsed time → cancelled
        assert!(
            !result.changed.contains(&node_b),
            "node_b should NOT be in changed (AlwaysCancelWhenStale → NeverCommit → cancelled)"
        );
    }

    /// Test that AlwaysCancelWhenStale drops result even with always_commit_after=0ms.
    /// Two nodes at same level: trigger fires cancel immediately, slow_node has
    /// AlwaysCancelWhenStale override and sleeps 50ms. CommitmentPolicy with
    /// always_commit_after=0ms means any elapsed time would normally auto-commit
    /// under CommitIfSlow — but AlwaysCancelWhenStale should override this and
    /// return NeverCommit, so slow_node should NOT appear in result.changed.
    #[tokio::test]
    async fn test_always_cancel_when_stale_drops_result() {
        let e = "ACWS";
        let trigger = NodeId::Value(ValueCellId::new(e, "trigger"));
        let slow_node = NodeId::Value(ValueCellId::new(e, "slow"));

        // Both at same level, empty traces → dirty by default
        let mut traces = HashMap::new();
        traces.insert(trigger.clone(), DependencyTrace::default());
        traces.insert(slow_node.clone(), DependencyTrace::default());

        let cancel = CancellationToken::new();

        struct AlwaysCancelEvaluator {
            cancel: CancellationToken,
            trigger: NodeId,
        }
        impl AsyncNodeEvaluator for AlwaysCancelEvaluator {
            async fn evaluate(&self, node: NodeId) -> EvalOutcome {
                if node == self.trigger {
                    // Trigger: fire cancel immediately
                    self.cancel.cancel();
                    EvalOutcome::Changed
                } else {
                    // Slow node: sleep to accumulate elapsed time past the 0ms threshold
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    EvalOutcome::Changed
                }
            }
        }

        let evaluator = Arc::new(AlwaysCancelEvaluator {
            cancel: cancel.clone(),
            trigger: trigger.clone(),
        });

        // CommitmentPolicy: always_commit_after=0ms — would normally auto-commit instantly
        let policy = CommitmentPolicy {
            always_commit_after: Duration::from_millis(0),
            commit_when_proportion_done: 0.5,
        };
        let tracker = Arc::new(Mutex::new(CommitmentTracker::new(policy)));

        // slow_node has AlwaysCancelWhenStale override
        let mut node_overrides = NodePolicyOverrides::new();
        node_overrides.set_instance(
            slow_node.clone(),
            NodeCommitmentOverride::AlwaysCancelWhenStale,
        );

        // Pin slow_node to P1Slow so B4 guard is skipped and AlwaysCancelWhenStale
        // is exercised (slow_node must be cancelled by its override despite 0ms threshold).
        let mut node_priorities = HashMap::new();
        node_priorities.insert(slow_node.clone(), Priority::P1Slow);

        let config = SchedulerConfig {
            commitment_tracker: Some(Arc::clone(&tracker)),
            node_overrides,
            node_priorities,
            ..SchedulerConfig::default()
        };

        let scheduler = ConcurrentScheduler;
        let eval_set = vec![trigger.clone(), slow_node.clone()];

        let result = scheduler
            .execute_with_config(
                eval_set,
                evaluator,
                &traces,
                &cancel,
                &HashSet::new(),
                config,
            )
            .await
            .unwrap();

        // Positive control: trigger node should have evaluated and committed
        assert!(
            result.changed.contains(&trigger),
            "trigger should be in changed (positive control: proves the scheduler actually ran)"
        );

        // slow_node (AlwaysCancelWhenStale): NeverCommit despite 0ms threshold → cancelled
        assert!(
            !result.changed.contains(&slow_node),
            "slow_node should NOT be in changed (AlwaysCancelWhenStale overrides 0ms always_commit_after)"
        );
        // AlwaysCancelWhenStale + NeverCommit: the node is dropped from both sets.
        // It is neither committed (changed) nor preemptively skipped — it ran but
        // its result was discarded due to cancellation, so it doesn't appear in
        // either set. This is distinct from OnlyRunOnFinalInputs which skips
        // evaluation entirely and places the node in `skipped`.
        assert!(
            !result.skipped.contains(&slow_node),
            "slow_node should NOT be in skipped (ran but result discarded, not pre-emptively skipped)"
        );
    }

    /// Test that OnlyRunOnFinalInputs runs normally when inputs are final.
    /// Two nodes at same level: node_a (default CommitIfSlow) and node_b
    /// (OnlyRunOnFinalInputs override). has_intermediate_inputs returns false
    /// for all nodes (all inputs are final). node_b should proceed normally:
    /// it should be in result.changed and NOT in result.skipped.
    #[tokio::test]
    async fn test_only_run_on_final_inputs_runs_when_final() {
        let e = "FINAL";
        let node_a = NodeId::Value(ValueCellId::new(e, "a"));
        let node_b = NodeId::Value(ValueCellId::new(e, "b"));

        // Both at same level, empty traces → dirty by default
        let mut traces = HashMap::new();
        traces.insert(node_a.clone(), DependencyTrace::default());
        traces.insert(node_b.clone(), DependencyTrace::default());

        // node_b has OnlyRunOnFinalInputs override
        let mut node_overrides = NodePolicyOverrides::new();
        node_overrides.set_instance(node_b.clone(), NodeCommitmentOverride::OnlyRunOnFinalInputs);

        // cache: None → has_non_final_inputs returns false for all nodes
        // (semantically identical to the old `Arc::new(|_| false)` closure)
        let config = SchedulerConfig {
            node_overrides,
            cache: None,
            ..SchedulerConfig::default()
        };

        let cancel = CancellationToken::new();
        let scheduler = ConcurrentScheduler;
        let evaluator = Arc::new(AllChangedAsync);
        let eval_set = vec![node_a.clone(), node_b.clone()];

        let result = scheduler
            .execute_with_config(
                eval_set,
                evaluator,
                &traces,
                &cancel,
                &HashSet::new(),
                config,
            )
            .await
            .unwrap();

        // node_b should be evaluated (not skipped) because inputs are final
        assert!(
            result.changed.contains(&node_b),
            "node_b should be in changed (OnlyRunOnFinalInputs with final inputs → runs normally)"
        );
        assert!(
            !result.skipped.contains(&node_b),
            "node_b should NOT be in skipped (inputs are final, not intermediate)"
        );

        // node_a should also be evaluated
        assert!(
            result.changed.contains(&node_a),
            "node_a should be in changed (default CommitIfSlow)"
        );
        assert!(
            !result.skipped.contains(&node_a),
            "node_a should NOT be in skipped (default CommitIfSlow, evaluated normally)"
        );
    }

    /// Regression-pin: OnlyRunOnFinalInputs node with a real CacheStore whose inputs
    /// are all-Final must be SPAWNED (not skipped), and must appear in `result.changed`.
    ///
    /// This test pins the wiring of the precomputed `has_non_final` flag at both former
    /// call sites in `execute_with_config` under the conditions where the refactor's
    /// invariant is observable:
    ///
    /// - Skip predicate: `if override_ == OnlyRunOnFinalInputs && has_non_final_flag`
    ///   must evaluate to `false` (all-Final → flag = false → NOT skipped).
    /// - Spawn loop: `has_intermediate` must be `false` (same flag, once computed).
    ///
    /// Complement: `test_only_run_on_final_inputs_skipped` covers the symmetric
    /// all-Intermediate case.  `test_only_run_on_final_inputs_runs_when_final` covers
    /// the same all-Final path but with `cache: None`.  This test is the only witness
    /// for `cache: Some(&cs)` + all-Final + OnlyRunOnFinalInputs.
    #[tokio::test]
    async fn test_only_run_on_final_inputs_runs_when_final_with_cache() {
        use reify_eval::cache::{CacheStore, CachedResult, NodeCache};
        use reify_core::VersionId;
        use reify_ir::{DeterminacyState, Freshness, Value};

        let e = "FINAL_CACHE";
        let node_a = NodeId::Value(ValueCellId::new(e, "a"));
        let node_b = NodeId::Value(ValueCellId::new(e, "b"));
        let upstream = ValueCellId::new(e, "upstream");

        // Build CacheStore: upstream at Final, node_b reads it (also Final).
        // node_a is absent → has_non_final_inputs returns false (vacuously runnable).
        let mut cs = CacheStore::new();
        cs.put(
            NodeId::Value(upstream.clone()),
            NodeCache::new(
                CachedResult::Value(Value::Real(0.0), DeterminacyState::Determined),
                Freshness::Final,
                DependencyTrace { realization_reads: Vec::new(), reads: vec![] },
                VersionId(1),
            ),
        );
        cs.put(
            node_b.clone(),
            NodeCache::new(
                CachedResult::Value(Value::Real(0.0), DeterminacyState::Determined),
                Freshness::Final,
                DependencyTrace { realization_reads: Vec::new(),
                    reads: vec![upstream.clone()],
                },
                VersionId(1),
            ),
        );

        // Both at same level, empty traces → dirty by default
        let mut traces = HashMap::new();
        traces.insert(node_a.clone(), DependencyTrace::default());
        traces.insert(node_b.clone(), DependencyTrace::default());

        // node_b has OnlyRunOnFinalInputs override
        let mut node_overrides = NodePolicyOverrides::new();
        node_overrides.set_instance(node_b.clone(), NodeCommitmentOverride::OnlyRunOnFinalInputs);

        // cache: Some(&cs) — has_non_final_inputs will be called against the real
        // CacheStore and must return false (all inputs are Final).
        let config = SchedulerConfig {
            node_overrides,
            cache: Some(&cs),
            ..SchedulerConfig::default()
        };

        let cancel = CancellationToken::new();
        let scheduler = ConcurrentScheduler;
        let evaluator = Arc::new(AllChangedAsync);
        let eval_set = vec![node_a.clone(), node_b.clone()];

        let result = scheduler
            .execute_with_config(
                eval_set,
                evaluator,
                &traces,
                &cancel,
                &HashSet::new(),
                config,
            )
            .await
            .unwrap();

        // node_b must be spawned (not skipped) because all inputs are Final.
        // This pins the skip predicate: flag = false → OnlyRunOnFinalInputs does not block.
        assert!(
            result.changed.contains(&node_b),
            "node_b should be in changed (OnlyRunOnFinalInputs with all-Final cache inputs → runs normally)"
        );

        // node_a should also be evaluated (default CommitIfSlow, absent from cache)
        assert!(
            result.changed.contains(&node_a),
            "node_a should be in changed (default CommitIfSlow)"
        );

        // Both nodes changed, none silently dropped (proves skip predicate did not fire
        // and both reached the spawn loop).
        assert_eq!(
            result.changed.len(),
            2,
            "both node_a and node_b should be in changed — no node silently dropped"
        );
    }

    /// Test that a type-level override in NodePolicyOverrides routes through resolve().
    ///
    /// Two Value nodes at the same level; `set_type(NodeKind::Value, OnlyRunOnFinalInputs)`
    /// is the only override — no per-instance entries. Both node_a and node_b have cache
    /// entries whose `dependency_trace.reads` reference an upstream cell at
    /// `Freshness::Intermediate`, so `has_non_final_inputs` returns `true` for both.
    /// Both nodes should appear in `result.skipped` because the type-level override is
    /// resolved by `NodePolicyOverrides::resolve` even without a per-instance entry —
    /// proving the scheduler honours type-level overrides after migration.
    #[tokio::test]
    async fn test_type_level_override_routes_through_resolve() {
        use reify_eval::cache::{CacheStore, CachedResult, NodeCache};
        use reify_core::VersionId;
        use reify_ir::{DeterminacyState, Freshness, Value};

        let e = "TYPE_OVERRIDE";
        let node_a = NodeId::Value(ValueCellId::new(e, "a"));
        let node_b = NodeId::Value(ValueCellId::new(e, "b"));
        let upstream = ValueCellId::new(e, "upstream");

        // Build CacheStore: upstream at Intermediate; both node_a and node_b read it.
        let mut cs = CacheStore::new();
        cs.put(
            NodeId::Value(upstream.clone()),
            NodeCache::new(
                CachedResult::Value(Value::Real(0.0), DeterminacyState::Determined),
                Freshness::Intermediate { generation: 1 },
                DependencyTrace { realization_reads: Vec::new(), reads: vec![] },
                VersionId(1),
            ),
        );
        for node in [node_a.clone(), node_b.clone()] {
            cs.put(
                node,
                NodeCache::new(
                    CachedResult::Value(Value::Real(0.0), DeterminacyState::Determined),
                    Freshness::Final,
                    DependencyTrace { realization_reads: Vec::new(),
                        reads: vec![upstream.clone()],
                    },
                    VersionId(1),
                ),
            );
        }

        // Both at same level, empty traces → dirty by default
        let mut traces = HashMap::new();
        traces.insert(node_a.clone(), DependencyTrace::default());
        traces.insert(node_b.clone(), DependencyTrace::default());

        // Set type-level override for all Value nodes — no instance overrides
        let mut node_overrides = NodePolicyOverrides::new();
        node_overrides.set_type(
            NodeKind::Value,
            NodeCommitmentOverride::OnlyRunOnFinalInputs,
        );

        let config = SchedulerConfig {
            node_overrides,
            cache: Some(&cs),
            ..SchedulerConfig::default()
        };

        let cancel = CancellationToken::new();
        let scheduler = ConcurrentScheduler;
        let evaluator = Arc::new(AllChangedAsync);
        let eval_set = vec![node_a.clone(), node_b.clone()];

        let result = scheduler
            .execute_with_config(
                eval_set,
                evaluator,
                &traces,
                &cancel,
                &HashSet::new(),
                config,
            )
            .await
            .unwrap();

        // Both Value nodes should be skipped by the type-level OnlyRunOnFinalInputs override
        assert!(
            result.skipped.contains(&node_a),
            "node_a should be in skipped (type-level OnlyRunOnFinalInputs with intermediate inputs)"
        );
        assert!(
            result.skipped.contains(&node_b),
            "node_b should be in skipped (type-level OnlyRunOnFinalInputs with intermediate inputs)"
        );
        assert!(
            !result.changed.contains(&node_a),
            "node_a should NOT be in changed set"
        );
        assert!(
            !result.changed.contains(&node_b),
            "node_b should NOT be in changed set"
        );
    }

    // -----------------------------------------------------------------------
    // T3: derived priority integration test (step-5 RED / step-6 GREEN)
    // -----------------------------------------------------------------------

    /// T3 (GR-038 γ / G2 signal): an unset NodeId::Compute is scheduled at
    /// P1Slow after `default_populate_priorities` is wired into
    /// `execute_with_config`.
    ///
    /// Three level-0 nodes (DependencyTrace::default, so all dirty):
    ///   anchor_fast   = NodeId::Value(ValueCellId::new("T3","fast"))   → explicit P1Fast
    ///   anchor_p3     = NodeId::Compute(ComputeNodeId::new("T3", 0))  → explicit P3Speculative
    ///   compute       = NodeId::Compute(ComputeNodeId::new("T3", 1))  → NO entry (must derive)
    ///
    /// Tie-break determinism:
    ///   PRE-wiring  : `compute` falls to unwrap_or(P3Speculative) and ties
    ///     with anchor_p3.  BTreeSet<DebugOrd> in compute_levels + stable
    ///     sort_by_key order idx-0 anchor_p3 before idx-1 compute, so
    ///     `compute_pos < anchor_p3_pos` is deterministically FALSE.
    ///   POST-wiring : `compute` derives P1Slow (WARM_STARTABLE|COMMITTABLE),
    ///     sits between anchor_fast (P1Fast) and anchor_p3 (explicit P3).
    ///     anchor_fast_pos < compute_pos < anchor_p3_pos is TRUE.
    ///
    /// The single assertion proves:
    ///   (a) The unset Compute derives P1Slow (not P3Speculative).
    ///   (b) The P1Slow value specifically (between P1Fast and P3).
    ///   (c) External-priority precedence: anchor_p3 is a Compute node yet
    ///       keeps its explicit P3 rather than deriving P1Slow.
    #[tokio::test]
    async fn t3_unset_compute_derives_p1slow_between_p1fast_and_p3() {
        use reify_core::{ComputeNodeId, ValueCellId};

        /// Tracks evaluation order by recording NodeId on spawn.
        struct OrderTrackingEvaluator {
            eval_order: Arc<Mutex<Vec<NodeId>>>,
        }
        impl AsyncNodeEvaluator for OrderTrackingEvaluator {
            // DETERMINISM NOTE: the order assertion below relies on each
            // spawned future completing on its *first* poll under the
            // current-thread (#[tokio::test]) runtime.  That holds because
            // there is NO `.await` point before the `eval_order.push(node)`
            // call, so the future is immediately ready.  If a future `.await`
            // is added before the push (or the runtime flavor changes to
            // multi-thread), the spawn-completion order may no longer match
            // the sort order and the assertion could become flaky.  Prefer
            // asserting on SharedPriorityPromoter-registered priorities
            // directly if you need to drop this assumption.
            async fn evaluate(&self, node: NodeId) -> EvalOutcome {
                self.eval_order.lock().unwrap().push(node);
                EvalOutcome::Changed
            }
        }

        let eval_order = Arc::new(Mutex::new(Vec::<NodeId>::new()));
        let evaluator = Arc::new(OrderTrackingEvaluator {
            eval_order: Arc::clone(&eval_order),
        });

        let anchor_fast = NodeId::Value(ValueCellId::new("T3", "fast"));
        // idx 0 < idx 1 → anchor_p3 sorts before compute in BTreeSet tie-break
        let anchor_p3 = NodeId::Compute(ComputeNodeId::new("T3", 0));
        let compute = NodeId::Compute(ComputeNodeId::new("T3", 1));

        // All three at level 0 (empty reads → dirty by default).
        let mut traces = HashMap::new();
        traces.insert(anchor_fast.clone(), DependencyTrace::default());
        traces.insert(anchor_p3.clone(), DependencyTrace::default());
        traces.insert(compute.clone(), DependencyTrace::default());

        // Only two explicit entries: anchor_fast=P1Fast, anchor_p3=P3Speculative.
        // `compute` has NO entry — must be derived by default_populate_priorities.
        let mut node_priorities = HashMap::new();
        node_priorities.insert(anchor_fast.clone(), Priority::P1Fast);
        node_priorities.insert(anchor_p3.clone(), Priority::P3Speculative);

        let promoter = Arc::new(SharedPriorityPromoter::new());
        let config = SchedulerConfig {
            priority_promoter: Some(Arc::clone(&promoter)),
            node_priorities,
            // node_traits left as default-empty; resolve() returns kind defaults.
            ..SchedulerConfig::default()
        };

        let cancel = CancellationToken::new();
        let scheduler = ConcurrentScheduler;
        // Scramble the eval_set order to prove sorting, not input order, matters.
        let eval_set = vec![compute.clone(), anchor_p3.clone(), anchor_fast.clone()];
        let changed_cells = HashSet::new();

        let result = scheduler
            .execute_with_config(eval_set, evaluator, &traces, &cancel, &changed_cells, config)
            .await
            .unwrap();

        assert_eq!(result.changed.len(), 3);

        let order = eval_order.lock().unwrap();
        let anchor_fast_pos = order.iter().position(|n| *n == anchor_fast).unwrap();
        let compute_pos = order.iter().position(|n| *n == compute).unwrap();
        let anchor_p3_pos = order.iter().position(|n| *n == anchor_p3).unwrap();

        assert!(
            anchor_fast_pos < compute_pos,
            "anchor_fast (explicit P1Fast) must be spawned before compute (derived P1Slow), \
             but anchor_fast_pos={anchor_fast_pos} compute_pos={compute_pos}"
        );
        assert!(
            compute_pos < anchor_p3_pos,
            "compute (derived P1Slow) must be spawned before anchor_p3 (explicit P3Speculative), \
             but compute_pos={compute_pos} anchor_p3_pos={anchor_p3_pos}"
        );
    }

    /// step-3 RED / step-4 GREEN: uncommitted P1Fast Value survives cancellation
    /// end-to-end (PRD §5 B4 / I-2).
    ///
    /// An uncommitted NodeId::Value with NO explicit node_priorities entry derives
    /// P1Fast via default_populate_priorities (B2, dep task 3570). The B4 guard
    /// in should_continue short-circuits cancellation for P1Fast nodes — so even
    /// though the node never commits, its result must survive.
    ///
    /// Under step-2's P1Slow placeholder the scheduler passes P1Slow to
    /// should_continue, the guard is skipped, and the uncommitted node is
    /// dropped → this assertion FAILS (behavioral RED). Step-4 wires dn.priority
    /// into the call, P1Fast reaches the guard, and the node survives (GREEN).
    ///
    /// Positive control: cancel is verified to have fired (proves the scheduler
    /// ran the cancellation path) and trigger appears in result.changed (proves
    /// evaluation actually happened).
    #[tokio::test]
    async fn test_immediate_value_survives_cancellation_end_to_end() {
        let e = "B4E2E";
        // trigger fires cancel; value_node is the uncommitted P1Fast node under test
        let trigger = NodeId::Value(ValueCellId::new(e, "trigger"));
        let value_node = NodeId::Value(ValueCellId::new(e, "value"));

        let mut traces = HashMap::new();
        traces.insert(trigger.clone(), DependencyTrace::default());
        traces.insert(value_node.clone(), DependencyTrace::default());

        let cancel = CancellationToken::new();

        // Determinism note: #[tokio::test] uses a single-threaded (current_thread) Tokio
        // runtime. `trigger` is listed first in `eval_set` and its `evaluate()` has no
        // `.await` (synchronous completion), so the runtime polls and completes `trigger`
        // — firing cancel — before `value_node` is polled. This guarantees that
        // `should_continue` is reached for `value_node` after cancel has already fired,
        // exercising the B4 guard on the hot path. A multi_thread runtime or an async
        // evaluator body (with an intermediate `.await`) would make the ordering racy.
        struct B4Evaluator {
            cancel: CancellationToken,
            trigger: NodeId,
        }
        impl AsyncNodeEvaluator for B4Evaluator {
            async fn evaluate(&self, node: NodeId) -> EvalOutcome {
                if node == self.trigger {
                    self.cancel.cancel();
                }
                EvalOutcome::Changed
            }
        }

        let evaluator = Arc::new(B4Evaluator {
            cancel: cancel.clone(),
            trigger: trigger.clone(),
        });

        // Long always_commit_after so value_node never commits before cancel fires.
        let policy = CommitmentPolicy {
            always_commit_after: Duration::from_secs(60),
            commit_when_proportion_done: 0.99,
        };
        let tracker = Arc::new(Mutex::new(CommitmentTracker::new(policy)));

        // No node_priorities entry for value_node → default_populate_priorities
        // derives IMMEDIATE (Value default_traits) → P1Fast at the scheduler.
        let config = SchedulerConfig {
            commitment_tracker: Some(Arc::clone(&tracker)),
            ..SchedulerConfig::default()
        };

        let scheduler = ConcurrentScheduler;
        let eval_set = vec![trigger.clone(), value_node.clone()];

        let result = scheduler
            .execute_with_config(
                eval_set,
                evaluator,
                &traces,
                &cancel,
                &HashSet::new(),
                config,
            )
            .await
            .unwrap();

        // Positive control: cancel actually fired.
        assert!(cancel.is_cancelled(), "cancel must fire (positive control)");

        // Positive control: trigger evaluated (proves scheduler ran).
        assert!(
            result.changed.contains(&trigger),
            "trigger must appear in changed (positive control: scheduler ran)"
        );

        // B4 assertion: P1Fast value_node survives despite being uncommitted.
        assert!(
            result.changed.contains(&value_node),
            "P1Fast value_node must survive cancellation (B4: never-cancel guard, PRD §5 B4 / I-2)"
        );
    }
} // mod execute_with_config_tests

/// Characterization: ConcurrentEvalAdapter returns Unchanged for a Value cell
/// whose `default_expr` is None.  Uses `.auto_param()` which produces
/// `kind = Auto { free: false }` and `default_expr = None`, exercising the
/// fallthrough branch that the step-2 refactor must preserve.
#[tokio::test]
async fn adapter_evaluate_returns_unchanged_for_cell_without_default_expr() {
    let e = "T";

    // Build template: param "a" (so the graph is non-empty) + auto-param "x"
    // (kind = Auto { free: false }, default_expr = None).
    let template = TopologyTemplateBuilder::new(e)
        .param(
            e,
            "a",
            Type::dimensionless_scalar(),
            Some(reify_ir::CompiledExpr::literal(
                Value::Real(1.0),
                Type::dimensionless_scalar(),
            )),
        )
        .auto_param(e, "x", Type::dimensionless_scalar())
        .build();

    let graph = EvaluationGraph::from_templates(&[template]);
    let reverse_index = ReverseDependencyIndex::build_from_graph(&graph);

    let mut values = ValueMap::new();
    values.insert(ValueCellId::new(e, "a"), Value::Real(1.0));

    let mut snapshot_values = PersistentMap::new();
    snapshot_values.insert(
        ValueCellId::new(e, "a"),
        (Value::Real(1.0), DeterminacyState::Determined),
    );

    let mut changed_cells = HashSet::new();
    changed_cells.insert(ValueCellId::new(e, "a"));

    let setup = ConcurrentEditSetup {
        eval_set: vec![NodeId::Value(ValueCellId::new(e, "x"))],
        graph,
        values,
        snapshot_values,
        traces: HashMap::new(),
        reverse_index,
        previous_hashes: HashMap::new(),
        version: VersionId(1),
        snapshot_id: SnapshotId(1),
        parent_snapshot_id: SnapshotId(0),
        changed_cells,
        functions: vec![].into(),
        meta_map: Arc::new(HashMap::new()),
        objective: None,
    };

    let adapter = ConcurrentEvalAdapter::from_setup(&setup);
    let x_node = NodeId::Value(ValueCellId::new(e, "x"));

    // Evaluate x: default_expr is None → must fall through and return Unchanged.
    let outcome = adapter.evaluate(x_node).await;

    assert_eq!(
        outcome,
        EvalOutcome::Unchanged,
        "auto-param with no default_expr must return Unchanged"
    );

    // No value must be written for x (fallthrough must not write).
    assert!(
        adapter.values().get(&ValueCellId::new(e, "x")).is_none(),
        "x must not appear in the values map when default_expr is None"
    );

    // No result entry must be pushed.
    assert!(
        adapter.take_results().is_empty(),
        "no result entry should be pushed for a cell without default_expr"
    );
}

/// Sibling characterization: ConcurrentEvalAdapter returns Changed for a Let
/// cell whose `default_expr` is `Some(literal)`.  This covers the positive
/// branch of the `if let Some(expr) = cell_node.default_expr.as_ref()` guard
/// introduced in step-2, complementing the None-default_expr test above.
///
/// The test deliberately uses no `previous_hashes` entry so that the adapter
/// always reports Changed (no prior hash → first evaluation path).
#[tokio::test]
async fn adapter_evaluate_returns_changed_for_let_cell_with_default_expr() {
    let e = "T";

    // A literal expression — no dependencies on other cells.
    let lit_expr = reify_ir::CompiledExpr::literal(Value::Int(42), Type::Int);

    let template = TopologyTemplateBuilder::new(e)
        .let_binding(e, "x", Type::Int, lit_expr)
        .build();

    let graph = EvaluationGraph::from_templates(&[template]);
    let reverse_index = ReverseDependencyIndex::build_from_graph(&graph);

    let x_id = ValueCellId::new(e, "x");
    let x_node = NodeId::Value(x_id.clone());

    let setup = ConcurrentEditSetup {
        eval_set: vec![x_node.clone()],
        graph,
        values: ValueMap::new(),
        snapshot_values: PersistentMap::new(),
        traces: HashMap::new(),
        reverse_index,
        // No previous hash → adapter takes the "first evaluation" path → Changed.
        previous_hashes: HashMap::new(),
        version: VersionId(1),
        snapshot_id: SnapshotId(1),
        parent_snapshot_id: SnapshotId(0),
        changed_cells: HashSet::new(),
        functions: vec![].into(),
        meta_map: Arc::new(HashMap::new()),
        objective: None,
    };

    let adapter = ConcurrentEvalAdapter::from_setup(&setup);

    // Evaluate x: default_expr = Some(literal(42)) → must evaluate and return Changed.
    let outcome = adapter.evaluate(x_node).await;

    assert_eq!(
        outcome,
        EvalOutcome::Changed,
        "Let cell with default_expr = Some(literal(42)) must return Changed"
    );

    // The value must be written.
    assert_eq!(
        adapter.values().get(&x_id),
        Some(&Value::Int(42)),
        "adapter must write Value::Int(42) for x after evaluation"
    );

    // A result entry must be recorded.
    let results = adapter.take_results();
    assert_eq!(results.len(), 1, "exactly one result entry must be pushed");
    assert_eq!(
        results[0].outcome,
        EvalOutcome::Changed,
        "result entry outcome must be Changed"
    );
}

#[cfg(feature = "test-utils")]
mod poison_fields_constants {
    use super::*;

    /// Sanity test: assert every compile-time constant in `poison_fields` holds the
    /// exact &str value expected by the structured-field schema from Task 600.
    ///
    /// This test fails to compile until `poison_fields` is added (Step 2), which is
    /// intentional TDD discipline: the test encodes the schema contract in the type
    /// system before the implementation exists.
    #[test]
    fn poison_fields_constants_exist_and_match_schema() {
        assert_eq!(poison_fields::LOCK_VALUES, "values");
        assert_eq!(poison_fields::LOCK_SNAPSHOT_VALUES, "snapshot_values");
        assert_eq!(poison_fields::LOCK_RESULTS, "results");
        assert_eq!(poison_fields::ACCESS_READ, "read");
        assert_eq!(poison_fields::ACCESS_WRITE, "write");
        assert_eq!(poison_fields::ACCESS_EXCLUSIVE, "exclusive");
        assert_eq!(poison_fields::PATH_INTO_INNER, "into_inner");
        assert_eq!(poison_fields::PATH_SHARED_FALLBACK, "shared_fallback");
        assert_eq!(
            poison_fields::MSG_LOCK_POISONED,
            "lock poisoned, recovering"
        );
    }
} // mod poison_fields_constants

/// Verify that `SchedulerConfig::default()` provides a `node_traits` field
/// whose default-empty `NodeTraitsMap<NodeId>` resolves to kind-derived defaults
/// for each node kind (PRD §5 B1 / §7.6 architecture default).
#[test]
fn scheduler_config_default_node_traits_resolves_kind_defaults() {
    use reify_eval::cache::NodeId;
    use reify_runtime::concurrent::SchedulerConfig;
    use reify_core::{ComputeNodeId, ValueCellId};
    use reify_ir::{NodeKind, NodeTraits};

    let config = SchedulerConfig::default();

    // Default-empty NodeTraitsMap<NodeId>: resolve falls through to kind-derived defaults.
    let v = NodeId::Value(ValueCellId::new("E", "x"));
    assert_eq!(config.node_traits.resolve(&v), NodeKind::Value.default_traits());
    assert_eq!(config.node_traits.resolve(&v), NodeTraits::IMMEDIATE);

    let c = NodeId::Compute(ComputeNodeId::new("E", 0));
    assert_eq!(
        config.node_traits.resolve(&c),
        NodeTraits::WARM_STARTABLE.union(NodeTraits::COMMITTABLE)
    );
}
