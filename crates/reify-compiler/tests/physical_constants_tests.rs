//! Tests for physical constants in `std/units` (task 4026).
//!
//! Initially pins two leaf signals for SPEED_OF_LIGHT (steps 1-2);
//! BOLTZMANN_CONSTANT tests are appended in step-3.
//!
//! SI references:
//!   - c = 299792458 m/s exactly — SI second/metre definition (BIPM, 1983).
//!   - k_B = 1.380649e-23 J/K exactly — 2019 SI redefinition
//!     (CGPM 26th meeting, Resolution 1).
//!
//! Pattern lifted from `standard_gravity_tests.rs`.

mod common;

use reify_core::{DimensionVector, Type};
use reify_ir::{CompiledExpr, Value, ValueMap};

// ─── Test 1: SPEED_OF_LIGHT present and has correct signature ─────────────────

/// `SPEED_OF_LIGHT` must be present in `std/units`, be `pub`, take no
/// parameters, and return `Scalar<LENGTH / TIME>`.
///
/// Return type uses the `Length / Time` type-expression form (not `Velocity`)
/// because `Velocity` is not in NAMED_DIMENSIONS — design decision recorded
/// in plan.json for task 4026.
#[test]
fn speed_of_light_function_present_in_std_units() {
    let module = common::units_module();

    let func = module
        .functions
        .iter()
        .find(|f| f.name == "SPEED_OF_LIGHT")
        .unwrap_or_else(|| {
            panic!(
                "SPEED_OF_LIGHT not found in std/units; found functions: {:?}",
                module.functions.iter().map(|f| &f.name).collect::<Vec<_>>()
            )
        });

    assert!(func.is_pub, "SPEED_OF_LIGHT should be pub");
    assert!(
        func.params.is_empty(),
        "SPEED_OF_LIGHT should take no params, got: {:?}",
        func.params
    );

    let expected_dim = DimensionVector::LENGTH.div(&DimensionVector::TIME);
    assert_eq!(
        func.return_type,
        Type::Scalar {
            dimension: expected_dim
        },
        "SPEED_OF_LIGHT return type should be Scalar<LENGTH / TIME>, got {:?}",
        func.return_type
    );
}

// ─── Test 2: SPEED_OF_LIGHT evaluates to 299792458 m/s ───────────────────────

/// Evaluating `SPEED_OF_LIGHT()` via `eval_expr` must yield a
/// `Value::Scalar` with `si_value ≈ 299792458.0` and `dimension = LENGTH / TIME`.
///
/// c = 299792458 m/s exactly (SI definition, BIPM 1983).
#[test]
fn speed_of_light_evaluates_to_299792458_si_with_length_over_time_dimension() {
    let module = common::units_module();

    let expected_dim = DimensionVector::LENGTH.div(&DimensionVector::TIME);
    let call_expr = CompiledExpr::user_function_call(
        "SPEED_OF_LIGHT".to_string(),
        vec![],
        Type::Scalar {
            dimension: expected_dim,
        },
    );
    let values = ValueMap::new();
    let ctx = reify_expr::EvalContext::new(&values, &module.functions);
    let result = reify_expr::eval_expr(&call_expr, &ctx);

    match result {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(
                dimension,
                DimensionVector::LENGTH.div(&DimensionVector::TIME),
                "SPEED_OF_LIGHT() should have LENGTH / TIME dimension, got {:?}",
                dimension
            );
            assert!(
                (si_value - 299792458.0).abs() < 1e-12,
                "SPEED_OF_LIGHT() si_value: expected 299792458.0, got {}",
                si_value
            );
        }
        other => panic!(
            "SPEED_OF_LIGHT() should return Value::Scalar, got {:?}",
            other
        ),
    }
}

// ─── Test 3: BOLTZMANN_CONSTANT present and has correct signature ─────────────

/// `BOLTZMANN_CONSTANT` must be present in `std/units`, be `pub`, take no
/// parameters, and return `Scalar<ENERGY / TEMPERATURE>`.
///
/// Return type resolves via the `HeatCapacity` type alias (`pub type HeatCapacity =
/// Energy / Temperature` in units.ri, introduced by esc-4026-121), which the
/// compiler expands to `Scalar<ENERGY/TEMPERATURE>`.
///
/// k_B = 1.380649e-23 J/K exactly — 2019 SI redefinition
/// (CGPM 26th meeting, Resolution 1).
#[test]
fn boltzmann_constant_function_present_in_std_units() {
    let module = common::units_module();

    let func = module
        .functions
        .iter()
        .find(|f| f.name == "BOLTZMANN_CONSTANT")
        .unwrap_or_else(|| {
            panic!(
                "BOLTZMANN_CONSTANT not found in std/units; found functions: {:?}",
                module.functions.iter().map(|f| &f.name).collect::<Vec<_>>()
            )
        });

    assert!(func.is_pub, "BOLTZMANN_CONSTANT should be pub");
    assert!(
        func.params.is_empty(),
        "BOLTZMANN_CONSTANT should take no params, got: {:?}",
        func.params
    );

    let expected_dim = DimensionVector::ENERGY.div(&DimensionVector::TEMPERATURE);
    assert_eq!(
        func.return_type,
        Type::Scalar {
            dimension: expected_dim
        },
        "BOLTZMANN_CONSTANT return type should be Scalar<ENERGY / TEMPERATURE>, got {:?}",
        func.return_type
    );
}

// ─── Test 4: BOLTZMANN_CONSTANT evaluates to 1.380649e-23 J/K ────────────────

/// Evaluating `BOLTZMANN_CONSTANT()` via `eval_expr` must yield a
/// `Value::Scalar` with `si_value ≈ 1.380649e-23` and
/// `dimension = ENERGY / TEMPERATURE`.
///
/// Tolerance is 1e-35: the stored decimal literal `0.00000000000000000000001380649`
/// has 7 significant figures; f64 precision is ~15-17 digits, so the round-trip
/// error is ≤ 1.5 × ulp ≈ 3e-39, comfortably under 1e-35.
#[test]
fn boltzmann_constant_evaluates_to_1p380649e_minus_23_si_with_energy_over_temperature_dimension() {
    let module = common::units_module();

    let expected_dim = DimensionVector::ENERGY.div(&DimensionVector::TEMPERATURE);
    let call_expr = CompiledExpr::user_function_call(
        "BOLTZMANN_CONSTANT".to_string(),
        vec![],
        Type::Scalar {
            dimension: expected_dim,
        },
    );
    let values = ValueMap::new();
    let ctx = reify_expr::EvalContext::new(&values, &module.functions);
    let result = reify_expr::eval_expr(&call_expr, &ctx);

    match result {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(
                dimension,
                DimensionVector::ENERGY.div(&DimensionVector::TEMPERATURE),
                "BOLTZMANN_CONSTANT() should have ENERGY / TEMPERATURE dimension, got {:?}",
                dimension
            );
            assert!(
                (si_value - 1.380649e-23).abs() < 1e-35,
                "BOLTZMANN_CONSTANT() si_value: expected 1.380649e-23, got {:.6e}",
                si_value
            );
        }
        other => panic!(
            "BOLTZMANN_CONSTANT() should return Value::Scalar, got {:?}",
            other
        ),
    }
}
