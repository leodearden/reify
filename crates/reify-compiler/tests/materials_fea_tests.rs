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

use reify_ir::*;
use reify_compiler::*;
use reify_test_support::compile_source_with_stdlib;
use reify_core::*;

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
                module
                    .trait_defs
                    .iter()
                    .map(|t| &t.name)
                    .collect::<Vec<_>>()
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
    // Compound-unit literals now parse and resolve per spec §2.7
    // (docs/prds/unit-expressions.md). `7800kg/m^3` is the canonical idiom,
    // matching examples/unit_expressions.ri:17 (`param density : Density = 7850kg/m^3`).
    let source = r#"
structure def Conformer : ElasticMaterial {
    param youngs_modulus : Pressure = 200GPa
    param poisson_ratio : Real = 0.3
    param density : Density = 7800kg/m^3
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
        conformer
            .trait_bounds
            .contains(&"ElasticMaterial".to_string()),
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

/// Reduce a `CompiledExpr` to a single SI scalar magnitude by walking the
/// expression tree. Handles the small subset of node kinds that appear in the
/// material defaults declared in `materials_fea.ri`:
///
///   - `Literal(Value::Scalar { si_value, .. })` — quantity literals like `205GPa`
///   - `Literal(Value::Real(v))`                 — bare numbers like `0.29` or `7850.0`
///   - `Literal(Value::Int(v))`                  — bare integers
///   - `BinOp { Mul | Div | Add | Sub }`         — compositional density form
///   - `OptionSome(inner)`                       — `some(310MPa)`
///
/// Anything else (function calls, struct constructors, conditionals, …) is a
/// programmer error here: the property defaults in `materials_fea.ri` are
/// pure dimensioned literals or simple `BinOp` compositions, and we
/// deliberately reject other shapes so a later refactor that smuggles in,
/// say, a `lookup_steel_youngs_modulus()` call surfaces immediately rather
/// than silently bypassing the value check.
///
/// This is compile-time numeric extraction — no engine, no `EvalContext` —
/// so it stays inside the `reify-compiler` test crate without dragging in a
/// dev-dep on `reify-eval` (which would create a `reify-compiler` ↔
/// `reify-eval` dev-dep cycle).
fn compute_si_value(expr: &CompiledExpr) -> f64 {
    match &expr.kind {
        CompiledExprKind::Literal(Value::Scalar { si_value, .. }) => *si_value,
        CompiledExprKind::Literal(Value::Real(v)) => *v,
        CompiledExprKind::Literal(Value::Int(v)) => *v as f64,
        CompiledExprKind::BinOp { op, left, right } => {
            let l = compute_si_value(left);
            let r = compute_si_value(right);
            match op {
                BinOp::Mul => l * r,
                BinOp::Div => l / r,
                BinOp::Add => l + r,
                BinOp::Sub => l - r,
                other => panic!(
                    "compute_si_value: unsupported BinOp {:?} in material default expr",
                    other
                ),
            }
        }
        CompiledExprKind::OptionSome(inner) => compute_si_value(inner),
        other => panic!(
            "compute_si_value: unsupported expression kind in material default: {:?}",
            other
        ),
    }
}

/// Assert that the named param cell on `template` carries a default
/// expression whose dimension and SI magnitude match `expected_dim` and
/// `expected_si`. Uses a 1e-6 relative tolerance — tight enough to catch the
/// `205kPa` vs `205GPa` class of typo (six orders of magnitude apart) but
/// loose enough to accommodate float round-off from compositional forms like
/// `7850.0 * 1kg / (1m * 1m * 1m)`.
fn assert_property_si_value(
    template: &TopologyTemplate,
    member: &str,
    expected_dim: DimensionVector,
    expected_si: f64,
) {
    let cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == member)
        .unwrap_or_else(|| panic!("{} missing param '{}'", template.name, member));
    let expr = cell
        .default_expr
        .as_ref()
        .unwrap_or_else(|| panic!("{}.{} missing default_expr", template.name, member));

    // Dimension is captured on the expression's typed result, regardless of
    // whether the expression is a single Literal or a BinOp tree. This is
    // the type-level half of the check: 205GPa and 205kPa both have
    // dimension PRESSURE, so this assertion does NOT distinguish them — the
    // SI value comparison below does.
    let actual_dim = match &expr.result_type {
        Type::Scalar { dimension } => *dimension,
        Type::Option(inner) => match inner.as_ref() {
            Type::Scalar { dimension } => *dimension,
            other => panic!(
                "{}.{} default_expr result_type Option<…> inner is not Scalar: {:?}",
                template.name, member, other
            ),
        },
        Type::Real => DimensionVector::DIMENSIONLESS,
        other => panic!(
            "{}.{} default_expr result_type is not Scalar/Option<Scalar>/Real: {:?}",
            template.name, member, other
        ),
    };
    assert_eq!(
        actual_dim, expected_dim,
        "{}.{} default_expr dimension should be {:?}, got {:?}",
        template.name, member, expected_dim, actual_dim
    );

    let actual_si = compute_si_value(expr);
    let tol = 1e-6 * expected_si.abs().max(1.0);
    assert!(
        (actual_si - expected_si).abs() <= tol,
        "{}.{} default_expr SI value should be {} (within {}), got {} \
         — guards against `kPa` vs `GPa` etc. unit-prefix typos",
        template.name,
        member,
        expected_si,
        tol,
        actual_si
    );
}

/// Asserts the four numeric property defaults of a concrete material
/// evaluate to the expected SI magnitudes, plus that each provenance field
/// carries a `MaterialPropertyProvenance(...)` constructor as its default
/// (verified indirectly via `cell_type` + `default_expr.is_some()` in
/// `assert_fea_material_template_shape`).
///
/// `expected_yield_pa = None` would currently be dead code — all four
/// starter materials declare `some(...)` yields — but the parameter is
/// `Option<f64>` to keep the door open for a future yield-less material
/// without forcing a helper redesign.
fn assert_fea_material_property_values(
    name: &str,
    expected_youngs_pa: f64,
    expected_poisson: f64,
    expected_density_kgm3: f64,
    expected_yield_pa: Option<f64>,
) {
    let template = find_structure(name);
    assert_property_si_value(
        template,
        "youngs_modulus",
        DimensionVector::PRESSURE,
        expected_youngs_pa,
    );
    assert_property_si_value(
        template,
        "poisson_ratio",
        DimensionVector::DIMENSIONLESS,
        expected_poisson,
    );
    assert_property_si_value(
        template,
        "density",
        DimensionVector::MASS_DENSITY,
        expected_density_kgm3,
    );
    if let Some(yield_pa) = expected_yield_pa {
        assert_property_si_value(
            template,
            "yield_stress",
            DimensionVector::PRESSURE,
            yield_pa,
        );
    }
}

/// Asserts the four-property × four-provenance shape of a concrete material
/// structure conforming to `ElasticMaterial`. Used by the per-material tests
/// (Steel_AISI_1045, Aluminium_6061_T6, Titanium_Ti6Al4V, ABS_Plastic) to keep
/// the eight-value-cell + trait-bound + constraint-injection check uniform.
///
/// This helper covers structural shape only (cell names, types, default
/// presence, trait bound, constraint count). Numeric SI values for each
/// property are asserted by `assert_fea_material_property_values`, called
/// alongside this helper in each per-material test.
fn assert_fea_material_template_shape(name: &str) {
    let template = find_structure(name);

    assert!(
        template
            .trait_bounds
            .contains(&"ElasticMaterial".to_string()),
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
    // Poisson-ratio constraints declared on `ElasticMaterial` must appear
    // here. Pinning to exactly 2 (rather than `>= 2`) catches the case of a
    // structure-local constraint being added without an explicit test
    // update; the four starter materials in `materials_fea.ri` deliberately
    // declare zero structure-local constraints, so the trait-injected pair
    // is the entire set.
    assert_eq!(
        template.constraints.len(),
        2,
        "{} should inherit exactly 2 constraints from ElasticMaterial \
         (poisson_ratio >= 0 and poisson_ratio < 0.5) and declare no \
         structure-local constraints, got {} constraints",
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
    // matweb-equivalent SI values: 205 GPa, 0.29, 7850 kg/m³, 310 MPa.
    // The SI check guards against `kPa` vs `GPa` etc. unit-prefix typos
    // that the shape check (which only verifies dimension == PRESSURE)
    // cannot detect.
    assert_fea_material_property_values("Steel_AISI_1045", 205.0e9, 0.29, 7850.0, Some(310.0e6));
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
    // matweb-equivalent SI values: 68.9 GPa, 0.33, 2700 kg/m³, 276 MPa.
    assert_fea_material_property_values("Aluminium_6061_T6", 68.9e9, 0.33, 2700.0, Some(276.0e6));
}

// ─── step-13: Titanium_Ti6Al4V starter material ──────────────────────────────

/// `Titanium_Ti6Al4V` is the most widely used titanium alloy (Grade 5,
/// alpha-beta), prized in aerospace and biomedical applications for its
/// strength-to-weight ratio and corrosion resistance. Properties below are
/// for the annealed condition. Asserts the same eight-cell shape as the
/// other starter materials via the shared helper.
///
/// PRD task #1 cites public matweb-equivalent values:
///   youngs_modulus = 113.8 GPa, poisson_ratio = 0.342,
///   density = 4430 kg/m³, yield_stress = some(880 MPa).
#[test]
fn titanium_ti6al4v_structure_conforms_with_correct_property_values_and_provenance() {
    assert_fea_material_template_shape("Titanium_Ti6Al4V");
    // matweb / ASM Handbook SI values: 113.8 GPa, 0.342, 4430 kg/m³, 880 MPa.
    assert_fea_material_property_values("Titanium_Ti6Al4V", 113.8e9, 0.342, 4430.0, Some(880.0e6));
}

// ─── step-15: ABS_Plastic starter material ───────────────────────────────────

/// `ABS_Plastic` is the general-purpose acrylonitrile-butadiene-styrene
/// thermoplastic widely used in injection-moulded consumer parts and FDM
/// 3D printing. Properties below are room-temperature values for moulded
/// general-purpose ABS; yield is approximate due to the polymer's
/// ductile-to-brittle behaviour at higher strain rates / lower
/// temperatures. Asserts the same eight-cell shape as the other starter
/// materials via the shared helper.
///
/// PRD task #1 cites public matweb-equivalent values:
///   youngs_modulus = 2.3 GPa, poisson_ratio = 0.35,
///   density = 1050 kg/m³, yield_stress = some(40 MPa).
#[test]
fn abs_plastic_structure_conforms_with_correct_property_values_and_provenance() {
    assert_fea_material_template_shape("ABS_Plastic");
    // matweb SI values: 2.3 GPa, 0.35, 1050 kg/m³, ~40 MPa (approximate
    // due to ABS's strain-rate-dependent ductile-to-brittle transition).
    assert_fea_material_property_values("ABS_Plastic", 2.3e9, 0.35, 1050.0, Some(40.0e6));
}

// ─── step-17: module summary regression test ─────────────────────────────────

/// Final regression covering the std/materials/fea module's overall shape.
/// At this point the previous tests already check each entity in detail; this
/// test exists to lock in the module's *cardinality* — exactly one trait,
/// exactly five top-level structures (one provenance record + four materials),
/// zero error diagnostics, every material carries the `ElasticMaterial` trait
/// bound. Adding or removing a top-level entity from `materials_fea.ri` will
/// fail this test, which is the intended behaviour: any future expansion should
/// be expressed as a deliberate update here, not silently introduced.
#[test]
fn std_materials_fea_module_summary_has_one_trait_one_provenance_struct_and_four_materials() {
    let module = load_stdlib_module();

    // Zero error diagnostics is also asserted in step-1; repeat here so this
    // single test fails loudly on any regression rather than silently relying
    // on the earlier check.
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "std/materials/fea should have zero error diagnostics, got: {:?}",
        errors
    );

    // Exactly one trait — `ElasticMaterial`.
    let trait_names: Vec<&str> = module.trait_defs.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(
        module.trait_defs.len(),
        1,
        "std/materials/fea should declare exactly 1 trait, got: {:?}",
        trait_names
    );
    assert!(
        module
            .trait_defs
            .iter()
            .any(|t| t.name == "ElasticMaterial"),
        "std/materials/fea should contain the 'ElasticMaterial' trait, got: {:?}",
        trait_names
    );

    // Exactly five top-level structures — one provenance record + four
    // starter materials. Filter on `EntityKind::Structure` so future
    // non-structure additions to the same module (enums, traits, ...)
    // don't perturb this assertion.
    let structures: Vec<&str> = module
        .templates
        .iter()
        .filter(|t| t.entity_kind == EntityKind::Structure)
        .map(|t| t.name.as_str())
        .collect();
    let expected_structures = [
        "MaterialPropertyProvenance",
        "Steel_AISI_1045",
        "Aluminium_6061_T6",
        "Titanium_Ti6Al4V",
        "ABS_Plastic",
    ];
    assert_eq!(
        structures.len(),
        expected_structures.len(),
        "std/materials/fea should declare exactly {} top-level structures, got: {:?}",
        expected_structures.len(),
        structures
    );
    for expected in &expected_structures {
        assert!(
            structures.iter().any(|s| s == expected),
            "std/materials/fea missing expected structure '{}'; got: {:?}",
            expected,
            structures
        );
    }

    // Every starter material must carry the `ElasticMaterial` trait bound.
    // `MaterialPropertyProvenance` is intentionally excluded — it is a plain
    // citation record with no trait bound.
    let material_names = [
        "Steel_AISI_1045",
        "Aluminium_6061_T6",
        "Titanium_Ti6Al4V",
        "ABS_Plastic",
    ];
    for material in &material_names {
        let template = find_structure(material);
        assert!(
            template
                .trait_bounds
                .contains(&"ElasticMaterial".to_string()),
            "{} should carry 'ElasticMaterial' trait bound, got: {:?}",
            material,
            template.trait_bounds
        );
    }
}
