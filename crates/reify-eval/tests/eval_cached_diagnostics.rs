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

use reify_eval::cache::{CachedResult, NodeId};
use reify_eval::{CachedEvalResult, Engine};
use reify_test_support::mocks::{MockConstraintChecker, MockConstraintSolver};
use reify_test_support::*;
use reify_core::{Diagnostic, DimensionVector, ModulePath, Severity, Type, ValueCellId, VersionId};
use reify_ir::{BinOp, CompiledExpr, DeterminacyState, Value};

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
        result.eval_result.diagnostics.iter().any(|d| d
            .message
            .contains("infeasible: x has no satisfying assignment")),
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
    let matches = |d: &Diagnostic| {
        d.message.contains("param_override for") && d.message.contains("type-kind mismatch")
    };

    // First call — cold start (cache-miss path, validation runs, diagnostic surfaces)
    let result1 = engine.eval_cached(&module, VersionId(1));
    assert_eq!(
        result1
            .eval_result
            .diagnostics
            .iter()
            .filter(|d| matches(d))
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
        result2
            .eval_result
            .diagnostics
            .iter()
            .filter(|d| matches(d))
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
        result3
            .eval_result
            .diagnostics
            .iter()
            .filter(|d| matches(d))
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
    let matches = |d: &Diagnostic| {
        d.message.contains("param_override for") && d.message.contains("dimension mismatch")
    };

    // First call — cold start (cache-miss path, validation runs, diagnostic surfaces)
    let result1 = engine.eval_cached(&module, VersionId(1));
    assert_eq!(
        result1
            .eval_result
            .diagnostics
            .iter()
            .filter(|d| matches(d))
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
        result2
            .eval_result
            .diagnostics
            .iter()
            .filter(|d| matches(d))
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
        result3
            .eval_result
            .diagnostics
            .iter()
            .filter(|d| matches(d))
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

// ── task-2273: regression-lock for valid-override repeat-call ────────────────

/// A VALID (dimensionally-correct) param override must:
/// (a) be applied on the first call (cache-miss path, `Some(Ok(()))` arm writes it to the cache),
/// (b) still be returned on a same-version repeat call (cached override value returned),
/// (c) produce no `param_override`-keyed diagnostic on either call.
///
/// This locks the cache-miss-write + same-version fast-path-read contract for a VALID override.
/// That contract is independently valuable: the existing rejection-warning regression tests only
/// exercise the `Err` arm and the invalid-override fallback path, so a regression in the
/// `Ok(())` cache-write or the fast-path cache-return for valid overrides would go undetected
/// without this test.
#[test]
fn eval_cached_applies_valid_param_override_on_repeat_call() {
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
    // Push a valid Length override: mm(12.0) = 0.012 m (SI), matching the Length cell type.
    engine.set_param_and_invalidate(&x_id, mm(12.0));

    // First call — cold start (cache-miss path).  The `Some(Ok(()))` arm must clone the override
    // into the cache and return it as the resolved value.
    let result1 = engine.eval_cached(&module, VersionId(1));
    assert!(
        result1.stats.cache_misses >= 1,
        "first eval_cached call must have at least one cache miss; stats: {:?}",
        result1.stats,
    );
    assert_eq!(
        result1.eval_result.values.get(&x_id),
        Some(&mm(12.0)),
        "first call: valid override mm(12.0) must be applied via cache-miss Ok(()) arm; got: {:?}",
        result1.eval_result.values.get(&x_id),
    );
    assert!(
        result1.eval_result.diagnostics.is_empty(),
        "first call: valid override must produce no diagnostics; got: {:?}",
        result1.eval_result.diagnostics,
    );

    // Second call — same version (fast-path hit).  The cached override value must be returned.
    // This assertion locks the fast-path-read half of the contract: a same-version repeat call
    // must return the cached override mm(12.0), not regress to the cell-default mm(1.0).
    let result2 = engine.eval_cached(&module, VersionId(1));
    assert!(
        result2.stats.cache_hits >= 1,
        "second eval_cached call (same version) must have at least one cache hit; stats: {:?}",
        result2.stats,
    );
    assert_eq!(
        result2.eval_result.values.get(&x_id),
        Some(&mm(12.0)),
        "second call (fast-path hit): cached override mm(12.0) must still be returned; got: {:?}",
        result2.eval_result.values.get(&x_id),
    );
    assert!(
        result2.eval_result.diagnostics.is_empty(),
        "second call: valid override must produce no diagnostics; got: {:?}",
        result2.eval_result.diagnostics,
    );
}

// ── task-2421: regression-lock for default_expr-fallback dedupe ────────────────────────────

/// `eval_cached` with a REJECTED override on a NO-DEFAULT param must cache
/// `(Value::Undef, DeterminacyState::Undetermined)`.
///
/// Without this test, a refactor that swapped `Undetermined` and `Determined`
/// between the two no-default branches (the `Err` arm vs the `None` arm of the
/// `match override_entry` in `eval_cached`) would pass the entire existing
/// eval_cached_diagnostics suite undetected — the four task-2267/2273 regression
/// tests all use a Param WITH a `default_expr`, so they exercise the `Determined`
/// branch of the fallback block in both arms.  This test pins the `Undetermined`
/// state that is unique to the `Err` arm when there is no default expression.
///
/// Characterization test: verifies the contract of the current implementation
/// BEFORE the refactor that extracts a `default_or` helper closure (task-2421
/// step-3).  Must pass against unrefactored code.
#[test]
fn eval_cached_rejected_param_override_no_default_caches_undef_undetermined() {
    let x_id = ValueCellId::new("S", "x");

    // No default expression — the `else` branch of the fallback block is taken.
    let template = TopologyTemplateBuilder::new("S")
        .param("S", "x", Type::length(), None)
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = Engine::with_prelude(Box::new(MockConstraintChecker::new()), None, &[]);
    // Push a Bool override into a Length param — triggers Err(TypeKindMismatch).
    engine.set_param_and_invalidate(&x_id, Value::Bool(true));

    let result = engine.eval_cached(&module, VersionId(1));

    // The rejected-override-no-default arm must write Value::Undef into the values map.
    assert_eq!(
        result.eval_result.values.get(&x_id),
        Some(&Value::Undef),
        "rejected Bool override on no-default param must produce Value::Undef in values map; \
         got: {:?}",
        result.eval_result.values.get(&x_id),
    );

    // The cache entry must record DeterminacyState::Undetermined — the unique discriminator
    // for this arm.  A swap with the None-arm's Determined would silently corrupt the LSP
    // determinacy signal but leave the values-map assertion above green.
    let node_id = NodeId::Value(x_id.clone());
    let entry = engine
        .cache_store()
        .get(&node_id)
        .expect("eval_cached must record a cache entry for the param node");
    assert!(
        matches!(
            &entry.result,
            CachedResult::Value(Value::Undef, DeterminacyState::Undetermined)
        ),
        "cache entry for rejected-override-no-default param must be \
         CachedResult::Value(Undef, Undetermined); got: {:?}",
        entry.result,
    );

    // The unconditional pre-check must surface the type-kind-mismatch warning.
    let matches_warn = |d: &Diagnostic| {
        d.message.contains("param_override for") && d.message.contains("type-kind mismatch")
    };
    assert!(
        result.eval_result.diagnostics.iter().any(matches_warn),
        "eval_cached must emit a type-kind mismatch warning for a rejected Bool override; \
         got: {:?}",
        result.eval_result.diagnostics,
    );
}

/// `eval_cached` with NO override on a NO-DEFAULT param must cache
/// `(Value::Undef, DeterminacyState::Determined)`.
///
/// This is the orthogonal sibling of the test above: together the two tests pin
/// BOTH no-default branches of the `match override_entry` block that the
/// `default_or` helper closure (task-2421 step-3) will parameterise over.
///
/// The key invariant is the `Determined` state — it reflects that the evaluator
/// ran to completion and no inconsistency was detected (there is simply nothing to
/// resolve for a param with no override and no default).  Note this is intentionally
/// asymmetric with the non-cached `eval()` path, which uses `continue` and omits
/// the param from `values` altogether; `eval_cached` always writes a result.
///
/// Characterization test: must pass against unrefactored code.
#[test]
fn eval_cached_no_override_no_default_param_caches_undef_determined() {
    let x_id = ValueCellId::new("S", "x");

    // No default expression, no override pushed.
    let template = TopologyTemplateBuilder::new("S")
        .param("S", "x", Type::length(), None)
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = Engine::with_prelude(Box::new(MockConstraintChecker::new()), None, &[]);
    // Deliberately no call to set_param_and_invalidate — None arm is exercised.

    let result = engine.eval_cached(&module, VersionId(1));

    // The None-arm-no-default path must write Value::Undef into the values map.
    assert_eq!(
        result.eval_result.values.get(&x_id),
        Some(&Value::Undef),
        "no-override-no-default param must produce Value::Undef in values map; \
         got: {:?}",
        result.eval_result.values.get(&x_id),
    );

    // The cache entry must record DeterminacyState::Determined — the unique discriminator
    // for the None arm (contrasts with Undetermined in the Err arm above).
    let node_id = NodeId::Value(x_id.clone());
    let entry = engine
        .cache_store()
        .get(&node_id)
        .expect("eval_cached must record a cache entry for the param node");
    assert!(
        matches!(
            &entry.result,
            CachedResult::Value(Value::Undef, DeterminacyState::Determined)
        ),
        "cache entry for no-override-no-default param must be \
         CachedResult::Value(Undef, Determined); got: {:?}",
        entry.result,
    );

    // No param-override warning must be emitted (there is no override to reject).
    assert!(
        result.eval_result.diagnostics.is_empty(),
        "no-override-no-default param must produce no diagnostics; \
         got: {:?}",
        result.eval_result.diagnostics,
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
        result1.eval_result.diagnostics.iter().any(|d| d
            .message
            .contains("infeasible: x has no satisfying assignment")),
        "first eval_cached call must forward Infeasible solver diagnostic; got: {:?}",
        result1.eval_result.diagnostics,
    );

    // Second call — same module, bumped version (models repeat keystroke in LSP).
    // Auto cell hits the cache, any_auto_miss=false — solver must still run.
    let result2 = engine.eval_cached(&module, VersionId(2));
    assert!(
        result2.eval_result.diagnostics.iter().any(|d| d
            .message
            .contains("infeasible: x has no satisfying assignment")),
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

/// Calling eval_cached three times on a module whose template has a sub_component
/// referencing a non-existent structure must continue to surface the
/// "references unknown structure" diagnostic on ALL calls: cold start, same-version
/// fast-path hit, and bumped-version repeat (1, 1, 2 cadence mirrors the
/// param_override repeat-call tests).
///
/// This locks the LSP requirement that sub-component validation surfaces on every
/// keystroke. Without this lock, a future change that gates the validation pass at
/// `engine_eval.rs:1696-1703` on `any_auto_miss` or similar cache-coherence flag
/// would silently drop the diagnostic on the second call (same-version fast-path hit),
/// breaking the LSP's real-time error reporting. The same-version call is the
/// canonical scenario that an `any_auto_miss`-style gate would break.
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
    let matches = |d: &Diagnostic| {
        d.message.contains("sub-component")
            && d.message.contains("references unknown structure")
            && d.message.contains("DoesNotExist")
    };

    // First call — cold start (cache-miss path, validation runs, diagnostic surfaces)
    let result1 = engine.eval_cached(&module, VersionId(1));
    assert_eq!(
        result1
            .eval_result
            .diagnostics
            .iter()
            .filter(|d| matches(d))
            .count(),
        1,
        "first eval_cached call must emit exactly one unknown-structure diagnostic; got: {:?}",
        result1.eval_result.diagnostics,
    );

    // Second call — same version (fast-path hit). Must still surface the diagnostic.
    // This is the canonical scenario that an any_auto_miss-style gate would break:
    // on a same-version repeat every cell hits the fast-path, so any_auto_miss=false.
    let result2 = engine.eval_cached(&module, VersionId(1));
    assert_eq!(
        result2
            .eval_result
            .diagnostics
            .iter()
            .filter(|d| matches(d))
            .count(),
        1,
        "second eval_cached call (same version, fast-path hit) must emit exactly one \
         unknown-structure diagnostic (must not drop or duplicate on cache hit); got: {:?}",
        result2.eval_result.diagnostics,
    );

    // Third call — bumped version (models LSP keystroke: each keypress increments
    // the version). The sub-component validation pass must run unconditionally and
    // must NOT be gated on cache state or any_auto_miss.
    let result3 = engine.eval_cached(&module, VersionId(2));
    assert_eq!(
        result3
            .eval_result
            .diagnostics
            .iter()
            .filter(|d| matches(d))
            .count(),
        1,
        "third eval_cached call (bumped version) must emit exactly one unknown-structure \
         diagnostic (must not drop or duplicate on cache hit); got: {:?}",
        result3.eval_result.diagnostics,
    );
}

// ── task-2266: cyclic let-cells must not be cached or appear in values ────────

/// Cyclic let cells must not be written to the cache or to `eval_result.values`
/// by `eval_cached`, mirroring `eval()`'s behavior in `evaluate_let_bindings()`.
///
/// Background: before the fix (task 2266, commit 49146ec0ae) `eval_cached` appended
/// cyclic cells to `ordered_let_cells` and iterated them in the second pass, calling
/// `cache.record_evaluation` and `values.insert` with forward-reference lookups that
/// produce Undef-derived garbage.  This persisted garbage entries in the cache and
/// diverged `eval_cached`'s value-map shape from `eval()`'s — and would corrupt the
/// cache fast-path on subsequent calls.
///
/// The test asserts the following for BOTH a first call (`VersionId(1)`) AND a second
/// call at a bumped version (`VersionId(2)`) on the same `eval_cached` engine:
///
/// - Cache parity: `cache_store().get(&node_id).is_none()` for BOTH the `eval()` engine
///   and the `eval_cached()` engine after each call.  Asserting both sides (and both
///   calls) pins the symmetric invariant and the persistence invariant.
/// - Value-map parity: `.values.get(&cell_id).is_none()` for BOTH result maps.
/// - The circular diagnostic is still emitted on the eval_cached side (defensive).
///
/// The test FAILED before the fix on the eval_cached side because `cache.get(&node_id)`
/// returned `Some` (garbage was cached) and `values.get(&cell_id)` returned
/// `Some(Value::Undef)` (the garbage value was inserted into the map).
#[test]
fn eval_cached_does_not_cache_cyclic_let_cells() {
    // `a = b + 1.0`  and  `b = a + 1.0` — mutually recursive (same shape as
    // eval_cached_emits_circular_let_binding_diagnostic)
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

    let cell_a = ValueCellId::new("S", "a");
    let cell_b = ValueCellId::new("S", "b");
    let node_a = NodeId::Value(cell_a.clone());
    let node_b = NodeId::Value(cell_b.clone());

    // Engine A: eval() path (the reference / known-good side)
    let mut engine_a = Engine::with_prelude(Box::new(MockConstraintChecker::new()), None, &[]);
    let result_a = engine_a.eval(&module);

    // Engine B: eval_cached() path (the side under fix)
    let mut engine_b = Engine::with_prelude(Box::new(MockConstraintChecker::new()), None, &[]);

    // Helper: asserts the three engine_b-side invariants (cache parity, value-map parity,
    // diagnostic count) for a single eval_cached call.  `eng` is passed explicitly rather
    // than captured so it doesn't conflict with the mutable borrows inside the two
    // eval_cached calls below.  `label` is used as a prefix in every failure message so
    // the caller (first vs. second) is unambiguous in test output.
    let assert_b_invariants = |label: &str, eng: &Engine, result: &CachedEvalResult| {
        assert!(
            eng.cache_store().get(&node_a).is_none(),
            "{label}: engine must NOT have a cache entry for cyclic cell 'a'; got: {:?}",
            eng.cache_store().get(&node_a),
        );
        assert!(
            eng.cache_store().get(&node_b).is_none(),
            "{label}: engine must NOT have a cache entry for cyclic cell 'b'; got: {:?}",
            eng.cache_store().get(&node_b),
        );
        assert!(
            result.eval_result.values.get(&cell_a).is_none(),
            "{label}: result must NOT contain a value for cyclic cell 'a'; got: {:?}",
            result.eval_result.values.get(&cell_a),
        );
        assert!(
            result.eval_result.values.get(&cell_b).is_none(),
            "{label}: result must NOT contain a value for cyclic cell 'b'; got: {:?}",
            result.eval_result.values.get(&cell_b),
        );
        // Assert structural shape only: exactly one error-severity diagnostic.
        // Wording is already pinned by `eval_cached_emits_circular_let_binding_diagnostic`.
        let error_count: usize = result
            .eval_result
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count();
        assert_eq!(
            error_count, 1,
            "{label}: must emit exactly one error-severity diagnostic for a cyclic-let module; got: {:?}",
            result.eval_result.diagnostics,
        );
    };

    let result_b = engine_b.eval_cached(&module, VersionId(1));

    // ── cache parity (eval() reference side) ─────────────────────────────────
    assert!(
        engine_a.cache_store().get(&node_a).is_none(),
        "eval() engine must NOT have a cache entry for cyclic cell 'a'; got: {:?}",
        engine_a.cache_store().get(&node_a),
    );
    assert!(
        engine_a.cache_store().get(&node_b).is_none(),
        "eval() engine must NOT have a cache entry for cyclic cell 'b'; got: {:?}",
        engine_a.cache_store().get(&node_b),
    );

    // ── value-map parity (eval() reference side) ──────────────────────────────
    assert!(
        result_a.values.get(&cell_a).is_none(),
        "eval() result must NOT contain a value for cyclic cell 'a'; got: {:?}",
        result_a.values.get(&cell_a),
    );
    assert!(
        result_a.values.get(&cell_b).is_none(),
        "eval() result must NOT contain a value for cyclic cell 'b'; got: {:?}",
        result_a.values.get(&cell_b),
    );

    // ── eval_cached() first call invariants ───────────────────────────────────
    assert_b_invariants(
        "first eval_cached() call (VersionId(1))",
        &engine_b,
        &result_b,
    );

    // ── second eval_cached call at bumped version: persistence invariant ──────
    // Models the LSP-keystroke pattern (VersionId increments per edit).  The fix
    // must hold across multiple calls; a regression that re-introduces caching only
    // on the second pass (e.g. via the cache fast-path) would be caught here.
    let result_b2 = engine_b.eval_cached(&module, VersionId(2));
    assert_b_invariants(
        "second eval_cached() call (VersionId(2))",
        &engine_b,
        &result_b2,
    );
}

// ── task-2268: wording-parity locks ────────────────────────────────────────────

/// Shared assertion helper for the eval-vs-eval_cached wording-parity tests below.
///
/// Asserts that exactly one diagnostic matching `predicate` is emitted by each
/// path, and that the two messages are byte-identical.  Now that both paths route
/// through a shared helper, divergence is mechanically impossible unless the helper
/// itself is modified — at which point the count assertions still catch off-by-one
/// regressions.
fn assert_diag_parity<F>(
    eval_diags: &[Diagnostic],
    cached_diags: &[Diagnostic],
    predicate: F,
    label: &str,
) where
    F: Fn(&Diagnostic) -> bool,
{
    let msgs_eval: Vec<&str> = eval_diags
        .iter()
        .filter(|d| predicate(d))
        .map(|d| d.message.as_str())
        .collect();
    let msgs_cached: Vec<&str> = cached_diags
        .iter()
        .filter(|d| predicate(d))
        .map(|d| d.message.as_str())
        .collect();

    assert_eq!(
        msgs_eval.len(),
        1,
        "eval() must emit exactly one {label} diagnostic; got: {eval_diags:?}",
    );
    assert_eq!(
        msgs_cached.len(),
        1,
        "eval_cached() must emit exactly one {label} diagnostic; got: {cached_diags:?}",
    );
    assert_eq!(
        msgs_eval[0], msgs_cached[0],
        "{label} must be byte-identical between eval() and eval_cached();\n\
         eval():        {:?}\n\
         eval_cached(): {:?}",
        msgs_eval[0], msgs_cached[0],
    );
}

/// Pin the wording-parity contract for param_override rejection warnings.
///
/// Asserts that `eval()` and `eval_cached()` emit **byte-identical** warning
/// messages for both `TypeKindMismatch` (Bool into Length) and
/// `ScalarDimensionMismatch` (Mass scalar into Length).
///
/// These tests pass today (wording is already byte-identical at every call site)
/// and will continue to pass after the step-2 refactor extracts the shared
/// `emit_param_override_rejection_warning` helper.  They become regression guards:
/// if a future fix changes the warning text at ONE call site without updating the
/// shared helper, these assertions fire — catching the exact "future fix to one
/// site silently fails to propagate" failure mode this task is designed to prevent.
#[test]
fn eval_and_eval_cached_emit_byte_identical_param_override_rejection_warnings() {
    // ── TypeKindMismatch: Bool override into a Length param ───────────────────
    {
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

        let mut engine_a = Engine::with_prelude(Box::new(MockConstraintChecker::new()), None, &[]);
        engine_a.set_param_and_invalidate(&x_id, Value::Bool(true));
        let result_a = engine_a.eval(&module);

        let mut engine_b = Engine::with_prelude(Box::new(MockConstraintChecker::new()), None, &[]);
        engine_b.set_param_and_invalidate(&x_id, Value::Bool(true));
        let result_b = engine_b.eval_cached(&module, VersionId(1));

        assert_diag_parity(
            &result_a.diagnostics,
            &result_b.eval_result.diagnostics,
            |d| {
                d.message.contains("param_override for") && d.message.contains("type-kind mismatch")
            },
            "TypeKindMismatch param_override rejection warning",
        );
    }

    // ── ScalarDimensionMismatch: Mass-dimensioned override into a Length param ─
    {
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

        let mass_val = Value::Scalar {
            si_value: 1.0,
            dimension: DimensionVector::MASS,
        };

        let mut engine_a = Engine::with_prelude(Box::new(MockConstraintChecker::new()), None, &[]);
        engine_a.set_param_and_invalidate(&x_id, mass_val.clone());
        let result_a = engine_a.eval(&module);

        let mut engine_b = Engine::with_prelude(Box::new(MockConstraintChecker::new()), None, &[]);
        engine_b.set_param_and_invalidate(&x_id, mass_val);
        let result_b = engine_b.eval_cached(&module, VersionId(1));

        assert_diag_parity(
            &result_a.diagnostics,
            &result_b.eval_result.diagnostics,
            |d| {
                d.message.contains("param_override for") && d.message.contains("dimension mismatch")
            },
            "ScalarDimensionMismatch param_override rejection warning",
        );
    }
}

/// Pin the wording-parity contract for circular let-binding diagnostics.
///
/// Asserts that `eval()` and `eval_cached()` emit **byte-identical** error
/// messages for a template with two mutually-recursive let cells (`let a = b + 1.0`,
/// `let b = a + 1.0`).
///
/// Passes today (wording is already identical at both call sites) and locks the
/// contract: a future "improvement" of the error message at one site that
/// forgets the other will cause this test to fail.
#[test]
fn eval_and_eval_cached_emit_byte_identical_circular_let_diagnostic() {
    // Same cyclic fixture as eval_cached_emits_circular_let_binding_diagnostic
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

    let mut engine_a = Engine::with_prelude(Box::new(MockConstraintChecker::new()), None, &[]);
    let result_a = engine_a.eval(&module);

    let mut engine_b = Engine::with_prelude(Box::new(MockConstraintChecker::new()), None, &[]);
    let result_b = engine_b.eval_cached(&module, VersionId(1));

    assert_diag_parity(
        &result_a.diagnostics,
        &result_b.eval_result.diagnostics,
        |d| d.message.contains("circular let-binding dependency"),
        "circular let-binding diagnostic",
    );
}

/// Pin the wording-parity contract for solver `NoProgress` warnings.
///
/// Asserts that `eval()` and `eval_cached()` emit **byte-identical** warning
/// messages when the constraint solver returns `SolveResult::NoProgress`.
///
/// Passes today (both paths format `"Constraint solver made no progress: {}"`
/// identically) and locks the contract: a future fix at one site that forgets
/// the other will cause this test to fail.
///
/// The `Infeasible` variant is not separately locked here: both paths forward
/// solver-supplied diagnostics verbatim via `diagnostics.extend(solver_diags)`,
/// so wording parity for `Infeasible` reduces to whatever `MockConstraintSolver`
/// was constructed with — there is no format string to keep in sync.
#[test]
fn eval_and_eval_cached_emit_byte_identical_solver_no_progress_warning() {
    let solver_a = MockConstraintSolver::new_no_progress("iteration limit reached");
    let solver_b = MockConstraintSolver::new_no_progress("iteration limit reached");

    let template = TopologyTemplateBuilder::new("S")
        .auto_param("S", "x", Type::length())
        .constraint("S", 0, None, gt(value_ref("S", "x"), literal(mm(1.0))))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine_a = Engine::with_prelude(Box::new(MockConstraintChecker::new()), None, &[])
        .with_solver(Box::new(solver_a));
    let result_a = engine_a.eval(&module);

    let mut engine_b = Engine::with_prelude(Box::new(MockConstraintChecker::new()), None, &[])
        .with_solver(Box::new(solver_b));
    let result_b = engine_b.eval_cached(&module, VersionId(1));

    assert_diag_parity(
        &result_a.diagnostics,
        &result_b.eval_result.diagnostics,
        |d| d.message.contains("Constraint solver made no progress"),
        "solver NoProgress warning",
    );
}

// ── task-2291: cache-miss let-cell path regression lock ──────────────────────

/// Regression lock for the cache-miss arm of `eval_cached`'s let-cell loop
/// (engine_eval.rs line that performs `let_traces.get(&node_id).cloned()`).
///
/// Scenario: a single non-cyclic let cell `y = a + 10` whose `default_expr`
/// reads param `a` (Int, default 5).  On the *first* call to `eval_cached` the
/// cache is empty, so every let cell goes through the cache-miss arm — the
/// exact arm whose trace-extraction line is tightened in task-2291.
///
/// Assertions (four pins):
/// 1. Value correctness: `y` evaluates to 15 (5 + 10), not Undef.
/// 2. Cache presence: `eval_cached` records a cache entry for `y`.
/// 3. Trace correctness: the entry's `dependency_trace.reads` contains param `a`.
///    This is the meaningful behavioral lock — it would catch any regression that
///    drops or misidentifies the trace at the modified line.
/// 4. No error-severity diagnostics (the fixture is valid; no cycles, no type
///    mismatches).
///
/// Inverts the polarity of `eval_cached_does_not_cache_cyclic_let_cells`:
/// non-cyclic let cells MUST appear in the cache; cyclic ones must not.
#[test]
fn eval_cached_caches_non_cyclic_let_cell_with_param_dependency_trace() {
    let a_id = ValueCellId::new("S", "a");
    let y_id = ValueCellId::new("S", "y");

    // y = a + 10  (single non-cyclic let cell; depends on param 'a')
    let y_expr = binop(BinOp::Add, value_ref("S", "a"), literal(Value::Int(10)));

    let template = TopologyTemplateBuilder::new("S")
        .param("S", "a", Type::Int, Some(literal(Value::Int(5))))
        .let_binding("S", "y", Type::Int, y_expr)
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let mut engine = Engine::with_prelude(Box::new(MockConstraintChecker::new()), None, &[]);
    let result = engine.eval_cached(&module, VersionId(1));

    let y_node = NodeId::Value(y_id.clone());

    // 1. Value correctness: y = a + 10 = 5 + 10 = 15.
    assert_eq!(
        result.eval_result.values.get(&y_id),
        Some(&Value::Int(15)),
        "eval_cached must compute y = a + 10 = 5 + 10 = 15 through the cache-miss path; got: {:?}",
        result.eval_result.values.get(&y_id),
    );

    // 2. Cache presence: a non-cyclic let cell must be cached.
    assert!(
        engine.cache_store().get(&y_node).is_some(),
        "eval_cached must record a cache entry for non-cyclic let cell 'y'; none found",
    );

    // 3. Trace correctness: the cache entry's dependency_trace.reads must contain
    //    the param 'a'.  This pins the trace-extraction path at the modified line.
    let cache_entry = engine
        .cache_store()
        .get(&y_node)
        .expect("cache entry must exist (asserted above)");
    assert!(
        cache_entry.dependency_trace.reads.contains(&a_id),
        "cache entry for 'y' must list param 'a' in dependency_trace.reads; got: {:?}",
        cache_entry.dependency_trace.reads,
    );

    // 4. No error-severity diagnostics.
    let error_count: usize = result
        .eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .count();
    assert_eq!(
        error_count, 0,
        "non-cyclic let cell fixture must emit zero error diagnostics; got: {:?}",
        result.eval_result.diagnostics,
    );
}
