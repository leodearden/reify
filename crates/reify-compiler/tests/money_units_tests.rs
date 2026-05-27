//! Tests for Money unit declarations in stdlib/units.ri (task 2378).
//!
//! Covers: USD (Money dimension base unit, factor = 1.0, no offset),
//! USD quantity literal resolution (25USD → Scalar{si_value: 25.0, MONEY}),
//! and USD compound arithmetic (25USD/1kg → BinOp with MONEY/MASS result).
//!
//! All four tests fail before step-2 adds `pub unit USD : Money` to units.ri,
//! then pass without any additional Rust code changes.

mod common;

use common::{
    assert_eq_rel, assert_simple_unit, compile_with_stdlib_helper, stdlib_param_si_value,
    units_module,
};
use reify_core::{DimensionVector, Severity};

// ─── (a) USD metadata: dimension = MONEY, factor ≈ 1.0, no offset ────────────

#[test]
#[allow(non_snake_case)]
fn stdlib_units_module_contains_USD_with_money_dimension() {
    assert_simple_unit("USD", DimensionVector::MONEY, 1.0, 1e-12);
}

// ─── (b) USD visibility: is_pub == true ──────────────────────────────────────

#[test]
#[allow(non_snake_case)]
fn stdlib_USD_is_publicly_visible_in_prelude() {
    let module = units_module();
    let u = module
        .units
        .iter()
        .find(|u| u.name == "USD")
        .unwrap_or_else(|| panic!("unit 'USD' not found in std/units"));
    assert!(
        u.is_pub,
        "USD must be declared `pub` so it seeds the prelude registry; got is_pub = false"
    );
}

// ─── (c) USD quantity literal resolves to Money scalar ───────────────────────

#[test]
#[allow(non_snake_case)]
fn stdlib_USD_quantity_literal_resolves_to_money_scalar() {
    let (v, d) = stdlib_param_si_value("Money", "25USD");
    assert_eq_rel(v, 25.0, 1e-12, "25USD should be 25.0 in SI (Money)");
    assert_eq!(
        d,
        DimensionVector::MONEY,
        "25USD should have MONEY dimension"
    );
}

// ─── (d) USD/kg compound resolves to Money/Mass ──────────────────────────────

#[test]
#[allow(non_snake_case)]
fn stdlib_USD_per_kg_compound_resolves_to_money_per_mass() {
    // The parser only accepts named types in param declarations, not inline
    // DimensionalOp expressions like `Money/Mass`. Define a top-level type
    // alias and use it as the param type — the established pattern from
    // type_alias_compile_tests.rs::dimensional_alias_force_div_area.
    let source =
        "type CostPerMass = Money / Mass\nstructure def S { param p : CostPerMass = 25USD/1kg }";
    let module = compile_with_stdlib_helper(source);

    let errs: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errs.is_empty(), "unexpected errors: {:?}", errs);

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S template not found");
    let cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "p")
        .expect("p cell not found");

    let expected_dim = DimensionVector::MONEY.div(&DimensionVector::MASS);
    assert_eq!(
        cell.cell_type,
        reify_core::Type::Scalar {
            dimension: expected_dim
        },
        "cell_type should be Scalar{{MONEY/MASS}}"
    );

    let expr = cell.default_expr.as_ref().expect("p has no default_expr");

    // BinOp result type must be Money / Mass.
    assert_eq!(
        expr.result_type,
        reify_core::Type::Scalar {
            dimension: expected_dim
        },
        "BinOp result_type should be Scalar{{MONEY/MASS}}, got {:?}",
        expr.result_type
    );

    // The outer expression must be a Divide BinOp.
    match &expr.kind {
        reify_ir::CompiledExprKind::BinOp {
            op: reify_ir::BinOp::Div,
            left,
            right,
        } => {
            // Left operand: 25USD → Scalar with MONEY dimension, si_value = 25.0
            match &left.kind {
                reify_ir::CompiledExprKind::Literal(reify_ir::Value::Scalar {
                    si_value,
                    dimension,
                }) => {
                    assert_eq!(
                        *dimension,
                        DimensionVector::MONEY,
                        "left operand dim should be MONEY"
                    );
                    assert!(
                        (si_value - 25.0).abs() < 1e-12,
                        "left si_value should be 25.0 (25USD), got {}",
                        si_value
                    );
                }
                other => panic!("left operand should be Literal(Scalar), got {:?}", other),
            }
            // Right operand: 1kg → Scalar with MASS dimension, si_value = 1.0
            match &right.kind {
                reify_ir::CompiledExprKind::Literal(reify_ir::Value::Scalar {
                    si_value,
                    dimension,
                }) => {
                    assert_eq!(
                        *dimension,
                        DimensionVector::MASS,
                        "right operand dim should be MASS"
                    );
                    assert!(
                        (si_value - 1.0).abs() < 1e-12,
                        "right si_value should be 1.0 (1kg), got {}",
                        si_value
                    );
                }
                other => panic!("right operand should be Literal(Scalar), got {:?}", other),
            }
        }
        other => panic!("expected BinOp{{Div, _, _}}, got {:?}", other),
    }
}
