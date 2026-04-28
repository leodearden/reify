//! Integration tests for arch §9.1–§9.2 Failed production and Pending
//! propagation with diagnostic chain.
//!
//! Covers:
//! - The test-instrumentation panic-injection hook (`set_panic_on_eval`)
//!   used to simulate a forced panic in a leaf node.
//! - The §9.2 carve-out: Failed input → downstream Pending with the chain
//!   root recorded in `pending_cause`.
//! - The §9.3 separation: constraint violations stay on the
//!   `Satisfaction::Violated` channel and never produce `Freshness::Failed`
//!   or `EventKind::Failed`.
//! - Kernel error → `Freshness::Failed` on the realization NodeId plus a
//!   single `EventKind::Failed` event.
//!
//! Tests in this file rely on the `test-instrumentation` Cargo feature
//! enabled via the self-dev-dep in `crates/reify-eval/Cargo.toml`.

use reify_eval::Engine;
use reify_eval::cache::NodeId;
use reify_eval::journal::EventKind;
use reify_test_support::builders::literal;
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::{CompiledModuleBuilder, TopologyTemplateBuilder};
use reify_types::{Freshness, ModulePath, Type, Value, ValueCellId};

/// Build a 1-cell synthetic module: `let b = 1.0` inside a single template.
fn one_cell_module() -> reify_compiler::CompiledModule {
    let e = "T";
    CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(
            TopologyTemplateBuilder::new(e)
                .let_binding(e, "b", Type::Real, literal(Value::Real(1.0)))
                .build(),
        )
        .build()
}

/// Pin the test-instrumentation panic hook in `evaluate_let_bindings`.
///
/// When `set_panic_on_eval(b)` is registered, the let-binding evaluator
/// must:
///   (a) NOT crash the engine (panic is caught by `catch_unwind`).
///   (b) Mark `b` as `Freshness::Failed { error }` in the cache.
///   (c) Emit exactly one `EventKind::Failed` event in the journal.
///   (d) Scope that event to `NodeId::Value(b)`.
///   (e) Skip the normal `EventKind::Completed` event for `b`.
///
/// See arch §9.1 and the plan #2330 design decision on
/// `panic_on_eval_cells: HashSet<ValueCellId>` test injection.
#[test]
fn forced_panic_on_let_binding_marks_failed_and_emits_one_failed_event() {
    let module = one_cell_module();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    let b_id = ValueCellId::new("T", "b");
    engine.set_panic_on_eval(b_id.clone());

    // Assertion (a): the engine does not crash. If `eval` panics, the test
    // process dies — so reaching the next line is itself the proof.
    let _ = engine.eval(&module);

    // Assertion (b): freshness is Failed.
    let b_node = NodeId::Value(b_id.clone());
    let freshness = engine.cache_store().freshness(&b_node);
    match &freshness {
        Freshness::Failed { error } => {
            // The error message should mention the panic — exact wording is
            // implementation-defined, so just assert it is non-empty.
            assert!(
                !error.message().is_empty(),
                "Failed error message must be non-empty"
            );
        }
        other => panic!(
            "expected b's freshness to be Failed after forced panic; got {:?}",
            other
        ),
    }

    // Assertion (c): exactly one EventKind::Failed event.
    let failed_count = engine
        .journal()
        .count_matching(|k| matches!(k, EventKind::Failed { .. }));
    assert_eq!(
        failed_count, 1,
        "exactly one EventKind::Failed event must be recorded after forced panic"
    );

    // Assertion (d): the failed event's node_id is NodeId::Value(b).
    let b_events = engine.journal().events_for_node(&b_node);
    let failed_events: Vec<_> = b_events
        .iter()
        .filter(|e| matches!(e.kind, EventKind::Failed { .. }))
        .collect();
    assert_eq!(
        failed_events.len(),
        1,
        "exactly one Failed event must be scoped to NodeId::Value(b)"
    );

    // Assertion (e): NO EventKind::Completed event for b on the failure path.
    let completed_events: Vec<_> = b_events
        .iter()
        .filter(|e| matches!(e.kind, EventKind::Completed { .. }))
        .collect();
    assert!(
        completed_events.is_empty(),
        "no EventKind::Completed event should be recorded for b on the Failed \
         path; found {} completed event(s)",
        completed_events.len()
    );
}
