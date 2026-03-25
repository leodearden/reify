//! Tests for the unit declaration registry (task 208).
//!
//! Validates UnitEntry, UnitRegistry, resolve_dimension_type,
//! evaluate_const_expr, compile_unit, and the full unit pre-pass in compile().

use reify_compiler::{compile, CompiledModule, UnitEntry, UnitRegistry};
use reify_types::{DimensionVector, ModulePath, Severity, SourceSpan};

// ─── helpers ──────────────────────────────────────────────────────────────────

fn parse_and_compile(source: &str) -> CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("unit_test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    compile(&parsed)
}

fn errors_only(module: &CompiledModule) -> Vec<&reify_types::Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

// ─── step-1: UnitEntry and UnitRegistry data structures ───────────────────────

#[test]
fn unit_entry_fields_exist() {
    // Construct a UnitEntry directly to verify the struct fields are accessible.
    let dummy_span = SourceSpan::new(0, 0);
    let hash = reify_types::ContentHash::of_str("meter");
    let entry = UnitEntry {
        name: "meter".to_string(),
        dimension: DimensionVector::LENGTH,
        factor: 1.0,
        offset: None,
        is_pub: true,
        span: dummy_span,
        content_hash: hash,
    };
    assert_eq!(entry.name, "meter");
    assert_eq!(entry.dimension, DimensionVector::LENGTH);
    assert!((entry.factor - 1.0).abs() < 1e-12);
    assert!(entry.offset.is_none());
    assert!(entry.is_pub);
}

#[test]
fn unit_registry_new_and_lookup_empty() {
    let reg = UnitRegistry::new();
    assert!(reg.lookup("meter").is_none());
    assert!(reg.lookup("mm").is_none());
}

#[test]
fn unit_registry_register_and_lookup() {
    let mut reg = UnitRegistry::new();
    let entry = UnitEntry {
        name: "mm".to_string(),
        dimension: DimensionVector::LENGTH,
        factor: 0.001,
        offset: None,
        is_pub: false,
        span: SourceSpan::new(0, 0),
        content_hash: reify_types::ContentHash::of_str("mm"),
    };
    reg.register(entry).expect("first register should succeed");
    let found = reg.lookup("mm").expect("should find mm");
    assert_eq!(found.name, "mm");
    assert!((found.factor - 0.001).abs() < 1e-12);
}

#[test]
fn unit_registry_duplicate_returns_err() {
    let mut reg = UnitRegistry::new();
    let make_entry = || UnitEntry {
        name: "mm".to_string(),
        dimension: DimensionVector::LENGTH,
        factor: 0.001,
        offset: None,
        is_pub: false,
        span: SourceSpan::new(0, 0),
        content_hash: reify_types::ContentHash::of_str("mm"),
    };
    reg.register(make_entry()).expect("first register ok");
    let result = reg.register(make_entry());
    assert!(result.is_err(), "duplicate register should return Err");
}

// ─── step-3: resolve_dimension_type ───────────────────────────────────────────

#[test]
fn resolve_dimension_type_length() {
    let module = parse_and_compile("unit meter : Length");
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));
    let unit = module.units.iter().find(|u| u.name == "meter").expect("meter not found");
    assert_eq!(unit.dimension, DimensionVector::LENGTH);
}

#[test]
fn resolve_dimension_type_mass() {
    let module = parse_and_compile("unit kilogram : Mass");
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));
    let unit = module.units.iter().find(|u| u.name == "kilogram").expect("kilogram not found");
    assert_eq!(unit.dimension, DimensionVector::MASS);
}

#[test]
fn resolve_dimension_type_time() {
    let module = parse_and_compile("unit second : Time");
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));
    let unit = module.units.iter().find(|u| u.name == "second").expect("second not found");
    assert_eq!(unit.dimension, DimensionVector::TIME);
}

#[test]
fn resolve_dimension_type_temperature() {
    let module = parse_and_compile("unit kelvin : Temperature");
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));
    let unit = module.units.iter().find(|u| u.name == "kelvin").expect("kelvin not found");
    assert_eq!(unit.dimension, DimensionVector::TEMPERATURE);
}

#[test]
fn resolve_dimension_type_angle() {
    let module = parse_and_compile("unit radian : Angle");
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));
    let unit = module.units.iter().find(|u| u.name == "radian").expect("radian not found");
    assert_eq!(unit.dimension, DimensionVector::ANGLE);
}

#[test]
fn resolve_dimension_type_area() {
    let module = parse_and_compile("unit sq_meter : Area");
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));
    let unit = module.units.iter().find(|u| u.name == "sq_meter").expect("sq_meter not found");
    assert_eq!(unit.dimension, DimensionVector::AREA);
}

#[test]
fn resolve_dimension_type_volume() {
    let module = parse_and_compile("unit cubic_meter : Volume");
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));
    let unit = module.units.iter().find(|u| u.name == "cubic_meter").expect("cubic_meter not found");
    assert_eq!(unit.dimension, DimensionVector::VOLUME);
}

#[test]
fn resolve_dimension_type_force() {
    let module = parse_and_compile("unit newton : Force");
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));
    let unit = module.units.iter().find(|u| u.name == "newton").expect("newton not found");
    assert_eq!(unit.dimension, reify_types::dimension::FORCE);
}

#[test]
fn resolve_dimension_type_current() {
    let module = parse_and_compile("unit ampere : Current");
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));
    let unit = module.units.iter().find(|u| u.name == "ampere").expect("ampere not found");
    assert_eq!(unit.dimension, DimensionVector::CURRENT);
}

#[test]
fn resolve_dimension_type_unknown_emits_error() {
    let module = parse_and_compile("unit foo : UnknownDimension");
    let errors = errors_only(&module);
    assert!(!errors.is_empty(), "expected error for unknown dimension type");
    assert!(
        errors.iter().any(|d| d.message.contains("unknown dimension") || d.message.contains("UnknownDimension")),
        "error should mention the unknown dimension; got: {:?}",
        errors
    );
}

// ─── step-5: evaluate_const_expr ──────────────────────────────────────────────

#[test]
fn evaluate_const_number_literal() {
    // A unit with a plain number literal as its conversion factor.
    let module = parse_and_compile("unit cm : Length = 0.01");
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));
    let unit = module.units.iter().find(|u| u.name == "cm").expect("cm not found");
    assert!((unit.factor - 0.01).abs() < 1e-12, "factor should be 0.01, got {}", unit.factor);
}

#[test]
fn evaluate_const_binop_multiply() {
    // Conversion factor as a binary multiplication: 25.4 * 0.001
    let module = parse_and_compile("unit inch_mm : Length = 25.4 * 0.001");
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));
    let unit = module.units.iter().find(|u| u.name == "inch_mm").expect("inch_mm not found");
    assert!((unit.factor - 0.0254).abs() < 1e-9, "factor should be 0.0254, got {}", unit.factor);
}

#[test]
fn evaluate_const_binop_divide() {
    // Conversion factor as division: 1 / 1000
    let module = parse_and_compile("unit milli : Length = 1 / 1000");
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));
    let unit = module.units.iter().find(|u| u.name == "milli").expect("milli not found");
    assert!((unit.factor - 0.001).abs() < 1e-12, "factor should be 0.001, got {}", unit.factor);
}

#[test]
fn evaluate_const_quantity_literal_cross_ref() {
    // thou = 0.0254mm uses a QuantityLiteral referencing mm from registry
    let module = parse_and_compile(
        "unit m : Length\nunit mm : Length = 0.001\nunit thou : Length = 0.0254mm"
    );
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));
    let thou = module.units.iter().find(|u| u.name == "thou").expect("thou not found");
    assert!(
        (thou.factor - 0.0000254).abs() < 1e-12,
        "thou factor should be 0.0000254, got {}",
        thou.factor
    );
}

// ─── step-7: compile_unit ─────────────────────────────────────────────────────

#[test]
fn compile_unit_base_unit_no_conversion() {
    // Base unit with no conversion expression: factor defaults to 1.0.
    let module = parse_and_compile("unit meter : Length");
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));
    let unit = module.units.iter().find(|u| u.name == "meter").expect("meter not found");
    assert!((unit.factor - 1.0).abs() < 1e-12);
    assert!(unit.offset.is_none());
    assert_eq!(unit.dimension, DimensionVector::LENGTH);
}

#[test]
fn compile_unit_derived_unit_with_factor() {
    let module = parse_and_compile("unit mm : Length = 0.001");
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));
    let unit = module.units.iter().find(|u| u.name == "mm").expect("mm not found");
    assert!((unit.factor - 0.001).abs() < 1e-12);
    assert!(unit.offset.is_none());
}

#[test]
fn compile_unit_affine_with_offset() {
    // degC: factor=1.0, offset=273.15
    let module = parse_and_compile("unit degC : Temperature = 1 offset 273.15");
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));
    let unit = module.units.iter().find(|u| u.name == "degC").expect("degC not found");
    assert!((unit.factor - 1.0).abs() < 1e-12);
    assert!(unit.offset.is_some());
    assert!((unit.offset.unwrap() - 273.15).abs() < 1e-9);
}

#[test]
fn compile_unit_unknown_dimension_emits_error() {
    let module = parse_and_compile("unit foo : Luminance = 1.0");
    let errors = errors_only(&module);
    assert!(!errors.is_empty(), "expected error for unknown dimension");
}

// ─── step-9: compile() pre-pass populates CompiledModule.units ────────────────

#[test]
fn compiled_module_has_units_field() {
    let module = parse_and_compile("unit mm : Length = 0.001\nunit m : Length");
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));
    assert_eq!(module.units.len(), 2, "expected 2 compiled units, got {:?}", module.units.iter().map(|u| &u.name).collect::<Vec<_>>());
}

#[test]
fn compiled_units_have_correct_dimensions_and_factors() {
    let module = parse_and_compile("unit mm : Length = 0.001\nunit m : Length");
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));
    let mm = module.units.iter().find(|u| u.name == "mm").expect("mm not found");
    assert_eq!(mm.dimension, DimensionVector::LENGTH);
    assert!((mm.factor - 0.001).abs() < 1e-12);
    let m = module.units.iter().find(|u| u.name == "m").expect("m not found");
    assert!((m.factor - 1.0).abs() < 1e-12);
}

// ─── step-11: unit_to_scalar with registry resolves registry units ─────────────

#[test]
fn quantity_literal_uses_registry_unit() {
    // Define thou in the source, then use it in a structure param default.
    let module = parse_and_compile(
        "unit thou : Length = 0.0000254\nstructure Bracket { param width : Length = 10thou }"
    );
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));
    let template = module.templates.iter().find(|t| t.name == "Bracket").expect("Bracket not found");
    let width_cell = template.value_cells.iter().find(|c| c.id.member == "width").expect("width not found");
    // Default value should be 10 * 0.0000254 = 0.000254
    if let Some(default_expr) = &width_cell.default_expr {
        if let reify_types::CompiledExprKind::Literal(reify_types::Value::Scalar { si_value, .. }) = &default_expr.kind {
            assert!(
                (si_value - 0.000254).abs() < 1e-9,
                "expected si_value≈0.000254, got {}",
                si_value
            );
        } else {
            panic!("expected scalar literal, got {:?}", default_expr.kind);
        }
    } else {
        panic!("width cell has no default_expr");
    }
}

#[test]
fn hardcoded_units_still_work_without_declarations() {
    // All hardcoded units should still work when no unit declarations are present.
    let module = parse_and_compile(
        "structure S { param a : Length = 10mm\n param b : Angle = 90deg\n param c : Mass = 1kg }"
    );
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));
    let template = module.templates.iter().find(|t| t.name == "S").expect("S not found");
    let a = template.value_cells.iter().find(|c| c.id.member == "a").expect("a not found");
    if let Some(expr) = &a.default_expr {
        if let reify_types::CompiledExprKind::Literal(reify_types::Value::Scalar { si_value, .. }) = &expr.kind {
            assert!((si_value - 0.01).abs() < 1e-9, "10mm should be 0.01m, got {}", si_value);
        } else {
            panic!("expected scalar literal for a");
        }
    } else {
        panic!("a has no default_expr");
    }
}

// ─── step-13: duplicate unit names produce diagnostic error ───────────────────

#[test]
fn duplicate_unit_name_emits_error() {
    let module = parse_and_compile("unit mm : Length = 0.001\nunit mm : Length = 0.001");
    let errors = errors_only(&module);
    assert!(!errors.is_empty(), "expected duplicate unit error");
    assert!(
        errors.iter().any(|d| d.message.contains("duplicate") && d.message.contains("mm")),
        "error should mention 'duplicate' and 'mm'; got: {:?}",
        errors
    );
}

// ─── step-15: unit referencing another unit in conversion ─────────────────────

#[test]
fn unit_cross_ref_in_conversion_expr() {
    // thou = 0.0254mm: should resolve mm from registry -> factor = 0.0254 * 0.001 = 0.0000254
    let module = parse_and_compile(
        "unit m : Length\nunit mm : Length = 0.001\nunit thou : Length = 0.0254mm"
    );
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));
    let thou = module.units.iter().find(|u| u.name == "thou").expect("thou not found");
    assert!(
        (thou.factor - 0.0000254).abs() < 1e-12,
        "thou should have factor 0.0000254, got {}",
        thou.factor
    );
}

// ─── step-17: pub visibility propagated to CompiledUnit ───────────────────────

#[test]
fn pub_unit_has_is_pub_true() {
    let module = parse_and_compile("pub unit mm : Length = 0.001");
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));
    let unit = module.units.iter().find(|u| u.name == "mm").expect("mm not found");
    assert!(unit.is_pub, "pub unit should have is_pub=true");
}

#[test]
fn private_unit_has_is_pub_false() {
    let module = parse_and_compile("unit internal_mm : Length = 0.001");
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));
    let unit = module.units.iter().find(|u| u.name == "internal_mm").expect("internal_mm not found");
    assert!(!unit.is_pub, "private unit should have is_pub=false");
}

// ─── step-19: integration test — unit declarations used in structure params ────

#[test]
fn integration_unit_in_structure_param() {
    let module = parse_and_compile(
        "unit mm : Length = 0.001\nstructure Bracket { param width : Length = 50mm }"
    );
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));
    let template = module.templates.iter().find(|t| t.name == "Bracket").expect("Bracket not found");
    let width_cell = template.value_cells.iter().find(|c| c.id.member == "width").expect("width not found");
    if let Some(default_expr) = &width_cell.default_expr {
        if let reify_types::CompiledExprKind::Literal(reify_types::Value::Scalar { si_value, .. }) = &default_expr.kind {
            assert!(
                (si_value - 0.05).abs() < 1e-9,
                "50mm should be 0.05m, got {}",
                si_value
            );
        } else {
            panic!("expected scalar literal for width, got {:?}", default_expr.kind);
        }
    } else {
        panic!("width has no default_expr");
    }
}

// ─── step-21: offset-only unit ────────────────────────────────────────────────

#[test]
fn offset_only_unit_has_factor_one() {
    // `unit kelvin : Temperature offset 273.15` -> factor=1.0, offset=Some(273.15)
    let module = parse_and_compile("unit kelvin : Temperature offset 273.15");
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));
    let unit = module.units.iter().find(|u| u.name == "kelvin").expect("kelvin not found");
    assert!((unit.factor - 1.0).abs() < 1e-12, "offset-only unit factor should be 1.0");
    assert!(unit.offset.is_some(), "offset-only unit should have Some(offset)");
    assert!((unit.offset.unwrap() - 273.15).abs() < 1e-9);
}

#[test]
fn offset_unit_quantity_literal_applies_offset() {
    // 100kelvin QuantityLiteral: si_value = 100 * 1.0 + 273.15 = 373.15
    let module = parse_and_compile(
        "unit kelvin : Temperature offset 273.15\nstructure S { param t : Temperature = 100kelvin }"
    );
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));
    let template = module.templates.iter().find(|t| t.name == "S").expect("S not found");
    let t_cell = template.value_cells.iter().find(|c| c.id.member == "t").expect("t not found");
    if let Some(expr) = &t_cell.default_expr {
        if let reify_types::CompiledExprKind::Literal(reify_types::Value::Scalar { si_value, .. }) = &expr.kind {
            assert!(
                (si_value - 373.15).abs() < 1e-9,
                "100kelvin should be 373.15K, got {}",
                si_value
            );
        } else {
            panic!("expected scalar literal for t, got {:?}", expr.kind);
        }
    } else {
        panic!("t has no default_expr");
    }
}

// ─── step-27: compile_unit returns None for broken conversion expression ──────

#[test]
fn compile_unit_broken_conversion_not_registered() {
    // 'some_var' is an identifier — evaluate_const_expr() returns None for
    // non-constant expressions. compile_unit() should return None, so the unit
    // is NOT added to module.units.
    let module = parse_and_compile("unit broken : Length = some_var");
    // The unit should NOT appear in compiled units
    assert!(
        !module.units.iter().any(|u| u.name == "broken"),
        "unit with broken conversion should NOT be registered; got: {:?}",
        module.units.iter().map(|u| &u.name).collect::<Vec<_>>()
    );
    // An error diagnostic should be present
    let errors = errors_only(&module);
    assert!(
        !errors.is_empty(),
        "expected error diagnostic for non-constant conversion expression"
    );
}

// ─── step-29: compile_unit returns None for broken offset expression ──────────

#[test]
fn compile_unit_broken_offset_not_registered() {
    // 'some_var' in offset position is a non-constant expression.
    // compile_unit() should return None, so the unit is NOT added to module.units.
    // Currently the offset code silently sets offset=None when eval fails,
    // registering the unit as if no offset was declared.
    let module = parse_and_compile("unit broken_off : Temperature = 1 offset some_var");
    // The unit should NOT appear in compiled units
    assert!(
        !module.units.iter().any(|u| u.name == "broken_off"),
        "unit with broken offset should NOT be registered; got: {:?}",
        module.units.iter().map(|u| &u.name).collect::<Vec<_>>()
    );
    // An error diagnostic should be present
    let errors = errors_only(&module);
    assert!(
        !errors.is_empty(),
        "expected error diagnostic for non-constant offset expression"
    );
}

// ─── step-23: regression test — hardcoded units still work ────────────────────

#[test]
fn regression_hardcoded_units_all_still_resolve() {
    // All 9 hardcoded units (mm, cm, m, in, deg, rad, kg, g, s) must still work
    // when no unit declarations are present.
    let module = parse_and_compile(
        "structure S {\n\
            param a : Length = 1mm\n\
            param b : Length = 1cm\n\
            param c : Length = 1m\n\
            param d : Angle = 1deg\n\
            param e : Angle = 1rad\n\
            param f : Mass = 1kg\n\
            param g : Mass = 1g\n\
            param h : Length = 1in\n\
            param i : Time = 1s\n\
        }"
    );
    assert!(errors_only(&module).is_empty(), "errors: {:?}", errors_only(&module));
    let template = module.templates.iter().find(|t| t.name == "S").expect("S not found");
    let check = |name: &str, expected_si: f64| {
        let cell = template.value_cells.iter().find(|c| c.id.member == name)
            .unwrap_or_else(|| panic!("{} not found", name));
        if let Some(expr) = &cell.default_expr {
            if let reify_types::CompiledExprKind::Literal(reify_types::Value::Scalar { si_value, .. }) = &expr.kind {
                assert!(
                    (si_value - expected_si).abs() < 1e-9,
                    "{}: expected {}, got {}",
                    name,
                    expected_si,
                    si_value
                );
            } else {
                panic!("{}: expected scalar literal, got {:?}", name, expr.kind);
            }
        } else {
            panic!("{}: no default_expr", name);
        }
    };
    check("a", 0.001);   // 1mm
    check("b", 0.01);    // 1cm
    check("c", 1.0);     // 1m
    check("d", std::f64::consts::PI / 180.0); // 1deg
    check("e", 1.0);     // 1rad
    check("f", 1.0);     // 1kg
    check("g", 0.001);   // 1g
    check("h", 0.0254);  // 1in (inch = 0.0254m)
    check("i", 1.0);     // 1s (second, SI base unit)
}
