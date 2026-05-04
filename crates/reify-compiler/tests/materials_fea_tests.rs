//! Tests for stdlib/materials_fea.ri — FEA-bound elastic-material trait + four
//! starter material instances (Steel_AISI_1045, Aluminium_6061_T6,
//! Titanium_Ti6Al4V, ABS_Plastic).
//!
//! Tests validate that the .ri file is loaded by the production stdlib path,
//! that `MaterialPropertyProvenance`, `ElasticMaterial`, and the four concrete
//! material structures are correctly represented in the compiled module, and
//! that trait conformance, constraint injection, and end-to-end value
//! evaluation through dimensioned defaults all work as expected.
//!
//! All tests use the production-path `load_stdlib_module()` helper that
//! exercises the same embedded + sequential-prelude compilation path as
//! production (not a standalone `.ri` file re-read). This mirrors the pattern
//! in `materials_thermal_tests.rs` and `materials_electrical_tests.rs`.

use reify_compiler::*;
use reify_test_support::compile_source_with_stdlib;
use reify_types::*;

/// Look up a structure template by name within the `std/materials/fea` module.
///
/// All four starter materials (`Steel_AISI_1045`, `Aluminium_6061_T6`,
/// `Titanium_Ti6Al4V`, `ABS_Plastic`) plus `MaterialPropertyProvenance`
/// are top-level structures, so we go through `module.templates` and filter on
/// `EntityKind::Structure` to keep the assertion stable against future
/// non-structure additions to the same module.
fn find_structure(name: &str) -> &'static TopologyTemplate {
    let module = load_stdlib_module();
    module
        .templates
        .iter()
        .find(|t| t.name == name && t.entity_kind == EntityKind::Structure)
        .unwrap_or_else(|| {
            panic!(
                "expected `structure def {}` template in std/materials/fea, got templates: {:?}",
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

// ─── helpers ──────────────────────────────────────────────────────────────────

/// Return the `std/materials/fea` CompiledModule from the production stdlib
/// loader. Exercises the exact same code path as production: embedded source,
/// sequential compilation with growing prelude, OnceLock caching.
///
/// Panics if the module is not found — which is the expected failure mode
/// until step-2 lands the .ri file and loader registration.
fn load_stdlib_module() -> &'static CompiledModule {
    stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/materials/fea")
        .expect("stdlib should contain std/materials/fea module")
}

// ─── step-1: module loads with zero error diagnostics ────────────────────────

/// The std/materials/fea module must load through the production stdlib path
/// with zero error-severity diagnostics. The loader-level `assert!` already
/// fails fast on Error diagnostics during init, but this test independently
/// asserts the post-init invariant so a regression is caught at the test
/// boundary rather than at first stdlib touch.
#[test]
fn std_materials_fea_module_loads_with_no_errors() {
    let module = load_stdlib_module();

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected error diagnostics in materials_fea.ri: {:?}",
        errors
    );
}

// ─── step-3: MaterialPropertyProvenance structure ────────────────────────────

/// `MaterialPropertyProvenance` is the citation record carried alongside each
/// property of a concrete material. It must exist as a top-level structure in
/// the compiled `std/materials/fea` module with exactly three required `param`
/// slots — `source`, `reference`, `notes` — each typed `String`.
///
/// The three-slot shape is the foundation of the per-property-provenance
/// design (see Plan §"Architecture chosen"): each material gets four parallel
/// `..._provenance : MaterialPropertyProvenance` fields, one per property,
/// rather than a single Map keyed by property name. This test locks in the
/// citation record's shape before any material structure refers to it.
#[test]
fn material_property_provenance_struct_has_three_string_fields() {
    let template = find_structure("MaterialPropertyProvenance");

    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();
    assert_eq!(
        params.len(),
        3,
        "MaterialPropertyProvenance should have exactly 3 param cells, got: {:?}",
        names
    );

    for expected in &["source", "reference", "notes"] {
        let cell = params
            .iter()
            .find(|vc| vc.id.member == *expected)
            .unwrap_or_else(|| {
                panic!(
                    "MaterialPropertyProvenance missing '{}' param; got: {:?}",
                    expected, names
                )
            });
        assert_eq!(
            cell.cell_type,
            Type::String,
            "MaterialPropertyProvenance.{} should be String, got {:?}",
            expected,
            cell.cell_type
        );
    }
}

// ─── step-5: ElasticMaterial trait ───────────────────────────────────────────

/// `ElasticMaterial` is the dimensioned FEA-bound material trait that the v0.3
/// solver consumes. It declares exactly four required members:
///
///   - `youngs_modulus : Pressure`            (kg·m⁻¹·s⁻²)
///   - `poisson_ratio  : Real`                 (dimensionless, [0, 0.5))
///   - `density        : Density`              (kg·m⁻³)
///   - `yield_stress   : Option<Pressure>`     (some(Pa) | none)
///
/// The trait is *new* and parallel to the existing `Elastic` trait in
/// `materials_mechanical.ri`; the latter uses `Real` placeholders and bundles
/// `shear_modulus`, neither of which fits the FEA solver's input shape. See
/// the file-level header comment in `materials_fea.ri` for the rationale.
#[test]
fn elastic_material_trait_has_four_dimensioned_members() {
    let module = load_stdlib_module();

    let elastic_material = module
        .trait_defs
        .iter()
        .find(|t| t.name == "ElasticMaterial")
        .unwrap_or_else(|| {
            panic!(
                "expected 'ElasticMaterial' trait in std/materials/fea, got traits: {:?}",
                module.trait_defs.iter().map(|t| &t.name).collect::<Vec<_>>()
            )
        });

    assert_eq!(
        elastic_material.required_members.len(),
        4,
        "ElasticMaterial should have exactly 4 required members, got: {:?}",
        elastic_material
            .required_members
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );

    // Each (name, expected type) tuple is asserted against the
    // RequirementKind::Param payload type.  Using a literal tuple list keeps
    // the test focused on the dimensioned-trait shape rather than mirroring
    // implementation order.
    let expected_members: &[(&str, Type)] = &[
        (
            "youngs_modulus",
            Type::Scalar {
                dimension: DimensionVector::PRESSURE,
            },
        ),
        ("poisson_ratio", Type::Real),
        (
            "density",
            Type::Scalar {
                dimension: DimensionVector::MASS_DENSITY,
            },
        ),
        (
            "yield_stress",
            Type::Option(Box::new(Type::Scalar {
                dimension: DimensionVector::PRESSURE,
            })),
        ),
    ];

    for (name, expected_ty) in expected_members {
        let req = elastic_material
            .required_members
            .iter()
            .find(|r| r.name == *name)
            .unwrap_or_else(|| {
                panic!(
                    "ElasticMaterial missing required member '{}'; got: {:?}",
                    name,
                    elastic_material
                        .required_members
                        .iter()
                        .map(|r| &r.name)
                        .collect::<Vec<_>>()
                )
            });
        match &req.kind {
            RequirementKind::Param(ty) => assert_eq!(
                ty, expected_ty,
                "ElasticMaterial.{} should be {:?}, got {:?}",
                name, expected_ty, ty
            ),
            other => panic!(
                "ElasticMaterial.{} should be a Param requirement, got {:?}",
                name, other
            ),
        }
    }
}

// ─── step-7: Poisson-ratio constraints injected from trait ────────────────────

/// `ElasticMaterial` constrains `poisson_ratio` to the half-open interval
/// `[0, 0.5)` via two trait-level `constraint` declarations:
///
///   constraint poisson_ratio >= 0
///   constraint poisson_ratio < 0.5
///
/// Trait-level constraints are propagated into every conforming structure by
/// the compiler's constraint-injection pass (see also
/// `materials_mechanical_tests.rs::strong_constraint_injected_into_steel`,
/// the precedent this test mirrors). When a structure declares
/// `: ElasticMaterial`, both Poisson constraints land in `template.constraints`
/// regardless of whether the default values would satisfy them.
///
/// This test compiles a minimal conforming structure with in-range defaults
/// and asserts the conformer template's `constraints` collection contains at
/// least two entries — the two Poisson constraints from the trait.
///
/// The compile-time injection assertion is the canonical RED→GREEN signal for
/// the constraint-injection wiring. Runtime constraint-violation semantics
/// (Satisfaction::Violated when poisson_ratio = 0.7 or -0.1) are exercised in
/// reify-eval/tests/constraint_def_eval.rs and reify-eval/tests/conformance_runtime.rs
/// against general engine behavior; we do not duplicate those checks here
/// because (a) the engine helpers `make_simple_engine` /
/// `check_source_with_stdlib` are gated behind the `eval-helpers` feature,
/// which is intentionally NOT enabled in `reify-compiler` dev-deps to avoid a
/// `reify-compiler` ↔ `reify-eval` dev-dep cycle, and (b) the existing
/// per-trait pattern in `materials_mechanical_tests.rs` checks only
/// compile-time injection, not runtime violation semantics.
#[test]
fn elastic_material_trait_constrains_poisson_ratio_to_half_open_unit() {
    // Note on density literal form: the spec lists `7800kg/m^3` as a Density
    // literal (§2.7), but the tree-sitter grammar's `quantity_literal` is
    // `number + identifier` only — compound `kg/m^3` is not a single token.
    // The working idiom (per `examples/dimensional_chains.ri:84`) is the
    // compositional form `7800.0 * 1kg / (1m * 1m * 1m)` which produces the
    // same dimensioned value (7800 kg·m⁻³).
    let source = r#"
structure def Conformer : ElasticMaterial {
    param youngs_modulus : Pressure = 200GPa
    param poisson_ratio : Real = 0.3
    param density : Density = 7800.0 * 1kg / (1m * 1m * 1m)
    param yield_stress : Option<Pressure> = some(250MPa)
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
        "Conformer should compile cleanly with in-range Poisson defaults, got: {:?}",
        errors
    );

    let conformer = compiled
        .templates
        .iter()
        .find(|t| t.name == "Conformer")
        .expect("expected Conformer template in compiled module");

    assert!(
        conformer.trait_bounds.contains(&"ElasticMaterial".to_string()),
        "Conformer should carry 'ElasticMaterial' trait bound, got: {:?}",
        conformer.trait_bounds
    );

    assert!(
        conformer.constraints.len() >= 2,
        "Conformer should inherit at least 2 constraints from ElasticMaterial \
         (poisson_ratio >= 0 and poisson_ratio < 0.5), got {} constraints",
        conformer.constraints.len()
    );
}

// ─── step-9: Steel_AISI_1045 starter material ────────────────────────────────

/// Asserts the four-property × four-provenance shape of a concrete material
/// structure conforming to `ElasticMaterial`. Used by the per-material tests
/// (Steel_AISI_1045, Aluminium_6061_T6, Titanium_Ti6Al4V, ABS_Plastic) to keep
/// the eight-value-cell + trait-bound + constraint-injection check uniform.
///
/// Per the file-level note on engine evaluation: this helper is **compile-time
/// only**. It does not evaluate default expressions to confirm SI numeric
/// values (e.g. `youngs_modulus ≈ 2.05e11 Pa` for steel) because the
/// `make_simple_engine` / `engine.eval` helpers live behind the `eval-helpers`
/// feature, and adding `reify-eval` as a `reify-compiler` dev-dep would create
/// a `reify-compiler` ↔ `reify-eval` cycle (see step-7 commentary).
/// The compile-time success of the dimensioned literal `205GPa` etc. already
/// proves the parse + type-check + dimension-tag path; runtime numeric
/// equality is exercised by separate engine-level tests in `reify-eval`.
fn assert_fea_material_template_shape(name: &str) {
    let template = find_structure(name);

    assert!(
        template.trait_bounds.contains(&"ElasticMaterial".to_string()),
        "{} should carry 'ElasticMaterial' trait bound, got: {:?}",
        name,
        template.trait_bounds
    );

    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();
    assert_eq!(
        params.len(),
        8,
        "{} should have exactly 8 param cells (4 ElasticMaterial members + 4 \
         per-property provenance), got: {:?}",
        name,
        names
    );

    // Each (member name, expected cell type) tuple. Provenance cells are typed
    // as `Type::StructureRef("MaterialPropertyProvenance")` per the structure-
    // name resolver in type_resolution.rs:658-660.
    let provenance_ty = Type::StructureRef("MaterialPropertyProvenance".to_string());
    let expected: &[(&str, Type)] = &[
        (
            "youngs_modulus",
            Type::Scalar {
                dimension: DimensionVector::PRESSURE,
            },
        ),
        ("poisson_ratio", Type::Real),
        (
            "density",
            Type::Scalar {
                dimension: DimensionVector::MASS_DENSITY,
            },
        ),
        (
            "yield_stress",
            Type::Option(Box::new(Type::Scalar {
                dimension: DimensionVector::PRESSURE,
            })),
        ),
        ("youngs_modulus_provenance", provenance_ty.clone()),
        ("poisson_ratio_provenance", provenance_ty.clone()),
        ("density_provenance", provenance_ty.clone()),
        ("yield_stress_provenance", provenance_ty),
    ];

    for (member, expected_ty) in expected {
        let cell = params
            .iter()
            .find(|vc| vc.id.member == *member)
            .unwrap_or_else(|| {
                panic!(
                    "{} missing required param '{}'; got: {:?}",
                    name, member, names
                )
            });
        assert_eq!(
            cell.cell_type, *expected_ty,
            "{}.{} should be {:?}, got {:?}",
            name, member, expected_ty, cell.cell_type
        );
        assert!(
            cell.default_expr.is_some(),
            "{}.{} must carry a default expression so a bare `{}()` instantiation \
             populates every cell; got default_expr: None",
            name,
            member,
            name
        );
    }

    // Trait constraints inject into every conforming structure, so the two
    // Poisson-ratio constraints declared on `ElasticMaterial` must appear here
    // alongside any structure-local ones.
    assert!(
        template.constraints.len() >= 2,
        "{} should inherit at least 2 constraints from ElasticMaterial \
         (poisson_ratio >= 0 and poisson_ratio < 0.5), got {} constraints",
        name,
        template.constraints.len()
    );
}

/// `Steel_AISI_1045` is the medium-carbon hot-rolled-steel starter material.
/// Asserts the structure's full shape: the eight expected value cells (four
/// `ElasticMaterial` parameters + four per-property `MaterialPropertyProvenance`
/// fields), the `ElasticMaterial` trait bound, that each cell carries a default
/// expression, and that the two Poisson-ratio constraints inject in.
///
/// PRD task #1 cites public matweb-equivalent values:
///   youngs_modulus = 205 GPa, poisson_ratio = 0.29,
///   density = 7850 kg/m³, yield_stress = some(310 MPa).
#[test]
fn steel_aisi_1045_structure_conforms_with_correct_property_values_and_provenance() {
    assert_fea_material_template_shape("Steel_AISI_1045");
}

// ─── step-11: Aluminium_6061_T6 starter material ─────────────────────────────

/// `Aluminium_6061_T6` is the precipitation-hardened aerospace-grade aluminium
/// starter material (T6 = solution-heat-treated + artificially aged).
/// Asserts the same eight-cell shape as Steel_AISI_1045 via the shared helper.
///
/// PRD task #1 cites public matweb-equivalent values:
///   youngs_modulus = 68.9 GPa, poisson_ratio = 0.33,
///   density = 2700 kg/m³, yield_stress = some(276 MPa).
#[test]
fn aluminium_6061_t6_structure_conforms_with_correct_property_values_and_provenance() {
    assert_fea_material_template_shape("Aluminium_6061_T6");
}
