//! Integration tests verifying that `Engine::eval_cached` emits the same
//! diagnostic kinds as the cold-start `Engine::eval` path.
//!
//! Task 2259: eval_cached was broken — it declared `let diagnostics = Vec::new()`
//! (immutable) and never appended anything, so cyclic let-bindings, param-override
//! mismatches, unknown sub-component references, and solver Infeasible/NoProgress
//! errors were silently dropped on all repeat calls.
//!
//! Each test here constructs a minimal failing scenario and asserts that the
//! relevant diagnostic substring appears in `result.eval_result.diagnostics`.
//! The tests are grouped with TDD step numbers in comments for traceability.

use reify_eval::Engine;
use reify_test_support::mocks::{MockConstraintChecker, MockConstraintSolver};
use reify_test_support::*;
use reify_types::{
    BinOp, CompiledExpr, Diagnostic, DimensionVector, ModulePath, Type, Value, ValueCellId,
    VersionId,
};

// ── step-1: circular let-binding ────────────────────────────────────────────

/// eval_cached must surface a "circular let-binding dependency" diagnostic when
/// the template contains two let cells whose default_exprs reference each other.
///
/// Scenario: `let a = b + 1.0` and `let b = a + 1.0` form a cycle.
/// Today `eval_cached` returns `diagnostics = Vec::new()` unconditionally,
/// so this test FAILS until step-2 wires the cycle detection.
#[test]
fn eval_cached_emits_circular_let_binding_diagnostic() {
    // `a = b + 1.0`  and  `b = a + 1.0` — mutually recursive
    let expr_a = binop(
        BinOp::Add,
        value_ref_typed("S", "b", Type::Real),
        literal(Value::Real(1.0)),
    );
    let expr_b = binop(
        BinOp::Add,
        value_ref_typed("S", "a", Type::Real),
        literal(Value::Real(1.0)),
    );

    let template = TopologyTemplateBuilder::new("S")
        .let_binding("S", "a", Type::Real, expr_a)
        .let_binding("S", "b", Type::Real, expr_b)
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = Engine::with_prelude(Box::new(MockConstraintChecker::new()), None, &[]);
    let result = engine.eval_cached(&module, VersionId(1));

    assert!(
        result.eval_result.diagnostics.iter().any(|d| {
            d.message.contains("circular let-binding dependency")
                && d.message.contains("in template S")
        }),
        "eval_cached must emit a circular let-binding diagnostic; got: {:?}",
        result.eval_result.diagnostics,
    );
}

// ── step-3: param_override validation ────────────────────────────────────────

/// eval_cached must warn when a param_override's type kind doesn't match the
/// cell type (e.g. pushing a Bool value into a Length cell).
///
/// Fails today because eval_cached's Param branch (engine_eval.rs:1414) uses
/// the override directly without calling `validate_param_override`.
#[test]
fn eval_cached_emits_param_override_type_kind_mismatch_warning() {
    let x_id = ValueCellId::new("S", "x");

    let template = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "x",
            Type::length(),
            Some(CompiledExpr::literal(mm(1.0), Type::length())),
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = Engine::with_prelude(Box::new(MockConstraintChecker::new()), None, &[]);
    // Push a Bool override into a Length param — type-kind mismatch
    engine.set_param_and_invalidate(&x_id, Value::Bool(true));

    let result = engine.eval_cached(&module, VersionId(1));

    assert!(
        result.eval_result.diagnostics.iter().any(|d| {
            d.message.contains("param_override for") && d.message.contains("type-kind mismatch")
        }),
        "eval_cached must warn about type-kind mismatch on param_override; got: {:?}",
        result.eval_result.diagnostics,
    );
}

/// eval_cached must warn when a param_override's scalar dimension doesn't match
/// the cell's declared dimension (e.g. pushing a Mass value into a Length cell).
///
/// Fails today because eval_cached's Param branch never calls `validate_param_override`.
#[test]
fn eval_cached_emits_param_override_dimension_mismatch_warning() {
    let x_id = ValueCellId::new("S", "x");

    let template = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "x",
            Type::length(),
            Some(CompiledExpr::literal(mm(1.0), Type::length())),
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = Engine::with_prelude(Box::new(MockConstraintChecker::new()), None, &[]);
    // Push a Mass-dimensioned scalar into a Length param — dimension mismatch
    engine.set_param_and_invalidate(
        &x_id,
        Value::Scalar {
            si_value: 1.0,
            dimension: DimensionVector::MASS,
        },
    );

    let result = engine.eval_cached(&module, VersionId(1));

    assert!(
        result.eval_result.diagnostics.iter().any(|d| {
            d.message.contains("param_override for") && d.message.contains("dimension mismatch")
        }),
        "eval_cached must warn about dimension mismatch on param_override; got: {:?}",
        result.eval_result.diagnostics,
    );
}

// ── step-5: sub-component unknown-structure ──────────────────────────────────

/// eval_cached must emit "sub-component references unknown structure" when a
/// template has a sub_component whose structure_name doesn't exist in the module.
///
/// Fails today because eval_cached has no sub-component iteration at all.
#[test]
fn eval_cached_emits_sub_component_unknown_structure_diagnostic() {
    let template = TopologyTemplateBuilder::new("Parent")
        .sub_component("rib", "DoesNotExist", vec![])
        .build();

    // Module only contains Parent — no DoesNotExist template
    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = Engine::with_prelude(Box::new(MockConstraintChecker::new()), None, &[]);
    let result = engine.eval_cached(&module, VersionId(1));

    assert!(
        result.eval_result.diagnostics.iter().any(|d| {
            d.message.contains("sub-component")
                && d.message.contains("references unknown structure")
                && d.message.contains("DoesNotExist")
        }),
        "eval_cached must emit unknown-structure diagnostic for missing sub-component; got: {:?}",
        result.eval_result.diagnostics,
    );
}

// ── step-7: solver Infeasible / NoProgress ───────────────────────────────────

/// eval_cached must forward Infeasible diagnostics from the constraint solver.
///
/// Fails today because eval_cached never invokes the solver.
#[test]
fn eval_cached_emits_solver_infeasible_diagnostic() {
    let solver = MockConstraintSolver::new_infeasible(vec![Diagnostic::error(
        "infeasible: x has no satisfying assignment",
    )]);

    let template = TopologyTemplateBuilder::new("S")
        .auto_param("S", "x", Type::length())
        .constraint("S", 0, None, gt(value_ref("S", "x"), literal(mm(1.0))))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = Engine::with_prelude(Box::new(MockConstraintChecker::new()), None, &[])
        .with_solver(Box::new(solver));

    let result = engine.eval_cached(&module, VersionId(1));

    assert!(
        result
            .eval_result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("infeasible: x has no satisfying assignment")),
        "eval_cached must forward Infeasible solver diagnostics; got: {:?}",
        result.eval_result.diagnostics,
    );
}

/// eval_cached must emit a "Constraint solver made no progress" warning when
/// the solver returns NoProgress.
///
/// Fails today because eval_cached never invokes the solver.
#[test]
fn eval_cached_emits_solver_no_progress_warning() {
    let solver = MockConstraintSolver::new_no_progress("iteration limit reached");

    let template = TopologyTemplateBuilder::new("S")
        .auto_param("S", "x", Type::length())
        .constraint("S", 0, None, gt(value_ref("S", "x"), literal(mm(1.0))))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = Engine::with_prelude(Box::new(MockConstraintChecker::new()), None, &[])
        .with_solver(Box::new(solver));

    let result = engine.eval_cached(&module, VersionId(1));

    assert!(
        result.eval_result.diagnostics.iter().any(|d| {
            d.message.contains("Constraint solver made no progress")
                && d.message.contains("iteration limit reached")
        }),
        "eval_cached must emit NoProgress warning from solver; got: {:?}",
        result.eval_result.diagnostics,
    );
}

// ── amendment: solver Solved arm noop pin ────────────────────────────────────

/// Pin the design decision that the Solved arm in eval_cached is intentionally a no-op.
///
/// eval_cached invokes the solver for diagnostic purposes only (Infeasible/NoProgress).
/// The Solved arm deliberately does NOT update `eval_result.values` — that would require
/// ~90 lines of snapshot/cache/journal work from eval() which is out of scope for this task
/// (see plan design decision: "Solver Solved arm in eval_cached is intentionally empty").
///
/// This test ensures a future engineer who wires up the Solved arm gets a failing test,
/// prompting them to verify the downstream consequences before landing the change.
#[test]
fn eval_cached_solver_solved_arm_is_intentional_noop() {
    let x_id = ValueCellId::new("S", "x");

    // Solver returns Solved with a concrete value for x (5 mm in SI)
    let solved_value = Value::Scalar {
        si_value: 0.005,
        dimension: DimensionVector::LENGTH,
    };
    let solved_values: std::collections::HashMap<ValueCellId, Value> =
        [(x_id.clone(), solved_value)].into_iter().collect();
    let solver = MockConstraintSolver::new_solved(solved_values);

    let template = TopologyTemplateBuilder::new("S")
        .auto_param("S", "x", Type::length())
        .constraint("S", 0, None, gt(value_ref("S", "x"), literal(mm(1.0))))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = Engine::with_prelude(Box::new(MockConstraintChecker::new()), None, &[])
        .with_solver(Box::new(solver));

    let result = engine.eval_cached(&module, VersionId(1));

    // Auto cell must remain Undef — the Solved arm is intentionally a no-op.
    // If this assertion fails, someone wired up value updates in the Solved arm without
    // updating this test and reading the plan design decision first.
    assert_eq!(
        result.eval_result.values.get(&x_id),
        Some(&Value::Undef),
        "eval_cached Solved arm must be a no-op: auto cell '{}' must remain Undef, \
         not be updated to the solver-bound value; got: {:?}",
        x_id,
        result.eval_result.values.get(&x_id),
    );
    assert!(
        result.eval_result.diagnostics.is_empty(),
        "Solved result must not produce diagnostics in eval_cached; got: {:?}",
        result.eval_result.diagnostics,
    );
}

// ── amendment: collection sub-component unknown structure ─────────────────────

/// eval_cached must emit "sub-component references unknown structure" for collection
/// sub-components (is_collection=true), not just scalar sub-components.
///
/// The eval_cached sub-component validation pass (added in step-6) iterates
/// template.sub_components unconditionally. This test pins that both collection and
/// non-collection subs are covered — preventing a regression where someone adds an
/// `if !sub.is_collection` guard and silently drops the collection case.
#[test]
fn eval_cached_emits_sub_component_unknown_structure_collection_diagnostic() {
    let count_id = ValueCellId::new("Parent", "count");

    // collection sub-component: `sub ribs : List<DoesNotExist>`
    let template = TopologyTemplateBuilder::new("Parent")
        .param(
            "Parent",
            "count",
            Type::Real,
            Some(literal(Value::Real(3.0))),
        )
        .collection_sub_component("ribs", "DoesNotExist", count_id)
        .build();

    // Module only contains Parent — no DoesNotExist template
    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = Engine::with_prelude(Box::new(MockConstraintChecker::new()), None, &[]);
    let result = engine.eval_cached(&module, VersionId(1));

    assert!(
        result.eval_result.diagnostics.iter().any(|d| {
            d.message.contains("sub-component")
                && d.message.contains("references unknown structure")
                && d.message.contains("DoesNotExist")
        }),
        "eval_cached must emit unknown-structure diagnostic for missing collection \
         sub-component; got: {:?}",
        result.eval_result.diagnostics,
    );
}

// ── task-2267: repeat-call regression lock for param_override diagnostics ────

/// Calling eval_cached multiple times on a param cell with a type-kind-mismatched
/// override must continue to surface the warning on EVERY call — including a
/// same-version fast-path hit and a bumped-version repeat. Also locks the
/// cached-value invariant: the rejected Bool override must NOT corrupt the cached
/// value; the fallback must be the default-expression result (mm(1.0)), not
/// Value::Bool(true).
///
/// Fails today because the Param branch emits the warning only in the cache-miss
/// path; after the first call caches the result, a same-version repeat hits the
/// fast-path and returns the cached fallback without re-running validation. The
/// LSP needs this on every keystroke.
#[test]
fn eval_cached_repeat_call_re_emits_param_override_type_kind_mismatch_warning() {
    let x_id = ValueCellId::new("S", "x");

    let template = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "x",
            Type::length(),
            Some(CompiledExpr::literal(mm(1.0), Type::length())),
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = Engine::with_prelude(Box::new(MockConstraintChecker::new()), None, &[]);
    // Push a Bool override into a Length param — type-kind mismatch
    engine.set_param_and_invalidate(&x_id, Value::Bool(true));

    // First call — cold start (cache-miss path, validation runs, diagnostic surfaces)
    let result1 = engine.eval_cached(&module, VersionId(1));
    assert_eq!(
        result1.eval_result.diagnostics.iter()
            .filter(|d| d.message.contains("param_override for") && d.message.contains("type-kind mismatch"))
            .count(),
        1,
        "first eval_cached call must emit exactly one type-kind mismatch warning; got: {:?}",
        result1.eval_result.diagnostics,
    );
    assert_eq!(
        result1.eval_result.values.get(&x_id),
        Some(&mm(1.0)),
        "rejected Bool override must NOT corrupt cached value; expected fallback mm(1.0); got: {:?}",
        result1.eval_result.values.get(&x_id),
    );

    // Second call — same version (fast-path hit). Must still surface the warning.
    // FAILS today because the fast-path returns the cached fallback without re-running
    // validation — the diagnostic emission is gated on the cache-miss path.
    let result2 = engine.eval_cached(&module, VersionId(1));
    assert_eq!(
        result2.eval_result.diagnostics.iter()
            .filter(|d| d.message.contains("param_override for") && d.message.contains("type-kind mismatch"))
            .count(),
        1,
        "second eval_cached call (same version, fast-path hit) must emit exactly one \
         type-kind mismatch warning (must not drop or duplicate on cache hit); got: {:?}",
        result2.eval_result.diagnostics,
    );
    assert_eq!(
        result2.eval_result.values.get(&x_id),
        Some(&mm(1.0)),
        "rejected Bool override must NOT corrupt cached value; expected fallback mm(1.0); got: {:?}",
        result2.eval_result.values.get(&x_id),
    );

    // Third call — bumped version. Must still surface the warning.
    let result3 = engine.eval_cached(&module, VersionId(2));
    assert_eq!(
        result3.eval_result.diagnostics.iter()
            .filter(|d| d.message.contains("param_override for") && d.message.contains("type-kind mismatch"))
            .count(),
        1,
        "third eval_cached call (bumped version) must emit exactly one type-kind mismatch \
         warning; got: {:?}",
        result3.eval_result.diagnostics,
    );
    assert_eq!(
        result3.eval_result.values.get(&x_id),
        Some(&mm(1.0)),
        "rejected Bool override must NOT corrupt cached value; expected fallback mm(1.0); got: {:?}",
        result3.eval_result.values.get(&x_id),
    );
}

/// Calling eval_cached multiple times on a param cell with a dimension-mismatched
/// override must continue to surface the warning on EVERY call — including a
/// same-version fast-path hit and a bumped-version repeat. Also locks the
/// cached-value invariant: the rejected MASS-dimensioned override must NOT corrupt
/// the cached value; the fallback must be the default-expression result (mm(1.0)),
/// not the rejected MASS scalar.
///
/// Locks the ScalarDimensionMismatch rejection variant independently — preventing
/// a future regression where someone accidentally narrows the unconditional pre-check
/// to only the TypeKindMismatch path. Should pass immediately after step-2 because
/// both rejection variants flow through the same pre-check block.
#[test]
fn eval_cached_repeat_call_re_emits_param_override_dimension_mismatch_warning() {
    let x_id = ValueCellId::new("S", "x");

    let template = TopologyTemplateBuilder::new("S")
        .param(
            "S",
            "x",
            Type::length(),
            Some(CompiledExpr::literal(mm(1.0), Type::length())),
        )
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = Engine::with_prelude(Box::new(MockConstraintChecker::new()), None, &[]);
    // Push a Mass-dimensioned scalar into a Length param — dimension mismatch
    engine.set_param_and_invalidate(
        &x_id,
        Value::Scalar {
            si_value: 1.0,
            dimension: DimensionVector::MASS,
        },
    );

    // First call — cold start (cache-miss path, validation runs, diagnostic surfaces)
    let result1 = engine.eval_cached(&module, VersionId(1));
    assert_eq!(
        result1.eval_result.diagnostics.iter()
            .filter(|d| d.message.contains("param_override for") && d.message.contains("dimension mismatch"))
            .count(),
        1,
        "first eval_cached call must emit exactly one dimension mismatch warning; got: {:?}",
        result1.eval_result.diagnostics,
    );
    assert_eq!(
        result1.eval_result.values.get(&x_id),
        Some(&mm(1.0)),
        "rejected MASS-dimensioned override must NOT corrupt cached value; expected fallback mm(1.0); got: {:?}",
        result1.eval_result.values.get(&x_id),
    );

    // Second call — same version (fast-path hit). Must still surface the warning.
    let result2 = engine.eval_cached(&module, VersionId(1));
    assert_eq!(
        result2.eval_result.diagnostics.iter()
            .filter(|d| d.message.contains("param_override for") && d.message.contains("dimension mismatch"))
            .count(),
        1,
        "second eval_cached call (same version, fast-path hit) must emit exactly one \
         dimension mismatch warning (must not drop or duplicate on cache hit); got: {:?}",
        result2.eval_result.diagnostics,
    );
    assert_eq!(
        result2.eval_result.values.get(&x_id),
        Some(&mm(1.0)),
        "rejected MASS-dimensioned override must NOT corrupt cached value; expected fallback mm(1.0); got: {:?}",
        result2.eval_result.values.get(&x_id),
    );

    // Third call — bumped version. Must still surface the warning.
    let result3 = engine.eval_cached(&module, VersionId(2));
    assert_eq!(
        result3.eval_result.diagnostics.iter()
            .filter(|d| d.message.contains("param_override for") && d.message.contains("dimension mismatch"))
            .count(),
        1,
        "third eval_cached call (bumped version) must emit exactly one dimension mismatch \
         warning; got: {:?}",
        result3.eval_result.diagnostics,
    );
    assert_eq!(
        result3.eval_result.values.get(&x_id),
        Some(&mm(1.0)),
        "rejected MASS-dimensioned override must NOT corrupt cached value; expected fallback mm(1.0); got: {:?}",
        result3.eval_result.values.get(&x_id),
    );
}

// ── step-10: repeat-call regression lock for solver diagnostics ──────────────

/// Calling eval_cached twice on the same (unchanged) module must continue to
/// surface Infeasible diagnostics from the solver on BOTH calls.
///
/// Fails today because the solver pass is gated on `any_auto_miss`: on the second
/// call every auto cell hits the version fast-path or cache-reuse path, so
/// `any_auto_miss` stays false and the solver is silently skipped — the Infeasible
/// diagnostic is dropped. The LSP needs this on every keystroke.
#[test]
fn eval_cached_repeat_call_re_emits_solver_infeasible_diagnostic() {
    let solver = MockConstraintSolver::new_infeasible(vec![Diagnostic::error(
        "infeasible: x has no satisfying assignment",
    )]);

    let template = TopologyTemplateBuilder::new("S")
        .auto_param("S", "x", Type::length())
        .constraint("S", 0, None, gt(value_ref("S", "x"), literal(mm(1.0))))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = Engine::with_prelude(Box::new(MockConstraintChecker::new()), None, &[])
        .with_solver(Box::new(solver));

    // First call — cold start (any_auto_miss = true, solver runs, diagnostic surfaces)
    let result1 = engine.eval_cached(&module, VersionId(1));
    assert!(
        result1
            .eval_result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("infeasible: x has no satisfying assignment")),
        "first eval_cached call must forward Infeasible solver diagnostic; got: {:?}",
        result1.eval_result.diagnostics,
    );

    // Second call — same module, bumped version (models repeat keystroke in LSP).
    // Auto cell hits the cache, any_auto_miss=false — solver must still run.
    let result2 = engine.eval_cached(&module, VersionId(2));
    assert!(
        result2
            .eval_result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("infeasible: x has no satisfying assignment")),
        "second eval_cached call must also forward Infeasible solver diagnostic \
         (must not drop on cache hit); got: {:?}",
        result2.eval_result.diagnostics,
    );
}

/// Calling eval_cached twice on the same (unchanged) module must continue to
/// surface NoProgress warnings from the solver on BOTH calls.
///
/// Fails today because the solver pass is gated on `any_auto_miss`: on the second
/// call every auto cell hits the cache, so `any_auto_miss` stays false and the
/// solver is silently skipped — the NoProgress warning is dropped.
#[test]
fn eval_cached_repeat_call_re_emits_solver_no_progress_warning() {
    let solver = MockConstraintSolver::new_no_progress("iteration limit reached");

    let template = TopologyTemplateBuilder::new("S")
        .auto_param("S", "x", Type::length())
        .constraint("S", 0, None, gt(value_ref("S", "x"), literal(mm(1.0))))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = Engine::with_prelude(Box::new(MockConstraintChecker::new()), None, &[])
        .with_solver(Box::new(solver));

    // First call — cold start
    let result1 = engine.eval_cached(&module, VersionId(1));
    assert!(
        result1.eval_result.diagnostics.iter().any(|d| {
            d.message.contains("Constraint solver made no progress")
                && d.message.contains("iteration limit reached")
        }),
        "first eval_cached call must emit NoProgress warning; got: {:?}",
        result1.eval_result.diagnostics,
    );

    // Second call — same module, bumped version (models repeat keystroke in LSP).
    // Auto cell hits the cache — solver must still run.
    let result2 = engine.eval_cached(&module, VersionId(2));
    assert!(
        result2.eval_result.diagnostics.iter().any(|d| {
            d.message.contains("Constraint solver made no progress")
                && d.message.contains("iteration limit reached")
        }),
        "second eval_cached call must also emit NoProgress warning \
         (must not drop on cache hit); got: {:?}",
        result2.eval_result.diagnostics,
    );
}

// ── step-9: repeat-call regression lock ──────────────────────────────────────

/// Calling eval_cached twice on the same (unchanged) module must continue to
/// surface the circular let-binding diagnostic on BOTH calls.
///
/// This locks the LSP scenario: when the user keeps typing in a broken-source
/// file, every keypress goes through eval_cached (content_unchanged=true) and
/// the error must appear each time — not only on the cold-start call.
///
/// Should pass immediately after step-2 because cycle detection runs
/// unconditionally on every eval_cached invocation (not gated by cache state).
#[test]
fn eval_cached_repeat_call_with_unchanged_module_re_emits_circular_diagnostic() {
    let expr_a = binop(
        BinOp::Add,
        value_ref_typed("S", "b", Type::Real),
        literal(Value::Real(1.0)),
    );
    let expr_b = binop(
        BinOp::Add,
        value_ref_typed("S", "a", Type::Real),
        literal(Value::Real(1.0)),
    );

    let template = TopologyTemplateBuilder::new("S")
        .let_binding("S", "a", Type::Real, expr_a)
        .let_binding("S", "b", Type::Real, expr_b)
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = Engine::with_prelude(Box::new(MockConstraintChecker::new()), None, &[]);

    // First call — cold start
    let result1 = engine.eval_cached(&module, VersionId(1));
    assert!(
        result1.eval_result.diagnostics.iter().any(|d| {
            d.message.contains("circular let-binding dependency")
                && d.message.contains("in template S")
        }),
        "first eval_cached call must emit circular diagnostic; got: {:?}",
        result1.eval_result.diagnostics,
    );

    // Second call — same module content, bumped version (models "user kept typing
    // unchanged source" in the LSP eval_cached path)
    let result2 = engine.eval_cached(&module, VersionId(2));
    assert!(
        result2.eval_result.diagnostics.iter().any(|d| {
            d.message.contains("circular let-binding dependency")
                && d.message.contains("in template S")
        }),
        "second eval_cached call must also emit circular diagnostic (must not drop on cache hit); \
         got: {:?}",
        result2.eval_result.diagnostics,
    );
}

// ── task-2269: sub-component repeat-call regression lock ──────────────────────

/// Calling eval_cached twice on a module whose template has a sub_component
/// referencing a non-existent structure must continue to surface the
/// "references unknown structure" diagnostic on BOTH calls.
///
/// This locks the LSP requirement that sub-component validation surfaces on every
/// keystroke. Without this lock, a future change that gates the validation pass at
/// `engine_eval.rs:1696-1703` on `any_auto_miss` or similar cache-coherence flag
/// would silently drop the diagnostic on the second call (bumped version / cache-hit
/// path), breaking the LSP's real-time error reporting.
///
/// Should pass immediately because the sub-component validation pass already
/// iterates `template.sub_components` unconditionally inside the per-template loop.
#[test]
fn eval_cached_repeat_call_re_emits_sub_component_unknown_structure_diagnostic() {
    let template = TopologyTemplateBuilder::new("Parent")
        .sub_component("rib", "DoesNotExist", vec![])
        .build();

    // Module only contains Parent — no DoesNotExist template
    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = Engine::with_prelude(Box::new(MockConstraintChecker::new()), None, &[]);

    // First call — cold start
    let result1 = engine.eval_cached(&module, VersionId(1));
    assert!(
        result1.eval_result.diagnostics.iter().any(|d| {
            d.message.contains("sub-component")
                && d.message.contains("references unknown structure")
                && d.message.contains("DoesNotExist")
        }),
        "first eval_cached call must emit unknown-structure diagnostic; got: {:?}",
        result1.eval_result.diagnostics,
    );

    // Second call — bumped version (models LSP keystroke: each keypress increments
    // the version). The sub-component validation pass must run unconditionally and
    // must NOT be gated on cache state or any_auto_miss.
    let result2 = engine.eval_cached(&module, VersionId(2));
    assert!(
        result2.eval_result.diagnostics.iter().any(|d| {
            d.message.contains("sub-component")
                && d.message.contains("references unknown structure")
                && d.message.contains("DoesNotExist")
        }),
        "second eval_cached call must also emit unknown-structure diagnostic \
         (must not drop on cache hit); got: {:?}",
        result2.eval_result.diagnostics,
    );
}
