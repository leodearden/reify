//! Tests for `crates/reify-compiler/stdlib/constitutive.ri` —
//! `std.constitutive` module: `Frame`, `ConstitutiveLaw` trait,
//! `OrthotropicMaterial`, `TransverseIsotropicMaterial`,
//! `AnisotropicMaterial`.
//!
//! Observable signal for PRD §task γ
//! (docs/prds/v0_5/anisotropic-heterogeneous-elastostatics.md): the trait
//! and three structures parse + compile through the production stdlib path,
//! and `Field<Point3<Length>, AnisotropicMaterial>` resolves in a downstream
//! `param` position.
//!
//! Mirrors the `fdm_stdlib_compile.rs` helper trio and discipline.

use reify_compiler::*;
use reify_test_support::compile_source_with_stdlib;
use reify_types::*;

// ─── helpers ──────────────────────────────────────────────────────────────────

fn load_stdlib_module() -> &'static CompiledModule {
    stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/constitutive")
        .unwrap_or_else(|| {
            panic!(
                "stdlib should contain std/constitutive module; available paths: {:?}",
                stdlib_loader::load_stdlib()
                    .iter()
                    .map(|m| m.path.to_string())
                    .collect::<Vec<_>>()
            )
        })
}

fn find_structure(name: &str) -> &'static TopologyTemplate {
    let module = load_stdlib_module();
    module
        .templates
        .iter()
        .find(|t| t.name == name && t.entity_kind == EntityKind::Structure)
        .unwrap_or_else(|| {
            panic!(
                "expected `structure def {}` in std/constitutive, got: {:?}",
                name,
                module
                    .templates
                    .iter()
                    .map(|t| (&t.name, &t.entity_kind))
                    .collect::<Vec<_>>()
            )
        })
}

fn param_cells(template: &TopologyTemplate) -> Vec<&ValueCellDecl> {
    template
        .value_cells
        .iter()
        .filter(|vc| matches!(vc.kind, ValueCellKind::Param))
        .collect()
}

// ─── module loads cleanly ────────────────────────────────────────────────────

#[test]
fn std_constitutive_module_loads_with_no_errors() {
    let module = load_stdlib_module();
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected error diagnostics in constitutive.ri: {:?}",
        errors
    );
}

// ─── ConstitutiveLaw trait is an empty marker ────────────────────────────────

#[test]
fn constitutive_law_trait_is_empty_marker() {
    let module = load_stdlib_module();
    let trait_def = module
        .trait_defs
        .iter()
        .find(|t| t.name == "ConstitutiveLaw")
        .expect("expected ConstitutiveLaw trait in std/constitutive");
    assert!(
        trait_def.required_members.is_empty() && trait_def.defaults.is_empty(),
        "ConstitutiveLaw trait should be an empty marker (body intentionally \
         empty; producer-side dispatch lives in reify-solver-elastic), \
         got requirements: {:?}, defaults: {:?}",
        trait_def.required_members.iter().map(|r| &r.name).collect::<Vec<_>>(),
        trait_def.defaults.iter().map(|d| &d.name).collect::<Vec<_>>(),
    );
}

// ─── OrthotropicMaterial shape ───────────────────────────────────────────────

#[test]
fn orthotropic_material_has_nine_elastic_constants_plus_density_plus_provenance() {
    let template = find_structure("OrthotropicMaterial");
    let trait_bound_names: Vec<&str> =
        template.trait_bounds.iter().map(|s| s.as_str()).collect();
    assert_eq!(
        trait_bound_names,
        vec!["ConstitutiveLaw"],
        "OrthotropicMaterial should conform to ConstitutiveLaw"
    );

    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    let pressure_ty = Type::Scalar {
        dimension: DimensionVector::PRESSURE,
    };
    let density_ty = Type::Scalar {
        dimension: DimensionVector::MASS_DENSITY,
    };
    let provenance_ty = Type::StructureRef("MaterialPropertyProvenance".to_string());

    let expected: &[(&str, Type)] = &[
        ("e1", pressure_ty.clone()),
        ("e2", pressure_ty.clone()),
        ("e3", pressure_ty.clone()),
        ("g12", pressure_ty.clone()),
        ("g13", pressure_ty.clone()),
        ("g23", pressure_ty.clone()),
        ("nu12", Type::Real),
        ("nu13", Type::Real),
        ("nu23", Type::Real),
        ("density", density_ty),
        ("e1_provenance", provenance_ty.clone()),
        ("e2_provenance", provenance_ty.clone()),
        ("e3_provenance", provenance_ty.clone()),
        ("g12_provenance", provenance_ty.clone()),
        ("g13_provenance", provenance_ty.clone()),
        ("g23_provenance", provenance_ty.clone()),
        ("nu12_provenance", provenance_ty.clone()),
        ("nu13_provenance", provenance_ty.clone()),
        ("nu23_provenance", provenance_ty.clone()),
        ("density_provenance", provenance_ty),
    ];

    let expected_names: Vec<&str> = expected.iter().map(|(m, _)| *m).collect();
    assert_eq!(
        names, expected_names,
        "OrthotropicMaterial params must be in canonical order \
         (10 physical then 10 provenance), got: {:?}",
        names
    );

    for (member, expected_ty) in expected {
        let cell = params
            .iter()
            .find(|vc| vc.id.member == *member)
            .unwrap_or_else(|| panic!("missing param '{}'", member));
        assert_eq!(
            cell.cell_type, *expected_ty,
            "OrthotropicMaterial.{} should be {:?}, got {:?}",
            member, expected_ty, cell.cell_type
        );
    }
}

// ─── TransverseIsotropicMaterial shape ───────────────────────────────────────

#[test]
fn transverse_isotropic_material_has_five_elastic_constants_plus_density_plus_provenance() {
    let template = find_structure("TransverseIsotropicMaterial");
    let trait_bound_names: Vec<&str> =
        template.trait_bounds.iter().map(|s| s.as_str()).collect();
    assert_eq!(
        trait_bound_names,
        vec!["ConstitutiveLaw"],
        "TransverseIsotropicMaterial should conform to ConstitutiveLaw"
    );

    let names: Vec<&str> = param_cells(template)
        .iter()
        .map(|vc| vc.id.member.as_str())
        .collect();

    assert_eq!(
        names,
        vec![
            "e_in_plane",
            "e_axial",
            "nu_in_plane",
            "nu_axial",
            "g_axial",
            "density",
            "e_in_plane_provenance",
            "e_axial_provenance",
            "nu_in_plane_provenance",
            "nu_axial_provenance",
            "g_axial_provenance",
            "density_provenance",
        ],
        "TransverseIsotropicMaterial params must be in canonical order \
         (6 physical then 6 provenance)"
    );
}

// ─── AnisotropicMaterial = {law, frame} ──────────────────────────────────────

#[test]
fn anisotropic_material_has_law_and_frame() {
    let template = find_structure("AnisotropicMaterial");
    assert!(
        template.trait_bounds.is_empty(),
        "AnisotropicMaterial is a concrete value type, not trait-bound; got: {:?}",
        template.trait_bounds
    );

    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();
    assert_eq!(
        names,
        vec!["law", "frame"],
        "AnisotropicMaterial should have exactly (law, frame) in that order"
    );

    let law_cell = params.iter().find(|p| p.id.member == "law").unwrap();
    assert_eq!(
        law_cell.cell_type,
        Type::TraitObject("ConstitutiveLaw".to_string()),
        "AnisotropicMaterial.law must be the ConstitutiveLaw trait object"
    );

    let frame_cell = params.iter().find(|p| p.id.member == "frame").unwrap();
    assert_eq!(
        frame_cell.cell_type,
        Type::StructureRef("Frame".to_string()),
        "AnisotropicMaterial.frame must be a Frame structure ref"
    );
}

// ─── Field<Point3<Length>, AnisotropicMaterial> resolves in param position ───
//
// PRD task γ's user-observable signal: the field codomain type
// `Field<Point3<Length>, AnisotropicMaterial>` must resolve when used in a
// `param` position by downstream code (task δ's generalised
// `solve_elastic_static.material` argument).

#[test]
fn field_of_anisotropic_material_resolves_in_param_position() {
    let source = r#"
structure def TestHolder {
    param material : Field<Point3<Length>, AnisotropicMaterial>
}
"#;
    let result = compile_source_with_stdlib(source);
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "Field<Point3<Length>, AnisotropicMaterial> should resolve in param \
         position (PRD task γ user-observable signal), got errors: {:?}",
        errors
    );
}

// ─── OrthotropicMaterial(...) evaluates to non-Undef StructureInstance ──────
//
// User-observable signal #1: constructing an OrthotropicMaterial yields a
// concrete StructureInstance (SIR-α task 3540 + SIR-β-mat 3542 wiring). We
// can't directly evaluate here, but we can confirm the structure is
// constructible — i.e. a use-site that constructs it compiles cleanly.

#[test]
fn orthotropic_material_construction_compiles() {
    let source = r#"
structure def OrthoConstructionProbe {
    let provenance = MaterialPropertyProvenance(source: "test", reference: "test", notes: "test")
    let mat = OrthotropicMaterial(
        e1: 200GPa, e2: 100GPa, e3: 100GPa,
        g12: 50GPa, g13: 50GPa, g23: 40GPa,
        nu12: 0.3, nu13: 0.3, nu23: 0.3,
        density: 7850.0 * 1kg / (1m * 1m * 1m),
        e1_provenance: provenance,
        e2_provenance: provenance,
        e3_provenance: provenance,
        g12_provenance: provenance,
        g13_provenance: provenance,
        g23_provenance: provenance,
        nu12_provenance: provenance,
        nu13_provenance: provenance,
        nu23_provenance: provenance,
        density_provenance: provenance,
    )
}
"#;
    let result = compile_source_with_stdlib(source);
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "OrthotropicMaterial(...) construction should compile (PRD task γ \
         user-observable signal #1), got errors: {:?}",
        errors
    );
}
