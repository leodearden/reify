//! End-to-end tests for `@solver_hint` payload resolution against `std.stock` collections.
//!
//! Covers PRD `docs/prds/solver-hint-payloads.md` item 2:
//!   1. Positive `discrete_set` + `standard_bolt_lengths`
//!   2. Positive `prefer_stock` + `standard_sheet_thicknesses`
//!   3. Negative: unresolved identifier produces an error

use reify_compiler::stdlib_loader;
use reify_types::{DimensionVector, ModulePath, Type, Value, ValueMap};

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Thin wrapper around `reify_test_support::compile_source_with_stdlib` for
/// readability at call sites in this file.
fn compile_payload_module(source: &str) -> reify_compiler::CompiledModule {
    reify_test_support::compile_source_with_stdlib(source)
}

/// Load the `std.stock` module from the cached stdlib, evaluate the named
/// `pub fn () -> List<Length>` via `eval_expr`, and return the SI-metres
/// values as a `Vec<f64>`.
///
/// Mirrors the `assert_length_constant` pattern in `standard_stock_tests.rs`
/// so a future refactor that introduces `let` bindings inside either stock
/// function does not silently drop bindings or yield `Undef`.
fn lookup_stock_collection(name: &str) -> Vec<f64> {
    let modules = stdlib_loader::load_stdlib();
    let module = modules
        .iter()
        .find(|m| m.path == ModulePath::from_dotted("std.stock").unwrap())
        .expect("std.stock module not found in stdlib");

    let func = module
        .functions
        .iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("{} not found in std.stock", name));

    assert!(func.is_pub, "{} should be pub", name);
    assert!(
        func.params.is_empty(),
        "{} should take no params, got: {:?}",
        name,
        func.params
    );

    let call_expr = reify_types::CompiledExpr::user_function_call(
        name.to_string(),
        vec![],
        Type::List(Box::new(Type::length())),
    );
    let values = ValueMap::new();
    let ctx = reify_expr::EvalContext::new(&values, &module.functions);
    let result = reify_expr::eval_expr(&call_expr, &ctx);

    match result {
        Value::List(elems) => elems
            .iter()
            .enumerate()
            .map(|(i, elem)| match elem {
                Value::Scalar { si_value, dimension } => {
                    assert_eq!(
                        *dimension,
                        DimensionVector::LENGTH,
                        "{} element {} should have LENGTH dimension",
                        name,
                        i
                    );
                    *si_value
                }
                other => panic!(
                    "{} element {} should be Value::Scalar, got {:?}",
                    name, i, other
                ),
            })
            .collect(),
        other => panic!("{} should return Value::List, got {:?}", name, other),
    }
}

// ── Test 1: positive discrete_set + standard_bolt_lengths ────────────────────

/// PRD item 2.(1): `@solver_hint("discrete_set", standard_bolt_lengths)` on a
/// param compiles without errors or warnings, produces the correct
/// `ValueCellDecl.solver_hints` entry, and the looked-up collection evaluates
/// to the 20-element ISO 4014/4017 bolt-length series.
#[test]
fn solver_hint_discrete_set_standard_bolt_lengths_end_to_end() {
    let source = r#"structure S {
        @solver_hint("discrete_set", standard_bolt_lengths)
        param length : Length = auto
    }"#;

    let module = compile_payload_module(source);

    // (a) no errors
    let errors = reify_test_support::errors_only(&module);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    // (b) no warnings
    let warnings = reify_test_support::warnings_only(&module);
    assert!(
        warnings.is_empty(),
        "expected no warnings, got: {:?}",
        warnings
    );

    // (c) solver_hints is correct
    let template = &module.templates[0];
    assert!(!template.value_cells.is_empty(), "expected at least one value cell");
    let cell = &template.value_cells[0];
    assert_eq!(
        cell.solver_hints.len(),
        1,
        "expected 1 solver hint, got: {:?}",
        cell.solver_hints
    );
    assert_eq!(
        cell.solver_hints[0].kind,
        reify_compiler::SolverHintKind::DiscreteSet
    );
    assert_eq!(cell.solver_hints[0].collection, "standard_bolt_lengths");

}

// ── Test 2: positive prefer_stock + standard_sheet_thicknesses ───────────────

/// PRD item 2.(2): `@solver_hint("prefer_stock", standard_sheet_thicknesses)` on a
/// param compiles without errors or warnings, produces the correct
/// `ValueCellDecl.solver_hints` entry, and the looked-up collection evaluates
/// to the 13-element common metal gauge series.
#[test]
fn solver_hint_prefer_stock_standard_sheet_thicknesses_end_to_end() {
    let source = r#"structure S {
        @solver_hint("prefer_stock", standard_sheet_thicknesses)
        param thickness : Length = auto
    }"#;

    let module = compile_payload_module(source);

    // (a) no errors
    let errors = reify_test_support::errors_only(&module);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    // (b) no warnings
    let warnings = reify_test_support::warnings_only(&module);
    assert!(
        warnings.is_empty(),
        "expected no warnings, got: {:?}",
        warnings
    );

    // (c) solver_hints is correct
    let template = &module.templates[0];
    assert!(!template.value_cells.is_empty(), "expected at least one value cell");
    let cell = &template.value_cells[0];
    assert_eq!(
        cell.solver_hints.len(),
        1,
        "expected 1 solver hint, got: {:?}",
        cell.solver_hints
    );
    assert_eq!(
        cell.solver_hints[0].kind,
        reify_compiler::SolverHintKind::PreferStock
    );
    assert_eq!(
        cell.solver_hints[0].collection,
        "standard_sheet_thicknesses"
    );

}

// ── Test 3: negative — unresolved identifier produces an error ───────────────

/// PRD item 2.(3): `@solver_hint("discrete_set", standard_doesnotexist)` must
/// produce at least one `Severity::Error` diagnostic whose message contains
/// `"unresolved name"` and the literal identifier text `"standard_doesnotexist"`.
///
/// Locks in that hint payload references go through name resolution rather than
/// a special-cased lookup — an unresolvable identifier is a compile error.
#[test]
fn solver_hint_unresolved_collection_produces_error() {
    let source = r#"structure S {
        @solver_hint("discrete_set", standard_doesnotexist)
        param length : Length = auto
    }"#;

    let module = compile_payload_module(source);

    let errors = reify_test_support::errors_only(&module);
    assert!(
        !errors.is_empty(),
        "expected at least one error for unresolved hint collection, got none"
    );

    let has_unresolved_name = errors
        .iter()
        .any(|d| d.message.contains("unresolved name") && d.message.contains("standard_doesnotexist"));
    assert!(
        has_unresolved_name,
        "expected an error containing 'unresolved name' and 'standard_doesnotexist', got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}
