//! Tests for stdlib/materials_chemical.ri — §6.6 chemical material traits.
//!
//! Tests validate that the .ri file is loaded by the production stdlib path,
//! that `CorrosionClass`, `BiocompatibilityClass`, `CorrosionResistant`, and
//! `Biocompatible` are correctly represented in the compiled module, and that
//! trait conformance with enum-typed params works as expected.
//!
//! Mirrors the `hardness_scale_enum_and_hard_trait` pattern from
//! materials_mechanical_tests.rs for the two chemical enum types.
//!
//! All tests use the production-path `load_stdlib_module()` helper that
//! exercises the same embedded + sequential-prelude compilation path as
//! production (not a standalone `.ri` file re-read).

use reify_compiler::*;
use reify_test_support::compile_source_with_stdlib;
use reify_types::*;

// ─── helpers ──────────────────────────────────────────────────────────────────

/// Return the `std/materials/chemical` CompiledModule from the production
/// stdlib loader. Exercises the exact same code path as production: embedded
/// source, sequential compilation with growing prelude, OnceLock caching.
///
/// Panics if the module is not found — the expected failure mode until step-8
/// lands the .ri file and loader registration.
fn load_stdlib_module() -> &'static CompiledModule {
    stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/materials/chemical")
        .expect("stdlib should contain std/materials/chemical module")
}

// ─── (a) module loads cleanly with two trait defs and two enum defs ───────────

/// The std/materials/chemical module must load with zero error-severity
/// diagnostics and contain exactly two trait definitions (CorrosionResistant,
/// Biocompatible) and exactly two enum definitions (CorrosionClass,
/// BiocompatibilityClass).
#[test]
fn chemical_module_loads_with_no_errors_two_traits_two_enums() {
    let module = load_stdlib_module();

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected error diagnostics in materials_chemical.ri: {:?}",
        errors
    );

    assert_eq!(
        module.trait_defs.len(),
        2,
        "expected exactly 2 trait defs in std/materials/chemical, got: {:?}",
        module
            .trait_defs
            .iter()
            .map(|t| &t.name)
            .collect::<Vec<_>>()
    );

    assert_eq!(
        module.enum_defs.len(),
        2,
        "expected exactly 2 enum defs in std/materials/chemical, got: {:?}",
        module.enum_defs.iter().map(|e| &e.name).collect::<Vec<_>>()
    );
}

// ─── (b) CorrosionClass enum has exactly [C1, C2, C3, C4, C5] ────────────────

/// CorrosionClass must have exactly 5 variants: C1, C2, C3, C4, C5.
#[test]
fn corrosion_class_enum_has_five_variants() {
    let module = load_stdlib_module();

    let enum_def = module
        .enum_defs
        .iter()
        .find(|e| e.name == "CorrosionClass")
        .expect("expected 'CorrosionClass' enum in std/materials/chemical");

    let expected_variants = ["C1", "C2", "C3", "C4", "C5"];
    assert_eq!(
        enum_def.variants.len(),
        expected_variants.len(),
        "CorrosionClass should have {} variants, got: {:?}",
        expected_variants.len(),
        enum_def.variants
    );
    for variant in &expected_variants {
        assert!(
            enum_def.variants.contains(&variant.to_string()),
            "CorrosionClass missing variant '{}', variants: {:?}",
            variant,
            enum_def.variants
        );
    }
}

// ─── (c) BiocompatibilityClass enum has exactly [USP_Class_I, USP_Class_VI, ISO_10993] ─

/// BiocompatibilityClass must have exactly 3 variants:
/// USP_Class_I, USP_Class_VI, ISO_10993.
#[test]
fn biocompatibility_class_enum_has_three_variants() {
    let module = load_stdlib_module();

    let enum_def = module
        .enum_defs
        .iter()
        .find(|e| e.name == "BiocompatibilityClass")
        .expect("expected 'BiocompatibilityClass' enum in std/materials/chemical");

    let expected_variants = ["USP_Class_I", "USP_Class_VI", "ISO_10993"];
    assert_eq!(
        enum_def.variants.len(),
        expected_variants.len(),
        "BiocompatibilityClass should have {} variants, got: {:?}",
        expected_variants.len(),
        enum_def.variants
    );
    for variant in &expected_variants {
        assert!(
            enum_def.variants.contains(&variant.to_string()),
            "BiocompatibilityClass missing variant '{}', variants: {:?}",
            variant,
            enum_def.variants
        );
    }
}

// ─── (d) CorrosionResistant refines MaterialSpec with Enum param ──────────────

/// CorrosionResistant must refine MaterialSpec and have one required member
/// `corrosion_class` typed as `Type::Enum("CorrosionClass")`.
#[test]
fn corrosion_resistant_refines_material_spec_with_enum_param() {
    let module = load_stdlib_module();

    let cr = module
        .trait_defs
        .iter()
        .find(|t| t.name == "CorrosionResistant")
        .expect("expected 'CorrosionResistant' trait in std/materials/chemical");

    assert!(
        cr.refinements.contains(&"MaterialSpec".to_string()),
        "CorrosionResistant must refine MaterialSpec, got refinements: {:?}",
        cr.refinements
    );

    assert_eq!(
        cr.required_members.len(),
        1,
        "CorrosionResistant should have exactly 1 required member, got: {:?}",
        cr.required_members
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );

    let corrosion_class = cr
        .required_members
        .iter()
        .find(|r| r.name == "corrosion_class")
        .expect("CorrosionResistant should have 'corrosion_class' member");

    match &corrosion_class.kind {
        RequirementKind::Param(ty) => assert_eq!(
            *ty,
            Type::Enum("CorrosionClass".to_string()),
            "corrosion_class should be Enum(CorrosionClass), got {:?}",
            ty
        ),
        other => panic!(
            "corrosion_class should be Param, got {:?}",
            other
        ),
    }
}

// ─── (e) Biocompatible refines MaterialSpec with Enum param ───────────────────

/// Biocompatible must refine MaterialSpec and have one required member
/// `biocompatibility_class` typed as `Type::Enum("BiocompatibilityClass")`.
#[test]
fn biocompatible_refines_material_spec_with_enum_param() {
    let module = load_stdlib_module();

    let bio = module
        .trait_defs
        .iter()
        .find(|t| t.name == "Biocompatible")
        .expect("expected 'Biocompatible' trait in std/materials/chemical");

    assert!(
        bio.refinements.contains(&"MaterialSpec".to_string()),
        "Biocompatible must refine MaterialSpec, got refinements: {:?}",
        bio.refinements
    );

    assert_eq!(
        bio.required_members.len(),
        1,
        "Biocompatible should have exactly 1 required member, got: {:?}",
        bio.required_members
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );

    let bio_class = bio
        .required_members
        .iter()
        .find(|r| r.name == "biocompatibility_class")
        .expect("Biocompatible should have 'biocompatibility_class' member");

    match &bio_class.kind {
        RequirementKind::Param(ty) => assert_eq!(
            *ty,
            Type::Enum("BiocompatibilityClass".to_string()),
            "biocompatibility_class should be Enum(BiocompatibilityClass), got {:?}",
            ty
        ),
        other => panic!(
            "biocompatibility_class should be Param, got {:?}",
            other
        ),
    }
}

// ─── (f) TitaniumImplant : Biocompatible + CorrosionResistant conformance ─────

/// A structure conforming to Biocompatible + CorrosionResistant must compile
/// cleanly via the full stdlib pipeline, carry both traits as bounds, and have
/// value cells for both enum-typed params with correct Enum cell_type.
#[test]
fn titanium_implant_conforms_to_biocompatible_and_corrosion_resistant() {
    // Note: the enums are declared inline here because the parser populates
    // `known_enums` only from the current source file (not from the stdlib
    // prelude), so `CorrosionClass.C5` and `BiocompatibilityClass.USP_Class_VI`
    // are only recognised as EnumAccess nodes if the enum names are present in
    // the same source string.  Redeclaring them here (identical to the stdlib
    // definitions) is safe: no duplicate-enum diagnostic is emitted, and the
    // compiler's resolution_enums has the prelude entry first so stdlib types win.
    let source = r#"
enum CorrosionClass { C1, C2, C3, C4, C5 }
enum BiocompatibilityClass { USP_Class_I, USP_Class_VI, ISO_10993 }

structure def TitaniumImplant : Biocompatible + CorrosionResistant {
    param density : Real = 4500.0
    param name : String = "titanium"
    param biocompatibility_class : BiocompatibilityClass = BiocompatibilityClass.USP_Class_VI
    param corrosion_class : CorrosionClass = CorrosionClass.C5
}
"#;

    let compiled = compile_source_with_stdlib(source);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "TitaniumImplant should compile cleanly, got errors: {:?}",
        errors
    );

    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "TitaniumImplant")
        .expect("expected 'TitaniumImplant' template in compiled module");

    assert!(
        template.trait_bounds.contains(&"Biocompatible".to_string()),
        "TitaniumImplant must have 'Biocompatible' trait bound, got: {:?}",
        template.trait_bounds
    );
    assert!(
        template
            .trait_bounds
            .contains(&"CorrosionResistant".to_string()),
        "TitaniumImplant must have 'CorrosionResistant' trait bound, got: {:?}",
        template.trait_bounds
    );

    // Verify enum-typed value cells.
    let bio_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "biocompatibility_class")
        .expect("expected 'biocompatibility_class' value cell in TitaniumImplant");
    assert_eq!(
        bio_cell.cell_type,
        Type::Enum("BiocompatibilityClass".to_string()),
        "biocompatibility_class should have Enum(BiocompatibilityClass) cell_type, got {:?}",
        bio_cell.cell_type
    );

    let corrosion_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "corrosion_class")
        .expect("expected 'corrosion_class' value cell in TitaniumImplant");
    assert_eq!(
        corrosion_cell.cell_type,
        Type::Enum("CorrosionClass".to_string()),
        "corrosion_class should have Enum(CorrosionClass) cell_type, got {:?}",
        corrosion_cell.cell_type
    );
}
