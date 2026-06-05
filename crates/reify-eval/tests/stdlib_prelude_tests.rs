//! Tests for stdlib prelude integration with the eval Engine.

use reify_compiler::stdlib_loader;
use reify_core::{ModulePath, ValueCellId};
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::{collect_errors, steel_elastic_source, steel_material_elastic_source};

// ─── step-7: Engine stores prelude ──────────────────────────────────

/// Engine::new() stores a non-empty prelude from stdlib_loader.
#[test]
fn engine_has_non_empty_prelude() {
    let checker = MockConstraintChecker::new();
    let engine = reify_eval::Engine::new(Box::new(checker), None);
    assert!(
        !engine.prelude().is_empty(),
        "Engine prelude should be non-empty after new()"
    );
}

/// eval() with a user module compiled via compile_with_prelude works for
/// a structure conforming to a prelude trait — values are populated and
/// no error diagnostics. Verifies the 3 specific Elastic params
/// (youngs_modulus, poissons_ratio, shear_modulus) are present.
#[test]
fn eval_with_prelude_trait_conformance() {
    let source = steel_elastic_source();
    let prelude = stdlib_loader::load_stdlib();
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile_with_prelude(&parsed, prelude);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // No error diagnostics from eval
    let eval_errors = collect_errors(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "eval should produce no error diagnostics, got: {:?}",
        eval_errors
    );

    // Verify all 3 Elastic params are present with correct values
    let entity = "Steel";
    let expected_params: &[(&str, f64)] = &[
        ("youngs_modulus", 200.0),
        ("poissons_ratio", 0.3),
        ("shear_modulus", 77.0),
    ];
    for (param, expected_val) in expected_params {
        let cell_id = ValueCellId::new(entity, *param);
        let value = result.values.get(&cell_id).unwrap_or_else(|| {
            panic!(
                "eval should produce a value for Elastic param '{}', but it was missing. \
                 Available values: {:?}",
                param,
                result
                    .values
                    .iter()
                    .map(|(k, _)| k.to_string())
                    .collect::<Vec<_>>()
            )
        });
        let actual = value.as_f64().unwrap_or_else(|| {
            panic!(
                "Elastic param '{}' should be numeric, got: {:?}",
                param, value
            )
        });
        assert!(
            (actual - expected_val).abs() < 1e-9,
            "Elastic param '{}' should be {}, got {}",
            param,
            expected_val,
            actual
        );
    }
}

// ─── step-1: Shadowing regression ────────────────────────────────────

/// Regression guard: user-defined functions shadow prelude functions with
/// identical signatures. A user-defined `symmetric_tolerance` that returns
/// `nominal - deviation` (subtraction) must win over the prelude's
/// `nominal + deviation` (addition) implementation.
///
/// With `5mm, 2mm`:
///   - user impl → 3mm = 0.003 m  (expected)
///   - prelude impl → 7mm = 0.007 m  (would indicate shadowing is broken)
#[test]
fn user_function_shadows_prelude_function() {
    let source = r#"
fn symmetric_tolerance(nominal: Length, deviation: Length) -> Length {
    nominal - deviation
}

structure S {
    let v : Length = symmetric_tolerance(5mm, 2mm)
}
"#;
    let prelude = stdlib_loader::load_stdlib();
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile_with_prelude(&parsed, prelude);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    let eval_errors = collect_errors(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "eval should produce no error diagnostics, got: {:?}",
        eval_errors
    );

    let cell_id = ValueCellId::new("S", "v");
    let value = result.values.get(&cell_id).unwrap_or_else(|| {
        panic!(
            "eval should produce a value for S.v, but it was missing. \
             Available values: {:?}",
            result
                .values
                .iter()
                .map(|(k, _)| k.to_string())
                .collect::<Vec<_>>()
        )
    });
    let actual = value
        .as_f64()
        .unwrap_or_else(|| panic!("S.v should be numeric, got: {:?}", value));
    // User impl: 5mm - 2mm = 3mm = 0.003 m
    // Prelude impl (if shadowing were broken): 5mm + 2mm = 7mm = 0.007 m
    assert!(
        (actual - 0.003).abs() < 1e-9,
        "user function should shadow prelude: expected 0.003 (3mm), got {} (prelude would give 0.007)",
        actual
    );
}

// ─── step-515-1: Arity-mismatch coexistence regression ───────────────

/// Regression guard: a user-defined function does NOT shadow a prelude function
/// when they share a name but differ in arity (or param types). Both must remain
/// independently callable — the dispatch rule is (name, arity, param types).
///
/// Setup:
///   - User defines `symmetric_tolerance(x: Length) -> Length` (1-arg, returns x).
///   - Prelude defines `symmetric_tolerance(nominal, deviation)` (2-arg, returns
///     DimensionalTolerance; `.upper_limit` == nominal+deviation).
///
/// Expected:
///   - `symmetric_tolerance(5mm)` → user impl → 5mm = 0.005 m
///   - `symmetric_tolerance(5mm, 2mm).upper_limit` → prelude impl → upper_limit = 7mm = 0.007 m
///
/// If the arity-mismatch were incorrectly treated as shadowing, the prelude's
/// 2-arg form would be inaccessible and `b` would fail to resolve.
#[test]
fn prelude_function_not_shadowed_by_arity_mismatch() {
    let source = r#"
fn symmetric_tolerance(x: Length) -> Length {
    x
}

structure S {
    let a : Length = symmetric_tolerance(5mm)
    let b : Length = symmetric_tolerance(5mm, 2mm).upper_limit
}
"#;
    let prelude = stdlib_loader::load_stdlib();
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile_with_prelude(&parsed, prelude);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    let eval_errors = collect_errors(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "eval should produce no error diagnostics, got: {:?}",
        eval_errors
    );

    // a = symmetric_tolerance(5mm) → user impl (1-arg, returns x) → 5mm = 0.005 m
    let cell_a = ValueCellId::new("S", "a");
    let a = result
        .values
        .get(&cell_a)
        .and_then(|v| v.as_f64())
        .unwrap_or_else(|| {
            panic!(
                "S.a should be numeric. Available values: {:?}",
                result
                    .values
                    .iter()
                    .map(|(k, _)| k.to_string())
                    .collect::<Vec<_>>()
            )
        });
    assert!(
        (a - 0.005).abs() < 1e-9,
        "S.a: user 1-arg symmetric_tolerance(5mm) should return 5mm=0.005, got {}",
        a
    );

    // b = symmetric_tolerance(5mm, 2mm).upper_limit → prelude impl (2-arg, returns DimensionalTolerance;
    //   .upper_limit == nominal+deviation = 5mm+2mm = 7mm = 0.007 m)
    let cell_b = ValueCellId::new("S", "b");
    let b = result
        .values
        .get(&cell_b)
        .and_then(|v| v.as_f64())
        .unwrap_or_else(|| {
            panic!(
                "S.b should be numeric. Available values: {:?}",
                result
                    .values
                    .iter()
                    .map(|(k, _)| k.to_string())
                    .collect::<Vec<_>>()
            )
        });
    assert!(
        (b - 0.007).abs() < 1e-9,
        "S.b: prelude 2-arg symmetric_tolerance(5mm, 2mm) should return 7mm=0.007, got {}",
        b
    );
}

// ─── step-4: Constraint solver path regression ───────────────────────

/// Regression guard: prelude functions must be reachable from the constraint
/// solver's `ResolutionProblem` (lib.rs:1361) and from the post-solver
/// let-binding re-evaluation (lib.rs:1455).
///
/// Uses `auto(free)` to skip uniqueness verification and an inequality
/// constraint whose initial point (10mm) is immediately feasible, so the
/// solver returns without running Nelder-Mead — giving a predictable x value.
///
/// Without the fix at 1361, `problem.functions` contains only user functions
/// (empty here), so `symmetric_tolerance(15mm, 5mm)` in the constraint
/// expression evaluates to Undef — the initial point appears infeasible, the
/// Nelder-Mead runs and fails, and an error diagnostic is emitted.
/// Without the fix at 1455, the post-solver `evaluate_let_bindings` uses
/// `&module.functions` (empty) so `symmetric_tolerance(x, 1mm)` → Undef.
///
/// Expected relationships (prelude `symmetric_tolerance(a, b)` returns DimensionalTolerance;
/// `.upper_limit = a + b`):
///   - `x` is finite and satisfies `x < 20mm` (solver produced a feasible point)
///   - `y = symmetric_tolerance(x, 1mm).upper_limit = x + 0.001` exactly (prelude was
///     reachable in post-solver re-eval)
///
/// The exact value of `x` is not asserted — it depends on DimensionalSolver's
/// feasibility-shortcut policy and pinning it would couple this test to solver
/// internals instead of the prelude reachability it guards.
#[test]
fn prelude_function_resolves_in_constraint_solver_path() {
    use reify_constraints::DimensionalSolver;

    let source = r#"
structure S {
    param x : Length = auto(free)
    let y : Length = symmetric_tolerance(x, 1mm).upper_limit
    constraint x < symmetric_tolerance(15mm, 5mm).upper_limit
}
"#;
    let prelude = stdlib_loader::load_stdlib();
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile_with_prelude(&parsed, prelude);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    let checker = MockConstraintChecker::new();
    let mut engine =
        reify_eval::Engine::new(Box::new(checker), None).with_solver(Box::new(DimensionalSolver));
    let result = engine.eval(&compiled);

    // Only the auto(free) non-unique warning is expected; no errors.
    let eval_errors = collect_errors(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "eval should produce no error diagnostics, got: {:?}",
        eval_errors
    );

    // x = 10mm (solver initial; x < 20mm is already satisfied → returned immediately)
    let cell_x = ValueCellId::new("S", "x");
    let x = result
        .values
        .get(&cell_x)
        .and_then(|v| v.as_f64())
        .unwrap_or_else(|| {
            panic!(
                "S.x should be numeric. Available: {:?}",
                result
                    .values
                    .iter()
                    .map(|(k, _)| k.to_string())
                    .collect::<Vec<_>>()
            )
        });
    // Assert solver feasibility rather than a specific x value: we care that the solver
    // produced a finite result satisfying the constraint `x < symmetric_tolerance(15mm, 5mm)`
    // = 20mm. The exact x depends on DimensionalSolver's feasibility-shortcut policy; pinning
    // x == 0.010 would couple this test to that internal behavior rather than to the prelude
    // reachability it is supposed to guard.
    assert!(
        x.is_finite() && x < 0.020,
        "S.x should be finite and satisfy x < 20mm constraint, got {}",
        x
    );

    // y = symmetric_tolerance(x, 1mm).upper_limit = x + 0.001 (exact, by prelude definition;
    //   upper_limit = nominal + upper_deviation = x + 1mm).
    // This is the real regression guard: it proves the prelude function was reachable
    // from the post-solver let-binding re-evaluation, regardless of solver x choice.
    let cell_y = ValueCellId::new("S", "y");
    let y = result
        .values
        .get(&cell_y)
        .and_then(|v| v.as_f64())
        .unwrap_or_else(|| {
            panic!(
                "S.y should be numeric. Available: {:?}",
                result
                    .values
                    .iter()
                    .map(|(k, _)| k.to_string())
                    .collect::<Vec<_>>()
            )
        });
    assert!(
        (y - (x + 0.001)).abs() < 1e-9,
        "S.y: post-solver re-eval should give symmetric_tolerance(x, 1mm).upper_limit \
         = x + 0.001 = {}, got {}",
        x + 0.001,
        y
    );
}

// ─── step-3: Eval idempotency (caching regression) ───────────────────

/// Regression guard: calling `eval()` twice on the same engine with the
/// same module must produce identical results.
///
/// The value assertion (`v1 == v2 == 0.003 m`) verifies per-eval resolution
/// stability. The user `symmetric_tolerance` uses **subtraction** (`nominal -
/// deviation`) — distinct from the prelude's addition variant — so this is an
/// independent claim: if shadowing silently broke and eval() resolved to the
/// prelude's `+`, v1/v2 would be 0.007 — this test would fail.
///
/// The accumulation-regression guard (eval() must replace, not extend,
/// `self.functions`) is covered by the unit test
/// `reify_eval::tests::eval_does_not_accumulate_functions` in `src/lib.rs`.
#[test]
fn eval_is_idempotent_for_prelude_functions() {
    let source = r#"
fn symmetric_tolerance(nominal: Length, deviation: Length) -> Length {
    nominal - deviation
}

structure S {
    let v : Length = symmetric_tolerance(5mm, 2mm)
}
"#;
    let prelude = stdlib_loader::load_stdlib();
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile_with_prelude(&parsed, prelude);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);

    // First eval
    let result1 = engine.eval(&compiled);
    let eval_errors1 = collect_errors(&result1.diagnostics);
    assert!(
        eval_errors1.is_empty(),
        "first eval: no error diagnostics expected, got: {:?}",
        eval_errors1
    );
    let cell_id = ValueCellId::new("S", "v");
    let v1 = result1
        .values
        .get(&cell_id)
        .and_then(|v| v.as_f64())
        .unwrap_or_else(|| panic!("S.v missing or non-numeric on first eval"));

    // Second eval on same engine
    let result2 = engine.eval(&compiled);
    let eval_errors2 = collect_errors(&result2.diagnostics);
    assert!(
        eval_errors2.is_empty(),
        "second eval: no error diagnostics expected, got: {:?}",
        eval_errors2
    );
    let v2 = result2
        .values
        .get(&cell_id)
        .and_then(|v| v.as_f64())
        .unwrap_or_else(|| panic!("S.v missing or non-numeric on second eval"));

    assert!(
        (v1 - v2).abs() < 1e-9,
        "eval() must be idempotent: first={} second={} (differ by {})",
        v1,
        v2,
        (v1 - v2).abs()
    );
    // User impl (subtraction, distinct from prelude): 5mm - 2mm = 3mm = 0.003 m
    // (prelude would give 0.007 if shadowing broke)
    assert!(
        (v1 - 0.003).abs() < 1e-9,
        "symmetric_tolerance(5mm, 2mm) should be 0.003 m (3mm), got {}",
        v1
    );
}

// ─── step-6: Downstream dispatch path regression ─────────────────────

/// Regression guard: prelude functions must be reachable at ALL dispatch
/// sites, not just inside `eval()`. The `check()` path calls
/// `dispatch_constraints` at lib.rs:2803 with `&module.functions` (user-only)
/// before the step-7 sweep.
///
/// `SimpleConstraintChecker` (unlike `MockConstraintChecker`) evaluates each
/// constraint expression via `eval_expr`. Without the fix at 2803,
/// `symmetric_tolerance(1mm, 1mm)` evaluates to Undef, making the constraint
/// `Indeterminate` rather than `Satisfied`.
///
/// With the fix: `symmetric_tolerance(1mm, 1mm)` = 2mm → `5mm > 2mm = true`
/// → `Satisfied`, no warning.
#[test]
fn prelude_function_resolves_in_downstream_dispatch_paths() {
    use reify_constraints::SimpleConstraintChecker;
    use reify_ir::Satisfaction;

    let source = r#"
structure S {
    let y : Length = 5mm
    constraint y > symmetric_tolerance(1mm, 1mm).upper_limit
}
"#;
    let prelude = stdlib_loader::load_stdlib();
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile_with_prelude(&parsed, prelude);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    let mut engine = reify_eval::Engine::new(Box::new(SimpleConstraintChecker), None);
    let check_result = engine.check(&compiled);

    let check_errors = collect_errors(&check_result.diagnostics);
    assert!(
        check_errors.is_empty(),
        "check() should produce no error diagnostics, got: {:?}",
        check_errors
    );

    assert_eq!(
        check_result.constraint_results.len(),
        1,
        "should have exactly 1 constraint result"
    );
    let cr = &check_result.constraint_results[0];
    assert_eq!(
        cr.satisfaction,
        Satisfaction::Satisfied,
        "constraint y > symmetric_tolerance(1mm,1mm) should be Satisfied (5mm > 2mm), \
         got {:?}. All diagnostics: {:?}",
        cr.satisfaction,
        check_result.diagnostics
    );
}

// ─── step-9: End-to-end prelude pipeline ─────────────────────────────

/// Full pipeline: .ri source → compile_with_prelude → Engine::eval.
/// User code conforms to both MaterialSpec and Elastic prelude traits.
/// Asserts: (1) no compile errors, (2) eval returns values for all 5 params,
/// (3) trait_bounds on template include both MaterialSpec and Elastic.
#[test]
fn end_to_end_material_elastic_conformance() {
    let source = steel_material_elastic_source();
    let prelude = stdlib_loader::load_stdlib();
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    // (1) No error diagnostics from compilation
    let compiled = reify_compiler::compile_with_prelude(&parsed, prelude);
    let compile_errors = collect_errors(&compiled.diagnostics);
    assert!(
        compile_errors.is_empty(),
        "compile should produce no error diagnostics, got: {:?}",
        compile_errors
    );

    // (3) trait_bounds on template include both MaterialSpec and Elastic
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "Steel")
        .expect("Steel template should exist in compiled module");
    assert!(
        template.trait_bounds.contains(&"MaterialSpec".to_string()),
        "Steel should have 'MaterialSpec' trait bound, got: {:?}",
        template.trait_bounds
    );
    assert!(
        template.trait_bounds.contains(&"Elastic".to_string()),
        "Steel should have 'Elastic' trait bound, got: {:?}",
        template.trait_bounds
    );

    // (2) eval returns values for all 5 params
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    let eval_errors = collect_errors(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "eval should produce no error diagnostics, got: {:?}",
        eval_errors
    );

    // Check that all 5 params have values in the result
    let entity = "Steel";
    let expected_params = [
        "density",
        "name",
        "youngs_modulus",
        "poissons_ratio",
        "shear_modulus",
    ];
    for param in &expected_params {
        let cell_id = reify_core::ValueCellId::new(entity, *param);
        assert!(
            result.values.get(&cell_id).is_some(),
            "eval should produce a value for param '{}', but it was missing. \
             Available values: {:?}",
            param,
            result
                .values
                .iter()
                .map(|(k, _)| k.to_string())
                .collect::<Vec<_>>()
        );
    }
}

// ── step-5: Engine::with_prelude ──────────────────────────────────────

/// Engine::with_prelude accepts an empty prelude slice; prelude() returns
/// an empty slice afterwards.
#[test]
fn engine_with_prelude_accepts_custom_slice() {
    let checker = MockConstraintChecker::new();
    let engine = reify_eval::Engine::with_prelude(Box::new(checker), None, &[]);
    assert!(
        engine.prelude().is_empty(),
        "Engine::with_prelude(&[]) should store an empty prelude, got {} modules",
        engine.prelude().len()
    );
}

/// Compiling a source that references a stdlib trait (`Elastic`) with an empty
/// prelude produces at least one diagnostic, confirming that the empty-prelude
/// opt-out actually suppresses stdlib trait resolution.
///
/// The preceding test (`engine_with_prelude_accepts_custom_slice`) already pins
/// the empty-prelude invariant; this test verifies the concrete consequence:
/// without stdlib in the prelude, trait references cannot be resolved and the
/// compiler reports an error.
#[test]
fn engine_with_prelude_empty_slice_disables_stdlib_resolution() {
    let source = steel_elastic_source();
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    // Compile WITHOUT any prelude (empty slice). The source declares conformance
    // to the stdlib trait `Elastic`, which is unavailable without the prelude.
    let compiled = reify_compiler::compile_with_prelude(&parsed, &[]);

    // Without stdlib in the prelude, trait conformance cannot be verified:
    // the compiler must emit at least one diagnostic about the unresolved trait.
    assert!(
        !compiled.diagnostics.is_empty(),
        "compiling 'Steel : Elastic' with an empty prelude should produce at least \
         one diagnostic (unresolved trait 'Elastic'), but got zero diagnostics. \
         This would indicate stdlib resolution is NOT disabled by the empty prelude."
    );
}
