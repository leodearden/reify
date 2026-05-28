//! Tests for stdlib/structural_physical.ri — structural/physical traits.
//!
//! Tests validate that the .ri file parses and compiles cleanly, that each
//! trait is correctly represented in the compiled module, and that trait
//! conformance and constraint injection work as expected.

use reify_ir::*;
use reify_compiler::*;
use reify_test_support::{compile_source_with_stdlib, errors_only};
use reify_core::*;

mod common;

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

/// Parse and compile `source` against the full stdlib prelude, asserting no
/// parse or compile errors. Returns the `CompiledModule` for further inspection.
fn compile_structure(source: &str) -> CompiledModule {
    let compiled = common::compile_with_stdlib_helper(source);
    let errors = errors_only(&compiled);
    assert!(errors.is_empty(), "compile errors: {:?}", errors);
    compiled
}

/// Assert that the constraint referencing `member` in `template` uses `expected`
/// as its BinOp operator. Panics with a message containing `member` on failure.
fn assert_constraint_op(template: &TopologyTemplate, member: &str, expected: BinOp) {
    let cc = template
        .constraints
        .iter()
        .find(|cc| {
            matches!(&cc.expr.kind, CompiledExprKind::BinOp { left, .. }
                if matches!(&left.kind, CompiledExprKind::ValueRef(id) if id.member == member))
        })
        .unwrap_or_else(|| panic!("expected a constraint referencing {member}"));
    let (op, _, _) = common::expect_binop(&cc.expr);
    assert_eq!(
        *op, expected,
        "{member} constraint expected BinOp::{expected:?}, got BinOp::{op:?}"
    );
}

/// Assert that `template.constraints` contains the spec-shape Physical
/// `material.density > 0` constraint (post-GHR-α / task 3603), positively
/// pinning its lowered shape:
///
///   `BinOp { op: Gt, left: IndexAccess { object: ValueRef(material),
///                                        index: Literal(String("density")) },
///            right: Literal(Int(0) | Real(~0)) }`
///
/// This is the shape produced by SIR-α's struct-member access lowering
/// (`expr.rs:1948-1964`), which turns `material.density` into
/// `IndexAccess(ValueRef(material), Literal("density"))`. Without a positive
/// shape check, an empty-or-wrong-but-still-counted injected constraint
/// would slip past the `constraints.len() == 2` count assertions used by
/// the TC/EC inheritance tests and the `!constraints.is_empty()` check used
/// by `physical_constraint_injected_into_conforming_structure`.
fn assert_density_positive_constraint_present(template: &TopologyTemplate) {
    let matches: Vec<_> = template
        .constraints
        .iter()
        .filter(|cc| {
            let CompiledExprKind::BinOp { op, left, right } = &cc.expr.kind else {
                return false;
            };
            if *op != BinOp::Gt {
                return false;
            }
            // Left side: material.density lowered as IndexAccess(ValueRef("material"), "density")
            let CompiledExprKind::IndexAccess { object, index } = &left.kind else {
                return false;
            };
            let CompiledExprKind::ValueRef(id) = &object.kind else {
                return false;
            };
            if id.member != "material" {
                return false;
            }
            let CompiledExprKind::Literal(Value::String(key)) = &index.kind else {
                return false;
            };
            if key != "density" {
                return false;
            }
            // Right side: literal 0 (Int or Real near-zero).
            match &right.kind {
                CompiledExprKind::Literal(Value::Int(v)) => *v == 0,
                CompiledExprKind::Literal(Value::Real(v)) => v.abs() < 1e-9,
                _ => false,
            }
        })
        .collect();

    assert!(
        !matches.is_empty(),
        "expected an injected `material.density > 0` constraint with shape \
         BinOp(Gt, IndexAccess(material, \"density\"), 0); got constraint \
         shapes: {:?}",
        template
            .constraints
            .iter()
            .map(|c| format!("{:?}", c.expr.kind))
            .collect::<Vec<_>>()
    );
}

/// Minimal structure that conforms to `Plastic` (which refines `Flexible`) and
/// carries all four injected constraints: `plastic_strain` (≥ 0, `BinOp::Ge`),
/// `hardening_modulus` (> 0, `BinOp::Gt`), `stiffness` (> 0, `BinOp::Gt`), and
/// `max_deflection` (> 0, `BinOp::Gt`). Shared by the `assert_constraint_op`
/// helper tests.
const PLASTIC_BODY_SRC: &str = r#"
structure def PlasticBody : Plastic {
    param plastic_strain : Real = 0.0
    param hardening_modulus : Real = 500.0
    param stiffness : Stiffness = 1000.0 * 1N / 1m
    param max_deflection : Real = 0.1
}
"#;

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

// ─── Physical trait shape (PRD §8 Phase 1 / GHR-α / task 3603) ───────────────

/// Spec-shape Physical (post-GHR-α / task 3603):
///   - `refinements` is EMPTY — `MaterialSpec` is no longer a trait edge;
///     Material is a struct slot now (see SIR-α / task 3540).
///   - `required_members` are exactly `geometry : Solid` (resolved to
///     `Type::Geometry`) and `material : Material` (resolved to
///     `Type::StructureRef("Material")`).
#[test]
fn physical_trait_has_geometry_and_material_params_only() {
    let module = load_stdlib_module();

    let physical = module
        .trait_defs
        .iter()
        .find(|t| t.name == "Physical")
        .expect("expected 'Physical' trait in compiled module");

    // (1) refinements is empty — Material is a struct slot now, not a trait edge.
    assert!(
        physical.refinements.is_empty(),
        "spec-shape Physical should have NO trait refinements (Material is a \
         struct slot now, not a trait edge); got: {:?}",
        physical.refinements
    );

    // (2) required_members: exactly `geometry` and `material`.
    let member_names: Vec<&str> = physical
        .required_members
        .iter()
        .map(|r| r.name.as_str())
        .collect();
    assert_eq!(
        physical.required_members.len(),
        2,
        "spec-shape Physical should have exactly 2 required members \
         (geometry, material), got: {:?}",
        member_names
    );

    let geometry_req = physical
        .required_members
        .iter()
        .find(|r| r.name == "geometry")
        .unwrap_or_else(|| {
            panic!(
                "Physical should require `geometry` member; got: {:?}",
                member_names
            )
        });
    match &geometry_req.kind {
        RequirementKind::Param(ty) => assert_eq!(
            *ty,
            Type::Geometry,
            "Physical.geometry should be Param(Geometry) (the resolved type behind \
             the `Solid` surface alias); got {:?}",
            ty
        ),
        other => panic!(
            "Physical.geometry should be RequirementKind::Param, got {:?}",
            other
        ),
    }

    let material_req = physical
        .required_members
        .iter()
        .find(|r| r.name == "material")
        .unwrap_or_else(|| {
            panic!(
                "Physical should require `material` member; got: {:?}",
                member_names
            )
        });
    match &material_req.kind {
        RequirementKind::Param(ty) => assert_eq!(
            *ty,
            Type::StructureRef("Material".to_string()),
            "Physical.material should be Param(StructureRef(\"Material\")); got {:?}",
            ty
        ),
        other => panic!(
            "Physical.material should be RequirementKind::Param, got {:?}",
            other
        ),
    }
}

/// Spec-shape Physical exposes two `Let` defaults — `mass = volume(geometry) *
/// material.density` and `centroid = centroid(geometry)`. Both are typecheck-only
/// in Phase 1 (kernel dispatch arrives in Phase 6 / GHR-ζ); this test pins their
/// presence as Let-defaults on the trait.
#[test]
fn physical_trait_has_mass_and_centroid_lets() {
    let module = load_stdlib_module();

    let physical = module
        .trait_defs
        .iter()
        .find(|t| t.name == "Physical")
        .expect("expected 'Physical' trait in compiled module");

    let let_defaults: Vec<_> = physical
        .defaults
        .iter()
        .filter(|d| matches!(d.kind, DefaultKind::Let { .. }))
        .collect();

    for expected_let in &["mass", "centroid"] {
        assert!(
            let_defaults
                .iter()
                .any(|d| d.name.as_deref() == Some(*expected_let)),
            "spec-shape Physical should have a `Let` default named '{}'; got: {:?}",
            expected_let,
            physical
                .defaults
                .iter()
                .map(|d| &d.name)
                .collect::<Vec<_>>()
        );
    }
}

// ─── Bracket : Physical conformance (GHR-α / task 3603) ──────────────────────

/// Spec-shape `Bracket : Physical` compiles cleanly with the new struct-slot
/// composition (geometry + material), trait_bound contains "Physical", and
/// the injected trait Let defaults produce `mass` + `centroid` value cells.
///
/// `geometry` is intentionally NOT checked in `value_cells` — Solid-typed
/// params lower to realizations (Type::Geometry is rejected by
/// `is_representable_cell_type`; see `solid_param_tests.rs`).
#[test]
fn bracket_conforms_to_physical_with_geometry_and_material() {
    let source = r#"
structure def Bracket : Physical {
    param geometry : Solid = box(10mm, 20mm, 30mm)
    param material : Material = Material(name: "steel", density: 7850.0, youngs_modulus: 200000000000.0)
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

    // Injected Let defaults from Physical: `mass` + `centroid`.
    for expected_cell in &["mass", "centroid", "material"] {
        assert!(
            template
                .value_cells
                .iter()
                .any(|vc| vc.id.member == *expected_cell),
            "expected '{}' value cell on Bracket, got cells: {:?}",
            expected_cell,
            template
                .value_cells
                .iter()
                .map(|vc| vc.id.member.as_str())
                .collect::<Vec<_>>()
        );
    }
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

// ─── step-11: ElasticallyDeformable same-module refinement ───────────────────

/// Step 11: ElasticallyDeformable refines Flexible (same-module refinement;
/// elastic deformation is a specific form of reversible flexibility). Verify
/// refinements list includes 'Flexible' and that the trait has
/// max_elastic_strain as a required member.
#[test]
fn elastically_deformable_refines_flexible_same_module() {
    let module = load_stdlib_module();

    let ed = module
        .trait_defs
        .iter()
        .find(|t| t.name == "ElasticallyDeformable")
        .expect("expected 'ElasticallyDeformable' trait in compiled module");

    // Refinements should contain "Flexible"
    assert!(
        ed.refinements.contains(&"Flexible".to_string()),
        "ElasticallyDeformable should refine Flexible, got refinements: {:?}",
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

/// Plastic refines Flexible (plastic deformation extends the elastic flexibility
/// contract). Verify refinements list includes 'Flexible'.
#[test]
fn plastic_refines_flexible() {
    let module = load_stdlib_module();

    let plastic = module
        .trait_defs
        .iter()
        .find(|t| t.name == "Plastic")
        .expect("expected 'Plastic' trait in compiled module");

    assert!(
        plastic.refinements.contains(&"Flexible".to_string()),
        "Plastic should refine Flexible, got refinements: {:?}",
        plastic.refinements
    );
}

/// ThermallyConductive refines Physical (a thermally conductive body is also a
/// physical body). Verify refinements list includes 'Physical'.
#[test]
fn thermally_conductive_refines_physical() {
    let module = load_stdlib_module();

    let tc = module
        .trait_defs
        .iter()
        .find(|t| t.name == "ThermallyConductive")
        .expect("expected 'ThermallyConductive' trait in compiled module");

    assert!(
        tc.refinements.contains(&"Physical".to_string()),
        "ThermallyConductive should refine Physical, got refinements: {:?}",
        tc.refinements
    );
}

/// ElectricallyConductive refines Physical (an electrically conductive body is
/// also a physical body). Verify refinements list includes 'Physical'.
#[test]
fn electrically_conductive_refines_physical() {
    let module = load_stdlib_module();

    let ec = module
        .trait_defs
        .iter()
        .find(|t| t.name == "ElectricallyConductive")
        .expect("expected 'ElectricallyConductive' trait in compiled module");

    assert!(
        ec.refinements.contains(&"Physical".to_string()),
        "ElectricallyConductive should refine Physical, got refinements: {:?}",
        ec.refinements
    );
}

/// Conformance test: a structure conforming to ThermallyConductive must satisfy
/// Physical's requirements (geometry, material) plus its own
/// (thermal_conductivity, max_service_temp). Physical's `material.density > 0`
/// constraint (post-GHR-α / task 3603, replacing the legacy `volume > 0`) must
/// be injected via inheritance. Exercises the two-level TC→Physical chain.
#[test]
fn structure_conforms_to_thermally_conductive_with_inherited_physical_constraints() {
    let compiled = compile_structure(
        r#"
structure def HeatSink : ThermallyConductive {
    param geometry : Solid = box(10mm, 20mm, 30mm)
    param material : Material = Material(name: "aluminum", density: 2700.0, youngs_modulus: 70000000000.0)
    param thermal_conductivity : ThermalConductivity = 205.0 * 1W / (1m * 1K)
    param max_service_temp : Real = 573.0
}
"#,
    );

    let template = compiled
        .templates
        .first()
        .expect("expected at least 1 template");

    assert!(
        template
            .trait_bounds
            .contains(&"ThermallyConductive".to_string()),
        "HeatSink should have 'ThermallyConductive' trait bound, got: {:?}",
        template.trait_bounds
    );

    // ThermallyConductive's own `thermal_conductivity > 0`.
    assert_constraint_op(template, "thermal_conductivity", BinOp::Gt);
    // Physical's `material.density > 0` (post-GHR-α) is injected via inheritance —
    // its left side is a member access through `material` (IndexAccess
    // lowering), so it isn't reachable via `assert_constraint_op`'s ValueRef-
    // only matcher. The positive shape check below pins that the inherited
    // constraint IS the expected density-positivity, not an empty / wrong /
    // count-only artefact (reviewer #1 follow-up).
    assert_density_positive_constraint_present(template);
    assert_eq!(
        template.constraints.len(),
        2,
        "expected exactly 2 constraints from chain ThermallyConductive→Physical \
         (material.density > 0 inherited from Physical, thermal_conductivity > 0 \
         from ThermallyConductive), got {}: {:?}",
        template.constraints.len(),
        template
            .constraints
            .iter()
            .map(|c| format!("{:?}", c.expr.kind))
            .collect::<Vec<_>>()
    );
}

/// Conformance test: a structure conforming to ElectricallyConductive must
/// satisfy Physical's requirements (geometry, material) plus its own
/// (electrical_conductivity, resistivity). Physical's `material.density > 0`
/// constraint (post-GHR-α / task 3603, replacing the legacy `volume > 0`) must
/// be injected via inheritance. Exercises the two-level EC→Physical chain.
#[test]
fn structure_conforms_to_electrically_conductive_with_inherited_physical_constraints() {
    let compiled = compile_structure(
        r#"
structure def Wire : ElectricallyConductive {
    param geometry : Solid = box(10mm, 20mm, 30mm)
    param material : Material = Material(name: "copper", density: 8960.0, youngs_modulus: 110000000000.0)
    param electrical_conductivity : ElectricalConductivity = 1000.0 * 1S / 1m
    param resistivity : ElectricResistivity = 0.001 * 1ohm * 1m
}
"#,
    );

    let template = compiled
        .templates
        .first()
        .expect("expected at least 1 template");

    assert!(
        template
            .trait_bounds
            .contains(&"ElectricallyConductive".to_string()),
        "Wire should have 'ElectricallyConductive' trait bound, got: {:?}",
        template.trait_bounds
    );

    // ElectricallyConductive's own `electrical_conductivity > 0`.
    assert_constraint_op(template, "electrical_conductivity", BinOp::Gt);
    // Physical's `material.density > 0` (post-GHR-α) is injected via inheritance;
    // see the TC analog above for the matcher-shape rationale. Positive shape
    // pin guards against an empty / wrong injected constraint slipping past
    // the count check below (reviewer #1 follow-up).
    assert_density_positive_constraint_present(template);
    assert_eq!(
        template.constraints.len(),
        2,
        "expected exactly 2 constraints from chain ElectricallyConductive→Physical \
         (material.density > 0 inherited from Physical, electrical_conductivity > 0 \
         from ElectricallyConductive), got {}: {:?}",
        template.constraints.len(),
        template
            .constraints
            .iter()
            .map(|c| format!("{:?}", c.expr.kind))
            .collect::<Vec<_>>()
    );
}

/// Same-module conformance: a structure conforming to ElasticallyDeformable
/// must provide Flexible's inherited members (stiffness, max_deflection) plus
/// ElasticallyDeformable's own max_elastic_strain. Exercises the same-module
/// inheritance path (both traits in std/structural/physical).
#[test]
fn structure_conforms_to_elastically_deformable_with_inherited_flexible_members() {
    let source = r#"
structure def Rubber : ElasticallyDeformable {
    param stiffness : Stiffness = 1000.0 * 1N / 1m
    param max_deflection : Real = 0.1
    param max_elastic_strain : Real = 5.0
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

    // Inherited from Flexible: `stiffness > 0` and `max_deflection > 0`.
    assert_constraint_op(template, "stiffness", BinOp::Gt);
    assert_constraint_op(template, "max_deflection", BinOp::Gt);
    // ElasticallyDeformable's own constraint: `max_elastic_strain > 0`.
    assert_constraint_op(template, "max_elastic_strain", BinOp::Gt);
    assert_eq!(
        template.constraints.len(),
        3,
        "expected exactly 3 constraints from ElasticallyDeformable+Flexible \
         (stiffness > 0, max_deflection > 0, max_elastic_strain > 0), got {}",
        template.constraints.len()
    );
}

// ─── constraint injection from Physical (post-GHR-α / task 3603) ────────────

/// Constraints from Physical (`material.density > 0` — post-GHR-α; previously
/// `volume > 0`) are injected into a conforming structure. The "non-empty
/// constraints" invariant survives the spec-shape rewrite.
#[test]
fn physical_constraint_injected_into_conforming_structure() {
    let source = r#"
structure def Block : Physical {
    param geometry : Solid = box(10mm, 20mm, 30mm)
    param material : Material = Material(name: "block", density: 7850.0, youngs_modulus: 200000000000.0)
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
        "Block : Physical should compile without errors, got: {:?}",
        errors
    );

    let template = compiled
        .templates
        .first()
        .expect("expected at least 1 template");
    assert!(
        !template.constraints.is_empty(),
        "expected constraint from Physical trait (material.density > 0) injected \
         into Block, but constraints is empty"
    );
    // Positive shape pin (reviewer #1 follow-up): the bare "non-empty" check
    // above is preserved, but a regression that injected an empty / wrong /
    // count-only constraint would silently pass. Pin the exact lowered shape
    // of `material.density > 0` (struct-member access via IndexAccess) so the
    // SIR-α member-access path — exactly the new/risky behavior this PRD
    // introduces — has at least one positive test in the suite.
    assert_density_positive_constraint_present(template);
}

// ─── missing geometry detection (post-GHR-α / task 3603) ─────────────────────

/// A structure conforming to Physical but omitting `geometry` produces an
/// error diagnostic mentioning 'geometry' or 'missing'. Mirrors the legacy
/// `missing_volume_produces_error_diagnostic` for the new spec-shape contract.
#[test]
fn missing_geometry_produces_error_diagnostic() {
    let source = r#"
structure def Incomplete : Physical {
    param material : Material = Material(name: "no geometry", density: 7850.0, youngs_modulus: 200000000000.0)
}
"#;
    let compiled = compile_source_with_stdlib(source);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected error for missing 'geometry' member in Physical conformance, but got no errors"
    );

    let has_geometry_error = errors.iter().any(|d| {
        let msg = d.message.to_lowercase();
        msg.contains("geometry") || msg.contains("missing")
    });
    assert!(
        has_geometry_error,
        "expected error mentioning 'geometry' or 'missing', got: {:?}",
        errors
    );
}

// ─── task-558 step-1: Plastic trait has constraint defaults ──────────────────

/// Plastic trait should have exactly 2 constraint defaults:
/// `hardening_modulus > 0` and `plastic_strain >= 0`.
/// FAILS before constraints are added to structural_physical.ri.
#[test]
fn plastic_trait_has_constraint_defaults() {
    let module = load_stdlib_module();

    let plastic = module
        .trait_defs
        .iter()
        .find(|t| t.name == "Plastic")
        .expect("expected 'Plastic' trait in compiled module");

    let constraint_defaults: Vec<_> = plastic
        .defaults
        .iter()
        .filter(|d| matches!(d.kind, DefaultKind::Constraint(_)))
        .collect();

    assert_eq!(
        constraint_defaults.len(),
        2,
        "Plastic trait should have exactly 2 constraint defaults \
         (hardening_modulus > 0 and plastic_strain >= 0), got {} defaults: {:?}",
        constraint_defaults.len(),
        plastic.defaults.iter().map(|d| &d.kind).collect::<Vec<_>>()
    );
}

// ─── task-558 step-3: Plastic conforming structure has constraints injected ───

/// A structure conforming to Plastic should have exactly 4 constraints injected
/// from the Plastic+Flexible traits: `hardening_modulus > 0`,
/// `plastic_strain >= 0`, `stiffness > 0`, and `max_deflection > 0`.
#[test]
fn plastic_conforming_structure_has_constraints_injected() {
    let compiled = compile_structure(
        r#"
structure def PlasticBody : Plastic {
    param plastic_strain : Real = 0.05
    param hardening_modulus : Real = 1000.0
    param stiffness : Stiffness = 1000.0 * 1N / 1m
    param max_deflection : Real = 0.1
}
"#,
    );

    let template = compiled
        .templates
        .first()
        .expect("expected at least 1 template");

    assert!(
        template.trait_bounds.contains(&"Plastic".to_string()),
        "PlasticBody should have 'Plastic' trait bound, got: {:?}",
        template.trait_bounds
    );

    assert_eq!(
        template.constraints.len(),
        4,
        "expected exactly 4 constraints injected from Plastic+Flexible traits, got {}",
        template.constraints.len()
    );
}

// ─── task-558 step-4: Plastic constraint expressions use correct operators ────

/// Among the 4 injected Plastic+Flexible constraints, the Plastic-specific ones
/// must use the correct comparison operators:
/// - `hardening_modulus > 0` uses BinOp::Gt (strictly greater-than), RHS = 0
/// - `plastic_strain >= 0` uses BinOp::Ge (greater-than-or-equal), RHS = 0
#[test]
fn plastic_constraint_expressions_use_correct_operators() {
    let compiled = compile_structure(
        r#"
structure def PlasticBody : Plastic {
    param plastic_strain : Real = 0.05
    param hardening_modulus : Real = 1000.0
    param stiffness : Stiffness = 1000.0 * 1N / 1m
    param max_deflection : Real = 0.1
}
"#,
    );

    let template = compiled
        .templates
        .first()
        .expect("expected at least 1 template");

    assert_eq!(
        template.constraints.len(),
        4,
        "expected exactly 4 constraints from Plastic+Flexible, got {}",
        template.constraints.len()
    );

    // Find constraint for hardening_modulus (should use Gt, RHS=0) and
    // plastic_strain (should use Ge, RHS=0).
    let mut found_hm_gt = false;
    let mut found_ps_ge = false;
    // Collect (member, op) pairs for diagnostic messages if assertions fail.
    let mut found_pairs: Vec<(String, String)> = Vec::new();
    // Collect any constraint shapes that are not BinOp; the test asserts this
    // stays empty so IR changes produce an explicit failure rather than silence.
    let mut unrecognised: Vec<String> = Vec::new();

    for cc in &template.constraints {
        match &cc.expr.kind {
            CompiledExprKind::BinOp { op, left, right } => {
                let member = match &left.kind {
                    CompiledExprKind::ValueRef(id) => id.member.as_str(),
                    _ => continue,
                };
                found_pairs.push((member.to_string(), format!("{:?}", op)));
                let rhs_is_zero = match &right.kind {
                    CompiledExprKind::Literal(Value::Int(v)) => *v == 0,
                    CompiledExprKind::Literal(Value::Real(v)) => v.abs() < 1e-9,
                    _ => false,
                };
                match (member, op) {
                    ("hardening_modulus", BinOp::Gt) if rhs_is_zero => found_hm_gt = true,
                    ("plastic_strain", BinOp::Ge) if rhs_is_zero => found_ps_ge = true,
                    _ => {}
                }
            }
            other => {
                unrecognised.push(format!("{:?}", other));
            }
        }
    }

    assert!(
        unrecognised.is_empty(),
        "expected all Plastic constraint expressions to be BinOp, \
         got unrecognised shapes: {:?}",
        unrecognised
    );
    assert!(
        found_hm_gt,
        "expected BinOp(Gt, hardening_modulus, 0), found_pairs: {:?}",
        found_pairs
    );
    assert!(
        found_ps_ge,
        "expected BinOp(Ge, plastic_strain, 0), found_pairs: {:?}",
        found_pairs
    );

    // Verify Flexible's inherited constraints also use strictly-greater-than so
    // a future regression (e.g. Gt→Ge for stiffness) is caught here, not just
    // by the len()==4 check above.
    assert_constraint_op(template, "stiffness", BinOp::Gt);
    assert_constraint_op(template, "max_deflection", BinOp::Gt);
}

// ─── task-558 step-5: plastic_strain=0.0 boundary value compiles ─────────────

/// Boundary-value test: compile a structure with plastic_strain=0.0.
/// Verifies two things:
/// 1. The structure compiles without errors (zero plastic_strain is a valid input).
/// 2. The injected constraint for plastic_strain uses `>=` (Ge), not `>` (Gt).
///    Because the compiler injects but does not evaluate constraints, the `>=`
///    semantics are confirmed by inspecting the BinOp operator directly.
#[test]
fn plastic_strain_zero_boundary_compiles() {
    let compiled = compile_structure(
        r#"
structure def PlasticBody : Plastic {
    param plastic_strain : Real = 0.0
    param hardening_modulus : Real = 500.0
    param stiffness : Stiffness = 1000.0 * 1N / 1m
    param max_deflection : Real = 0.1
}
"#,
    );

    let template = compiled
        .templates
        .first()
        .expect("expected at least 1 template");

    assert_constraint_op(template, "plastic_strain", BinOp::Ge);
}

// ─── task-1688: hardening_modulus=0.0 boundary value compiles ────────────────

/// Boundary-value test: compile a structure with hardening_modulus=0.0.
/// Verifies two things:
/// 1. The structure compiles without errors (zero hardening_modulus is accepted
///    at compile time — the compiler injects but does not evaluate constraints).
/// 2. The injected constraint for hardening_modulus uses `>` (Gt), not `>=` (Ge).
///    This is the strictly-greater-than boundary, distinct from plastic_strain's
///    `>=` boundary. Mirrors plastic_strain_zero_boundary_compiles for the other
///    Plastic boundary dimension.
#[test]
fn hardening_modulus_zero_boundary_compiles() {
    let compiled = compile_structure(
        r#"
structure def PlasticBody : Plastic {
    param plastic_strain : Real = 0.05
    param hardening_modulus : Real = 0.0
    param stiffness : Stiffness = 1000.0 * 1N / 1m
    param max_deflection : Real = 0.1
}
"#,
    );

    let template = compiled
        .templates
        .first()
        .expect("expected at least 1 template");

    assert_constraint_op(template, "hardening_modulus", BinOp::Gt);
}

// ─── task-1699: assert_constraint_op helper ───────────────────────────────────

/// Validates that `assert_constraint_op` panics with the member name in the
/// message when the wrong operator is passed. Deliberately passes `BinOp::Gt`
/// for `plastic_strain` (the real constraint is `BinOp::Ge`).
#[test]
#[should_panic(expected = "plastic_strain constraint expected BinOp::Gt")]
fn assert_constraint_op_detects_wrong_operator() {
    let compiled = compile_structure(PLASTIC_BODY_SRC);

    let template = compiled
        .templates
        .first()
        .expect("expected at least 1 template");

    // Deliberately pass Gt (wrong) instead of Ge (correct) — must panic.
    assert_constraint_op(template, "plastic_strain", BinOp::Gt);
}

// ─── task-1700: assert_constraint_op member-not-found path ───────────────────

/// Validates that `assert_constraint_op` panics with a message containing
/// "expected a constraint referencing" when the member name is not found in
/// any constraint. Exercises the `unwrap_or_else` panic path at line 43.
#[test]
#[should_panic(expected = "expected a constraint referencing")]
fn assert_constraint_op_detects_member_not_found() {
    let compiled = compile_structure(PLASTIC_BODY_SRC);

    let template = compiled
        .templates
        .first()
        .expect("expected at least 1 template");

    // Pass a non-existent member name — the .find() returns None and
    // the unwrap_or_else triggers the "expected a constraint referencing" panic.
    assert_constraint_op(template, "nonexistent", BinOp::Ge);
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

// ─── cross-module refinement chain via load_stdlib (post-GHR-α / task 3603) ──

/// Verify two-level Rigid→Physical refinement chain works end-to-end through
/// load_stdlib(). Post-GHR-α (task 3603), the chain is two levels (Rigid
/// refines Physical), not three — Physical no longer refines MaterialSpec
/// because Material is a struct slot now, not a trait edge.
///
/// Assert no errors and verify value cells exist for the inherited members:
/// `material` (struct-slot param from Physical), `moment_of_inertia` (from
/// Rigid), and the trait-injected lets `mass` + `centroid` from Physical.
/// (`geometry : Solid` lowers to a realization, not a value cell — see
/// `solid_param_tests.rs`.)
#[test]
fn rigid_cross_module_three_level_refinement_chain() {
    let source = r#"
structure def Beam : Rigid {
    // Physical requirements (post-GHR-α: geometry + material struct slot)
    param geometry : Solid = box(10mm, 20mm, 30mm)
    param material : Material = Material(name: "steel", density: 7850.0, youngs_modulus: 200000000000.0)

    // Rigid requirement (from structural_physical.ri)
    param moment_of_inertia : Real = 0.00012
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
        "Beam : Rigid should compile without errors (2-level Rigid→Physical chain), got: {:?}",
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

    // Verify value cells exist for the representable members across levels.
    let cell_names: Vec<&str> = template
        .value_cells
        .iter()
        .map(|vc| vc.id.member.as_str())
        .collect();

    // From Physical: `material` is a struct-typed param (StructureRef("Material")),
    // which IS representable, so it gets a value cell.
    assert!(
        cell_names.contains(&"material"),
        "missing 'material' from Physical, cells: {:?}",
        cell_names
    );

    // From Rigid (level 2, structural_physical.ri)
    assert!(
        cell_names.contains(&"moment_of_inertia"),
        "missing 'moment_of_inertia' from Rigid, cells: {:?}",
        cell_names
    );

    // Trait-injected Let defaults from Physical: `mass` and `centroid`.
    assert!(
        cell_names.contains(&"mass"),
        "missing 'mass' computed default from Physical, cells: {:?}",
        cell_names
    );
    assert!(
        cell_names.contains(&"centroid"),
        "missing 'centroid' computed default from Physical, cells: {:?}",
        cell_names
    );

    // From Physical: `geometry : Solid` lowers to a realization (Type::Geometry
    // is unrepresentable per `is_representable_cell_type`), so it does NOT
    // appear in value_cells. Pin the realization-side presence instead.
    assert!(
        !template.realizations.is_empty(),
        "expected at least one realization from `param geometry : Solid` on Beam; got none"
    );
    assert!(
        !cell_names.contains(&"geometry"),
        "geometry must NOT appear in value_cells (Solid params lower to realizations); \
         cells: {:?}",
        cell_names
    );
}

// ─── task #3115: blocked-composite trait members now carry dimension aliases ─

/// Task #3115: `Flexible.stiffness` is the named-dimension alias `Stiffness`
/// (N/m), tightened from the prior blocked-composite `Real` placeholder.
/// Pin the dimension so a future loosening would fail loudly.
#[test]
fn flexible_stiffness_member_is_stiffness_dimension() {
    let module = load_stdlib_module();

    let flexible = module
        .trait_defs
        .iter()
        .find(|t| t.name == "Flexible")
        .expect("expected 'Flexible' trait in compiled module");

    let req = flexible
        .required_members
        .iter()
        .find(|r| r.name == "stiffness")
        .expect("Flexible should have 'stiffness' member");

    match &req.kind {
        RequirementKind::Param(ty) => assert_eq!(
            *ty,
            Type::Scalar {
                dimension: DimensionVector::STIFFNESS,
            },
            "stiffness should be Scalar{{STIFFNESS}}, got {:?}",
            ty
        ),
        other => panic!("stiffness should be Param, got {:?}", other),
    }
}

/// Task #3115: `ThermallyConductive.thermal_conductivity` is the named-dimension
/// alias `ThermalConductivity` (W/(m·K)), tightened from the prior
/// blocked-composite `Real` placeholder.
#[test]
fn thermally_conductive_thermal_conductivity_member_is_thermal_conductivity_dimension() {
    let module = load_stdlib_module();

    let tc = module
        .trait_defs
        .iter()
        .find(|t| t.name == "ThermallyConductive")
        .expect("expected 'ThermallyConductive' trait in compiled module");

    let req = tc
        .required_members
        .iter()
        .find(|r| r.name == "thermal_conductivity")
        .expect("ThermallyConductive should have 'thermal_conductivity' member");

    match &req.kind {
        RequirementKind::Param(ty) => assert_eq!(
            *ty,
            Type::Scalar {
                dimension: DimensionVector::THERMAL_CONDUCTIVITY,
            },
            "thermal_conductivity should be Scalar{{THERMAL_CONDUCTIVITY}}, got {:?}",
            ty
        ),
        other => panic!("thermal_conductivity should be Param, got {:?}", other),
    }
}

/// Task #3115: `ElectricallyConductive.electrical_conductivity` is the
/// named-dimension alias `ElectricalConductivity` (S/m), tightened from the
/// prior blocked-composite `Real` placeholder.
#[test]
fn electrically_conductive_electrical_conductivity_member_is_electrical_conductivity_dimension() {
    let module = load_stdlib_module();

    let ec = module
        .trait_defs
        .iter()
        .find(|t| t.name == "ElectricallyConductive")
        .expect("expected 'ElectricallyConductive' trait in compiled module");

    let req = ec
        .required_members
        .iter()
        .find(|r| r.name == "electrical_conductivity")
        .expect("ElectricallyConductive should have 'electrical_conductivity' member");

    match &req.kind {
        RequirementKind::Param(ty) => assert_eq!(
            *ty,
            Type::Scalar {
                dimension: DimensionVector::ELECTRICAL_CONDUCTIVITY,
            },
            "electrical_conductivity should be Scalar{{ELECTRICAL_CONDUCTIVITY}}, got {:?}",
            ty
        ),
        other => panic!("electrical_conductivity should be Param, got {:?}", other),
    }
}

/// Task #3115: `ElectricallyConductive.resistivity` is the named-dimension
/// alias `ElectricResistivity` (Ω·m), tightened from the prior
/// blocked-composite `Real` placeholder. Distinct from the bare `Resistance`
/// dimension (Ω) so the alias name is `ElectricResistivity`.
#[test]
fn electrically_conductive_resistivity_member_is_electric_resistivity_dimension() {
    let module = load_stdlib_module();

    let ec = module
        .trait_defs
        .iter()
        .find(|t| t.name == "ElectricallyConductive")
        .expect("expected 'ElectricallyConductive' trait in compiled module");

    let req = ec
        .required_members
        .iter()
        .find(|r| r.name == "resistivity")
        .expect("ElectricallyConductive should have 'resistivity' member");

    match &req.kind {
        RequirementKind::Param(ty) => assert_eq!(
            *ty,
            Type::Scalar {
                dimension: DimensionVector::ELECTRIC_RESISTIVITY,
            },
            "resistivity should be Scalar{{ELECTRIC_RESISTIVITY}}, got {:?}",
            ty
        ),
        other => panic!("resistivity should be Param, got {:?}", other),
    }
}
