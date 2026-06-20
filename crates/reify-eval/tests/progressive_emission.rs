//! Integration test pinning the PROGRESSIVE-trait intermediate-emission
//! contract over a real Engine cache.
//!
//! Implements PRD task #5 from `docs/prds/node-trait-composition.md`:
//! "scheduler expects multiple cache updates per evaluation. Cross-check with
//! freshness-propagation: each intermediate emission updates the cache and
//! triggers a freshness-only downstream walk (#2335). Integration test with a
//! synthetic progressive node emitting 3 intermediates then Final, assert
//! downstream sees Intermediate then Final transitions."
//!
//! Pins arch §7.6 (`NodeTraits::PROGRESSIVE`) + arch §3.5 line 432 ("no value
//! recomputation occurs") over a real Engine cache populated by cold
//! `Engine::eval()`.
//!
//! ## Scenario
//!
//! Two-cell synthetic module: `param a = 5.0`, `let b = a * 2.0`.
//! After cold eval all cells are `Final`. We synthetically drive `a` through
//! `Intermediate{1} → Intermediate{2} → Intermediate{3} → Final`, calling
//! `propagate_freshness_only` after each emission step and asserting that `b`
//! follows the same freshness sequence.
//!
//! ## Key invariants pinned
//!
//! - Each call to `propagate_freshness_only(generation=g)` after writing
//!   `a → Intermediate{g}` transitions `b` to `Intermediate{generation: g}`.
//! - After writing `a → Final` and calling `propagate_freshness_only`, `b`
//!   transitions to `Final`.
//! - The `updated` set returned by each walk contains `b` — propagation through
//!   the `a→b` let-binding edge fired on every emission step.
//! - b's `result_hash` and cached `Value` are byte-identical to the pre-emission
//!   snapshot throughout all four walks — no value evaluator ran during any of the
//!   freshness-only propagation rounds (arch §3.5 line 432).

use reify_core::{ModulePath, Type, ValueCellId};
use reify_eval::Engine;
use reify_eval::cache::{CachedResult, NodeId};
use reify_eval::freshness_walk;
use reify_ir::{BinOp, Freshness, NodeTraits, Value};
use reify_test_support::builders::{binop, literal, value_ref_typed};
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::{CompiledModuleBuilder, TopologyTemplateBuilder};
use std::collections::HashSet;

/// Build the 2-cell synthetic module: param `a` + let `b = a * 2.0`.
///
/// Identical to the fixture in `tests/freshness_only_propagation.rs:24-43` so
/// a future refactor of the fixture is caught by both files in the same change.
fn two_cell_module() -> reify_compiler::CompiledModule {
    let e = "T";
    CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(
            TopologyTemplateBuilder::new(e)
                .param(
                    e,
                    "a",
                    Type::dimensionless_scalar(),
                    Some(literal(Value::Real(5.0))),
                )
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

/// Cold-start `Engine::eval()`, then drive `a` through one
/// `Intermediate{1} → Final` emission cycle, asserting that `b` follows the
/// same freshness sequence via `propagate_freshness_only`.
///
/// Establishes the basic single-Intermediate-then-Final progressive contract:
///
/// 1. Write `a → Intermediate{1}`, run `propagate_freshness_only(generation=1)`
///    from `{a}`, assert `b == Intermediate{1}` and `updated` contains `b`.
/// 2. Write `a → Final`, run `propagate_freshness_only(generation=2)` from
///    `{a}`, assert `b == Final` and `updated` contains `b`.
#[test]
fn progressive_node_emits_one_intermediate_then_final_propagates_to_downstream() {
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

    // Clone the reverse_index before any `cache_store_mut()` call — borrow
    // checker forbids holding `&engine.eval_state()` and
    // `&mut engine.cache_store_mut()` simultaneously (same idiom as
    // `freshness_only_propagation.rs:126-130`).
    let reverse_index = engine
        .eval_state()
        .expect("eval_state populated by Engine::eval")
        .reverse_index
        .clone();
    // P3.3 step-16: clone the graph for edge #12 fan-out (Compute → output VCs).
    let graph = engine
        .eval_state()
        .expect("eval_state populated by Engine::eval")
        .snapshot
        .graph
        .clone();

    // Tag `a` as PROGRESSIVE — positive permit for `write_intermediate`
    // (M-009 fix: gives the PROGRESSIVE trait runtime teeth for this node).
    engine
        .cache_store_mut()
        .node_traits_mut()
        .set_instance(a_node.clone(), NodeTraits::PROGRESSIVE);

    // ── Emission step 1: a → Intermediate{1} ─────────────────────────────
    let emit_1 = engine.cache_store_mut().write_intermediate(&a_node, 1);
    assert!(
        emit_1.is_none(),
        "PROGRESSIVE node must emit silently (positive permit): got {:?}",
        emit_1
    );
    assert_eq!(
        engine.cache_store().freshness(&a_node),
        Freshness::Intermediate { generation: 1 },
        "write_intermediate must land Intermediate{{1}} freshness on a"
    );

    let mut changed = HashSet::new();
    changed.insert(a_id.clone());

    let updated_1 = freshness_walk::propagate_freshness_only(
        engine.cache_store_mut(),
        &reverse_index,
        &graph,
        &changed,
        1,
    );

    // b must appear in the updated set — propagation through a→b fired.
    assert!(
        updated_1.contains(&b_node),
        "updated must contain Value(b) after Intermediate{{1}} emission, got: {:?}",
        updated_1
    );
    // b's freshness must be Intermediate{1} — the walk derived it from a's
    // Intermediate{1} state (§7.2/§9.2 truth table: non-Final input → Intermediate).
    assert_eq!(
        engine.cache_store().freshness(&b_node),
        Freshness::Intermediate { generation: 1 },
        "b must be Intermediate{{1}} after propagating a's Intermediate{{1}}"
    );

    // ── Emission step 2 (final): a → Final ───────────────────────────────
    assert!(
        engine
            .cache_store_mut()
            .set_freshness(&a_node, Freshness::Final),
        "a must still be in the cache"
    );

    let updated_2 = freshness_walk::propagate_freshness_only(
        engine.cache_store_mut(),
        &reverse_index,
        &graph,
        &changed,
        2,
    );

    // b must again appear in the updated set.
    assert!(
        updated_2.contains(&b_node),
        "updated must contain Value(b) after Final emission, got: {:?}",
        updated_2
    );
    // b's freshness must now be Final — all inputs are Final.
    assert_eq!(
        engine.cache_store().freshness(&b_node),
        Freshness::Final,
        "b must be Final after propagating a's Final"
    );
}

/// Full PRD task #5 acceptance test: drive `a` through three Intermediate
/// emissions then Final, asserting that `b` follows
/// `Intermediate{1} → Intermediate{2} → Intermediate{3} → Final`.
///
/// Pins the multi-emission contract: `propagate_freshness_only` is invoked
/// once per emission step and each invocation transitions `b` cleanly through
/// the sequence. The §7.2 `Intermediate{N} → Intermediate{N+1}` transition is
/// a "legitimate generation-bumping" step (see `freshness_walk.rs:154-162`);
/// passing `generation=g` matching `a`'s emission step ensures the walk's
/// early-cutoff gate (`new == current`) does NOT fire across consecutive
/// Intermediate generations.
#[test]
fn progressive_node_emits_three_intermediates_then_final_transitions_downstream() {
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

    // Clone the reverse_index before any `cache_store_mut()` call.
    let reverse_index = engine
        .eval_state()
        .expect("eval_state populated by Engine::eval")
        .reverse_index
        .clone();
    // P3.3 step-16: clone the graph for edge #12 fan-out.
    let graph = engine
        .eval_state()
        .expect("eval_state populated by Engine::eval")
        .snapshot
        .graph
        .clone();

    // Tag `a` as PROGRESSIVE — positive permit for `write_intermediate`.
    engine
        .cache_store_mut()
        .node_traits_mut()
        .set_instance(a_node.clone(), NodeTraits::PROGRESSIVE);

    let mut changed = HashSet::new();
    changed.insert(a_id.clone());

    // ── Three Intermediate emission steps ─────────────────────────────────
    for g in 1u64..=3 {
        // Write a → Intermediate{g} via the guarded deliberate-emission entry.
        let emit = engine.cache_store_mut().write_intermediate(&a_node, g);
        assert!(
            emit.is_none(),
            "PROGRESSIVE node must emit silently at step g={}: got {:?}",
            g,
            emit
        );
        assert_eq!(
            engine.cache_store().freshness(&a_node),
            Freshness::Intermediate { generation: g },
            "write_intermediate must land Intermediate{{{}}} freshness on a",
            g
        );

        let updated = freshness_walk::propagate_freshness_only(
            engine.cache_store_mut(),
            &reverse_index,
            &graph,
            &changed,
            g,
        );

        // b must appear in the updated set every round.
        assert!(
            updated.contains(&b_node),
            "updated must contain Value(b) at Intermediate{{{}}} emission, got: {:?}",
            g,
            updated
        );
        // b's freshness must match the current emission generation.
        assert_eq!(
            engine.cache_store().freshness(&b_node),
            Freshness::Intermediate { generation: g },
            "b must be Intermediate{{{}}} after propagating a's Intermediate{{{}}}",
            g,
            g
        );
    }

    // ── Final emission step ───────────────────────────────────────────────
    assert!(
        engine
            .cache_store_mut()
            .set_freshness(&a_node, Freshness::Final),
        "a must still be in the cache for the Final emission"
    );

    let updated_final = freshness_walk::propagate_freshness_only(
        engine.cache_store_mut(),
        &reverse_index,
        &graph,
        &changed,
        4,
    );

    // b must appear in the updated set on the Final emission as well.
    assert!(
        updated_final.contains(&b_node),
        "updated must contain Value(b) after Final emission, got: {:?}",
        updated_final
    );
    // b's freshness must now be Final.
    assert_eq!(
        engine.cache_store().freshness(&b_node),
        Freshness::Final,
        "b must be Final after propagating a's Final"
    );
}

/// Pins arch §3.5 line 432 "no value recomputation" invariant across the full
/// 3-Intermediate-then-Final progressive emission cycle.
///
/// After a cold eval, the value evaluator computed `b = a * 2.0 = 10.0` and
/// stored the result under `b`'s `result_hash`. Each of the four
/// `propagate_freshness_only` calls only touches `freshness` — never `result`,
/// `result_hash`, or `dependency_trace`. The byte-identical snapshot of
/// `result_hash` + inner `Value` is the strongest possible witness that no
/// value evaluator call fired during any of the four propagation rounds: the
/// evaluator would have written a new `result_hash` if it had run.
///
/// Also carries the compile-time anchor tying this test to
/// `NodeTraits::PROGRESSIVE` (arch §7.6): deletion or renaming of the trait
/// constant will break this test's compile, catching the cross-task break at
/// CI time without requiring a runtime `node.traits.contains(PROGRESSIVE)`
/// assertion (no node-id → traits map exists today; see `node_traits.rs:148-153`).
#[test]
fn progressive_emission_does_not_recompute_downstream_value() {
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

    // Snapshot b's entry BEFORE any freshness manipulation — these are the
    // "no value evaluator ran" witness values (result_hash + inner Value).
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
    // Sanity: b = a * 2.0 = 5.0 * 2.0 = 10.0.
    assert_eq!(
        b_before_value,
        Value::Real(10.0),
        "b must equal 10.0 after cold eval"
    );

    // Clone the reverse_index before any `cache_store_mut()` call.
    let reverse_index = engine
        .eval_state()
        .expect("eval_state populated by Engine::eval")
        .reverse_index
        .clone();
    // P3.3 step-16: clone the graph for edge #12 fan-out.
    let graph = engine
        .eval_state()
        .expect("eval_state populated by Engine::eval")
        .snapshot
        .graph
        .clone();

    // Tag `a` as PROGRESSIVE — positive permit for `write_intermediate`.
    // This gives the PROGRESSIVE flag real runtime teeth (M-009 fix):
    // the runtime verifies the permit on every emission, replacing the
    // former compile-time-only anchor (`let _: NodeTraits = NodeTraits::PROGRESSIVE`).
    engine
        .cache_store_mut()
        .node_traits_mut()
        .set_instance(a_node.clone(), NodeTraits::PROGRESSIVE);

    let mut changed = HashSet::new();
    changed.insert(a_id.clone());

    // ── Run the full 3-Intermediate-then-Final emission cycle ─────────────
    for g in 1u64..=3 {
        let emit = engine.cache_store_mut().write_intermediate(&a_node, g);
        assert!(
            emit.is_none(),
            "PROGRESSIVE node must emit silently at step g={}: got {:?}",
            g,
            emit
        );
        freshness_walk::propagate_freshness_only(
            engine.cache_store_mut(),
            &reverse_index,
            &graph,
            &changed,
            g,
        );
    }
    // Final emission.
    assert!(
        engine
            .cache_store_mut()
            .set_freshness(&a_node, Freshness::Final),
        "a must still be in the cache"
    );
    freshness_walk::propagate_freshness_only(
        engine.cache_store_mut(),
        &reverse_index,
        &graph,
        &changed,
        4,
    );

    // ── "No value recomputation" witness ──────────────────────────────────
    // Snapshot b's entry AFTER the full emission cycle.
    let b_after = engine
        .cache_store()
        .get(&b_node)
        .expect("b still cached after walks")
        .clone();

    assert_eq!(
        b_after.result_hash, b_before_hash,
        "b's result_hash must be byte-identical across all 4 propagation rounds \
         (the walk MUST NOT recompute values)"
    );
    let b_after_value = match &b_after.result {
        CachedResult::Value(v, _) => v.clone(),
        other => panic!(
            "expected b to still have CachedResult::Value after walks, got {:?}",
            other
        ),
    };
    assert_eq!(
        b_after_value, b_before_value,
        "b's cached Value must be byte-identical after all 4 propagation rounds \
         (no value evaluator calls fired)"
    );

    // b's final freshness must be Final (the last emission step).
    assert_eq!(
        engine.cache_store().freshness(&b_node),
        Freshness::Final,
        "b must be Final after the full emission cycle"
    );
}
