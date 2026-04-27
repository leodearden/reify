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

// ─── C3: Torque ≠ Energy at the DimensionVector level ────────────────────────

/// Torque (Force·Length/Angle = kg·m²·s⁻²·rad⁻¹) and Energy (Force·Length =
/// kg·m²·s⁻²) must be distinct dimensions.  The only difference is slot 7
/// (Angle): torque has −1, energy has 0.
///
/// This is the core regression guard: a 10-slot exponent vector that silently
/// conflates Angle with another slot would make these two dimensions equal,
/// breaking all engineering models that distinguish "rotational energy" from
/// "translational energy".
#[test]
fn torque_dim_differs_from_energy_dim_via_angle_slot() {
    let torque = DimensionVector::FORCE
        .mul(&DimensionVector::LENGTH)
        .div(&DimensionVector::ANGLE);
    let energy = DimensionVector::FORCE.mul(&DimensionVector::LENGTH);

    assert_ne!(torque, energy, "Torque and Energy must be distinct dimensions");
    assert_eq!(
        torque.0[7],
        Rational::new(-1, 1),
        "Torque slot 7 (Angle) should be -1"
    );
    assert_eq!(
        energy.0[7],
        Rational::ZERO,
        "Energy slot 7 (Angle) should be ZERO"
    );
}

// ─── C4: Money added to Torque/Energy preserves the Angle-slot distinction ───

/// Multiplying both Torque and Energy by MONEY produces `Money·Torque` and
/// `Money·Energy`. Each compound dimension must:
///   (a) carry slot 9 (Money) = +1, and
///   (b) retain its original Angle-slot exponent (−1 vs 0 respectively),
///       so the two compound dimensions remain distinct.
///
/// Guards against any future slot-propagation bug that might collapse the
/// Angle-slot distinction once a Money factor is mixed in.
#[test]
fn torque_with_money_factor_remains_distinct_from_energy_with_money_factor() {
    let torque = DimensionVector::FORCE
        .mul(&DimensionVector::LENGTH)
        .div(&DimensionVector::ANGLE);
    let energy = DimensionVector::FORCE.mul(&DimensionVector::LENGTH);

    let cost_per_torque = DimensionVector::MONEY.mul(&torque);
    let cost_per_energy = DimensionVector::MONEY.mul(&energy);

    assert_ne!(
        cost_per_torque,
        cost_per_energy,
        "Money·Torque and Money·Energy must remain distinct"
    );
    assert_eq!(
        cost_per_torque.0[9],
        Rational::ONE,
        "Money·Torque slot 9 should be ONE"
    );
    assert_eq!(
        cost_per_energy.0[9],
        Rational::ONE,
        "Money·Energy slot 9 should be ONE"
    );
    assert_eq!(
        cost_per_torque.0[7],
        Rational::new(-1, 1),
        "Money·Torque slot 7 (Angle) should be -1"
    );
    assert_eq!(
        cost_per_energy.0[7],
        Rational::ZERO,
        "Money·Energy slot 7 (Angle) should be ZERO"
    );
}

/// The content hashes of Torque and Energy must differ so that any future
/// change to the hash-buffer layout cannot silently conflate the two dimensions.
///
/// Complements `torque_dim_differs_from_energy_dim_via_angle_slot` at the
/// hash layer: if the content_hash incorrectly encodes slot 7, then dimension
/// type-checking built on hashes would mis-identify torque as energy even
/// though `DimensionVector::eq` would still be correct.
#[test]
fn torque_dim_content_hash_differs_from_energy_dim_content_hash() {
    let torque = DimensionVector::FORCE
        .mul(&DimensionVector::LENGTH)
        .div(&DimensionVector::ANGLE);
    let energy = DimensionVector::FORCE.mul(&DimensionVector::LENGTH);
    assert_ne!(
        torque.content_hash(),
        energy.content_hash(),
        "Torque and Energy content hashes must differ"
    );
}

// ─── C5: USD literal through stdlib path — slot 9 = 1, slot 7 = 0 ───────────

/// Compile `structure def S { param x : Money = 25USD }` with the full stdlib
/// prelude (which includes `pub unit USD : Money` from task 2378) and assert
/// that the resolved Scalar dimension has slot 9 = ONE and every other slot —
/// especially Angle slot 7 — = ZERO.
///
/// Locks the 2378→2379→2383 chain: if the stdlib USD declaration ever drifts
/// out of the MONEY basis vector, this test will catch it before it reaches
/// downstream code.
#[test]
fn usd_via_stdlib_prelude_resolves_with_only_money_slot_set() {
    let (si, dim) = stdlib_param_si_value("Money", "25USD");
    assert!(
        (si - 25.0).abs() < UNIT_EPSILON,
        "25USD si_value should be 25.0, got {}",
        si
    );
    assert_eq!(dim.0[9], Rational::ONE, "slot 9 (Money) should be ONE");
    assert_eq!(dim.0[7], Rational::ZERO, "slot 7 (Angle) should be ZERO");
    for i in [0usize, 1, 2, 3, 4, 5, 6, 8] {
        assert_eq!(
            dim.0[i],
            Rational::ZERO,
            "slot {} should be ZERO for USD dimension",
            i
        );
    }
}

/// Using the inline `pub unit USD : Money` declaration (hermetic, no stdlib),
/// compile `25USD/1kg` in a `CostPerMass` param and assert the cell's
/// `cell_type` carries a Scalar dimension whose Angle slot 7 is ZERO.
///
/// This mirrors the hermeticity convention from `money_arithmetic_tests.rs`
/// (inline seed, not prelude-dependent) and confirms the compile pipeline
/// does not introduce a spurious Angle exponent in the compound type.
#[test]
fn inline_usd_decl_compound_via_inline_pattern_keeps_angle_slot_zero() {
    let source = "pub unit USD : Money\n\
                  type CostPerMass = Money / Mass\n\
                  structure def S { param p : CostPerMass = 25USD/1kg }";
    let module = compile_source(source);
    let errs = errors_only(&module);
    assert!(errs.is_empty(), "unexpected compile errors: {:?}", errs);

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template 'S' not found");
    let cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "p")
        .expect("cell 'p' not found");

    match &cell.cell_type {
        Type::Scalar { dimension } => {
            assert_eq!(
                dimension.0[7],
                Rational::ZERO,
                "Angle slot 7 should be ZERO in CostPerMass (Money/Mass) cell_type"
            );
        }
        other => panic!("expected Type::Scalar {{ dimension }}, got {:?}", other),
    }
}
