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
use reify_core::*;
use reify_ir::*;
use reify_test_support::compile_source_with_stdlib;

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
        ("poisson_ratio", Type::dimensionless_scalar()),
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

// ─── task α (#6877): Damped mixin + DampedMaterial named intersection ─────────

/// `Damped` is the free-standing hysteretic-loss mixin the damped-modal solve
/// consumes, and `DampedMaterial` is the ergonomic named intersection
/// `ElasticMaterial + Damped` (PRD `docs/prds/v0_6/damped-modal-bonded-heterogeneous.md`
/// §Contract C1, §Resolved decisions 1-2).
///
/// Shape pinned here:
///
///   - `Damped` has NO refinements. It is deliberately *not* a `ConstitutiveLaw`
///     refinement so it composes with future non-isotropic damped laws
///     (orthotropic / transverse-isotropic) rather than being welded to the
///     isotropic family (PRD decision 1).
///   - `Damped` declares exactly one required member, `loss_factor : Real`
///     (dimensionless η — ratio of dissipated to stored energy per cycle).
///   - `Damped` carries exactly one trait-level constraint (`loss_factor >= 0`).
///     The normative η ≥ 0 half of PRD C1 is pinned in a *constraint*, not a
///     comment, mirroring `ElasticMaterial`'s two Poisson bounds.
///   - `DampedMaterial` has an EMPTY body — `CompiledTrait.required_members` is
///     own-members-only (proven by `Damping : MaterialSpec` asserting exactly 2
///     at `materials_mechanical_tests.rs:626` while `MaterialSpec` contributes
///     more), so the four `ElasticMaterial` members arrive transitively via
///     `collect_all_requirements` (`trait_requirements.rs:152`) rather than
///     being restated here.
///   - `DampedMaterial.refinements == ["ElasticMaterial", "Damped"]` in
///     declaration order, pinning the named-intersection shape. Multi-parent
///     precedent: `trait Watertight : Closed + Manifold {}`
///     (`stdlib/geometry_traits.ri:84`).
#[test]
fn damped_mixin_and_damped_material_named_intersection_shape() {
    let module = load_stdlib_module();

    let trait_names = || {
        module
            .trait_defs
            .iter()
            .map(|t| &t.name)
            .collect::<Vec<_>>()
    };

    // ── Damped: free-standing single-member mixin ────────────────────────────
    let damped = module
        .trait_defs
        .iter()
        .find(|t| t.name == "Damped")
        .unwrap_or_else(|| {
            panic!(
                "expected 'Damped' trait in std/materials/fea, got traits: {:?}",
                trait_names()
            )
        });

    assert!(
        damped.refinements.is_empty(),
        "Damped must be a free-standing mixin with NO parent traits (PRD decision 1 \
         — it must compose with future non-isotropic damped laws, so it is \
         deliberately not a ConstitutiveLaw refinement), got refinements: {:?}",
        damped.refinements
    );

    assert_eq!(
        damped.required_members.len(),
        1,
        "Damped should declare exactly 1 required member (loss_factor), got: {:?}",
        damped
            .required_members
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );

    // Mirrors the `expected_members` tuple-table idiom used for
    // `ElasticMaterial` above.
    let expected_members: &[(&str, Type)] =
        &[("loss_factor", Type::dimensionless_scalar())];
    for (name, expected_ty) in expected_members {
        let req = damped
            .required_members
            .iter()
            .find(|r| r.name == *name)
            .unwrap_or_else(|| {
                panic!(
                    "Damped missing required member '{}'; got: {:?}",
                    name,
                    damped
                        .required_members
                        .iter()
                        .map(|r| &r.name)
                        .collect::<Vec<_>>()
                )
            });
        match &req.kind {
            RequirementKind::Param(ty) => assert_eq!(
                ty, expected_ty,
                "Damped.{} should be {:?}, got {:?}",
                name, expected_ty, ty
            ),
            other => panic!(
                "Damped.{} should be a Param requirement, got {:?}",
                name, other
            ),
        }
    }

    // The η ≥ 0 half of PRD C1 lives in a trait-level constraint, which the
    // compiler carries as a `DefaultKind::Constraint` entry in `defaults`
    // and injects into every conformer.
    let constraint_defaults: Vec<_> = damped
        .defaults
        .iter()
        .filter(|d| matches!(d.kind, DefaultKind::Constraint(_)))
        .collect();
    assert_eq!(
        constraint_defaults.len(),
        1,
        "Damped should declare exactly 1 trait-level constraint (loss_factor >= 0 \
         — the normative η ≥ 0 half of PRD C1, pinned in a constraint rather than \
         a comment), got {} constraint defaults",
        constraint_defaults.len()
    );

    // ── DampedMaterial: empty-bodied named intersection ──────────────────────
    let damped_material = module
        .trait_defs
        .iter()
        .find(|t| t.name == "DampedMaterial")
        .unwrap_or_else(|| {
            panic!(
                "expected 'DampedMaterial' trait in std/materials/fea, got traits: {:?}",
                trait_names()
            )
        });

    assert!(
        damped_material.required_members.is_empty(),
        "DampedMaterial has an empty body — `required_members` is own-members-only, \
         so the ElasticMaterial + Damped members arrive transitively via \
         collect_all_requirements, not by restatement here; got: {:?}",
        damped_material
            .required_members
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );

    assert_eq!(
        damped_material.refinements,
        vec!["ElasticMaterial".to_string(), "Damped".to_string()],
        "DampedMaterial should be the named intersection `ElasticMaterial + Damped` \
         in declaration order (multi-parent precedent: \
         `trait Watertight : Closed + Manifold {{}}` at geometry_traits.ri:84), got: {:?}",
        damped_material.refinements
    );
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
    // (docs/prds/unit-expressions.md); `7800kg/m^3` is the canonical idiom
    // (see examples/unit_expressions.ri:17). This fixture intentionally uses the
    // compositional form `7800.0 * 1kg / (1m * 1m * 1m)` for test isolation:
    // this test's purpose is Poisson-ratio constraint injection, not compound-unit
    // resolution — using the compound-unit surface would cause a parser/resolver
    // regression to masquerade as a constraint-injection failure here.
    // Compound-unit resolution is covered canonically by compound_unit_resolution_tests.rs
    // and unit_expressions_e2e.rs.
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

/// Asserts the five numeric property defaults of a concrete material
/// evaluate to the expected SI magnitudes, plus that each provenance field
/// carries a `MaterialPropertyProvenance(...)` constructor as its default
/// (verified indirectly via `cell_type` + `default_expr.is_some()` in
/// `assert_fea_material_template_shape`).
///
/// `expected_yield_pa = None` would currently be dead code — all four
/// starter materials declare `some(...)` yields — but the parameter is
/// `Option<f64>` to keep the door open for a future yield-less material
/// without forcing a helper redesign.
///
/// `expected_loss_factor` is task α's (#6877) `Damped` member η. It is
/// dimensionless, so it is checked against `DimensionVector::DIMENSIONLESS`
/// exactly as `poisson_ratio` is.
fn assert_fea_material_property_values(
    name: &str,
    expected_youngs_pa: f64,
    expected_poisson: f64,
    expected_density_kgm3: f64,
    expected_yield_pa: Option<f64>,
    expected_loss_factor: f64,
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
    assert_property_si_value(
        template,
        "loss_factor",
        DimensionVector::DIMENSIONLESS,
        expected_loss_factor,
    );
}

/// Asserts the five-property × five-provenance + one editorial appearance shape
/// of a concrete material structure conforming to `ElasticMaterial + Visual`
/// (task γ, #4762) with the `Damped` member added by task α (#6877). Used by
/// the per-material tests (Steel_AISI_1045, Aluminium_6061_T6,
/// Titanium_Ti6Al4V, ABS_Plastic) to keep the eleven-value-cell +
/// dual-trait-bound + constraint-injection check uniform.
///
/// This helper covers structural shape only (cell names, types, default
/// presence, trait bounds, constraint count). Numeric SI values for each
/// property are asserted by `assert_fea_material_property_values`, called
/// alongside this helper in each per-material test.
fn assert_fea_material_template_shape(name: &str) {
    let template = find_structure(name);

    // γ (#4762) gave each FEA material a second bound, `Visual`; α (#6877)
    // then replaced the first with the named intersection, so the header is
    // now `: DampedMaterial + Visual`.
    //
    // `TopologyTemplate.trait_bounds` is DECLARED-ONLY ("Names of traits this
    // structure declares conformance to", compiler/src/types.rs:688-689), so
    // "ElasticMaterial" is legitimately absent from this vec even though the
    // preset still satisfies that bound transitively. That transitive
    // conformance — the property this literal pin can no longer witness — is
    // asserted separately by
    // `presets_still_satisfy_elastic_material_and_constitutive_law_transitively`.
    assert!(
        template
            .trait_bounds
            .contains(&"DampedMaterial".to_string()),
        "{} should carry 'DampedMaterial' trait bound (α #6877 flipped the \
         preset header from `: ElasticMaterial + Visual` to \
         `: DampedMaterial + Visual`), got: {:?}",
        name,
        template.trait_bounds
    );
    assert!(
        template.trait_bounds.contains(&"Visual".to_string()),
        "{} should carry 'Visual' trait bound (added by task γ #4762), got: {:?}",
        name,
        template.trait_bounds
    );

    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();
    // γ added one `appearance : Appearance` param; α (#6877) adds
    // `loss_factor` and its provenance → count is now 11 (4 ElasticMaterial
    // members + 1 Damped member + 5 per-property provenance + 1 appearance).
    assert_eq!(
        params.len(),
        11,
        "{} should have exactly 11 param cells (4 ElasticMaterial members + 1 \
         Damped member + 5 per-property provenance + 1 editorial appearance), \
         got: {:?}",
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
        ("poisson_ratio", Type::dimensionless_scalar()),
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
        // α (#6877): the `Damped` member, dimensionless η.
        ("loss_factor", Type::dimensionless_scalar()),
        ("youngs_modulus_provenance", provenance_ty.clone()),
        ("poisson_ratio_provenance", provenance_ty.clone()),
        ("density_provenance", provenance_ty.clone()),
        ("yield_stress_provenance", provenance_ty.clone()),
        // α (#6877): η keeps the one-provenance-per-property convention.
        ("loss_factor_provenance", provenance_ty),
        // γ (#4762): editorial appearance member.
        ("appearance", Type::StructureRef("Appearance".to_string())),
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

    // Trait constraints inject into every conforming structure, and they do
    // so TRANSITIVELY: the preset declares only `DampedMaterial`, but the
    // two-hop chain `preset → DampedMaterial → {ElasticMaterial, Damped}`
    // delivers all three — `ElasticMaterial`'s two Poisson bounds plus
    // `Damped`'s `loss_factor >= 0` (α #6877). Pinning to exactly 3 (rather
    // than `>= 3`) catches the case of a structure-local constraint being
    // added without an explicit test update; the four starter materials in
    // `materials_fea.ri` deliberately declare zero structure-local
    // constraints, so the trait-injected trio is the entire set.
    assert_eq!(
        template.constraints.len(),
        3,
        "{} should inherit exactly 3 constraints through DampedMaterial \
         (poisson_ratio >= 0 and poisson_ratio < 0.5 from ElasticMaterial, \
         loss_factor >= 0 from Damped) and declare no structure-local \
         constraints, got {} constraints",
        name,
        template.constraints.len()
    );
}

/// `Steel_AISI_1045` is the medium-carbon hot-rolled-steel starter material.
/// Asserts the structure's full shape: every expected value cell (four
/// `ElasticMaterial` parameters + the `Damped` η + one
/// `MaterialPropertyProvenance` field per property), the `ElasticMaterial`
/// trait bound, that each cell carries a default expression, and that the
/// two Poisson-ratio constraints inject in.
///
/// PRD task #1 cites public matweb-equivalent values:
///   youngs_modulus = 205 GPa, poisson_ratio = 0.29,
///   density = 7850 kg/m³, yield_stress = some(310 MPa).
///
/// α (#6877): loss_factor η = 6.0e-4. CLASS-LEVEL, not alloy-specific —
/// structural / plain-carbon steel spans roughly 2e-4 … 1e-3, and η is
/// amplitude-dependent (pronounced in metals at low stress). Design-critical
/// damping work needs a measurement of the specific alloy, temper and joint
/// condition, not this value.
#[test]
fn steel_aisi_1045_structure_conforms_with_correct_property_values_and_provenance() {
    assert_fea_material_template_shape("Steel_AISI_1045");
    // matweb-equivalent SI values: 205 GPa, 0.29, 7850 kg/m³, 310 MPa,
    // plus α's class-level η = 6.0e-4.
    // The SI check guards against `kPa` vs `GPa` etc. unit-prefix typos
    // that the shape check (which only verifies dimension == PRESSURE)
    // cannot detect.
    assert_fea_material_property_values(
        "Steel_AISI_1045",
        205.0e9,
        0.29,
        7850.0,
        Some(310.0e6),
        6.0e-4,
    );
}

// ─── step-11: Aluminium_6061_T6 starter material ─────────────────────────────

/// `Aluminium_6061_T6` is the precipitation-hardened aerospace-grade aluminium
/// starter material (T6 = solution-heat-treated + artificially aged).
/// Asserts the same eight-cell shape as Steel_AISI_1045 via the shared helper.
///
/// PRD task #1 cites public matweb-equivalent values:
///   youngs_modulus = 68.9 GPa, poisson_ratio = 0.33,
///   density = 2700 kg/m³, yield_stress = some(276 MPa).
///
/// α (#6877): loss_factor η = 2.0e-4. CLASS-LEVEL, not temper-specific —
/// wrought aluminium alloys span roughly 1e-4 … 4e-4, and η is
/// amplitude-dependent. Design-critical damping work needs a measurement of
/// the specific alloy, temper and joint condition.
#[test]
fn aluminium_6061_t6_structure_conforms_with_correct_property_values_and_provenance() {
    assert_fea_material_template_shape("Aluminium_6061_T6");
    // matweb-equivalent SI values: 68.9 GPa, 0.33, 2700 kg/m³, 276 MPa,
    // plus α's class-level η = 2.0e-4.
    assert_fea_material_property_values(
        "Aluminium_6061_T6",
        68.9e9,
        0.33,
        2700.0,
        Some(276.0e6),
        2.0e-4,
    );
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
///
/// α (#6877): loss_factor η = 4.0e-4. CLASS-LEVEL, not condition-specific —
/// alpha-beta titanium alloys span roughly 1e-4 … 1e-3, and η is
/// amplitude-dependent. Design-critical damping work needs a measurement of
/// the specific alloy, temper and joint condition.
#[test]
fn titanium_ti6al4v_structure_conforms_with_correct_property_values_and_provenance() {
    assert_fea_material_template_shape("Titanium_Ti6Al4V");
    // matweb / ASM Handbook SI values: 113.8 GPa, 0.342, 4430 kg/m³, 880 MPa,
    // plus α's class-level η = 4.0e-4.
    assert_fea_material_property_values(
        "Titanium_Ti6Al4V",
        113.8e9,
        0.342,
        4430.0,
        Some(880.0e6),
        4.0e-4,
    );
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
///
/// α (#6877): loss_factor η = 2.0e-2 — roughly two orders of magnitude above
/// the metals, which is the physically meaningful ordering. CLASS-LEVEL, not
/// grade-specific: amorphous thermoplastics span roughly 1e-2 … 4e-2 and are
/// strongly frequency- and temperature-dependent. Design-critical damping
/// work needs a measurement of the specific resin grade at the service
/// frequency and temperature.
#[test]
fn abs_plastic_structure_conforms_with_correct_property_values_and_provenance() {
    assert_fea_material_template_shape("ABS_Plastic");
    // matweb SI values: 2.3 GPa, 0.35, 1050 kg/m³, ~40 MPa (approximate
    // due to ABS's strain-rate-dependent ductile-to-brittle transition),
    // plus α's class-level η = 2.0e-2.
    assert_fea_material_property_values("ABS_Plastic", 2.3e9, 0.35, 1050.0, Some(40.0e6), 2.0e-2);
}

// ─── task α (#6877): prd-gate fixtures compiled from a test target ────────────

/// Compiles both `damped_material_*.ri` prd-gate fixtures through the real
/// stdlib and asserts zero error diagnostics.
///
/// Reading them from a `#[test]` is what GATES them. `tests/prd-gate/README.md`
/// is explicit that fixtures there are "not required to parse or to pass
/// `reify check`" and that "nothing in the repo compiles this directory
/// wholesale" — so an unread fixture is inert, and the sanctioned way to make
/// one genuinely gated is to name it from a compiled test target and register
/// its basename in `scripts/verify.sh`'s `_RUST_COUPLED_RI_FIXTURES`. Path
/// resolution follows `harness_units/torque_unit_tests.rs`'s
/// `prd_gate_fixture_unit_nm_torque_immediate_compiles_clean` verbatim.
///
/// Read-only input: this test must never edit either fixture.
///
/// The two fixtures pin DIFFERENT things, which is why both are covered here
/// rather than one standing in for the other:
///
///   - `damped_material_mixin_conformance.ri` is the pre-existing SUBSTRATE
///     probe. Its trait names are probe-local (`DampedProbe` /
///     `DampedElasticProbe`) and deliberately do not collide with the stdlib
///     `Damped` / `DampedMaterial`, so it pins the grammar and semantics the
///     mixin design assumes — user trait declaration, multi-parent refinement,
///     a conformer declaring both parents' params, and member access through a
///     trait-typed param. It was green before α and must stay green after;
///     that is this task's first user-observable signal.
///   - `damped_material_preset_conformance.ri` is the sibling that pins the
///     LANDED STDLIB SURFACE — the PRD §Goal user spelling plus preset η
///     exposure.
///
/// If the mixin fixture's arm is what fails, the substrate broke and that must
/// be root-caused rather than papered over.
#[test]
fn prd_gate_damped_material_fixtures_compile_clean() {
    // Each path is spelled as a SINGLE `<dir>/<name>.ri` leaf literal, never
    // as a `.join(dir).join(name)` pair. `tests/infra/test_verify_scope.sh`'s
    // PG-DRIFT-DIR scenario fails on any *.rs that names the fixtures
    // DIRECTORY without a reviewed marker — a directory reference could mean
    // the target walks it, which would void verify.sh's "adding a fixture is
    // inert" premise. The leaf form is also what PG-DRIFT greps for when it
    // re-derives the coupled set, so this spelling is what makes the two
    // fixtures visible to the drift guard at all.
    for rel in [
        "../../tests/prd-gate/fixtures/damped_material_mixin_conformance.ri",
        "../../tests/prd-gate/fixtures/damped_material_preset_conformance.ri",
    ] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
        let src = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read fixture {}: {}", path.display(), e));

        let module = compile_source_with_stdlib(&src);
        let errors: Vec<_> = module
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "fixture {} should compile with no errors, got: {:?}",
            path.display(),
            errors
        );
    }
}

// ─── task α (#6877): transitive conformance survives the DampedMaterial flip ──

/// The four presets now DECLARE only `DampedMaterial + Visual`, and
/// `TopologyTemplate.trait_bounds` records declared bounds only ("Names of
/// traits this structure declares conformance to", compiler/src/types.rs:688).
/// So the literal `contains("ElasticMaterial")` pins that the shape helper and
/// the module-summary test used to carry could no longer be written — but the
/// property they stood in for, that a preset is still usable wherever an
/// `ElasticMaterial` / `ConstitutiveLaw` is expected, is exactly what must not
/// regress. This test asserts that property directly, so the weakening of
/// those literal pins cannot hide a real conformance loss.
///
/// NOTE on probe shape — this was measured, not assumed. A
/// `param p : SomeTrait = SomePreset()` slot inside a plain `structure` block
/// emits NO diagnostic even when the value is flagrantly non-conforming
/// (verified: `param a : ElasticMaterial = MaterialPropertyProvenance(...)`
/// compiles with zero errors), so that shape is toothless as a conformance
/// probe and is deliberately NOT used here. The two shapes that ARE enforced:
///
///   - a trait-bounded FUNCTION PARAMETER, which emits
///     `TypeNotConformingToTrait` (same code as
///     `fea_supertrait_conformance_tests::box_at_material_slot_still_errors`);
///   - a `structure def X : SomeTrait` body, which emits
///     `MissingRequiredMember` for any unprovided requirement.
///
/// Both are exercised below, one per direction of the refinement chain.
#[test]
fn presets_still_satisfy_elastic_material_and_constitutive_law_transitively() {
    // ── Direction 1: "satisfies" — a preset is accepted at a supertrait slot.
    //
    // `solve_elastic_static(material : ConstitutiveLaw, …)` is the two-hop
    // case (`preset → DampedMaterial → ElasticMaterial → ConstitutiveLaw`) and
    // `solve_buckling(material : ElasticMaterial, …)` the one-hop case. Both
    // are real stdlib consumers of the bounds the α flip stopped declaring.
    // `trait_satisfies` (entity.rs:6186-6215) walks ALL parents of a
    // multi-parent refinement under a visited guard, which is what makes this
    // hold.
    let satisfies_source = r#"
structure ProbeTransitiveConformance {
    let a = solve_elastic_static(
        ABS_Plastic(),
        1000mm, 100mm, 100mm,
        [PointLoad(point: "tip", force: 1000.0)],
        [FixedSupport(target: "root")],
        ElasticOptions()
    )
    let b = solve_buckling(
        Steel_AISI_1045(),
        1000mm, 100mm, 100mm,
        [PointLoad(point: "tip", force: 1000.0)],
        [FixedSupport(target: "root")],
        BucklingOptions()
    )
}
"#;
    let compiled = compile_source_with_stdlib(satisfies_source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "a preset declaring only `DampedMaterial + Visual` must still be \
         accepted at a `ConstitutiveLaw` slot (solve_elastic_static, 2 hops) \
         and at an `ElasticMaterial` slot (solve_buckling, 1 hop) — a \
         TypeNotConformingToTrait here means the α flip broke a real consumer, \
         not just a literal pin; got: {:?}",
        errors
    );

    // ── Direction 2: "requires" — the contract still arrives transitively.
    //
    // A conformer declaring `: DampedMaterial` must supply BOTH parents'
    // members. Omitting `loss_factor` (the `Damped` half, reached through the
    // two-hop chain) must be a hard error, and supplying all five must compile
    // clean. Without this arm, a `DampedMaterial` that had silently lost its
    // parents would still pass direction 1 vacuously.
    let missing_loss_factor = r#"
structure def ProbeMissingLossFactor : DampedMaterial {
    param youngs_modulus : Pressure = 30GPa
    param poisson_ratio : Real = 0.25
    param density : Density = 2300kg/m^3
    param yield_stress : Option<Pressure> = none
}
"#;
    let compiled = compile_source_with_stdlib(missing_loss_factor);
    assert!(
        compiled.diagnostics.iter().any(|d| {
            d.severity == Severity::Error
                && d.code == Some(DiagnosticCode::MissingRequiredMember)
                && d.message.contains("loss_factor")
        }),
        "a `: DampedMaterial` conformer omitting loss_factor must raise \
         MissingRequiredMember — that is the proof the `Damped` half of the \
         named intersection is still reached transitively; got: {:?}",
        compiled
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect::<Vec<_>>()
    );

    let all_five = r#"
structure def ProbeAllFive : DampedMaterial {
    param youngs_modulus : Pressure = 30GPa
    param poisson_ratio : Real = 0.25
    param density : Density = 2300kg/m^3
    param yield_stress : Option<Pressure> = none
    param loss_factor : Real = 0.03
}
"#;
    let compiled = compile_source_with_stdlib(all_five);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "a `: DampedMaterial` conformer declaring all five members (four from \
         ElasticMaterial + loss_factor from Damped) must compile cleanly, got: {:?}",
        errors
    );
}

// ─── step-17: module summary regression test ─────────────────────────────────

/// Final regression covering the std/materials/fea module's overall shape.
/// At this point the previous tests already check each entity in detail; this
/// test exists to lock in the module's *cardinality* — exactly four traits
/// (`ConstitutiveLaw` marker + `ElasticMaterial` + `Damped` mixin +
/// `DampedMaterial` named intersection), exactly five top-level structures
/// (one provenance record + four materials), zero error diagnostics, every
/// material carries the `ElasticMaterial` trait bound. Adding or removing a
/// top-level entity from `materials_fea.ri` will fail this test, which is the
/// intended behaviour: any future expansion should be expressed as a deliberate
/// update here, not silently introduced.
///
/// `ConstitutiveLaw` is declared here (task γ relocated it from constitutive.ri)
/// so that `trait ElasticMaterial : ConstitutiveLaw` is not a forward-reference
/// (materials_fea loads before constitutive in stdlib_loader.rs — PRD §4.2 γ).
///
/// `Damped` and `DampedMaterial` were added by task α (#6877). They live in
/// THIS file, not `materials_mechanical.ri` — `materials_mechanical_tests.rs`
/// pins that module at exactly 10 traits, so a misplacement reds there while
/// this count would still pass.
#[test]
fn std_materials_fea_module_summary_has_four_traits_one_provenance_struct_and_four_materials() {
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

    // Exactly four traits: `ConstitutiveLaw` (relocated marker, task γ),
    // `ElasticMaterial` (dimensioned FEA trait), and — added by task α
    // (#6877) — the `Damped` hysteretic-loss mixin plus the `DampedMaterial`
    // named intersection `ElasticMaterial + Damped`.
    let trait_names: Vec<&str> = module.trait_defs.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(
        module.trait_defs.len(),
        4,
        "std/materials/fea should declare exactly 4 traits \
         (ConstitutiveLaw + ElasticMaterial + Damped + DampedMaterial), got: {:?}",
        trait_names
    );
    for expected_trait in &[
        "ConstitutiveLaw",
        "ElasticMaterial",
        "Damped",
        "DampedMaterial",
    ] {
        assert!(
            module.trait_defs.iter().any(|t| t.name == *expected_trait),
            "std/materials/fea should contain the '{}' trait, got: {:?}",
            expected_trait,
            trait_names
        );
    }

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

    // Every starter material must carry the `DampedMaterial` trait bound —
    // α (#6877) flipped the four headers from `: ElasticMaterial + Visual` to
    // `: DampedMaterial + Visual`, and `trait_bounds` is declared-only.
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
                .contains(&"DampedMaterial".to_string()),
            "{} should carry 'DampedMaterial' trait bound, got: {:?}",
            material,
            template.trait_bounds
        );
    }
}
