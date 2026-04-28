//! Integration test for `reify_eval::freshness_walk::propagate_freshness_only`
//! over a real Engine cache populated by `Engine::eval()`.
//!
//! Pins arch §3.5 lines 432-436 at the integration level: when an upstream
//! node's value is unchanged but its freshness flips Intermediate → Final,
//! the walk propagates Final downstream WITHOUT invoking the value
//! evaluator. The "no value evaluator calls fired" assertion is enforced
//! by snapshotting the downstream entry's `result_hash` and inner `Value`
//! before/after the walk and asserting byte-identical equality.

use reify_eval::Engine;
use reify_eval::cache::{CachedResult, NodeId};
use reify_eval::freshness_walk;
use reify_test_support::builders::{binop, literal, value_ref_typed};
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::{CompiledModuleBuilder, TopologyTemplateBuilder};
use reify_types::{BinOp, Freshness, ModulePath, Type, Value, ValueCellId};
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
                .param(e, "a", Type::Real, Some(literal(Value::Real(5.0))))
                .let_binding(
                    e,
                    "b",
                    Type::Real,
                    binop(
                        BinOp::Mul,
                        value_ref_typed(e, "a", Type::Real),
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
    let b_before = engine.cache_store().get(&b_node).expect("b cached after eval").clone();
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

    // Clone the reverse_index out of `eval_state()` so the borrow is
    // released before we hand `cache_store_mut()` to the walk. (The
    // borrow checker forbids holding `&engine.eval_state()` and
    // `&mut engine.cache_store_mut()` simultaneously — see plan
    // design-decision §step-15 in `.task/plan.json`.)
    let reverse_index_clone = engine
        .eval_state()
        .expect("eval_state populated by Engine::eval")
        .reverse_index
        .clone();

    let mut changed = HashSet::new();
    changed.insert(a_id.clone());

    let updated = freshness_walk::propagate_freshness_only(
        engine.cache_store_mut(),
        &reverse_index_clone,
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
    let b_after = engine.cache_store().get(&b_node).expect("b still cached").clone();
    assert_eq!(
        b_after.result_hash, b_before_hash,
        "b's result_hash must be byte-identical (the walk MUST NOT recompute values)"
    );
    let b_after_value = match &b_after.result {
        CachedResult::Value(v, _) => v.clone(),
        other => panic!("expected b to still have CachedResult::Value, got {:?}", other),
    };
    assert_eq!(
        b_after_value, b_before_value,
        "b's cached Value must be byte-identical (no value evaluator calls fired)"
    );
}
