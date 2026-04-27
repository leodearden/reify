//! Tests for Money unit declarations in stdlib/units.ri (task 2378).
//!
//! Covers: USD (Money dimension base unit, factor = 1.0, no offset),
//! USD quantity literal resolution (25USD → Scalar{si_value: 25.0, MONEY}),
//! and USD compound arithmetic (25USD/1kg → BinOp with MONEY/MASS result).
//!
//! All five tests fail before step-2 adds `pub unit USD : Money` to units.ri,
//! then pass without any additional Rust code changes.

mod common;

use common::compile_with_stdlib_helper;
use reify_compiler::{CompiledModule, stdlib_loader};
use reify_types::{DimensionVector, Severity};

// ─── local helpers ─────────────────────────────────────────────────────────────

/// Return the compiled `std/units` module from the cached stdlib.
///
/// Uses the `OnceLock`-backed `load_stdlib()` so repeated calls are free.
fn units_module() -> &'static CompiledModule {
    stdlib_loader::load_stdlib()
        .iter()
        .find(|m| format!("{}", m.path) == "std/units")
        .expect("std/units module not found")
}

/// Assert `a ≈ b` within `max(|a|, |b|) * rel_tol`.
///
/// Using the larger magnitude as the scale makes the tolerance robust when one
/// operand is zero.
fn assert_eq_rel(a: f64, b: f64, rel_tol: f64, msg: &str) {
    let scale = a.abs().max(b.abs());
    let tol = if scale == 0.0 { rel_tol } else { scale * rel_tol };
    assert!(
        (a - b).abs() < tol,
        "{}: expected {} ≈ {} (tol {})",
        msg,
        a,
        b,
        tol
    );
}

/// Assert that a named unit in `std/units` has the expected dimension, factor
/// (within `rel_tol` relative tolerance), and no offset.
fn assert_simple_unit(
    name: &str,
    expected_dim: DimensionVector,
    expected_factor: f64,
    rel_tol: f64,
) {
    let module = units_module();
    let u = module
        .units
        .iter()
        .find(|u| u.name == name)
        .unwrap_or_else(|| panic!("unit '{}' not found in std/units", name));
    assert_eq!(u.dimension, expected_dim, "unit '{}' dimension wrong", name);
    assert_eq_rel(
        u.factor,
        expected_factor,
        rel_tol,
        &format!("unit '{}' factor", name),
    );
    assert!(u.offset.is_none(), "unit '{}' should have no offset", name);
}

/// Compile a structure with a single default-valued param and return the
/// Scalar's (si_value, dimension) from its default expression.
///
/// Source compiled: `structure def S { param x : <param_type> = <literal> }`
fn stdlib_param_si_value(param_type: &str, literal: &str) -> (f64, DimensionVector) {
    let source = format!(
        "structure def S {{ param x : {} = {} }}",
        param_type, literal
    );
    let module = compile_with_stdlib_helper(&source);
    let errs: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errs.is_empty(),
        "source `{}` produced errors: {:?}",
        source,
        errs
    );
    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S template not found");
    let cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "x")
        .expect("x cell not found");
    let expr = cell.default_expr.as_ref().expect("x has no default_expr");
    if let reify_types::CompiledExprKind::Literal(reify_types::Value::Scalar {
        si_value,
        dimension,
    }) = &expr.kind
    {
        (*si_value, *dimension)
    } else {
        panic!("unexpected expression kind: {:?}", expr.kind);
    }
}

// ─── (a) USD metadata: dimension = MONEY, factor ≈ 1.0, no offset ────────────

#[test]
fn stdlib_units_module_contains_USD_with_money_dimension() {
    assert_simple_unit("USD", DimensionVector::MONEY, 1.0, 1e-12);
}

// ─── (b) USD visibility: is_pub == true ──────────────────────────────────────

#[test]
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
fn stdlib_USD_quantity_literal_resolves_to_money_scalar() {
    let (v, d) = stdlib_param_si_value("Money", "25USD");
    assert_eq_rel(v, 25.0, 1e-12, "25USD should be 25.0 in SI (Money)");
    assert_eq!(d, DimensionVector::MONEY, "25USD should have MONEY dimension");
}

// ─── (d) USD/kg compound resolves to Money/Mass ──────────────────────────────

#[test]
fn stdlib_USD_per_kg_compound_resolves_to_money_per_mass() {
    let source = "structure def S { param p : Money/Mass = 25USD/1kg }";
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
        reify_types::Type::Scalar {
            dimension: expected_dim
        },
        "cell_type should be Scalar{{MONEY/MASS}}"
    );

    let expr = cell.default_expr.as_ref().expect("p has no default_expr");

    // BinOp result type must be Money / Mass.
    assert_eq!(
        expr.result_type,
        reify_types::Type::Scalar {
            dimension: expected_dim
        },
        "BinOp result_type should be Scalar{{MONEY/MASS}}, got {:?}",
        expr.result_type
    );

    // The outer expression must be a Divide BinOp.
    match &expr.kind {
        reify_types::CompiledExprKind::BinOp {
            op: reify_types::BinOp::Div,
            left,
            right,
        } => {
            // Left operand: 25USD → Scalar with MONEY dimension, si_value = 25.0
            let left_si = match &left.kind {
                reify_types::CompiledExprKind::Literal(reify_types::Value::Scalar {
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
                    *si_value
                }
                other => panic!("left operand should be Literal(Scalar), got {:?}", other),
            };
            // Right operand: 1kg → Scalar with MASS dimension, si_value = 1.0
            let right_si = match &right.kind {
                reify_types::CompiledExprKind::Literal(reify_types::Value::Scalar {
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
                    *si_value
                }
                other => panic!("right operand should be Literal(Scalar), got {:?}", other),
            };
            // Division check: left_si / right_si = 25.0 / 1.0 = 25.0
            let ratio = left_si / right_si;
            assert!(
                (ratio - 25.0).abs() < 1e-12,
                "25USD/1kg operand ratio should be 25.0, got {}",
                ratio
            );
        }
        other => panic!("expected BinOp{{Div, _, _}}, got {:?}", other),
    }
}

// ─── (e) USD existence canary ─────────────────────────────────────────────────

#[test]
fn stdlib_units_count_grows_to_include_USD() {
    let module = units_module();
    assert!(
        module.units.iter().any(|u| u.name == "USD"),
        "std/units should contain a unit named 'USD'; found units: {:?}",
        module.units.iter().map(|u| &u.name).collect::<Vec<_>>()
    );
}
