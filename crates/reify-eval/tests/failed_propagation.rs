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

use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
use reify_constraints::SimpleConstraintChecker;
use reify_eval::Engine;
use reify_eval::cache::{CachedResult, FAILED_REALIZATION_STUB_HANDLE, NodeId};
use reify_eval::journal::EventKind;
use reify_test_support::builders::{binop, gt, literal, value_ref_typed};
use reify_test_support::mocks::{FailingMockGeometryKernel, MockConstraintChecker};
use reify_test_support::{CompiledModuleBuilder, TopologyTemplateBuilder, mm};
use reify_types::{
    BinOp, CompiledExpr, ConstraintNodeId, DiagnosticCode, ExportFormat, Freshness,
    GeometryHandleId, ModulePath, RealizationNodeId, Satisfaction, Severity, Type, Value,
    ValueCellId, VersionId,
};

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

/// Pin the recovery path for the test-instrumentation panic hook.
///
/// After `set_panic_on_eval(b)` drives `b` to `Freshness::Failed`,
/// calling `remove_panic_on_eval(&b)` withdraws the injection and
/// a subsequent `eval` must restore `b` to a non-Failed freshness state.
///
/// This covers the recovery branch (`remove_panic_on_eval` → re-eval)
/// that `forced_panic_on_let_binding_marks_failed_and_emits_one_failed_event`
/// leaves unpinned; together the two tests form the complete
/// set → eval → Failed → remove → re-eval → recovered contract.
///
/// See arch §9.1 and plan #2555 for the cfg-gating rationale; this test
/// pins the public-API contract that the refactor must preserve.
///
/// Assertions:
///   (a) After `set_panic_on_eval(b)` and the first `eval`, freshness(b)
///       is `Freshness::Failed { .. }` (mirrors the existing test).
///   (b) `remove_panic_on_eval(&b)` returns `true` (cell was registered).
///   (c) After a second `eval`, freshness(b) is `Final` and the cached
///       value is `Value::Real(1.0)` — the recovery branch re-evaluates
///       the cell cleanly and produces the expected result.
///   (d) A second call to `remove_panic_on_eval(&b)` returns `false`
///       (the cell is no longer in the injection set).
#[test]
fn forced_panic_recovers_after_remove_panic_on_eval() {
    let module = one_cell_module();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);

    let b_id = ValueCellId::new("T", "b");
    let b_node = NodeId::Value(b_id.clone());

    // === Pass 1: force panic on b ===
    engine.set_panic_on_eval(b_id.clone());
    let _ = engine.eval(&module);

    // (a) b is Failed after the forced panic.
    let b_freshness_pass1 = engine.cache_store().freshness(&b_node);
    assert!(
        matches!(b_freshness_pass1, Freshness::Failed { .. }),
        "(a) freshness(b) must be Failed after set_panic_on_eval + eval; got {:?}",
        b_freshness_pass1
    );

    // (b) remove_panic_on_eval returns true (cell was registered).
    let removed = engine.remove_panic_on_eval(&b_id);
    assert!(
        removed,
        "(b) remove_panic_on_eval must return true when b was registered"
    );

    // === Pass 2: re-eval after removing the panic injection ===
    let _ = engine.eval(&module);

    // (c) b recovers to Final with the expected value after re-eval.
    let b_cache_pass2 = engine
        .cache_store()
        .get(&b_node)
        .expect("(c) b must be cached after re-eval");
    assert_eq!(
        b_cache_pass2.freshness,
        Freshness::Final,
        "(c) freshness(b) must be Final after remove_panic_on_eval + re-eval; \
         got {:?}",
        b_cache_pass2.freshness
    );
    match &b_cache_pass2.result {
        CachedResult::Value(v, _) => assert_eq!(
            *v,
            Value::Real(1.0),
            "(c) recovered value must be Value::Real(1.0); got {:?}",
            v
        ),
        other => panic!(
            "(c) cache result for b must be CachedResult::Value; got {:?}",
            other
        ),
    }

    // (d) A second call to remove_panic_on_eval returns false (no longer registered).
    let removed_again = engine.remove_panic_on_eval(&b_id);
    assert!(
        !removed_again,
        "(d) remove_panic_on_eval must return false when b is no longer registered"
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

/// Build a single-template module with one always-false constraint:
///   `param x : Real = 5.0; constraint c0 : x > 100.0`
///
/// `x > 100.0` evaluates to `Bool(false)` for the default `x = 5.0`,
/// driving `SimpleConstraintChecker` into the `Satisfaction::Violated`
/// branch with a `DiagnosticCode::ConstraintViolated` Diagnostic.
fn always_false_constraint_module() -> reify_compiler::CompiledModule {
    let e = "T";
    CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(
            TopologyTemplateBuilder::new(e)
                .param(
                    e,
                    "x",
                    Type::Real,
                    Some(literal(Value::Real(5.0))),
                )
                .constraint(
                    e,
                    0,
                    Some("x_gt_100"),
                    gt(
                        value_ref_typed(e, "x", Type::Real),
                        literal(Value::Real(100.0)),
                    ),
                )
                .build(),
        )
        .build()
}

/// Regression test pinning the arch §9.3 separation between constraint
/// satisfaction and value-cell freshness:
///
///   A constraint that evaluates to `false` must produce a
///   `Satisfaction::Violated` `ConstraintCheckEntry` plus a
///   `DiagnosticCode::ConstraintViolated` Diagnostic, but it must NOT
///   touch any node's `Freshness::Failed { .. }` and it must NOT emit
///   any `EventKind::Failed` event.
///
/// `Freshness::Failed` is reserved for evaluation-pipeline failures (panic
/// boundary, kernel error). Conflating constraint violations with
/// `Failed` would silently fold two orthogonal channels into one, break
/// downstream consumers that filter on `EventKind::Failed`, and break
/// `pending_cause` chains into nodes that should never have been
/// chain roots.
///
/// The constraint pipeline (`SimpleConstraintChecker` →
/// `ConstraintCheckEntry` → `engine_constraints.rs::push_constraint_result`)
/// already keeps the two channels separate by construction; this test
/// pins that contract against future refactors.
///
/// See arch `docs/reify-implementation-architecture.md` §9.3 lines 891-905
/// and the corresponding design decision in plan #2330.
#[test]
fn constraint_violation_does_not_produce_failed_freshness_or_error_event() {
    let module = always_false_constraint_module();
    let checker = SimpleConstraintChecker;
    let mut engine = Engine::new(Box::new(checker), None);

    let check_result = engine.check(&module);

    let e = "T";
    let x_id = ValueCellId::new(e, "x");
    let c0_id = ConstraintNodeId::new(e, 0);

    // Sanity: ensure the test setup actually exercises the violation
    // pipeline. If this assertion ever flips, the rest of the test
    // becomes vacuous — we'd be asserting "no Failed produced" by a
    // pipeline that never ran.
    let x_value = check_result
        .values
        .get(&x_id)
        .expect("x must be present in CheckResult.values after engine.check");
    assert_eq!(
        x_value,
        &Value::Real(5.0),
        "test setup: x must hold its default Real(5.0) so the \
         constraint x > 100.0 actually evaluates to false"
    );

    // (a) constraint_results contains an entry with Satisfaction::Violated.
    let violated_entries: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|entry| entry.id == c0_id && entry.satisfaction == Satisfaction::Violated)
        .collect();
    assert_eq!(
        violated_entries.len(),
        1,
        "(a) §9.3: exactly one Satisfaction::Violated entry must be \
         recorded for c0; got constraint_results = {:?}",
        check_result.constraint_results
    );

    // (b) diagnostics include exactly one Diagnostic with
    //     code == Some(DiagnosticCode::ConstraintViolated) and
    //     Severity::Error. SimpleConstraintChecker emits this on the
    //     Bool(false) branch (reify-constraints/src/lib.rs:43-49).
    let constraint_violated_diagnostics: Vec<_> = check_result
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && d.code == Some(DiagnosticCode::ConstraintViolated)
        })
        .collect();
    assert_eq!(
        constraint_violated_diagnostics.len(),
        1,
        "(b) §9.3: exactly one Severity::Error Diagnostic with \
         code == Some(DiagnosticCode::ConstraintViolated) must be \
         recorded; got diagnostics = {:?}",
        check_result.diagnostics
    );

    // (c) NO cache entry has Freshness::Failed. The two NodeIds we
    //     might expect to find here are NodeId::Value(x) (param cell)
    //     and NodeId::Constraint(c0). Neither must be Failed.
    //
    //     `freshness()` returns `Freshness::Final` (the default) for
    //     absent nodes — that is also not Failed, so the assertion
    //     stays robust whether a constraint-only check populates a
    //     cache entry for c0 or not.
    let x_freshness = engine.cache_store().freshness(&NodeId::Value(x_id.clone()));
    assert!(
        !matches!(x_freshness, Freshness::Failed { .. }),
        "(c) §9.3: NodeId::Value(x) must NOT have Freshness::Failed \
         after a Violated-only constraint pass; got {:?}",
        x_freshness
    );
    let c0_freshness = engine
        .cache_store()
        .freshness(&NodeId::Constraint(c0_id.clone()));
    assert!(
        !matches!(c0_freshness, Freshness::Failed { .. }),
        "(c) §9.3: NodeId::Constraint(c0) must NOT have Freshness::Failed \
         after a Violated-only constraint pass; got {:?}",
        c0_freshness
    );

    // (d) journal records ZERO EventKind::Failed events.
    //     `EventKind::Failed` is reserved for evaluation-pipeline
    //     failures (arch §9.1-§9.2). A constraint violation must never
    //     emit one.
    let failed_count = engine
        .journal()
        .count_matching(|k| matches!(k, EventKind::Failed { .. }));
    assert_eq!(
        failed_count, 0,
        "(d) §9.3: NO EventKind::Failed event must be recorded for a \
         Violated-only constraint pass; got {} Failed event(s)",
        failed_count
    );
}

/// Assert the §9.1 / §2554 version-tagging contract for a kernel-error
/// `Failed` event scoped to `node`:
///
///   (c) Exactly one `EventKind::Failed` is recorded in the journal.
///   (d) That event is scoped to `node`.
///   (f) `event.version == expected` — the Failed event carries the eval
///       round whose values caused the kernel error, not the un-used
///       `next_version_id`.
///
/// Called from both the `build` and `build_snapshot` regression tests so
/// the contract cannot silently regress at one site while the other is updated.
fn assert_one_failed_event_at_version(
    engine: &Engine,
    node: &NodeId,
    expected: reify_types::VersionId,
) {
    // (c) exactly one Failed event in the journal.
    let failed_count = engine
        .journal()
        .count_matching(|k| matches!(k, EventKind::Failed { .. }));
    assert_eq!(
        failed_count,
        1,
        "(c) §9.1: exactly one Failed event must be recorded; got {} event(s)",
        failed_count
    );

    // (d) that event is scoped to `node`.
    let r_events = engine.journal().events_for_node(node);
    let r_failed: Vec<_> = r_events
        .iter()
        .filter(|ev| matches!(ev.kind, EventKind::Failed { .. }))
        .collect();
    assert_eq!(
        r_failed.len(),
        1,
        "(d) §9.1: the Failed event must be scoped to {:?}; got {} event(s)",
        node,
        r_failed.len()
    );

    // (f) version matches the expected eval round.
    assert_eq!(
        r_failed[0].version,
        expected,
        "(f) §2554: Failed event version must match the eval round whose \
         values caused the kernel error; got {:?}, expected {:?}",
        r_failed[0].version,
        expected
    );
}

/// Build a single-realization module with one Box primitive op:
///   `param width:Length=80mm; param height:Length=100mm; param depth:Length=5mm;`
///   plus `realization[0] = Box(width, height, depth)`.
///
/// `FailingMockGeometryKernel::execute` always returns
/// `Err(GeometryError::OperationFailed("simulated kernel failure"))`, so the
/// realization triggers the §9.1 kernel-error path inside
/// `execute_realization_ops`. The realization NodeId is
/// `RealizationNodeId::new("KernelFail", 0)`.
fn one_realization_box_module() -> reify_compiler::CompiledModule {
    let e = "KernelFail";
    let mm_lit = |v: f64| CompiledExpr::literal(mm(v), Type::length());

    let box_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Box,
        args: vec![
            ("width".into(), mm_lit(80.0)),
            ("height".into(), mm_lit(100.0)),
            ("depth".into(), mm_lit(5.0)),
        ],
    };

    let template = TopologyTemplateBuilder::new(e)
        .param(e, "width", Type::length(), Some(mm_lit(80.0)))
        .param(e, "height", Type::length(), Some(mm_lit(100.0)))
        .param(e, "depth", Type::length(), Some(mm_lit(5.0)))
        .realization(e, 0, vec![box_op])
        .build();

    CompiledModuleBuilder::new(ModulePath::single("test_kernel_fail"))
        .template(template)
        .build()
}

/// Regression test pinning the §9.1 kernel-error → `Freshness::Failed`
/// production path on the realization NodeId.
///
/// When `kernel.execute(...)` returns `Err(...)` from
/// `engine_build.rs::execute_realization_ops`, the engine must:
///
///   (a) Mark the realization NodeId as
///       `Freshness::Failed { error }` in the cache.
///   (b) The error message must include the wrapped geometry error string
///       (the same "geometry error: …" prefix already used by the
///       Diagnostic).
///   (c) Emit exactly one `EventKind::Failed` event in the journal.
///   (d) Scope that event to `NodeId::Realization(rnid)`.
///   (e) The pre-existing `Diagnostic::error("geometry error: …")` must
///       still be present in `BuildResult.diagnostics` — the existing
///       diagnostic surface must NOT be removed by the new Failed-write
///       behaviour.
///
/// Implements arch §9.1 lines 868–877 ("kernel.execute(...) Err → mark
/// realization Failed + emit one error event"). This is the second
/// Failed-production path (besides the `evaluate_let_bindings` panic
/// boundary covered by step-11/step-12).
///
/// See plan #2330 step-17 / step-18 for the design.
#[test]
fn kernel_execute_error_marks_realization_failed_and_emits_one_error_event() {
    let module = one_realization_box_module();
    let checker = MockConstraintChecker::new();
    let kernel = FailingMockGeometryKernel;
    let mut engine = Engine::new(Box::new(checker), Some(Box::new(kernel)));

    let e = "KernelFail";
    let rnid = RealizationNodeId::new(e, 0);
    let r_node = NodeId::Realization(rnid.clone());

    // Pin the expected eval-round version *before* `build()` runs. After
    // `Engine::new`, `next_version_id == 0`; `build()` calls `check()` which
    // calls `eval()`, stamping `snapshot.version = VersionId(0)` and bumping
    // `next_version_id` to 1. Hardcoding the expectation (rather than reading
    // `engine.snapshot().version` after `build()` returns) means a future bug
    // that bumps the version mid-build would fail this assertion instead of
    // silently tracking the buggy value.
    let expected_version = VersionId(0);

    let build_result = engine.build(&module, ExportFormat::Step);

    // (a) freshness(NodeId::Realization(rnid)) == Failed { error }.
    let r_freshness = engine.cache_store().freshness(&r_node);
    let error_message = match &r_freshness {
        Freshness::Failed { error } => error.message().to_string(),
        other => panic!(
            "(a) §9.1: realization NodeId must be Failed after kernel \
             error; got {:?}",
            other
        ),
    };

    // (b) the error message wraps the geometry error string.
    //     `FailingMockGeometryKernel` raises
    //     `OperationFailed("simulated kernel failure")` and
    //     `execute_realization_ops` already prefixes "geometry error: ".
    assert!(
        error_message.contains("geometry error"),
        "(b) §9.1: Failed error message must wrap the geometry error \
         string; got {:?}",
        error_message
    );
    assert!(
        error_message.contains("simulated kernel failure"),
        "(b) §9.1: Failed error message must include the kernel's own \
         error text; got {:?}",
        error_message
    );

    // (c)/(d)/(f) — shared helper pins the count, node scope, and version
    // tagging contract; see `assert_one_failed_event_at_version`.
    assert_one_failed_event_at_version(&engine, &r_node, expected_version);

    // (e) the existing Diagnostic::error("geometry error: …") survives —
    //     adding the Failed write must not double-handle and remove the
    //     existing diagnostic surface.
    let geom_diags = build_result
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("geometry error"))
        .count();
    assert!(
        geom_diags >= 1,
        "(e) §9.1: the pre-existing Diagnostic::error(\"geometry \
         error: …\") must still be emitted alongside the Failed write; \
         got 0 such diagnostics in {:?}",
        build_result.diagnostics
    );
}

/// Regression test pinning the §9.1 kernel-error → `Freshness::Failed`
/// version tagging on the `build_snapshot` path.
///
/// Mirrors the version-assertion logic of
/// `kernel_execute_error_marks_realization_failed_and_emits_one_error_event`
/// for `Engine::build_snapshot` (engine_build.rs:45). The buggy code reads
/// `VersionId(self.next_version_id)` which is ahead of
/// `state.snapshot.version` after `eval()` + `edit_param()` have both bumped
/// the counter. Running `edit_param` between `eval` and `build_snapshot`
/// ensures `snapshot.version != 0`, so a constant-zero miswire is also caught.
///
/// Flow:
///   1. `eval()` → populates eval_state, snapshot.version = 0.
///   2. `edit_param(width, 90mm)` → snapshot.version = 1, next_version_id = 2.
///   3. `build_snapshot()` fires the failing kernel.
///
/// Assertions: exactly one `EventKind::Failed` scoped to the realization
/// NodeId, and `event.version == snapshot.version` (1, not 2).
#[test]
fn kernel_execute_error_in_build_snapshot_tags_failed_event_with_snapshot_version() {
    let module = one_realization_box_module();
    let checker = MockConstraintChecker::new();
    let kernel = FailingMockGeometryKernel;
    let mut engine = Engine::new(Box::new(checker), Some(Box::new(kernel)));

    let e = "KernelFail";
    let rnid = RealizationNodeId::new(e, 0);
    let r_node = NodeId::Realization(rnid.clone());

    // Step 1: eval to populate eval_state (eval does not call kernel.execute).
    engine.eval(&module);

    // Step 2: edit_param bumps snapshot.version from 0 to 1 and
    // next_version_id from 1 to 2, so the off-by-one is non-trivial.
    engine
        .edit_param(ValueCellId::new(e, "width"), mm(90.0))
        .expect("edit_param must succeed on a valid param");

    // Capture the canonical eval-round version BEFORE invoking the code under
    // test. `build_snapshot` must NOT mutate `snapshot.version`, so pinning the
    // value here means a future bug that bumps version mid-build_snapshot would
    // fail this assertion instead of silently tracking the buggy value.
    let eval_version = engine
        .snapshot()
        .expect("eval_state must be populated after eval+edit_param")
        .version;

    // Step 3: build_snapshot fires the failing kernel → triggers §9.1 path.
    let _build_result = engine.build_snapshot(&module, ExportFormat::Step);

    // (c)/(d)/(f) — shared helper pins the count, node scope, and version
    // tagging contract; see `assert_one_failed_event_at_version`.
    assert_one_failed_event_at_version(&engine, &r_node, eval_version);
}

/// `FAILED_REALIZATION_STUB_HANDLE` is the documented sentinel
/// that `Engine::mark_realization_failed` embeds inside a cold-start
/// `NodeCache` when no prior `GeometryHandle` exists. This test pins
/// the const's identity invariants (so `0`-collision regressions are
/// caught) and pins that the stub never escapes the `Freshness::Failed`
/// gate during an end-to-end kernel-failure build.
///
/// Reviewer (#2330 amendment) flagged that the original
/// `GeometryHandleId(0)` placeholder was plausibly indistinguishable
/// from a real first-allocated handle in counters that start at zero.
/// `FAILED_REALIZATION_STUB_HANDLE` is `u64::MAX - 1` — adjacent to
/// `GeometryHandleId::INVALID` (`u64::MAX`) but not equal to it, so
/// `GeometryHandleId::content_hash` does not debug-assert.
#[test]
fn failed_realization_stub_handle_is_distinct_from_zero_and_invalid() {
    // Identity invariants — pin the const in case someone "simplifies" it
    // back to `GeometryHandleId(0)` or aliases it onto INVALID.
    assert_ne!(
        FAILED_REALIZATION_STUB_HANDLE,
        GeometryHandleId(0),
        "FAILED_REALIZATION_STUB_HANDLE must NOT be GeometryHandleId(0); \
         kernels that start handle counters at 0 would conflate the stub \
         with a real allocated handle"
    );
    assert_ne!(
        FAILED_REALIZATION_STUB_HANDLE,
        GeometryHandleId::INVALID,
        "FAILED_REALIZATION_STUB_HANDLE must NOT equal GeometryHandleId::INVALID; \
         GeometryHandleId::content_hash debug-asserts on INVALID, so embedding \
         INVALID in a NodeCache::new(...) result would crash"
    );

    // End-to-end: the stub IS what the cold-start fallback embeds, AND
    // it is gated by Freshness::Failed.
    let module = one_realization_box_module();
    let checker = MockConstraintChecker::new();
    let kernel = FailingMockGeometryKernel;
    let mut engine = Engine::new(Box::new(checker), Some(Box::new(kernel)));

    let e = "KernelFail";
    let rnid = RealizationNodeId::new(e, 0);
    let r_node = NodeId::Realization(rnid.clone());

    let _ = engine.build(&module, ExportFormat::Step);

    // The cache entry exists.
    let entry = engine
        .cache_store()
        .get(&r_node)
        .expect("cold-start fallback must insert a NodeCache for the failed realization");

    // Freshness gate is Failed — consumers MUST observe this before reading.
    assert!(
        matches!(entry.freshness, Freshness::Failed { .. }),
        "freshness gate must be Failed before consumers reach the stub handle; got {:?}",
        entry.freshness
    );

    // The cached result IS the documented stub, not an arbitrary `0` or
    // INVALID. Pins that `mark_realization_failed` is wired through the
    // const, not a stray literal.
    match &entry.result {
        CachedResult::GeometryHandle(h) => {
            assert_eq!(
                *h, FAILED_REALIZATION_STUB_HANDLE,
                "cold-start failed-realization fallback must embed \
                 FAILED_REALIZATION_STUB_HANDLE; got {:?}",
                h
            );
        }
        other => panic!("expected CachedResult::GeometryHandle stub, got {:?}", other),
    }
}
