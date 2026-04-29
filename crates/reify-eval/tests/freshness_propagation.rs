//! Integration tests for arch §7.2 freshness propagation rule.
//!
//! Verifies that `CacheStore::derive_output_freshness_for_node` implements the
//! §7.2 truth table over real Engine state with synthetic input-freshness injection.

use reify_eval::Engine;
use reify_eval::cache::NodeId;
use reify_test_support::builders::{binop, literal, value_ref_typed};
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::{CompiledModuleBuilder, TopologyTemplateBuilder};
use reify_types::{BinOp, ErrorRef, Freshness, ModulePath, ResultRef, Type, Value, ValueCellId};

/// Build the 2-cell synthetic module: param `a` + let `b = a * 2.0`.
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

/// End-to-end regression guard: after a cold-start `Engine::eval()`, the let-binding `b`
/// must have `Freshness::Final` in the cache.
///
/// This test does NOT depend on being able to observe non-Final freshness end-to-end
/// (params are all-Final before let-bindings run), but it does pin that the wire-in in
/// `evaluate_let_bindings` actually writes the correct freshness via
/// `record_evaluation_propagating_freshness`. If that call were removed or broken,
/// this test would fail.
#[test]
fn let_binding_freshness_is_final_after_cold_start_eval() {
    let module = two_cell_module();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    engine.eval(&module);

    let b_id = ValueCellId::new("T", "b");
    let b_node = NodeId::Value(b_id);
    assert_eq!(
        engine.cache_store().freshness(&b_node),
        Freshness::Final,
        "let binding b must be Final after cold-start eval with all-Final param inputs"
    );
}

/// Arch §7.2 truth table at integration level over a real Engine + 2-cell module.
///
/// Injects synthetic input freshness on `a` via `cache_store_mut()`, then
/// calls `derive_output_freshness_for_node` on `b` and asserts the §7.2 result.
///
/// Note: the non-final-inputs case is NOT directly observable through `Engine::eval()`
/// end-to-end (the param pass rewrites all param freshness to Final before let-bindings
/// run). This test exercises the derivation logic directly — correctness of the
/// all-Final case is pinned by `freshness_final_after_cold_start` in incremental.rs.
#[test]
fn derive_output_freshness_for_node_implements_arch_7_2_over_synthetic_graph() {
    let module = two_cell_module();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    // Cold-start eval to populate the cache with b's dependency_trace.reads = [a]
    engine.eval(&module);

    let e = "T";
    let a_id = ValueCellId::new(e, "a");
    let b_id = ValueCellId::new(e, "b");
    let b_node = NodeId::Value(b_id.clone());
    let g = 99u64;

    // Helper: restore `a` to Final between rows
    let restore_a_final = |engine: &mut Engine| {
        let a_node = NodeId::Value(ValueCellId::new(e, "a"));
        let _ = engine
            .cache_store_mut()
            .set_freshness(&a_node, Freshness::Final);
    };

    // --- Row 1: still_refining=true, a=Final → Intermediate{generation: 99} ---
    // a is Final by default after eval(); no injection needed
    assert_eq!(
        engine
            .cache_store()
            .derive_output_freshness_for_node(&b_node, true, g),
        Freshness::Intermediate { generation: g },
        "Row 1: still_refining=true, all-Final inputs → Intermediate"
    );

    // --- Row 2: still_refining=true, a=Intermediate → Intermediate{generation: 99} ---
    let _ = engine.cache_store_mut().set_freshness(
        &NodeId::Value(a_id.clone()),
        Freshness::Intermediate { generation: 3 },
    );
    assert_eq!(
        engine
            .cache_store()
            .derive_output_freshness_for_node(&b_node, true, g),
        Freshness::Intermediate { generation: g },
        "Row 2: still_refining=true, Intermediate input → Intermediate"
    );
    restore_a_final(&mut engine);

    // --- Row 3: still_refining=false, a=Final → Final ---
    assert_eq!(
        engine
            .cache_store()
            .derive_output_freshness_for_node(&b_node, false, g),
        Freshness::Final,
        "Row 3: still_refining=false, all-Final inputs → Final"
    );

    // --- Row 4: still_refining=false, a=Intermediate → Intermediate{generation: 99} ---
    let _ = engine.cache_store_mut().set_freshness(
        &NodeId::Value(a_id.clone()),
        Freshness::Intermediate { generation: 5 },
    );
    assert_eq!(
        engine
            .cache_store()
            .derive_output_freshness_for_node(&b_node, false, g),
        Freshness::Intermediate { generation: g },
        "Row 4: still_refining=false, Intermediate input → Intermediate"
    );
    restore_a_final(&mut engine);

    // --- Row 5: still_refining=false, a=Pending → Pending{none()} (§7.2 line 748) ---
    // Inject via mark_pending (canonical path per task #2326 contract).
    // Updated for arch §7.2 line 748 + §9.2 line 890 carve-out (task #2330):
    // Pending input now produces Pending output (not Intermediate). The pure helper
    // drops the chain — `(Pending, None)` is the chain-incomplete sentinel; chain
    // forwarding is exercised by Row 7 and `derive_output_freshness_with_cause`.
    let marked = engine
        .cache_store_mut()
        .mark_pending(&NodeId::Value(a_id.clone()));
    assert!(marked, "a must be in cache after eval()");
    assert_eq!(
        engine
            .cache_store()
            .derive_output_freshness_for_node(&b_node, false, g),
        Freshness::Pending {
            last_substantive: ResultRef::none()
        },
        "Row 5: still_refining=false, Pending input → Pending (arch §7.2 line 748: \
         Pending naturally quiets the downstream subtree)"
    );
    restore_a_final(&mut engine);

    // --- Row 6: still_refining=false, a=Failed → Pending{none()} (§9.2 line 890 carve-out) ---
    // Updated for arch §9.2 line 890 (task #2330): Failed input is carved out of
    // §7.2's "any non-Final → Intermediate" rule and produces Pending output instead,
    // so the downstream subtree is naturally quieted. The chain root (the failing
    // NodeId) is recovered via `derive_output_freshness_for_node_with_cause`
    // (exercised in Row 7).
    let _ = engine.cache_store_mut().mark_failed(
        &NodeId::Value(a_id.clone()),
        ErrorRef::new("synthetic failure"),
    );
    assert_eq!(
        engine
            .cache_store()
            .derive_output_freshness_for_node(&b_node, false, g),
        Freshness::Pending {
            last_substantive: ResultRef::none()
        },
        "Row 6: still_refining=false, Failed input → Pending (arch §9.2 line 890 carve-out: \
         downstream of Failed is Pending so the subtree is naturally quieted)"
    );
    restore_a_final(&mut engine);

    // --- Row 7: cause-bearing variant returns the failing NodeId ---
    // Pin the chain semantics: when `a` is Failed, the cause-bearing variant returns
    // (Pending, Some(NodeId::Value(a))) so the downstream Pending entry can record
    // the chain root in its `pending_cause` side-table. See arch §9.2 lines 880-890
    // and the plan #2330 design decision on side-table storage.
    let a_node = NodeId::Value(a_id.clone());
    let _ = engine
        .cache_store_mut()
        .mark_failed(&a_node, ErrorRef::new("synthetic failure for chain root"));
    let (fresh, cause) = engine
        .cache_store()
        .derive_output_freshness_for_node_with_cause(&b_node, false, g);
    assert_eq!(
        fresh,
        Freshness::Pending {
            last_substantive: ResultRef::none()
        },
        "Row 7: Failed input must produce Pending output (parity with the pure helper)"
    );
    assert_eq!(
        cause,
        Some(a_node),
        "Row 7: cause-bearing variant must return the failing NodeId as the chain root"
    );
    restore_a_final(&mut engine);
}
