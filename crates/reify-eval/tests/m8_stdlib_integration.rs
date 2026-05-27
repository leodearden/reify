//! M8.1 stdlib integration tests.
//!
//! Exercises math and linalg builtins (from tasks 300+301) through the full
//! parse → compile → eval pipeline using .ri fixture files in examples/.
//!
//! Two fixtures:
//!   - math_linalg.ri:            vec3/dot/cross/magnitude/normalize/acos/clamp/lerp
//!   - dimensional_consistency.ri: dimension-preservation across all builtins

use std::fs;

use reify_test_support::mocks::MockConstraintChecker;
use reify_core::{DimensionVector, ModulePath, Severity, ValueCellId};
use reify_ir::Value;

// ── Helper ───────────────────────────────────────────────────────────────────

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

// ── Section 1: math_linalg.ri ────────────────────────────────────────────────

// ── step-1: math_linalg_parses_and_compiles ──────────────────────────────────

/// Load math_linalg.ri, parse, compile, eval — no errors, non-empty values.
/// Tests the full pipeline smoke test for the math/linalg fixture.
#[test]
fn math_linalg_parses_and_compiles() {
    let result = eval_ri_file("../../examples/math_linalg.ri", "math_linalg");
    assert!(
        !result.values.is_empty(),
        "eval should produce non-empty values for math_linalg.ri"
    );
}

// ── step-3: dot product (work) and cross product (torque) ────────────────────

/// dot(force, displacement) where force=(10N,0,0), displacement=(5m,0,0).
/// Expected: work = Scalar { si_value ≈ 50.0, dimension = FORCE*LENGTH } (Joules).
#[test]
fn math_linalg_dot_product_work() {
    let result = eval_ri_file("../../examples/math_linalg.ri", "math_linalg");
    let id = ValueCellId::new("MathLinalg", "work");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'work' not found in eval result"));

    let expected_dim = reify_core::dimension::FORCE.mul(&DimensionVector::LENGTH);
    match val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 50.0).abs() < 1e-9,
                "work si_value should be ≈50.0 J, got {}",
                si_value
            );
            assert_eq!(
                *dimension, expected_dim,
                "work dimension should be FORCE*LENGTH (energy), got {:?}",
                dimension
            );
        }
        other => panic!("work should be Value::Scalar, got {:?}", other),
    }
}

/// cross(r_arm, f_applied) where r_arm=(0,2m,0), f_applied=(3N,0,0).
/// Expected torque: (cx=0, cy=0, cz=-6) with dimension LENGTH*FORCE.
/// Cross product: cz = r_x*f_y - r_y*f_x = 0*0 - 2*3 = -6 N·m.
#[test]
fn math_linalg_cross_product_torque() {
    let result = eval_ri_file("../../examples/math_linalg.ri", "math_linalg");
    let id = ValueCellId::new("MathLinalg", "torque");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'torque' not found in eval result"));

    let expected_dim = DimensionVector::LENGTH.mul(&reify_core::dimension::FORCE);
    match val {
        Value::Vector(components) => {
            assert_eq!(components.len(), 3, "torque should have 3 components");
            // cx ≈ 0.0
            match &components[0] {
                Value::Scalar {
                    si_value,
                    dimension,
                } => {
                    assert!(
                        si_value.abs() < 1e-9,
                        "torque x-component should be ≈0, got {}",
                        si_value
                    );
                    assert_eq!(*dimension, expected_dim, "torque x dimension mismatch");
                }
                other => panic!("torque x-component should be Scalar, got {:?}", other),
            }
            // cy ≈ 0.0
            match &components[1] {
                Value::Scalar {
                    si_value,
                    dimension,
                } => {
                    assert!(
                        si_value.abs() < 1e-9,
                        "torque y-component should be ≈0, got {}",
                        si_value
                    );
                    assert_eq!(*dimension, expected_dim, "torque y dimension mismatch");
                }
                other => panic!("torque y-component should be Scalar, got {:?}", other),
            }
            // cz ≈ -6.0
            match &components[2] {
                Value::Scalar {
                    si_value,
                    dimension,
                } => {
                    assert!(
                        (si_value - (-6.0)).abs() < 1e-9,
                        "torque z-component should be ≈-6, got {}",
                        si_value
                    );
                    assert_eq!(*dimension, expected_dim, "torque z dimension mismatch");
                }
                other => panic!("torque z-component should be Scalar, got {:?}", other),
            }
        }
        other => panic!("torque should be Value::Vector, got {:?}", other),
    }
}

// ── step-5: normalize, magnitude, angle, clamp+lerp ─────────────────────────

/// magnitude(vec3(3m/s, 4m/s, 0)) = Scalar(5.0, LENGTH/TIME).
/// direction = normalize(vec3(3m/s, 4m/s, 0)) = Vector([Real(0.6), Real(0.8), Real(0.0)]).
#[test]
fn math_linalg_normalize_and_magnitude() {
    let result = eval_ri_file("../../examples/math_linalg.ri", "math_linalg");

    // speed
    let speed_id = ValueCellId::new("MathLinalg", "speed");
    let speed = result
        .values
        .get(&speed_id)
        .unwrap_or_else(|| panic!("'speed' not found in eval result"));

    let expected_vel_dim = DimensionVector::LENGTH.div(&DimensionVector::TIME);
    match speed {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 5.0).abs() < 1e-9,
                "speed should be ≈5.0 m/s, got {}",
                si_value
            );
            assert_eq!(
                *dimension, expected_vel_dim,
                "speed dimension should be LENGTH/TIME, got {:?}",
                dimension
            );
        }
        other => panic!("speed should be Value::Scalar, got {:?}", other),
    }

    // direction
    let dir_id = ValueCellId::new("MathLinalg", "direction");
    let direction = result
        .values
        .get(&dir_id)
        .unwrap_or_else(|| panic!("'direction' not found in eval result"));

    match direction {
        Value::Vector(components) => {
            assert_eq!(components.len(), 3, "direction should have 3 components");
            let expected = [0.6_f64, 0.8_f64, 0.0_f64];
            for (i, (comp, exp)) in components.iter().zip(expected.iter()).enumerate() {
                match comp {
                    Value::Real(v) => {
                        assert!(
                            (v - exp).abs() < 1e-9,
                            "direction[{}] should be ≈{}, got {}",
                            i,
                            exp,
                            v
                        );
                    }
                    other => panic!(
                        "direction[{}] should be Value::Real (dimensionless), got {:?}",
                        i, other
                    ),
                }
            }
        }
        other => panic!("direction should be Value::Vector, got {:?}", other),
    }
}

/// acos(dot(normalize(unit_x), normalize(unit_y))) = Scalar(π/2, ANGLE).
/// clamp(lerp(1mm, 10mm, 0.3), 2mm, 8mm) = Scalar(0.0037, LENGTH) (3.7mm).
#[test]
fn math_linalg_angle_and_chained() {
    let result = eval_ri_file("../../examples/math_linalg.ri", "math_linalg");

    // angle_between
    let angle_id = ValueCellId::new("MathLinalg", "angle_between");
    let angle = result
        .values
        .get(&angle_id)
        .unwrap_or_else(|| panic!("'angle_between' not found in eval result"));

    match angle {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - std::f64::consts::FRAC_PI_2).abs() < 1e-9,
                "angle_between should be ≈π/2, got {}",
                si_value
            );
            assert_eq!(
                *dimension,
                DimensionVector::ANGLE,
                "angle_between dimension should be ANGLE, got {:?}",
                dimension
            );
        }
        other => panic!("angle_between should be Value::Scalar, got {:?}", other),
    }

    // chained = clamp(lerp(1mm, 10mm, 0.3), 2mm, 8mm)
    // lerp(0.001, 0.010, 0.3) = 0.001 + 0.009*0.3 = 0.001 + 0.0027 = 0.0037
    // clamp(0.0037, 0.002, 0.008) = 0.0037 (within range)
    let chained_id = ValueCellId::new("MathLinalg", "chained");
    let chained = result
        .values
        .get(&chained_id)
        .unwrap_or_else(|| panic!("'chained' not found in eval result"));

    match chained {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 0.0037).abs() < 1e-9,
                "chained should be ≈0.0037 (3.7mm), got {}",
                si_value
            );
            assert_eq!(
                *dimension,
                DimensionVector::LENGTH,
                "chained dimension should be LENGTH, got {:?}",
                dimension
            );
        }
        other => panic!("chained should be Value::Scalar, got {:?}", other),
    }
}

// ── Section 2: dimensional_consistency.ri ────────────────────────────────────

// ── step-7: dimensional_consistency_parses_and_compiles ──────────────────────

/// Load dimensional_consistency.ri, parse, compile, eval — no errors, non-empty values.
#[test]
fn dimensional_consistency_parses_and_compiles() {
    let result = eval_ri_file(
        "../../examples/dimensional_consistency.ri",
        "dimensional_consistency",
    );
    assert!(
        !result.values.is_empty(),
        "eval should produce non-empty values for dimensional_consistency.ri"
    );
}

// ── step-9: dimension assertion tests ────────────────────────────────────────

/// lerp(5mm, 15mm, 0.5) = Scalar(0.010, LENGTH) — lerp preserves LENGTH dimension.
#[test]
fn dim_lerp_preserves_length() {
    let result = eval_ri_file(
        "../../examples/dimensional_consistency.ri",
        "dimensional_consistency",
    );
    let id = ValueCellId::new("DimensionalConsistency", "lerp_mm");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'lerp_mm' not found"));

    match val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            // lerp(0.005, 0.015, 0.5) = 0.010
            assert!(
                (si_value - 0.010).abs() < 1e-9,
                "lerp_mm si_value should be ≈0.010 (10mm), got {}",
                si_value
            );
            assert_eq!(
                *dimension,
                DimensionVector::LENGTH,
                "lerp_mm dimension should be LENGTH"
            );
        }
        other => panic!("lerp_mm should be Scalar, got {:?}", other),
    }
}

/// dot(m_vec, n_vec) where m_vec=[2m,1m,0m] and n_vec=[3N,0N,0N] = Scalar(6.0, FORCE*LENGTH).
#[test]
fn dim_dot_energy() {
    let result = eval_ri_file(
        "../../examples/dimensional_consistency.ri",
        "dimensional_consistency",
    );
    let id = ValueCellId::new("DimensionalConsistency", "dot_result");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'dot_result' not found"));

    let expected_dim = reify_core::dimension::FORCE.mul(&DimensionVector::LENGTH);
    match val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            // dot([2,1,0], [3,0,0]) = 2*3 + 1*0 + 0*0 = 6.0
            assert!(
                (si_value - 6.0).abs() < 1e-9,
                "dot_result si_value should be ≈6.0, got {}",
                si_value
            );
            assert_eq!(
                *dimension, expected_dim,
                "dot_result dimension should be FORCE*LENGTH, got {:?}",
                dimension
            );
        }
        other => panic!("dot_result should be Scalar, got {:?}", other),
    }
}

/// cross(m_vec, n_vec) where m_vec=[2m,1m,0m] and n_vec=[3N,0N,0N].
/// cross z-component = 2*0 - 1*3 = -3 N·m, dimension = LENGTH*FORCE.
/// Wait: cx = my*nz - mz*ny = 1*0 - 0*0 = 0
///       cy = mz*nx - mx*nz = 0*3 - 2*0 = 0
///       cz = mx*ny - my*nx = 2*0 - 1*3 = -3
#[test]
fn dim_cross_torque() {
    let result = eval_ri_file(
        "../../examples/dimensional_consistency.ri",
        "dimensional_consistency",
    );
    let id = ValueCellId::new("DimensionalConsistency", "cross_result");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'cross_result' not found"));

    let expected_dim = DimensionVector::LENGTH.mul(&reify_core::dimension::FORCE);
    match val {
        Value::Vector(components) => {
            assert_eq!(components.len(), 3, "cross_result should have 3 components");
            match &components[2] {
                Value::Scalar {
                    si_value,
                    dimension,
                } => {
                    assert!(
                        (si_value - (-3.0)).abs() < 1e-9,
                        "cross_result z-component should be ≈-3, got {}",
                        si_value
                    );
                    assert_eq!(
                        *dimension, expected_dim,
                        "cross_result z dimension should be LENGTH*FORCE"
                    );
                }
                other => panic!("cross_result z should be Scalar, got {:?}", other),
            }
        }
        other => panic!("cross_result should be Vector, got {:?}", other),
    }
}

/// 9.81 * 1m / (1s*1s) * 1kg = Scalar(9.81, FORCE) — gravitational force on 1 kg.
/// g_accel = 9.81 m/s², weight = g_accel * 1kg = 9.81 N.
#[test]
fn dim_gravity_force() {
    let result = eval_ri_file(
        "../../examples/dimensional_consistency.ri",
        "dimensional_consistency",
    );
    let id = ValueCellId::new("DimensionalConsistency", "weight");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'weight' not found"));

    match val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 9.81).abs() < 1e-9,
                "weight si_value should be ≈9.81 N, got {}",
                si_value
            );
            assert_eq!(
                *dimension,
                reify_core::dimension::FORCE,
                "weight dimension should be FORCE (N = kg·m/s²), got {:?}",
                dimension
            );
        }
        other => panic!("weight should be Scalar, got {:?}", other),
    }
}

/// mod(7, 3) = Value::Int(1).
#[test]
fn dim_mod_integer() {
    let result = eval_ri_file(
        "../../examples/dimensional_consistency.ri",
        "dimensional_consistency",
    );
    let id = ValueCellId::new("DimensionalConsistency", "mod_result");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'mod_result' not found"));

    assert_eq!(
        val,
        &Value::Int(1),
        "mod(7, 3) should be Int(1), got {:?}",
        val
    );
}

/// normalize(vec3(3m, 4m, 0m)) produces Vector with Real (dimensionless) components.
/// normalize always returns dimensionless components regardless of input dimension.
#[test]
fn dim_normalize_dimensionless() {
    let result = eval_ri_file(
        "../../examples/dimensional_consistency.ri",
        "dimensional_consistency",
    );
    let id = ValueCellId::new("DimensionalConsistency", "norm_result");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'norm_result' not found"));

    match val {
        Value::Vector(components) => {
            assert_eq!(components.len(), 3, "norm_result should have 3 components");
            for (i, comp) in components.iter().enumerate() {
                assert!(
                    matches!(comp, Value::Real(_)),
                    "norm_result[{}] should be Value::Real (dimensionless), got {:?}",
                    i,
                    comp
                );
            }
            // [3,4,0] normalized = [0.6, 0.8, 0.0]
            match (&components[0], &components[1], &components[2]) {
                (Value::Real(x), Value::Real(y), Value::Real(z)) => {
                    assert!((x - 0.6).abs() < 1e-9, "norm[0] should be ≈0.6, got {}", x);
                    assert!((y - 0.8).abs() < 1e-9, "norm[1] should be ≈0.8, got {}", y);
                    assert!(z.abs() < 1e-9, "norm[2] should be ≈0.0, got {}", z);
                }
                _ => unreachable!(),
            }
        }
        other => panic!("norm_result should be Vector, got {:?}", other),
    }
}

/// magnitude(vec3(3m, 4m, 0m)) = Scalar(5.0, LENGTH) — preserves input dimension.
#[test]
fn dim_magnitude_preserves() {
    let result = eval_ri_file(
        "../../examples/dimensional_consistency.ri",
        "dimensional_consistency",
    );
    let id = ValueCellId::new("DimensionalConsistency", "mag_result");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'mag_result' not found"));

    match val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 5.0).abs() < 1e-9,
                "mag_result si_value should be ≈5.0 m, got {}",
                si_value
            );
            assert_eq!(
                *dimension,
                DimensionVector::LENGTH,
                "mag_result dimension should be LENGTH, got {:?}",
                dimension
            );
        }
        other => panic!("mag_result should be Scalar, got {:?}", other),
    }
}

/// clamp(3mm, 1mm, 10mm) = Scalar(0.003, LENGTH) — clamp preserves dimension.
#[test]
fn dim_clamp_preserves() {
    let result = eval_ri_file(
        "../../examples/dimensional_consistency.ri",
        "dimensional_consistency",
    );
    let id = ValueCellId::new("DimensionalConsistency", "clamped_dim");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'clamped_dim' not found"));

    match val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - 0.003).abs() < 1e-9,
                "clamped_dim si_value should be ≈0.003 (3mm), got {}",
                si_value
            );
            assert_eq!(
                *dimension,
                DimensionVector::LENGTH,
                "clamped_dim dimension should be LENGTH, got {:?}",
                dimension
            );
        }
        other => panic!("clamped_dim should be Scalar, got {:?}", other),
    }
}
