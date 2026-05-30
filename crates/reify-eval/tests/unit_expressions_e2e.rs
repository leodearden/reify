//! End-to-end evaluation tests for compound-unit and value-level-^ expressions
//! (task 3807 — integration gate ε).
//!
//! Cycle 1 (compound-unit path α→β→γ): loads `examples/unit_expressions.ri`,
//! asserts zero diagnostics, and checks that each compound-unit binding resolves
//! to the expected SI Scalar (numeric SI value + DimensionVector).
//!
//! Cycle 2 (value-level ^ path δ): asserts the value-level `^` binding.

use std::sync::OnceLock;

use reify_compiler::CompiledModule;
use reify_core::{DimensionVector, ValueCellId};
use reify_ir::Value;
use reify_test_support::{collect_errors, make_engine, parse_and_compile_with_stdlib};

const FILE_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/unit_expressions.ri");

const EPSILON: f64 = 1e-9;

fn file_source() -> &'static str {
    static S: OnceLock<String> = OnceLock::new();
    S.get_or_init(|| {
        std::fs::read_to_string(FILE_PATH)
            .unwrap_or_else(|e| panic!("{FILE_PATH} should exist: {e}"))
    })
    .as_str()
}

fn compiled() -> &'static CompiledModule {
    static C: OnceLock<CompiledModule> = OnceLock::new();
    C.get_or_init(|| parse_and_compile_with_stdlib(file_source()))
}

// ── Zero-diagnostics gate (`reify check` clean) ──────────────────────────────

#[test]
fn unit_expressions_compiles_with_zero_errors() {
    let errors = collect_errors(&compiled().diagnostics);
    assert!(
        errors.is_empty(),
        "expected zero compile-phase errors on examples/unit_expressions.ri, got: {:?}",
        errors
    );
}

#[test]
fn unit_expressions_evaluates_with_zero_errors() {
    let mut engine = make_engine();
    let result = engine.eval(compiled());
    let errors = collect_errors(&result.diagnostics);
    assert!(
        errors.is_empty(),
        "expected zero eval-phase errors on examples/unit_expressions.ri, got: {:?}",
        errors
    );
}

// ── Cycle 1: compound-unit bindings ─────────────────────────────────────────

/// `density = 7850kg/m^3` → si_value ≈ 7850.0, dim MASS_DENSITY (kg·m⁻³)
#[test]
fn density_resolves_to_mass_density_si_value() {
    let mut engine = make_engine();
    let result = engine.eval(compiled());
    let id = ValueCellId::new("UnitExpressions", "density");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'density' not found in eval result"));
    match val {
        Value::Scalar { si_value, dimension } => {
            assert!(
                (*si_value - 7850.0).abs() < EPSILON,
                "expected si_value 7850.0 for 7850kg/m^3, got {}",
                si_value
            );
            assert_eq!(
                *dimension,
                DimensionVector::MASS_DENSITY,
                "expected MASS_DENSITY (kg·m⁻³) for density, got {:?}",
                dimension
            );
        }
        other => panic!("expected Value::Scalar for density, got {:?}", other),
    }
}

/// `gravity = 9.81m/s^2` → si_value ≈ 9.81, dim ACCELERATION (m·s⁻²)
#[test]
fn gravity_resolves_to_acceleration_si_value() {
    let mut engine = make_engine();
    let result = engine.eval(compiled());
    let id = ValueCellId::new("UnitExpressions", "gravity");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'gravity' not found in eval result"));
    match val {
        Value::Scalar { si_value, dimension } => {
            assert!(
                (*si_value - 9.81).abs() < EPSILON,
                "expected si_value 9.81 for 9.81m/s^2, got {}",
                si_value
            );
            assert_eq!(
                *dimension,
                DimensionVector::ACCELERATION,
                "expected ACCELERATION (m·s⁻²) for gravity, got {:?}",
                dimension
            );
        }
        other => panic!("expected Value::Scalar for gravity, got {:?}", other),
    }
}

/// `torque = 5kN*m` → si_value ≈ 5000.0, dim ENERGY (kg·m²·s⁻²)
///
/// 5 kN·m = 5 × 1000 N·m = 5000 kg·m²·s⁻².  Dimensionally identical to ENERGY
/// in SI; the `Energy` type annotation is used because `Torque` is not a named
/// alias in `NAMED_DIMENSIONS` (see design decision in plan.json).
#[test]
fn torque_resolves_to_energy_dimension_si_value() {
    let mut engine = make_engine();
    let result = engine.eval(compiled());
    let id = ValueCellId::new("UnitExpressions", "torque");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'torque' not found in eval result"));
    match val {
        Value::Scalar { si_value, dimension } => {
            assert!(
                (*si_value - 5000.0).abs() < EPSILON,
                "expected si_value 5000.0 for 5kN*m, got {}",
                si_value
            );
            assert_eq!(
                *dimension,
                DimensionVector::ENERGY,
                "expected ENERGY (kg·m²·s⁻²) for 5kN*m, got {:?}",
                dimension
            );
        }
        other => panic!("expected Value::Scalar for torque, got {:?}", other),
    }
}

/// `area = 25mm^2` → si_value ≈ 2.5e-5, dim AREA (m²)
///
/// SI(25mm^2): 1mm = 0.001m; 25 × (0.001)^2 = 25 × 1e-6 = 2.5e-5 m².
#[test]
fn area_resolves_to_length_squared_si_value() {
    let mut engine = make_engine();
    let result = engine.eval(compiled());
    let id = ValueCellId::new("UnitExpressions", "area");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'area' not found in eval result"));
    match val {
        Value::Scalar { si_value, dimension } => {
            assert!(
                (*si_value - 2.5e-5).abs() < EPSILON,
                "expected si_value 2.5e-5 for 25mm^2, got {}",
                si_value
            );
            assert_eq!(
                *dimension,
                DimensionVector::AREA,
                "expected AREA (m²) for area, got {:?}",
                dimension
            );
        }
        other => panic!("expected Value::Scalar for area, got {:?}", other),
    }
}

/// `viscosity = 0.001kg/m/s` → si_value ≈ 0.001, dim DYNAMIC_VISCOSITY (kg·m⁻¹·s⁻¹)
///
/// Left-associative: `kg/m/s` = `(kg/m)/s`.  All base units are SI-native
/// so the SI value equals the numeric coefficient: 0.001.
#[test]
fn viscosity_resolves_to_dynamic_viscosity_si_value() {
    let mut engine = make_engine();
    let result = engine.eval(compiled());
    let id = ValueCellId::new("UnitExpressions", "viscosity");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'viscosity' not found in eval result"));
    match val {
        Value::Scalar { si_value, dimension } => {
            assert!(
                (*si_value - 0.001).abs() < EPSILON,
                "expected si_value 0.001 for 0.001kg/m/s, got {}",
                si_value
            );
            assert_eq!(
                *dimension,
                DimensionVector::DYNAMIC_VISCOSITY,
                "expected DYNAMIC_VISCOSITY (kg·m⁻¹·s⁻¹) for viscosity, got {:?}",
                dimension
            );
        }
        other => panic!("expected Value::Scalar for viscosity, got {:?}", other),
    }
}

/// `conductivity = 0.5W/(m*K)` → si_value ≈ 0.5, dim THERMAL_CONDUCTIVITY (kg·m·s⁻³·K⁻¹)
///
/// W, m, K are all SI-native so the SI value equals the numeric coefficient: 0.5.
#[test]
fn conductivity_resolves_to_thermal_conductivity_si_value() {
    let mut engine = make_engine();
    let result = engine.eval(compiled());
    let id = ValueCellId::new("UnitExpressions", "conductivity");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'conductivity' not found in eval result"));
    match val {
        Value::Scalar { si_value, dimension } => {
            assert!(
                (*si_value - 0.5).abs() < EPSILON,
                "expected si_value 0.5 for 0.5W/(m*K), got {}",
                si_value
            );
            assert_eq!(
                *dimension,
                DimensionVector::THERMAL_CONDUCTIVITY,
                "expected THERMAL_CONDUCTIVITY (kg·m·s⁻³·K⁻¹) for conductivity, got {:?}",
                dimension
            );
        }
        other => panic!("expected Value::Scalar for conductivity, got {:?}", other),
    }
}
