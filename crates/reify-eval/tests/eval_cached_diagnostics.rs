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
