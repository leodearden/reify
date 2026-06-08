//! Tests for stdlib/tensegrity.ri — Tensegrity structure network types:
//! Strut, Cable, Tensegrity, TensegrityWire, FormFindResult.
//!
//! Tests validate that the .ri file is loaded by the production stdlib path,
//! that all five structure_defs are correctly represented in the compiled
//! module, and that param signatures match PRD §3.
//!
//! All tests use the production-path `load_stdlib_module()` helper that
//! exercises the same embedded + sequential-prelude compilation path as
//! production (not a standalone `.ri` file re-read). This mirrors the
//! pattern in `materials_fea_tests.rs`.
//!
//! RED state for step-1: `load_stdlib_module()` panics because
//! `std/tensegrity` is not yet registered — every test below will fail
//! at the `.expect("stdlib should contain std/tensegrity module")` panic.

use reify_compiler::*;
use reify_core::*;

// ─── helpers ──────────────────────────────────────────────────────────────────

/// Look up a structure template by name within the `std/tensegrity` module.
fn find_structure(name: &str) -> &'static TopologyTemplate {
    let module = load_stdlib_module();
    module
        .templates
        .iter()
        .find(|t| t.name == name && t.entity_kind == EntityKind::Structure)
        .unwrap_or_else(|| {
            panic!(
                "expected `structure def {}` template in std/tensegrity, got templates: {:?}",
                name,
                module
                    .templates
                    .iter()
                    .map(|t| (&t.name, &t.entity_kind))
                    .collect::<Vec<_>>()
            )
        })
}

/// Collect the param-kind value cells (ignoring `let` and auto cells) from a
/// template, returning them in the file order they were declared.
fn param_cells(template: &TopologyTemplate) -> Vec<&ValueCellDecl> {
    template
        .value_cells
        .iter()
        .filter(|vc| matches!(vc.kind, ValueCellKind::Param))
        .collect()
}

/// Return the `std/tensegrity` CompiledModule from the production stdlib
/// loader. Exercises the exact same code path as production: embedded source,
/// sequential compilation with growing prelude, OnceLock caching.
///
/// Panics if the module is not found — which is the expected failure mode
/// until step-2 lands the .ri file and loader registration.
fn load_stdlib_module() -> &'static CompiledModule {
    stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/tensegrity")
        .expect("stdlib should contain std/tensegrity module")
}

// ─── step-1: module loads with zero error diagnostics ─────────────────────────

/// The std/tensegrity module must load through the production stdlib path with
/// zero error-severity diagnostics.
#[test]
fn std_tensegrity_module_loads_with_no_errors() {
    let module = load_stdlib_module();

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected error diagnostics in tensegrity.ri: {:?}",
        errors
    );
}

/// The std/tensegrity module must declare exactly six top-level structures:
/// Strut, Cable, Membrane, Tensegrity, TensegrityWire, FormFindResult.
///
/// RED (step-3): expects 6 — fails until `structure def Membrane` is added
/// to tensegrity.ri in step-4.
#[test]
fn std_tensegrity_module_has_six_structures() {
    let module = load_stdlib_module();

    let structures: Vec<&str> = module
        .templates
        .iter()
        .filter(|t| t.entity_kind == EntityKind::Structure)
        .map(|t| t.name.as_str())
        .collect();

    let expected = ["Strut", "Cable", "Membrane", "Tensegrity", "TensegrityWire", "FormFindResult"];
    assert_eq!(
        structures.len(),
        expected.len(),
        "std/tensegrity should declare exactly {} top-level structures, got: {:?}",
        expected.len(),
        structures
    );
    for name in &expected {
        assert!(
            structures.iter().any(|s| s == name),
            "std/tensegrity missing expected structure '{}'; got: {:?}",
            name,
            structures
        );
    }
}

// ─── Membrane structure ───────────────────────────────────────────────────────

/// `Membrane` has exactly 3 params:
///   - `thickness : Length`          — required, no default
///   - `material  : ElasticMaterial` — required, no default
///   - `prestress : Pressure`        — defaults to 0*1Pa (isotropic surface stress σ)
///
/// RED (step-3): fails until `structure def Membrane` is added in step-4.
#[test]
fn membrane_structure_has_thickness_material_and_prestress_default() {
    let template = find_structure("Membrane");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    assert_eq!(
        params.len(),
        3,
        "Membrane should have exactly 3 param cells (thickness, material, prestress), got: {:?}",
        names
    );

    // thickness: Length, required
    let thickness = params
        .iter()
        .find(|vc| vc.id.member == "thickness")
        .unwrap_or_else(|| panic!("Membrane missing 'thickness' param; got: {:?}", names));
    assert_eq!(
        thickness.cell_type,
        Type::Scalar { dimension: DimensionVector::LENGTH },
        "Membrane.thickness should be Length (Scalar[m]), got {:?}",
        thickness.cell_type
    );
    assert!(
        thickness.default_expr.is_none(),
        "Membrane.thickness should have no default (required param)"
    );

    // material: ElasticMaterial, required
    let material = params
        .iter()
        .find(|vc| vc.id.member == "material")
        .unwrap_or_else(|| panic!("Membrane missing 'material' param; got: {:?}", names));
    assert_eq!(
        material.cell_type,
        Type::TraitObject("ElasticMaterial".to_string()),
        "Membrane.material should be TraitObject(ElasticMaterial), got {:?}",
        material.cell_type
    );
    assert!(
        material.default_expr.is_none(),
        "Membrane.material should have no default (required param)"
    );

    // prestress: Pressure, defaults to 0*1Pa
    let prestress = params
        .iter()
        .find(|vc| vc.id.member == "prestress")
        .unwrap_or_else(|| panic!("Membrane missing 'prestress' param; got: {:?}", names));
    assert_eq!(
        prestress.cell_type,
        Type::Scalar { dimension: DimensionVector::PRESSURE },
        "Membrane.prestress should be Pressure (Scalar[Pa]), got {:?}",
        prestress.cell_type
    );
    // prestress defaults to 0*1Pa per PRD §3.
    assert!(
        prestress.default_expr.is_some(),
        "Membrane.prestress should have a default expression (= 0*1Pa per PRD §3)"
    );
}

// ─── Strut structure ─────────────────────────────────────────────────────────

/// `Strut` has exactly 2 required params: `section_area : Area` and
/// `material : ElasticMaterial`. Both are required (no defaults).
#[test]
fn strut_structure_has_required_section_area_and_material() {
    let template = find_structure("Strut");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    assert_eq!(
        params.len(),
        2,
        "Strut should have exactly 2 param cells (section_area, material), got: {:?}",
        names
    );

    let section_area = params
        .iter()
        .find(|vc| vc.id.member == "section_area")
        .unwrap_or_else(|| panic!("Strut missing 'section_area' param; got: {:?}", names));
    assert_eq!(
        section_area.cell_type,
        Type::Scalar { dimension: DimensionVector::AREA },
        "Strut.section_area should be Area (Scalar[m²]), got {:?}",
        section_area.cell_type
    );
    // Required means no default.
    assert!(
        section_area.default_expr.is_none(),
        "Strut.section_area should have no default (required param)"
    );

    let material = params
        .iter()
        .find(|vc| vc.id.member == "material")
        .unwrap_or_else(|| panic!("Strut missing 'material' param; got: {:?}", names));
    assert_eq!(
        material.cell_type,
        Type::TraitObject("ElasticMaterial".to_string()),
        "Strut.material should be TraitObject(ElasticMaterial), got {:?}",
        material.cell_type
    );
    assert!(
        material.default_expr.is_none(),
        "Strut.material should have no default (required param)"
    );
}

// ─── Cable structure ──────────────────────────────────────────────────────────

/// `Cable` has exactly 3 params: `section_area : Area`, `material : ElasticMaterial`
/// (both required, no defaults), and `pretension : Force = 0N` (defaults to 0N).
#[test]
fn cable_structure_has_required_fields_and_pretension_default() {
    let template = find_structure("Cable");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    assert_eq!(
        params.len(),
        3,
        "Cable should have exactly 3 param cells (section_area, material, pretension), got: {:?}",
        names
    );

    let section_area = params
        .iter()
        .find(|vc| vc.id.member == "section_area")
        .unwrap_or_else(|| panic!("Cable missing 'section_area' param; got: {:?}", names));
    assert_eq!(
        section_area.cell_type,
        Type::Scalar { dimension: DimensionVector::AREA },
        "Cable.section_area should be Area (Scalar[m²]), got {:?}",
        section_area.cell_type
    );
    assert!(
        section_area.default_expr.is_none(),
        "Cable.section_area should have no default (required param)"
    );

    let material = params
        .iter()
        .find(|vc| vc.id.member == "material")
        .unwrap_or_else(|| panic!("Cable missing 'material' param; got: {:?}", names));
    assert_eq!(
        material.cell_type,
        Type::TraitObject("ElasticMaterial".to_string()),
        "Cable.material should be TraitObject(ElasticMaterial), got {:?}",
        material.cell_type
    );
    assert!(
        material.default_expr.is_none(),
        "Cable.material should have no default (required param)"
    );

    let pretension = params
        .iter()
        .find(|vc| vc.id.member == "pretension")
        .unwrap_or_else(|| panic!("Cable missing 'pretension' param; got: {:?}", names));
    assert_eq!(
        pretension.cell_type,
        Type::Scalar { dimension: DimensionVector::FORCE },
        "Cable.pretension should be Force (Scalar[kg·m·s⁻²]), got {:?}",
        pretension.cell_type
    );
    // pretension defaults to 0N per PRD §3.
    assert!(
        pretension.default_expr.is_some(),
        "Cable.pretension should have a default expression (= 0N per PRD §3)"
    );
}

// ─── Tensegrity structure ─────────────────────────────────────────────────────

/// `Tensegrity` has exactly 4 required params: `nodes : List<Point3<Length>>`,
/// `struts : List<List<Int>>`, `cables : List<List<Int>>`,
/// `surfaces : List<List<Int>>`. All required (no defaults).
///
/// RED (step-1): expects 4 params — fails until `param surfaces` is added to
/// tensegrity.ri in step-2.
#[test]
fn tensegrity_structure_has_nodes_struts_cables_surfaces_params() {
    let template = find_structure("Tensegrity");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    assert_eq!(
        params.len(),
        4,
        "Tensegrity should have exactly 4 param cells (nodes, struts, cables, surfaces), got: {:?}",
        names
    );

    let length_type = Type::Scalar { dimension: DimensionVector::LENGTH };
    let point3_length = Type::Point { n: 3, quantity: Box::new(length_type) };
    let nodes = params
        .iter()
        .find(|vc| vc.id.member == "nodes")
        .unwrap_or_else(|| panic!("Tensegrity missing 'nodes' param; got: {:?}", names));
    assert_eq!(
        nodes.cell_type,
        Type::List(Box::new(point3_length)),
        "Tensegrity.nodes should be List<Point3<Length>>, got {:?}",
        nodes.cell_type
    );
    assert!(
        nodes.default_expr.is_none(),
        "Tensegrity.nodes should have no default (required param)"
    );

    let list_int = Type::List(Box::new(Type::Int));
    let list_list_int = Type::List(Box::new(list_int));

    let struts = params
        .iter()
        .find(|vc| vc.id.member == "struts")
        .unwrap_or_else(|| panic!("Tensegrity missing 'struts' param; got: {:?}", names));
    assert_eq!(
        struts.cell_type,
        list_list_int.clone(),
        "Tensegrity.struts should be List<List<Int>>, got {:?}",
        struts.cell_type
    );
    assert!(
        struts.default_expr.is_none(),
        "Tensegrity.struts should have no default (required param)"
    );

    let cables = params
        .iter()
        .find(|vc| vc.id.member == "cables")
        .unwrap_or_else(|| panic!("Tensegrity missing 'cables' param; got: {:?}", names));
    assert_eq!(
        cables.cell_type,
        list_list_int.clone(),
        "Tensegrity.cables should be List<List<Int>>, got {:?}",
        cables.cell_type
    );
    assert!(
        cables.default_expr.is_none(),
        "Tensegrity.cables should have no default (required param)"
    );

    // step-1: surfaces param — List<List<Int>>, required, no default.
    let surfaces = params
        .iter()
        .find(|vc| vc.id.member == "surfaces")
        .unwrap_or_else(|| panic!("Tensegrity missing 'surfaces' param; got: {:?}", names));
    assert_eq!(
        surfaces.cell_type,
        list_list_int,
        "Tensegrity.surfaces should be List<List<Int>>, got {:?}",
        surfaces.cell_type
    );
    assert!(
        surfaces.default_expr.is_none(),
        "Tensegrity.surfaces should have no default (required param)"
    );
}

// ─── TensegrityWire structure ─────────────────────────────────────────────────

// ─── step-1 (task-4151): form_find_free stdlib declaration ───────────────────

/// Look up `form_find_free` in the `std/tensegrity` module's `functions` vec.
///
/// Panics if not found — the expected RED failure until step-2 adds the
/// declaration to tensegrity.ri.
fn find_form_find_free_fn() -> &'static CompiledFunction {
    let module = load_stdlib_module();
    module
        .functions
        .iter()
        .find(|f| f.name == "form_find_free")
        .unwrap_or_else(|| {
            panic!(
                "fn form_find_free not found in std/tensegrity; \
                 available functions: {:?}",
                module
                    .functions
                    .iter()
                    .map(|f| f.name.as_str())
                    .collect::<Vec<_>>()
            )
        })
}

/// Pin: `fn form_find_free` must carry `@optimized("solver::form_find_free")`.
/// The @optimized → ComputeNode lowering fires only when this target is set;
/// without it the function body is inlined and no trampoline is dispatched.
#[test]
fn form_find_free_has_optimized_target() {
    let f = find_form_find_free_fn();
    assert_eq!(
        f.optimized_target,
        Some("solver::form_find_free".to_string()),
        "fn form_find_free must be annotated @optimized(\"solver::form_find_free\")"
    );
}

/// Pin: `fn form_find_free` must have exactly 4 parameters.
///
/// Expected signature:
///   (structure: Tensegrity, group_ids: List<Int>,
///    seed_ratios: List<Real>, reference_group: Int)
///
/// A param-count change here means the trampoline's `value_inputs` indexing
/// needs to be updated in lock-step with this test.
#[test]
fn form_find_free_has_four_params() {
    let f = find_form_find_free_fn();
    assert_eq!(
        f.params.len(),
        4,
        "expected 4 params (structure, group_ids, seed_ratios, reference_group), got {:?}",
        f.params.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>()
    );
}

/// Pin: `fn form_find_free` param types and names match the T1b contract.
#[test]
fn form_find_free_param_types_match_contract() {
    let f = find_form_find_free_fn();

    let expected: &[(&str, Type)] = &[
        ("structure", Type::StructureRef("Tensegrity".to_string())),
        ("group_ids", Type::List(Box::new(Type::Int))),
        ("seed_ratios", Type::List(Box::new(Type::dimensionless_scalar()))),
        ("reference_group", Type::Int),
    ];

    for (i, (exp_name, exp_type)) in expected.iter().enumerate() {
        let (got_name, got_type) = &f.params[i];
        assert_eq!(
            got_name.as_str(),
            *exp_name,
            "form_find_free params[{i}] name: expected {exp_name:?}, got {got_name:?}"
        );
        assert_eq!(
            got_type, exp_type,
            "form_find_free params[{i}] ({exp_name}) type: expected {exp_type:?}, got {got_type:?}"
        );
    }
}

/// Pin: `fn form_find_free` return type is `FormFindResult`.
#[test]
fn form_find_free_return_type_is_form_find_result() {
    let f = find_form_find_free_fn();
    assert_eq!(
        f.return_type,
        Type::StructureRef("FormFindResult".to_string()),
        "fn form_find_free must return FormFindResult, got {:?}",
        f.return_type
    );
}

// ─── TensegrityWire structure ─────────────────────────────────────────────────

/// `TensegrityWire` has 9 params: `kind : String`, `from_index : Int`,
/// `to_index : Int`, and `x1/y1/z1/x2/y2/z2 : Length`.
/// This structure is Rust-side constructed; the .ri declaration exists so
/// the CLI dump shows `TensegrityWire { ... }` rather than `{ ... }`.
#[test]
fn tensegrity_wire_structure_has_nine_params() {
    let template = find_structure("TensegrityWire");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    assert_eq!(
        params.len(),
        9,
        "TensegrityWire should have exactly 9 param cells \
         (kind, from_index, to_index, x1, y1, z1, x2, y2, z2), got: {:?}",
        names
    );

    let length = Type::Scalar { dimension: DimensionVector::LENGTH };

    let expected: &[(&str, Type)] = &[
        ("kind", Type::String),
        ("from_index", Type::Int),
        ("to_index", Type::Int),
        ("x1", length.clone()),
        ("y1", length.clone()),
        ("z1", length.clone()),
        ("x2", length.clone()),
        ("y2", length.clone()),
        ("z2", length.clone()),
    ];

    for (member, expected_ty) in expected {
        let cell = params
            .iter()
            .find(|vc| vc.id.member == *member)
            .unwrap_or_else(|| {
                panic!(
                    "TensegrityWire missing '{}' param; got: {:?}",
                    member, names
                )
            });
        assert_eq!(
            cell.cell_type, *expected_ty,
            "TensegrityWire.{} should be {:?}, got {:?}",
            member, expected_ty, cell.cell_type
        );
    }
}
