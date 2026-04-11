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

use reify_compiler::{CompiledModule, compile, compile_with_prelude};
use reify_types::{DimensionVector, ModulePath, Severity};

// ─── helpers ───────────────────────────────────────────────────────────────────

fn parse_and_compile(source: &str) -> CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("unit_test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    compile(&parsed)
}

fn compile_with_prelude_helper(source: &str, prelude: &[CompiledModule]) -> CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("unit_test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    compile_with_prelude(&parsed, prelude)
}

fn errors_only(module: &CompiledModule) -> Vec<&reify_types::Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

// ─── category 1: basic unit declaration ───────────────────────────────────────

#[test]
fn custom_unit_declaration_registers_name_dimension_factor() {
    // Declare a simple length unit with an explicit SI factor.
    // The compiled module's unit list must contain the entry with the correct
    // name, dimension, factor, and no offset.
    let module = parse_and_compile("unit thou : Length = 0.0000254");
    let errors = errors_only(&module);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
    let unit = module
        .units
        .iter()
        .find(|u| u.name == "thou")
        .expect("unit 'thou' not found in module.units");
    assert_eq!(unit.dimension, DimensionVector::LENGTH, "dimension should be LENGTH");
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
    let module = parse_and_compile("unit degC : Temperature = 1 offset 273.15");
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
}

// ─── category 3: conversion factor arithmetic ─────────────────────────────────

#[test]
fn conversion_factor_arithmetic_multiplication_correct() {
    // factor = 25.4 * 0.001 = 0.0254  (BinOp Multiply in evaluate_const_expr).
    let module = parse_and_compile("unit inch : Length = 25.4 * 0.001");
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
    let module = parse_and_compile("unit milli : Length = 1 / 1000");
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
    let module = parse_and_compile(
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
        if let reify_types::CompiledExprKind::Literal(reify_types::Value::Scalar {
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
        if let reify_types::CompiledExprKind::Literal(reify_types::Value::Scalar {
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
    let module = parse_and_compile("unit kN : Force = 1000");
    let errors = errors_only(&module);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
    let unit = module
        .units
        .iter()
        .find(|u| u.name == "kN")
        .expect("unit 'kN' not found");
    assert_eq!(
        unit.dimension,
        reify_types::dimension::FORCE,
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
fn custom_unit_with_compound_dimension_area_in_quantity_literal() {
    // cm2: Area = 0.0001 m²;  50cm2 → si_value = 50 * 0.0001 = 0.005 m².
    // Exercises the full pipeline for a compound-dimension custom unit used in
    // a quantity literal.
    let module = parse_and_compile(
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
    assert_eq!(unit.dimension, DimensionVector::AREA, "cm2 should have AREA dimension");
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
        if let reify_types::CompiledExprKind::Literal(reify_types::Value::Scalar {
            si_value, ..
        }) = &expr.kind
        {
            assert!(
                (si_value - 0.005).abs() < 1e-12,
                "50cm2 should be ≈0.005 m², got {}",
                si_value
            );
        } else {
            panic!("expected scalar literal for 'a', got {:?}", expr.kind);
        }
    } else {
        panic!("value cell 'a' has no default_expr");
    }
}
