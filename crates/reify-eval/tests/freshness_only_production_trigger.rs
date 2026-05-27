//! Integration test for the production trigger path of
//! `reify_eval::freshness_walk::propagate_freshness_only`.
//!
//! Pins the `Engine::propagate_freshness_only` facade (arch §3.5 lines
//! 432-436) at the integration level: calling this new Engine method fires
//! the freshness-only BFS walk, propagating a Pending→Final upstream
//! transition through the full downstream chain WITHOUT invoking the value
//! evaluator on any downstream cell.
//!
//! Scenario: a 3-cell chain `a (Param) → b (let = a * 2) → c (let = b + 1)`.
//! After cold-start eval, all cells are Final. Synthetic injection makes all
//! three Pending; then `a` is flipped back to Final (value unchanged, only
//! freshness changed). Calling `engine.propagate_freshness_only({a_id}, 1)`
//! must propagate Final to `b` and then to `c` via BFS, without invoking
//! the let-binding evaluator on either downstream cell. The "no re-evaluation"
//! property is pinned three ways:
//! 1. Returned-set membership (only `propagate_freshness_only` produces this).
//! 2. Byte-identical `result_hash` + inner `Value` before/after the walk.
//! 3. `set_panic_on_eval` registration on `b` and `c` — any accidental
//!    `evaluate_let_bindings` invocation would panic loudly.

use reify_eval::Engine;
use reify_eval::cache::{CachedResult, NodeId};
use reify_test_support::builders::{binop, literal, value_ref_typed};
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::{CompiledModuleBuilder, TopologyTemplateBuilder};
use reify_core::{ModulePath, Type, ValueCellId};
use reify_ir::{BinOp, Freshness, Value};
use std::collections::HashSet;

/// Build the 3-cell synthetic module: param `a` + let `b = a * 2.0` +
/// let `c = b + 1.0`. The chain exercises BFS propagation past one hop:
/// the walk must push `b_id` onto the frontier (after updating b) and then
/// derive `c`'s freshness from `b`'s updated Final state.
fn three_cell_module() -> reify_compiler::CompiledModule {
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
                .let_binding(
                    e,
                    "c",
                    Type::Real,
                    binop(
                        BinOp::Add,
                        value_ref_typed(e, "b", Type::Real),
                        literal(Value::Real(1.0)),
                    ),
                )
                .build(),
        )
        .build()
}

/// Production-trigger integration test for `Engine::propagate_freshness_only`.
///
/// Pins arch §3.5 lines 432-436 through the Engine's public facade rather
/// than calling `freshness_walk::propagate_freshness_only` directly (which
/// is what `tests/freshness_only_propagation.rs` does). The new Engine method
/// is the "production trigger path" that audit M-013 flags as missing.
///
/// The "no re-evaluation" property is layered three ways:
/// 1. Returned-set membership.
/// 2. Byte-identical `result_hash` + inner `Value` snapshots.
/// 3. `set_panic_on_eval` on `b` and `c` — the evaluator panics if invoked.
#[test]
fn engine_propagate_freshness_only_drives_pending_to_final_downstream_without_re_evaluation() {
    let module = three_cell_module();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    // Cold-start eval: populates cache and eval_state (with reverse_index).
    // After eval, a, b, and c are all Final with concrete Value::Real payloads.
    engine.eval(&module);

    let e = "T";
    let a_id = ValueCellId::new(e, "a");
    let b_id = ValueCellId::new(e, "b");
    let c_id = ValueCellId::new(e, "c");
    let a_node = NodeId::Value(a_id.clone());
    let b_node = NodeId::Value(b_id.clone());
    let c_node = NodeId::Value(c_id.clone());

    // Step 2: Snapshot b and c BEFORE any freshness manipulation.
    // These witnesses pin the "no value evaluator ran" property.
    let b_before = engine
        .cache_store()
        .get(&b_node)
        .expect("b cached after cold-start eval")
        .clone();
    let b_before_hash = b_before.result_hash;
    let b_before_value = match &b_before.result {
        CachedResult::Value(v, _) => v.clone(),
        other => panic!("expected b to have CachedResult::Value, got {:?}", other),
    };

    let c_before = engine
        .cache_store()
        .get(&c_node)
        .expect("c cached after cold-start eval")
        .clone();
    let c_before_hash = c_before.result_hash;
    let c_before_value = match &c_before.result {
        CachedResult::Value(v, _) => v.clone(),
        other => panic!("expected c to have CachedResult::Value, got {:?}", other),
    };

    // Step 3: Synthetic injection — mark all three nodes Pending.
    // This constructs the "upstream Pending + downstream Pending" state
    // that the task description names. The walk's job is to resolve this
    // state when a transitions Pending→Final without a value change.
    {
        let cs = engine.cache_store_mut();
        assert!(
            cs.mark_pending(&a_node),
            "mark_pending(a) must succeed — a is in cache after eval"
        );
        assert!(
            cs.mark_pending(&b_node),
            "mark_pending(b) must succeed — b is in cache after eval"
        );
        assert!(
            cs.mark_pending(&c_node),
            "mark_pending(c) must succeed — c is in cache after eval"
        );
    }

    // Step 4: Flip a back to Final (Pending→Final transition, NO value change).
    // a's cached Value (5.0) is unchanged; only freshness flips.
    assert!(
        engine
            .cache_store_mut()
            .set_freshness(&a_node, Freshness::Final),
        "set_freshness(a, Final) must succeed — a is in cache after eval"
    );

    // Register b and c with the panic-on-eval sentinel: any accidental
    // invocation of `evaluate_let_bindings` on these cells will panic,
    // making the "no re-evaluation" guarantee hard rather than soft.
    engine.set_panic_on_eval(b_id.clone());
    engine.set_panic_on_eval(c_id.clone());

    // Step 5: Production trigger — call the new Engine method.
    // This is the call that will fail to compile until step-2 adds the method.
    let mut changed = HashSet::new();
    changed.insert(a_id.clone());
    let updated = engine.propagate_freshness_only(&changed, 1);

    // Step 6: Assertions.

    // (i) Returned-set membership: walk must have reached both b and c.
    assert!(
        updated.contains(&b_node),
        "updated set must contain Value(b) — walk propagated through a→b edge; got: {:?}",
        updated
    );
    assert!(
        updated.contains(&c_node),
        "updated set must contain Value(c) — walk's BFS frontier propagated b→c; got: {:?}",
        updated
    );

    // (ii) Freshness transitions: both b and c must now be Final.
    assert_eq!(
        engine.cache_store().freshness(&b_node),
        Freshness::Final,
        "b must be Final after the walk (Pending→Final propagation via a→b edge)"
    );
    assert_eq!(
        engine.cache_store().freshness(&c_node),
        Freshness::Final,
        "c must be Final after the walk (Pending→Final propagation via b→c edge)"
    );

    // (iii) Byte-identical result_hash: no value recomputation occurred.
    let b_after = engine
        .cache_store()
        .get(&b_node)
        .expect("b still cached after walk")
        .clone();
    assert_eq!(
        b_after.result_hash, b_before_hash,
        "b's result_hash must be byte-identical (the walk MUST NOT recompute values)"
    );

    let c_after = engine
        .cache_store()
        .get(&c_node)
        .expect("c still cached after walk")
        .clone();
    assert_eq!(
        c_after.result_hash, c_before_hash,
        "c's result_hash must be byte-identical (the walk MUST NOT recompute values)"
    );

    // (iv) Byte-identical inner Value: no evaluator fired.
    let b_after_value = match &b_after.result {
        CachedResult::Value(v, _) => v.clone(),
        other => panic!(
            "expected b to still have CachedResult::Value after walk, got {:?}",
            other
        ),
    };
    assert_eq!(
        b_after_value, b_before_value,
        "b's cached Value must be byte-identical (no value evaluator calls fired)"
    );

    let c_after_value = match &c_after.result {
        CachedResult::Value(v, _) => v.clone(),
        other => panic!(
            "expected c to still have CachedResult::Value after walk, got {:?}",
            other
        ),
    };
    assert_eq!(
        c_after_value, c_before_value,
        "c's cached Value must be byte-identical (no value evaluator calls fired)"
    );

    // (v) Implicit: set_panic_on_eval registered on b and c; reaching this
    //     line without a panic proves evaluate_let_bindings was never invoked.
}

/// Step-3 (task 3649): `Engine::propagate_freshness_only` accepts any
/// `IntoIterator<Item = &ValueCellId>`, not only `&HashSet<ValueCellId>`.
///
/// The unique coverage here is compile-time: both `std::iter::once(&id)` and
/// `&[id]` must type-check against the widened facade signature. Behavioral
/// correctness (updated-set membership, Final propagation) is covered by
/// `engine_propagate_freshness_only_drives_pending_to_final_downstream_without_re_evaluation`
/// above; no assertions are repeated here.
///
/// RED: does NOT compile against the current facade signature
/// `changed: &std::collections::HashSet<ValueCellId>`.
#[test]
fn engine_propagate_freshness_only_accepts_borrowed_iterator() {
    let module = three_cell_module();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    engine.eval(&module);

    let e = "T";
    let a_id = ValueCellId::new(e, "a");

    // Compile-time checks: both non-HashSet iterator forms must type-check
    // against `impl IntoIterator<Item = &ValueCellId>`.
    let _ = engine.propagate_freshness_only(std::iter::once(&a_id), 1);
    let _ = engine.propagate_freshness_only(std::slice::from_ref(&a_id), 1);
}
