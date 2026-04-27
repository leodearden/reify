//! Tests for the `std.stock` stdlib module — standard bolt lengths and
//! sheet thickness collections.

use reify_compiler::stdlib_loader;
use reify_test_support::collect_errors;
use reify_types::{DimensionVector, ModulePath, Type, Value, ValueMap};

/// Helper: load the stdlib and find the std.stock CompiledModule.
fn stock_module() -> &'static reify_compiler::CompiledModule {
    let modules = stdlib_loader::load_stdlib();
    modules
        .iter()
        .find(|m| m.path == ModulePath::from_dotted("std.stock").unwrap())
        .expect("std.stock module not found in stdlib")
}

/// Assert that a named `pub fn` in `module` has no params, returns `List<Length>`,
/// and evaluates to a `Value::List` whose elements match `expected_si` (SI metres,
/// within 1e-12) with `DimensionVector::LENGTH` dimension.
///
/// Evaluation goes **via `eval_user_function_call`** (populating
/// `ctx.functions = &module.functions`) rather than evaluating
/// `func.body.result_expr` directly, so a future refactor that introduces
/// `let` bindings inside either function does not silently drop bindings or
/// yield `Undef`.
fn assert_length_constant(
    module: &reify_compiler::CompiledModule,
    name: &str,
    expected_si: &[f64],
) {
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
    assert_eq!(
        func.return_type,
        Type::List(Box::new(Type::length())),
        "{} return type should be List<Length>",
        name
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
        Value::List(elems) => {
            assert_eq!(
                elems.len(),
                expected_si.len(),
                "{} should have {} elements, got {}",
                name,
                expected_si.len(),
                elems.len()
            );
            for (i, (elem, &expected)) in elems.iter().zip(expected_si.iter()).enumerate() {
                match elem {
                    Value::Scalar { si_value, dimension } => {
                        assert_eq!(
                            *dimension,
                            DimensionVector::LENGTH,
                            "{} element {} should have LENGTH dimension",
                            name,
                            i
                        );
                        assert!(
                            (si_value - expected).abs() < 1e-12,
                            "{} element {} si_value: expected {}, got {}",
                            name,
                            i,
                            expected,
                            si_value
                        );
                    }
                    other => panic!(
                        "{} element {} should be Value::Scalar, got {:?}",
                        name, i, other
                    ),
                }
            }
        }
        other => panic!("{} should return Value::List, got {:?}", name, other),
    }
}

// ─── step-1: module loads with no errors ─────────────────────────────────────

/// std.stock module is present in the stdlib and has zero error diagnostics.
#[test]
fn std_stock_module_loads_with_no_errors() {
    let module = stock_module();
    let errors = collect_errors(&module.diagnostics);
    assert!(
        errors.is_empty(),
        "std.stock module has error diagnostics: {:?}",
        errors
    );
}

// ─── step-3: standard_bolt_lengths function ───────────────────────────────────

/// standard_bolt_lengths is present in std.stock, is pub, has no params,
/// returns List<Length>, and evaluates to the 20-element ISO 4014/4017 series.
#[test]
fn standard_bolt_lengths_function_present_and_returns_iso_4014_series() {
    #[rustfmt::skip]
    assert_length_constant(
        stock_module(),
        "standard_bolt_lengths",
        &[
            0.008, 0.010, 0.012, 0.014, 0.016, 0.020, 0.025, 0.030, 0.035, 0.040,
            0.045, 0.050, 0.055, 0.060, 0.065, 0.070, 0.075, 0.080, 0.090, 0.100,
        ],
    );
}

// ─── step-5: standard_sheet_thicknesses function ─────────────────────────────

/// standard_sheet_thicknesses is present in std.stock, is pub, has no params,
/// returns List<Length>, and evaluates to the 13-element common metal gauge series.
#[test]
fn standard_sheet_thicknesses_function_present_and_returns_metal_gauge_series() {
    #[rustfmt::skip]
    assert_length_constant(
        stock_module(),
        "standard_sheet_thicknesses",
        &[
            0.0005, 0.0008, 0.0010, 0.0012, 0.0015, 0.0020, 0.0025,
            0.0030, 0.0040, 0.0050, 0.0060, 0.0080, 0.0100,
        ],
    );
}
