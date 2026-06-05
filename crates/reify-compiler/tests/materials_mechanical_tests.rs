//! Tests for stdlib/materials_mechanical.ri — mechanical material traits.
//!
//! Tests validate that the .ri file parses and compiles cleanly, that each
//! trait and enum is correctly represented in the compiled module, and that
//! trait conformance and constraint injection work as expected.

use reify_compiler::*;
use reify_test_support::{compile_first_template, compile_source_with_stdlib};
use reify_core::*;
use std::path::PathBuf;

// ─── helpers ──────────────────────────────────────────────────────────────────

/// Load and compile stdlib/materials_mechanical.ri from the crate root.
/// Panics on parse errors or if the file doesn't exist.
fn load_stdlib_module() -> CompiledModule {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "stdlib",
        "materials_mechanical.ri",
    ]
    .iter()
    .collect();
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e));
    let parsed = reify_syntax::parse(&source, ModulePath::single("stdlib"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors in materials_mechanical.ri: {:?}",
        parsed.errors
    );
    reify_compiler::compile(&parsed)
}

// ─── step-1: file exists, parses, compiles without errors ────────────────────

/// Step 1: materials_mechanical.ri file exists, parses cleanly, compiles
/// without error-severity diagnostics, and has at least one trait def.
#[test]
fn stdlib_file_parses_and_compiles_without_errors() {
    let module = load_stdlib_module();

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected error diagnostics in materials_mechanical.ri: {:?}",
        errors
    );

    assert!(
        !module.trait_defs.is_empty(),
        "expected at least one trait def, got zero"
    );
}

// ─── step-3: Elastic trait ───────────────────────────────────────────────────

/// Step 3: Elastic trait has 2 required members: youngs_modulus (Pressure)
/// and poissons_ratio (Real, dimensionless). (task α #4239: shear_modulus is
/// now `= undef` optional, so it lives in `defaults`, not `required_members`.)
/// (task #3111: youngs_modulus tightened from Real to Pressure.)
#[test]
fn elastic_trait_required_members_have_correct_types() {
    let module = load_stdlib_module();

    let elastic = module
        .trait_defs
        .iter()
        .find(|t| t.name == "Elastic")
        .expect("expected 'Elastic' trait in compiled module");

    assert_eq!(
        elastic.required_members.len(),
        2,
        "Elastic should have exactly 2 required members, got: {:?}",
        elastic
            .required_members
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );

    let member_names: Vec<&str> = elastic
        .required_members
        .iter()
        .map(|r| r.name.as_str())
        .collect();
    assert!(
        member_names.contains(&"youngs_modulus"),
        "expected 'youngs_modulus' in Elastic, got: {:?}",
        member_names
    );
    assert!(
        member_names.contains(&"poissons_ratio"),
        "expected 'poissons_ratio' in Elastic, got: {:?}",
        member_names
    );
    // shear_modulus is now optional (`= undef`, task α #4239) → it lives in
    // `defaults`, not `required_members`.
    assert!(
        !member_names.contains(&"shear_modulus"),
        "shear_modulus is now optional and should NOT be a required member, got: {:?}",
        member_names
    );

    // youngs_modulus must be Pressure (dimensioned), not Real — task #3111.
    let youngs = elastic
        .required_members
        .iter()
        .find(|r| r.name == "youngs_modulus")
        .expect("expected 'youngs_modulus' required member");
    match &youngs.kind {
        RequirementKind::Param(ty) => assert_eq!(
            *ty,
            Type::Scalar {
                dimension: DimensionVector::PRESSURE,
            },
            "youngs_modulus should be Scalar{{PRESSURE}}, got {:?}",
            ty
        ),
        other => panic!("youngs_modulus should be Param, got {:?}", other),
    }

    // poissons_ratio must stay Real (genuinely dimensionless).
    let poissons = elastic
        .required_members
        .iter()
        .find(|r| r.name == "poissons_ratio")
        .expect("expected 'poissons_ratio' required member");
    match &poissons.kind {
        RequirementKind::Param(ty) => assert_eq!(
            *ty,
            Type::Real,
            "poissons_ratio should remain Real (dimensionless), got {:?}",
            ty
        ),
        other => panic!("poissons_ratio should be Param, got {:?}", other),
    }
}

// ─── step-5: Strong trait ────────────────────────────────────────────────────

/// Step 5: Strong trait has 2 required members and at least 1 constraint
/// default (the `ultimate_tensile_strength >= yield_strength` constraint). (task α #4239:
/// compressive_strength is now `= undef` optional → in `defaults`.)
#[test]
fn strong_trait_has_members_and_constraint_default() {
    let module = load_stdlib_module();

    let strong = module
        .trait_defs
        .iter()
        .find(|t| t.name == "Strong")
        .expect("expected 'Strong' trait in compiled module");

    assert_eq!(
        strong.required_members.len(),
        2,
        "Strong should have exactly 2 required members, got: {:?}",
        strong
            .required_members
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );

    let member_names: Vec<&str> = strong
        .required_members
        .iter()
        .map(|r| r.name.as_str())
        .collect();
    assert!(
        member_names.contains(&"yield_strength"),
        "expected 'yield_strength' in Strong"
    );
    assert!(
        member_names.contains(&"ultimate_tensile_strength"),
        "expected 'ultimate_tensile_strength' in Strong (β #4240 renamed from old name)"
    );
    // compressive_strength is now optional (`= undef`, task α #4239) → it lives
    // in `defaults`, not `required_members`.
    assert!(
        !member_names.contains(&"compressive_strength"),
        "compressive_strength is now optional and should NOT be a required member"
    );

    let constraint_defaults: Vec<_> = strong
        .defaults
        .iter()
        .filter(|d| matches!(d.kind, DefaultKind::Constraint(_)))
        .collect();
    assert!(
        !constraint_defaults.is_empty(),
        "Strong trait should have at least 1 constraint default (ultimate_tensile_strength >= yield_strength)"
    );

    // Type assertions: yield_strength and ultimate_tensile_strength should be
    // Pressure (dimensioned) after task #3111 tightening.
    for name in &["yield_strength", "ultimate_tensile_strength"] {
        let req = strong
            .required_members
            .iter()
            .find(|r| r.name == *name)
            .unwrap_or_else(|| panic!("Strong missing required member '{}'", name));
        match &req.kind {
            RequirementKind::Param(ty) => assert_eq!(
                *ty,
                Type::Scalar {
                    dimension: DimensionVector::PRESSURE,
                },
                "Strong member '{}' should be Scalar{{PRESSURE}}, got {:?}",
                name,
                ty
            ),
            other => panic!(
                "Strong member '{}' should be Param, got {:?}",
                name, other
            ),
        }
    }
}

// ─── MaterialSpec.density type pin ───────────────────────────────────────────

/// MaterialSpec.density must be typed as Density (DimensionVector::MASS_DENSITY),
/// not plain Real, after task #3111 tightening.
#[test]
fn material_spec_density_member_is_density_type() {
    let module = load_stdlib_module();

    let spec = module
        .trait_defs
        .iter()
        .find(|t| t.name == "MaterialSpec")
        .expect("expected 'MaterialSpec' trait in compiled module");

    let density_req = spec
        .required_members
        .iter()
        .find(|r| r.name == "density")
        .expect("MaterialSpec should have 'density' as a required member");

    match &density_req.kind {
        RequirementKind::Param(ty) => assert_eq!(
            *ty,
            Type::Scalar {
                dimension: DimensionVector::MASS_DENSITY,
            },
            "MaterialSpec.density should be Scalar{{MASS_DENSITY}} (Density type), got {:?}",
            ty
        ),
        other => panic!(
            "MaterialSpec.density should be a Param requirement, got {:?}",
            other
        ),
    }
}

// ─── step-7: HardnessScale enum and Hard trait ───────────────────────────────

/// Step 7: HardnessScale enum has exactly 7 variants (Rockwell_A, Rockwell_B,
/// Rockwell_C, Brinell, Vickers, Shore_A, Shore_D) and Hard trait has 2
/// required members (hardness_value : Real, hardness_scale : Enum(HardnessScale)).
#[test]
fn hardness_scale_enum_and_hard_trait() {
    let module = load_stdlib_module();

    // Enum check
    let enum_def = module
        .enum_defs
        .iter()
        .find(|e| e.name == "HardnessScale")
        .expect("expected 'HardnessScale' enum in compiled module");

    let expected_variants = [
        "Rockwell_A",
        "Rockwell_B",
        "Rockwell_C",
        "Brinell",
        "Vickers",
        "Shore_A",
        "Shore_D",
    ];
    assert_eq!(
        enum_def.variants.len(),
        expected_variants.len(),
        "HardnessScale should have {} variants, got: {:?}",
        expected_variants.len(),
        enum_def.variants
    );
    for variant in &expected_variants {
        assert!(
            enum_def.variants.contains(&variant.to_string()),
            "HardnessScale missing variant '{}', variants: {:?}",
            variant,
            enum_def.variants
        );
    }

    // Hard trait check
    let hard = module
        .trait_defs
        .iter()
        .find(|t| t.name == "Hard")
        .expect("expected 'Hard' trait in compiled module");

    assert_eq!(
        hard.required_members.len(),
        2,
        "Hard should have exactly 2 required members, got: {:?}",
        hard.required_members
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );

    let hardness_value = hard
        .required_members
        .iter()
        .find(|r| r.name == "hardness_value")
        .expect("Hard trait should have 'hardness_value' member");
    match &hardness_value.kind {
        RequirementKind::Param(ty) => assert_eq!(
            *ty,
            Type::Real,
            "hardness_value should be Real, got {:?}",
            ty
        ),
        other => panic!("hardness_value should be Param, got {:?}", other),
    }

    let hardness_scale = hard
        .required_members
        .iter()
        .find(|r| r.name == "hardness_scale")
        .expect("Hard trait should have 'hardness_scale' member");
    match &hardness_scale.kind {
        RequirementKind::Param(ty) => assert_eq!(
            *ty,
            Type::Enum("HardnessScale".to_string()),
            "hardness_scale should be Enum(HardnessScale), got {:?}",
            ty
        ),
        other => panic!("hardness_scale should be Param, got {:?}", other),
    }
}

// ─── step-9: Steel conformance with Elastic + Strong ─────────────────────────

/// Step 9: `structure def Steel : Elastic + Strong { ... }` type-checks
/// without errors. The structure provides all 6 required params. The compiled
/// template has trait_bounds containing both 'Elastic' and 'Strong'.
#[test]
fn steel_conforms_to_elastic_and_strong() {
    let source = r#"
trait Elastic {
    param youngs_modulus : Real
    param poissons_ratio : Real
    param shear_modulus : Real
}

trait Strong {
    param yield_strength : Real
    param ultimate_tensile_strength : Real
    param compressive_strength : Real
    constraint ultimate_tensile_strength >= yield_strength
}

structure def Steel : Elastic + Strong {
    param youngs_modulus : Real = 200.0
    param poissons_ratio : Real = 0.3
    param shear_modulus : Real = 77.0
    param yield_strength : Real = 250.0
    param ultimate_tensile_strength : Real = 400.0
    param compressive_strength : Real = 250.0
}
"#;
    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "Steel conformance should have no errors, got: {:?}",
        errors
    );

    assert!(
        template.trait_bounds.contains(&"Elastic".to_string()),
        "Steel should have 'Elastic' trait bound, got: {:?}",
        template.trait_bounds
    );
    assert!(
        template.trait_bounds.contains(&"Strong".to_string()),
        "Steel should have 'Strong' trait bound, got: {:?}",
        template.trait_bounds
    );
}

// ─── step-11: constraint injection into Steel ─────────────────────────────────

/// Step 11: The constraint from Strong (`ultimate_tensile_strength >= yield_strength`)
/// is injected into a conforming Steel structure — template.constraints is non-empty.
#[test]
fn strong_constraint_injected_into_steel() {
    let source = r#"
trait Strong {
    param yield_strength : Real
    param ultimate_tensile_strength : Real
    param compressive_strength : Real
    constraint ultimate_tensile_strength >= yield_strength
}

structure def Steel : Strong {
    param yield_strength : Real = 250.0
    param ultimate_tensile_strength : Real = 400.0
    param compressive_strength : Real = 250.0
}
"#;
    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    assert!(
        !template.constraints.is_empty(),
        "expected constraint from Strong trait injected into Steel, but constraints is empty"
    );
}

// ─── step-13: HardnessScale.Vickers access ────────────────────────────────────

/// Step 13: `let scale = HardnessScale.Vickers` compiles without errors in a
/// structure, and the resulting 'scale' value cell has Enum type.
#[test]
fn hardness_scale_vickers_access() {
    let source = r#"
enum HardnessScale { Rockwell_A, Rockwell_B, Rockwell_C, Brinell, Vickers, Shore_A, Shore_D }

structure def S {
    let scale = HardnessScale.Vickers
}
"#;
    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no errors for HardnessScale.Vickers access, got: {:?}",
        errors
    );

    let scale_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "scale")
        .expect("expected 'scale' value cell in template");

    assert_eq!(
        scale_cell.cell_type,
        Type::Enum("HardnessScale".to_string()),
        "scale should have Enum(HardnessScale) type, got {:?}",
        scale_cell.cell_type
    );
}

// ─── step-15: remaining 5 traits ─────────────────────────────────────────────

/// Step 15: FatigueRated, FractureTough, Ductile, ImpactResistant, Damping
/// all exist in the compiled module with correct required member counts.
#[test]
fn remaining_five_traits_exist() {
    let module = load_stdlib_module();

    // FatigueRated: 0 required members (all three new params are optional = undef).
    // task β #4240: drop single endurance_limit (required); add fatigue_limit,
    // fatigue_strength_at (Real = undef) and fatigue_cycles (Int = undef) — all optional.
    let fatigue = module
        .trait_defs
        .iter()
        .find(|t| t.name == "FatigueRated")
        .expect("expected 'FatigueRated' trait");
    assert_eq!(
        fatigue.required_members.len(),
        0,
        "FatigueRated should have 0 required members (all params optional = undef), got: {:?}",
        fatigue
            .required_members
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );
    assert!(
        !fatigue
            .required_members
            .iter()
            .any(|r| r.name == "endurance_limit"),
        "endurance_limit must no longer exist as a required member (dropped in β #4240)"
    );
    // Richer per-param defaults + cell_type assertions live in
    // materials_param_surface_tests::fatigue_rated_optional_params_in_defaults.

    // FractureTough: 1 member (fracture_toughness)
    let fracture = module
        .trait_defs
        .iter()
        .find(|t| t.name == "FractureTough")
        .expect("expected 'FractureTough' trait");
    assert_eq!(
        fracture.required_members.len(),
        1,
        "FractureTough should have 1 required member"
    );
    assert!(
        fracture
            .required_members
            .iter()
            .any(|r| r.name == "fracture_toughness"),
        "FractureTough should have 'fracture_toughness' member"
    );

    // Ductile: 1 required member (elongation_at_break). (task α #4239:
    // reduction_of_area is now `= undef` optional → in `defaults`.
    // task β #4240: elongation renamed → elongation_at_break.)
    let ductile = module
        .trait_defs
        .iter()
        .find(|t| t.name == "Ductile")
        .expect("expected 'Ductile' trait");
    assert_eq!(
        ductile.required_members.len(),
        1,
        "Ductile should have 1 required member"
    );
    assert!(
        ductile
            .required_members
            .iter()
            .any(|r| r.name == "elongation_at_break"),
        "Ductile should have 'elongation_at_break' member (β #4240 renamed from 'elongation')"
    );
    assert!(
        !ductile
            .required_members
            .iter()
            .any(|r| r.name == "reduction_of_area"),
        "reduction_of_area is now optional and should NOT be a required member"
    );

    // ImpactResistant: 0 required members (both new params are optional = undef).
    // task β #4240: drop single impact_energy (required); add charpy_impact and
    // izod_impact (Real = undef) — both optional.
    let impact = module
        .trait_defs
        .iter()
        .find(|t| t.name == "ImpactResistant")
        .expect("expected 'ImpactResistant' trait");
    assert_eq!(
        impact.required_members.len(),
        0,
        "ImpactResistant should have 0 required members (all params optional = undef), got: {:?}",
        impact
            .required_members
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );
    assert!(
        !impact
            .required_members
            .iter()
            .any(|r| r.name == "impact_energy"),
        "impact_energy must no longer exist as a required member (dropped in β #4240)"
    );
    // Richer per-param defaults + cell_type assertions live in
    // materials_param_surface_tests::impact_resistant_optional_params_in_defaults.

    // Damping: 2 members (damping_ratio, loss_factor)
    let damping = module
        .trait_defs
        .iter()
        .find(|t| t.name == "Damping")
        .expect("expected 'Damping' trait");
    assert_eq!(
        damping.required_members.len(),
        2,
        "Damping should have 2 required members"
    );
    assert!(
        damping
            .required_members
            .iter()
            .any(|r| r.name == "damping_ratio"),
        "Damping should have 'damping_ratio' member"
    );
    assert!(
        damping
            .required_members
            .iter()
            .any(|r| r.name == "loss_factor"),
        "Damping should have 'loss_factor' member"
    );
}

// ─── FractureTough fracture_toughness member type (task #3115) ──────────────

/// Task #3115: `FractureTough.fracture_toughness` is the canonical
/// fracture-toughness alias `FractureToughness` (Pa·√m), the fractional-Length
/// composite. The audit doc flagged this as blocked-composite pending the
/// composite-dim aliases delivered in this task; pin the dimension here so a
/// future loosening would fail loudly.
#[test]
fn fracture_tough_member_type_is_fracture_toughness_dimension() {
    let module = load_stdlib_module();

    let fracture = module
        .trait_defs
        .iter()
        .find(|t| t.name == "FractureTough")
        .expect("expected 'FractureTough' trait in compiled module");

    let req = fracture
        .required_members
        .iter()
        .find(|r| r.name == "fracture_toughness")
        .expect("FractureTough should have 'fracture_toughness' member");

    match &req.kind {
        RequirementKind::Param(ty) => assert_eq!(
            *ty,
            Type::Scalar {
                dimension: DimensionVector::FRACTURE_TOUGHNESS,
            },
            "fracture_toughness should be Scalar{{FRACTURE_TOUGHNESS}}, got {:?}",
            ty
        ),
        other => panic!("fracture_toughness should be Param, got {:?}", other),
    }
}

// ─── four mechanical traits refine MaterialSpec ───────────────────────────────

/// FatigueRated, FractureTough, ImpactResistant, Damping must each declare
/// `MaterialSpec` as a parent trait (refinements == ["MaterialSpec"]).
/// This verifies that the four §6.2 mechanical-material traits are properly
/// anchored to the base material contract, not free-standing.
#[test]
fn four_mechanical_traits_refine_material_spec() {
    let module = load_stdlib_module();

    for trait_name in &[
        "FatigueRated",
        "FractureTough",
        "ImpactResistant",
        "Damping",
    ] {
        let trait_def = module
            .trait_defs
            .iter()
            .find(|t| t.name == *trait_name)
            .unwrap_or_else(|| panic!("expected '{}' trait in compiled module", trait_name));

        // `contains` rather than `assert_eq!`: expresses the invariant that the trait
        // refines MaterialSpec without over-constraining the refinements list, tolerating
        // legitimate future additions of additional parent traits.  If exact-membership
        // becomes important (e.g. to catch accidental extra refinements), revisit and
        // switch back to assert_eq!(trait_def.refinements, vec!["MaterialSpec"…]).
        assert!(
            trait_def.refinements.contains(&"MaterialSpec".to_string()),
            "'{}' should refine MaterialSpec but got refinements: {:?}",
            trait_name,
            trait_def.refinements
        );
    }
}

// ─── conformance: all four §6.2 refining traits enforce MaterialSpec at compile time ─

/// Each of the four §6.2 traits that refine MaterialSpec (`FatigueRated`,
/// `FractureTough`, `ImpactResistant`, `Damping`) must enforce the inherited
/// `density` and `name` contract at compile time.  A structure that declares
/// only the trait's own members but omits `density`/`name` must produce error
/// diagnostics that specifically mention those missing inherited members.
#[test]
fn four_refining_traits_without_material_members_is_conformance_error() {
    // (trait_name, trait-specific params to include in the structure — inherited
    // MaterialSpec params deliberately omitted to trigger the conformance error)
    let cases: &[(&str, &str)] = &[
        ("FatigueRated", "    param fatigue_limit : Real = 500.0"),
        (
            "FractureTough",
            "    param fracture_toughness : FractureToughness = 50.0 * 1Pa * sqrt(1m)",
        ),
        ("ImpactResistant", "    param charpy_impact : Real = 30.0"),
        (
            "Damping",
            "    param damping_ratio : Real = 0.05\n    param loss_factor : Real = 0.1",
        ),
    ];

    for (trait_name, own_members) in cases {
        let source = format!(
            "structure def TestFoo : {} {{\n{}\n}}\n",
            trait_name, own_members
        );
        let compiled = compile_source_with_stdlib(&source);

        let errors: Vec<_> = compiled
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            !errors.is_empty(),
            "'{}': expected conformance errors for structure missing inherited density/name, but got none",
            trait_name
        );
        // Assert on `density` specifically — it is always among the missing inherited
        // MaterialSpec members and provides a concrete, unambiguous signal.  Checking
        // bare "name" is avoided because that substring appears in many unrelated
        // diagnostics ("unknown name", "name resolution failed", etc.) and could cause
        // a false pass.  The diagnostic is also anchored on the typed
        // `DiagnosticCode::MissingRequiredMember` (not just the message substring)
        // so that an unrelated stdlib regression that merely *mentions* "density"
        // cannot produce a false-positive pass.
        assert!(
            errors.iter().any(|d| {
                d.code == Some(DiagnosticCode::MissingRequiredMember)
                    && d.message.contains("density")
            }),
            "'{}': expected MissingRequiredMember error mentioning 'density', got: {:?}",
            trait_name,
            errors
        );
    }
}

/// Each of the four §6.2 traits that refine MaterialSpec (`FatigueRated`,
/// `FractureTough`, `ImpactResistant`, `Damping`) must accept a structure that
/// supplies both the inherited MaterialSpec params (`density`, `name`) and the
/// trait's own required params.  No error diagnostics should be emitted and the
/// compiled template must carry the trait as a bound.
#[test]
fn four_refining_traits_with_all_material_members_conform_cleanly() {
    // (trait_name, trait-specific params to include alongside inherited density/name)
    let cases: &[(&str, &str)] = &[
        ("FatigueRated", "    param fatigue_limit : Real = 500.0"),
        (
            "FractureTough",
            "    param fracture_toughness : FractureToughness = 50.0 * 1Pa * sqrt(1m)",
        ),
        ("ImpactResistant", "    param charpy_impact : Real = 30.0"),
        (
            "Damping",
            "    param damping_ratio : Real = 0.05\n    param loss_factor : Real = 0.1",
        ),
    ];

    for (trait_name, own_members) in cases {
        let source = format!(
            "structure def Test{} : {} {{\n    param density : Real = 7850.0\n    param name : String = \"steel\"\n{}\n}}\n",
            trait_name, trait_name, own_members
        );
        let compiled = compile_source_with_stdlib(&source);

        let errors: Vec<_> = compiled
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "'{}': structure with all required members should compile cleanly, got: {:?}",
            trait_name,
            errors
        );

        let expected_name = format!("Test{}", trait_name);
        let template = compiled
            .templates
            .iter()
            .find(|t| t.name == expected_name)
            .unwrap_or_else(|| {
                panic!(
                    "'{}': expected compiled template named '{}', got templates: {:?}",
                    trait_name,
                    expected_name,
                    compiled
                        .templates
                        .iter()
                        .map(|t| &t.name)
                        .collect::<Vec<_>>()
                )
            });
        assert!(
            template.trait_bounds.contains(&trait_name.to_string()),
            "'{}': compiled template should have trait bound, got: {:?}",
            trait_name,
            template.trait_bounds
        );
    }
}

// ─── step-17: full integration ────────────────────────────────────────────────

/// Step 17: The complete .ri file compiles to exactly 10 traits and 1 enum,
/// with zero error-severity diagnostics. (task α #4239 added the
/// free-standing `TemperatureDependent` base trait, §6.1.)
#[test]
fn full_module_has_nine_traits_and_one_enum() {
    let module = load_stdlib_module();

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected zero error diagnostics in complete .ri file, got: {:?}",
        errors
    );

    assert_eq!(
        module.trait_defs.len(),
        10,
        "expected exactly 10 traits, got: {:?}",
        module
            .trait_defs
            .iter()
            .map(|t| &t.name)
            .collect::<Vec<_>>()
    );

    assert_eq!(
        module.enum_defs.len(),
        1,
        "expected exactly 1 enum, got: {:?}",
        module.enum_defs.iter().map(|e| &e.name).collect::<Vec<_>>()
    );

    // Verify all expected trait names are present
    let expected_traits = [
        "MaterialSpec",
        "TemperatureDependent",
        "Elastic",
        "Strong",
        "Hard",
        "FatigueRated",
        "FractureTough",
        "Ductile",
        "ImpactResistant",
        "Damping",
    ];
    for trait_name in &expected_traits {
        assert!(
            module.trait_defs.iter().any(|t| t.name == *trait_name),
            "expected trait '{}' in compiled module, but it's missing",
            trait_name
        );
    }

    assert!(
        module.enum_defs.iter().any(|e| e.name == "HardnessScale"),
        "expected 'HardnessScale' enum in compiled module"
    );
}
