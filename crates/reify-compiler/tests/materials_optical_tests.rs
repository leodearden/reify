//! Tests for stdlib/materials_optical.ri — §6.5 optical material traits.
//!
//! Tests validate that the .ri file is loaded by the production stdlib path,
//! that `OpticallyCharacterized` is correctly represented in the compiled
//! module, and that trait conformance works as expected.
//!
//! All tests use the production-path `load_stdlib_module()` helper that
//! exercises the same embedded + sequential-prelude compilation path as
//! production (not a standalone `.ri` file re-read).

use reify_compiler::*;
use reify_test_support::compile_source_with_stdlib;
use reify_core::*;

// ─── helpers ──────────────────────────────────────────────────────────────────

/// Return the `std/materials/optical` CompiledModule from the production
/// stdlib loader. Exercises the exact same code path as production: embedded
/// source, sequential compilation with growing prelude, OnceLock caching.
///
/// Panics if the module is not found — the expected failure mode until step-6
/// lands the .ri file and loader registration.
fn load_stdlib_module() -> &'static CompiledModule {
    stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/materials/optical")
        .expect("stdlib should contain std/materials/optical module")
}

// ─── (a) module loads cleanly with one trait def ─────────────────────────────

/// The std/materials/optical module must load with zero error-severity
/// diagnostics and contain exactly one trait definition: OpticallyCharacterized.
#[test]
fn optical_module_loads_with_no_errors_and_one_trait() {
    let module = load_stdlib_module();

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected error diagnostics in materials_optical.ri: {:?}",
        errors
    );

    assert_eq!(
        module.trait_defs.len(),
        1,
        "expected exactly 1 trait def in std/materials/optical, got: {:?}",
        module
            .trait_defs
            .iter()
            .map(|t| &t.name)
            .collect::<Vec<_>>()
    );
}

// ─── (b) OpticallyCharacterized: 1 required + 3 optional members ─────────────

/// OpticallyCharacterized must refine MaterialSpec and declare exactly one
/// required member (refractive_index) plus three optional params with
/// `= undef` defaults (absorption_coefficient, transmittance,
/// reference_thickness), which become optional in task #4241 γ.
///
/// Required:
///   refractive_index        → Type::Real (genuine-dimensionless)
///
/// Optional (DefaultKind::Param in oc.defaults):
///   absorption_coefficient  → Type::Scalar { dimension: ABSORPTION_COEFF }
///                             (= undef; type tightened by task #3115)
///   transmittance           → Type::Real (= undef; genuine-dimensionless)
///   reference_thickness     → Type::Scalar { dimension: LENGTH }
///                             (= undef; tightened by task #3113)
#[test]
fn optically_characterized_has_one_required_and_three_optional_members() {
    let module = load_stdlib_module();

    let oc = module
        .trait_defs
        .iter()
        .find(|t| t.name == "OpticallyCharacterized")
        .expect("expected 'OpticallyCharacterized' trait in std/materials/optical");

    assert!(
        oc.refinements.contains(&"MaterialSpec".to_string()),
        "OpticallyCharacterized must refine MaterialSpec, got refinements: {:?}",
        oc.refinements
    );

    // Exactly one required member: refractive_index.
    assert_eq!(
        oc.required_members.len(),
        1,
        "OpticallyCharacterized should have exactly 1 required member, got: {:?}",
        oc.required_members
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );

    let req = oc
        .required_members
        .iter()
        .find(|r| r.name == "refractive_index")
        .expect("OpticallyCharacterized missing required member 'refractive_index'");
    match &req.kind {
        RequirementKind::Param(ty) => assert_eq!(
            ty,
            &Type::Real,
            "refractive_index expected Type::Real, got {:?}",
            ty
        ),
        other => panic!(
            "refractive_index should be RequirementKind::Param, got {:?}",
            other
        ),
    }

    // Three optional params must appear in oc.defaults as DefaultKind::Param with the
    // correct cell_type — guarding against silent regressions that revert or mis-type them.
    let expected_optional: [(&str, Type); 3] = [
        (
            "absorption_coefficient",
            Type::Scalar {
                dimension: DimensionVector::ABSORPTION_COEFF,
            },
        ),
        ("transmittance", Type::Real),
        (
            "reference_thickness",
            Type::Scalar {
                dimension: DimensionVector::LENGTH,
            },
        ),
    ];
    for (param_name, expected_ty) in &expected_optional {
        let default = oc
            .defaults
            .iter()
            .find(|d| d.name.as_deref() == Some(param_name))
            .unwrap_or_else(|| {
                panic!(
                    "OpticallyCharacterized missing optional default for '{}', defaults: {:?}",
                    param_name,
                    oc.defaults
                        .iter()
                        .map(|d| &d.name)
                        .collect::<Vec<_>>()
                )
            });
        match &default.kind {
            DefaultKind::Param { cell_type, .. } => assert_eq!(
                cell_type, expected_ty,
                "OpticallyCharacterized optional param '{}' expected cell_type {:?}, got {:?}",
                param_name, expected_ty, cell_type
            ),
            other => panic!(
                "OpticallyCharacterized optional param '{}' should be DefaultKind::Param, got {:?}",
                param_name, other
            ),
        }
    }
}

// ─── (c) BorosilicateGlass : OpticallyCharacterized conformance test ──────────

/// A structure conforming to OpticallyCharacterized must compile cleanly via
/// the full stdlib pipeline, carry OpticallyCharacterized as a trait bound,
/// and have value cells for all four optical params plus the inherited
/// MaterialSpec members (density, name).
#[test]
fn borosilicate_glass_conforms_to_optically_characterized() {
    let source = r#"
structure def BorosilicateGlass : OpticallyCharacterized {
    param density : Density = 2230kg/m^3
    param name : String = "borosilicate_glass"
    param refractive_index : Real = 1.52
    param absorption_coefficient : AbsorptionCoeff = 0.001 / 1m
    param transmittance : Real = 0.92
    param reference_thickness : Length = 1mm
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
        "BorosilicateGlass : OpticallyCharacterized should compile cleanly, got errors: {:?}",
        errors
    );

    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "BorosilicateGlass")
        .expect("expected 'BorosilicateGlass' template in compiled module");

    assert!(
        template
            .trait_bounds
            .contains(&"OpticallyCharacterized".to_string()),
        "BorosilicateGlass must have 'OpticallyCharacterized' trait bound, got: {:?}",
        template.trait_bounds
    );

    // Verify all expected value cells are present (optical params + inherited).
    let expected_cells = [
        "density",
        "name",
        "refractive_index",
        "absorption_coefficient",
        "transmittance",
        "reference_thickness",
    ];
    for cell_name in &expected_cells {
        assert!(
            template
                .value_cells
                .iter()
                .any(|vc| vc.id.member == *cell_name),
            "BorosilicateGlass template missing value cell '{}', cells: {:?}",
            cell_name,
            template
                .value_cells
                .iter()
                .map(|vc| &vc.id.member)
                .collect::<Vec<_>>()
        );
    }
}
