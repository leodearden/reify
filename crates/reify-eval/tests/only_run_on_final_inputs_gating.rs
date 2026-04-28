//! Integration test pinning the full freshness-walk → gating-helpers unblock loop.
//!
//! Exercises the composition of:
//! - `reify_eval::freshness_walk::propagate_freshness_only` (from task #2335)
//! - `reify_eval::gating::has_intermediate_inputs` and `unblocked_gated_nodes`
//!   (from task #2356)
//!
//! over a real `Engine` cache populated by cold `Engine::eval()`.
//!
//! ## Scenario
//!
//! Three-cell synthetic module: `param a = 5.0`, `param c = 3.0`, `let b = a + c`.
//! After cold eval all three cells are `Final`. We synthetically inject a
//! mixed-freshness state (`a=Intermediate{1}`, `c=Intermediate{1}`, `b=Pending`)
//! to simulate the mid-cycle state that the `OnlyRunOnFinalInputs` scheduler
//! policy must gate on, then step through two `propagate_freshness_only` passes
//! to unblock `b`.
//!
//! ## Key invariants pinned
//!
//! - `has_intermediate_inputs(b) = true` while any input is non-Final.
//! - `unblocked_gated_nodes([b]) = {}` until all inputs are Final.
//! - After both inputs become Final via the freshness walk,
//!   `has_intermediate_inputs(b) = false` and `unblocked_gated_nodes([b]) = {b}`.
//! - b's `result_hash` and cached `Value` are byte-identical to the pre-injection
//!   snapshot throughout — no value evaluator ran during the walk-driven unblock
//!   (arch §3.5 line 432: "the input hash for downstream nodes is unchanged, so
//!   no value recomputation occurs").
//!
//! See arch §7.3 lines 762-767 (`OnlyRunOnFinalInputs` policy) and §3.5 line 436
//! ("freshness propagation can unlock gated work").

use reify_eval::Engine;
use reify_eval::cache::{CachedResult, NodeId};
use reify_eval::freshness_walk;
use reify_eval::gating;
use reify_test_support::builders::{binop, literal, value_ref_typed};
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::{CompiledModuleBuilder, TopologyTemplateBuilder};
use reify_types::{BinOp, Freshness, ModulePath, Type, Value, ValueCellId};
use std::collections::HashSet;

/// Build the 3-cell synthetic module: param `a = 5.0`, param `c = 3.0`,
/// `let b: Real = a + c`.
///
/// Mirrors the 2-cell fixture from `freshness_only_propagation.rs` extended by
/// one extra param to demonstrate the "both inputs must become Final" gating
/// requirement.
fn three_cell_module() -> reify_compiler::CompiledModule {
    let e = "T";
    CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(
            TopologyTemplateBuilder::new(e)
                .param(e, "a", Type::Real, Some(literal(Value::Real(5.0))))
                .param(e, "c", Type::Real, Some(literal(Value::Real(3.0))))
                .let_binding(
                    e,
                    "b",
                    Type::Real,
                    binop(
                        BinOp::Add,
                        value_ref_typed(e, "a", Type::Real),
                        value_ref_typed(e, "c", Type::Real),
                    ),
                )
                .build(),
        )
        .build()
}

/// Full freshness-walk → gating-helpers unblock loop over a real Engine cache.
///
/// Steps:
/// 1. Cold-eval populates the cache (all cells Final, b = 8.0).
/// 2. Inject mixed-freshness: a=Intermediate{1}, c=Intermediate{1}, b=Pending.
/// 3. Assert gating helpers block b before any walk.
/// 4. Flip a→Final; run `propagate_freshness_only` from {a}.
///    - b transitions Pending → Intermediate{1} (c still blocks), appears in `updated`.
///    - `has_intermediate_inputs(b)` is still true; `unblocked_gated_nodes` is empty.
/// 5. Flip c→Final; run `propagate_freshness_only` from {c}.
///    - b transitions Intermediate{1} → Final; `updated` contains b.
///    - `has_intermediate_inputs(b)` = false; `unblocked_gated_nodes([b])` = {b}.
/// 6. Witness: b's `result_hash` and `Value` are byte-identical to pre-injection
///    snapshot — no value evaluator was invoked during either walk.
#[test]
fn freshness_walk_unblocks_gated_node_when_all_inputs_become_final() {
    let module = three_cell_module();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    // ── 1. Cold-start eval: all cells become Final ─────────────────────────
    engine.eval(&module);

    let e = "T";
    let a_id = ValueCellId::new(e, "a");
    let b_id = ValueCellId::new(e, "b");
    let c_id = ValueCellId::new(e, "c");
    let a_node = NodeId::Value(a_id.clone());
    let b_node = NodeId::Value(b_id.clone());
    let c_node = NodeId::Value(c_id.clone());

    // Verify post-eval state: all Final.
    assert_eq!(
        engine.cache_store().freshness(&a_node),
        Freshness::Final,
        "a must be Final after cold eval"
    );
    assert_eq!(
        engine.cache_store().freshness(&c_node),
        Freshness::Final,
        "c must be Final after cold eval"
    );
    assert_eq!(
        engine.cache_store().freshness(&b_node),
        Freshness::Final,
        "b must be Final after cold eval"
    );

    // Snapshot b's result_hash and Value BEFORE any freshness manipulation.
    // These are the "no value evaluator ran" witness (see step 6).
    let b_snap = engine
        .cache_store()
        .get(&b_node)
        .expect("b cached after eval")
        .clone();
    let b_snap_hash = b_snap.result_hash;
    let b_snap_value = match &b_snap.result {
        CachedResult::Value(v, _) => v.clone(),
        other => panic!("expected b to hold CachedResult::Value after eval, got {:?}", other),
    };
    // Sanity check: b = a + c = 5.0 + 3.0 = 8.0.
    assert_eq!(b_snap_value, Value::Real(8.0), "b must equal 8.0 after cold eval");

    // ── 2. Inject mixed-freshness state ────────────────────────────────────
    {
        let cs = engine.cache_store_mut();
        assert!(
            cs.set_freshness(&a_node, Freshness::Intermediate { generation: 1 }),
            "a must exist in the cache after eval"
        );
        assert!(
            cs.set_freshness(&c_node, Freshness::Intermediate { generation: 1 }),
            "c must exist in the cache after eval"
        );
        // Use the canonical `mark_pending` writer (not `set_freshness`) so
        // `last_substantive` is derived from b's existing `result_hash` and
        // `pending_transition_count` is incremented.
        assert!(
            cs.mark_pending(&b_node),
            "b must exist in the cache after eval"
        );
    }

    // Verify injected state.
    assert_eq!(
        engine.cache_store().freshness(&b_node),
        Freshness::Pending { last_substantive: reify_types::ResultRef::of_hash(b_snap_hash) },
        "b must be Pending after injection"
    );

    // ── 3. Build gated set; assert gating blocks b before any walk ─────────
    let gated = vec![b_node.clone()];

    assert!(
        gating::has_intermediate_inputs(engine.cache_store(), &b_node),
        "b has Intermediate inputs (a, c) before any walk"
    );
    assert!(
        gating::unblocked_gated_nodes(engine.cache_store(), &gated).is_empty(),
        "b must be blocked (not in unblocked set) before any walk"
    );

    // ── 4. First walk: flip a→Final, propagate from {a} ───────────────────
    // Clone the reverse_index before taking `cache_store_mut()` to satisfy
    // the borrow checker (same idiom as `freshness_only_propagation.rs:126-130`).
    let reverse_index = engine
        .eval_state()
        .expect("eval_state populated by Engine::eval")
        .reverse_index
        .clone();

    assert!(
        engine
            .cache_store_mut()
            .set_freshness(&a_node, Freshness::Final),
        "a must still be in the cache"
    );

    let mut changed_a = HashSet::new();
    changed_a.insert(a_id.clone());

    let updated_after_a = freshness_walk::propagate_freshness_only(
        engine.cache_store_mut(),
        &reverse_index,
        &changed_a,
        1,
    );

    // After a→Final, b transitions Pending → Intermediate{1} because c is
    // still Intermediate. The walk writes b (Pending ≠ Intermediate) so b
    // IS in updated. Propagation stops at b (no further dependents).
    assert!(
        updated_after_a.contains(&b_node),
        "b must be in updated after a→Final walk (Pending → Intermediate[1]); got: {:?}",
        updated_after_a
    );
    assert_eq!(
        engine.cache_store().freshness(&b_node),
        Freshness::Intermediate { generation: 1 },
        "b must be Intermediate{{1}} after a→Final walk (c still blocks)"
    );

    // Gating helpers still block b (c is still Intermediate).
    assert!(
        gating::has_intermediate_inputs(engine.cache_store(), &b_node),
        "b still has Intermediate input (c) after first walk"
    );
    assert!(
        gating::unblocked_gated_nodes(engine.cache_store(), &gated).is_empty(),
        "b must still be blocked (c not yet Final)"
    );

    // ── 5. Second walk: flip c→Final, propagate from {c} ──────────────────
    assert!(
        engine
            .cache_store_mut()
            .set_freshness(&c_node, Freshness::Final),
        "c must still be in the cache"
    );

    let mut changed_c = HashSet::new();
    changed_c.insert(c_id.clone());

    let updated_after_c = freshness_walk::propagate_freshness_only(
        engine.cache_store_mut(),
        &reverse_index,
        &changed_c,
        1,
    );

    // Both inputs are now Final → b transitions Intermediate{1} → Final.
    assert!(
        updated_after_c.contains(&b_node),
        "b must be in updated after c→Final walk (Intermediate{{1}} → Final); got: {:?}",
        updated_after_c
    );
    assert_eq!(
        engine.cache_store().freshness(&b_node),
        Freshness::Final,
        "b must be Final after both inputs are Final"
    );

    // Gating helpers now unblock b.
    assert!(
        !gating::has_intermediate_inputs(engine.cache_store(), &b_node),
        "b must have no intermediate inputs after both inputs are Final"
    );
    let unblocked = gating::unblocked_gated_nodes(engine.cache_store(), &gated);
    assert_eq!(
        unblocked,
        std::iter::once(b_node.clone()).collect::<HashSet<_>>(),
        "unblocked_gated_nodes must return {{b}} after both inputs are Final"
    );

    // ── 6. Witness: no value evaluator ran ────────────────────────────────
    // Snapshot b's entry AFTER both walks and assert byte-identical equality
    // of value-bearing fields against the pre-injection snapshot. The value
    // evaluator would have recomputed b = a + c = 8.0 and updated result_hash,
    // so identical snapshots are the strongest possible witness.
    let b_after = engine
        .cache_store()
        .get(&b_node)
        .expect("b still cached after walks")
        .clone();
    assert_eq!(
        b_after.result_hash, b_snap_hash,
        "b's result_hash must be byte-identical (no value evaluator calls fired)"
    );
    let b_after_value = match &b_after.result {
        CachedResult::Value(v, _) => v.clone(),
        other => panic!("expected b to still hold CachedResult::Value, got {:?}", other),
    };
    assert_eq!(
        b_after_value, b_snap_value,
        "b's cached Value must be byte-identical (no value evaluator calls fired)"
    );
}
