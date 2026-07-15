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
use reify_test_support::builders::{binop, conditional_expr, gt, literal, value_ref_typed};
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

// ─── step-3: let-site parity + RED provenance signal ─────────────────────────

/// Site 2 (`elaborate_child_lets_only`, unfold.rs :546-611) parity +
/// journal-provenance RED signal.
///
/// Mirrors `tests/recursive_unfold.rs::unfold_recursive_leaves_first_order`:
/// S(n=2) with `let total = if n>0 then n + S.child.total else n` exercises the
/// cross-level (leaves-first) let-binding path. Assertions (a)/(b) are parity
/// locks (green throughout). Assertion (c) is RED today: the current let
/// commit writes journal `Started` with `payload: None`, not the
/// `Custom("guarded-group")` slug `commit_cell_result` would write. The
/// step-2 (site-1/param) migration does not touch let cells, so this stays
/// RED until site 2 migrates.
#[test]
fn unfold_let_cell_commits_via_primitive() {
    let guard = gt(value_ref_typed("S", "n", Type::Int), literal(Value::Int(0)));
    let n_minus_1 = binop(
        BinOp::Sub,
        value_ref_typed("S", "n", Type::Int),
        literal(Value::Int(1)),
    );
    // total = if n > 0 then n + S.child.total else n
    let total_expr = conditional_expr(
        gt(value_ref_typed("S", "n", Type::Int), literal(Value::Int(0))),
        binop(
            BinOp::Add,
            value_ref_typed("S", "n", Type::Int),
            value_ref_typed("S.child", "total", Type::Int),
        ),
        value_ref_typed("S", "n", Type::Int),
    );

    let template = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "n",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(2), Type::Int)),
        )
        .let_binding("S", "total", Type::Int, total_expr)
        .is_recursive(true)
        .sub_component_with_guard("child", "S", vec![("n".to_string(), n_minus_1)], guard)
        .build();

    let (engine, result) = eval_single_template(template);

    let child_total = ValueCellId::new("S.child", "total");

    // (a) PARITY: cross-level committed value. S.child.child (n=0): total = 0
    // (else branch); S.child (n=1): total = 1 + S.child.child.total = 1 + 0 = 1.
    assert_eq!(
        result.values.get(&child_total),
        Some(&Value::Int(1)),
        "S.child.total should be 1 (= 1 + S.child.child.total = 1 + 0)"
    );

    // (b) PARITY: committed determinacy.
    let snap = engine.snapshot().expect("snapshot should exist after eval");
    assert_eq!(
        snap.values.get(&child_total).map(|(_, det)| *det),
        Some(DeterminacyState::Determined),
        "S.child.total determinacy should be Determined"
    );

    // (c) RED: journal Started-event provenance slug.
    let node_id = NodeId::Value(child_total.clone());
    let events = engine.journal().events_for_node(&node_id);
    assert!(
        has_guarded_group_started_provenance(&events),
        "expected a Started event for S.child.total carrying EventPayload::Custom(\"guarded-group\"), \
         got: {:?}",
        events
    );
}

// ─── step-3: B3 no-garbage lock (cyclic let-binding, behaviour preserved) ────

/// Behaviour-preservation lock (green throughout, both before and after the
/// site-2 migration): a cyclic let-binding pair on the recursive template
/// mirrors `tests/recursive_unfold.rs::cyclic_let_bindings_emit_diagnostic`,
/// but targets the `S.child` entity specifically — the level evaluated via
/// `elaborate_child_lets_only`'s cyclic-let detection (unfold.rs :547-566) —
/// rather than the root `S` entity's own (separately-evaluated) let-bindings.
///
/// Asserts the circular-let-binding diagnostic surfaces AND that the cyclic
/// cells are never committed. A pre-existing baseline placeholder —
/// `(Value::Undef, DeterminacyState::Undetermined)` with zero journal events —
/// is already present in `engine.snapshot().values` for `S.child.count_x`/
/// `count_y` before `elaborate_child_lets_only` ever runs (seeded by an
/// earlier pass, confirmed empirically: `events_for_node` returns `[]`).
/// The lock that matters for this migration is that the cyclic-let skip
/// leaves that baseline untouched: determinacy must stay `Undetermined`
/// (never flipped to `Determined`) and no journal `Started`/`Completed`
/// event is ever recorded for the node. Both migrated (`commit_cell_result`)
/// and unmigrated (hand-inlined) code paths must skip the commit entirely
/// for cyclic cells — invoking a commit (or a `CacheLeg::Skip` commit, which
/// still writes values/snapshot/journal) would flip determinacy and record
/// events that do not exist today, which is exactly the garbage this lock
/// forbids.
#[test]
fn unfold_cyclic_let_writes_no_garbage_cache_entry() {
    // let count_x = count_y + 1 (depends on S.count_y)
    let count_x_expr = binop(
        BinOp::Add,
        value_ref_typed("S", "count_y", Type::Int),
        literal(Value::Int(1)),
    );
    // let count_y = count_x + 1 (depends on S.count_x)
    let count_y_expr = binop(
        BinOp::Add,
        value_ref_typed("S", "count_x", Type::Int),
        literal(Value::Int(1)),
    );

    let guard = gt(value_ref_typed("S", "n", Type::Int), literal(Value::Int(0)));
    let n_minus_1 = binop(
        BinOp::Sub,
        value_ref_typed("S", "n", Type::Int),
        literal(Value::Int(1)),
    );

    let template = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "n",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(2), Type::Int)),
        )
        .let_binding("S", "count_x", Type::Int, count_x_expr)
        .let_binding("S", "count_y", Type::Int, count_y_expr)
        .is_recursive(true)
        .sub_component_with_guard("child", "S", vec![("n".to_string(), n_minus_1)], guard)
        .build();

    let (engine, result) = eval_single_template(template);

    // The circular let-binding diagnostic surfaces for the child entity
    // (unfold.rs's elaborate_child_lets_only cyclic-let detection).
    let has_cycle_error_for_child = result.diagnostics.iter().any(|d| {
        d.severity == Severity::Error
            && (d.message.contains("circular")
                || d.message.contains("cycle")
                || d.message.contains("cyclic"))
            && d.message.contains("S.child")
            && d.message.contains("count_x")
            && d.message.contains("count_y")
    });
    assert!(
        has_cycle_error_for_child,
        "expected an error diagnostic about circular let-binding dependency for entity \
         S.child, got: {:?}",
        result.diagnostics
    );

    // No-garbage lock: determinacy stays Undetermined (never flipped to
    // Determined by a commit) and no journal event is ever recorded — for
    // both S.child.count_x and S.child.count_y — behaviour preserved across
    // the migration.
    let snap = engine.snapshot().expect("snapshot should exist after eval");
    for member in ["count_x", "count_y"] {
        let cyclic_id = ValueCellId::new("S.child", member);
        assert_eq!(
            snap.values.get(&cyclic_id).map(|(_, det)| *det),
            Some(DeterminacyState::Undetermined),
            "S.child.{member} determinacy should stay Undetermined (cyclic let, never committed)"
        );
        let events = engine.journal().events_for_node(&NodeId::Value(cyclic_id));
        assert!(
            events.is_empty(),
            "S.child.{member} should have no journal events (cyclic let, never committed \
             via commit_cell_result or its predecessor), got {:?}",
            events
        );
    }
}

#[test]
fn experiment_determinacy_predicate_in_child_let() {
    let n_ready_expr =
        CompiledExpr::determinacy_predicate(DeterminacyPredicateKind::Determined, ValueCellId::new("S", "n"));

    let guard = gt(value_ref_typed("S", "n", Type::Int), literal(Value::Int(0)));
    let n_minus_1 = binop(
        BinOp::Sub,
        value_ref_typed("S", "n", Type::Int),
        literal(Value::Int(1)),
    );

    let template = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "n",
            Type::Int,
            Some(CompiledExpr::literal(Value::Int(2), Type::Int)),
        )
        .let_binding("S", "n_ready", Type::Bool, n_ready_expr)
        .is_recursive(true)
        .sub_component_with_guard("child", "S", vec![("n".to_string(), n_minus_1)], guard)
        .build();

    let (_engine, result) = eval_single_template(template);

    eprintln!("diagnostics = {:#?}", result.diagnostics);
    eprintln!("S.n_ready = {:?}", result.values.get(&ValueCellId::new("S", "n_ready")));
    eprintln!(
        "S.child.n_ready = {:?}",
        result.values.get(&ValueCellId::new("S.child", "n_ready"))
    );
    eprintln!(
        "S.child.child.n_ready = {:?}",
        result.values.get(&ValueCellId::new("S.child.child", "n_ready"))
    );
    panic!("experiment observation printed above");
}
