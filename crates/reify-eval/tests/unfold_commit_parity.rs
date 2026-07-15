//! Parity fixture for eval cell-commit ε (task #5057): migrating the two
//! inline eval-and-commit copies in `unfold.rs` (`elaborate_child_params_only`,
//! `elaborate_child_lets_only`) onto the `commit_cell_result` primitive (task
//! α, #5038) + the required-args `cell_eval_ctx` free-fn (task β, #5039).
//!
//! This is a behaviour-preserving refactor: committed values, determinacy,
//! and no-garbage-on-skip semantics do not change, so those assertions are
//! parity locks (green throughout). The one new observable the migration
//! introduces is `commit_cell_result` writing the journal `Started` event's
//! payload as `EventPayload::Custom("guarded-group")` — today's hand-inlined
//! commit writes `payload: None`. That provenance assertion is the RED→GREEN
//! driver for each site's migration step.
//!
//! Conventions follow `tests/recursive_unfold.rs` (`TopologyTemplateBuilder`,
//! `reify_test_support::builders`, `Engine::eval`).

use reify_core::*;
use reify_eval::Engine;
use reify_eval::cache::NodeId;
use reify_eval::journal::{EventKind, EventPayload};
use reify_ir::*;
use reify_test_support::builders::{binop, gt, literal, value_ref_typed};
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::{CompiledModuleBuilder, TopologyTemplateBuilder};

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Build a simple recursive structure S with:
///   param n: Int = `n_default`
///   sub child = S(n: n-1) where n > 0
///   is_recursive = true
///
/// Mirrors `tests/recursive_unfold.rs::build_recursive_s` (integration test
/// binaries cannot share private helpers across files).
fn build_recursive_s(n_default: i64) -> reify_compiler::TopologyTemplate {
    let guard = gt(value_ref_typed("S", "n", Type::Int), literal(Value::Int(0)));
    let n_minus_1 = binop(
        BinOp::Sub,
        value_ref_typed("S", "n", Type::Int),
        literal(Value::Int(1)),
    );

    TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "n",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(n_default), Type::Int)),
        )
        .is_recursive(true)
        .sub_component_with_guard("child", "S", vec![("n".to_string(), n_minus_1)], guard)
        .build()
}

/// Create a single-template module and run eval on it, returning both the
/// engine (for post-eval `snapshot()`/`journal()` inspection) and the result.
fn eval_single_template(
    template: reify_compiler::TopologyTemplate,
) -> (Engine, reify_eval::EvalResult) {
    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let result = engine.eval(&module);
    (engine, result)
}

/// True if `events` contains a `Started` event carrying
/// `EventPayload::Custom("guarded-group")` — the provenance slug
/// `commit_cell_result` writes for `TraceSource::GuardedGroup` commits.
fn has_guarded_group_started_provenance(events: &[&reify_eval::journal::EvalEvent]) -> bool {
    events.iter().any(|e| {
        matches!(&e.kind, EventKind::Started)
            && matches!(&e.payload, Some(EventPayload::Custom(s)) if s == "guarded-group")
    })
}

// ─── step-1: param-site parity + RED provenance signal ───────────────────────

/// Site 1 (`elaborate_child_params_only`, unfold.rs :336-375) parity +
/// journal-provenance RED signal.
///
/// S(n=2) unfolds one level: S.child(n=1). Assertions (a)/(b) are parity
/// locks (green throughout — the migration must not change committed value
/// or determinacy). Assertion (c) is RED today: the current param commit
/// writes journal `Started` with `payload: None`, not the
/// `Custom("guarded-group")` slug `commit_cell_result` would write.
#[test]
fn unfold_param_cell_commits_via_primitive() {
    let template = build_recursive_s(2);
    let (engine, result) = eval_single_template(template);

    let child_n = ValueCellId::new("S.child", "n");

    // (a) PARITY: committed value.
    assert_eq!(
        result.values.get(&child_n),
        Some(&Value::Int(1)),
        "S.child.n should be 1 (= 2 - 1)"
    );

    // (b) PARITY: committed determinacy.
    let snap = engine.snapshot().expect("snapshot should exist after eval");
    assert_eq!(
        snap.values.get(&child_n).map(|(_, det)| *det),
        Some(DeterminacyState::Determined),
        "S.child.n determinacy should be Determined"
    );

    // (c) RED: journal Started-event provenance slug.
    let node_id = NodeId::Value(child_n.clone());
    let events = engine.journal().events_for_node(&node_id);
    assert!(
        has_guarded_group_started_provenance(&events),
        "expected a Started event for S.child.n carrying EventPayload::Custom(\"guarded-group\"), \
         got: {:?}",
        events
    );
}
