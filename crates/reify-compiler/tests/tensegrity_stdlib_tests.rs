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

/// The std/tensegrity module must declare exactly seven top-level structures:
/// Strut, Cable, Membrane, Tensegrity, TensegrityWire, TensegritySurface,
/// FormFindResult.
///
/// RED (step-5): expects 7 — fails until `structure def TensegritySurface` is
/// added to tensegrity.ri in step-6.
#[test]
fn std_tensegrity_module_has_seven_structures() {
    let module = load_stdlib_module();

    let structures: Vec<&str> = module
        .templates
        .iter()
        .filter(|t| t.entity_kind == EntityKind::Structure)
        .map(|t| t.name.as_str())
        .collect();

    let expected = [
        "Strut", "Cable", "Membrane", "Tensegrity", "TensegrityWire",
        "TensegritySurface", "FormFindResult",
    ];
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

/// Pin: `fn form_find_free` must have exactly 5 parameters.
///
/// Expected signature:
///   (structure: Tensegrity, group_ids: List<Int>,
///    seed_ratios: List<Real>, reference_group: Int,
///    surface_stresses: List<Real> = [])
///
/// A param-count change here means the trampoline's `value_inputs` indexing
/// needs to be updated in lock-step with this test.
#[test]
fn form_find_free_has_five_params() {
    let f = find_form_find_free_fn();
    assert_eq!(
        f.params.len(),
        5,
        "expected 5 params (structure, group_ids, seed_ratios, reference_group, surface_stresses), got {:?}",
        f.params.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>()
    );
}

/// Pin: `fn form_find_free` param types and names match the δ contract (5 params).
#[test]
fn form_find_free_param_types_match_contract() {
    let f = find_form_find_free_fn();

    let expected: &[(&str, Type)] = &[
        ("structure", Type::StructureRef("Tensegrity".to_string())),
        ("group_ids", Type::List(Box::new(Type::Int))),
        ("seed_ratios", Type::List(Box::new(Type::dimensionless_scalar()))),
        ("reference_group", Type::Int),
        ("surface_stresses", Type::List(Box::new(Type::dimensionless_scalar()))),
    ];

    assert_eq!(
        f.params.len(),
        expected.len(),
        "form_find_free arity changed: expected {} params, got {:?}",
        expected.len(),
        f.params.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>()
    );

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

// ─── step-7 (task-4414): form_find (anchored) γ surface declaration ───────────

/// Look up `form_find` in the `std/tensegrity` module's `functions` vec.
///
/// Panics if not found — used by the γ surface-extension pins below.
fn find_form_find_fn() -> &'static CompiledFunction {
    let module = load_stdlib_module();
    module
        .functions
        .iter()
        .find(|f| f.name == "form_find")
        .unwrap_or_else(|| {
            panic!(
                "fn form_find not found in std/tensegrity; \
                 available functions: {:?}",
                module
                    .functions
                    .iter()
                    .map(|f| f.name.as_str())
                    .collect::<Vec<_>>()
            )
        })
}

/// Pin: `fn form_find` must keep `@optimized("solver::form_find")`. The γ
/// surface extension is additive — the @optimized target (hence the
/// ComputeNode lowering + trampoline dispatch) is unchanged.
#[test]
fn form_find_has_optimized_target() {
    let f = find_form_find_fn();
    assert_eq!(
        f.optimized_target,
        Some("solver::form_find".to_string()),
        "fn form_find must be annotated @optimized(\"solver::form_find\")"
    );
}

/// Pin: `fn form_find` must have exactly 4 parameters after the γ extension.
///
/// Expected signature:
///   (structure: Tensegrity, force_densities: List<Real>,
///    anchors: List<Int>, surface_stresses: List<Real> = [])
///
/// A param-count change here means the trampoline's `value_inputs` indexing
/// must be updated in lock-step with this test.
///
/// RED (step-7): form_find currently has 3 params — fails until step-8 adds
/// `surface_stresses : List<Real> = []`.
#[test]
fn form_find_has_four_params() {
    let f = find_form_find_fn();
    assert_eq!(
        f.params.len(),
        4,
        "expected 4 params (structure, force_densities, anchors, surface_stresses), got {:?}",
        f.params.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>()
    );
}

/// Pin: `fn form_find` param names and types match the γ surface contract.
///
/// RED (step-7): the 4th param does not exist yet.
#[test]
fn form_find_param_types_match_contract() {
    let f = find_form_find_fn();

    let expected: &[(&str, Type)] = &[
        ("structure", Type::StructureRef("Tensegrity".to_string())),
        ("force_densities", Type::List(Box::new(Type::dimensionless_scalar()))),
        ("anchors", Type::List(Box::new(Type::Int))),
        ("surface_stresses", Type::List(Box::new(Type::dimensionless_scalar()))),
    ];

    assert_eq!(
        f.params.len(),
        expected.len(),
        "form_find arity changed: expected {} params, got {:?}",
        expected.len(),
        f.params.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>()
    );

    for (i, (exp_name, exp_type)) in expected.iter().enumerate() {
        let (got_name, got_type) = &f.params[i];
        assert_eq!(
            got_name.as_str(),
            *exp_name,
            "form_find params[{i}] name: expected {exp_name:?}, got {got_name:?}"
        );
        assert_eq!(
            got_type, exp_type,
            "form_find params[{i}] ({exp_name}) type: expected {exp_type:?}, got {got_type:?}"
        );
    }
}

/// Pin: the new 4th param `surface_stresses` carries a default expression
/// (`= []`) so the existing 3-arg callers (examples/tensegrity_cable_net.ri and
/// the landed direct-3-input trampoline unit tests) keep working via
/// try_default_padding. The first three params remain required (no default).
///
/// `param_defaults` is parallel to `params` (strict length invariant), so it is
/// indexed positionally.
///
/// RED (step-7): form_find has 3 params and no defaults — fails until step-8.
#[test]
fn form_find_surface_stresses_param_has_default() {
    let f = find_form_find_fn();
    assert_eq!(
        f.param_defaults.len(),
        f.params.len(),
        "param_defaults must be parallel to params (length invariant)"
    );
    assert_eq!(
        f.params.len(),
        4,
        "expected 4 params before checking defaults; got {:?}",
        f.params.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>()
    );

    // First three params are required — no default expression.
    for i in 0..3 {
        assert!(
            f.param_defaults[i].is_none(),
            "form_find params[{i}] ({}) should have NO default",
            f.params[i].0
        );
    }
    // surface_stresses (4th) defaults to [].
    assert!(
        f.param_defaults[3].is_some(),
        "form_find.surface_stresses (params[3]) must have a default expression (= [])"
    );
}

/// Pin: `fn form_find` return type is `FormFindResult` (unchanged by γ).
#[test]
fn form_find_return_type_is_form_find_result() {
    let f = find_form_find_fn();
    assert_eq!(
        f.return_type,
        Type::StructureRef("FormFindResult".to_string()),
        "fn form_find must return FormFindResult, got {:?}",
        f.return_type
    );
}

/// Pin: `structure def FormFindResult` gains a `surface_stresses : List<Real>`
/// field (per-triangle solved σ echo), bringing it to 5 fields:
/// nodes, member_forces, force_densities, converged, surface_stresses.
///
/// Like the other four fields it is required (no default) — a FormFindResult is
/// only ever constructed by the solver trampoline.
///
/// RED (step-7): FormFindResult has 4 fields — fails until step-8 adds the field.
#[test]
fn form_find_result_structure_has_surface_stresses_field() {
    let template = find_structure("FormFindResult");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    assert_eq!(
        params.len(),
        5,
        "FormFindResult should have exactly 5 param cells \
         (nodes, member_forces, force_densities, converged, surface_stresses), got: {:?}",
        names
    );

    // The four pre-existing fields must still be present.
    for field in ["nodes", "member_forces", "force_densities", "converged"] {
        assert!(
            names.contains(&field),
            "FormFindResult missing pre-existing field '{}'; got: {:?}",
            field, names
        );
    }

    let surface_stresses = params
        .iter()
        .find(|vc| vc.id.member == "surface_stresses")
        .unwrap_or_else(|| {
            panic!(
                "FormFindResult missing 'surface_stresses' field; got: {:?}",
                names
            )
        });
    assert_eq!(
        surface_stresses.cell_type,
        Type::List(Box::new(Type::dimensionless_scalar())),
        "FormFindResult.surface_stresses should be List<Real>, got {:?}",
        surface_stresses.cell_type
    );
    // Required field (no default) — mirrors the other four FormFindResult fields.
    assert!(
        surface_stresses.default_expr.is_none(),
        "FormFindResult.surface_stresses should have no default (solver-constructed only)"
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

// ─── TensegritySurface structure ──────────────────────────────────────────────

/// `TensegritySurface` has 13 params, all required (no defaults):
///   kind : String
///   i0, i1, i2 : Int   (0-based node indices)
///   x0,y0,z0 : Length  (corner 0 coordinates)
///   x1,y1,z1 : Length  (corner 1 coordinates)
///   x2,y2,z2 : Length  (corner 2 coordinates)
///
/// This structure is Rust-side constructed by `tensegrity_surfaces`; the .ri
/// declaration exists so the CLI dump shows `TensegritySurface { ... }`.
///
/// RED (step-5): fails until `structure def TensegritySurface` is added in step-6.
#[test]
fn tensegrity_surface_structure_has_thirteen_params() {
    let template = find_structure("TensegritySurface");
    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    assert_eq!(
        params.len(),
        13,
        "TensegritySurface should have exactly 13 param cells \
         (kind, i0, i1, i2, x0, y0, z0, x1, y1, z1, x2, y2, z2), got: {:?}",
        names
    );

    let length = Type::Scalar { dimension: DimensionVector::LENGTH };

    let expected: &[(&str, Type)] = &[
        ("kind", Type::String),
        ("i0",   Type::Int),
        ("i1",   Type::Int),
        ("i2",   Type::Int),
        ("x0",   length.clone()),
        ("y0",   length.clone()),
        ("z0",   length.clone()),
        ("x1",   length.clone()),
        ("y1",   length.clone()),
        ("z1",   length.clone()),
        ("x2",   length.clone()),
        ("y2",   length.clone()),
        ("z2",   length.clone()),
    ];

    for (member, expected_ty) in expected {
        let cell = params
            .iter()
            .find(|vc| vc.id.member == *member)
            .unwrap_or_else(|| {
                panic!(
                    "TensegritySurface missing '{}' param; got: {:?}",
                    member, names
                )
            });
        assert_eq!(
            cell.cell_type, *expected_ty,
            "TensegritySurface.{} should be {:?}, got {:?}",
            member, expected_ty, cell.cell_type
        );
        // All params must be required (no default).
        assert!(
            cell.default_expr.is_none(),
            "TensegritySurface.{} should have no default (required param, never user-constructed)",
            member
        );
    }
}
