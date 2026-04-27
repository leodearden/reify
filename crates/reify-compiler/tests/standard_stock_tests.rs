//! Tests for the `std.stock` stdlib module — standard bolt lengths and
//! sheet thickness collections.

use reify_compiler::stdlib_loader;
use reify_test_support::collect_errors;
use reify_types::{DimensionVector, Type, Value, ValueMap};

/// Helper: load the stdlib and find the std.stock CompiledModule.
fn stock_module() -> &'static reify_compiler::CompiledModule {
    let modules = stdlib_loader::load_stdlib();
    modules
        .iter()
        .find(|m| format!("{}", m.path) == "std/stock")
        .expect("std.stock module not found in stdlib")
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
    let module = stock_module();

    let func = module
        .functions
        .iter()
        .find(|f| f.name == "standard_bolt_lengths")
        .expect("standard_bolt_lengths not found in std.stock");

    assert!(func.is_pub, "standard_bolt_lengths should be pub");
    assert!(
        func.params.is_empty(),
        "standard_bolt_lengths should take no params, got: {:?}",
        func.params
    );
    assert_eq!(
        func.return_type,
        Type::List(Box::new(Type::length())),
        "standard_bolt_lengths return type should be List<Length>"
    );

    // Evaluate the function body (no params, no let-bindings needed).
    let values = ValueMap::new();
    let ctx = reify_expr::EvalContext::simple(&values);
    let result = reify_expr::eval_expr(&func.body.result_expr, &ctx);

    // Expected ISO 4014/4017 bolt length series in SI units (meters).
    let expected_si: &[f64] = &[
        0.008, 0.010, 0.012, 0.014, 0.016, 0.020, 0.025, 0.030, 0.035, 0.040,
        0.045, 0.050, 0.055, 0.060, 0.065, 0.070, 0.075, 0.080, 0.090, 0.100,
    ];

    match result {
        Value::List(elems) => {
            assert_eq!(
                elems.len(),
                expected_si.len(),
                "standard_bolt_lengths should have {} elements, got {}",
                expected_si.len(),
                elems.len()
            );
            for (i, (elem, &expected)) in elems.iter().zip(expected_si.iter()).enumerate() {
                match elem {
                    Value::Scalar { si_value, dimension } => {
                        assert_eq!(
                            *dimension,
                            DimensionVector::LENGTH,
                            "element {} should have LENGTH dimension",
                            i
                        );
                        assert!(
                            (si_value - expected).abs() < 1e-12,
                            "element {} si_value: expected {}, got {}",
                            i,
                            expected,
                            si_value
                        );
                    }
                    other => panic!(
                        "element {} should be Value::Scalar, got {:?}",
                        i, other
                    ),
                }
            }
        }
        other => panic!("standard_bolt_lengths should return Value::List, got {:?}", other),
    }
}
