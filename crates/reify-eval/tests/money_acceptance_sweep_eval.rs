//! Acceptance tests for the Money dimension (slot 9) and Angle/Torque-vs-Energy regression guard.

use reify_test_support::eval_source;
use reify_types::{DimensionVector, Rational, Value, ValueCellId};

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

// ─── Torque and Energy remain distinct at runtime ─────────────────────────────

/// At runtime, expressions that evaluate to Torque (Force·Length/Angle) and
/// Energy (Force·Length) must produce Scalars with distinct dimensions:
///   - torque dimension slot 7 (Angle) = −1
///   - energy dimension slot 7 (Angle) = 0
///
/// Uses only built-in unit literals (m, kg, s, rad) to keep the test hermetic
/// (no stdlib dependency). The inline `pub unit USD : Money` is included per
/// convention but the USD value is not evaluated in this test.
#[test]
fn eval_torque_value_differs_from_energy_value_at_runtime() {
    let source = r#"
        pub unit USD : Money
        type Torque = Force * Length / Angle
        type Energy = Force * Length
        structure S {
            param torque : Torque = (((1kg * 1m) / (1s * 1s)) * 1m) / 1rad
            param energy : Energy = ((1kg * 1m) / (1s * 1s)) * 1m
        }
    "#;
    let result = eval_source(source);

    let torque_val = result
        .values
        .get(&ValueCellId::new("S", "torque"))
        .unwrap_or_else(|| panic!("'torque' not found in eval result"));
    let energy_val = result
        .values
        .get(&ValueCellId::new("S", "energy"))
        .unwrap_or_else(|| panic!("'energy' not found in eval result"));

    let t_dim = match torque_val {
        Value::Scalar { dimension, .. } => *dimension,
        other => panic!("expected torque to be Value::Scalar, got {:?}", other),
    };
    let e_dim = match energy_val {
        Value::Scalar { dimension, .. } => *dimension,
        other => panic!("expected energy to be Value::Scalar, got {:?}", other),
    };

    assert_ne!(t_dim, e_dim, "Torque and Energy must have distinct runtime dimensions");
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
}

/// Introducing a USD Money factor to both Torque and Energy at runtime must
/// keep their dimensions distinct. Specifically:
///   - Money·Torque carries slot 9 (Money) = +1 and slot 7 (Angle) = −1
///   - Money·Energy carries slot 9 (Money) = +1 and slot 7 (Angle) = 0
///
/// Confirms that the runtime dimension-propagation path does not collapse the
/// Angle-slot distinction under a Money multiplication.
#[test]
fn eval_torque_with_money_factor_remains_distinct_from_energy_with_money_factor() {
    let source = r#"
        pub unit USD : Money
        type Torque = Force * Length / Angle
        type Energy = Force * Length
        type MoneyTorque = Money * Torque
        type MoneyEnergy = Money * Energy
        structure S {
            param cost_torque : MoneyTorque = 1USD * ((((1kg * 1m) / (1s * 1s)) * 1m) / 1rad)
            param cost_energy : MoneyEnergy = 1USD * (((1kg * 1m) / (1s * 1s)) * 1m)
        }
    "#;
    let result = eval_source(source);

    let ct_val = result
        .values
        .get(&ValueCellId::new("S", "cost_torque"))
        .unwrap_or_else(|| panic!("'cost_torque' not found in eval result"));
    let ce_val = result
        .values
        .get(&ValueCellId::new("S", "cost_energy"))
        .unwrap_or_else(|| panic!("'cost_energy' not found in eval result"));

    let ct_dim = match ct_val {
        Value::Scalar { dimension, .. } => *dimension,
        other => panic!("expected cost_torque to be Value::Scalar, got {:?}", other),
    };
    let ce_dim = match ce_val {
        Value::Scalar { dimension, .. } => *dimension,
        other => panic!("expected cost_energy to be Value::Scalar, got {:?}", other),
    };

    assert_ne!(
        ct_dim,
        ce_dim,
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
