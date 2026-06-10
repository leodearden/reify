//! End-to-end eval smoke tests for FEA-5 stress-tensor reductions.
//!
//! Pins `von_mises`, `principal_stresses`, and `stress_invariants` through the
//! full parse → compile-with-stdlib → `Engine::eval` path.  Uses a uniaxial
//! pressure tensor as the canonical fixture so eval values match the
//! closed-form invariants exactly (diagonal, off-diagonals zero).
//!
//! These tests are RED until the full composition of step-4 (expr.rs typing),
//! step-6 (stress_invariants stdlib builtin), and the fea.ri struct def all
//! compose through the real stdlib+engine path.

#![allow(clippy::mutable_key_type)]

use reify_core::{DimensionVector, ValueCellId};
use reify_ir::{PersistentMap, Value};
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

/// `PersistentMap<String, Value>::get` is keyed by `&String`; this lets the
/// scenarios index `StructureInstance.fields` with a string literal.
fn field<'a>(m: &'a PersistentMap<String, Value>, k: &str) -> Option<&'a Value> {
    m.get(&k.to_string())
}

// ── fixture ──────────────────────────────────────────────────────────────────

/// `.ri` fixture: uniaxial stress tensor (σ_xx = 1 MPa, all others 0).
///
/// von_mises([[σ,0,0],[0,0,0],[0,0,0]]) = σ  (closed-form)
/// principal_stresses(...)             = [0, 0, σ] (sorted ascending)
/// stress_invariants(...):
///   I1 = σ, I2 = 0, I3 = 0
const UNIAXIAL_FIXTURE: &str = r#"
structure def StressReductionsFixture {
    let stress = matrix([[1.0e6Pa, 0.0Pa, 0.0Pa],
                         [0.0Pa,   0.0Pa, 0.0Pa],
                         [0.0Pa,   0.0Pa, 0.0Pa]])

    let vm    = von_mises(stress)
    let ps    = principal_stresses(stress)
    let inv   = stress_invariants(stress)
}
"#;

/// Helper: compile and eval the fixture, returning the `Engine::eval` result.
fn run_fixture() -> reify_eval::EvalResult {
    let compiled = parse_and_compile_with_stdlib(UNIAXIAL_FIXTURE);
    let mut engine = make_simple_engine();
    engine.eval(&compiled)
}

// ── von_mises eval ────────────────────────────────────────────────────────────

/// `von_mises([[σ,0,0],[0,0,0],[0,0,0]])` must eval to `Scalar<PRESSURE>(1e6)`.
///
/// (uniaxial stress: von Mises = σ)
#[test]
fn von_mises_uniaxial_evals_to_scalar_pressure() {
    let result = run_fixture();
    let id = ValueCellId::new("StressReductionsFixture", "vm");
    let vm = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("StressReductionsFixture.vm cell missing from eval result"));

    match vm {
        Value::Scalar { si_value, dimension } => {
            assert_eq!(
                *dimension,
                DimensionVector::PRESSURE,
                "von_mises must have PRESSURE dimension, got {:?}",
                dimension
            );
            let expected = 1.0e6_f64;
            assert!(
                (si_value - expected).abs() < 1.0,
                "von_mises(uniaxial σ=1MPa) must be ~1e6 Pa, got {}",
                si_value
            );
        }
        other => panic!(
            "expected Value::Scalar<PRESSURE> for von_mises, got {other:?}"
        ),
    }
}

// ── principal_stresses eval ───────────────────────────────────────────────────

/// `principal_stresses([[σ,0,0],[0,0,0],[0,0,0]])` must eval to a 3-element
/// `Value::List` sorted ascending: `[0 Pa, 0 Pa, 1 MPa]`.
#[test]
fn principal_stresses_uniaxial_evals_to_sorted_list() {
    let result = run_fixture();
    let id = ValueCellId::new("StressReductionsFixture", "ps");
    let ps = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("StressReductionsFixture.ps cell missing from eval result"));

    match ps {
        Value::List(items) => {
            assert_eq!(items.len(), 3, "principal_stresses must return a 3-element list");
            // All elements must be PRESSURE scalars
            for (i, item) in items.iter().enumerate() {
                match item {
                    Value::Scalar { dimension, .. } => {
                        assert_eq!(
                            *dimension,
                            DimensionVector::PRESSURE,
                            "principal_stresses[{}] must have PRESSURE dimension",
                            i
                        );
                    }
                    other => panic!(
                        "principal_stresses[{}] must be Scalar<PRESSURE>, got {other:?}",
                        i
                    ),
                }
            }
            // Sorted ascending: [0, 0, 1e6]
            let vals: Vec<f64> = items
                .iter()
                .map(|v| v.as_f64().expect("principal stress must be numeric"))
                .collect();
            assert!(
                vals[0] <= vals[1] && vals[1] <= vals[2],
                "principal_stresses must be sorted ascending, got {:?}",
                vals
            );
            let sigma = 1.0e6_f64;
            assert!(
                (vals[2] - sigma).abs() < 1.0,
                "largest principal stress must be ~1 MPa for uniaxial σ=1MPa, got {}",
                vals[2]
            );
        }
        other => panic!("expected Value::List for principal_stresses, got {other:?}"),
    }
}

// ── stress_invariants eval ────────────────────────────────────────────────────

/// `stress_invariants([[σ,0,0],[0,0,0],[0,0,0]])` must eval to a
/// `StructureInstance("StressInvariants")` with:
///   i1 = σ   (PRESSURE)
///   i2 = 0   (PRESSURE²)
///   i3 = 0   (PRESSURE³)
#[test]
fn stress_invariants_uniaxial_evals_to_structure_instance() {
    let result = run_fixture();
    let id = ValueCellId::new("StressReductionsFixture", "inv");
    let inv = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("StressReductionsFixture.inv cell missing from eval result"));

    let data = match inv {
        Value::StructureInstance(data) => data,
        other => panic!(
            "expected Value::StructureInstance for stress_invariants, got {other:?}"
        ),
    };

    assert_eq!(
        data.type_name, "StressInvariants",
        "stress_invariants type_name must be 'StressInvariants', got {:?}",
        data.type_name
    );

    // i1 = σ_xx = 1e6 Pa (PRESSURE)
    let i1 = field(&data.fields, "i1")
        .unwrap_or_else(|| panic!("StressInvariants.i1 field missing; fields: {:?}", data.fields));
    match i1 {
        Value::Scalar { si_value, dimension } => {
            assert_eq!(
                *dimension,
                DimensionVector::PRESSURE,
                "stress_invariants.i1 must have PRESSURE dimension"
            );
            let expected = 1.0e6_f64;
            assert!(
                (si_value - expected).abs() < 1.0,
                "stress_invariants.i1 must be ~1 MPa for uniaxial σ=1MPa, got {}",
                si_value
            );
        }
        other => panic!("stress_invariants.i1 must be Scalar<PRESSURE>, got {other:?}"),
    }

    let dim2 = DimensionVector::PRESSURE.mul(&DimensionVector::PRESSURE);
    let dim3 = dim2.mul(&DimensionVector::PRESSURE);

    // i2 = 0 (PRESSURE²)
    let i2 = field(&data.fields, "i2")
        .unwrap_or_else(|| panic!("StressInvariants.i2 field missing; fields: {:?}", data.fields));
    match i2 {
        Value::Scalar { si_value, dimension } => {
            assert_eq!(
                *dimension, dim2,
                "stress_invariants.i2 must have PRESSURE² dimension"
            );
            assert!(
                si_value.abs() < 1.0,
                "stress_invariants.i2 must be ~0 for uniaxial off-diagonals=0, got {}",
                si_value
            );
        }
        // When i2 == 0 and dim2 == DIMENSIONLESS, from_real_scalar returns Real(0.0)
        // but for PRESSURE² this is a Scalar. If for any reason it comes back as
        // Value::Real(0.0) accept it too (sanitize_value normalises 0 scalars).
        Value::Real(v) => {
            assert!(
                v.abs() < 1.0,
                "stress_invariants.i2 must be ~0 for uniaxial, got Real({})",
                v
            );
        }
        other => panic!("stress_invariants.i2 must be Scalar<PRESSURE²> or Real(0), got {other:?}"),
    }

    // i3 = 0 (PRESSURE³)
    let i3 = field(&data.fields, "i3")
        .unwrap_or_else(|| panic!("StressInvariants.i3 field missing; fields: {:?}", data.fields));
    match i3 {
        Value::Scalar { si_value, dimension } => {
            assert_eq!(
                *dimension, dim3,
                "stress_invariants.i3 must have PRESSURE³ dimension"
            );
            assert!(
                si_value.abs() < 1.0,
                "stress_invariants.i3 must be ~0 for uniaxial off-diagonals=0, got {}",
                si_value
            );
        }
        Value::Real(v) => {
            assert!(
                v.abs() < 1.0,
                "stress_invariants.i3 must be ~0 for uniaxial, got Real({})",
                v
            );
        }
        other => panic!("stress_invariants.i3 must be Scalar<PRESSURE³> or Real(0), got {other:?}"),
    }
}
