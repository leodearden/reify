//! Stress tests for dimensional type system via dimensional_chains.ri fixture.
//!
//! Covers:
//!   - smoke test: fixture parses, compiles, evaluates without errors
//!   - 10-step chain assertions: correct dimension at each chain step
//!   - sqrt round-trip assertions: sqrt(AREA)→LENGTH, sqrt(L^4)→AREA, etc.
//!   - engineering formula dimensions: Reynolds, MOI, pressure, power, PE, spring energy

use std::fs;

use reify_core::{DimensionVector, ModulePath, Severity, ValueCellId};
use reify_ir::Value;
use reify_test_support::mocks::MockConstraintChecker;

// ── Helper ────────────────────────────────────────────────────────────────────

/// Load a .ri file, parse, compile (asserting no errors), and evaluate.
/// Returns the full EvalResult for per-test assertions.
fn eval_ri_file(path: &str, module_name: &str) -> reify_eval::EvalResult {
    let source =
        fs::read_to_string(path).unwrap_or_else(|e| panic!("{} should exist: {}", path, e));
    let parsed = reify_syntax::parse(&source, ModulePath::single(module_name));
    assert!(
        parsed.errors.is_empty(),
        "parse errors in {}: {:?}",
        path,
        parsed.errors
    );
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "compile errors in {}: {:?}",
        path,
        errors
    );
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);
    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        eval_errors.is_empty(),
        "eval errors in {}: {:?}",
        path,
        eval_errors
    );
    result
}

// ── step-1: smoke test ────────────────────────────────────────────────────────

/// Load dimensional_chains.ri, parse, compile, eval — no errors, non-empty values.
#[test]
fn dimensional_chains_parses_and_compiles() {
    let result = eval_ri_file("../../examples/dimensional_chains.ri", "dimensional_chains");
    assert!(
        !result.values.is_empty(),
        "eval should produce non-empty values for dimensional_chains.ri"
    );
}

// ── step-3: 10-step chain dimension assertions ────────────────────────────────

/// Step 1: chain_L1 = 3.0 * 1m → Scalar(3.0, LENGTH)
#[test]
fn chain_step1_length() {
    let result = eval_ri_file("../../examples/dimensional_chains.ri", "dimensional_chains");
    let id = ValueCellId::new("DimensionalChains", "chain_L1");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'chain_L1' not found"));
    match val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 3.0).abs() < 1e-9,
                "chain_L1 should be ≈3.0 m, got {}",
                si_value
            );
            assert_eq!(
                *dimension,
                DimensionVector::LENGTH,
                "chain_L1 dimension should be LENGTH"
            );
        }
        other => panic!("chain_L1 should be Scalar, got {:?}", other),
    }
}

/// Step 2: chain_A = chain_L1 * chain_L1 → Scalar(9.0, AREA)
#[test]
fn chain_step2_area() {
    let result = eval_ri_file("../../examples/dimensional_chains.ri", "dimensional_chains");
    let id = ValueCellId::new("DimensionalChains", "chain_A");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'chain_A' not found"));
    match val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 9.0).abs() < 1e-9,
                "chain_A should be ≈9.0 m², got {}",
                si_value
            );
            assert_eq!(
                *dimension,
                DimensionVector::AREA,
                "chain_A dimension should be AREA"
            );
        }
        other => panic!("chain_A should be Scalar, got {:?}", other),
    }
}

/// Step 3: chain_V = chain_A * chain_L1 → Scalar(27.0, VOLUME)
#[test]
fn chain_step3_volume() {
    let result = eval_ri_file("../../examples/dimensional_chains.ri", "dimensional_chains");
    let id = ValueCellId::new("DimensionalChains", "chain_V");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'chain_V' not found"));
    match val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 27.0).abs() < 1e-9,
                "chain_V should be ≈27.0 m³, got {}",
                si_value
            );
            assert_eq!(
                *dimension,
                DimensionVector::VOLUME,
                "chain_V dimension should be VOLUME"
            );
        }
        other => panic!("chain_V should be Scalar, got {:?}", other),
    }
}

/// Step 4: chain_L2 = sqrt(chain_A) = sqrt(9.0 m²) → Scalar(3.0, LENGTH)
#[test]
fn chain_step4_sqrt_length() {
    let result = eval_ri_file("../../examples/dimensional_chains.ri", "dimensional_chains");
    let id = ValueCellId::new("DimensionalChains", "chain_L2");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'chain_L2' not found"));
    match val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 3.0).abs() < 1e-9,
                "chain_L2 should be ≈3.0 m, got {}",
                si_value
            );
            assert_eq!(
                *dimension,
                DimensionVector::LENGTH,
                "chain_L2 = sqrt(AREA) dimension should be LENGTH"
            );
        }
        other => panic!("chain_L2 should be Scalar, got {:?}", other),
    }
}

/// Step 6: chain_vel = chain_L1 / chain_T1 → Scalar(1.5, LENGTH/TIME)
#[test]
fn chain_step6_velocity() {
    let result = eval_ri_file("../../examples/dimensional_chains.ri", "dimensional_chains");
    let id = ValueCellId::new("DimensionalChains", "chain_vel");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'chain_vel' not found"));
    let expected_dim = DimensionVector::LENGTH.div(&DimensionVector::TIME);
    match val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 1.5).abs() < 1e-9,
                "chain_vel should be ≈1.5 m/s, got {}",
                si_value
            );
            assert_eq!(
                *dimension, expected_dim,
                "chain_vel dimension should be LENGTH/TIME"
            );
        }
        other => panic!("chain_vel should be Scalar, got {:?}", other),
    }
}

/// Step 8: chain_F = 1kg * chain_acc → Scalar(0.75, FORCE)
#[test]
fn chain_step8_force() {
    let result = eval_ri_file("../../examples/dimensional_chains.ri", "dimensional_chains");
    let id = ValueCellId::new("DimensionalChains", "chain_F");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'chain_F' not found"));
    match val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 0.75).abs() < 1e-9,
                "chain_F should be ≈0.75 N, got {}",
                si_value
            );
            assert_eq!(
                *dimension,
                reify_core::dimension::FORCE,
                "chain_F dimension should be FORCE"
            );
        }
        other => panic!("chain_F should be Scalar, got {:?}", other),
    }
}

/// Step 9: chain_E = chain_F * chain_L1 → Scalar(2.25, FORCE*LENGTH = ENERGY)
#[test]
fn chain_step9_energy() {
    let result = eval_ri_file("../../examples/dimensional_chains.ri", "dimensional_chains");
    let id = ValueCellId::new("DimensionalChains", "chain_E");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'chain_E' not found"));
    let expected_dim = reify_core::dimension::FORCE.mul(&DimensionVector::LENGTH);
    match val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 2.25).abs() < 1e-9,
                "chain_E should be ≈2.25 J, got {}",
                si_value
            );
            assert_eq!(
                *dimension, expected_dim,
                "chain_E dimension should be FORCE*LENGTH (energy)"
            );
        }
        other => panic!("chain_E should be Scalar, got {:?}", other),
    }
}

/// Step 10: chain_L3 = chain_E / chain_F → Scalar(3.0, LENGTH) — round-trip back to LENGTH
#[test]
fn chain_step10_length_roundtrip() {
    let result = eval_ri_file("../../examples/dimensional_chains.ri", "dimensional_chains");
    let id = ValueCellId::new("DimensionalChains", "chain_L3");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'chain_L3' not found"));
    match val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 3.0).abs() < 1e-9,
                "chain_L3 should be ≈3.0 m (round-trip), got {}",
                si_value
            );
            assert_eq!(
                *dimension,
                DimensionVector::LENGTH,
                "chain_L3 = ENERGY/FORCE round-trip should be LENGTH"
            );
        }
        other => panic!("chain_L3 should be Scalar, got {:?}", other),
    }
}

// ── step-5: sqrt round-trip assertions ───────────────────────────────────────

/// sqrt_wh = sqrt(4m * 9m) = sqrt(36 m²) = 6.0 m → Scalar(6.0, LENGTH)
#[test]
fn sqrt_wh_is_length() {
    let result = eval_ri_file("../../examples/dimensional_chains.ri", "dimensional_chains");
    let id = ValueCellId::new("DimensionalChains", "sqrt_wh");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'sqrt_wh' not found"));
    match val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 6.0).abs() < 1e-9,
                "sqrt_wh should be ≈6.0 m, got {}",
                si_value
            );
            assert_eq!(
                *dimension,
                DimensionVector::LENGTH,
                "sqrt(AREA) should have LENGTH dimension"
            );
        }
        other => panic!("sqrt_wh should be Scalar, got {:?}", other),
    }
}

/// sqrt_V_L = sqrt(VOLUME * LENGTH) = sqrt(L^4) → Scalar(9.0, AREA)
/// chain_V = 27.0 m³, chain_L1 = 3.0 m → product = 81.0 m⁴ → sqrt = 9.0 m²
#[test]
fn sqrt_volume_times_length_is_area() {
    let result = eval_ri_file("../../examples/dimensional_chains.ri", "dimensional_chains");
    let id = ValueCellId::new("DimensionalChains", "sqrt_V_L");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'sqrt_V_L' not found"));
    match val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 9.0).abs() < 1e-9,
                "sqrt_V_L should be ≈9.0 m², got {}",
                si_value
            );
            assert_eq!(
                *dimension,
                DimensionVector::AREA,
                "sqrt(VOLUME*LENGTH) = sqrt(L^4) should have AREA dimension"
            );
        }
        other => panic!("sqrt_V_L should be Scalar, got {:?}", other),
    }
}

/// sqrt(sqrt(L^4)) = sqrt(AREA) = LENGTH
/// sqrt_sqrt_L4 = sqrt(sqrt(9m² * 9m²)) = sqrt(sqrt(81 m⁴)) = sqrt(9 m²) = 3.0 m
#[test]
fn sqrt_sqrt_l4_is_length() {
    let result = eval_ri_file("../../examples/dimensional_chains.ri", "dimensional_chains");
    let id = ValueCellId::new("DimensionalChains", "sqrt_sqrt_L4");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'sqrt_sqrt_L4' not found"));
    match val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 3.0).abs() < 1e-9,
                "sqrt_sqrt_L4 should be ≈3.0 m, got {}",
                si_value
            );
            assert_eq!(
                *dimension,
                DimensionVector::LENGTH,
                "sqrt(sqrt(L^4)) should have LENGTH dimension"
            );
        }
        other => panic!("sqrt_sqrt_L4 should be Scalar, got {:?}", other),
    }
}

/// sqrt_L_frac = sqrt(3.0 m) → fractional exponent LENGTH^(1/2)
/// Verifies fractional dimension exponents are preserved.
#[test]
fn sqrt_length_gives_fractional_dimension() {
    let result = eval_ri_file("../../examples/dimensional_chains.ri", "dimensional_chains");
    let id = ValueCellId::new("DimensionalChains", "sqrt_L_frac");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'sqrt_L_frac' not found"));
    let expected_dim = DimensionVector::LENGTH.root(2);
    match val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            let expected_val = 3.0_f64.sqrt();
            assert!(
                (si_value - expected_val).abs() < 1e-9,
                "sqrt_L_frac should be ≈{}, got {}",
                expected_val,
                si_value
            );
            assert_eq!(
                *dimension, expected_dim,
                "sqrt(LENGTH) should have LENGTH^(1/2) fractional dimension"
            );
        }
        other => panic!("sqrt_L_frac should be Scalar, got {:?}", other),
    }
}

// ── step-7: engineering formula dimension assertions ──────────────────────────

/// reynolds = ρ·v·L/μ — dimensionless (Re ≈ 200000)
#[test]
fn reynolds_number_is_dimensionless() {
    let result = eval_ri_file("../../examples/dimensional_chains.ri", "dimensional_chains");
    let id = ValueCellId::new("DimensionalChains", "reynolds");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'reynolds' not found"));
    match val {
        Value::Real(v) => {
            assert!(
                (v - 200000.0).abs() < 1e-3,
                "reynolds should be ≈200000, got {}",
                v
            );
        }
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                dimension.is_dimensionless(),
                "reynolds dimension should be DIMENSIONLESS, got {:?}",
                dimension
            );
            assert!(
                (si_value - 200000.0).abs() < 1e-3,
                "reynolds should be ≈200000, got {}",
                si_value
            );
        }
        other => panic!(
            "reynolds should be Real or dimensionless Scalar, got {:?}",
            other
        ),
    }
}

/// moment_of_inertia = i_mass * i_radius² → Scalar(0.5, MASS * AREA = kg·m²)
#[test]
fn moment_of_inertia_dimension() {
    let result = eval_ri_file("../../examples/dimensional_chains.ri", "dimensional_chains");
    let id = ValueCellId::new("DimensionalChains", "moment_of_inertia");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'moment_of_inertia' not found"));
    let expected_dim = DimensionVector::MASS.mul(&DimensionVector::AREA);
    match val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 0.5).abs() < 1e-9,
                "moment_of_inertia should be ≈0.5 kg·m², got {}",
                si_value
            );
            assert_eq!(
                *dimension, expected_dim,
                "moment_of_inertia dimension should be MASS*LENGTH² (kg·m²)"
            );
        }
        other => panic!("moment_of_inertia should be Scalar, got {:?}", other),
    }
}

/// pressure = p_force / p_area → Scalar(10000, FORCE/AREA = kg·m⁻¹·s⁻²)
#[test]
fn pressure_dimension() {
    let result = eval_ri_file("../../examples/dimensional_chains.ri", "dimensional_chains");
    let id = ValueCellId::new("DimensionalChains", "pressure");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'pressure' not found"));
    let expected_dim = reify_core::dimension::FORCE.div(&DimensionVector::AREA);
    match val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 10000.0).abs() < 1e-6,
                "pressure should be ≈10000 Pa, got {}",
                si_value
            );
            assert_eq!(
                *dimension, expected_dim,
                "pressure dimension should be FORCE/AREA (Pa)"
            );
        }
        other => panic!("pressure should be Scalar, got {:?}", other),
    }
}

/// power = pw_force * pw_vel → Scalar(150, FORCE*LENGTH/TIME = kg·m²·s⁻³)
#[test]
fn power_dimension() {
    let result = eval_ri_file("../../examples/dimensional_chains.ri", "dimensional_chains");
    let id = ValueCellId::new("DimensionalChains", "power");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'power' not found"));
    let expected_dim = reify_core::dimension::FORCE
        .mul(&DimensionVector::LENGTH)
        .div(&DimensionVector::TIME);
    match val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 150.0).abs() < 1e-9,
                "power should be ≈150 W, got {}",
                si_value
            );
            assert_eq!(
                *dimension, expected_dim,
                "power dimension should be FORCE*LENGTH/TIME (W = kg·m²·s⁻³)"
            );
        }
        other => panic!("power should be Scalar, got {:?}", other),
    }
}

/// grav_pe = pe_mass * g_earth * pe_height → Scalar(98.1, FORCE*LENGTH = J)
#[test]
fn gravitational_pe_dimension() {
    let result = eval_ri_file("../../examples/dimensional_chains.ri", "dimensional_chains");
    let id = ValueCellId::new("DimensionalChains", "grav_pe");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'grav_pe' not found"));
    let expected_dim = reify_core::dimension::FORCE.mul(&DimensionVector::LENGTH);
    match val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 98.1).abs() < 1e-6,
                "grav_pe should be ≈98.1 J, got {}",
                si_value
            );
            assert_eq!(
                *dimension, expected_dim,
                "grav_pe dimension should be ENERGY (FORCE*LENGTH)"
            );
        }
        other => panic!("grav_pe should be Scalar, got {:?}", other),
    }
}

/// spring_e = 0.5 * spring_k * spring_x² → Scalar(2.5, FORCE*LENGTH = J)
#[test]
fn spring_energy_dimension() {
    let result = eval_ri_file("../../examples/dimensional_chains.ri", "dimensional_chains");
    let id = ValueCellId::new("DimensionalChains", "spring_e");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'spring_e' not found"));
    let expected_dim = reify_core::dimension::FORCE.mul(&DimensionVector::LENGTH);
    match val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 2.5).abs() < 1e-9,
                "spring_e should be ≈2.5 J, got {}",
                si_value
            );
            assert_eq!(
                *dimension, expected_dim,
                "spring_e dimension should be ENERGY (FORCE*LENGTH)"
            );
        }
        other => panic!("spring_e should be Scalar, got {:?}", other),
    }
}
