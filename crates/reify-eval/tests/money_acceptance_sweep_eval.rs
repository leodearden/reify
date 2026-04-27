//! Eval-level acceptance sweep for the Money dimension (slot 9) purity guard
//! and Angle/Torque-vs-Energy regression (task 2383).
//!
//! This file covers acceptance criteria C6–C8:
//!   C6. Eval-level: Length × Mass arithmetic leaves Money slot 9 = ZERO at
//!       runtime (eval-layer analogue of `money_does_not_leak_into_unrelated_
//!       arithmetic` from `dimension.rs:700`; PRD §(c) deliverable).
//!   C7. Eval-level: Money cancellation arithmetic `(25USD/1kg) * 2kg` keeps
//!       Angle slot 7 = ZERO at runtime.
//!   C8. Eval-level: a torque value and an energy value have distinct runtime
//!       dimensions; that distinction is preserved when each is multiplied by a
//!       USD Money factor (PRD §(b) deliverable lifted to eval).
//!
//! Criteria C1–C5 (compile-level) are covered in
//! `crates/reify-compiler/tests/money_acceptance_sweep_tests.rs`.
//!
//! Each source begins with `pub unit USD : Money` for hermeticity — tests are
//! order-independent of the stdlib USD entry's merge order. The inline-decl
//! pattern is documented in `money_arithmetic_eval.rs` (task 2379). Units `m`,
//! `kg`, `s`, and `rad` resolve via the built-in `unit_to_scalar` table and
//! require no stdlib.
//!
//! NOT referenced here: artifacts from sibling tasks 2380/2381/2382. Those
//! tasks own pinning their own deliverables; this task's declared dependencies
//! are 2377, 2379, 2444 (all merged).

use reify_test_support::eval_source;
use reify_types::{DimensionVector, Rational, Value, ValueCellId};

// ─── C6: Length × Mass does not set Money slot 9 at runtime ─────────────────

/// At runtime, `2m * 3kg` (Length × Mass = Momentum) must evaluate to a
/// Scalar whose Money slot 9 is ZERO and Angle slot 7 is ZERO.
///
/// This is the eval-layer mirror of `money_does_not_leak_into_unrelated_
/// arithmetic` from `crates/reify-types/src/dimension.rs:700`, which tests
/// at the DimensionVector level.  The eval-layer pin is PRD §(c)'s deliverable:
/// the runtime dimension-propagation path must be as clean as the type-vector
/// layer.
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

// ─── C7: Money cancellation arithmetic keeps Angle slot 7 = ZERO ─────────────

/// At runtime, `(25USD/1kg) * 2kg` evaluates to a Scalar with dimension MONEY.
/// The Angle slot 7 of that result must be ZERO.
///
/// Distinct from `money_per_mass_times_mass_evaluates_to_50_usd` (task 2379)
/// which asserts `dimension == DimensionVector::MONEY`; this test pinpoints
/// the Angle-slot purity directly, so a future bug that corrupts exactly slot 7
/// will surface as a specific failure rather than a catch-all dimension mismatch.
#[test]
fn eval_money_compound_arithmetic_keeps_angle_slot_zero() {
    let source = "pub unit USD : Money\n\
                  structure S { param p : Money = (25USD/1kg) * 2kg }";
    let result = eval_source(source);
    let id = ValueCellId::new("S", "p");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("'p' not found in eval result"));
    match val {
        Value::Scalar { dimension, .. } => {
            assert_eq!(
                dimension.0[7],
                Rational::ZERO,
                "Angle slot 7 must be ZERO after Money cancellation; dimension = {:?}",
                dimension
            );
        }
        other => panic!("expected Value::Scalar {{ .. }}, got {:?}", other),
    }
}

/// At runtime, the `25USD` literal must evaluate to a Scalar whose dimension
/// exactly matches `DimensionVector::MONEY`: slot 9 = ONE, all other slots
/// (including Angle slot 7) = ZERO.
///
/// A slot-by-slot assertion is used so that any single-slot regression is
/// immediately localised in the failure message, rather than just showing
/// a dimension mismatch against the MONEY constant.
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
                dimension.0[9],
                Rational::ONE,
                "slot 9 (Money) should be ONE for 25USD; dimension = {:?}",
                dimension
            );
            for i in 0..9usize {
                assert_eq!(
                    dimension.0[i],
                    Rational::ZERO,
                    "slot {} should be ZERO for 25USD; dimension = {:?}",
                    i,
                    dimension
                );
            }
        }
        other => panic!("expected Value::Scalar {{ .. }}, got {:?}", other),
    }
}

// ─── C8: Torque and Energy remain distinct at runtime, with and without Money ─

/// At runtime, expressions that evaluate to Torque (Force·Length/Angle) and
/// Energy (Force·Length) must produce Scalars with distinct dimensions:
///   - torque dimension slot 7 (Angle) = −1
///   - energy dimension slot 7 (Angle) = 0
///
/// Uses only built-in unit literals (m, kg, s, rad) to keep the test hermetic
/// (no stdlib dependency). The inline `pub unit USD : Money` is included per
/// convention but the USD value is not evaluated in this test.
///
/// This is the first half of PRD §(b): the Angle/Torque-vs-Energy regression
/// holds at the eval layer, not just at the DimensionVector-algebra layer.
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
/// This is the second half of PRD §(b) and the full C8 criterion: the
/// Angle/Torque-vs-Energy distinction must survive when Money is introduced
/// at the eval layer, confirming that the runtime dimension-propagation path
/// does not collapse the Angle-slot distinction under a Money multiplication.
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
