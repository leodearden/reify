//! Acceptance tests for user-defined unit declarations (task 210).
//!
//! Focused acceptance suite covering behaviors NOT already exercised by
//! `unit_registry_tests.rs` (granular registry internals, 60+ tests) or
//! `user_defined_unit_tests.rs` (cross-module integration, ModuleDag,
//! compile_project). Duplicate coverage was consolidated in task 1420.
//!
//! Categories covered:
//!   1. Basic declaration                    (test 1)
//!   2. Offset unit / degC                   (test 1)
//!   3. Conversion factor arithmetic         (tests 2)
//!   4. User unit in quantity literal → SI   (test 1, multi-unit)
//!   5. Unit with compound dimension         (tests 2)

use reify_test_support::{compile_source, compile_source_with_stdlib, errors_only};
use reify_core::DimensionVector;

// ─── category 1: basic unit declaration ───────────────────────────────────────

#[test]
fn custom_unit_declaration_registers_name_dimension_factor() {
    // Declare a simple length unit with an explicit SI factor.
    // The compiled module's unit list must contain the entry with the correct
    // name, dimension, factor, and no offset.
    let module = compile_source("unit thou : Length = 0.0000254");
    let errors = errors_only(&module);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
    let unit = module
        .units
        .iter()
        .find(|u| u.name == "thou")
        .expect("unit 'thou' not found in module.units");
    assert_eq!(
        unit.dimension,
        DimensionVector::LENGTH,
        "dimension should be LENGTH"
    );
    assert!(
        (unit.factor - 0.0000254).abs() < 1e-12,
        "factor should be ≈0.0000254, got {}",
        unit.factor
    );
    assert!(unit.offset.is_none(), "simple unit should have no offset");
}

// ─── category 2: offset unit (degC) ───────────────────────────────────────────

#[test]
fn offset_unit_degc_compiles_with_factor_and_offset() {
    // The canonical degC declaration: factor=1 (1:1 Kelvin scale) plus additive
    // offset. The compiled entry must have dimension=TEMPERATURE, factor≈1.0,
    // offset=Some(273.15).
    //
    // Also verifies the explicit `= 1 offset X` form round-trips through a
    // quantity literal (unit_registry_tests.rs::offset_unit_quantity_literal_applies_offset
    // covers the short `offset X` form; this exercises the explicit-factor path).
    let module = compile_source(
        "unit degC : Temperature = 1 offset 273.15\n\
         structure S { param t : Temperature = 5degC }",
    );
    let errors = errors_only(&module);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
    let unit = module
        .units
        .iter()
        .find(|u| u.name == "degC")
        .expect("unit 'degC' not found");
    assert_eq!(
        unit.dimension,
        DimensionVector::TEMPERATURE,
        "dimension should be TEMPERATURE"
    );
    assert!(
        (unit.factor - 1.0).abs() < 1e-12,
        "degC factor should be ≈1.0, got {}",
        unit.factor
    );
    let off = unit.offset.expect("degC should have Some(offset)");
    assert!(
        (off - 273.15).abs() < 1e-9,
        "degC offset should be ≈273.15, got {}",
        off
    );

    // Quantity-literal round-trip: 5degC → 5 * 1.0 + 273.15 = 278.15 K
    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template 'S' not found");
    let t_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "t")
        .expect("value cell 't' not found");
    if let Some(expr) = &t_cell.default_expr {
        if let reify_ir::CompiledExprKind::Literal(reify_ir::Value::Scalar {
            si_value, ..
        }) = &expr.kind
        {
            assert!(
                (si_value - 278.15).abs() < 1e-9,
                "5degC should be ≈278.15 K (5 * 1.0 + 273.15), got {}",
                si_value
            );
        } else {
            panic!("expected scalar literal for 't', got {:?}", expr.kind);
        }
    } else {
        panic!("value cell 't' has no default_expr");
    }
}

// ─── category 3: conversion factor arithmetic ─────────────────────────────────

#[test]
fn conversion_factor_arithmetic_multiplication_correct() {
    // factor = 25.4 * 0.001 = 0.0254  (BinOp Multiply in evaluate_const_expr).
    let module = compile_source("unit inch : Length = 25.4 * 0.001");
    let errors = errors_only(&module);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
    let unit = module
        .units
        .iter()
        .find(|u| u.name == "inch")
        .expect("unit 'inch' not found");
    assert!(
        (unit.factor - 0.0254).abs() < 1e-12,
        "inch factor should be ≈0.0254 (25.4 * 0.001), got {}",
        unit.factor
    );
}

#[test]
fn conversion_factor_division_correct() {
    // factor = 1 / 1000 = 0.001  (BinOp Divide in evaluate_const_expr).
    let module = compile_source("unit milli : Length = 1 / 1000");
    let errors = errors_only(&module);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
    let unit = module
        .units
        .iter()
        .find(|u| u.name == "milli")
        .expect("unit 'milli' not found");
    assert!(
        (unit.factor - 0.001).abs() < 1e-12,
        "milli factor should be ≈0.001 (1/1000), got {}",
        unit.factor
    );
}

// ─── category 4: user unit in quantity literal → SI (multi-unit) ──────────────

#[test]
fn user_unit_in_quantity_literal_evaluates_to_si_value_multiple_units() {
    // Two user units in the same module, each used in a separate param.
    // km → 2000 m,  ms → 0.5 s.
    let module = compile_source(
        "unit km : Length = 1000\n\
         unit ms : Time = 0.001\n\
         structure S {\n\
             param d : Length = 2km\n\
             param t : Time = 500ms\n\
         }",
    );
    let errors = errors_only(&module);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template 'S' not found");

    let d_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "d")
        .expect("value cell 'd' not found");
    if let Some(expr) = &d_cell.default_expr {
        if let reify_ir::CompiledExprKind::Literal(reify_ir::Value::Scalar {
            si_value, ..
        }) = &expr.kind
        {
            assert!(
                (si_value - 2000.0).abs() < 1e-9,
                "2km should be ≈2000.0 m, got {}",
                si_value
            );
        } else {
            panic!("expected scalar literal for 'd', got {:?}", expr.kind);
        }
    } else {
        panic!("value cell 'd' has no default_expr");
    }

    let t_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "t")
        .expect("value cell 't' not found");
    if let Some(expr) = &t_cell.default_expr {
        if let reify_ir::CompiledExprKind::Literal(reify_ir::Value::Scalar {
            si_value, ..
        }) = &expr.kind
        {
            assert!(
                (si_value - 0.5).abs() < 1e-9,
                "500ms should be ≈0.5 s, got {}",
                si_value
            );
        } else {
            panic!("expected scalar literal for 't', got {:?}", expr.kind);
        }
    } else {
        panic!("value cell 't' has no default_expr");
    }
}

// ─── category 5: unit with compound dimension ─────────────────────────────────

#[test]
fn custom_unit_with_compound_dimension_force() {
    // kN: Force = kg·m·s⁻² × 1000.
    // Verifies that resolve_dimension_type handles the compound FORCE dimension
    // and that the conversion factor is stored correctly.
    let module = compile_source("unit kN : Force = 1000");
    let errors = errors_only(&module);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
    let unit = module
        .units
        .iter()
        .find(|u| u.name == "kN")
        .expect("unit 'kN' not found");
    assert_eq!(
        unit.dimension,
        reify_core::dimension::FORCE,
        "kN should have FORCE dimension"
    );
    assert!(
        (unit.factor - 1000.0).abs() < 1e-9,
        "kN factor should be ≈1000, got {}",
        unit.factor
    );
    assert!(unit.offset.is_none(), "kN should have no offset");
}

#[test]
fn negative_factor_unit_in_quantity_literal() {
    // unit neg_m : Length = 0 - 0.5  → factor = -0.5.
    // 10neg_m → si_value = 10 * (-0.5) = -5.0 m.
    // unit_registry_tests.rs::valid_negative_factor_still_compiles verifies the
    // negative factor is registered, but no existing test verifies it is applied
    // correctly in a runtime quantity literal.
    let module = compile_source(
        "unit neg_m : Length = 0 - 0.5\n\
         structure S { param x : Length = 10neg_m }",
    );
    let errors = errors_only(&module);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    let unit = module
        .units
        .iter()
        .find(|u| u.name == "neg_m")
        .expect("unit 'neg_m' not found in module.units");
    assert!(
        (unit.factor - (-0.5)).abs() < 1e-12,
        "neg_m factor should be ≈-0.5, got {}",
        unit.factor
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template 'S' not found");
    let x_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "x")
        .expect("value cell 'x' not found");
    if let Some(expr) = &x_cell.default_expr {
        if let reify_ir::CompiledExprKind::Literal(reify_ir::Value::Scalar {
            si_value,
            dimension,
            ..
        }) = &expr.kind
        {
            assert!(
                (si_value - (-5.0)).abs() < 1e-12,
                "10neg_m should be ≈-5.0 m, got {}",
                si_value
            );
            assert_eq!(
                *dimension,
                DimensionVector::LENGTH,
                "10neg_m quantity literal should carry LENGTH dimension"
            );
        } else {
            panic!("expected scalar literal for 'x', got {:?}", expr.kind);
        }
    } else {
        panic!("value cell 'x' has no default_expr");
    }
}

#[test]
fn compound_dimension_volume_unit_in_quantity_literal() {
    // mm3: Volume = 1e-9 m³;  5mm3 → si_value = 5 * 1e-9 = 5e-9 m³.
    // Covers the Volume compound dimension in a quantity literal — not exercised
    // by any existing test in unit_registry_tests.rs or unit_declaration_tests.rs.
    let module = compile_source(
        "unit mm3 : Volume = 0.000000001\n\
         structure S { param v : Volume = 5mm3 }",
    );
    let errors = errors_only(&module);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    // Unit-table entry
    let unit = module
        .units
        .iter()
        .find(|u| u.name == "mm3")
        .expect("unit 'mm3' not found in module.units");
    assert_eq!(
        unit.dimension,
        DimensionVector::VOLUME,
        "mm3 should have VOLUME dimension"
    );

    // Quantity-literal scalar value
    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template 'S' not found");
    let v_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "v")
        .expect("value cell 'v' not found");
    if let Some(expr) = &v_cell.default_expr {
        if let reify_ir::CompiledExprKind::Literal(reify_ir::Value::Scalar {
            si_value,
            dimension,
            ..
        }) = &expr.kind
        {
            assert!(
                (si_value - 5e-9).abs() < 1e-15,
                "5mm3 should be ≈5e-9 m³, got {}",
                si_value
            );
            assert_eq!(
                *dimension,
                DimensionVector::VOLUME,
                "5mm3 quantity literal should carry VOLUME dimension"
            );
        } else {
            panic!("expected scalar literal for 'v', got {:?}", expr.kind);
        }
    } else {
        panic!("value cell 'v' has no default_expr");
    }
}

#[test]
fn custom_unit_with_compound_dimension_force_in_quantity_literal() {
    // kN: Force = 1000 N (SI = kg·m·s⁻²);  2kN → si_value = 2 * 1000 = 2000 N.
    // Extends custom_unit_with_compound_dimension_force (which only checks the
    // unit-table entry) by verifying the full quantity-literal-to-scalar pipeline
    // including the dimension carried by the compiled scalar value.
    let module = compile_source(
        "unit kN : Force = 1000\n\
         structure S { param f : Force = 2kN }",
    );
    let errors = errors_only(&module);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template 'S' not found");
    let f_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "f")
        .expect("value cell 'f' not found");
    if let Some(expr) = &f_cell.default_expr {
        if let reify_ir::CompiledExprKind::Literal(reify_ir::Value::Scalar {
            si_value,
            dimension,
            ..
        }) = &expr.kind
        {
            assert!(
                (si_value - 2000.0).abs() < 1e-9,
                "2kN should be ≈2000.0 N (SI), got {}",
                si_value
            );
            assert_eq!(
                *dimension,
                reify_core::dimension::FORCE,
                "2kN quantity literal should carry FORCE dimension"
            );
        } else {
            panic!("expected scalar literal for 'f', got {:?}", expr.kind);
        }
    } else {
        panic!("value cell 'f' has no default_expr");
    }
}

#[test]
fn custom_unit_with_compound_dimension_area_in_quantity_literal() {
    // cm2: Area = 0.0001 m²;  50cm2 → si_value = 50 * 0.0001 = 0.005 m².
    // Exercises the full pipeline for a compound-dimension custom unit used in
    // a quantity literal.
    let module = compile_source(
        "unit cm2 : Area = 0.0001\n\
         structure S { param a : Area = 50cm2 }",
    );
    let errors = errors_only(&module);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    let unit = module
        .units
        .iter()
        .find(|u| u.name == "cm2")
        .expect("unit 'cm2' not found");
    assert_eq!(
        unit.dimension,
        DimensionVector::AREA,
        "cm2 should have AREA dimension"
    );
    assert!(
        (unit.factor - 0.0001).abs() < 1e-12,
        "cm2 factor should be ≈0.0001, got {}",
        unit.factor
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template 'S' not found");
    let a_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "a")
        .expect("value cell 'a' not found");
    if let Some(expr) = &a_cell.default_expr {
        if let reify_ir::CompiledExprKind::Literal(reify_ir::Value::Scalar {
            si_value,
            dimension,
            ..
        }) = &expr.kind
        {
            assert!(
                (si_value - 0.005).abs() < 1e-12,
                "50cm2 should be ≈0.005 m², got {}",
                si_value
            );
            assert_eq!(
                *dimension,
                DimensionVector::AREA,
                "50cm2 quantity literal should carry AREA dimension"
            );
        } else {
            panic!("expected scalar literal for 'a', got {:?}", expr.kind);
        }
    } else {
        panic!("value cell 'a' has no default_expr");
    }
}

// ─── category 6: stdlib cross-reference (angle via deg) ───────────────────────

#[test]
fn user_defined_angle_unit_via_stdlib_deg() {
    // Declares `unit quarter_turn : Angle = 90deg` with the stdlib prelude seeded
    // so that `deg` (factor = PI/180) is already in the registry.
    // Expected: quarter_turn.factor ≈ 90 * (PI/180) = PI/2 ≈ 1.5707963267948966.
    // unit_registry_tests.rs::prelude_unit_resolves_in_unit_conversion_expr
    // covers the same cross-registry path for the Length/mm variant; this test
    // extends coverage to the affine-free Angle dimension via stdlib deg.
    let module = compile_source_with_stdlib("unit quarter_turn : Angle = 90deg");
    let errors = errors_only(&module);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
    let unit = module
        .units
        .iter()
        .find(|u| u.name == "quarter_turn")
        .expect("unit 'quarter_turn' not found in module.units");
    assert_eq!(
        unit.dimension,
        DimensionVector::ANGLE,
        "quarter_turn should have ANGLE dimension"
    );
    let expected = 90.0 * std::f64::consts::PI / 180.0; // PI/2
    assert!(
        (unit.factor - expected).abs() < 1e-12,
        "quarter_turn factor should be ≈{} (PI/2), got {}",
        expected,
        unit.factor
    );
    assert!(unit.offset.is_none(), "quarter_turn should have no offset");
}
