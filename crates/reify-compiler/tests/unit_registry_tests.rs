//! Tests for the unit declaration registry (task 208).
//!
//! Validates UnitEntry, UnitRegistry, resolve_dimension_type,
//! evaluate_const_expr, compile_unit, and the full unit pre-pass in compile().

use reify_compiler::{
    CompiledModule, UnitEntry, UnitRegistry, compile, compile_with_prelude, compile_with_stdlib,
    stdlib_loader,
};
use reify_types::{DimensionVector, ModulePath, Severity, SourceSpan};

// ─── helpers ──────────────────────────────────────────────────────────────────

fn parse_and_compile(source: &str) -> CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("unit_test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
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

// ─── seed_prelude_unit tests ─────────────────────────────────────────────────

#[test]
fn seed_prelude_unit_inserts_and_lookups() {
    let mut reg = UnitRegistry::new();
    let entry = UnitEntry {
        name: "mm".to_string(),
        dimension: DimensionVector::LENGTH,
        factor: 0.001,
        offset: None,
        is_pub: true,
        span: SourceSpan::new(0, 0),
        content_hash: reify_types::ContentHash::of_str("mm"),
    };
    reg.seed_prelude_unit(entry);
    let found = reg
        .lookup("mm")
        .expect("seed_prelude_unit should make mm visible");
    assert_eq!(found.name, "mm");
    assert!((found.factor - 0.001).abs() < 1e-12);
}

#[test]
fn seed_prelude_unit_overwrites_on_duplicate() {
    let mut reg = UnitRegistry::new();
    let entry1 = UnitEntry {
        name: "mm".to_string(),
        dimension: DimensionVector::LENGTH,
        factor: 0.001,
        offset: None,
        is_pub: true,
        span: SourceSpan::new(0, 0),
        content_hash: reify_types::ContentHash::of_str("mm-v1"),
    };
    let entry2 = UnitEntry {
        name: "mm".to_string(),
        dimension: DimensionVector::LENGTH,
        factor: 0.002,
        offset: None,
        is_pub: true,
        span: SourceSpan::new(10, 15),
        content_hash: reify_types::ContentHash::of_str("mm-v2"),
    };
    reg.seed_prelude_unit(entry1);
    reg.seed_prelude_unit(entry2);
    let found = reg.lookup("mm").unwrap();
    assert!(
        (found.factor - 0.002).abs() < 1e-12,
        "seed_prelude_unit should overwrite: expected 0.002, got {}",
        found.factor
    );
}

// ─── step-3: resolve_dimension_type ───────────────────────────────────────────

#[test]
fn resolve_dimension_type_length() {
    let module = parse_and_compile("unit meter : Length");
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    let unit = module
        .units
        .iter()
        .find(|u| u.name == "meter")
        .expect("meter not found");
    assert_eq!(unit.dimension, DimensionVector::LENGTH);
}

#[test]
fn resolve_dimension_type_mass() {
    let module = parse_and_compile("unit kilogram : Mass");
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    let unit = module
        .units
        .iter()
        .find(|u| u.name == "kilogram")
        .expect("kilogram not found");
    assert_eq!(unit.dimension, DimensionVector::MASS);
}

#[test]
fn resolve_dimension_type_time() {
    let module = parse_and_compile("unit second : Time");
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    let unit = module
        .units
        .iter()
        .find(|u| u.name == "second")
        .expect("second not found");
    assert_eq!(unit.dimension, DimensionVector::TIME);
}

#[test]
fn resolve_dimension_type_temperature() {
    let module = parse_and_compile("unit kelvin : Temperature");
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    let unit = module
        .units
        .iter()
        .find(|u| u.name == "kelvin")
        .expect("kelvin not found");
    assert_eq!(unit.dimension, DimensionVector::TEMPERATURE);
}

#[test]
fn resolve_dimension_type_angle() {
    let module = parse_and_compile("unit radian : Angle");
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    let unit = module
        .units
        .iter()
        .find(|u| u.name == "radian")
        .expect("radian not found");
    assert_eq!(unit.dimension, DimensionVector::ANGLE);
}

#[test]
fn resolve_dimension_type_area() {
    let module = parse_and_compile("unit sq_meter : Area");
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    let unit = module
        .units
        .iter()
        .find(|u| u.name == "sq_meter")
        .expect("sq_meter not found");
    assert_eq!(unit.dimension, DimensionVector::AREA);
}

#[test]
fn resolve_dimension_type_volume() {
    let module = parse_and_compile("unit cubic_meter : Volume");
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    let unit = module
        .units
        .iter()
        .find(|u| u.name == "cubic_meter")
        .expect("cubic_meter not found");
    assert_eq!(unit.dimension, DimensionVector::VOLUME);
}

#[test]
fn resolve_dimension_type_force() {
    let module = parse_and_compile("unit newton : Force");
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    let unit = module
        .units
        .iter()
        .find(|u| u.name == "newton")
        .expect("newton not found");
    assert_eq!(unit.dimension, reify_types::dimension::FORCE);
}

#[test]
fn resolve_dimension_type_current() {
    let module = parse_and_compile("unit ampere : Current");
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    let unit = module
        .units
        .iter()
        .find(|u| u.name == "ampere")
        .expect("ampere not found");
    assert_eq!(unit.dimension, DimensionVector::CURRENT);
}

#[test]
fn resolve_dimension_type_unknown_emits_error() {
    let module = parse_and_compile("unit foo : UnknownDimension");
    let errors = errors_only(&module);
    assert!(
        !errors.is_empty(),
        "expected error for unknown dimension type"
    );
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("unknown dimension")
                || d.message.contains("UnknownDimension")),
        "error should mention the unknown dimension; got: {:?}",
        errors
    );
}

// ─── step-5: evaluate_const_expr ──────────────────────────────────────────────

#[test]
fn evaluate_const_number_literal() {
    // A unit with a plain number literal as its conversion factor.
    let module = parse_and_compile("unit cm : Length = 0.01");
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    let unit = module
        .units
        .iter()
        .find(|u| u.name == "cm")
        .expect("cm not found");
    assert!(
        (unit.factor - 0.01).abs() < 1e-12,
        "factor should be 0.01, got {}",
        unit.factor
    );
}

#[test]
fn evaluate_const_binop_multiply() {
    // Conversion factor as a binary multiplication: 25.4 * 0.001
    let module = parse_and_compile("unit inch_mm : Length = 25.4 * 0.001");
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    let unit = module
        .units
        .iter()
        .find(|u| u.name == "inch_mm")
        .expect("inch_mm not found");
    assert!(
        (unit.factor - 0.0254).abs() < 1e-9,
        "factor should be 0.0254, got {}",
        unit.factor
    );
}

#[test]
fn evaluate_const_binop_divide() {
    // Conversion factor as division: 1 / 1000
    let module = parse_and_compile("unit milli : Length = 1 / 1000");
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    let unit = module
        .units
        .iter()
        .find(|u| u.name == "milli")
        .expect("milli not found");
    assert!(
        (unit.factor - 0.001).abs() < 1e-12,
        "factor should be 0.001, got {}",
        unit.factor
    );
}

#[test]
fn evaluate_const_quantity_literal_cross_ref() {
    // thou = 0.0254mm uses a QuantityLiteral referencing mm from registry
    let module = parse_and_compile(
        "unit m : Length\nunit mm : Length = 0.001\nunit thou : Length = 0.0254mm",
    );
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    let thou = module
        .units
        .iter()
        .find(|u| u.name == "thou")
        .expect("thou not found");
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
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    let unit = module
        .units
        .iter()
        .find(|u| u.name == "meter")
        .expect("meter not found");
    assert!((unit.factor - 1.0).abs() < 1e-12);
    assert!(unit.offset.is_none());
    assert_eq!(unit.dimension, DimensionVector::LENGTH);
}

#[test]
fn compile_unit_derived_unit_with_factor() {
    let module = parse_and_compile("unit mm : Length = 0.001");
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    let unit = module
        .units
        .iter()
        .find(|u| u.name == "mm")
        .expect("mm not found");
    assert!((unit.factor - 0.001).abs() < 1e-12);
    assert!(unit.offset.is_none());
}

#[test]
fn compile_unit_affine_with_offset() {
    // degC: factor=1.0, offset=273.15
    let module = parse_and_compile("unit degC : Temperature = 1 offset 273.15");
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    let unit = module
        .units
        .iter()
        .find(|u| u.name == "degC")
        .expect("degC not found");
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
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    assert_eq!(
        module.units.len(),
        2,
        "expected 2 compiled units, got {:?}",
        module.units.iter().map(|u| &u.name).collect::<Vec<_>>()
    );
}

#[test]
fn compiled_units_have_correct_dimensions_and_factors() {
    let module = parse_and_compile("unit mm : Length = 0.001\nunit m : Length");
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    let mm = module
        .units
        .iter()
        .find(|u| u.name == "mm")
        .expect("mm not found");
    assert_eq!(mm.dimension, DimensionVector::LENGTH);
    assert!((mm.factor - 0.001).abs() < 1e-12);
    let m = module
        .units
        .iter()
        .find(|u| u.name == "m")
        .expect("m not found");
    assert!((m.factor - 1.0).abs() < 1e-12);
}

// ─── step-11: unit_to_scalar with registry resolves registry units ─────────────

#[test]
fn quantity_literal_uses_registry_unit() {
    // Define thou in the source, then use it in a structure param default.
    let module = parse_and_compile(
        "unit thou : Length = 0.0000254\nstructure Bracket { param width : Length = 10thou }",
    );
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    let template = module
        .templates
        .iter()
        .find(|t| t.name == "Bracket")
        .expect("Bracket not found");
    let width_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "width")
        .expect("width not found");
    // Default value should be 10 * 0.0000254 = 0.000254
    if let Some(default_expr) = &width_cell.default_expr {
        if let reify_types::CompiledExprKind::Literal(reify_types::Value::Scalar {
            si_value, ..
        }) = &default_expr.kind
        {
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
    // Covers mm (Length), deg (Angle), and kg (Mass) fallback paths.
    let module = parse_and_compile(
        "structure S { param a : Length = 10mm\n param b : Angle = 90deg\n param c : Mass = 1kg }",
    );
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S not found");

    // Helper: extract si_value from a param's default_expr
    let check = |name: &str, expected: f64, desc: &str| {
        let cell = template
            .value_cells
            .iter()
            .find(|c| c.id.member == name)
            .unwrap_or_else(|| panic!("{} not found", name));
        let expr = cell
            .default_expr
            .as_ref()
            .unwrap_or_else(|| panic!("{} has no default_expr", name));
        if let reify_types::CompiledExprKind::Literal(reify_types::Value::Scalar {
            si_value, ..
        }) = &expr.kind
        {
            assert!(
                (si_value - expected).abs() < 1e-9,
                "{} expected {}, got {}",
                desc,
                expected,
                si_value
            );
        } else {
            panic!("expected scalar literal for {}", name);
        }
    };

    check("a", 0.01, "10mm should be 0.01m");
    check(
        "b",
        90.0 * std::f64::consts::PI / 180.0,
        "90deg should be PI/2 rad",
    );
    check("c", 1.0, "1kg should be 1.0kg");
}

// ─── step-13: duplicate unit names produce diagnostic error ───────────────────

#[test]
fn duplicate_unit_name_emits_error() {
    let module = parse_and_compile("unit mm : Length = 0.001\nunit mm : Length = 0.001");
    let errors = errors_only(&module);
    assert!(!errors.is_empty(), "expected duplicate unit error");
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("duplicate") && d.message.contains("mm")),
        "error should mention 'duplicate' and 'mm'; got: {:?}",
        errors
    );
}

// ─── step-15: unit referencing another unit in conversion ─────────────────────

#[test]
fn unit_cross_ref_in_conversion_expr() {
    // thou = 0.0254mm: should resolve mm from registry -> factor = 0.0254 * 0.001 = 0.0000254
    let module = parse_and_compile(
        "unit m : Length\nunit mm : Length = 0.001\nunit thou : Length = 0.0254mm",
    );
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    let thou = module
        .units
        .iter()
        .find(|u| u.name == "thou")
        .expect("thou not found");
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
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    let unit = module
        .units
        .iter()
        .find(|u| u.name == "mm")
        .expect("mm not found");
    assert!(unit.is_pub, "pub unit should have is_pub=true");
}

#[test]
fn private_unit_has_is_pub_false() {
    let module = parse_and_compile("unit internal_mm : Length = 0.001");
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    let unit = module
        .units
        .iter()
        .find(|u| u.name == "internal_mm")
        .expect("internal_mm not found");
    assert!(!unit.is_pub, "private unit should have is_pub=false");
}

// ─── step-19: integration test — unit declarations used in structure params ────

#[test]
fn integration_unit_in_structure_param() {
    let module = parse_and_compile(
        "unit mm : Length = 0.001\nstructure Bracket { param width : Length = 50mm }",
    );
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    let template = module
        .templates
        .iter()
        .find(|t| t.name == "Bracket")
        .expect("Bracket not found");
    let width_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "width")
        .expect("width not found");
    if let Some(default_expr) = &width_cell.default_expr {
        if let reify_types::CompiledExprKind::Literal(reify_types::Value::Scalar {
            si_value, ..
        }) = &default_expr.kind
        {
            assert!(
                (si_value - 0.05).abs() < 1e-9,
                "50mm should be 0.05m, got {}",
                si_value
            );
        } else {
            panic!(
                "expected scalar literal for width, got {:?}",
                default_expr.kind
            );
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
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    let unit = module
        .units
        .iter()
        .find(|u| u.name == "kelvin")
        .expect("kelvin not found");
    assert!(
        (unit.factor - 1.0).abs() < 1e-12,
        "offset-only unit factor should be 1.0"
    );
    assert!(
        unit.offset.is_some(),
        "offset-only unit should have Some(offset)"
    );
    assert!((unit.offset.unwrap() - 273.15).abs() < 1e-9);
}

#[test]
fn offset_unit_quantity_literal_applies_offset() {
    // 100kelvin QuantityLiteral: si_value = 100 * 1.0 + 273.15 = 373.15
    let module = parse_and_compile(
        "unit kelvin : Temperature offset 273.15\nstructure S { param t : Temperature = 100kelvin }",
    );
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S not found");
    let t_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "t")
        .expect("t not found");
    if let Some(expr) = &t_cell.default_expr {
        if let reify_types::CompiledExprKind::Literal(reify_types::Value::Scalar {
            si_value, ..
        }) = &expr.kind
        {
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
        }",
    );
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S not found");
    let check = |name: &str, expected_si: f64| {
        let cell = template
            .value_cells
            .iter()
            .find(|c| c.id.member == name)
            .unwrap_or_else(|| panic!("{} not found", name));
        if let Some(expr) = &cell.default_expr {
            if let reify_types::CompiledExprKind::Literal(reify_types::Value::Scalar {
                si_value,
                ..
            }) = &expr.kind
            {
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
    check("a", 0.001); // 1mm
    check("b", 0.01); // 1cm
    check("c", 1.0); // 1m
    check("d", std::f64::consts::PI / 180.0); // 1deg
    check("e", 1.0); // 1rad
    check("f", 1.0); // 1kg
    check("g", 0.001); // 1g
    check("h", 0.0254); // 1in (inch = 0.0254m)
    check("i", 1.0); // 1s (second, SI base unit)
}

// ─── step-40: affine units rejected in conversion expressions ─────────────────

#[test]
fn affine_unit_rejected_in_conversion_expression() {
    // Declaring 'unit mytemp : Temperature = 1.0degC' where degC has an offset
    // should fail: evaluate_const_expr() must reject affine (offset) units in
    // conversion expressions. The offset semantics only make sense for runtime
    // value expressions (e.g., '25degC' → 298.15K), not for defining conversion
    // factors.
    let module = parse_and_compile(
        "unit degC : Temperature = 1 offset 273.15\nunit mytemp : Temperature = 1.0degC",
    );
    // mytemp should NOT be registered (compile_unit returns None)
    assert!(
        !module.units.iter().any(|u| u.name == "mytemp"),
        "mytemp should not be registered when conversion references an affine unit"
    );
    // An error diagnostic should mention that affine/offset units cannot be used
    let errors = errors_only(&module);
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("affine") || d.message.contains("offset")),
        "expected diagnostic about affine/offset unit in conversion; got: {:?}",
        errors
    );
}

// ─── step-42: non-affine units in conversion still work; affine in runtime ok ─

#[test]
fn non_affine_unit_in_conversion_still_works_after_guard() {
    // Non-affine QuantityLiteral in conversion expressions must still work.
    // 'thou = 0.0254mm' references mm (no offset) — should produce factor ≈ 0.0000254.
    let module = parse_and_compile("unit mm : Length = 0.001\nunit thou : Length = 0.0254mm");
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    let thou = module
        .units
        .iter()
        .find(|u| u.name == "thou")
        .expect("thou not found");
    assert!(
        (thou.factor - 0.0000254).abs() < 1e-12,
        "thou should have factor 0.0000254, got {}",
        thou.factor
    );
}

#[test]
fn affine_unit_still_works_in_runtime_value_expression() {
    // Affine units must still work in runtime value expressions (QuantityLiteral
    // in structure params). '25degC' → si_value = 25*1 + 273.15 = 298.15K.
    // This goes through lookup_unit_in_registry(), NOT evaluate_const_expr().
    let module = parse_and_compile(
        "unit degC : Temperature = 1 offset 273.15\nstructure S { param t : Temperature = 25degC }",
    );
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S not found");
    let t_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "t")
        .expect("t not found");
    if let Some(expr) = &t_cell.default_expr {
        if let reify_types::CompiledExprKind::Literal(reify_types::Value::Scalar {
            si_value, ..
        }) = &expr.kind
        {
            assert!(
                (si_value - 298.15).abs() < 1e-9,
                "25degC should be 298.15K, got {}",
                si_value
            );
        } else {
            panic!("expected scalar literal for t, got {:?}", expr.kind);
        }
    } else {
        panic!("t has no default_expr");
    }
}

// ─── step-1 (task-208): evaluate_const_expr rejects non-finite arithmetic ─────

#[test]
fn overflow_multiplication_rejected() {
    // f64::MAX * 2.0 → inf — must NOT be registered.
    let src = format!("unit huge : Length = {} * 2.0", f64::MAX);
    let module = parse_and_compile(&src);
    assert!(
        !module.units.iter().any(|u| u.name == "huge"),
        "unit with overflow factor should not be registered"
    );
    let errors = errors_only(&module);
    assert!(
        errors.iter().any(|d| d.message.contains("overflow")),
        "expected overflow diagnostic; got: {:?}",
        errors
    );
}

#[test]
fn overflow_addition_rejected() {
    // f64::MAX + f64::MAX → inf
    let src = format!("unit huge_add : Length = {} + {}", f64::MAX, f64::MAX);
    let module = parse_and_compile(&src);
    assert!(
        !module.units.iter().any(|u| u.name == "huge_add"),
        "unit with overflow addition should not be registered"
    );
    let errors = errors_only(&module);
    assert!(
        errors.iter().any(|d| d.message.contains("overflow")),
        "expected overflow diagnostic; got: {:?}",
        errors
    );
}

#[test]
fn overflow_division_result_rejected() {
    // f64::MAX / very_small → inf (not div-by-zero, but result is inf)
    let src = format!("unit huge_div : Length = {} / 0.0000000000000001", f64::MAX);
    let module = parse_and_compile(&src);
    assert!(
        !module.units.iter().any(|u| u.name == "huge_div"),
        "unit with overflow division result should not be registered"
    );
    let errors = errors_only(&module);
    assert!(
        errors.iter().any(|d| d.message.contains("overflow")),
        "expected overflow diagnostic; got: {:?}",
        errors
    );
}

// ─── step-3 (task-208): compile_unit rejects zero and non-finite factors ──────

#[test]
fn zero_literal_factor_rejected() {
    // A unit with literal zero factor destroys unit information.
    let module = parse_and_compile("unit z : Length = 0");
    assert!(
        !module.units.iter().any(|u| u.name == "z"),
        "unit with zero factor should not be registered"
    );
    let errors = errors_only(&module);
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("non-zero") || d.message.contains("zero")),
        "expected zero-factor diagnostic; got: {:?}",
        errors
    );
}

#[test]
fn zero_from_arithmetic_factor_rejected() {
    // Zero via arithmetic: 0 * 1 → 0.
    let module = parse_and_compile("unit z2 : Length = 0 * 1");
    assert!(
        !module.units.iter().any(|u| u.name == "z2"),
        "unit with zero arithmetic factor should not be registered"
    );
    let errors = errors_only(&module);
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("non-zero") || d.message.contains("zero")),
        "expected zero-factor diagnostic; got: {:?}",
        errors
    );
}

// ─── step-5 (task-208): compile_unit rejects non-finite offset ────────────────

#[test]
fn non_finite_offset_rejected() {
    // Offset expression overflows: MAX + MAX → inf.
    let src = format!(
        "unit bad_off : Temperature = 1 offset {} + {}",
        f64::MAX,
        f64::MAX
    );
    let module = parse_and_compile(&src);
    assert!(
        !module.units.iter().any(|u| u.name == "bad_off"),
        "unit with non-finite offset should not be registered"
    );
    let errors = errors_only(&module);
    assert!(
        !errors.is_empty(),
        "expected error diagnostic for non-finite offset; got none"
    );
}

// ─── step-7 (task-208): QuantityLiteral overflow in evaluate_const_expr ───────

#[test]
fn quantity_literal_overflow_rejected() {
    // Define a unit with small factor, then reference it with a huge value.
    // value * factor overflows: MAX * 0.001 doesn't overflow, but MAX * 1.0 stays MAX,
    // and we need value * entry.factor → inf. Use MAX as value with factor > 1.
    let src = format!(
        "unit big : Length = 2.0\nunit derived : Length = {}big",
        f64::MAX
    );
    let module = parse_and_compile(&src);
    assert!(
        !module.units.iter().any(|u| u.name == "derived"),
        "unit with quantity-literal overflow should not be registered"
    );
    let errors = errors_only(&module);
    assert!(
        errors.iter().any(|d| d.message.contains("overflow")),
        "expected overflow diagnostic for quantity literal; got: {:?}",
        errors
    );
}

// ─── step-9 (task-208): regression tests — valid units still compile ──────────

#[test]
fn valid_arithmetic_factor_still_compiles() {
    // 25.4 * 0.001 = 0.0254 — valid, should compile fine.
    let module = parse_and_compile("unit inch : Length = 25.4 * 0.001");
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    let unit = module
        .units
        .iter()
        .find(|u| u.name == "inch")
        .expect("inch not found");
    assert!((unit.factor - 0.0254).abs() < 1e-9);
}

#[test]
fn valid_offset_still_compiles() {
    // 1 offset 273.15 — valid affine unit.
    let module = parse_and_compile("unit degC : Temperature = 1 offset 273.15");
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    let unit = module
        .units
        .iter()
        .find(|u| u.name == "degC")
        .expect("degC not found");
    assert!((unit.offset.unwrap() - 273.15).abs() < 1e-9);
}

#[test]
fn valid_quantity_literal_cross_ref_still_compiles() {
    // 0.0254mm — valid cross-reference.
    let module = parse_and_compile("unit mm : Length = 0.001\nunit thou : Length = 0.0254mm");
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    let thou = module
        .units
        .iter()
        .find(|u| u.name == "thou")
        .expect("thou not found");
    assert!((thou.factor - 0.0000254).abs() < 1e-12);
}

#[test]
fn valid_negative_factor_still_compiles() {
    // Negative factor is finite and non-zero — should compile.
    // (Semantically odd but not a validation error at this level.)
    let module = parse_and_compile("unit neg : Length = 0 - 1");
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    let unit = module
        .units
        .iter()
        .find(|u| u.name == "neg")
        .expect("neg not found");
    assert!((unit.factor - (-1.0)).abs() < 1e-12);
}

#[test]
fn valid_small_factor_still_compiles() {
    // Very small but finite factor.
    let module = parse_and_compile("unit pico : Length = 0.000000000001");
    assert!(
        errors_only(&module).is_empty(),
        "errors: {:?}",
        errors_only(&module)
    );
    let unit = module
        .units
        .iter()
        .find(|u| u.name == "pico")
        .expect("pico not found");
    assert!((unit.factor - 1e-12).abs() < 1e-24);
}

// ─── step-10 (task-208): compile_expr_guarded rejects non-finite quantity literals ─

#[test]
fn overflow_user_unit_in_structure_param_rejected() {
    // User-defined unit with factor 2.0, value that parses as infinity in a
    // structure param. si_value = inf * 2.0 + 0.0 = inf — must emit overflow diagnostic.
    // Note: tree-sitter grammar doesn't support scientific notation in number_literal,
    // so we use a 309-digit integer which str::parse::<f64>() maps to f64::INFINITY.
    let big_num = "9".repeat(309);
    let src = format!(
        "unit big : Length = 2.0\nstructure S {{ param x : Length = {}big }}",
        big_num
    );
    let module = parse_and_compile(&src);
    let errors = errors_only(&module);
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("overflow") || d.message.contains("finite")),
        "expected overflow diagnostic for non-finite quantity literal in structure param; got: {:?}",
        errors
    );
}

#[test]
fn overflow_hardcoded_unit_in_structure_param_rejected() {
    // Hardcoded mm path with value that parses as infinity.
    // si_value = inf * 0.001 = inf — must emit overflow diagnostic.
    let big_num = "9".repeat(309);
    let src = format!("structure S {{ param y : Length = {}mm }}", big_num);
    let module = parse_and_compile(&src);
    let errors = errors_only(&module);
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("overflow") || d.message.contains("finite")),
        "expected overflow diagnostic for non-finite quantity literal in structure param; got: {:?}",
        errors
    );
}

// ─── step-12 (task-208): regression — valid quantity literals in structure params ─

#[test]
fn valid_quantity_literal_in_structure_param_still_compiles() {
    // A normal quantity literal in a structure param must still compile without errors.
    let module = parse_and_compile("structure Foo { param x : Length = 10mm }");
    assert!(
        errors_only(&module).is_empty(),
        "expected no errors for valid quantity literal in structure param; got: {:?}",
        errors_only(&module)
    );
    // Verify the compiled value is correct: 10mm = 0.01m
    let template = module
        .templates
        .iter()
        .find(|t| t.name == "Foo")
        .expect("Foo not found");
    let x_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "x")
        .expect("x not found");
    if let Some(expr) = &x_cell.default_expr {
        if let reify_types::CompiledExprKind::Literal(reify_types::Value::Scalar {
            si_value, ..
        }) = &expr.kind
        {
            assert!(
                (si_value - 0.01).abs() < 1e-9,
                "10mm should be 0.01m, got {}",
                si_value
            );
        } else {
            panic!("expected scalar literal for x, got {:?}", expr.kind);
        }
    } else {
        panic!("x has no default_expr");
    }
}

// ─── task-208: prelude unit seeding in compile_with_prelude ─────────────────

fn compile_with_stdlib_helper(source: &str) -> CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("user_test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    compile_with_prelude(&parsed, stdlib_loader::load_stdlib())
}

/// compile_with_prelude resolves a prelude unit (mm) in a structure param.
/// The user source declares NO units — mm comes entirely from prelude.
#[test]
fn prelude_unit_resolves_in_structure_param() {
    let source = r#"
structure def Bracket {
    param width : Length = 10mm
}
"#;
    let module = compile_with_stdlib_helper(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "compile_with_prelude should resolve prelude mm without errors, got: {:?}",
        errors
    );
    let template = module
        .templates
        .iter()
        .find(|t| t.name == "Bracket")
        .expect("Bracket not found");
    let width = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "width")
        .expect("width not found");
    if let Some(expr) = &width.default_expr {
        if let reify_types::CompiledExprKind::Literal(reify_types::Value::Scalar {
            si_value, ..
        }) = &expr.kind
        {
            assert!(
                (si_value - 0.01).abs() < 1e-9,
                "10mm should be 0.01m via prelude, got {}",
                si_value
            );
        } else {
            panic!("expected scalar literal, got {:?}", expr.kind);
        }
    } else {
        panic!("width has no default_expr");
    }
}

/// compile_with_prelude resolves prelude units in unit conversion expressions.
/// User defines 'unit mylen : Length = 0.0254mm' with NO local mm declaration.
#[test]
fn prelude_unit_resolves_in_unit_conversion_expr() {
    let source = "unit mylen : Length = 0.0254mm";
    let module = compile_with_stdlib_helper(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "prelude mm should resolve in unit conversion, got errors: {:?}",
        errors
    );
    let mylen = module
        .units
        .iter()
        .find(|u| u.name == "mylen")
        .expect("mylen not found");
    assert!(
        (mylen.factor - 0.0000254).abs() < 1e-12,
        "mylen factor should be 0.0254 * 0.001 = 0.0000254, got {}",
        mylen.factor
    );
}

// ─── step-11 (task-208): module-local duplicate of prelude unit ──────────────

/// Module-local unit declaration with the same name as a prelude unit produces
/// a "duplicate" error diagnostic.
#[test]
fn local_unit_duplicate_of_prelude_emits_error() {
    let source = "unit mm : Length = 0.002";
    let module = compile_with_stdlib_helper(source);
    let errors = errors_only(&module);
    assert!(
        errors
            .iter()
            .any(|d| { d.message.contains("duplicate") && d.message.contains("mm") }),
        "expected a 'duplicate' error mentioning 'mm' when module-local unit shadows prelude, got: {:?}",
        errors
    );
}

// ─── step-13 (task-208): compile_with_stdlib convenience function ────────────

/// compile_with_stdlib() compiles user source with full stdlib prelude and
/// resolves '10mm' correctly.
#[test]
fn compile_with_stdlib_resolves_quantity_literals() {
    let source = r#"
structure def Plate {
    param thickness : Length = 10mm
}
"#;
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let module = compile_with_stdlib(&parsed);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "compile_with_stdlib should produce no errors, got: {:?}",
        errors
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "Plate")
        .expect("Plate not found");
    let thickness = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "thickness")
        .expect("thickness not found");
    if let Some(expr) = &thickness.default_expr {
        if let reify_types::CompiledExprKind::Literal(reify_types::Value::Scalar {
            si_value, ..
        }) = &expr.kind
        {
            assert!(
                (si_value - 0.01).abs() < 1e-9,
                "10mm should be 0.01m via compile_with_stdlib, got {}",
                si_value
            );
        } else {
            panic!("expected scalar literal for thickness, got {:?}", expr.kind);
        }
    } else {
        panic!("thickness has no default_expr");
    }
}

// ─── step-15 (task-208): regression — all 9 hardcoded units via compile_with_stdlib ─

/// All 9 original hardcoded units resolve correctly via compile_with_stdlib().
/// Each unit's si_value must match the hardcoded conversion factor.
#[test]
fn all_nine_hardcoded_units_resolve_via_stdlib() {
    // (unit_suffix, expected_factor_for_1_unit)
    let cases: &[(&str, f64)] = &[
        ("mm", 0.001),
        ("cm", 0.01),
        ("m", 1.0),
        ("in", 0.0254),
        ("deg", std::f64::consts::PI / 180.0),
        ("rad", 1.0),
        ("kg", 1.0),
        ("g", 0.001),
        ("s", 1.0),
    ];

    for (unit, expected_factor) in cases {
        // Each unit is tested with value 1.0, so si_value == factor
        let source = format!("structure def T_{u} {{ param v : Real = 1{u} }}", u = unit);
        let parsed = reify_syntax::parse(&source, ModulePath::single("test"));
        assert!(
            parsed.errors.is_empty(),
            "parse errors for unit '{}': {:?}",
            unit,
            parsed.errors
        );

        let module = compile_with_stdlib(&parsed);
        let errors = errors_only(&module);
        assert!(
            errors.is_empty(),
            "compile_with_stdlib errors for unit '{}': {:?}",
            unit,
            errors
        );

        let template = module
            .templates
            .first()
            .unwrap_or_else(|| panic!("no template for unit '{}'", unit));
        let cell = template
            .value_cells
            .first()
            .unwrap_or_else(|| panic!("no value cell for unit '{}'", unit));
        let expr = cell
            .default_expr
            .as_ref()
            .unwrap_or_else(|| panic!("no default_expr for unit '{}'", unit));
        if let reify_types::CompiledExprKind::Literal(reify_types::Value::Scalar {
            si_value, ..
        }) = &expr.kind
        {
            assert!(
                (si_value - expected_factor).abs() < 1e-15,
                "1{} should have si_value ≈ {}, got {}",
                unit,
                expected_factor,
                si_value
            );
        } else {
            panic!("expected scalar literal for 1{}, got {:?}", unit, expr.kind);
        }
    }
}

// ─── step-18 (task-706): degF stdlib affine unit at three canonical temperatures ─

/// degF stdlib unit converts correctly at three canonical temperature points:
/// ice (32°F → 273.15K), steam (212°F → 373.15K), body temp (98.6°F → 310.15K).
/// The affine conversion formula is: si_value = value × (5/9) + 255.3722222222222.
/// Note: negative quantity literals like -40degF are parsed as -(40degF), which
/// applies negation after the affine offset — giving a wrong physical value. This
/// is a known parser limitation, so we use positive values only here.
#[test]
fn degf_stdlib_unit_converts_correctly() {
    let source = r#"
structure def TempCheck {
    param ice     : Temperature = 32degF
    param steam   : Temperature = 212degF
    param bodytemp: Temperature = 98.6degF
}
"#;
    let module = compile_with_stdlib_helper(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "compile_with_stdlib_helper should resolve degF without errors, got: {:?}",
        errors
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "TempCheck")
        .expect("TempCheck not found");

    // Helper: extract si_value from a named param's default_expr
    let si_value_of = |name: &str| -> f64 {
        let cell = template
            .value_cells
            .iter()
            .find(|c| c.id.member == name)
            .unwrap_or_else(|| panic!("{} not found", name));
        let expr = cell
            .default_expr
            .as_ref()
            .unwrap_or_else(|| panic!("{} has no default_expr", name));
        if let reify_types::CompiledExprKind::Literal(reify_types::Value::Scalar {
            si_value, ..
        }) = &expr.kind
        {
            *si_value
        } else {
            panic!("expected scalar literal for {}, got {:?}", name, expr.kind);
        }
    };

    let ice_si = si_value_of("ice");
    let steam_si = si_value_of("steam");
    let bodytemp_si = si_value_of("bodytemp");

    assert!(
        (ice_si - 273.15).abs() < 1e-6,
        "32degF (ice point) should be 273.15K, got {}",
        ice_si
    );
    assert!(
        (steam_si - 373.15).abs() < 1e-6,
        "212degF (steam point) should be 373.15K, got {}",
        steam_si
    );
    assert!(
        (bodytemp_si - 310.15).abs() < 1e-6,
        "98.6degF (body temperature) should be 310.15K, got {}",
        bodytemp_si
    );
}

// ─── step-22 (task-706): prelude unit collision diagnostic mentions stdlib ──

/// When a module-local unit shadows a prelude unit, the diagnostic should:
/// (1) contain 'duplicate' and the unit name,
/// (2) mention 'stdlib prelude' so the user knows the original is built-in,
/// (3) NOT include any label with SourceSpan::empty(0) (the misleading
///     prelude sentinel that points to byte 0 of the user's file).
#[test]
fn prelude_unit_collision_diagnostic_mentions_stdlib() {
    let source = "unit mm : Length = 0.002";
    let module = compile_with_stdlib_helper(source);
    let errors = errors_only(&module);

    // (1) At least one error mentions 'duplicate' and 'mm'
    let dup_diag = errors
        .iter()
        .find(|d| d.message.contains("duplicate") && d.message.contains("mm"));
    assert!(
        dup_diag.is_some(),
        "expected a 'duplicate' error mentioning 'mm', got: {:?}",
        errors
    );
    let dup_diag = dup_diag.unwrap();

    // (2) The error message should mention 'stdlib prelude'
    assert!(
        dup_diag.message.contains("stdlib prelude"),
        "expected diagnostic message to contain 'stdlib prelude', got: {:?}",
        dup_diag.message
    );

    // (3) No label should have SourceSpan::empty(0) — the misleading prelude sentinel
    let empty_span = reify_types::SourceSpan::empty(0);
    for label in &dup_diag.labels {
        assert_ne!(
            label.span, empty_span,
            "diagnostic label '{}' has SourceSpan::empty(0) — misleading prelude offset",
            label.message
        );
    }
}
