//! Acceptance tests for user-defined unit declarations (task 210).
//!
//! Each of the 14 tests maps directly onto one of the 7 acceptance categories:
//!
//!   1. Custom unit declaration            (tests 1–2)
//!   2. Offset unit / degC                 (tests 3–4)
//!   3. Conversion factor correctness      (tests 5–6)
//!   4. Cross-module unit                  (tests 7–8)
//!   5. Duplicate name error               (tests 9–10)
//!   6. User unit in quantity literal → SI (tests 11–12)
//!   7. Unit with compound dimension       (tests 13–14)

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

// ─── category 1: custom unit declaration ──────────────────────────────────────

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

#[test]
fn custom_unit_declaration_via_cross_ref_quantity_literal() {
    // Declare `mm`, then declare `thou` using a QuantityLiteral cross-reference.
    // Both units must appear in module.units, and `thou`'s factor must equal
    // 0.0254 * 0.001 = 0.0000254 (resolved via evaluate_const_expr).
    let module = parse_and_compile(
        "unit mm : Length = 0.001\n\
         unit thou : Length = 0.0254mm",
    );
    let errors = errors_only(&module);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
    let mm = module
        .units
        .iter()
        .find(|u| u.name == "mm")
        .expect("unit 'mm' not found");
    let thou = module
        .units
        .iter()
        .find(|u| u.name == "thou")
        .expect("unit 'thou' not found");
    assert!(
        (mm.factor - 0.001).abs() < 1e-12,
        "mm factor should be 0.001, got {}",
        mm.factor
    );
    let expected = 0.0254 * 0.001;
    assert!(
        (thou.factor - expected).abs() < 1e-12,
        "thou factor should be ≈{} (0.0254mm), got {}",
        expected,
        thou.factor
    );
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

#[test]
fn offset_unit_degc_in_quantity_literal_converts_to_kelvin() {
    // 25degC → si_value = 25 * 1 + 273.15 = 298.15 K.
    // Verifies the affine-unit path in the quantity-literal evaluator.
    let module = parse_and_compile(
        "unit degC : Temperature = 1 offset 273.15\n\
         structure S { param t : Temperature = 25degC }",
    );
    let errors = errors_only(&module);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
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
        if let reify_types::CompiledExprKind::Literal(reify_types::Value::Scalar {
            si_value, ..
        }) = &expr.kind
        {
            assert!(
                (si_value - 298.15).abs() < 1e-9,
                "25degC should convert to 298.15 K, got {}",
                si_value
            );
        } else {
            panic!("expected scalar literal for 't', got {:?}", expr.kind);
        }
    } else {
        panic!("value cell 't' has no default_expr");
    }
}

// ─── category 3: conversion factor correctness ────────────────────────────────

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

// ─── category 4: cross-module unit ────────────────────────────────────────────

#[test]
fn cross_module_pub_unit_visible_via_prelude() {
    // A `pub unit` in a prelude module must be seeded into the importing module's
    // registry and resolve correctly in quantity literals.
    let prelude = parse_and_compile("pub unit mil : Length = 0.0000254");
    assert!(
        errors_only(&prelude).is_empty(),
        "prelude errors: {:?}",
        errors_only(&prelude)
    );
    let module = compile_with_prelude_helper(
        "structure S { param w : Length = 5mil }",
        &[prelude],
    );
    let errors = errors_only(&module);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("template 'S' not found");
    let w_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "w")
        .expect("value cell 'w' not found");
    if let Some(expr) = &w_cell.default_expr {
        if let reify_types::CompiledExprKind::Literal(reify_types::Value::Scalar {
            si_value, ..
        }) = &expr.kind
        {
            let expected = 5.0 * 0.0000254;
            assert!(
                (si_value - expected).abs() < 1e-10,
                "5mil should be ≈{} m, got {}",
                expected,
                si_value
            );
        } else {
            panic!("expected scalar literal for 'w', got {:?}", expr.kind);
        }
    } else {
        panic!("value cell 'w' has no default_expr");
    }
}

#[test]
fn cross_module_private_unit_not_visible_via_prelude() {
    // A private unit (no `pub`) must NOT be seeded into importing modules;
    // any reference to it from another module should produce an error.
    let prelude = parse_and_compile("unit hidden_mil : Length = 0.0000254");
    assert!(
        errors_only(&prelude).is_empty(),
        "prelude errors: {:?}",
        errors_only(&prelude)
    );
    let module = compile_with_prelude_helper(
        "structure S { param w : Length = 5hidden_mil }",
        &[prelude],
    );
    let errors = errors_only(&module);
    assert!(
        !errors.is_empty(),
        "expected error for private unit 'hidden_mil' used across module boundary"
    );
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("unknown") || d.message.contains("hidden_mil")),
        "error should mention 'unknown' or 'hidden_mil'; got: {:?}",
        errors
    );
}

// ─── category 5: duplicate name error ─────────────────────────────────────────

#[test]
fn duplicate_unit_name_in_same_module_is_error() {
    // Two unit declarations with the same name must produce an error mentioning
    // 'duplicate' and the unit name.
    let module = parse_and_compile(
        "unit foo : Length = 0.001\n\
         unit foo : Length = 0.002",
    );
    let errors = errors_only(&module);
    assert!(
        !errors.is_empty(),
        "expected error for duplicate unit 'foo', got none"
    );
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("duplicate") && d.message.contains("foo")),
        "error should mention 'duplicate' and 'foo'; got: {:?}",
        errors
    );
}

#[test]
fn duplicate_unit_name_shadowing_prelude_is_error() {
    // Re-declaring a unit whose name is already seeded from a prelude must fail
    // with a duplicate error (exercises the prelude-shadow Err path).
    let prelude = parse_and_compile("pub unit mil : Length = 0.0000254");
    assert!(
        errors_only(&prelude).is_empty(),
        "prelude errors: {:?}",
        errors_only(&prelude)
    );
    let module = compile_with_prelude_helper("unit mil : Length = 0.0000001", &[prelude]);
    let errors = errors_only(&module);
    assert!(
        !errors.is_empty(),
        "expected duplicate unit error when shadowing a prelude unit"
    );
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("duplicate") && d.message.contains("mil")),
        "error should mention 'duplicate' and 'mil'; got: {:?}",
        errors
    );
}

// ─── category 6: user unit in quantity literal → SI value ─────────────────────

#[test]
fn user_unit_in_quantity_literal_evaluates_to_si_value_length() {
    // 10thou → si_value = 10 * 0.0000254 = 0.000254 m.
    let module = parse_and_compile(
        "unit thou : Length = 0.0000254\n\
         structure Bracket { param w : Length = 10thou }",
    );
    let errors = errors_only(&module);
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
    let template = module
        .templates
        .iter()
        .find(|t| t.name == "Bracket")
        .expect("template 'Bracket' not found");
    let w_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "w")
        .expect("value cell 'w' not found");
    if let Some(expr) = &w_cell.default_expr {
        if let reify_types::CompiledExprKind::Literal(reify_types::Value::Scalar {
            si_value, ..
        }) = &expr.kind
        {
            let expected = 10.0 * 0.0000254;
            assert!(
                (si_value - expected).abs() < 1e-12,
                "10thou should be ≈{} m, got {}",
                expected,
                si_value
            );
        } else {
            panic!("expected scalar literal for 'w', got {:?}", expr.kind);
        }
    } else {
        panic!("value cell 'w' has no default_expr");
    }
}

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

// ─── category 7: unit with compound dimension ─────────────────────────────────

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
