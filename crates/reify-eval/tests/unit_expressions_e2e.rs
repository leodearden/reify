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
use reify_eval::EvalResult;
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

/// Parse and compile (with stdlib) the example file, caching the result.
///
/// `parse_and_compile_with_stdlib` panics with "compile errors: …" if there are
/// any error-severity diagnostics, so the compile-gate is enforced by this
/// shared initialiser — no additional zero-errors test is needed at the compile
/// phase.  The eval-phase gate (`unit_expressions_evaluates_with_zero_errors`)
/// asserts separately that no *evaluation* errors arise.
fn compiled() -> &'static CompiledModule {
    static C: OnceLock<CompiledModule> = OnceLock::new();
    C.get_or_init(|| parse_and_compile_with_stdlib(file_source()))
}

/// Evaluate the example file with the default engine, caching the result.
///
/// Shares the memoised `compiled()` result so the full parse→compile→eval
/// pipeline runs at most once across the entire test binary.
fn eval_result() -> &'static EvalResult {
    static E: OnceLock<EvalResult> = OnceLock::new();
    E.get_or_init(|| {
        let mut engine = make_engine();
        engine.eval(compiled())
    })
}

// Compile-time guard: OnceLock<T> requires T: Send + Sync.
// If a future refactor adds a non-Send field to EvalResult this function —
// rather than the OnceLock static above — produces the compiler error.
fn _assert_send_sync() {
    fn _assert<T: Send + Sync>() {}
    _assert::<EvalResult>();
}

// ── Shared assertion helpers ──────────────────────────────────────────────────

/// Assert that the named `UnitExpressions` binding evaluates to a
/// `Value::Scalar` with the given SI value (within [`EPSILON`]) and exact
/// `DimensionVector`.
fn assert_scalar(member: &str, expected_si: f64, expected_dim: DimensionVector) {
    let id = ValueCellId::new("UnitExpressions", member);
    let val = eval_result()
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'{}' not found in eval result", member));
    match val {
        Value::Scalar { si_value, dimension } => {
            assert!(
                (*si_value - expected_si).abs() < EPSILON,
                "'{}': expected si_value {}, got {}",
                member,
                expected_si,
                si_value
            );
            assert_eq!(
                *dimension, expected_dim,
                "'{}': expected dimension {:?}, got {:?}",
                member, expected_dim, dimension
            );
        }
        other => panic!("expected Value::Scalar for '{}', got {:?}", member, other),
    }
}

// ── Zero-diagnostics gate (`reify check` clean) ──────────────────────────────

/// Assert that eval-phase produces zero error-severity diagnostics.
///
/// Note: compile-phase errors are already caught by `compiled()` (which calls
/// `parse_and_compile_with_stdlib` and panics on errors), so a separate
/// compile-phase test would be dead.  This test guards the eval phase only.
#[test]
fn unit_expressions_evaluates_with_zero_errors() {
    let errors = collect_errors(&eval_result().diagnostics);
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
    assert_scalar("density", 7850.0, DimensionVector::MASS_DENSITY);
}

/// `gravity = 9.81m/s^2` → si_value ≈ 9.81, dim ACCELERATION (m·s⁻²)
#[test]
fn gravity_resolves_to_acceleration_si_value() {
    assert_scalar("gravity", 9.81, DimensionVector::ACCELERATION);
}

/// `torque = 5kN*m` → si_value ≈ 5000.0, dim ENERGY (kg·m²·s⁻²)
///
/// 5 kN·m = 5 × 1000 N·m = 5000 kg·m²·s⁻².  Dimensionally identical to ENERGY
/// in SI; the `Energy` type annotation is used because `Torque` is not a named
/// alias in `NAMED_DIMENSIONS` (see design decision in plan.json).
#[test]
fn torque_resolves_to_energy_dimension_si_value() {
    assert_scalar("torque", 5000.0, DimensionVector::ENERGY);
}

/// `area = 25mm^2` → si_value ≈ 2.5e-5, dim AREA (m²)
///
/// SI(25mm^2): 1mm = 0.001m; 25 × (0.001)^2 = 25 × 1e-6 = 2.5e-5 m².
#[test]
fn area_resolves_to_length_squared_si_value() {
    assert_scalar("area", 2.5e-5, DimensionVector::AREA);
}

/// `viscosity = 0.001kg/m/s` → si_value ≈ 0.001, dim DYNAMIC_VISCOSITY (kg·m⁻¹·s⁻¹)
///
/// Left-associative: `kg/m/s` = `(kg/m)/s`.  All base units are SI-native
/// so the SI value equals the numeric coefficient: 0.001.
#[test]
fn viscosity_resolves_to_dynamic_viscosity_si_value() {
    assert_scalar("viscosity", 0.001, DimensionVector::DYNAMIC_VISCOSITY);
}

/// `conductivity = 0.5W/(m*K)` → si_value ≈ 0.5, dim THERMAL_CONDUCTIVITY (kg·m·s⁻³·K⁻¹)
///
/// W, m, K are all SI-native so the SI value equals the numeric coefficient: 0.5.
#[test]
fn conductivity_resolves_to_thermal_conductivity_si_value() {
    assert_scalar("conductivity", 0.5, DimensionVector::THERMAL_CONDUCTIVITY);
}

// ── Cycle 2: value-level ^ binding (exercises δ end-to-end) ─────────────────

/// `stress_sq = (5mm ^ 2) / (1mm ^ 2)` → `Value::Scalar { si_value ≈ 25.0, dimension: DIMENSIONLESS }`
///
/// (5mm)² / (1mm)² = 25mm² / 1mm² = 25 (clean dimension cancellation).
///
/// The eval engine's arithmetic path produces `Value::Scalar` with a zero-vector
/// dimension for dimensionless results — it does NOT collapse to `Value::Real`.
/// (`from_real_scalar` in `reify-ir/value.rs` would fold a DIMENSIONLESS scalar
/// to `Value::Real`, but it is a construction helper; the eval arithmetic path
/// directly emits `Value::Scalar` and leaves the folding to callers that opt in.)
/// Asserting `Value::Scalar` with `dimension.is_dimensionless()` pins this
/// representation against a silent flip to `Value::Real`.
#[test]
fn stress_sq_evaluates_to_dimensionless_25() {
    let id = ValueCellId::new("UnitExpressions", "stress_sq");
    let val = eval_result()
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'stress_sq' not found in eval result"));
    match val {
        Value::Scalar { si_value, dimension } => {
            assert!(
                dimension.is_dimensionless(),
                "expected dimensionless dimension for stress_sq, got {:?}",
                dimension
            );
            assert!(
                (*si_value - 25.0).abs() < EPSILON,
                "expected si_value ≈ 25.0 for (5mm^2)/(1mm^2), got {}",
                si_value
            );
        }
        other => panic!(
            "expected Value::Scalar {{ si_value ≈ 25.0, dimension: DIMENSIONLESS }} for stress_sq, got {:?}",
            other
        ),
    }
}
