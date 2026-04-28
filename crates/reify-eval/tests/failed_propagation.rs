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
use reify_test_support::builders::{binop, literal, value_ref_typed};
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::{CompiledModuleBuilder, TopologyTemplateBuilder};
use reify_types::{BinOp, Freshness, ModulePath, Type, Value, ValueCellId};

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

/// Build the 3-cell synthetic chain: `let a = 5.0; let b = a * 2.0; let c = b + 1.0`.
///
/// All three cells live in the same template, so they share a topological
/// ordering: a → b → c. This is the canonical chain used to exercise the
/// arch §9.1–§9.2 Failed → Pending propagation path with chain forwarding.
fn three_cell_chain_module() -> reify_compiler::CompiledModule {
    let e = "T";
    CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(
            TopologyTemplateBuilder::new(e)
                .let_binding(e, "a", Type::Real, literal(Value::Real(5.0)))
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

/// End-to-end propagation chain: panic on leaf `a` → b becomes Pending with
/// `pending_cause = Some(a)`, c becomes Pending with the chain forwarded
/// (`pending_cause = Some(a)`), and the value computation on b/c is quieted
/// (no `Completed { Changed }` event for either).
///
/// Pins the integration of:
///   - Step-12 panic boundary in `evaluate_let_bindings`.
///   - Step-14 pre-eval Pending gate in `evaluate_let_bindings` (writes
///     `mark_pending_with_cause` and emits `Completed { Unchanged }`).
///   - The cause-bearing derive helpers added in step-6 / step-8.
///
/// Implements arch §9.1 lines 868–877 (Failed → Pending propagation), §9.2
/// line 890 (Failed input → Pending output carve-out), and §7.2 line 748
/// (Pending naturally quiets the downstream subtree without re-running the
/// value computation).
///
/// Note on assertion (5): step-13 originally specified "no Started event
/// for c" as the strong "naturally quiets" interpretation; step-14's gate
/// design (per the plan) emits `Started` + `Completed { Unchanged }` so the
/// journal still tracks the node visit. We assert the weaker but spec-
/// consistent guarantee instead — that c's value is NOT recomputed
/// (`Completed { Changed }` does not fire for c) — which is the operational
/// meaning of "quieting" inside the gate-fires design.
#[test]
fn panic_in_leaf_propagates_pending_with_chain_to_mid_and_quiets_downstream() {
    let module = three_cell_chain_module();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    let e = "T";
    let a_id = ValueCellId::new(e, "a");
    let b_id = ValueCellId::new(e, "b");
    let c_id = ValueCellId::new(e, "c");
    let a_node = NodeId::Value(a_id.clone());
    let b_node = NodeId::Value(b_id.clone());
    let c_node = NodeId::Value(c_id.clone());

    // === Pass 1: cold-start, all-Final baseline ===
    let _ = engine.eval(&module);

    assert_eq!(
        engine.cache_store().freshness(&a_node),
        Freshness::Final,
        "Pass 1: a must be Final after cold-start"
    );
    assert_eq!(
        engine.cache_store().freshness(&b_node),
        Freshness::Final,
        "Pass 1: b must be Final after cold-start"
    );
    assert_eq!(
        engine.cache_store().freshness(&c_node),
        Freshness::Final,
        "Pass 1: c must be Final after cold-start"
    );

    // Capture the pre-failure result hashes so we can prove last_substantive
    // (carried in `Pending { last_substantive }`) preserves the prior value
    // across the propagation gate.
    let prev_b_hash = engine
        .cache_store()
        .get(&b_node)
        .expect("b cache entry must exist after Pass 1")
        .result_hash;
    let prev_c_hash = engine
        .cache_store()
        .get(&c_node)
        .expect("c cache entry must exist after Pass 1")
        .result_hash;

    // === Pass 2: re-eval with set_panic_on_eval(a) ===
    engine.set_panic_on_eval(a_id.clone());
    let _ = engine.eval(&module);

    // (1) freshness(a) == Failed.
    let a_freshness = engine.cache_store().freshness(&a_node);
    assert!(
        matches!(a_freshness, Freshness::Failed { .. }),
        "Pass 2 (1): a must be Failed after forced-panic re-eval; got {:?}",
        a_freshness
    );

    // (2) freshness(b) == Pending { last_substantive = prev_b_hash } AND
    //     pending_cause(b) == Some(NodeId::Value(a)).
    let b_freshness = engine.cache_store().freshness(&b_node);
    match &b_freshness {
        Freshness::Pending { last_substantive } => {
            assert_eq!(
                last_substantive.content_hash(),
                Some(prev_b_hash),
                "Pass 2 (2): b's last_substantive must point to its prior \
                 result_hash so the pre-failure value survives the gate; \
                 got {:?}",
                last_substantive
            );
        }
        other => panic!(
            "Pass 2 (2): b must be Pending after Failed input; got {:?}",
            other
        ),
    }
    assert_eq!(
        engine.cache_store().pending_cause(&b_node),
        Some(a_node.clone()),
        "Pass 2 (2): b's pending_cause must point at a (chain root)"
    );

    // (3) freshness(c) == Pending { last_substantive = prev_c_hash } AND
    //     pending_cause(c) == Some(NodeId::Value(a)) (chain forwarded).
    let c_freshness = engine.cache_store().freshness(&c_node);
    match &c_freshness {
        Freshness::Pending { last_substantive } => {
            assert_eq!(
                last_substantive.content_hash(),
                Some(prev_c_hash),
                "Pass 2 (3): c's last_substantive must point to its prior \
                 result_hash so the pre-failure value survives the gate; \
                 got {:?}",
                last_substantive
            );
        }
        other => panic!(
            "Pass 2 (3): c must be Pending after Pending input; got {:?}",
            other
        ),
    }
    assert_eq!(
        engine.cache_store().pending_cause(&c_node),
        Some(a_node.clone()),
        "Pass 2 (3): c's pending_cause must equal a (chain forwarded from b)"
    );

    // (4) journal contains exactly one Failed event, scoped to a.
    let failed_count = engine
        .journal()
        .count_matching(|k| matches!(k, EventKind::Failed { .. }));
    assert_eq!(
        failed_count, 1,
        "Pass 2 (4): exactly one Failed event must be recorded for the \
         entire failed re-eval"
    );
    let a_events = engine.journal().events_for_node(&a_node);
    let a_failed: Vec<_> = a_events
        .iter()
        .filter(|e| matches!(e.kind, EventKind::Failed { .. }))
        .collect();
    assert_eq!(
        a_failed.len(),
        1,
        "Pass 2 (4): the Failed event must be scoped to NodeId::Value(a)"
    );

    // (5) c's value was NOT recomputed during Pass 2: the gate path emits
    //     `Completed { Unchanged }`, never `Completed { Changed }`. Searching
    //     all c events from Pass 2 onwards ensures Pass 1's Completed event
    //     does not pollute the assertion.
    use reify_eval::cache::EvalOutcome;
    let c_events = engine.journal().events_for_node(&c_node);
    let pass2_started_count = c_events
        .iter()
        .filter(|e| matches!(e.kind, EventKind::Started))
        .count();
    let c_changed_after_pass1 = c_events.iter().rev().take(pass2_started_count).any(|e| {
        matches!(
            e.kind,
            EventKind::Completed {
                outcome: EvalOutcome::Changed
            }
        )
    });
    assert!(
        !c_changed_after_pass1,
        "Pass 2 (5): c's value must NOT be recomputed after the failed \
         re-eval (no Completed{{Changed}} event from Pass 2) — the gate \
         must quiet the downstream subtree per arch §7.2 line 748"
    );
}
