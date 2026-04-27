//! Integration tests for arch §7.2 freshness propagation rule.
//!
//! Verifies that `CacheStore::derive_output_freshness_for_node` implements the
//! §7.2 truth table over real Engine state with synthetic input-freshness injection.

use reify_eval::Engine;
use reify_eval::cache::NodeId;
use reify_test_support::builders::{binop, literal, value_ref_typed};
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::{CompiledModuleBuilder, TopologyTemplateBuilder};
use reify_types::{BinOp, ErrorRef, Freshness, ModulePath, Type, Value, ValueCellId};

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

    // --- Row 5: still_refining=false, a=Pending → Intermediate{generation: 99} ---
    // Inject via mark_pending (canonical path per task #2326 contract)
    let marked = engine
        .cache_store_mut()
        .mark_pending(&NodeId::Value(a_id.clone()));
    assert!(marked, "a must be in cache after eval()");
    assert_eq!(
        engine
            .cache_store()
            .derive_output_freshness_for_node(&b_node, false, g),
        Freshness::Intermediate { generation: g },
        "Row 5: still_refining=false, Pending input → Intermediate (Pending is non-Final per §7.2)"
    );
    restore_a_final(&mut engine);

    // --- Row 6: still_refining=false, a=Failed → Intermediate{generation: 99} ---
    let _ = engine.cache_store_mut().set_freshness(
        &NodeId::Value(a_id.clone()),
        Freshness::Failed {
            error: ErrorRef::new("synthetic failure"),
        },
    );
    assert_eq!(
        engine
            .cache_store()
            .derive_output_freshness_for_node(&b_node, false, g),
        Freshness::Intermediate { generation: g },
        "Row 6: still_refining=false, Failed input → Intermediate (Failed is non-Final per §7.2)"
    );
    restore_a_final(&mut engine);
}
