//! Tests for stdlib/structural_physical.ri — structural/physical traits.
//!
//! Tests validate that the .ri file parses and compiles cleanly, that each
//! trait is correctly represented in the compiled module, and that trait
//! conformance and constraint injection work as expected.

use reify_compiler::*;
use reify_types::*;

// ─── helpers ──────────────────────────────────────────────────────────────────

/// Return the `std/structural/physical` CompiledModule from the production
/// stdlib loader. Exercises the exact same code path as production: embedded
/// source, sequential compilation with growing prelude, OnceLock caching.
fn load_stdlib_module() -> &'static CompiledModule {
    stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/structural/physical")
        .expect("stdlib should contain std/structural/physical module")
}

// ─── step-1: file exists, parses, compiles without errors ────────────────────

/// Step 1: structural_physical.ri file exists, parses cleanly, compiles
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
        "unexpected error diagnostics in structural_physical.ri: {:?}",
        errors
    );

    assert!(
        !module.trait_defs.is_empty(),
        "expected at least one trait def, got zero"
    );
}

// ─── step-3: all 8 trait names present ───────────────────────────────────────

/// Step 3: All 8 structural/physical trait names are present in the compiled
/// module: Physical, Rigid, Flexible, ElasticallyDeformable, Plastic,
/// ThermallyConductive, ElectricallyConductive, Sealed.
#[test]
fn all_eight_traits_present() {
    let module = load_stdlib_module();

    let trait_names: Vec<&str> = module.trait_defs.iter().map(|t| t.name.as_str()).collect();

    let expected = [
        "Physical",
        "Rigid",
        "Flexible",
        "ElasticallyDeformable",
        "Plastic",
        "ThermallyConductive",
        "ElectricallyConductive",
        "Sealed",
    ];

    assert_eq!(
        module.trait_defs.len(),
        expected.len(),
        "expected exactly {} traits, got: {:?}",
        expected.len(),
        trait_names
    );

    for name in &expected {
        assert!(
            trait_names.contains(name),
            "expected trait '{}' in compiled module, found: {:?}",
            name,
            trait_names
        );
    }
}

// ─── step-5: Physical trait details ──────────────────────────────────────────

/// Step 5: Physical trait has correct required_members (volume, centroid_x,
/// centroid_y, centroid_z as Real params), defaults include a Let named 'mass',
/// and refinements contains 'Material'.
#[test]
fn physical_trait_has_correct_members_and_refinements() {
    let module = load_stdlib_module();

    let physical = module
        .trait_defs
        .iter()
        .find(|t| t.name == "Physical")
        .expect("expected 'Physical' trait in compiled module");

    // Refinements should contain "Material"
    assert!(
        physical.refinements.contains(&"Material".to_string()),
        "Physical should refine Material, got refinements: {:?}",
        physical.refinements
    );

    // Required members: volume, centroid_x, centroid_y, centroid_z
    let member_names: Vec<&str> = physical
        .required_members
        .iter()
        .map(|r| r.name.as_str())
        .collect();

    for expected_member in &["volume", "centroid_x", "centroid_y", "centroid_z"] {
        assert!(
            member_names.contains(expected_member),
            "Physical should have '{}' required member, got: {:?}",
            expected_member,
            member_names
        );
    }

    // Physical's own params (volume, centroid_x/y/z) should be Real.
    // Only check these four by name — not ALL required_members — to avoid
    // false failures if the compiler ever flattens inherited members of
    // different types (e.g., name:String from Material) into required_members.
    for param_name in &["volume", "centroid_x", "centroid_y", "centroid_z"] {
        let req = physical
            .required_members
            .iter()
            .find(|r| r.name == *param_name)
            .unwrap_or_else(|| {
                panic!(
                    "Physical should have '{}' in required_members, got: {:?}",
                    param_name, member_names
                )
            });
        match &req.kind {
            RequirementKind::Param(ty) => {
                assert_eq!(
                    *ty,
                    Type::Real,
                    "Physical param '{}' should be Real, got {:?}",
                    param_name,
                    ty
                );
            }
            other => panic!(
                "Physical member '{}' should be RequirementKind::Param, got {:?}",
                param_name, other
            ),
        }
    }

    // Defaults should include a Let named 'mass'
    let let_defaults: Vec<_> = physical
        .defaults
        .iter()
        .filter(|d| matches!(d.kind, DefaultKind::Let(_)))
        .collect();
    assert!(
        let_defaults
            .iter()
            .any(|d| d.name.as_deref() == Some("mass")),
        "Physical trait should have a Let default named 'mass', got defaults: {:?}",
        physical
            .defaults
            .iter()
            .map(|d| &d.name)
            .collect::<Vec<_>>()
    );
}

// ─── step-23: targeted Physical own-params assertion (over-broad fix) ────────

/// Step 23: Targeted test for review issue [over_broad_assertion].
/// Checks ONLY the four Physical-specific params by name. Resilient to compiler
/// changes that might flatten inherited members of different types (e.g.,
/// name:String from Material) into required_members.
#[test]
fn physical_own_params_are_real() {
    let module = load_stdlib_module();
    let physical = module
        .trait_defs
        .iter()
        .find(|t| t.name == "Physical")
        .expect("expected 'Physical' trait in compiled module");

    let own_params = ["volume", "centroid_x", "centroid_y", "centroid_z"];
    for param_name in &own_params {
        let req = physical
            .required_members
            .iter()
            .find(|r| r.name == *param_name)
            .unwrap_or_else(|| {
                panic!(
                    "Physical should have '{}' in required_members, got: {:?}",
                    param_name,
                    physical
                        .required_members
                        .iter()
                        .map(|r| &r.name)
                        .collect::<Vec<_>>()
                )
            });
        match &req.kind {
            RequirementKind::Param(ty) => {
                assert_eq!(
                    *ty,
                    Type::Real,
                    "Physical param '{}' should be Real, got {:?}",
                    param_name,
                    ty
                );
            }
            other => panic!(
                "Physical member '{}' should be RequirementKind::Param, got {:?}",
                param_name, other
            ),
        }
    }
}

// ─── step-7: Bracket : Physical conformance (mass computed) ──────────────────

/// Step 7: structure def Bracket : Physical compiles with all required members
/// provided (density, name from Material refinement; volume, centroid_x/y/z
/// from Physical). Assert no errors, Bracket has 'Physical' in trait_bounds,
/// and a 'mass' value cell exists (injected let default).
/// This is the task's first explicit test case.
#[test]
fn bracket_conforms_to_physical_with_mass_computed() {
    let source = r#"
structure def Bracket : Physical {
    param density : Real = 7850.0
    param name : String = "steel bracket"
    param volume : Real = 0.001
    param centroid_x : Real = 0.0
    param centroid_y : Real = 0.0
    param centroid_z : Real = 0.0
}
"#;
    let prelude = stdlib_loader::load_stdlib();
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile_with_prelude(&parsed, prelude);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "Bracket : Physical should compile without errors, got: {:?}",
        errors
    );

    let template = compiled
        .templates
        .first()
        .expect("expected at least 1 template");

    assert!(
        template.trait_bounds.contains(&"Physical".to_string()),
        "Bracket should have 'Physical' trait bound, got: {:?}",
        template.trait_bounds
    );

    // The injected `let mass = volume * density` should create a 'mass' value cell
    let mass_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "mass");
    assert!(
        mass_cell.is_some(),
        "expected 'mass' value cell from Physical trait's let default, got cells: {:?}",
        template
            .value_cells
            .iter()
            .map(|vc| &vc.id.member)
            .collect::<Vec<_>>()
    );
}

// ─── step-9: Rigid trait refines Physical with moment_of_inertia ─────────────

/// Step 9: Rigid trait refines Physical (refinements contains 'Physical'),
/// has moment_of_inertia as a required member of type Real.
/// This is the task's second explicit test case.
#[test]
fn rigid_refines_physical_with_moment_of_inertia() {
    let module = load_stdlib_module();

    let rigid = module
        .trait_defs
        .iter()
        .find(|t| t.name == "Rigid")
        .expect("expected 'Rigid' trait in compiled module");

    // Refinements should contain "Physical"
    assert!(
        rigid.refinements.contains(&"Physical".to_string()),
        "Rigid should refine Physical, got refinements: {:?}",
        rigid.refinements
    );

    // Required members should include moment_of_inertia
    let member_names: Vec<&str> = rigid
        .required_members
        .iter()
        .map(|r| r.name.as_str())
        .collect();
    assert!(
        member_names.contains(&"moment_of_inertia"),
        "Rigid should have 'moment_of_inertia' required member, got: {:?}",
        member_names
    );

    // moment_of_inertia should be Real
    let moi = rigid
        .required_members
        .iter()
        .find(|r| r.name == "moment_of_inertia")
        .expect("expected 'moment_of_inertia' member");
    match &moi.kind {
        RequirementKind::Param(ty) => {
            assert_eq!(
                *ty,
                Type::Real,
                "moment_of_inertia should be Real, got {:?}",
                ty
            );
        }
        other => panic!("moment_of_inertia should be Param, got {:?}", other),
    }
}

// ─── step-11: ElasticallyDeformable cross-module refinement ──────────────────

/// Step 11: ElasticallyDeformable refines Elastic (cross-module refinement to
/// materials_mechanical.ri). Verify refinements list includes 'Elastic' and
/// that a structure conforming to ElasticallyDeformable via compile_with_prelude
/// works (provides youngs_modulus, poissons_ratio, shear_modulus from Elastic
/// plus max_elastic_strain).
#[test]
fn elastically_deformable_refines_elastic_cross_module() {
    let module = load_stdlib_module();

    let ed = module
        .trait_defs
        .iter()
        .find(|t| t.name == "ElasticallyDeformable")
        .expect("expected 'ElasticallyDeformable' trait in compiled module");

    // Refinements should contain "Elastic"
    assert!(
        ed.refinements.contains(&"Elastic".to_string()),
        "ElasticallyDeformable should refine Elastic, got refinements: {:?}",
        ed.refinements
    );

    // Has max_elastic_strain required member
    assert!(
        ed.required_members
            .iter()
            .any(|r| r.name == "max_elastic_strain"),
        "ElasticallyDeformable should have 'max_elastic_strain' member, got: {:?}",
        ed.required_members
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );
}

/// Cross-module conformance: a structure conforming to ElasticallyDeformable
/// must provide Elastic's members (youngs_modulus, poissons_ratio, shear_modulus)
/// plus max_elastic_strain — all via prelude.
#[test]
fn structure_conforms_to_elastically_deformable_via_prelude() {
    let source = r#"
structure def Rubber : ElasticallyDeformable {
    param youngs_modulus : Real = 0.01
    param poissons_ratio : Real = 0.49
    param shear_modulus : Real = 0.003
    param max_elastic_strain : Real = 5.0
}
"#;
    let prelude = stdlib_loader::load_stdlib();
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile_with_prelude(&parsed, prelude);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "Rubber : ElasticallyDeformable should compile without errors, got: {:?}",
        errors
    );

    let template = compiled
        .templates
        .first()
        .expect("expected at least 1 template");
    assert!(
        template
            .trait_bounds
            .contains(&"ElasticallyDeformable".to_string()),
        "Rubber should have 'ElasticallyDeformable' trait bound, got: {:?}",
        template.trait_bounds
    );
}

// ─── step-13: constraint injection from Physical ─────────────────────────────

/// Step 13: constraints from Physical (volume > 0) are injected into a
/// conforming structure. Assert template.constraints is non-empty.
#[test]
fn physical_constraint_injected_into_conforming_structure() {
    let source = r#"
structure def Block : Physical {
    param density : Real = 7850.0
    param name : String = "block"
    param volume : Real = 0.5
    param centroid_x : Real = 0.0
    param centroid_y : Real = 0.0
    param centroid_z : Real = 0.0
}
"#;
    let prelude = stdlib_loader::load_stdlib();
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile_with_prelude(&parsed, prelude);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "Block : Physical should compile without errors, got: {:?}",
        errors
    );

    let template = compiled
        .templates
        .first()
        .expect("expected at least 1 template");
    assert!(
        !template.constraints.is_empty(),
        "expected constraint from Physical trait (volume > 0) injected into Block, but constraints is empty"
    );
}

// ─── step-15: missing member detection ───────────────────────────────────────

/// Step 15: A structure conforming to Physical but omitting 'volume' produces
/// an error diagnostic mentioning 'missing required member'.
#[test]
fn missing_volume_produces_error_diagnostic() {
    let source = r#"
structure def Incomplete : Physical {
    param density : Real = 7850.0
    param name : String = "no volume"
    param centroid_x : Real = 0.0
    param centroid_y : Real = 0.0
    param centroid_z : Real = 0.0
}
"#;
    let prelude = stdlib_loader::load_stdlib();
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile_with_prelude(&parsed, prelude);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected error for missing 'volume' member in Physical conformance, but got no errors"
    );

    // At least one error should mention 'volume' or 'missing'
    let has_volume_error = errors.iter().any(|d| {
        let msg = d.message.to_lowercase();
        msg.contains("volume") || msg.contains("missing")
    });
    assert!(
        has_volume_error,
        "expected error mentioning 'volume' or 'missing', got: {:?}",
        errors
    );
}

// ─── step-21: load_stdlib_module uses production path (wrong code path) ──────

/// Step 21: Regression test for review issue [wrong_code_path_under_test].
/// Asserts that load_stdlib_module() returns a module with the production path
/// `std/structural/physical`, NOT the standalone `stdlib` path from the old
/// helper that used compile(&parsed) with ModulePath::single("stdlib").
#[test]
fn load_stdlib_module_uses_production_path() {
    let module = load_stdlib_module();

    assert_eq!(
        module.path.to_string(),
        "std/structural/physical",
        "load_stdlib_module() should return the production module path \
         (std/structural/physical), not a standalone compilation path. \
         This indicates the helper is using the wrong code path."
    );
}

// ─── step-17: all stdlib modules error-free (silent failure boundary) ────────

/// Step 17: Verify that ALL stdlib modules returned by load_stdlib() have zero
/// Error-severity diagnostics. This validates the postcondition that the loader
/// never silently caches a broken module. Iterates through every CompiledModule,
/// not just structural_physical.
#[test]
fn all_stdlib_modules_have_zero_error_diagnostics() {
    let modules = stdlib_loader::load_stdlib();
    assert!(
        !modules.is_empty(),
        "load_stdlib() should return at least one module"
    );

    for module in modules {
        let errors: Vec<_> = module
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "stdlib module '{}' has Error-severity diagnostics (silent failure at initialization boundary): {:?}",
            module.path,
            errors
        );

        // Verify each non-units module has at least one trait def (not empty/broken).
        // std/units only contains unit declarations, no traits.
        if !module.path.to_string().contains("units") {
            assert!(
                !module.trait_defs.is_empty(),
                "stdlib module '{}' has zero trait definitions — may be silently broken",
                module.path
            );
        }
    }
}

// ─── step-19: cross-module refinement chain via load_stdlib ──────────────────

/// Step 19: Verify cross-module refinement chain works end-to-end through
/// load_stdlib(). Compile a structure conforming to Rigid (which refines
/// Physical, which refines Material from materials_mechanical.ri — a 3-level
/// chain spanning two stdlib modules). Assert no errors and verify requirements
/// from ALL three levels are inherited: moment_of_inertia from Rigid,
/// volume/centroid_x/y/z from Physical, and density/name from Material.
#[test]
fn rigid_cross_module_three_level_refinement_chain() {
    let source = r#"
structure def Beam : Rigid {
    // Material requirements (from materials_mechanical.ri)
    param density : Real = 7850.0
    param name : String = "steel beam"

    // Physical requirements (from structural_physical.ri)
    param volume : Real = 0.01
    param centroid_x : Real = 0.0
    param centroid_y : Real = 0.0
    param centroid_z : Real = 0.0

    // Rigid requirement (from structural_physical.ri)
    param moment_of_inertia : Real = 0.00012
}
"#;
    let prelude = stdlib_loader::load_stdlib();
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile_with_prelude(&parsed, prelude);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "Beam : Rigid should compile without errors (3-level cross-module chain), got: {:?}",
        errors
    );

    let template = compiled
        .templates
        .first()
        .expect("expected at least 1 template");

    // Verify trait bound
    assert!(
        template.trait_bounds.contains(&"Rigid".to_string()),
        "Beam should have 'Rigid' trait bound, got: {:?}",
        template.trait_bounds
    );

    // Verify value cells from all three levels exist
    let cell_names: Vec<&str> = template
        .value_cells
        .iter()
        .map(|vc| vc.id.member.as_str())
        .collect();

    // From Material (level 1, materials_mechanical.ri)
    assert!(
        cell_names.contains(&"density"),
        "missing 'density' from Material, cells: {:?}",
        cell_names
    );
    assert!(
        cell_names.contains(&"name"),
        "missing 'name' from Material, cells: {:?}",
        cell_names
    );

    // From Physical (level 2, structural_physical.ri)
    assert!(
        cell_names.contains(&"volume"),
        "missing 'volume' from Physical, cells: {:?}",
        cell_names
    );
    assert!(
        cell_names.contains(&"centroid_x"),
        "missing 'centroid_x' from Physical, cells: {:?}",
        cell_names
    );

    // From Rigid (level 3, structural_physical.ri)
    assert!(
        cell_names.contains(&"moment_of_inertia"),
        "missing 'moment_of_inertia' from Rigid, cells: {:?}",
        cell_names
    );

    // Computed default from Physical: mass = volume * density
    assert!(
        cell_names.contains(&"mass"),
        "missing 'mass' computed default from Physical, cells: {:?}",
        cell_names
    );
}
