//! Stdlib gate tests for `pub fn through<T>(x: T) -> T` in std.fields
//! (task 4233 δ step-7/step-8).
//!
//! This file is the literal 4218 completion gate: "a generic stdlib .ri fn
//! type-checks end-to-end." Both tests are RED until step-8 adds
//! `pub fn through<T>(x: T) -> T { x }` to crates/reify-compiler/stdlib/fields.ri.

use reify_compiler::stdlib_loader;
use reify_core::{DimensionVector, ModulePath, Severity, ValueCellId};
use reify_test_support::collect_errors;
use reify_test_support::mocks::MockConstraintChecker;

// ── (a) stdlib loads with the generic fn, no Error diagnostics ───────────────

/// stdlib_loader::load_stdlib() must produce zero Error-severity diagnostics
/// in every module AND the std.fields CompiledModule must contain a
/// CompiledFunction named "through" with a non-empty `type_params` list.
///
/// Proves that `pub fn through<T>` in fields.ri type-checks correctly during
/// stdlib load and that the type parameters are lowered.
///
/// RED until step-8: std.fields contains no CompiledFunctions today.
#[test]
fn stdlib_loads_generic_fn_clean() {
    let stdlib = stdlib_loader::load_stdlib();

    // No Error-severity diagnostic in any module.
    for module in stdlib {
        let errors: Vec<_> = module
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "stdlib module '{}' should have no Error diagnostics, got: {errors:?}",
            module.path
        );
    }

    // std.fields must contain a function named "through" with type_params.
    // Note: ModulePath::Display joins segments with "/" (not "."), so
    // ModulePath::from_dotted("std.fields") displays as "std/fields".
    let fields_module = stdlib
        .iter()
        .find(|m| m.path.to_string() == "std/fields")
        .expect("std.fields module must be present in stdlib");

    let through_fn = fields_module
        .functions
        .iter()
        .find(|f| f.name == "through")
        .expect(
            "std.fields must contain a function named 'through' (pub fn through<T>(x: T) -> T)",
        );

    assert!(
        !through_fn.type_params.is_empty(),
        "through must have at least one type parameter (T), got empty type_params"
    );
}

// ── (b) identity evals end-to-end from user source ───────────────────────────

/// A user source `structure S {{ let v = through(5mm) }}` (through is
/// prelude-resolved from std.fields; no import needed) must:
///   - Compile with zero Error diagnostics
///   - Eval to Value::Scalar{{ si_value: 0.005, dimension: LENGTH }}
///
/// Proves the generic stdlib fn type-checks AND evaluates end-to-end.
///
/// RED until step-8: `through` is not in fields.ri yet — the call resolves
/// to NoMatch and the compile produces a diagnostic (or the value is wrong).
#[test]
fn stdlib_generic_fn_evals_end_to_end() {
    let source = "structure S { let v = through(5mm) }";

    let prelude = stdlib_loader::load_stdlib();
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors for through(5mm) source: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile_with_prelude(&parsed, prelude);
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "compile errors for through(5mm): {errors:?}"
    );

    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    let eval_errors = collect_errors(&result.diagnostics);
    assert!(
        eval_errors.is_empty(),
        "eval errors for through(5mm): {eval_errors:?}"
    );

    let cell_id = ValueCellId::new("S", "v");
    let value = result.values.get(&cell_id).unwrap_or_else(|| {
        panic!(
            "eval should produce a value for S.v; available: {:?}",
            result
                .values
                .iter()
                .map(|(k, _)| k.to_string())
                .collect::<Vec<_>>()
        )
    });

    assert_eq!(
        *value,
        reify_ir::Value::Scalar {
            si_value: 0.005,
            dimension: DimensionVector::LENGTH,
        },
        "through(5mm) should evaluate to the 5mm length scalar, got {value:?}"
    );
}
