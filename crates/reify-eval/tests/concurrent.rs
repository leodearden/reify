//! Tests for concurrent evaluation support in Engine.
//!
//! Verifies that Engine::prepare_concurrent_edit() correctly extracts state
//! for concurrent evaluation and Engine::apply_concurrent_edit() correctly
//! merges results back.

use std::collections::{HashMap, HashSet};

use reify_core::{ConstraintNodeId, ModulePath, Type, ValueCellId};
use reify_eval::cache::{EvalOutcome, NodeId};
use reify_eval::deps::DependencyTrace;
use reify_eval::journal::{EventKind, EventPayload};
use reify_eval::{ConcurrentEditResult, ConcurrentEditSetup, ConcurrentNodeResult, Engine};
use reify_ir::{BinOp, DeterminacyState, Freshness, SnapshotProvenance, SolveResult, Value};
use reify_test_support::mocks::{
    MockConstraintChecker, MultiCallSpyConstraintSolver, SequencedMockConstraintSolver,
};
use reify_test_support::{
    CompiledModuleBuilder, TopologyTemplateBuilder, binop, bracket_compiled_module, gt, literal,
    mm, value_ref,
};

/// Test that prepare_concurrent_edit returns ConcurrentEditSetup with correct state.
#[test]
fn prepare_concurrent_edit_returns_correct_setup() {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    // Cold-start eval to establish baseline
    let _initial = engine.eval(&module);

    let e = "Bracket";
    let width_id = ValueCellId::new(e, "width");

    // Prepare concurrent edit: change width from 80mm to 100mm
    let setup = engine
        .prepare_concurrent_edit(width_id.clone(), Value::length(0.1))
        .unwrap();

    // (1) eval_set should match sequential dirty∩demand set for width change
    // width change → dirty = {volume, C1, R0}; all are demanded → eval_set = {volume, C1, R0}
    assert_eq!(
        setup.eval_set.len(),
        3,
        "eval_set should have 3 nodes (volume + C1 + R0), got: {:?}",
        setup.eval_set
    );
    assert!(
        setup
            .eval_set
            .contains(&NodeId::Value(ValueCellId::new(e, "volume"))),
        "eval_set should contain volume"
    );
    assert!(
        setup
            .eval_set
            .contains(&NodeId::Constraint(ConstraintNodeId::new(e, 1))),
        "eval_set should contain C1"
    );
    assert!(
        setup
            .eval_set
            .contains(&NodeId::Realization(reify_core::RealizationNodeId::new(
                e, 0
            ))),
        "eval_set should contain R0"
    );

    // (2) previous_hashes should contain entries for nodes that had cache entries
    let volume_node = NodeId::Value(ValueCellId::new(e, "volume"));
    assert!(
        setup.previous_hashes.contains_key(&volume_node),
        "previous_hashes should contain volume"
    );

    // (3) values map should have all current parameter values
    assert_eq!(
        setup.values.get(&ValueCellId::new(e, "width")),
        Some(&Value::length(0.1)),
        "values should have updated width"
    );
    assert_eq!(
        setup.values.get(&ValueCellId::new(e, "height")),
        Some(&Value::length(0.10)),
        "values should have height"
    );

    // (4) graph should have correct number of value cells
    assert_eq!(
        setup.graph.value_cells.len(),
        6,
        "graph should have 6 value cells"
    );

    // (5) version should be bumped from initial (initial eval uses version 0)
    assert!(
        setup.version.0 > 0,
        "version should be bumped from initial, got: {:?}",
        setup.version
    );

    // Verify changed_cells contains the edited parameter
    assert!(
        setup.changed_cells.contains(&ValueCellId::new(e, "width")),
        "changed_cells should contain width"
    );
}

/// step-13: Engine::apply_concurrent_edit correctly updates Engine state.
#[test]
fn apply_concurrent_edit_updates_engine_state() {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    // Cold-start eval to establish baseline
    let _initial = engine.eval(&module);

    let e = "Bracket";
    let width_id = ValueCellId::new(e, "width");
    let volume_id = ValueCellId::new(e, "volume");
    let volume_node = NodeId::Value(volume_id.clone());

    // Prepare concurrent edit
    let setup = engine
        .prepare_concurrent_edit(width_id.clone(), Value::length(0.1))
        .unwrap();

    // Simulate what ConcurrentEvalAdapter would produce:
    // Volume = width * height * thickness = 0.1 * 0.1 * 0.005 = 5e-5
    let new_volume = Value::Scalar {
        si_value: 5e-5,
        dimension: reify_core::dimension::DimensionVector::VOLUME,
    };

    let mut snapshot_values = setup.snapshot_values.clone();
    snapshot_values.insert(
        volume_id.clone(),
        (new_volume.clone(), DeterminacyState::Determined),
    );

    let node_results = vec![ConcurrentNodeResult {
        node: volume_node.clone(),
        value: new_volume.clone(),
        determinacy: DeterminacyState::Determined,
        trace: DependencyTrace {
            realization_reads: Vec::new(),
            reads: vec![
                ValueCellId::new(e, "width"),
                ValueCellId::new(e, "height"),
                ValueCellId::new(e, "thickness"),
            ],
        },
        outcome: EvalOutcome::Changed,
        eval_duration: None,
    }];

    let mut values = setup.values.clone();
    values.insert(volume_id.clone(), new_volume.clone());

    // C1 is in eval_set but was not evaluated (constraint node)
    let c1_node = NodeId::Constraint(ConstraintNodeId::new(e, 1));
    let skipped: HashSet<NodeId> = [c1_node.clone()].into_iter().collect();

    let result = ConcurrentEditResult {
        values,
        snapshot_values,
        node_results,
        actual_eval_set: vec![volume_node.clone()],
        skipped: skipped.clone(),
        resolved_params: std::collections::HashMap::new(),
        diagnostics: Vec::new(),
    };

    // Apply the result
    engine.apply_concurrent_edit(&setup, result);

    // (1) Cache should have updated entry for volume with correct freshness
    let cache_entry = engine.cache_store().get(&volume_node);
    assert!(cache_entry.is_some(), "volume should be in cache");
    let entry = cache_entry.unwrap();
    assert_eq!(entry.freshness, Freshness::Final);
    assert_eq!(entry.basis_version, setup.version);

    // (2) Snapshot should be updated with Edit provenance
    let snapshot = engine.snapshot().unwrap();
    assert_eq!(snapshot.id, setup.snapshot_id);
    assert_eq!(snapshot.version, setup.version);
    match &snapshot.provenance {
        SnapshotProvenance::Edit { changed, parent } => {
            assert!(changed.contains(&width_id));
            assert_eq!(*parent, setup.parent_snapshot_id);
        }
        other => panic!("Expected Edit provenance, got: {:?}", other),
    }

    // (3) last_eval_set should match actual_eval_set
    assert!(
        engine.last_eval_set().contains(&volume_node),
        "last_eval_set should contain volume"
    );

    // (4) Journal should have Started+Completed event pairs for volume
    let volume_events = engine.journal().events_for_node(&volume_node);
    // After eval(), volume already has events. After apply, we add 2 more.
    let new_events: Vec<_> = volume_events
        .iter()
        .filter(|e| e.version == setup.version)
        .collect();
    assert_eq!(
        new_events.len(),
        2,
        "should have Started+Completed for volume"
    );

    // (5) Completed event Duration payload must be Some(_) even when eval_duration is None.
    // This exercises the `unwrap_or_else(|| start.elapsed())` fallback path —
    // verifying that the fallback produces a valid, non-None Duration when no
    // eval_duration was supplied by the concurrent adapter.
    let completed = new_events
        .iter()
        .find(|ev| matches!(ev.kind, EventKind::Completed { .. }))
        .expect("should have a Completed event");
    assert!(
        matches!(completed.payload, Some(EventPayload::Duration(_))),
        "Completed event must carry a Duration payload even when eval_duration is None; \
         got: {:?}",
        completed.payload
    );
}

/// step-19: Engine::rollback_concurrent_edit() restores all eval_set nodes
/// from Pending back to Final and rolls back version/snapshot IDs.
#[test]
fn rollback_concurrent_edit_restores_pending_to_final() {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    // Cold-start eval to establish baseline
    let _initial = engine.eval(&module);

    let e = "Bracket";
    let width_id = ValueCellId::new(e, "width");
    let volume_node = NodeId::Value(ValueCellId::new(e, "volume"));
    let c1_node = NodeId::Constraint(ConstraintNodeId::new(e, 1));

    // Record pre-prepare state for verification after rollback
    let _pre_snapshot_id = engine.snapshot().unwrap().id;
    let _pre_version = engine.snapshot().unwrap().version;
    let pre_volume_hash = engine.cache_store().get(&volume_node).unwrap().result_hash;

    // Prepare concurrent edit — marks eval_set nodes as Pending
    let setup = engine
        .prepare_concurrent_edit(width_id.clone(), Value::length(0.1))
        .unwrap();

    // Verify nodes are in Pending state after prepare
    let volume_entry = engine.cache_store().get(&volume_node).unwrap();
    assert!(
        matches!(volume_entry.freshness, Freshness::Pending { .. }),
        "volume should be Pending after prepare, got: {:?}",
        volume_entry.freshness
    );

    // Rollback the concurrent edit
    engine.rollback_concurrent_edit(&setup);

    // (1) All nodes in eval_set should have freshness=Final (not Pending)
    let volume_entry = engine.cache_store().get(&volume_node).unwrap();
    assert_eq!(
        volume_entry.freshness,
        Freshness::Final,
        "volume should be Final after rollback"
    );
    // C1 may not have a cache entry (constraint nodes might not be cached),
    // but if it does, it should be Final
    if let Some(c1_entry) = engine.cache_store().get(&c1_node) {
        assert_eq!(
            c1_entry.freshness,
            Freshness::Final,
            "C1 should be Final after rollback"
        );
    }

    // (2) Cache entries should still contain original result_hash values
    let volume_entry = engine.cache_store().get(&volume_node).unwrap();
    assert_eq!(
        volume_entry.result_hash, pre_volume_hash,
        "volume result_hash should be preserved after rollback"
    );

    // (3) Version and snapshot IDs should be rolled back to pre-prepare values
    // The next_snapshot_id and next_version_id should be decremented so next
    // prepare/edit uses the same IDs (no gaps).
    // We can verify this indirectly: calling edit_param should produce the
    // same version/snapshot IDs that the failed prepare would have used.
    let seq_result = engine
        .edit_param(width_id.clone(), Value::length(0.1))
        .unwrap();

    // The snapshot after edit_param should have the same IDs as the setup had
    let post_snapshot = engine.snapshot().unwrap();
    assert_eq!(
        post_snapshot.id, setup.snapshot_id,
        "snapshot ID should be reused after rollback"
    );
    assert_eq!(
        post_snapshot.version, setup.version,
        "version ID should be reused after rollback"
    );

    // (4) Subsequent edit_param produces correct values (engine not corrupted)
    let volume_val = seq_result.values.get(&ValueCellId::new(e, "volume"));
    assert!(
        volume_val.is_some(),
        "volume should have a value after sequential edit"
    );
}

/// Helper: build the canonical auto-param module used by pipeline tests.
///
/// Template S:
///   param a (default mm(3.0))
///   auto x (length)
///   let y = x * 2.0 (length)
///   constraint 0: x > a
fn build_auto_param_module() -> reify_compiler::CompiledModule {
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
    CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build()
}

/// Helper: build a SequencedMockConstraintSolver with two Solved results for x.
fn make_two_call_solver(
    x_id: &ValueCellId,
    first: Value,
    second: Value,
) -> SequencedMockConstraintSolver {
    let mut solved1 = HashMap::new();
    solved1.insert(x_id.clone(), first);
    let mut solved2 = HashMap::new();
    solved2.insert(x_id.clone(), second);
    SequencedMockConstraintSolver::new(vec![
        SolveResult::Solved {
            values: solved1,
            unique: true,
        },
        SolveResult::Solved {
            values: solved2,
            unique: true,
        },
    ])
}

/// Helper: construct a "no-op" ConcurrentEditResult seeded from setup with
/// empty node_results, skipped, resolved_params, and diagnostics. Used by
/// tests that exercise resolve_concurrent_edit / apply_concurrent_edit
/// without simulating scheduler-produced node results.
fn empty_result_from_setup(setup: &ConcurrentEditSetup) -> ConcurrentEditResult {
    ConcurrentEditResult {
        values: setup.values.clone(),
        snapshot_values: setup.snapshot_values.clone(),
        node_results: vec![],
        actual_eval_set: setup.eval_set.clone(),
        skipped: HashSet::new(),
        resolved_params: HashMap::new(),
        diagnostics: Vec::new(),
    }
}

/// step-1: Full prepare → resolve → apply pipeline re-resolves auto param.
///
/// Module: param a (mm(3.0)), auto x, let y = x*2, constraint x > a.
/// Solver call 1: x=mm(5.0). Solver call 2: x=mm(20.0).
/// After cold eval (call 1), change a → mm(8.0), resolve_concurrent_edit should
/// call the solver again (call 2) and update result.resolved_params and result.values[x].
/// apply_concurrent_edit should persist the new snapshot.
#[test]
fn pipeline_prepare_resolve_apply_re_resolves_auto_param() {
    let x_id = ValueCellId::new("S", "x");
    let a_id = ValueCellId::new("S", "a");

    let solver = make_two_call_solver(&x_id, mm(5.0), mm(20.0));
    let module = build_auto_param_module();
    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(solver));

    // Cold eval: solver call 1 → x = mm(5.0) = 0.005 SI
    let cold = engine.eval(&module);
    let cold_x = cold
        .values
        .get(&x_id)
        .expect("x should be in cold eval values");
    assert!(
        matches!(cold_x, Value::Scalar { si_value, .. } if (*si_value - 0.005).abs() < 1e-10),
        "expected cold x = mm(5.0) = 0.005 SI, got {:?}",
        cold_x
    );

    // prepare_concurrent_edit: change a from mm(3.0) to mm(8.0)
    let setup = engine
        .prepare_concurrent_edit(a_id.clone(), mm(8.0))
        .expect("prepare_concurrent_edit should succeed");

    // Build a minimal ConcurrentEditResult: cloned values from setup, no node_results.
    // The scheduler would have evaluated constraint nodes, but we pass an empty result
    // since only the solver path (resolve) is under test here.
    let mut result = empty_result_from_setup(&setup);

    // resolve_concurrent_edit: solver call 2 → x = mm(20.0) = 0.02 SI
    engine.resolve_concurrent_edit(&setup, &mut result);

    // resolved_params must contain x → mm(20.0)
    let resolved_x = result
        .resolved_params
        .get(&x_id)
        .expect("resolved_params should contain x after resolve");
    assert!(
        matches!(resolved_x, Value::Scalar { si_value, .. } if (*si_value - 0.02).abs() < 1e-10),
        "expected resolved x = mm(20.0) = 0.02 SI, got {:?}",
        resolved_x
    );

    // values map must also be updated
    let val_x = result.values.get(&x_id).expect("values should contain x");
    assert!(
        matches!(val_x, Value::Scalar { si_value, .. } if (*si_value - 0.02).abs() < 1e-10),
        "expected result.values[x] = mm(20.0) = 0.02 SI, got {:?}",
        val_x
    );

    // No diagnostics for a successful solve
    assert!(
        result.diagnostics.is_empty(),
        "expected empty diagnostics, got {:?}",
        result.diagnostics
    );

    // Apply the result
    engine.apply_concurrent_edit(&setup, result);

    // Snapshot should carry x = mm(20.0)
    let snap = engine
        .snapshot()
        .expect("snapshot should exist after apply");
    let (snap_x, snap_det) = snap.values.get(&x_id).expect("x should be in snapshot");
    assert!(
        matches!(snap_x, Value::Scalar { si_value, .. } if (*si_value - 0.02).abs() < 1e-10),
        "expected snapshot x = mm(20.0) = 0.02 SI, got {:?}",
        snap_x
    );
    assert_eq!(*snap_det, DeterminacyState::Determined);

    // Snapshot version should match setup.version
    assert_eq!(
        snap.version, setup.version,
        "snapshot version should match setup"
    );

    // Provenance should be Edit with changed = {a}
    match &snap.provenance {
        SnapshotProvenance::Edit { changed, parent } => {
            assert!(changed.contains(&a_id), "changed set should contain a");
            assert_eq!(*parent, setup.parent_snapshot_id);
        }
        other => panic!("expected Edit provenance, got {:?}", other),
    }
}

/// step-3: resolve_concurrent_edit propagates resolved auto params to dependent
/// let bindings via a second dirty-cone sweep (wave-2).
///
/// The constraint `x > a` references a directly, so changing `a` dirties the
/// constraint node.  After the solver resolves x in wave-1, wave-2 must find y
/// (which depends on x, not on a) and re-evaluate it.
///
/// Assertion: result.values[y] == mm(20.0)*2 = 0.04 SI after resolve.
#[test]
fn resolve_concurrent_edit_second_wave_updates_dependent_let_binding() {
    let x_id = ValueCellId::new("S", "x");
    let y_id = ValueCellId::new("S", "y");
    let a_id = ValueCellId::new("S", "a");

    let solver = make_two_call_solver(&x_id, mm(5.0), mm(20.0));
    let module = build_auto_param_module();
    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(solver));

    // Cold eval: x = mm(5.0) = 0.005 SI, y = 0.01 SI
    let _cold = engine.eval(&module);

    // prepare_concurrent_edit: change a → mm(8.0)
    let setup = engine
        .prepare_concurrent_edit(a_id.clone(), mm(8.0))
        .expect("prepare_concurrent_edit should succeed");

    let mut result = empty_result_from_setup(&setup);

    // resolve_concurrent_edit: wave-1 re-resolves x to mm(20.0); wave-2 re-evaluates y.
    engine.resolve_concurrent_edit(&setup, &mut result);

    // Wave-2 must have updated y to x*2 = mm(20.0)*2 = 0.04 SI
    let val_y = result.values.get(&y_id).expect("values should contain y");
    assert!(
        matches!(val_y, Value::Scalar { si_value, .. } if (*si_value - 0.04).abs() < 1e-10),
        "expected y = 0.04 SI (mm(20.0)*2) after wave-2 propagation, got {:?}",
        val_y
    );

    // snapshot_values[y] must also be updated
    let (snap_y, snap_det) = result
        .snapshot_values
        .get(&y_id)
        .expect("snapshot_values should contain y");
    assert!(
        matches!(snap_y, Value::Scalar { si_value, .. } if (*si_value - 0.04).abs() < 1e-10),
        "expected snapshot_values[y] = 0.04 SI, got {:?}",
        snap_y
    );
    assert_eq!(*snap_det, DeterminacyState::Determined);

    // Cache should have an updated entry for y (wave-2 calls record_evaluation)
    let y_node = NodeId::Value(y_id.clone());
    assert!(
        engine.cache_store().get(&y_node).is_some(),
        "cache should have an entry for y after wave-2 record_evaluation"
    );
    let cache_y = engine.cache_store().get(&y_node).unwrap();
    assert_eq!(
        cache_y.basis_version, setup.version,
        "cache y basis_version should be setup.version"
    );
}

/// Returns an `(Engine, ConcurrentEditSetup)` for a minimal N-template:
///   `param a` (default 3 mm), `let b = a * 2` — no solver, no constraints.
///
/// Shared setup for `resolve_concurrent_edit_panics_*` and the fresh-input
/// no-op test so the engine/module/prepare boilerplate is not repeated in
/// every test body.
fn setup_minimal_concurrent_edit() -> (Engine, ConcurrentEditSetup) {
    let a_id = ValueCellId::new("N", "a");
    let template = TopologyTemplateBuilder::new("N")
        .param("N", "a", Type::length(), Some(literal(mm(3.0))))
        .let_binding(
            "N",
            "b",
            Type::length(),
            binop(BinOp::Mul, value_ref("N", "a"), literal(Value::Real(2.0))),
        )
        .build();
    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = Engine::new(Box::new(MockConstraintChecker::new()), None);
    let _cold = engine.eval(&module);

    let setup = engine
        .prepare_concurrent_edit(a_id, mm(5.0))
        .expect("prepare_concurrent_edit should succeed");

    (engine, setup)
}

/// Verifies that `resolve_concurrent_edit` does not panic and leaves both
/// output buckets empty when called with a fresh `ConcurrentEditResult`
/// and an Engine that has no constraint solver.
///
/// The no-solver short-circuit returns immediately without populating
/// `resolved_params` or `diagnostics`. This documents the happy-path
/// contract: callers pass empty buckets and receive empty buckets back.
#[test]
fn resolve_concurrent_edit_without_solver_is_noop_fresh_input() {
    let (mut engine, setup) = setup_minimal_concurrent_edit();
    // setup_minimal_concurrent_edit constructs the Engine with `None` as the
    // solver argument (see its body: `Engine::new(..., None)`), so the
    // `if let Some(ref solver) = self.solver` guard in `resolve_concurrent_edit`
    // (src/concurrent.rs) cannot be entered — that guard is what guarantees
    // the no-op behavior asserted below.

    let mut result = empty_result_from_setup(&setup);

    // Should not panic and should not populate either output bucket.
    engine.resolve_concurrent_edit(&setup, &mut result);

    assert!(
        result.resolved_params.is_empty(),
        "no solver => no resolved params"
    );
    assert!(result.diagnostics.is_empty(), "no solver => no diagnostics");
}

/// Verifies that `resolve_concurrent_edit` panics in both debug and release
/// builds when `result.resolved_params` is not empty on entry.
///
/// Callers must pass a fresh `ConcurrentEditResult`; pre-populating
/// `resolved_params` indicates a double-call or incorrect usage. The
/// `assert!` guard enforces this contract uniformly across profiles.
#[test]
#[should_panic(expected = "resolved_params must be empty")]
fn resolve_concurrent_edit_panics_on_prepopulated_resolved_params() {
    let (mut engine, setup) = setup_minimal_concurrent_edit();

    // Pre-populate resolved_params with one stale entry; diagnostics is empty.
    let mut stale_resolved: HashMap<ValueCellId, Value> = HashMap::new();
    stale_resolved.insert(ValueCellId::new("N", "bogus"), mm(99.0));

    let mut result = empty_result_from_setup(&setup);
    result.resolved_params = stale_resolved;

    // Must panic on the first debug_assert (resolved_params not empty).
    engine.resolve_concurrent_edit(&setup, &mut result);
}

/// Verifies that `resolve_concurrent_edit` panics in both debug and release
/// builds when `result.diagnostics` is not empty on entry.
///
/// Only `diagnostics` is pre-populated here so the `resolved_params`
/// assert passes and the `diagnostics` assert is the one that fires.
#[test]
#[should_panic(expected = "diagnostics must be empty")]
fn resolve_concurrent_edit_panics_on_prepopulated_diagnostics() {
    let (mut engine, setup) = setup_minimal_concurrent_edit();

    // resolved_params is empty (first debug_assert passes),
    // diagnostics has one stale warning (second debug_assert fires).
    let stale_diag = reify_core::Diagnostic::warning("stale diagnostic".to_string());

    let mut result = empty_result_from_setup(&setup);
    result.diagnostics = vec![stale_diag];

    // Must panic on the second debug_assert (diagnostics not empty).
    engine.resolve_concurrent_edit(&setup, &mut result);
}

/// step-7: rollback_concurrent_edit restores ALL eval_set nodes to Final, not just
/// the ones that individual existing tests inspect (volume and C1 only).
///
/// After rollback, for every node in setup.eval_set that has a cache entry:
/// - freshness must be Final
/// - result_hash must equal the pre-prepare hash (rollback did not corrupt content)
#[test]
fn rollback_restores_every_pending_node_to_final() {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    // Cold eval to populate cache with Final entries.
    let _initial = engine.eval(&module);

    let e = "Bracket";
    let width_id = ValueCellId::new(e, "width");

    // Capture pre-prepare result_hashes for every cached node.
    // We capture them BEFORE prepare so we can compare after rollback.
    // We'll snapshot the full cache by interrogating it for each node
    // in the eval_set after prepare (which is the same set that will be Pending).

    // prepare_concurrent_edit: width changes → dirties {volume, C1, R0}
    let setup = engine
        .prepare_concurrent_edit(width_id.clone(), Value::length(0.1))
        .expect("prepare_concurrent_edit should succeed");

    // Collect pre-prepare hashes from previous_hashes in setup.
    // setup.previous_hashes contains content hashes captured before prepare
    // marked nodes as Pending.
    let pre_hashes = setup.previous_hashes.clone();

    // After prepare, every node in eval_set with a cache entry should be Pending.
    for node_id in &setup.eval_set {
        if let Some(entry) = engine.cache_store().get(node_id) {
            assert!(
                matches!(entry.freshness, Freshness::Pending { .. }),
                "node {:?} should be Pending after prepare, got {:?}",
                node_id,
                entry.freshness
            );
        }
    }

    // Rollback the concurrent edit.
    engine.rollback_concurrent_edit(&setup);

    // Every node in eval_set with a cache entry must now be Final with original hash.
    let mut examined_count = 0usize;
    for node_id in &setup.eval_set {
        if let Some(entry) = engine.cache_store().get(node_id) {
            examined_count += 1;
            assert_eq!(
                entry.freshness,
                Freshness::Final,
                "node {:?} should be Final after rollback, got {:?}",
                node_id,
                entry.freshness
            );
            // Hash must be preserved (rollback did not touch the value).
            if let Some(&pre_hash) = pre_hashes.get(node_id) {
                assert_eq!(
                    entry.result_hash, pre_hash,
                    "node {:?} result_hash should be unchanged after rollback",
                    node_id
                );
            }
        }
    }
    assert!(
        examined_count >= 1,
        "expected to examine at least 1 cached node after rollback (volume is always cached), \
         got {}; test would pass vacuously if cache were empty. \
         Note: eval_set contains 3 nodes (volume, C1, R0) but only value cells \
         (NodeId::Value) have entries in the value cache; constraint and realization \
         nodes are not cached as value cells",
        examined_count
    );

    // Sanity: confirm volume's value cell is in eval_set, so the count above is non-vacuous.
    let volume_node = NodeId::Value(ValueCellId::new(e, "volume"));
    assert!(
        setup.eval_set.contains(&volume_node),
        "eval_set should contain volume (sanity check)"
    );
}

/// step-9: Three consecutive prepare/rollback cycles do not leak version/snapshot IDs.
///
/// After each rollback, the engine's internal counters are restored so the next
/// prepare would reuse the same IDs. After three failed cycles, edit_param
/// should produce the same snapshot_id/version that the FIRST prepare allocated
/// (not some gap-inflated value).
#[test]
fn rollback_multiple_cycles_reuse_ids_no_gaps() {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    // Cold eval to establish baseline snapshot.
    let _initial = engine.eval(&module);

    let e = "Bracket";
    let width_id = ValueCellId::new(e, "width");

    // Capture the snapshot id/version from the cold eval.
    let cold_id = engine.snapshot().unwrap().id;
    let cold_version = engine.snapshot().unwrap().version;

    // Track the first prepare's allocated ids for later comparison.
    let mut first_snapshot_id = None;
    let mut first_version = None;

    // Three prepare/rollback cycles — none should leak IDs.
    for i in 0..3usize {
        // Before each prepare the snapshot should still be the cold-eval snapshot.
        assert_eq!(
            engine.snapshot().unwrap().id,
            cold_id,
            "cycle {}: snapshot id should be cold-eval id before prepare",
            i
        );
        assert_eq!(
            engine.snapshot().unwrap().version,
            cold_version,
            "cycle {}: snapshot version should be cold-eval version before prepare",
            i
        );

        let setup = engine
            .prepare_concurrent_edit(width_id.clone(), Value::length(0.1))
            .expect("prepare_concurrent_edit should succeed");

        if i == 0 {
            first_snapshot_id = Some(setup.snapshot_id);
            first_version = Some(setup.version);
        }

        // Rollback: counters restored to pre-prepare values.
        engine.rollback_concurrent_edit(&setup);

        // After rollback the snapshot is unchanged.
        assert_eq!(
            engine.snapshot().unwrap().id,
            cold_id,
            "cycle {}: snapshot id should be restored after rollback",
            i
        );
        assert_eq!(
            engine.snapshot().unwrap().version,
            cold_version,
            "cycle {}: snapshot version should be restored after rollback",
            i
        );
    }

    // After three failed cycles, a committed edit_param should reuse the first
    // prepare's ids — no gaps leaked by the three rollbacks.
    let _seq_result = engine
        .edit_param(width_id.clone(), Value::length(0.1))
        .expect("edit_param should succeed");

    let post_snap = engine.snapshot().unwrap();
    assert_eq!(
        post_snap.id,
        first_snapshot_id.unwrap(),
        "edit_param after 3 rollback cycles should produce the same snapshot_id as the first prepare"
    );
    assert_eq!(
        post_snap.version,
        first_version.unwrap(),
        "edit_param after 3 rollback cycles should produce the same version as the first prepare"
    );
}

/// step-11: apply_concurrent_edit persists resolved auto params to param_overrides.
///
/// The `param_overrides` commit loop inside `apply_concurrent_edit` writes solved
/// auto-param values into the engine's param_overrides map. We verify this by asserting:
/// (a) The snapshot immediately after apply carries x = mm(20.0).
/// (b) A subsequent engine.eval() returns x == mm(99.0) exactly. Solver call 3 returns
///     Solved(mm(99.0)), and the resolution phase in eval() writes this to values.
///     Asserting the exact value (0.099 SI) is stronger than the previous check of
///     `Value::Scalar { .. }` — it pins the solver result and would catch bugs where
///     the wrong solver call is dispatched or where x is an incorrect scalar.
#[test]
fn apply_concurrent_edit_persists_resolved_params_to_param_overrides() {
    let x_id = ValueCellId::new("S", "x");
    let a_id = ValueCellId::new("S", "a");

    // Three solver results: cold eval, concurrent resolve, second eval.
    let mut solved1 = HashMap::new();
    solved1.insert(x_id.clone(), mm(5.0));
    let mut solved2 = HashMap::new();
    solved2.insert(x_id.clone(), mm(20.0));
    let mut solved3 = HashMap::new();
    solved3.insert(x_id.clone(), mm(99.0));

    let solver = SequencedMockConstraintSolver::new(vec![
        SolveResult::Solved {
            values: solved1,
            unique: true,
        },
        SolveResult::Solved {
            values: solved2,
            unique: true,
        },
        SolveResult::Solved {
            values: solved3,
            unique: true,
        },
    ]);

    let module = build_auto_param_module();
    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(solver));

    // Cold eval: solver call 1 → x = mm(5.0)
    let _cold = engine.eval(&module);

    // prepare → resolve → apply: solver call 2 → x = mm(20.0)
    let setup = engine
        .prepare_concurrent_edit(a_id.clone(), mm(8.0))
        .expect("prepare_concurrent_edit should succeed");

    let mut result = empty_result_from_setup(&setup);
    engine.resolve_concurrent_edit(&setup, &mut result);
    engine.apply_concurrent_edit(&setup, result);

    // (a) Snapshot immediately after apply must carry x = mm(20.0)
    let snap = engine.snapshot().expect("snapshot must exist after apply");
    let (snap_x, _) = snap.values.get(&x_id).expect("x must be in snapshot");
    assert!(
        matches!(snap_x, Value::Scalar { si_value, .. } if (*si_value - 0.02).abs() < 1e-10),
        "expected snapshot x = mm(20.0) = 0.02 SI after apply, got {:?}",
        snap_x
    );

    // (b) A subsequent eval() must return x == mm(99.0) exactly (solver call 3).
    //     Derive expected SI from mm(99.0) so the assertion stays in sync with the
    //     solver setup if unit conversion semantics ever change.
    let Value::Scalar {
        si_value: expected_si,
        ..
    } = mm(99.0)
    else {
        unreachable!("mm() always returns Value::Scalar")
    };
    let second = engine.eval(&module);
    let second_x = second
        .values
        .get(&x_id)
        .expect("x must be in second eval values");
    assert!(
        matches!(second_x, Value::Scalar { si_value, .. } if (*si_value - expected_si).abs() < 1e-10),
        "expected second eval x == mm(99.0) (from solver call 3), got {:?}",
        second_x
    );
}

/// step-13: resolve_concurrent_edit skips the solver when the dirty cone from the
/// changed cell does NOT include any constraint that references the auto param.
/// This exercises the `constraints_dirty` guard inside `resolve_concurrent_edit`,
/// which short-circuits the solver invocation when no relevant constraint is dirty.
///
/// Template: auto x (length), constraint 0: x > 2mm (no param ref), param a (mm(3.0)),
/// let b = a*2. Changing `a` dirties b but NOT constraint 0 (which only reads x,
/// a literal). The solver must NOT be called during resolve_concurrent_edit.
///
/// We use MultiCallSpyConstraintSolver and assert call_count() == 1 after resolve
/// (still the cold-eval call only).
#[test]
fn resolve_concurrent_edit_skips_solve_when_no_auto_group_constraints_are_dirty() {
    let x_id = ValueCellId::new("Q", "x");
    let a_id = ValueCellId::new("Q", "a");

    // Template: auto x, constraint x > 2mm (literal, no reference to a),
    //           param a = mm(3.0), let b = a * 2.
    let template = TopologyTemplateBuilder::new("Q")
        .auto_param("Q", "x", Type::length())
        // constraint references only x (not a) and a literal mm(2.0)
        .constraint("Q", 0, None, gt(value_ref("Q", "x"), literal(mm(2.0))))
        .param("Q", "a", Type::length(), Some(literal(mm(3.0))))
        .let_binding(
            "Q",
            "b",
            Type::length(),
            binop(BinOp::Mul, value_ref("Q", "a"), literal(Value::Real(2.0))),
        )
        .build();
    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    // Spy solver:
    //   call 1 (cold eval)       → x = mm(5.0)
    //   call 2 (erroneous solve) → x = mm(999.0)  ← bogus; leaks into result if guard fails
    let mut cold_solved: HashMap<ValueCellId, Value> = HashMap::new();
    cold_solved.insert(x_id.clone(), mm(5.0));

    let mut bogus_solved: HashMap<ValueCellId, Value> = HashMap::new();
    bogus_solved.insert(x_id.clone(), mm(999.0));

    let spy = MultiCallSpyConstraintSolver::new(vec![
        SolveResult::Solved {
            values: cold_solved,
            unique: true,
        },
        // If the solver is incorrectly called a second time, mm(999.0) would
        // appear in result.resolved_params — any emptiness assertion would fail loudly.
        SolveResult::Solved {
            values: bogus_solved,
            unique: true,
        },
    ]);

    // Capture the shared call-counter handle BEFORE moving spy into the engine.
    let captured = spy.captured_problems();

    let mut engine =
        Engine::new(Box::new(MockConstraintChecker::new()), None).with_solver(Box::new(spy));

    // Cold eval: solver call 1 → x = mm(5.0)
    let _cold = engine.eval(&module);

    // prepare_concurrent_edit: change a → mm(5.0)
    // Dirty cone from a: b (let binding) — constraint 0 does NOT reference a.
    let setup = engine
        .prepare_concurrent_edit(a_id.clone(), mm(5.0))
        .expect("prepare_concurrent_edit should succeed");

    let mut result = empty_result_from_setup(&setup);

    // resolve_concurrent_edit: constraint 0 is NOT in the dirty cone from a,
    // so the `if !constraints_dirty { continue; }` guard fires → solver NOT called.
    engine.resolve_concurrent_edit(&setup, &mut result);

    // (a) The solver must have been called exactly once (cold eval only).
    //     If the constraints_dirty guard fails, a second call would push a second
    //     entry into `captured` AND leak mm(999.0) into result.resolved_params.
    assert_eq!(
        captured.lock().unwrap().len(),
        1,
        "solver should have been called exactly once (cold eval), but was called {} times \
         — the constraints_dirty guard inside resolve_concurrent_edit is not firing",
        captured.lock().unwrap().len()
    );

    // (b) result.resolved_params is empty (no solver ran during resolve).
    assert!(
        result.resolved_params.is_empty(),
        "resolved_params should be empty when no constraint is dirty (got {:?})",
        result.resolved_params
    );
    // (c) result.diagnostics is empty.
    assert!(
        result.diagnostics.is_empty(),
        "diagnostics should be empty when no constraint is dirty (got {:?})",
        result.diagnostics
    );
}

/// Verifies that `apply_concurrent_edit` records the `eval_duration` from
/// `ConcurrentNodeResult` into the journal Completed event's `Duration` payload
/// (not apply-loop wall time).
///
/// The implementation uses `node_result.eval_duration.unwrap_or_else(|| start.elapsed())`,
/// so when `eval_duration` is `Some`, the journal entry reflects the value supplied by
/// the node evaluator. The fallback path is covered by a separate test.
#[test]
fn apply_concurrent_edit_journal_uses_eval_duration() {
    use std::time::Duration;

    use reify_eval::journal::{EventKind, EventPayload};

    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    let _initial = engine.eval(&module);

    let e = "Bracket";
    let width_id = ValueCellId::new(e, "width");
    let volume_id = ValueCellId::new(e, "volume");
    let volume_node = NodeId::Value(volume_id.clone());

    let setup = engine
        .prepare_concurrent_edit(width_id.clone(), Value::length(0.1))
        .unwrap();

    let new_volume = Value::Scalar {
        si_value: 5e-5,
        dimension: reify_core::dimension::DimensionVector::VOLUME,
    };

    let known_eval_duration = Duration::from_millis(100);

    let mut snapshot_values = setup.snapshot_values.clone();
    snapshot_values.insert(
        volume_id.clone(),
        (new_volume.clone(), DeterminacyState::Determined),
    );

    let mut values = setup.values.clone();
    values.insert(volume_id.clone(), new_volume.clone());

    let node_results = vec![ConcurrentNodeResult {
        node: volume_node.clone(),
        value: new_volume.clone(),
        determinacy: DeterminacyState::Determined,
        trace: DependencyTrace::default(),
        outcome: reify_eval::cache::EvalOutcome::Changed,
        eval_duration: Some(known_eval_duration),
    }];

    let result = ConcurrentEditResult {
        values,
        snapshot_values,
        node_results,
        actual_eval_set: vec![volume_node.clone()],
        skipped: HashSet::new(),
        resolved_params: std::collections::HashMap::new(),
        diagnostics: Vec::new(),
    };

    engine.apply_concurrent_edit(&setup, result);

    // Find the Completed event for volume at setup.version
    let volume_events = engine.journal().events_for_node(&volume_node);
    let completed_event = volume_events
        .iter()
        .filter(|ev| ev.version == setup.version)
        .find(|ev| matches!(ev.kind, EventKind::Completed { .. }))
        .expect("should have a Completed event for volume");

    // The Duration payload must equal the eval_duration we supplied via node_result.
    // The `unwrap_or_else(|| start.elapsed())` fallback only fires when eval_duration
    // is None, which is covered by a separate test.
    match &completed_event.payload {
        Some(EventPayload::Duration(d)) => {
            assert_eq!(
                *d, known_eval_duration,
                "Completed event Duration should be the eval_duration from ConcurrentNodeResult, \
                 not apply-loop time. Got: {:?}",
                d
            );
        }
        other => panic!("Expected Duration payload, got: {:?}", other),
    }
}
