//! Boundary tests pinning D1/INV-1: graph `Freshness::Failed` vs language
//! Option-recovery orthogonality (result-and-fallback PRD §8 Phase 3 / §9.6).
//!
//! Two sides:
//!   (a) Determined-`none` IS recovered: `unwrap_or(none, dflt)` evaluates to
//!       `dflt` with `Freshness::Final`; `unwrap_or(some(x), dflt)` evaluates to
//!       `x` (tag-driven, not blanket-default).
//!   (b) Graph-`Failed` is NOT recovered — two facets:
//!       Facet 1: set_panic_on_eval on the consumer LET cell itself keeps the
//!                cell `Freshness::Failed`; the language-recovery combinator
//!                never fires.
//!       Facet 2 (C-4): a downstream `unwrap_or` consumer of a `Failed`
//!                upstream becomes `Freshness::Pending` rooted at the failed
//!                node — it is NOT recovered to the default.
//!
//! All assertions observe only the engine's public read paths:
//! `Engine::eval`, `Engine::cache_store().freshness()/.pending_cause()`,
//! `Engine::journal()`, `result.values`.  No production code is changed.
//!
//! Relies on the `test-instrumentation` feature, wired via the self-dev-dep in
//! `crates/reify-eval/Cargo.toml` — no Cargo change needed.

use reify_constraints::SimpleConstraintChecker;
use reify_core::ValueCellId;
use reify_eval::Engine;
use reify_eval::EvalResult;
use reify_eval::cache::NodeId;
use reify_eval::journal::EventKind;
use reify_ir::{Freshness, Value};
use reify_test_support::{collect_errors, mm, parse_and_compile_with_stdlib};

// ── shared harness ────────────────────────────────────────────────────────────

/// Run `parse_and_compile_with_stdlib(src)` then `Engine::eval`, returning
/// both the engine and the eval result so callers can read freshness and
/// journal state after the eval.
///
/// Mirrors `option_recovery_map_or_e2e.rs`'s harness pattern.
/// Panics if the fixture source has any Error diagnostics (compile-guard).
fn eval_module(src: &str) -> (Engine, EvalResult) {
    let compiled = parse_and_compile_with_stdlib(src);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "fixture source must compile with no Error diagnostics; got: {:?}",
        errors
    );
    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), None);
    let result = engine.eval(&compiled);
    (engine, result)
}

// ── tests ─────────────────────────────────────────────────────────────────────

/// Side (a): `unwrap_or(none, 6mm)` recovers the determined-`none` to its
/// default `6mm` with `Freshness::Final`; the companion `unwrap_or(some(5mm),
/// 6mm)` evaluates to `5mm` (tag-driven, not the blanket default).
///
/// Pins INV-1 from the "language recovery works on determined-none" side.
#[test]
fn determined_none_is_recovered_by_unwrap_or_to_default_final() {
    let src = r#"
structure S {
    param o_none : Option<Length> = none
    param o_some : Option<Length> = some(5mm)
    let recovered = unwrap_or(o_none, 6mm)
    let kept = unwrap_or(o_some, 6mm)
}
"#;
    let (engine, result) = eval_module(src);

    // (1) Determined none recovers to the default 6mm.
    let recovered_id = ValueCellId::new("S", "recovered");
    let recovered = result.values.get(&recovered_id).unwrap_or_else(|| {
        panic!(
            "eval must produce S.recovered; available: {:?}",
            result.values.iter().map(|(k, _)| k.to_string()).collect::<Vec<_>>()
        )
    });
    assert_eq!(
        *recovered,
        mm(6.0),
        "unwrap_or(none, 6mm) must recover to 6mm; got {:?}",
        recovered
    );

    // (2) Freshness of the recovered cell is Final (clean eval, not stale).
    let recovered_node = NodeId::Value(recovered_id.clone());
    assert_eq!(
        engine.cache_store().freshness(&recovered_node),
        Freshness::Final,
        "S.recovered must have Freshness::Final after clean eval"
    );

    // (3) Tag-driven: some(5mm) yields 5mm, not the 6mm default.
    let kept_id = ValueCellId::new("S", "kept");
    let kept = result.values.get(&kept_id).expect("eval must produce S.kept");
    assert_eq!(
        *kept,
        mm(5.0),
        "unwrap_or(some(5mm), 6mm) must yield 5mm (tag-driven); got {:?}",
        kept
    );
}

/// Side (b) facet 1: when the `unwrap_or` LET cell itself is forced-Failed via
/// `set_panic_on_eval`, it stays `Freshness::Failed`; the language-recovery
/// combinator never fires, so the value is absent/Undef (NOT mm(5.0), NOT the
/// mm(6.0) default).
///
/// Pins INV-1 from the "graph-Failed cell is NOT recovered" side.
/// Pattern source: `failed_propagation.rs::forced_panic_on_let_binding_marks_failed_and_emits_one_failed_event`.
#[test]
fn graph_failed_unwrap_or_cell_stays_failed_and_is_not_recovered() {
    let src = r#"
structure S {
    param o_some : Option<Length> = some(5mm)
    let present = unwrap_or(o_some, 6mm)
}
"#;
    let compiled = parse_and_compile_with_stdlib(src);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(errors.is_empty(), "fixture must compile clean; got: {:?}", errors);

    let present_id = ValueCellId::new("S", "present");
    let present_node = NodeId::Value(present_id.clone());

    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), None);
    // Force a panic on the unwrap_or LET cell.
    engine.set_panic_on_eval(present_id.clone());
    let result = engine.eval(&compiled);

    // (i) The cell's freshness is Failed with a non-empty error message.
    let freshness = engine.cache_store().freshness(&present_node);
    match &freshness {
        Freshness::Failed { error } => {
            assert!(
                !error.message().is_empty(),
                "Failed error message must be non-empty"
            );
        }
        other => panic!(
            "S.present must be Freshness::Failed after forced panic on the LET cell; \
             got {:?}",
            other
        ),
    }

    // (ii) The value is absent or Undef — the language recovery never fired.
    //      The cell must NOT hold mm(5.0) (the unwrapped value) or mm(6.0)
    //      (the default that the combinator would have returned on determined-none).
    match result.values.get(&present_id) {
        None | Some(Value::Undef) => { /* expected — no value was produced */ }
        Some(v) => panic!(
            "S.present must be absent or Value::Undef after forced-Failed eval; \
             got {:?} — the language recovery must NOT have fired on a Failed cell",
            v
        ),
    }

    // (iii) Exactly one EventKind::Failed, scoped to NodeId::Value(S.present).
    let failed_count = engine
        .journal()
        .count_matching(|k| matches!(k, EventKind::Failed { .. }));
    assert_eq!(
        failed_count, 1,
        "exactly one EventKind::Failed must be recorded after forced panic"
    );
    let present_events = engine.journal().events_for_node(&present_node);
    let failed_events: Vec<_> = present_events
        .iter()
        .filter(|e| matches!(e.kind, EventKind::Failed { .. }))
        .collect();
    assert_eq!(
        failed_events.len(),
        1,
        "the Failed event must be scoped to NodeId::Value(S.present)"
    );
}

/// Side (b) facet 2 (C-4): a downstream `unwrap_or` consumer of a force-Failed
/// upstream Option LET becomes `Freshness::Pending` rooted at the failed node —
/// it is NOT recovered to the mm(6.0) default.
///
/// Two-pass incremental eval is REQUIRED: the pre-eval Pending gate attaches
/// `pending_cause` only when a prior cache entry exists (cold eval falls through
/// to Pending-without-cause per engine_eval.rs:5208-5215).
///
/// Pattern source: `failed_propagation.rs::panic_in_leaf_propagates_pending_with_chain_to_mid_and_quiets_downstream`.
#[test]
fn graph_failed_input_is_not_recovered_by_downstream_unwrap_or() {
    let src = r#"
structure S {
    param o_some : Option<Length> = some(5mm)
    let failing = or_else(o_some, o_some)
    let consumer = unwrap_or(failing, 6mm)
}
"#;
    let compiled = parse_and_compile_with_stdlib(src);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(errors.is_empty(), "fixture must compile clean; got: {:?}", errors);

    let failing_id = ValueCellId::new("S", "failing");
    let consumer_id = ValueCellId::new("S", "consumer");
    let failing_node = NodeId::Value(failing_id.clone());
    let consumer_node = NodeId::Value(consumer_id.clone());

    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), None);

    // === Pass 1: cold-start, all-Final baseline ===
    // or_else(some(5mm), some(5mm)) = some(5mm); unwrap_or(some(5mm), 6mm) = 5mm.
    let result1 = engine.eval(&compiled);

    assert_eq!(
        *result1
            .values
            .get(&consumer_id)
            .expect("S.consumer must be present in Pass 1"),
        mm(5.0),
        "Pass 1: S.consumer must be mm(5.0)"
    );
    assert_eq!(
        engine.cache_store().freshness(&consumer_node),
        Freshness::Final,
        "Pass 1: S.consumer must be Freshness::Final"
    );
    assert_eq!(
        engine.cache_store().freshness(&failing_node),
        Freshness::Final,
        "Pass 1: S.failing must be Freshness::Final"
    );

    // === Pass 2: force panic on the upstream `failing` LET cell ===
    engine.set_panic_on_eval(failing_id.clone());
    let result2 = engine.eval(&compiled);

    // (i) S.failing is Freshness::Failed.
    let failing_freshness = engine.cache_store().freshness(&failing_node);
    assert!(
        matches!(failing_freshness, Freshness::Failed { .. }),
        "Pass 2 (i): S.failing must be Freshness::Failed after forced panic; \
         got {:?}",
        failing_freshness
    );

    // (ii) S.consumer is Freshness::Pending (not Final, not the 6mm default).
    //      The §9.2 carve-out: Failed input → downstream Pending.
    let consumer_freshness = engine.cache_store().freshness(&consumer_node);
    assert!(
        matches!(consumer_freshness, Freshness::Pending { .. }),
        "Pass 2 (ii): S.consumer must be Freshness::Pending after its upstream \
         (S.failing) became Failed; got {:?}. \
         The language-recovery combinator must NOT have fired (consumer is not \
         Final with the 6mm default).",
        consumer_freshness
    );

    // (iii) pending_cause(S.consumer) == NodeId::Value(S.failing) —
    //       the Failed lineage propagated; C-4 recovery did NOT fire.
    assert_eq!(
        engine.cache_store().pending_cause(&consumer_node),
        Some(failing_node.clone()),
        "Pass 2 (iii): S.consumer's pending_cause must point at S.failing \
         (the Failed upstream), confirming C-4: a Failed input is NOT recovered \
         to the default by the downstream unwrap_or combinator"
    );

    // (iv) S.consumer's value in result2 is NOT the mm(6.0) default —
    //      the language-recovery combinator did not apply.
    if let Some(v) = result2.values.get(&consumer_id) {
        assert_ne!(
            *v,
            mm(6.0),
            "Pass 2 (iv): S.consumer must NOT hold mm(6.0) (the unwrap_or \
             default) — language recovery must NOT have fired on the Failed upstream"
        );
    }
    // None is also valid for a Pending cell.

    // (v) Exactly one EventKind::Failed, scoped to NodeId::Value(S.failing).
    let failed_count = engine
        .journal()
        .count_matching(|k| matches!(k, EventKind::Failed { .. }));
    assert_eq!(
        failed_count, 1,
        "Pass 2 (v): exactly one EventKind::Failed must be recorded, \
         scoped to the forced-panic cell (S.failing)"
    );
    let failing_events = engine.journal().events_for_node(&failing_node);
    let failed_events: Vec<_> = failing_events
        .iter()
        .filter(|e| matches!(e.kind, EventKind::Failed { .. }))
        .collect();
    assert_eq!(
        failed_events.len(),
        1,
        "Pass 2 (v): the Failed event must be scoped to NodeId::Value(S.failing)"
    );
}
