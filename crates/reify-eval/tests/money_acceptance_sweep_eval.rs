//! Acceptance tests for the Money dimension (slot 9) and Angle/Torque-vs-Energy regression guard.

use reify_test_support::eval_source;
use reify_types::{DimensionVector, Rational, Value, ValueCellId};

/// Fetch the cell at `entity.member` from `result`, assert it is a
/// `Value::Scalar`, and return its `DimensionVector`. Panics with a
/// localised message if the cell is missing or has a non-Scalar variant.
/// `#[track_caller]` keeps panic line numbers pointing at the test's call site.
#[track_caller]
fn extract_scalar_dimension(
    result: &reify_eval::EvalResult,
    entity: &str,
    member: &str,
) -> DimensionVector {
    let id = ValueCellId::new(entity, member);
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'{}.{}' not found in eval result", entity, member));
    match val {
        Value::Scalar { dimension, .. } => *dimension,
        other => panic!(
            "expected '{}.{}' to be Value::Scalar, got {:?}",
            entity, member, other
        ),
    }
}

// ─── Length × Mass does not set Money slot 9 at runtime ─────────────────────

/// At runtime, `2m * 3kg` (Length × Mass = Momentum) must evaluate to a
/// Scalar whose Money slot 9 is ZERO and Angle slot 7 is ZERO.
///
/// This is the eval-layer mirror of `money_does_not_leak_into_unrelated_
/// arithmetic` from `crates/reify-types/src/dimension.rs`, which tests
/// at the DimensionVector level. The eval-layer pin confirms that the runtime
/// dimension-propagation path is as clean as the type-vector layer.
#[test]
fn eval_length_times_mass_does_not_set_money_slot() {
    let source = "pub unit USD : Money\n\
                  type Momentum = Length * Mass\n\
                  structure S { param p : Momentum = 2m * 3kg }";
    let result = eval_source(source);
    let id = ValueCellId::new("S", "p");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'p' not found in eval result"));
    match val {
        Value::Scalar { dimension, .. } => {
            assert_eq!(
                dimension.0[9],
                Rational::ZERO,
                "Money slot 9 must be ZERO for Length*Mass; got {:?}",
                dimension
            );
            assert_eq!(
                dimension.0[7],
                Rational::ZERO,
                "Angle slot 7 must be ZERO for Length*Mass; got {:?}",
                dimension
            );
        }
        other => panic!("expected Value::Scalar {{ .. }}, got {:?}", other),
    }
}

// ─── USD literal evaluates to exactly DimensionVector::MONEY ─────────────────

/// At runtime, the `25USD` literal must evaluate to a Scalar whose dimension
/// exactly matches `DimensionVector::MONEY` (slot 9 = ONE, all other slots
/// ZERO). The Debug-formatted panic message localises any slot regression.
#[test]
fn eval_usd_literal_runtime_dimension_has_only_slot_nine_set() {
    let source = "pub unit USD : Money\n\
                  structure S { param p : Money = 25USD }";
    let result = eval_source(source);
    let id = ValueCellId::new("S", "p");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'p' not found in eval result"));
    match val {
        Value::Scalar { dimension, .. } => {
            assert_eq!(
                *dimension,
                DimensionVector::MONEY,
                "25USD dimension should equal DimensionVector::MONEY; got {:?}",
                dimension
            );
        }
        other => panic!("expected Value::Scalar {{ .. }}, got {:?}", other),
    }
}

// ─── Torque and Energy remain distinct at runtime, with and without a Money factor ───

/// At runtime, expressions that evaluate to Torque (Force·Length/Angle) and
/// Energy (Force·Length) must produce Scalars with distinct dimensions:
///   - torque slot 7 (Angle) = −1, energy slot 7 (Angle) = 0
///
/// Introducing a USD Money factor to both must keep their dimensions distinct:
///   - Money·Torque slot 9 (Money) = +1, slot 7 (Angle) = −1
///   - Money·Energy slot 9 (Money) = +1, slot 7 (Angle) = 0
///
/// All four params are evaluated in a single `eval_source` call over one S
/// structure to eliminate the duplicated source-string and eval-call boilerplate.
#[test]
fn eval_torque_and_energy_remain_distinct_at_runtime_with_and_without_money_factor() {
    let source = r#"
        pub unit USD : Money
        type Torque = Force * Length / Angle
        type Energy = Force * Length
        type MoneyTorque = Money * Torque
        type MoneyEnergy = Money * Energy
        structure S {
            param torque       : Torque       = (((1kg * 1m) / (1s * 1s)) * 1m) / 1rad
            param energy       : Energy       = ((1kg * 1m) / (1s * 1s)) * 1m
            param cost_torque  : MoneyTorque  = 1USD * ((((1kg * 1m) / (1s * 1s)) * 1m) / 1rad)
            param cost_energy  : MoneyEnergy  = 1USD * (((1kg * 1m) / (1s * 1s)) * 1m)
        }
    "#;
    let result = eval_source(source);

    let t_dim = extract_scalar_dimension(&result, "S", "torque");
    let e_dim = extract_scalar_dimension(&result, "S", "energy");
    let ct_dim = extract_scalar_dimension(&result, "S", "cost_torque");
    let ce_dim = extract_scalar_dimension(&result, "S", "cost_energy");

    assert_ne!(
        t_dim, e_dim,
        "Torque and Energy must have distinct runtime dimensions"
    );
    assert_eq!(
        t_dim.0[7],
        Rational::new(-1, 1),
        "Torque Angle slot 7 should be -1 at runtime; dimension = {:?}",
        t_dim
    );
    assert_eq!(
        e_dim.0[7],
        Rational::ZERO,
        "Energy Angle slot 7 should be ZERO at runtime; dimension = {:?}",
        e_dim
    );
    assert_ne!(
        ct_dim, ce_dim,
        "Money·Torque and Money·Energy must have distinct runtime dimensions"
    );
    assert_eq!(
        ct_dim.0[9],
        Rational::ONE,
        "Money·Torque slot 9 (Money) should be ONE at runtime; dimension = {:?}",
        ct_dim
    );
    assert_eq!(
        ce_dim.0[9],
        Rational::ONE,
        "Money·Energy slot 9 (Money) should be ONE at runtime; dimension = {:?}",
        ce_dim
    );
    assert_eq!(
        ct_dim.0[7],
        Rational::new(-1, 1),
        "Money·Torque slot 7 (Angle) should be -1 at runtime; dimension = {:?}",
        ct_dim
    );
    assert_eq!(
        ce_dim.0[7],
        Rational::ZERO,
        "Money·Energy slot 7 (Angle) should be ZERO at runtime; dimension = {:?}",
        ce_dim
    );
}
