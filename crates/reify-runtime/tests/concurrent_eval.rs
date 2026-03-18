//! Tests for ConcurrentEvalAdapter and edit_param_concurrent.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use reify_eval::cache::{CachedResult, EvalOutcome, NodeId};
use reify_eval::deps::{DependencyTrace, ReverseDependencyIndex};
use reify_eval::graph::EvaluationGraph;
use reify_eval::{ConcurrentEditSetup, Engine};
use reify_runtime::concurrent::{AsyncNodeEvaluator, CancellationToken, ConcurrentScheduler};
use reify_runtime::concurrent_eval::{edit_param_concurrent, ConcurrentEvalAdapter};
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::TopologyTemplateBuilder;
use reify_types::{
    BinOp, DeterminacyState, PersistentMap, SnapshotId, Type, Value,
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
    let old_hash = CachedResult::Value(
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

/// Helper to build a compiled module from a template for Engine tests.
fn build_module(template: reify_compiler::TopologyTemplate) -> reify_compiler::CompiledModule {
    reify_test_support::CompiledModuleBuilder::new(reify_types::ModulePath::single("test"))
        .template(template)
        .build()
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

/// step-5: edit_param_concurrent on a linear chain produces correct values.
#[tokio::test]
async fn edit_param_concurrent_linear_chain() {
    let e = "T";
    let a_ref = reify_types::CompiledExpr::value_ref(ValueCellId::new(e, "a"), Type::Real);
    let two = reify_types::CompiledExpr::literal(Value::Real(2.0), Type::Real);
    let b_expr = reify_types::CompiledExpr::binop(BinOp::Mul, a_ref, two, Type::Real);

    let template = TopologyTemplateBuilder::new(e)
        .param(e, "a", Type::Real, Some(reify_types::CompiledExpr::literal(Value::Real(5.0), Type::Real)))
        .let_binding(e, "b", Type::Real, b_expr)
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
    let (_setup, result) = edit_param_concurrent(
        &mut engine,
        a_id.clone(),
        Value::Real(50.0),
        &cancel,
    ).await.unwrap();

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
    let a_ref = || reify_types::CompiledExpr::value_ref(ValueCellId::new(e, "a"), Type::Real);
    let x_expr = reify_types::CompiledExpr::binop(
        BinOp::Add, a_ref(),
        reify_types::CompiledExpr::literal(Value::Real(1.0), Type::Real),
        Type::Real,
    );
    let y_expr = reify_types::CompiledExpr::binop(
        BinOp::Add, a_ref(),
        reify_types::CompiledExpr::literal(Value::Real(2.0), Type::Real),
        Type::Real,
    );
    let z_expr = reify_types::CompiledExpr::binop(
        BinOp::Add, a_ref(),
        reify_types::CompiledExpr::literal(Value::Real(3.0), Type::Real),
        Type::Real,
    );

    let template = TopologyTemplateBuilder::new(e)
        .param(e, "a", Type::Real, Some(reify_types::CompiledExpr::literal(Value::Real(5.0), Type::Real)))
        .let_binding(e, "x", Type::Real, x_expr)
        .let_binding(e, "y", Type::Real, y_expr)
        .let_binding(e, "z", Type::Real, z_expr)
        .build();

    let module = build_module(template);
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let _initial = engine.eval(&module);

    let a_id = ValueCellId::new(e, "a");
    let cancel = CancellationToken::new();

    // Change a from 5 to 10
    let (_setup, result) = edit_param_concurrent(
        &mut engine,
        a_id.clone(),
        Value::Real(10.0),
        &cancel,
    ).await.unwrap();

    // All three should be correct: x=11, y=12, z=13
    assert_eq!(result.values.get(&ValueCellId::new(e, "x")), Some(&Value::Real(11.0)));
    assert_eq!(result.values.get(&ValueCellId::new(e, "y")), Some(&Value::Real(12.0)));
    assert_eq!(result.values.get(&ValueCellId::new(e, "z")), Some(&Value::Real(13.0)));

    // All three should appear in actual_eval_set and node_results
    assert_eq!(result.actual_eval_set.len(), 3, "actual_eval_set: {:?}", result.actual_eval_set);
    assert_eq!(result.node_results.len(), 3, "node_results: {:?}", result.node_results);
}

/// step-9: multi-level diamond dependency.
#[tokio::test]
async fn concurrent_diamond_dependency() {
    let e = "T";

    // param a, let b = a * 2, let c = a + 1, let d = b + c
    let a_ref = || reify_types::CompiledExpr::value_ref(ValueCellId::new(e, "a"), Type::Real);
    let b_ref = || reify_types::CompiledExpr::value_ref(ValueCellId::new(e, "b"), Type::Real);
    let c_ref = || reify_types::CompiledExpr::value_ref(ValueCellId::new(e, "c"), Type::Real);

    let b_expr = reify_types::CompiledExpr::binop(
        BinOp::Mul, a_ref(),
        reify_types::CompiledExpr::literal(Value::Real(2.0), Type::Real),
        Type::Real,
    );
    let c_expr = reify_types::CompiledExpr::binop(
        BinOp::Add, a_ref(),
        reify_types::CompiledExpr::literal(Value::Real(1.0), Type::Real),
        Type::Real,
    );
    let d_expr = reify_types::CompiledExpr::binop(BinOp::Add, b_ref(), c_ref(), Type::Real);

    let template = TopologyTemplateBuilder::new(e)
        .param(e, "a", Type::Real, Some(reify_types::CompiledExpr::literal(Value::Real(5.0), Type::Real)))
        .let_binding(e, "b", Type::Real, b_expr)
        .let_binding(e, "c", Type::Real, c_expr)
        .let_binding(e, "d", Type::Real, d_expr)
        .build();

    let module = build_module(template);
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let _initial = engine.eval(&module);

    let a_id = ValueCellId::new(e, "a");
    let cancel = CancellationToken::new();

    // Change a from 5 to 10
    let (_setup, result) = edit_param_concurrent(
        &mut engine,
        a_id.clone(),
        Value::Real(10.0),
        &cancel,
    ).await.unwrap();

    // b = 10 * 2 = 20, c = 10 + 1 = 11, d = 20 + 11 = 31
    assert_eq!(result.values.get(&ValueCellId::new(e, "b")), Some(&Value::Real(20.0)));
    assert_eq!(result.values.get(&ValueCellId::new(e, "c")), Some(&Value::Real(11.0)));
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
    let a_ref = || reify_types::CompiledExpr::value_ref(ValueCellId::new(e, "a"), Type::Real);
    let x_ref = || reify_types::CompiledExpr::value_ref(ValueCellId::new(e, "x"), Type::Real);

    let x_expr = reify_types::CompiledExpr::binop(BinOp::Sub, a_ref(), a_ref(), Type::Real);
    let y_expr = reify_types::CompiledExpr::binop(
        BinOp::Add, x_ref(),
        reify_types::CompiledExpr::literal(Value::Real(1.0), Type::Real),
        Type::Real,
    );

    let template = TopologyTemplateBuilder::new(e)
        .param(e, "a", Type::Real, Some(reify_types::CompiledExpr::literal(Value::Real(5.0), Type::Real)))
        .let_binding(e, "x", Type::Real, x_expr)
        .let_binding(e, "y", Type::Real, y_expr)
        .build();

    let module = build_module(template);
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let _initial = engine.eval(&module);

    let a_id = ValueCellId::new(e, "a");
    let cancel = CancellationToken::new();

    // Change a from 5 to 7 — x = a - a = 0 (same as before) → Unchanged
    let (_setup, result) = edit_param_concurrent(
        &mut engine,
        a_id.clone(),
        Value::Real(7.0),
        &cancel,
    ).await.unwrap();

    // (1) x should appear in actual_eval_set with outcome Unchanged
    let x_node = NodeId::Value(ValueCellId::new(e, "x"));
    let y_node = NodeId::Value(ValueCellId::new(e, "y"));

    assert!(
        result.actual_eval_set.contains(&x_node),
        "x should be in actual_eval_set: {:?}", result.actual_eval_set
    );
    let x_result = result.node_results.iter().find(|r| r.node == x_node).unwrap();
    assert_eq!(x_result.outcome, EvalOutcome::Unchanged, "x should be Unchanged");

    // (2) y should be in skipped set
    assert!(
        result.skipped.contains(&y_node),
        "y should be in skipped set: {:?}", result.skipped
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
    let a_ref = || reify_types::CompiledExpr::value_ref(ValueCellId::new(e, "a"), Type::Real);
    let b_ref = || reify_types::CompiledExpr::value_ref(ValueCellId::new(e, "b"), Type::Real);

    let b_expr = reify_types::CompiledExpr::binop(
        BinOp::Mul, a_ref(),
        reify_types::CompiledExpr::literal(Value::Real(2.0), Type::Real),
        Type::Real,
    );
    let c_expr = reify_types::CompiledExpr::binop(
        BinOp::Add, b_ref(),
        reify_types::CompiledExpr::literal(Value::Real(1.0), Type::Real),
        Type::Real,
    );

    let template = TopologyTemplateBuilder::new(e)
        .param(e, "a", Type::Real, Some(reify_types::CompiledExpr::literal(Value::Real(5.0), Type::Real)))
        .let_binding(e, "b", Type::Real, b_expr)
        .let_binding(e, "c", Type::Real, c_expr)
        .build();

    let module = build_module(template);
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let _initial = engine.eval(&module);

    // Set up a cancel token that we'll trigger after level 0
    let cancel = CancellationToken::new();

    // Use the lower-level API to control cancellation timing
    let a_id = ValueCellId::new(e, "a");
    let setup = engine.prepare_concurrent_edit(a_id, Value::Real(10.0));
    let eval_set = setup.eval_set.clone();
    let traces = setup.traces.clone();

    // Create a custom evaluator that cancels after first evaluation
    struct CancellingAdapter {
        inner: ConcurrentEvalAdapter,
        cancel: CancellationToken,
    }

    impl AsyncNodeEvaluator for CancellingAdapter {
        fn is_dirty(&self, node: &NodeId) -> bool {
            self.inner.is_dirty(node)
        }

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
    let _changed = scheduler
        .execute(eval_set.clone(), cancelling.clone(), &traces, &cancel)
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
    assert!(!c_evaluated, "c should NOT have been evaluated (cancelled between levels)");

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
    let seq_result = engine_seq.edit_param(width_id.clone(), Value::length(0.1));

    // Concurrent edit
    let cancel = CancellationToken::new();
    let (_setup, con_result) = edit_param_concurrent(
        &mut engine_con,
        width_id.clone(),
        Value::length(0.1),
        &cancel,
    ).await.unwrap();

    // (1) All values should match exactly
    for (id, seq_val) in seq_result.values.iter() {
        let con_val = con_result.values.get(id);
        assert_eq!(
            Some(seq_val), con_val,
            "values should match for {:?}", id
        );
    }

    // (2) Both should report the same evaluated nodes
    // Sequential: volume is the Value node in eval set for width change
    let seq_eval_set = engine_seq.last_eval_set();
    let volume_node = NodeId::Value(ValueCellId::new(e, "volume"));

    // Both should have volume in their eval sets
    assert!(
        seq_eval_set.contains(&volume_node),
        "sequential eval set should contain volume: {:?}", seq_eval_set
    );
    assert!(
        con_result.actual_eval_set.contains(&volume_node),
        "concurrent eval set should contain volume: {:?}", con_result.actual_eval_set
    );
}

/// step-21: rollback Pending state when scheduler returns Err(TaskPanicked).
#[tokio::test]
async fn rollback_on_task_panicked_restores_engine_state() {
    use reify_runtime::concurrent::SchedulerError;
    use reify_types::Freshness;

    let e = "T";
    let a_ref = reify_types::CompiledExpr::value_ref(ValueCellId::new(e, "a"), Type::Real);
    let two = reify_types::CompiledExpr::literal(Value::Real(2.0), Type::Real);
    let b_expr = reify_types::CompiledExpr::binop(BinOp::Mul, a_ref, two, Type::Real);

    let template = TopologyTemplateBuilder::new(e)
        .param(e, "a", Type::Real, Some(reify_types::CompiledExpr::literal(Value::Real(5.0), Type::Real)))
        .let_binding(e, "b", Type::Real, b_expr)
        .build();

    let module = build_module(template);
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let _initial = engine.eval(&module);

    let a_id = ValueCellId::new(e, "a");
    let b_node = NodeId::Value(ValueCellId::new(e, "b"));

    // Prepare concurrent edit — marks b as Pending
    let setup = engine.prepare_concurrent_edit(a_id.clone(), Value::Real(50.0));

    // Verify b is Pending
    let entry = engine.cache_store().get(&b_node).unwrap();
    assert!(
        matches!(entry.freshness, Freshness::Pending { .. }),
        "b should be Pending after prepare"
    );

    // Create a panicking evaluator
    struct PanickingEvaluator;
    impl AsyncNodeEvaluator for PanickingEvaluator {
        fn is_dirty(&self, _node: &NodeId) -> bool {
            true
        }
        async fn evaluate(&self, _node: NodeId) -> EvalOutcome {
            panic!("intentional panic in evaluator");
        }
    }

    let panicking = Arc::new(PanickingEvaluator);
    let cancel = CancellationToken::new();
    let scheduler = ConcurrentScheduler;

    // Execute — should return Err(TaskPanicked)
    let result = scheduler
        .execute(setup.eval_set.clone(), panicking, &setup.traces, &cancel)
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
        entry.freshness, Freshness::Final,
        "b should be Final after rollback, got: {:?}", entry.freshness
    );

    // (2) Sequential edit_param should succeed with correct values
    let seq_result = engine.edit_param(a_id.clone(), Value::Real(50.0));
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
    use reify_types::Freshness;

    let e = "T";

    // param a, let b = a * 2, let c = b + 1
    let a_ref = || reify_types::CompiledExpr::value_ref(ValueCellId::new(e, "a"), Type::Real);
    let b_ref = || reify_types::CompiledExpr::value_ref(ValueCellId::new(e, "b"), Type::Real);

    let b_expr = reify_types::CompiledExpr::binop(
        BinOp::Mul, a_ref(),
        reify_types::CompiledExpr::literal(Value::Real(2.0), Type::Real),
        Type::Real,
    );
    let c_expr = reify_types::CompiledExpr::binop(
        BinOp::Add, b_ref(),
        reify_types::CompiledExpr::literal(Value::Real(1.0), Type::Real),
        Type::Real,
    );

    let template = TopologyTemplateBuilder::new(e)
        .param(e, "a", Type::Real, Some(reify_types::CompiledExpr::literal(Value::Real(5.0), Type::Real)))
        .let_binding(e, "b", Type::Real, b_expr)
        .let_binding(e, "c", Type::Real, c_expr)
        .build();

    let module = build_module(template);
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let _initial = engine.eval(&module);

    let a_id = ValueCellId::new(e, "a");
    let b_node = NodeId::Value(ValueCellId::new(e, "b"));
    let c_node = NodeId::Value(ValueCellId::new(e, "c"));

    // === First cycle: prepare → panicking scheduler → rollback ===
    let setup1 = engine.prepare_concurrent_edit(a_id.clone(), Value::Real(20.0));

    struct PanickingEvaluator;
    impl AsyncNodeEvaluator for PanickingEvaluator {
        fn is_dirty(&self, _node: &NodeId) -> bool { true }
        async fn evaluate(&self, _node: NodeId) -> EvalOutcome {
            panic!("intentional panic in evaluator");
        }
    }

    let cancel = CancellationToken::new();
    let scheduler = ConcurrentScheduler;
    let err_result = scheduler
        .execute(setup1.eval_set.clone(), Arc::new(PanickingEvaluator), &setup1.traces, &cancel)
        .await;
    assert!(matches!(err_result, Err(SchedulerError::TaskPanicked(_))));

    // Rollback
    engine.rollback_concurrent_edit(&setup1);

    // Verify engine is in clean state
    let entry_b = engine.cache_store().get(&b_node).unwrap();
    assert_eq!(entry_b.freshness, Freshness::Final, "b should be Final after rollback");
    let entry_c = engine.cache_store().get(&c_node).unwrap();
    assert_eq!(entry_c.freshness, Freshness::Final, "c should be Final after rollback");

    // === Second cycle: edit_param_concurrent should succeed normally ===
    let (setup2, result2) = edit_param_concurrent(
        &mut engine,
        a_id.clone(),
        Value::Real(20.0),
        &cancel,
    ).await.unwrap();

    // Values should be correct: a=20, b=40, c=41
    assert_eq!(result2.values.get(&ValueCellId::new(e, "a")), Some(&Value::Real(20.0)));
    assert_eq!(result2.values.get(&ValueCellId::new(e, "b")), Some(&Value::Real(40.0)));
    assert_eq!(result2.values.get(&ValueCellId::new(e, "c")), Some(&Value::Real(41.0)));

    // === Third: apply and verify engine state is fully correct ===
    engine.apply_concurrent_edit(&setup2, result2);

    // Cache freshness should be Final for all evaluated nodes
    let entry_b = engine.cache_store().get(&b_node).unwrap();
    assert_eq!(entry_b.freshness, Freshness::Final, "b should be Final after apply");
    let entry_c = engine.cache_store().get(&c_node).unwrap();
    assert_eq!(entry_c.freshness, Freshness::Final, "c should be Final after apply");

    // Snapshot should have correct version
    let snapshot = engine.snapshot().unwrap();
    assert_eq!(snapshot.version, setup2.version, "version should match setup2");

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
    use reify_types::{CompiledExpr, CompiledExprKind, ContentHash};

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
        .param(e, "a", Type::Int, Some(CompiledExpr::literal(Value::Int(5), Type::Int)))
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
    let (_setup, result) = edit_param_concurrent(
        &mut engine,
        a_id.clone(),
        Value::Int(10),
        &cancel,
    ).await.unwrap();

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
    use reify_types::{CompiledExpr, CompiledExprKind, ContentHash};

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
        .param(e, "a", Type::Int, Some(CompiledExpr::literal(Value::Int(5), Type::Int)))
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
    let (_setup, result) = edit_param_concurrent(
        &mut engine,
        a_id.clone(),
        Value::Int(10),
        &cancel,
    ).await.unwrap();

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
///
/// Currently FAILS because the concurrent edit path doesn't include a resolution phase.
#[tokio::test]
async fn edit_param_concurrent_re_resolves_auto_params() {
    use reify_test_support::builders::{binop, gt, literal, value_ref};
    use reify_test_support::mocks::SequencedMockConstraintSolver;
    use reify_test_support::mm;
    use reify_types::SolveResult;

    let a_id = ValueCellId::new("S", "a");
    let x_id = ValueCellId::new("S", "x");
    let y_id = ValueCellId::new("S", "y");

    // Sequenced solver: first x=mm(5.0), second x=mm(20.0)
    let mut solved1 = HashMap::new();
    solved1.insert(x_id.clone(), mm(5.0));
    let mut solved2 = HashMap::new();
    solved2.insert(x_id.clone(), mm(20.0));

    let solver = SequencedMockConstraintSolver::new(vec![
        SolveResult::Solved { values: solved1 },
        SolveResult::Solved { values: solved2 },
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
    let mut engine = Engine::new(Box::new(checker), None)
        .with_solver(Box::new(solver));

    // Cold eval: x resolved to mm(5.0), y = 0.005*2 = 0.01
    let cold = engine.eval(&module);
    assert!(
        !cold.resolved_params.is_empty(),
        "cold eval should resolve auto params"
    );

    let cancel = CancellationToken::new();

    // Concurrent edit: change a from mm(3.0) to mm(8.0)
    let (setup, result) = edit_param_concurrent(
        &mut engine,
        a_id.clone(),
        mm(8.0),
        &cancel,
    ).await.unwrap();

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
    use reify_test_support::mocks::SequencedMockConstraintSolver;
    use reify_test_support::mm;
    use reify_types::SolveResult;

    let a_id = ValueCellId::new("S", "a");
    let x_id = ValueCellId::new("S", "x");

    // Sequenced solver: first x=mm(5.0), second x=mm(20.0)
    let mut solved1 = HashMap::new();
    solved1.insert(x_id.clone(), mm(5.0));
    let mut solved2 = HashMap::new();
    solved2.insert(x_id.clone(), mm(20.0));

    let solver = SequencedMockConstraintSolver::new(vec![
        SolveResult::Solved { values: solved1 },
        SolveResult::Solved { values: solved2 },
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
    let mut engine = Engine::new(Box::new(checker), None)
        .with_solver(Box::new(solver));

    // Cold eval: x resolved to mm(5.0)
    let cold = engine.eval(&module);
    assert!(!cold.resolved_params.is_empty(), "cold eval should resolve auto params");

    let cancel = CancellationToken::new();

    // Concurrent edit: change a from mm(3.0) to mm(8.0)
    let (_setup, result) = edit_param_concurrent(
        &mut engine,
        a_id.clone(),
        mm(8.0),
        &cancel,
    ).await.unwrap();

    // The ConcurrentEditResult should carry resolved_params
    assert!(
        !result.resolved_params.is_empty(),
        "ConcurrentEditResult should include resolved_params after re-resolution"
    );
    let resolved_x = result.resolved_params.get(&x_id)
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
