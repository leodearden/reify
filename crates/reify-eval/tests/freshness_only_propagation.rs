//! Integration test for `reify_eval::freshness_walk::propagate_freshness_only`
//! over a real Engine cache populated by `Engine::eval()`.
//!
//! Pins arch §3.5 lines 432-436 at the integration level: when an upstream
//! node's value is unchanged but its freshness flips Intermediate → Final,
//! the walk propagates Final downstream WITHOUT invoking the value
//! evaluator. The "no value evaluator calls fired" assertion is enforced
//! by snapshotting the downstream entry's `result_hash` and inner `Value`
//! before/after the walk and asserting byte-identical equality.

use reify_core::{ContentHash, ModulePath, Type, ValueCellId};
use reify_eval::Engine;
use reify_eval::cache::{CachedResult, NodeId};
use reify_eval::freshness_walk;
use reify_ir::{BinOp, ErrorRef, Freshness, ResultRef, Value};
use reify_test_support::builders::{binop, literal, value_ref_typed};
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::{CompiledModuleBuilder, TopologyTemplateBuilder};
use std::collections::HashSet;

/// Build the 2-cell synthetic module: param `a` + let `b = a * 2.0`.
///
/// Identical to the fixture in `tests/freshness_propagation.rs:14-33` so a
/// future refactor of the fixture is caught by both files in the same change.
fn two_cell_module() -> reify_compiler::CompiledModule {
    let e = "T";
    CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(
            TopologyTemplateBuilder::new(e)
                .param(e, "a", Type::dimensionless_scalar(), Some(literal(Value::Real(5.0))))
                .let_binding(
                    e,
                    "b",
                    Type::dimensionless_scalar(),
                    binop(
                        BinOp::Mul,
                        value_ref_typed(e, "a", Type::dimensionless_scalar()),
                        literal(Value::Real(2.0)),
                    ),
                )
                .build(),
        )
        .build()
}

/// Cold-start `Engine::eval()` to populate the cache, then synthetically
/// inject `a → Intermediate{1}` and `b → Intermediate{1}` (the post-eval
/// state is all-Final, so we have to manufacture the non-Final input case).
/// Snapshot b's `result_hash` and inner `Value` BEFORE flipping a back to
/// Final and running `propagate_freshness_only` from `{a}`.
///
/// Asserts:
/// - The walk's returned `updated` set contains `Value(b)`.
/// - b's freshness is `Final` (the upstream Intermediate→Final transition
///   propagated through the let-binding edge).
/// - b's `result_hash` is byte-identical to the pre-walk snapshot.
/// - b's inner `Value` (extracted via `CachedResult::Value(_, _)`) is
///   byte-identical to the pre-walk snapshot.
///
/// Steps 3-4 together pin the "no value evaluator calls fired" invariant
/// the task description mandates: the value evaluator would have computed
/// `b = a * 2.0 = 10.0` and updated `result_hash` accordingly, so a
/// byte-identical snapshot is the strongest possible witness that the
/// walk only touched `freshness`.
#[test]
fn walk_over_engine_propagates_intermediate_to_final_without_value_recomputation() {
    let module = two_cell_module();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    // Cold-start eval populates the cache and `eval_state` (with
    // `reverse_index`); after eval all params/let-bindings are Final.
    engine.eval(&module);

    let e = "T";
    let a_id = ValueCellId::new(e, "a");
    let b_id = ValueCellId::new(e, "b");
    let a_node = NodeId::Value(a_id.clone());
    let b_node = NodeId::Value(b_id.clone());

    // Snapshot b's entry BEFORE any freshness manipulation. b's result_hash
    // and Value are what we use as the "no value evaluator ran" witness.
    let b_before = engine
        .cache_store()
        .get(&b_node)
        .expect("b cached after eval")
        .clone();
    let b_before_hash = b_before.result_hash;
    let b_before_value = match &b_before.result {
        CachedResult::Value(v, _) => v.clone(),
        other => panic!("expected b to have CachedResult::Value, got {:?}", other),
    };

    // Inject the synthetic non-Final state: both a and b are Intermediate{1}.
    // This simulates the mid-eval state where an upstream propagation has
    // marked the chain Intermediate but the value cache is already settled.
    {
        let cs = engine.cache_store_mut();
        assert!(
            cs.set_freshness(&a_node, Freshness::Intermediate { generation: 1 }),
            "a must exist in the cache after eval"
        );
        assert!(
            cs.set_freshness(&b_node, Freshness::Intermediate { generation: 1 }),
            "b must exist in the cache after eval"
        );
    }

    // Flip a to Final — the edge that the freshness-only walk follows.
    assert!(
        engine
            .cache_store_mut()
            .set_freshness(&a_node, Freshness::Final),
        "a must still be in the cache"
    );

    // Clone the reverse_index out of `eval_state()` so the immutable
    // borrow is released before we hand `cache_store_mut()` (a mutable
    // borrow on the same `Engine`) to the walk. The borrow checker
    // forbids holding `&engine.eval_state()` and `engine.cache_store_mut()`
    // simultaneously, so a clone is the simplest way to thread the
    // reverse_index through to the walk.
    //
    // `ReverseDependencyIndex` is `Clone`-able specifically to support
    // this idiom (see `concurrent.rs:172` for another instance). When
    // this walk is eventually wired into `engine_edit.rs::edit_param`
    // (out-of-scope for task #2335 — see plan design-decision #5), the
    // call site will own both `&mut self.cache` and `&self.eval_state`
    // through the `Engine` struct's interior fields and the clone won't
    // be necessary.
    let reverse_index_clone = engine
        .eval_state()
        .expect("eval_state populated by Engine::eval")
        .reverse_index
        .clone();
    // P3.3 step-16: clone the graph as well so the walk can look up
    // `compute_nodes[cn_id].output_value_cells` for edge #12 fan-out.
    // Same borrow-checker rationale as the reverse_index clone above.
    let graph_clone = engine
        .eval_state()
        .expect("eval_state populated by Engine::eval")
        .snapshot
        .graph
        .clone();

    let mut changed = HashSet::new();
    changed.insert(a_id.clone());

    let updated = freshness_walk::propagate_freshness_only(
        engine.cache_store_mut(),
        &reverse_index_clone,
        &graph_clone,
        &changed,
        1,
    );

    // (i) The walk's return set must include b — propagation through the
    //     a→b edge fired.
    assert!(
        updated.contains(&b_node),
        "updated must contain Value(b), got: {:?}",
        updated
    );

    // (ii) b's freshness must now be Final (Intermediate → Final
    //      propagation through the let-binding edge).
    assert_eq!(
        engine.cache_store().freshness(&b_node),
        Freshness::Final,
        "b must be Final after the walk (Intermediate → Final propagation)"
    );

    // Snapshot b's entry AFTER the walk and assert "no value evaluator
    // calls fired" by checking byte-identical equality of the value-bearing
    // fields against the pre-walk snapshot.
    let b_after = engine
        .cache_store()
        .get(&b_node)
        .expect("b still cached")
        .clone();
    assert_eq!(
        b_after.result_hash, b_before_hash,
        "b's result_hash must be byte-identical (the walk MUST NOT recompute values)"
    );
    let b_after_value = match &b_after.result {
        CachedResult::Value(v, _) => v.clone(),
        other => panic!(
            "expected b to still have CachedResult::Value, got {:?}",
            other
        ),
    };
    assert_eq!(
        b_after_value, b_before_value,
        "b's cached Value must be byte-identical (no value evaluator calls fired)"
    );
}

/// I1 — Integration: when the upstream cell `a` in a real Engine cache is
/// Failed, the freshness-only walk must propagate `Pending` downstream to `b`
/// and record the Failed `a` as the diagnostic-chain root via `pending_cause`.
///
/// Setup mirrors `walk_over_engine_propagates_intermediate_to_final_without_value_recomputation`
/// (two_cell_module + Engine::eval cold-start + reverse_index clone idiom) up
/// to the freshness-injection step; only that step and the assertion target
/// differ.
///
/// Diverges at injection: `a` is marked Failed via `mark_failed` (contrast with
/// the Intermediate injection in the existing test). The walk then propagates
/// from Failed `a` to `b` via the `a→b` let-binding edge in the Engine cache's
/// reverse_index.
///
/// Post-walk assertions pin arch §9.2 (Failed upstream → downstream Pending
/// with cause = the Failed node's own NodeId) at integration level over a real
/// Engine cache — the unit-test counterpart is
/// `failed_upstream_propagates_pending_with_cause` in `freshness_walk.rs:809`.
#[test]
fn walk_over_engine_propagates_failed_upstream_to_pending_with_cause() {
    let module = two_cell_module();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    // Cold-start eval: a and b are both Final after eval.
    engine.eval(&module);

    let e = "T";
    let a_id = ValueCellId::new(e, "a");
    let b_id = ValueCellId::new(e, "b");
    let a_node = NodeId::Value(a_id.clone());
    let b_node = NodeId::Value(b_id.clone());

    // Snapshot b's entry BEFORE any freshness manipulation — used as the
    // "no value evaluator ran" witness (result_hash + inner Value).
    let b_before = engine
        .cache_store()
        .get(&b_node)
        .expect("b cached after eval")
        .clone();
    let b_before_hash: ContentHash = b_before.result_hash;
    let b_before_value = match &b_before.result {
        CachedResult::Value(v, _) => v.clone(),
        other => panic!("expected CachedResult::Value, got {:?}", other),
    };

    // Freshness injection: mark `a` as Failed (diverges from the Intermediate
    // injection in the existing integration test).
    assert!(
        engine
            .cache_store_mut()
            .mark_failed(&a_node, ErrorRef::new("synthetic kernel failure")),
        "mark_failed must succeed — a is in the cache after eval"
    );

    // Clone the reverse_index out of `eval_state()` so the immutable borrow
    // releases before `cache_store_mut()` is called (same borrow-checker
    // rationale as the existing integration test at lines 116-130).
    let reverse_index_clone = engine
        .eval_state()
        .expect("eval_state populated by Engine::eval")
        .reverse_index
        .clone();
    // P3.3 step-16: clone the graph as well so the walk can look up
    // `compute_nodes[cn_id].output_value_cells` for edge #12 fan-out.
    // Same borrow-checker rationale as the reverse_index clone above.
    let graph_clone = engine
        .eval_state()
        .expect("eval_state populated by Engine::eval")
        .snapshot
        .graph
        .clone();

    let mut changed = HashSet::new();
    changed.insert(a_id.clone());

    let updated = freshness_walk::propagate_freshness_only(
        engine.cache_store_mut(),
        &reverse_index_clone,
        &graph_clone,
        &changed,
        1,
    );

    // (i) b must be in the updated set — walk propagated through the a→b edge.
    assert!(
        updated.contains(&b_node),
        "updated must contain Value(b), got: {:?}",
        updated
    );

    // (ii) b's freshness must be Pending with last_substantive = b_before_hash
    //      (arch §9.2: mark_pending_with_cause captures the cached hash).
    assert_eq!(
        engine.cache_store().freshness(&b_node),
        Freshness::Pending {
            last_substantive: ResultRef::of_hash(b_before_hash),
        },
        "b must be Pending with last_substantive = ResultRef::of_hash(b_before_hash)"
    );

    // (iii) b's pending_cause must point at a — Failed a is the chain root
    //       (cache.rs:977-982: Failed inputs contribute their own NodeId).
    assert_eq!(
        engine.cache_store().pending_cause(&b_node),
        Some(a_node),
        "b's pending_cause must be Some(Value(a)) — Failed a is the chain root"
    );

    // (iv) b's result_hash must be byte-identical — no value recomputation.
    assert_eq!(
        engine.cache_store().get(&b_node).unwrap().result_hash,
        b_before_hash,
        "b's result_hash must be byte-identical (the walk MUST NOT recompute values)"
    );

    // (v) b's extracted Value must be byte-identical — no evaluator fired.
    let b_after_value = match &engine.cache_store().get(&b_node).unwrap().result {
        CachedResult::Value(v, _) => v.clone(),
        other => panic!("expected CachedResult::Value after walk, got {:?}", other),
    };
    assert_eq!(
        b_after_value, b_before_value,
        "b's cached Value must be byte-identical (no value evaluator calls fired)"
    );
}

/// I2 — Integration: when the upstream cell `a` is Pending with a synthetic
/// chain root, the freshness-only walk must forward that chain root verbatim
/// to downstream `b` — NOT set `b.pending_cause = Some(Value(a))`.
///
/// Contrasts with I1 (`walk_over_engine_propagates_failed_upstream_to_pending_with_cause`):
/// - **I1** (Failed upstream): `b.pending_cause = Some(Value(a))` — the failing
///   node's own NodeId becomes the chain root (cache.rs:977-982).
/// - **I2** (Pending upstream): `b.pending_cause = Some(synthetic_chain_root)` —
///   the upstream cause forwards verbatim (arch §7.2 line 748 /
///   cache.rs:965-999). `Value(a)` is NOT the chain root here.
///
/// Using a non-cached synthetic NodeId as the chain root isolates the test to
/// the forwarding property: "does b.pending_cause == a.pending_cause?" without
/// conflating with whether the chain root NodeId resolves to a real cache entry.
///
/// Pins both shapes of the §9.2/§7.2 chain semantics over a real Engine cache.
#[test]
fn walk_over_engine_forwards_pending_cause_through_chain() {
    let module = two_cell_module();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    // Cold-start eval: a and b are both Final after eval.
    engine.eval(&module);

    let e = "T";
    let a_id = ValueCellId::new(e, "a");
    let b_id = ValueCellId::new(e, "b");
    let a_node = NodeId::Value(a_id.clone());
    let b_node = NodeId::Value(b_id.clone());

    // Snapshot b's entry BEFORE any freshness manipulation.
    let b_before = engine
        .cache_store()
        .get(&b_node)
        .expect("b cached after eval")
        .clone();
    let b_before_hash: ContentHash = b_before.result_hash;
    let b_before_value = match &b_before.result {
        CachedResult::Value(v, _) => v.clone(),
        other => panic!("expected CachedResult::Value, got {:?}", other),
    };

    // Freshness injection: mark `a` as Pending with a synthetic chain root.
    // The synthetic chain root does NOT exist in the cache — mark_pending_with_cause
    // accepts any NodeId as cause (cache.rs:581-590 just stores the value).
    // Using a non-cached NodeId isolates the "forwarding verbatim" property.
    let synthetic_chain_root = NodeId::Value(ValueCellId::new("synthetic_chain_root", "x"));
    assert!(
        engine
            .cache_store_mut()
            .mark_pending_with_cause(&a_node, synthetic_chain_root.clone()),
        "mark_pending_with_cause must succeed — a is in the cache after eval"
    );

    // Sanity: a's pending_cause is the synthetic root before the walk.
    assert_eq!(
        engine.cache_store().pending_cause(&a_node),
        Some(synthetic_chain_root.clone()),
        "sanity: a's pending_cause must be the synthetic chain root before the walk"
    );

    // Clone the reverse_index (same borrow-checker rationale as other integration tests).
    let reverse_index_clone = engine
        .eval_state()
        .expect("eval_state populated by Engine::eval")
        .reverse_index
        .clone();
    // P3.3 step-16: clone the graph as well so the walk can look up
    // `compute_nodes[cn_id].output_value_cells` for edge #12 fan-out.
    // Same borrow-checker rationale as the reverse_index clone above.
    let graph_clone = engine
        .eval_state()
        .expect("eval_state populated by Engine::eval")
        .snapshot
        .graph
        .clone();

    let mut changed = HashSet::new();
    changed.insert(a_id.clone());

    let updated = freshness_walk::propagate_freshness_only(
        engine.cache_store_mut(),
        &reverse_index_clone,
        &graph_clone,
        &changed,
        1,
    );

    // (i) b must be in the updated set — walk propagated through the a→b edge.
    assert!(
        updated.contains(&b_node),
        "updated must contain Value(b), got: {:?}",
        updated
    );

    // (ii) b's freshness must be Pending with last_substantive = b_before_hash
    //      (mark_pending_with_cause captures b's cached hash, NOT a's).
    assert_eq!(
        engine.cache_store().freshness(&b_node),
        Freshness::Pending {
            last_substantive: ResultRef::of_hash(b_before_hash),
        },
        "b must be Pending with last_substantive = ResultRef::of_hash(b_before_hash)"
    );

    // (iii) CRITICAL: b's pending_cause must be the synthetic chain root, NOT
    //       Some(Value(a)). Pending-upstream case forwards the upstream cause
    //       verbatim (arch §7.2 line 748 / cache.rs:965-999).
    assert_eq!(
        engine.cache_store().pending_cause(&b_node),
        Some(synthetic_chain_root),
        "b's pending_cause must be Some(synthetic_chain_root) — forwarded from a's \
         pending_cause, NOT Some(Value(a))"
    );

    // (iv) b's result_hash must be byte-identical — no value recomputation.
    assert_eq!(
        engine.cache_store().get(&b_node).unwrap().result_hash,
        b_before_hash,
        "b's result_hash must be byte-identical (the walk MUST NOT recompute values)"
    );

    // (v) b's extracted Value must be byte-identical — no evaluator fired.
    let b_after_value = match &engine.cache_store().get(&b_node).unwrap().result {
        CachedResult::Value(v, _) => v.clone(),
        other => panic!("expected CachedResult::Value after walk, got {:?}", other),
    };
    assert_eq!(
        b_after_value, b_before_value,
        "b's cached Value must be byte-identical (no value evaluator calls fired)"
    );
}
