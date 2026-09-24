//! Guarded-groups fall-through tests for `Engine::eval_cached` (task 5240).
//!
//! `eval_cached`'s unified Param+Let pass builds its dependency graph via
//! `build_combined_param_let_graph` (engine_eval.rs:509), which iterates ONLY
//! `template.value_cells`. Guarded param/let group members live in a separate
//! compiled-IR collection (`template.guarded_groups`) and are therefore never
//! visited, never written to `values`/`snapshot_values`. A plain reader
//! silently sees a `where`-guarded cell as missing; a sibling `determined(x)`
//! DeterminacyPredicate probe reads the never-populated determinacy snapshot
//! entry, hitting the debug_assert at reify-expr/src/lib.rs:894 ("references
//! cell ... not in determinacy snapshot") — the esc-5053-5 panic.
//!
//! These tests lock the fix: any module with a non-empty `guarded_groups`
//! falls all the way through to a full cold `eval()`, returning correct
//! guarded-cell values and a diagnostic warning naming the incremental-path
//! bypass, instead of silently dropping the cells.

use reify_compiler::{ValueCellDecl, ValueCellKind, Visibility};
use reify_core::{DiagnosticCode, ModulePath, SourceSpan, Type, ValueCellId, VersionId};
use reify_eval::Engine;
use reify_ir::{CompiledExpr, DeterminacyPredicateKind, Value};
use reify_test_support::builders::value_ref_typed;
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::{CompiledModuleBuilder, TopologyTemplateBuilder};

/// Helper to create a ValueCellDecl for tests (mirrors guard_eval.rs's
/// `make_param_decl`).
fn make_param_decl(entity: &str, member: &str, cell_type: Type, default: Value) -> ValueCellDecl {
    ValueCellDecl {
        id: ValueCellId::new(entity, member),
        kind: ValueCellKind::Param,
        visibility: Visibility::Public,
        is_aux: false,
        cell_type: cell_type.clone(),
        default_expr: Some(CompiledExpr::literal(default, cell_type)),
        solver_hints: Vec::new(),
        span: SourceSpan::new(0, 0),
    }
}

/// esc-5053-5 repro: a guarded member param `x` (default 5mm, guard `active`
/// default true) plus a top-level `let probe = determined(x)`.
///
/// RED today: `eval_cached` never visits `template.guarded_groups`, so `x` is
/// absent from `values` and from the determinacy snapshot — `probe` (which
/// reads `determined(x)`) hits the "not in determinacy snapshot" debug_assert
/// (reify-expr/src/lib.rs:894) and/or comes back wrong, and no diagnostic
/// names the bypass.
///
/// GREEN after the fix: `eval_cached` falls through to a full cold `eval()`
/// for guarded modules, so `x` == 0.005 (5mm SI), `probe` == Bool(true), and a
/// warning diagnostic naming "guarded groups" and "incremental" is present.
#[test]
fn eval_cached_supports_or_signals_guarded_groups_and_does_not_panic_on_determinacy_probe() {
    let guard_id = ValueCellId::new("S", "__guard_0");
    let x_id = ValueCellId::new("S", "x");
    let probe_id = ValueCellId::new("S", "probe");

    let guard_expr = value_ref_typed("S", "active", Type::Bool);
    let x_decl = make_param_decl("S", "x", Type::length(), Value::length(0.005));
    let probe_expr =
        CompiledExpr::determinacy_predicate(DeterminacyPredicateKind::Determined, x_id.clone());

    let template = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "active",
            Type::Bool,
            Some(CompiledExpr::literal(Value::Bool(true), Type::Bool)),
        )
        .guarded_group(
            guard_expr,
            guard_id.clone(),
            vec![x_decl], // members
            vec![],       // constraints
            vec![],       // else_members
            vec![],       // else_constraints
        )
        .let_binding("S", "probe", Type::Bool, probe_expr)
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = Engine::with_prelude(Box::new(MockConstraintChecker::new()), None, &[]);

    // (a) must not panic — today this can trip the "not in determinacy
    // snapshot" debug_assert (reify-expr/src/lib.rs:894) when debug
    // assertions are enabled for the reify-expr dependency crate.
    let result = engine.eval_cached(&module, VersionId(1));

    // (b) a diagnostic naming the incremental-path bypass must be present.
    assert!(
        result
            .eval_result
            .diagnostics
            .iter()
            .any(|d| {
                d.code == Some(DiagnosticCode::EvalCachedGuardedGroupsFallback)
                    && d.message.contains("guarded groups")
                    && d.message.contains("incremental")
            }),
        "expected a DiagnosticCode::EvalCachedGuardedGroupsFallback warning naming \
         the guarded-groups/incremental fallback. The CODE half is load-bearing: \
         reify-lsp/src/diagnostics.rs::compute_diagnostics_with_state drops this \
         diagnostic from the editor stream by matching that code, so removing the \
         `.with_code(..)` would silently re-open the spurious-editor-warning leak \
         (task 5240). Got: {:?}",
        result.eval_result.diagnostics,
    );

    // (c) the guarded member must no longer be silently dropped.
    assert_eq!(
        result.eval_result.values.get(&x_id),
        Some(&Value::length(0.005)),
        "guarded member x should be 0.005 (5mm SI) via the cold-eval fallback; got {:?}",
        result.eval_result.values.get(&x_id),
    );

    // (d) determined(x) must be true for a defaulted guarded param — proves
    // the determinacy snapshot was populated for the guarded cell (no panic,
    // no silent Undef fallback).
    assert_eq!(
        result.eval_result.values.get(&probe_id),
        Some(&Value::Bool(true)),
        "determined(x) should be true for a defaulted guarded param; got {:?}",
        result.eval_result.values.get(&probe_id),
    );
}

/// Cross-guard-state value-parity lock: `eval_cached`'s guarded-cell values
/// for the guard=false / else-branch-active state must equal a cold
/// `eval()` engine's, and the fallback warning must be present.
///
/// Build (shape mirrors guard_eval.rs's `eval_guard_false_includes_else`):
/// Bool param `active` default FALSE; guarded_group with member param `x`
/// (default 5mm, active when the guard is true) and else_member param `y`
/// (default 10mm, active when the guard is false).
#[test]
fn eval_cached_guarded_module_matches_cold_eval_values_for_guard_false() {
    let guard_id = ValueCellId::new("S", "__guard_0");
    let x_id = ValueCellId::new("S", "x");
    let y_id = ValueCellId::new("S", "y");

    let guard_expr = value_ref_typed("S", "active", Type::Bool);
    let x_decl = make_param_decl("S", "x", Type::length(), Value::length(0.005));
    let y_decl = make_param_decl("S", "y", Type::length(), Value::length(0.01));

    let template = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "active",
            Type::Bool,
            Some(CompiledExpr::literal(Value::Bool(false), Type::Bool)),
        )
        .guarded_group(
            guard_expr,
            guard_id.clone(),
            vec![x_decl], // members (active when guard is true)
            vec![],       // constraints
            vec![y_decl], // else_members (active when guard is false)
            vec![],       // else_constraints
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    // Engine A: eval() path (the reference / known-good side).
    let mut engine_a = Engine::with_prelude(Box::new(MockConstraintChecker::new()), None, &[]);
    let result_a = engine_a.eval(&module);

    // Engine B: eval_cached() path (the side under fix).
    let mut engine_b = Engine::with_prelude(Box::new(MockConstraintChecker::new()), None, &[]);
    let result_b = engine_b.eval_cached(&module, VersionId(1));

    // The fallback warning must be present.
    assert!(
        result_b
            .eval_result
            .diagnostics
            .iter()
            .any(|d| {
                d.code == Some(DiagnosticCode::EvalCachedGuardedGroupsFallback)
                    && d.message.contains("guarded groups")
                    && d.message.contains("incremental")
            }),
        "expected a DiagnosticCode::EvalCachedGuardedGroupsFallback warning naming \
         the guarded-groups/incremental fallback. The CODE half is load-bearing: \
         reify-lsp/src/diagnostics.rs::compute_diagnostics_with_state drops this \
         diagnostic from the editor stream by matching that code, so removing the \
         `.with_code(..)` would silently re-open the spurious-editor-warning leak \
         (task 5240). Got: {:?}",
        result_b.eval_result.diagnostics,
    );

    // Cross-engine value parity for every guarded-specific cell.
    //
    // NOTE: the fix makes `eval_cached` delegate wholesale to `eval()` for
    // any guarded module (see the fall-through at the top of
    // engine_eval.rs's `eval_cached`), so `result_b` and `result_a` below
    // execute the *identical* code path here — this loop cannot diverge
    // while that fall-through is taken. It is a delegation-equivalence
    // check (it would catch a future change that drops or rewrites values
    // on the way out of the `CachedEvalResult` wrapper), not an
    // independent cross-implementation verification. The real guarded-
    // semantics lock is the concrete-value pins on `result_a` below.
    for (label, id) in [
        ("guard cell", &guard_id),
        ("member x", &x_id),
        ("else_member y", &y_id),
    ] {
        assert_eq!(
            result_b.eval_result.values.get(id),
            result_a.values.get(id),
            "{label} ({id:?}): eval_cached value must match cold eval() value",
        );
    }

    // Pin the concrete expected values too (guards against both engines
    // being wrong in the same way).
    assert_eq!(
        result_a.values.get(&guard_id),
        Some(&Value::Bool(false)),
        "guard cell should evaluate to false"
    );
    assert_eq!(
        result_a.values.get(&x_id),
        Some(&Value::Undef),
        "guarded member x should be Undef when guard is false"
    );
    assert_eq!(
        result_a.values.get(&y_id),
        Some(&Value::length(0.01)),
        "else member y should be 0.01 (10mm SI) when guard is false"
    );
}
