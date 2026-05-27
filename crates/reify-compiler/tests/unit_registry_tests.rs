//! Tests for the unit declaration registry (task 208).
//!
//! Validates UnitEntry, UnitRegistry, resolve_dimension_type,
//! evaluate_const_expr, compile_unit, and the full unit pre-pass in compile().

mod common;

use reify_compiler::{
    AutoTypeSubstitution, UnitEntry, UnitRegistry, compile, compile_with_prelude,
    compile_with_stdlib,
};
use reify_test_support::{compile_source, compile_source_with_stdlib, errors_only};
use reify_core::{DimensionVector, ModulePath, SourceSpan};

// ─── step-1: UnitEntry and UnitRegistry data structures ───────────────────────

#[test]
fn unit_entry_fields_exist() {
    // Construct a UnitEntry directly to verify the struct fields are accessible.
    let dummy_span = SourceSpan::new(0, 0);
    let hash = reify_core::ContentHash::of_str("meter");
    let entry = UnitEntry {
        name: "meter".to_string(),
        dimension: DimensionVector::LENGTH,
        factor: 1.0,
        offset: None,
        is_pub: true,
        span: dummy_span,
        content_hash: hash,
        source_module: None,
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
        content_hash: reify_core::ContentHash::of_str("mm"),
        source_module: None,
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
        content_hash: reify_core::ContentHash::of_str("mm"),
        source_module: None,
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
        content_hash: reify_core::ContentHash::of_str("mm"),
        source_module: None,
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
        content_hash: reify_core::ContentHash::of_str("mm-v1"),
        source_module: None,
    };
    let entry2 = UnitEntry {
        name: "mm".to_string(),
        dimension: DimensionVector::LENGTH,
        factor: 0.002,
        offset: None,
        is_pub: true,
        span: SourceSpan::new(10, 15),
        content_hash: reify_core::ContentHash::of_str("mm-v2"),
        source_module: None,
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
    let module = compile_source("unit meter : Length");
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
    let module = compile_source("unit kilogram : Mass");
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
    let module = compile_source("unit second : Time");
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
    let module = compile_source("unit kelvin : Temperature");
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
    let module = compile_source("unit radian : Angle");
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
    let module = compile_source("unit sq_meter : Area");
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
    let module = compile_source("unit cubic_meter : Volume");
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
    let module = compile_source("unit newton : Force");
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
    assert_eq!(unit.dimension, reify_core::dimension::FORCE);
}

#[test]
fn resolve_dimension_type_current() {
    let module = compile_source("unit ampere : Current");
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
    let module = compile_source("unit foo : UnknownDimension");
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
    let module = compile_source("unit cm : Length = 0.01");
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
    let module = compile_source("unit inch_mm : Length = 25.4 * 0.001");
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
    let module = compile_source("unit milli : Length = 1 / 1000");
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
    let module =
        compile_source("unit m : Length\nunit mm : Length = 0.001\nunit thou : Length = 0.0254mm");
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
    let module = compile_source("unit meter : Length");
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
    let module = compile_source("unit mm : Length = 0.001");
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
    let module = compile_source("unit degC : Temperature = 1 offset 273.15");
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
    let module = compile_source("unit foo : Luminance = 1.0");
    let errors = errors_only(&module);
    assert!(!errors.is_empty(), "expected error for unknown dimension");
}

// ─── step-9: compile() pre-pass populates CompiledModule.units ────────────────

#[test]
fn compiled_module_has_units_field() {
    let module = compile_source("unit mm : Length = 0.001\nunit m : Length");
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
    let module = compile_source("unit mm : Length = 0.001\nunit m : Length");
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
    let module = compile_source(
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
    let default_expr = width_cell
        .default_expr
        .as_ref()
        .expect("width cell has no default_expr");
    let (si_value, _dimension) = common::expect_scalar(default_expr);
    assert!(
        (si_value - 0.000254).abs() < common::UNIT_EPSILON,
        "expected si_value≈0.000254, got {}",
        si_value
    );
}

#[test]
fn hardcoded_units_still_work_without_declarations() {
    // All hardcoded units should still work when no unit declarations are present.
    // Covers mm (Length), deg (Angle), and kg (Mass) fallback paths.
    let module = compile_source(
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
        let (si_value, _dimension) = common::expect_scalar(expr);
        assert!(
            (si_value - expected).abs() < common::UNIT_EPSILON,
            "{} expected {}, got {}",
            desc,
            expected,
            si_value
        );
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
    let module = compile_source("unit mm : Length = 0.001\nunit mm : Length = 0.001");
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
    let module =
        compile_source("unit m : Length\nunit mm : Length = 0.001\nunit thou : Length = 0.0254mm");
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
    let module = compile_source("pub unit mm : Length = 0.001");
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
    let module = compile_source("unit internal_mm : Length = 0.001");
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
    let module = compile_source(
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
    let default_expr = width_cell
        .default_expr
        .as_ref()
        .expect("width has no default_expr");
    let (si_value, _dimension) = common::expect_scalar(default_expr);
    assert!(
        (si_value - 0.05).abs() < common::UNIT_EPSILON,
        "50mm should be 0.05m, got {}",
        si_value
    );
}

// ─── step-21: offset-only unit ────────────────────────────────────────────────

#[test]
fn offset_only_unit_has_factor_one() {
    // `unit kelvin : Temperature offset 273.15` -> factor=1.0, offset=Some(273.15)
    let module = compile_source("unit kelvin : Temperature offset 273.15");
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
    let module = compile_source(
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
    let expr = t_cell.default_expr.as_ref().expect("t has no default_expr");
    let (si_value, _dimension) = common::expect_scalar(expr);
    assert!(
        (si_value - 373.15).abs() < common::UNIT_EPSILON,
        "100kelvin should be 373.15K, got {}",
        si_value
    );
}

// ─── step-27: compile_unit returns None for broken conversion expression ──────

#[test]
fn compile_unit_broken_conversion_not_registered() {
    // 'some_var' is an identifier — evaluate_const_expr() returns None for
    // non-constant expressions. compile_unit() should return None, so the unit
    // is NOT added to module.units.
    let module = compile_source("unit broken : Length = some_var");
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
    let module = compile_source("unit broken_off : Temperature = 1 offset some_var");
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
    let module = compile_source(
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
        let expr = cell
            .default_expr
            .as_ref()
            .unwrap_or_else(|| panic!("{}: no default_expr", name));
        let (si_value, _dimension) = common::expect_scalar(expr);
        assert!(
            (si_value - expected_si).abs() < common::UNIT_EPSILON,
            "{}: expected {}, got {}",
            name,
            expected_si,
            si_value
        );
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
    let module = compile_source(
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
    let module = compile_source("unit mm : Length = 0.001\nunit thou : Length = 0.0254mm");
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
    let module = compile_source(
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
    let expr = t_cell.default_expr.as_ref().expect("t has no default_expr");
    let (si_value, _dimension) = common::expect_scalar(expr);
    assert!(
        (si_value - 298.15).abs() < common::UNIT_EPSILON,
        "25degC should be 298.15K, got {}",
        si_value
    );
}

// ─── step-1 (task-208): evaluate_const_expr rejects non-finite arithmetic ─────

#[test]
fn overflow_multiplication_rejected() {
    // f64::MAX * 2.0 → inf — must NOT be registered.
    let src = format!("unit huge : Length = {} * 2.0", f64::MAX);
    let module = compile_source(&src);
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
    let module = compile_source(&src);
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
    let module = compile_source(&src);
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
    let module = compile_source("unit z : Length = 0");
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
    let module = compile_source("unit z2 : Length = 0 * 1");
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
    let module = compile_source(&src);
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
    let module = compile_source(&src);
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
    let module = compile_source("unit inch : Length = 25.4 * 0.001");
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
    let module = compile_source("unit degC : Temperature = 1 offset 273.15");
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
    let module = compile_source("unit mm : Length = 0.001\nunit thou : Length = 0.0254mm");
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
    let module = compile_source("unit neg : Length = 0 - 1");
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
    let module = compile_source("unit pico : Length = 0.000000000001");
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
    let module = compile_source(&src);
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
    let module = compile_source(&src);
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
    let module = compile_source("structure Foo { param x : Length = 10mm }");
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
    let expr = x_cell.default_expr.as_ref().expect("x has no default_expr");
    let (si_value, _dimension) = common::expect_scalar(expr);
    assert!(
        (si_value - 0.01).abs() < common::UNIT_EPSILON,
        "10mm should be 0.01m, got {}",
        si_value
    );
}

// ─── task-208: prelude unit seeding in compile_with_prelude ─────────────────

/// compile_with_prelude resolves a prelude unit (mm) in a structure param.
/// The user source declares NO units — mm comes entirely from prelude.
#[test]
fn prelude_unit_resolves_in_structure_param() {
    let source = r#"
structure def Bracket {
    param width : Length = 10mm
}
"#;
    let module = compile_source_with_stdlib(source);
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
    let expr = width
        .default_expr
        .as_ref()
        .expect("width has no default_expr");
    let (si_value, _dimension) = common::expect_scalar(expr);
    assert!(
        (si_value - 0.01).abs() < common::UNIT_EPSILON,
        "10mm should be 0.01m via prelude, got {}",
        si_value
    );
}

/// compile_with_prelude resolves prelude units in unit conversion expressions.
/// User defines 'unit mylen : Length = 0.0254mm' with NO local mm declaration.
#[test]
fn prelude_unit_resolves_in_unit_conversion_expr() {
    let source = "unit mylen : Length = 0.0254mm";
    let module = compile_source_with_stdlib(source);
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
    let module = compile_source_with_stdlib(source);
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
    let expr = thickness
        .default_expr
        .as_ref()
        .expect("thickness has no default_expr");
    let (si_value, _dimension) = common::expect_scalar(expr);
    assert!(
        (si_value - 0.01).abs() < common::UNIT_EPSILON,
        "10mm should be 0.01m via compile_with_stdlib, got {}",
        si_value
    );
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
        let (si_value, _dimension) = common::expect_scalar(expr);
        assert!(
            (si_value - expected_factor).abs() < 1e-15,
            "1{} should have si_value ≈ {}, got {}",
            unit,
            expected_factor,
            si_value
        );
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
    let module = compile_source_with_stdlib(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "compile_source_with_stdlib should resolve degF without errors, got: {:?}",
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
        let (si_value, _dimension) = common::expect_scalar(expr);
        si_value
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
    let module = compile_source_with_stdlib(source);
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

    // (3) Two labels: labels[0] is the user's in-file dup decl span (non-empty,
    //     not SourceSpan::empty(0)); labels[1] is the prelude sentinel with a
    //     message containing 'stdlib prelude'.
    assert_eq!(
        dup_diag.labels.len(),
        2,
        "stdlib collision should emit two labels, got {:?}",
        dup_diag.labels
    );
    let empty_span = reify_core::SourceSpan::empty(0);
    assert_ne!(
        dup_diag.labels[0].span, empty_span,
        "first label '{}' must not be SourceSpan::empty(0)",
        dup_diag.labels[0].message
    );
    assert!(
        dup_diag.labels[1].span.is_prelude(),
        "second label '{}' must have is_prelude() span, got {:?}",
        dup_diag.labels[1].message,
        dup_diag.labels[1].span
    );
    assert!(
        dup_diag.labels[1].message.contains("stdlib prelude"),
        "second label message must contain 'stdlib prelude', got: {:?}",
        dup_diag.labels[1].message
    );
}

// ── step-1 (task-1074): cross-module user unit collision blames source module ───

/// When a module-local unit shadows a unit from a user module (not stdlib),
/// the diagnostic should:
/// (a) NOT contain 'stdlib',
/// (b) mention the source module name ('dep'),
/// (c) have no label with SourceSpan::empty(0).
#[test]
fn cross_module_user_unit_collision_blames_source_module() {
    // Build a 'dep' module with a pub unit 'myunit'
    let dep_parsed = reify_syntax::parse(
        "pub unit myunit : Length = 0.001",
        ModulePath::single("dep"),
    );
    assert!(
        dep_parsed.errors.is_empty(),
        "parse errors in dep: {:?}",
        dep_parsed.errors
    );
    let dep_module = compile(&dep_parsed);
    assert!(
        errors_only(&dep_module).is_empty(),
        "dep compilation errors: {:?}",
        errors_only(&dep_module)
    );

    // User module redeclares 'myunit' — dep module is used as prelude
    let user_parsed = reify_syntax::parse(
        "unit myunit : Length = 0.002",
        ModulePath::single("user_module"),
    );
    assert!(
        user_parsed.errors.is_empty(),
        "parse errors in user: {:?}",
        user_parsed.errors
    );
    let user_module = compile_with_prelude(&user_parsed, &[dep_module]);
    let errors = errors_only(&user_module);

    // (1) At least one error mentions 'duplicate' and 'myunit'
    let dup_diag = errors
        .iter()
        .find(|d| d.message.contains("duplicate") && d.message.contains("myunit"));
    assert!(
        dup_diag.is_some(),
        "expected a 'duplicate' error mentioning 'myunit', got: {:?}",
        errors
    );
    let dup_diag = dup_diag.unwrap();

    // (a) The error message should NOT contain 'stdlib'
    assert!(
        !dup_diag.message.contains("stdlib"),
        "diagnostic should NOT mention 'stdlib' for user module collision, got: {:?}",
        dup_diag.message
    );

    // (b) The error message should mention the source module name 'dep'
    assert!(
        dup_diag.message.contains("dep"),
        "diagnostic should mention 'dep' module name, got: {:?}",
        dup_diag.message
    );

    // (c) Two labels: labels[0] is the user's in-file dup decl span;
    // labels[1] is the prelude sentinel with provenance in its message.
    common::assert_prelude_collision_labels(dup_diag);
}

// ── step-7 (task-416): affine unit referencing another affine unit is rejected ─

#[test]
fn affine_unit_in_conversion_referencing_other_affine_rejected() {
    // When unit B's conversion expression references unit A, and unit A has an
    // offset (is affine), evaluate_const_expr() must reject it — because the
    // offset only makes sense for runtime value expressions, not for defining
    // a conversion factor.  This edge case complements step-40 which only tests
    // a single-level affine rejection; here we verify the guard works even when
    // the referenced unit itself has an offset (i.e., another affine unit).
    //
    // Scenario: degC is affine (offset 273.15).  Declaring
    //   unit myK : Temperature = 1.0degC
    // means "1 myK = 1 degC", which would involve adding an offset to define a
    // conversion factor — that is invalid.
    let module = compile_source(
        "unit degC : Temperature = 1 offset 273.15\nunit myK : Temperature = 1.0degC",
    );

    // myK should NOT be registered
    assert!(
        !module.units.iter().any(|u| u.name == "myK"),
        "myK should not be registered when its conversion references an affine unit"
    );

    // An error diagnostic should mention affine or offset
    let errors = errors_only(&module);
    assert!(
        !errors.is_empty(),
        "expected an error when conversion expression references affine unit"
    );
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("affine") || d.message.contains("offset")),
        "expected diagnostic about affine/offset unit in conversion; got: {:?}",
        errors
    );
}

// ─── helper-extraction tests ──────────────────────────────────────────────────

#[test]
fn test_expect_scalar_extracts_si_value_and_dimension() {
    // Compile a simple Length param and verify that common::expect_scalar
    // can extract the si_value and dimension without nested if-let boilerplate.
    let module = compile_source("structure S { param x : Length = 10mm }");
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
    let cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "x")
        .expect("x not found");
    let expr = cell.default_expr.as_ref().expect("x has no default_expr");
    let (si_value, dimension) = common::expect_scalar(expr);
    assert!(
        (si_value - 0.01).abs() < common::UNIT_EPSILON,
        "expected si_value≈0.01 (10mm), got {}",
        si_value
    );
    assert_eq!(dimension, DimensionVector::LENGTH);
}

#[test]
fn test_expect_binop_extracts_op_and_operands() {
    // Compile a source with a let-binding BinOp expression and verify that
    // common::expect_binop can extract op/left/right without nested if-let boilerplate.
    let module = compile_source(
        "unit thou : Length = 0.0000254\n\
         structure S {\n\
             param w : Length = 10thou\n\
             let x = w + 5thou\n\
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
    let cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "x")
        .expect("x not found");
    let expr = cell.default_expr.as_ref().expect("x has no default_expr");
    let (op, _left, right) = common::expect_binop(expr);
    assert!(
        matches!(op, reify_ir::BinOp::Add),
        "expected Add op for w + 5thou, got {:?}",
        op
    );
    // The right operand is `5thou` — verify si_value and dimension via expect_scalar.
    let (si_value, dimension) = common::expect_scalar(right);
    let expected = 5.0 * 0.0000254;
    assert!(
        (si_value - expected).abs() < common::UNIT_EPSILON,
        "expected si_value≈{} (5 * 0.0000254), got {}",
        expected,
        si_value
    );
    assert_eq!(dimension, DimensionVector::LENGTH);
}

// ── step-7 (task-765): cross-prelude collision emits a warning ─────────────────

/// Build a prelude `CompiledModule` that exports `pub unit foo : Length = <factor>`
/// under `ModulePath::single(name)`. Panics on any parse or compile errors so
/// callers get a descriptive failure site instead of a silent misconfiguration.
fn prelude_module(name: &str, factor: f64) -> reify_compiler::CompiledModule {
    // Debug fmt preserves the decimal point so the source lexes as a float literal
    // (Display would emit '1' for 1.0, which could lex as an integer literal).
    let source = format!("pub unit foo : Length = {factor:?}");
    let parsed = reify_syntax::parse(&source, ModulePath::single(name));
    assert!(
        parsed.errors.is_empty(),
        "parse errors in {}: {:?}",
        name,
        parsed.errors
    );
    let compiled = compile(&parsed);
    assert!(
        errors_only(&compiled).is_empty(),
        "{} compilation errors: {:?}",
        name,
        errors_only(&compiled)
    );
    compiled
}

/// When two prelude modules export the same pub unit name, `compile_with_prelude`
/// should emit exactly one `Severity::Warning` diagnostic whose message:
/// (a) mentions the unit name 'foo',
/// (b) names both conflicting module paths ('mod_a' and 'mod_b'),
/// (c) mentions 'prelude' (in message or label).
///
/// The compilation must still succeed (no Error diagnostics), and last-wins
/// ordering is preserved: the later module's (mod_b's) factor 2.0 is used.
#[test]
fn prelude_module_unit_collision_emits_warning() {
    // mod_a: pub unit foo with SI factor 1.0 m
    let mod_a = prelude_module("mod_a", 1.0);

    // mod_b: pub unit foo with SI factor 2.0 m — same name, different factor
    let mod_b = prelude_module("mod_b", 2.0);

    // User module uses `foo` (which will resolve to mod_b's definition via last-wins).
    // `unit bar : Length = 1foo` means bar's SI factor = 1 * foo's factor.
    let user_parsed = reify_syntax::parse(
        "unit bar : Length = 1foo",
        ModulePath::single("user_module"),
    );
    assert!(
        user_parsed.errors.is_empty(),
        "parse errors in user: {:?}",
        user_parsed.errors
    );
    let user_module = compile_with_prelude(&user_parsed, &[mod_a, mod_b]);

    // (a) Exactly one Warning diagnostic mentioning 'foo'
    let warnings: Vec<_> = user_module
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Warning)
        .collect();
    let collision_warns: Vec<_> = warnings
        .iter()
        .filter(|w| w.message.contains("foo"))
        .collect();
    assert_eq!(
        collision_warns.len(),
        1,
        "expected exactly 1 collision warning, got: {:?}",
        warnings
    );
    let collision_warn = collision_warns[0];

    // (b) Warning names both conflicting module paths
    assert!(
        collision_warn.message.contains("mod_a"),
        "warning must name first module 'mod_a', got: {:?}",
        collision_warn.message
    );
    assert!(
        collision_warn.message.contains("mod_b"),
        "warning must name winning module 'mod_b', got: {:?}",
        collision_warn.message
    );

    // (c) Warning mentions 'prelude' (in message or in a label)
    let mentions_prelude = collision_warn.message.contains("prelude")
        || collision_warn
            .labels
            .iter()
            .any(|l| l.message.contains("prelude"));
    assert!(
        mentions_prelude,
        "warning must mention 'prelude', got: {:?}",
        collision_warn
    );

    // No Error-severity diagnostics — user compilation succeeds
    let errors = errors_only(&user_module);
    assert!(
        errors.is_empty(),
        "user compilation should succeed (no errors), got: {:?}",
        errors
    );

    // Last-wins: mod_b's foo (factor 2.0 SI) wins, so bar's factor = 2.0
    let bar = user_module.units.iter().find(|u| u.name == "bar");
    assert!(bar.is_some(), "'bar' should be compiled successfully");
    let bar = bar.unwrap();
    assert!(
        (bar.factor - 2.0).abs() < common::UNIT_EPSILON,
        "bar's SI factor should be 2.0 (mod_b's foo wins last-wins), got {}",
        bar.factor
    );
}

// ── amendment (task-765): three-prelude collision — two chained warnings ───────

/// When three prelude modules all export the same unit name, the seeding loop
/// fires two warnings (one per collision), each naming the pair of modules
/// involved at that moment (chained last-wins).
///
/// Specifically:
///  - mod_a seeds first (no collision yet)
///  - mod_b collides with mod_a → warning #1 names "mod_a" and "mod_b"
///  - mod_c collides with mod_b → warning #2 names "mod_b" and "mod_c"
///  - Final winner is mod_c (factor 3.0)
#[test]
fn three_prelude_collision_emits_two_chained_warnings() {
    // mod_a: pub unit foo with SI factor 1.0 m
    let mod_a = prelude_module("mod_a", 1.0);

    // mod_b: pub unit foo with SI factor 2.0 m
    let mod_b = prelude_module("mod_b", 2.0);

    // mod_c: pub unit foo with SI factor 3.0 m — final winner
    let mod_c = prelude_module("mod_c", 3.0);

    // User module: references foo (resolved via last-wins to mod_c)
    let user_parsed = reify_syntax::parse(
        "unit bar : Length = 1foo",
        ModulePath::single("user_module"),
    );
    assert!(
        user_parsed.errors.is_empty(),
        "parse errors in user: {:?}",
        user_parsed.errors
    );

    let user_module = compile_with_prelude(&user_parsed, &[mod_a, mod_b, mod_c]);

    // (a) Exactly two Warning diagnostics mentioning 'foo'
    let warnings: Vec<_> = user_module
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Warning && d.message.contains("foo"))
        .collect();
    assert_eq!(
        warnings.len(),
        2,
        "expected exactly 2 warnings about 'foo', got: {:?}",
        warnings
    );

    // (b) Warning #1 names mod_a and mod_b (the first collision pair)
    let warn1 = warnings
        .iter()
        .find(|w| w.message.contains("mod_a") && w.message.contains("mod_b"));
    assert!(
        warn1.is_some(),
        "expected a warning naming both 'mod_a' and 'mod_b', warnings: {:?}",
        warnings
    );

    // (c) Warning #2 names mod_b and mod_c (the second collision pair)
    let warn2 = warnings
        .iter()
        .find(|w| w.message.contains("mod_b") && w.message.contains("mod_c"));
    assert!(
        warn2.is_some(),
        "expected a warning naming both 'mod_b' and 'mod_c', warnings: {:?}",
        warnings
    );

    // No Error-severity diagnostics — user compilation succeeds
    let errors = errors_only(&user_module);
    assert!(
        errors.is_empty(),
        "user compilation should succeed, got: {:?}",
        errors
    );

    // (d) Last-wins: mod_c's foo (factor 3.0) is the final winner
    let bar = user_module.units.iter().find(|u| u.name == "bar");
    assert!(bar.is_some(), "'bar' should be compiled successfully");
    let bar = bar.unwrap();
    assert!(
        (bar.factor - 3.0).abs() < common::UNIT_EPSILON,
        "bar's SI factor should be 3.0 (mod_c's foo wins last-wins), got {}",
        bar.factor
    );
}

// ── task-1931: intra-module duplicate prelude units — no nonsense collision warning ──

/// Regression test: when a (malformed) prelude `CompiledModule` contains two
/// `CompiledUnit` entries with the same name from the same module, the seeding
/// loop must NOT emit a `'mod_a' and 'mod_a'`-style collision warning. That
/// message is nonsense — it names the same module twice, implying a cross-module
/// conflict that doesn't exist.
///
/// Intra-module duplicate unit declarations are rejected earlier by `compile()`
/// before they can reach `CompiledModule.units`. The cross-prelude warning only
/// makes sense for genuine cross-module pairs. This test bypasses `compile()` by
/// hand-constructing the malformed `CompiledModule` directly.
#[test]
fn intra_module_duplicate_prelude_units_suppresses_nonsense_collision_warning() {
    use reify_compiler::{CompiledModule, CompiledUnit};
    use reify_core::ContentHash;

    // Hand-construct a malformed prelude module with two CompiledUnit entries
    // both named 'foo' — bypassing compile() which would reject the duplicate.
    // First entry: factor 1.0; second entry: factor 2.0 (last-wins in registry).
    let malformed_mod = CompiledModule {
        path: ModulePath::single("mod_a"),
        imports: vec![],
        enum_defs: vec![],
        functions: vec![],
        trait_defs: vec![],
        fields: vec![],
        compiled_purposes: vec![],
        templates: vec![],
        units: vec![
            CompiledUnit {
                name: "foo".to_string(),
                is_pub: true,
                dimension: DimensionVector::LENGTH,
                factor: 1.0,
                offset: None,
                content_hash: ContentHash::of_str("foo-1"),
            },
            CompiledUnit {
                name: "foo".to_string(),
                is_pub: true,
                dimension: DimensionVector::LENGTH,
                factor: 2.0,
                offset: None,
                content_hash: ContentHash::of_str("foo-2"),
            },
        ],
        type_aliases: vec![],
        constraint_defs: vec![],
        pragmas: vec![],
        default_tolerance: None,
        declared_version: None,
        solver_pragma: None,
        kernel_pragma: None,
        auto_type_substitution: AutoTypeSubstitution::default(),
        diagnostics: vec![],
        content_hash: ContentHash::of_str(""),
    };

    // Parse a trivial user module that references `foo`.
    let user_parsed = reify_syntax::parse(
        "unit bar : Length = 1foo",
        ModulePath::single("user_module"),
    );
    assert!(
        user_parsed.errors.is_empty(),
        "parse errors in user: {:?}",
        user_parsed.errors
    );
    let user_module = compile_with_prelude(&user_parsed, &[malformed_mod]);

    // (a) No Warning diagnostic containing the nonsense substring "'mod_a' and 'mod_a'".
    //     Current (unfixed) code emits: "prelude unit 'foo' declared in both 'mod_a' and 'mod_a'; last-wins"
    let nonsense_warnings: Vec<_> = user_module
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == reify_core::Severity::Warning
                && d.message.contains("'mod_a' and 'mod_a'")
        })
        .collect();
    assert!(
        nonsense_warnings.is_empty(),
        "no nonsense \"'mod_a' and 'mod_a'\" warning should fire for intra-module dup, got: {:?}",
        nonsense_warnings
    );

    // (b) Stronger: no Warning about 'foo' with 'declared in both' at all —
    //     intra-module dups are silent in the seeding loop; that responsibility
    //     belongs to compile().
    let foo_collision_warns: Vec<_> = user_module
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == reify_core::Severity::Warning
                && d.message.contains("foo")
                && d.message.contains("declared in both")
        })
        .collect();
    assert!(
        foo_collision_warns.is_empty(),
        "no 'declared in both' warning for 'foo' should fire for intra-module dup, got: {:?}",
        foo_collision_warns
    );

    // (c) User compilation still succeeds — the guard doesn't regress the happy path.
    let errors = errors_only(&user_module);
    assert!(
        errors.is_empty(),
        "user compilation should succeed (no errors), got: {:?}",
        errors
    );

    // (d) Last-wins semantics still apply in the registry — second 'foo' (factor 2.0) wins.
    //     The fix changes only the diagnostic emission, not the registry overwrite.
    let bar = user_module.units.iter().find(|u| u.name == "bar");
    assert!(bar.is_some(), "'bar' should be compiled successfully");
    let bar = bar.unwrap();
    assert!(
        (bar.factor - 2.0).abs() < common::UNIT_EPSILON,
        "bar's SI factor should be 2.0 (last-wins: second foo wins), got {}",
        bar.factor
    );
}

// ─── UnitEntry::from_compiled_for_prelude ────────────────────────────────────

#[test]
fn from_compiled_for_prelude_populates_shared_fields_and_prelude_defaults() {
    use reify_compiler::CompiledUnit;
    use reify_core::ContentHash;

    let hash = ContentHash::of_str("newton");
    let cu = CompiledUnit {
        name: "newton".to_string(),
        is_pub: true,
        dimension: DimensionVector::FORCE,
        factor: 1.5,
        offset: Some(2.5),
        content_hash: hash,
    };

    let entry = UnitEntry::from_compiled_for_prelude(&cu, "test/module".to_string());

    assert_eq!(entry.name, "newton");
    assert!(entry.is_pub);
    assert_eq!(entry.dimension, DimensionVector::FORCE);
    assert!(
        (entry.factor - 1.5).abs() < 1e-12,
        "factor mismatch: {}",
        entry.factor
    );
    assert_eq!(entry.offset, Some(2.5));
    assert_eq!(entry.content_hash, hash);
    assert!(entry.span.is_prelude(), "span must be the prelude sentinel");
    assert_eq!(entry.source_module, Some("test/module".to_string()));
}

/// Pipeline integration: `from_compiled_for_prelude` correctly wires `span`
/// and `source_module` through the full `units_phase → seed_prelude_unit →
/// registry` path so later compilation stages can use those values.
///
/// Observable signal: when a user module shadows a prelude unit the collision
/// diagnostic reveals what was stored in the registry.  If `span` were
/// `SourceSpan::empty(0)` the prelude label's `is_prelude()` check would
/// fail; if `source_module` were `None` the originating module path would be
/// absent from the diagnostic message.
#[test]
fn seeded_prelude_unit_carries_prelude_span_and_module_through_registry() {
    // Build a named prelude module that exports `pub unit foo`.
    let prelude_parsed = reify_syntax::parse(
        "pub unit foo : Length = 1.0",
        ModulePath::single("test_prelude"),
    );
    assert!(
        prelude_parsed.errors.is_empty(),
        "parse errors in prelude: {:?}",
        prelude_parsed.errors
    );
    let prelude_module = compile(&prelude_parsed);
    assert!(
        errors_only(&prelude_module).is_empty(),
        "compile errors in prelude: {:?}",
        errors_only(&prelude_module)
    );

    // User module re-declares `foo`, triggering a registry collision.
    let user_parsed = reify_syntax::parse("unit foo : Length = 2.0", ModulePath::single("user"));
    assert!(
        user_parsed.errors.is_empty(),
        "parse errors in user: {:?}",
        user_parsed.errors
    );
    let user_module = compile_with_prelude(&user_parsed, &[prelude_module]);
    let errors = errors_only(&user_module);

    let dup_diag = errors
        .iter()
        .find(|d| d.message.contains("duplicate") && d.message.contains("foo"))
        .expect("expected a 'duplicate' error mentioning 'foo'");

    // span was set correctly by from_compiled_for_prelude: prelude label must
    // carry SourceSpan::prelude(), not SourceSpan::empty(0).
    assert_eq!(
        dup_diag.labels.len(),
        2,
        "expected two labels (user decl + prelude sentinel), got {:?}",
        dup_diag.labels
    );
    assert!(
        dup_diag.labels[1].span.is_prelude(),
        "prelude label must use SourceSpan::prelude(), got {:?}",
        dup_diag.labels[1].span
    );

    // source_module was set correctly: the originating module path surfaces in
    // the diagnostic message so the user knows where the prelude unit came from.
    assert!(
        dup_diag.message.contains("test_prelude"),
        "diagnostic must mention originating module 'test_prelude', got: {:?}",
        dup_diag.message
    );
}
