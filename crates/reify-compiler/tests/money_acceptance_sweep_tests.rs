//! Compile-pipeline acceptance tests for the Money dimension: stdlib-prelude
//! `USD` resolution and inline `pub unit USD : Money` declaration.

mod common;

use common::{UNIT_EPSILON, stdlib_param_si_value};
use reify_test_support::{compile_source, errors_only};
use reify_types::{DimensionVector, Rational, Type};

// ─── USD literal through stdlib path ─────────────────────────────────────────

/// Compile `structure def S { param x : Money = 25USD }` with the full stdlib
/// prelude (which includes `pub unit USD : Money`) and assert that the resolved
/// Scalar dimension has slot 9 = ONE and every other slot — especially Angle
/// slot 7 — = ZERO.
///
/// Locks the stdlib integration: if the stdlib USD declaration ever drifts
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
/// `cell_type` carries a Scalar dimension equal to `Money / Mass`.
///
/// This mirrors the hermeticity convention from `money_arithmetic_tests.rs`
/// (inline seed, not prelude-dependent) and confirms the compile pipeline
/// produces the correct compound dimension with no spurious slot contamination.
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
                *dimension,
                DimensionVector::MONEY.div(&DimensionVector::MASS),
                "CostPerMass cell_type dimension should equal Money/Mass"
            );
        }
        other => panic!("expected Type::Scalar {{ dimension }}, got {:?}", other),
    }
}
