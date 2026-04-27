//! Acceptance test sweep for the Money dimension (slot 9) and
//! Angle/Torque-vs-Energy regression guard (task 2383).
//!
//! This file pins compile-level behaviour for acceptance criteria C1–C5:
//!   C1. Money slot-9 isolation under composition (DimensionVector layer).
//!   C2. Money does not touch Angle slot 7 under any composition.
//!   C3. Torque (Force·Length/Angle) ≠ Energy (Force·Length) at the
//!       DimensionVector level.
//!   C4. Adding Money factors to Torque and Energy preserves the distinction.
//!   C5. USD literal through stdlib path produces slot 9 = 1, slot 7 = 0.
//!
//! Criteria C6–C8 (runtime purity and runtime Angle/Torque guard) are
//! covered at the eval layer in `money_acceptance_sweep_eval.rs`.
//!
//! Inline `pub unit USD : Money` declaration pattern is used for hermeticity
//! (order-independent of the stdlib's `pub unit USD : Money` from task 2378).
//! A single stdlib-path integration test in the C5 section exercises the
//! prelude connection explicitly.
//!
//! NOT referenced here: artifacts from sibling tasks 2380
//! (`money_force_diag_tests.rs`), 2381 (`examples/cost-aggregation.ri`),
//! and 2382 (LSP source-form display). Those tasks own pinning their own
//! deliverables; this task's declared dependencies are 2377, 2379, 2444
//! (all merged).

mod common;

use common::{UNIT_EPSILON, stdlib_param_si_value};
use reify_test_support::{compile_source, errors_only};
use reify_types::{DimensionVector, Rational, Type};

// ─── C1: Money slot-9 isolation under composition ────────────────────────────

/// `DimensionVector::MONEY` must have slot 9 = ONE and every other slot
/// (0..=8, including Angle slot 7) = ZERO.  This strengthens the existing
/// `money_constant_populates_slot_9` pin in `dimension.rs` by locking ALL
/// non-Money slots, not just the single Money slot.
///
/// Guards against any future 10-slot-vector reorganisation that might
/// accidentally populate a non-9 slot when constructing the MONEY basis vector.
#[test]
fn money_constant_isolates_slot_nine_only() {
    let m = DimensionVector::MONEY;
    assert_eq!(m.0[9], Rational::ONE, "slot 9 should be ONE");
    for i in 0..9usize {
        assert_eq!(
            m.0[i],
            Rational::ZERO,
            "slot {} should be ZERO for DimensionVector::MONEY",
            i
        );
    }
}

/// `MONEY / MASS` must have slot 1 (Mass) = −1, slot 9 (Money) = +1, and all
/// other slots — crucially slot 7 (Angle) — = ZERO.
///
/// Verifies that the element-wise subtraction in `DimensionVector::div` keeps
/// non-involved slots at zero when composing Money with a purely-Mass dimension.
#[test]
fn money_per_mass_keeps_only_slots_one_and_nine() {
    let result = DimensionVector::MONEY.div(&DimensionVector::MASS);
    assert_eq!(result.0[1], Rational::new(-1, 1), "slot 1 (Mass) should be -1");
    assert_eq!(result.0[9], Rational::ONE, "slot 9 (Money) should be ONE");
    for i in [0usize, 2, 3, 4, 5, 6, 7, 8] {
        assert_eq!(
            result.0[i],
            Rational::ZERO,
            "slot {} should be ZERO for MONEY/MASS",
            i
        );
    }
}

// ─── C2: Money does not touch Angle slot 7 under any composition ─────────────

/// `MONEY × FORCE` must leave Angle slot 7 = ZERO.
///
/// Force (kg·m·s⁻²) has no Angle component. Composing it with Money must not
/// introduce any spurious Angle exponent — this guards against any future
/// 10-slot exponent-buffer bug that might cross-populate adjacent slots when
/// `mul` iterates indices.
#[test]
fn money_compound_with_force_keeps_angle_slot_zero() {
    let result = DimensionVector::MONEY.mul(&DimensionVector::FORCE);
    assert_eq!(
        result.0[7],
        Rational::ZERO,
        "Angle slot 7 should remain ZERO after MONEY × FORCE"
    );
}
