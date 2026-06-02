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

// ─── Test 5: AVOGADRO_CONSTANT present and has correct signature ──────────────

/// `AVOGADRO_CONSTANT` must be present in `std/units`, be `pub`, take no
/// parameters, and return `Scalar<DIMENSIONLESS / AMOUNT_OF_SUBSTANCE>`.
///
/// N_A = 6.02214076×10²³ mol⁻¹ exactly — 2019 SI redefinition (CGPM 26th meeting).
#[test]
fn avogadro_constant_function_present_in_std_units() {
    let module = common::units_module();

    let func = module
        .functions
        .iter()
        .find(|f| f.name == "AVOGADRO_CONSTANT")
        .unwrap_or_else(|| {
            panic!(
                "AVOGADRO_CONSTANT not found in std/units; found functions: {:?}",
                module.functions.iter().map(|f| &f.name).collect::<Vec<_>>()
            )
        });

    assert!(func.is_pub, "AVOGADRO_CONSTANT should be pub");
    assert!(
        func.params.is_empty(),
        "AVOGADRO_CONSTANT should take no params, got: {:?}",
        func.params
    );

    let expected_dim =
        DimensionVector::DIMENSIONLESS.div(&DimensionVector::AMOUNT_OF_SUBSTANCE);
    assert_eq!(
        func.return_type,
        Type::Scalar {
            dimension: expected_dim
        },
        "AVOGADRO_CONSTANT return type should be Scalar<DIMENSIONLESS / AMOUNT_OF_SUBSTANCE>, got {:?}",
        func.return_type
    );
}

// ─── Test 6: AVOGADRO_CONSTANT evaluates to 6.02214076e23 mol⁻¹ ──────────────

/// Evaluating `AVOGADRO_CONSTANT()` via `eval_expr` must yield a
/// `Value::Scalar` with `si_value ≈ 6.02214076e23` and
/// `dimension = DIMENSIONLESS / AMOUNT_OF_SUBSTANCE`.
///
/// N_A = 6.02214076×10²³ mol⁻¹ exactly (2019 SI redefinition).
#[test]
fn avogadro_constant_evaluates_to_6p02214076e23_si_with_inverse_amount_dimension() {
    let module = common::units_module();

    let expected_dim =
        DimensionVector::DIMENSIONLESS.div(&DimensionVector::AMOUNT_OF_SUBSTANCE);
    let call_expr = CompiledExpr::user_function_call(
        "AVOGADRO_CONSTANT".to_string(),
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
                DimensionVector::DIMENSIONLESS.div(&DimensionVector::AMOUNT_OF_SUBSTANCE),
                "AVOGADRO_CONSTANT() should have DIMENSIONLESS / AMOUNT_OF_SUBSTANCE dimension, got {:?}",
                dimension
            );
            common::assert_eq_rel(
                si_value,
                6.02214076e23,
                1e-12,
                "AVOGADRO_CONSTANT() si_value",
            );
        }
        other => panic!(
            "AVOGADRO_CONSTANT() should return Value::Scalar, got {:?}",
            other
        ),
    }
}

// ─── Test 7: PLANCK_CONSTANT present and has correct signature ────────────────

/// `PLANCK_CONSTANT` must be present in `std/units`, be `pub`, take no
/// parameters, and return `Scalar<ENERGY * TIME>` (= kg·m²·s⁻¹).
///
/// h = 6.62607015×10⁻³⁴ J·s exactly — 2019 SI redefinition (CGPM 26th meeting).
#[test]
fn planck_constant_function_present_in_std_units() {
    let module = common::units_module();

    let func = module
        .functions
        .iter()
        .find(|f| f.name == "PLANCK_CONSTANT")
        .unwrap_or_else(|| {
            panic!(
                "PLANCK_CONSTANT not found in std/units; found functions: {:?}",
                module.functions.iter().map(|f| &f.name).collect::<Vec<_>>()
            )
        });

    assert!(func.is_pub, "PLANCK_CONSTANT should be pub");
    assert!(
        func.params.is_empty(),
        "PLANCK_CONSTANT should take no params, got: {:?}",
        func.params
    );

    let expected_dim = DimensionVector::ENERGY.mul(&DimensionVector::TIME);
    assert_eq!(
        func.return_type,
        Type::Scalar {
            dimension: expected_dim
        },
        "PLANCK_CONSTANT return type should be Scalar<ENERGY * TIME>, got {:?}",
        func.return_type
    );
}

// ─── Test 8: PLANCK_CONSTANT evaluates to 6.62607015e-34 J·s ─────────────────

/// Evaluating `PLANCK_CONSTANT()` via `eval_expr` must yield a
/// `Value::Scalar` with `si_value ≈ 6.62607015e-34` and
/// `dimension = ENERGY * TIME` (kg·m²·s⁻¹).
///
/// h = 6.62607015×10⁻³⁴ J·s exactly (2019 SI redefinition).
#[test]
fn planck_constant_evaluates_to_6p62607015e_minus_34_si_with_action_dimension() {
    let module = common::units_module();

    let expected_dim = DimensionVector::ENERGY.mul(&DimensionVector::TIME);
    let call_expr = CompiledExpr::user_function_call(
        "PLANCK_CONSTANT".to_string(),
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
                DimensionVector::ENERGY.mul(&DimensionVector::TIME),
                "PLANCK_CONSTANT() should have ENERGY * TIME dimension, got {:?}",
                dimension
            );
            common::assert_eq_rel(
                si_value,
                6.62607015e-34,
                1e-12,
                "PLANCK_CONSTANT() si_value",
            );
        }
        other => panic!(
            "PLANCK_CONSTANT() should return Value::Scalar, got {:?}",
            other
        ),
    }
}

// ─── Test 9: STEFAN_BOLTZMANN_CONSTANT present and has correct signature ──────

/// `STEFAN_BOLTZMANN_CONSTANT` must be present in `std/units`, be `pub`, take no
/// parameters, and return `Scalar<POWER / AREA / T / T / T / T>` (= kg·s⁻³·K⁻⁴).
///
/// σ = 5.670374419×10⁻⁸ W·m⁻²·K⁻⁴ — CODATA 2018 value.
#[test]
fn stefan_boltzmann_constant_function_present_in_std_units() {
    let module = common::units_module();

    let func = module
        .functions
        .iter()
        .find(|f| f.name == "STEFAN_BOLTZMANN_CONSTANT")
        .unwrap_or_else(|| {
            panic!(
                "STEFAN_BOLTZMANN_CONSTANT not found in std/units; found functions: {:?}",
                module.functions.iter().map(|f| &f.name).collect::<Vec<_>>()
            )
        });

    assert!(func.is_pub, "STEFAN_BOLTZMANN_CONSTANT should be pub");
    assert!(
        func.params.is_empty(),
        "STEFAN_BOLTZMANN_CONSTANT should take no params, got: {:?}",
        func.params
    );

    let expected_dim = DimensionVector::POWER
        .div(&DimensionVector::AREA)
        .div(&DimensionVector::TEMPERATURE)
        .div(&DimensionVector::TEMPERATURE)
        .div(&DimensionVector::TEMPERATURE)
        .div(&DimensionVector::TEMPERATURE);
    assert_eq!(
        func.return_type,
        Type::Scalar {
            dimension: expected_dim
        },
        "STEFAN_BOLTZMANN_CONSTANT return type should be Scalar<POWER/AREA/T^4>, got {:?}",
        func.return_type
    );
}

// ─── Test 10: STEFAN_BOLTZMANN_CONSTANT evaluates to 5.670374419e-8 ───────────

/// Evaluating `STEFAN_BOLTZMANN_CONSTANT()` via `eval_expr` must yield a
/// `Value::Scalar` with `si_value ≈ 5.670374419e-8` and
/// `dimension = POWER / AREA / T^4` (kg·s⁻³·K⁻⁴).
///
/// σ = 5.670374419×10⁻⁸ W·m⁻²·K⁻⁴ — CODATA 2018.
#[test]
fn stefan_boltzmann_constant_evaluates_to_5p670374419e_minus_8_si_with_stefan_boltzmann_dim() {
    let module = common::units_module();

    let expected_dim = DimensionVector::POWER
        .div(&DimensionVector::AREA)
        .div(&DimensionVector::TEMPERATURE)
        .div(&DimensionVector::TEMPERATURE)
        .div(&DimensionVector::TEMPERATURE)
        .div(&DimensionVector::TEMPERATURE);
    let call_expr = CompiledExpr::user_function_call(
        "STEFAN_BOLTZMANN_CONSTANT".to_string(),
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
                DimensionVector::POWER
                    .div(&DimensionVector::AREA)
                    .div(&DimensionVector::TEMPERATURE)
                    .div(&DimensionVector::TEMPERATURE)
                    .div(&DimensionVector::TEMPERATURE)
                    .div(&DimensionVector::TEMPERATURE),
                "STEFAN_BOLTZMANN_CONSTANT() should have POWER/AREA/T^4 dimension, got {:?}",
                dimension
            );
            common::assert_eq_rel(
                si_value,
                5.670374419e-8,
                1e-12,
                "STEFAN_BOLTZMANN_CONSTANT() si_value",
            );
        }
        other => panic!(
            "STEFAN_BOLTZMANN_CONSTANT() should return Value::Scalar, got {:?}",
            other
        ),
    }
}

// ─── Test 11: VACUUM_PERMITTIVITY present and has correct signature ────────────

/// `VACUUM_PERMITTIVITY` must be present in `std/units`, be `pub`, take no
/// parameters, and return `Scalar<CAPACITANCE / LENGTH>` (= kg⁻¹·m⁻³·s⁴·A²).
///
/// ε₀ = 8.8541878128×10⁻¹² F/m — CODATA 2018 value.
#[test]
fn vacuum_permittivity_function_present_in_std_units() {
    let module = common::units_module();

    let func = module
        .functions
        .iter()
        .find(|f| f.name == "VACUUM_PERMITTIVITY")
        .unwrap_or_else(|| {
            panic!(
                "VACUUM_PERMITTIVITY not found in std/units; found functions: {:?}",
                module.functions.iter().map(|f| &f.name).collect::<Vec<_>>()
            )
        });

    assert!(func.is_pub, "VACUUM_PERMITTIVITY should be pub");
    assert!(
        func.params.is_empty(),
        "VACUUM_PERMITTIVITY should take no params, got: {:?}",
        func.params
    );

    let expected_dim = DimensionVector::CAPACITANCE.div(&DimensionVector::LENGTH);
    assert_eq!(
        func.return_type,
        Type::Scalar {
            dimension: expected_dim
        },
        "VACUUM_PERMITTIVITY return type should be Scalar<CAPACITANCE / LENGTH>, got {:?}",
        func.return_type
    );
}

// ─── Test 12: VACUUM_PERMITTIVITY evaluates to 8.8541878128e-12 ───────────────

/// Evaluating `VACUUM_PERMITTIVITY()` via `eval_expr` must yield a
/// `Value::Scalar` with `si_value ≈ 8.8541878128e-12` and
/// `dimension = CAPACITANCE / LENGTH` (kg⁻¹·m⁻³·s⁴·A²).
///
/// ε₀ = 8.8541878128×10⁻¹² F/m — CODATA 2018 value.
#[test]
fn vacuum_permittivity_evaluates_to_8p8541878128e_minus_12_si_with_permittivity_dim() {
    let module = common::units_module();

    let expected_dim = DimensionVector::CAPACITANCE.div(&DimensionVector::LENGTH);
    let call_expr = CompiledExpr::user_function_call(
        "VACUUM_PERMITTIVITY".to_string(),
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
                DimensionVector::CAPACITANCE.div(&DimensionVector::LENGTH),
                "VACUUM_PERMITTIVITY() should have CAPACITANCE / LENGTH dimension, got {:?}",
                dimension
            );
            common::assert_eq_rel(
                si_value,
                8.8541878128e-12,
                1e-12,
                "VACUUM_PERMITTIVITY() si_value",
            );
        }
        other => panic!(
            "VACUUM_PERMITTIVITY() should return Value::Scalar, got {:?}",
            other
        ),
    }
}

// ─── Test 13: VACUUM_PERMEABILITY present and has correct signature ────────────

/// `VACUUM_PERMEABILITY` must be present in `std/units`, be `pub`, take no
/// parameters, and return `Scalar<INDUCTANCE / LENGTH>` (= kg·m·s⁻²·A⁻²).
///
/// μ₀ = 1.25663706212×10⁻⁶ H/m — CODATA 2018 value.
#[test]
fn vacuum_permeability_function_present_in_std_units() {
    let module = common::units_module();

    let func = module
        .functions
        .iter()
        .find(|f| f.name == "VACUUM_PERMEABILITY")
        .unwrap_or_else(|| {
            panic!(
                "VACUUM_PERMEABILITY not found in std/units; found functions: {:?}",
                module.functions.iter().map(|f| &f.name).collect::<Vec<_>>()
            )
        });

    assert!(func.is_pub, "VACUUM_PERMEABILITY should be pub");
    assert!(
        func.params.is_empty(),
        "VACUUM_PERMEABILITY should take no params, got: {:?}",
        func.params
    );

    let expected_dim = DimensionVector::INDUCTANCE.div(&DimensionVector::LENGTH);
    assert_eq!(
        func.return_type,
        Type::Scalar {
            dimension: expected_dim
        },
        "VACUUM_PERMEABILITY return type should be Scalar<INDUCTANCE / LENGTH>, got {:?}",
        func.return_type
    );
}

// ─── Test 14: VACUUM_PERMEABILITY evaluates to 1.25663706212e-6 ───────────────

/// Evaluating `VACUUM_PERMEABILITY()` via `eval_expr` must yield a
/// `Value::Scalar` with `si_value ≈ 1.25663706212e-6` and
/// `dimension = INDUCTANCE / LENGTH` (kg·m·s⁻²·A⁻²).
///
/// μ₀ = 1.25663706212×10⁻⁶ H/m — CODATA 2018 value.
#[test]
fn vacuum_permeability_evaluates_to_1p25663706212e_minus_6_si_with_permeability_dim() {
    let module = common::units_module();

    let expected_dim = DimensionVector::INDUCTANCE.div(&DimensionVector::LENGTH);
    let call_expr = CompiledExpr::user_function_call(
        "VACUUM_PERMEABILITY".to_string(),
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
                DimensionVector::INDUCTANCE.div(&DimensionVector::LENGTH),
                "VACUUM_PERMEABILITY() should have INDUCTANCE / LENGTH dimension, got {:?}",
                dimension
            );
            common::assert_eq_rel(
                si_value,
                1.25663706212e-6,
                1e-12,
                "VACUUM_PERMEABILITY() si_value",
            );
        }
        other => panic!(
            "VACUUM_PERMEABILITY() should return Value::Scalar, got {:?}",
            other
        ),
    }
}
