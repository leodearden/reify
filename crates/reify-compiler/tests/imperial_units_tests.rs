//! Tests for imperial unit declarations in stdlib/units.ri (task 335).
//!
//! Covers: yd (Length), lbf (Force), psi/ksi (Pressure), fl_oz/gal (Volume),
//! plus regression guards for the four pre-existing imperial units
//! (ft, thou, lb, oz) and a cross-unit arithmetic check (lbf * mm → Energy).

mod common;

use common::{
    assert_eq_rel, assert_simple_unit, compile_with_stdlib_helper, stdlib_param_si_value,
    units_module,
};
use reify_core::{DimensionVector, Severity};

// ─── step-1/2: yd — Length ────────────────────────────────────────────────────

#[test]
fn stdlib_yd_resolves_to_length_0p9144() {
    let (v, d) = stdlib_param_si_value("Length", "1yd");
    assert_eq_rel(v, 0.9144, 1e-12, "1yd should be 0.9144 m");
    assert_eq!(d, DimensionVector::LENGTH);
}

#[test]
fn stdlib_units_module_contains_yd_with_no_offset() {
    assert_simple_unit("yd", DimensionVector::LENGTH, 0.9144, 1e-12);
}

// ─── step-3/4: lbf — Force ────────────────────────────────────────────────────

/// lbf = 0.45359237 kg × 9.80665 m/s² = 4.4482216152605 N (exact)
const LBF_SI: f64 = 4.4482216152605;

#[test]
fn stdlib_lbf_resolves_to_force_4p4482216152605() {
    let (v, d) = stdlib_param_si_value("Force", "1lbf");
    assert_eq_rel(v, LBF_SI, 1e-9, "1lbf should match LBF_SI");
    assert_eq!(d, DimensionVector::FORCE);
}

#[test]
fn stdlib_units_module_contains_lbf_with_force_dimension() {
    assert_simple_unit("lbf", DimensionVector::FORCE, LBF_SI, 1e-9);
}

// ─── step-5/6: psi and ksi — Pressure ────────────────────────────────────────

/// psi = lbf / in² = 4.4482216152605 / 0.00064516 Pa (exact)
const PSI_SI: f64 = 6894.757293168361;
/// ksi = 1000 × psi
const KSI_SI: f64 = 6894757.293168361;

#[test]
fn stdlib_psi_resolves_to_pressure_6894p757293168361() {
    let (v, d) = stdlib_param_si_value("Pressure", "1psi");
    assert_eq_rel(v, PSI_SI, 1e-12, "1psi should match PSI_SI");
    assert_eq!(d, DimensionVector::PRESSURE);
}

#[test]
fn stdlib_ksi_resolves_to_pressure_6894757p293168361() {
    let (v, d) = stdlib_param_si_value("Pressure", "1ksi");
    assert_eq_rel(v, KSI_SI, 1e-12, "1ksi should match KSI_SI");
    assert_eq!(d, DimensionVector::PRESSURE);
}

#[test]
fn stdlib_units_module_contains_psi_and_ksi_with_pressure_dimension() {
    assert_simple_unit("psi", DimensionVector::PRESSURE, PSI_SI, 1e-9);
    assert_simple_unit("ksi", DimensionVector::PRESSURE, KSI_SI, 1e-9);
}

// ─── step-7/8: fl_oz and gal — Volume (US customary) ─────────────────────────

/// US fl_oz = 231/128 in³ = 29.5735295625 mL exactly
const FL_OZ_SI: f64 = 0.0000295735295625;
/// US liquid gallon = 231 in³ exactly
const GAL_SI: f64 = 0.003785411784;

#[test]
fn stdlib_fl_oz_resolves_to_volume_2p9573e_minus5() {
    let (v, d) = stdlib_param_si_value("Volume", "1fl_oz");
    assert_eq_rel(v, FL_OZ_SI, 1e-12, "1fl_oz should match FL_OZ_SI");
    assert_eq!(d, DimensionVector::VOLUME);
}

#[test]
fn stdlib_gal_resolves_to_volume_3p785e_minus3() {
    let (v, d) = stdlib_param_si_value("Volume", "1gal");
    assert_eq_rel(v, GAL_SI, 1e-12, "1gal should match GAL_SI");
    assert_eq!(d, DimensionVector::VOLUME);
}

#[test]
fn stdlib_units_module_contains_fl_oz_and_gal_with_volume_dimension() {
    assert_simple_unit("fl_oz", DimensionVector::VOLUME, FL_OZ_SI, 1e-9);
    assert_simple_unit("gal", DimensionVector::VOLUME, GAL_SI, 1e-9);
}

// ─── cross-relationships: dimensional identities across imperial units ────────

#[test]
fn cross_relationships_between_imperial_units() {
    // 1 yd == 3 ft
    let (yd_si, _) = stdlib_param_si_value("Length", "1yd");
    let (ft3_si, _) = stdlib_param_si_value("Length", "3ft");
    assert_eq_rel(yd_si, ft3_si, 1e-12, "1yd should equal 3ft in SI");

    // 1 ksi == 1000 psi
    let (ksi_si, _) = stdlib_param_si_value("Pressure", "1ksi");
    let (psi1000_si, _) = stdlib_param_si_value("Pressure", "1000psi");
    assert_eq_rel(ksi_si, psi1000_si, 1e-12, "1ksi should equal 1000psi in SI");

    // 1 gal == 128 fl_oz
    let (gal_si, _) = stdlib_param_si_value("Volume", "1gal");
    let (fl_oz128_si, _) = stdlib_param_si_value("Volume", "128fl_oz");
    assert_eq_rel(
        gal_si,
        fl_oz128_si,
        1e-12,
        "1gal should equal 128fl_oz in SI",
    );

    // 1 psi == 1 lbf / (1 in * 1 in)
    // Compound Reify arithmetic compiles to BinOp (not a Literal), so compute
    // the RHS in Rust from separately compiled SI values.
    let (psi_si, _) = stdlib_param_si_value("Pressure", "1psi");
    let (lbf_si, _) = stdlib_param_si_value("Force", "1lbf");
    let (in_si, _) = stdlib_param_si_value("Length", "1in");
    let psi_from_lbf_in2 = lbf_si / (in_si * in_si);
    assert_eq_rel(
        psi_si,
        psi_from_lbf_in2,
        1e-12,
        "1psi should equal 1lbf/(1in²) in SI",
    );
}

// ─── step-9: cross-unit arithmetic lbf * mm → Energy ─────────────────────────

#[test]
fn lbf_times_mm_produces_energy_dimension_via_stdlib() {
    // `2lbf * 3mm` compiles to a BinOp whose result_type carries FORCE×LENGTH = ENERGY.
    let source = "structure def S { param e : Energy = 2lbf * 3mm }";
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
        .find(|c| c.id.member == "e")
        .expect("e cell not found");

    // The declared type must be Energy's dimension.
    assert_eq!(
        cell.cell_type,
        reify_core::Type::Scalar {
            dimension: DimensionVector::ENERGY
        },
        "cell_type should be Scalar{{ENERGY}}"
    );

    let expr = cell.default_expr.as_ref().expect("e has no default_expr");

    // BinOp result type must be Force × Length = Energy.
    let expected_dim = DimensionVector::FORCE.mul(&DimensionVector::LENGTH);
    assert_eq!(
        expr.result_type,
        reify_core::Type::Scalar {
            dimension: expected_dim
        },
        "BinOp result_type should be Scalar{{FORCE×LENGTH}}, got {:?}",
        expr.result_type
    );

    // The outer expression must be a Multiply BinOp.
    match &expr.kind {
        reify_ir::CompiledExprKind::BinOp {
            op: reify_ir::BinOp::Mul,
            left,
            right,
        } => {
            // Left operand: 2lbf → Scalar with FORCE dimension, si_value ≈ 2 * LBF_SI
            let left_si = match &left.kind {
                reify_ir::CompiledExprKind::Literal(reify_ir::Value::Scalar {
                    si_value,
                    dimension,
                    ..
                }) => {
                    assert_eq!(
                        *dimension,
                        DimensionVector::FORCE,
                        "left dim should be FORCE"
                    );
                    let expected = 2.0 * LBF_SI;
                    assert!(
                        (si_value - expected).abs() < 1e-9,
                        "left si_value should be {} (2×lbf), got {}",
                        expected,
                        si_value
                    );
                    *si_value
                }
                other => panic!("left operand should be Literal(Scalar), got {:?}", other),
            };
            // Right operand: 3mm → Scalar with LENGTH dimension, si_value = 0.003
            let right_si = match &right.kind {
                reify_ir::CompiledExprKind::Literal(reify_ir::Value::Scalar {
                    si_value,
                    dimension,
                    ..
                }) => {
                    assert_eq!(
                        *dimension,
                        DimensionVector::LENGTH,
                        "right dim should be LENGTH"
                    );
                    assert!(
                        (si_value - 0.003).abs() < 1e-12,
                        "right si_value should be 0.003 (3mm), got {}",
                        si_value
                    );
                    *si_value
                }
                other => panic!("right operand should be Literal(Scalar), got {:?}", other),
            };
            // Product check: operand SI values must multiply to the expected energy.
            // Guards against a class of bug where dimension math is right but the
            // scaling factors are not multiplied through correctly.
            let energy_si = left_si * right_si;
            let expected_energy = 2.0 * LBF_SI * 0.003;
            assert!(
                (energy_si - expected_energy).abs() < 1e-9,
                "2lbf * 3mm energy should be {} J, got {}",
                expected_energy,
                energy_si
            );
        }
        other => panic!("expected BinOp{{Mul, _, _}}, got {:?}", other),
    }
}

// ─── step-10: regression guard for pre-existing imperial units ────────────────

#[test]
fn existing_imperial_units_ft_thou_in_lb_oz_unchanged_post_task_335() {
    // Verify the four pre-existing imperial unit factors and dimensions
    // are unchanged by this task's edits to units.ri.

    // Length units (SI base: metre)
    let length_cases: &[(&str, f64)] = &[("1ft", 0.3048), ("1thou", 0.0000254), ("1in", 0.0254)];
    for (literal, expected) in length_cases {
        let (v, d) = stdlib_param_si_value("Length", literal);
        assert_eq_rel(
            v,
            *expected,
            1e-12,
            &format!("{} should be {} m", literal, expected),
        );
        assert_eq!(d, DimensionVector::LENGTH, "{} dimension wrong", literal);
    }

    // Mass units (SI base: kilogram)
    let mass_cases: &[(&str, f64)] = &[("1lb", 0.45359237), ("1oz", 0.028349523125)];
    for (literal, expected) in mass_cases {
        let (v, d) = stdlib_param_si_value("Mass", literal);
        assert_eq_rel(
            v,
            *expected,
            1e-12,
            &format!("{} should be {} kg", literal, expected),
        );
        assert_eq!(d, DimensionVector::MASS, "{} dimension wrong", literal);
    }

    // Verify offset=None for all four via the stdlib module units list.
    let units_module = units_module();

    for name in &["ft", "thou", "in", "lb", "oz"] {
        let u = units_module
            .units
            .iter()
            .find(|u| u.name == *name)
            .unwrap_or_else(|| panic!("unit '{}' not found in std/units", name));
        assert!(u.offset.is_none(), "unit '{}' should have no offset", name);
    }
}
