//! Step-3 RED: `compile_materialization_annotation_args` surface test.
//!
//! Pins the compiler-side helper that extracts `AtMaterialization` annotation arg
//! slots and compiles their `reify_ast::Expr`s into `reify_ir::CompiledExpr`s.
//!
//! This test FAILS TO COMPILE on base (before step-4 GREEN):
//!   - `reify_compiler::compile_materialization_annotation_args` is absent.
//!   - `reify_compiler::MaterializationAnnotationArg` is absent.
//!   - `reify_compiler::MaterializationArgType` is absent.
//!   - The `@test_eval` schema entry is absent (`lookup_schema("test_eval")` returns None).
//!
//! All of those are added in step-4 GREEN.

use reify_compiler::{MaterializationAnnotationArg, MaterializationArgType};
use reify_ir::{Value, ValueMap};
use reify_test_support::{errors_only, parse_and_compile_with_stdlib};

const SMOKE_SOURCE: &str = include_str!("fixtures/eval_annotation_smoke.ri");

/// `@test_eval(2.0 * 1.5)` on `Foo` produces one `MaterializationAnnotationArg`
/// with annotation "test_eval", arg_name "value", expected Real, and the compiled
/// expr evaluates to `Value::Real(3.0)`.
///
/// Also asserts that compiling the smoke source produces NO Error-severity
/// diagnostics — the `@test_eval` Expr arg validates cleanly via `arg_check: None`.
#[test]
fn compile_materialization_args_smoke() {
    // Compile with stdlib (required for standard Real type resolution).
    let module = parse_and_compile_with_stdlib(SMOKE_SOURCE);

    // No Error-severity diagnostics on smoke source.
    assert!(
        errors_only(&module).is_empty(),
        "unexpected compile errors: {:?}",
        errors_only(&module)
    );

    // Find the Foo template.
    let foo = module
        .templates
        .iter()
        .find(|t| t.name == "Foo")
        .expect("Foo template not found in compiled module");

    // compile_materialization_annotation_args should find exactly one arg entry.
    let margs = reify_compiler::compile_materialization_annotation_args(
        foo,
        &module.enum_defs,
        &module.functions,
    );

    assert_eq!(
        margs.len(),
        1,
        "expected exactly one materialization arg entry from @test_eval, got: {}",
        margs.len()
    );

    let entry: &MaterializationAnnotationArg = &margs[0];
    assert_eq!(entry.annotation, "test_eval", "annotation name mismatch");
    assert_eq!(
        entry.arg_name, "value",
        "arg name should come from schema positional_index 0 → name \"value\""
    );
    assert!(
        matches!(entry.expected, MaterializationArgType::Real),
        "expected MaterializationArgType::Real, got: {:?}",
        entry.expected
    );

    // Evaluate the compiled expr — 2.0 * 1.5 = 3.0 (f64-exact).
    let empty_values = ValueMap::new();
    let ctx = reify_expr::EvalContext::new(&empty_values, &[]);
    let result = reify_expr::eval_expr(&entry.expr, &ctx);
    assert_eq!(
        result,
        Value::Real(3.0),
        "2.0 * 1.5 should evaluate to Real(3.0), got: {:?}",
        result
    );
}
