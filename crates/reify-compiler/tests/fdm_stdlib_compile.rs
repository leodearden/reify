//! Tests for `crates/reify-compiler/stdlib/fdm.ri` —
//! `std.fdm` module: `InfillPattern` enum + `FDMProcess` structure —
//! the process-parameters record for the v0.5 FDM-as-printed-FEA PRD.
//!
//! Observable signal for PRD §task α / slice 1
//! (docs/prds/v0_5/fdm-as-printed-fea.md). Per the PRD, this file
//! parses the enum and structure_def and confirms the compiled shape
//! matches the PRD §"Process-parameter type surface" spec.
//!
//! Tests validate that the .ri file is loaded by the production stdlib path
//! (mirroring `materials_fea_tests.rs` / `trajectory_stdlib_compile.rs`),
//! that the declared enum and structure are correctly represented in the
//! compiled module, and that the per-property provenance convention from
//! `materials_fea.ri` is faithfully reproduced.
//!
//! All tests use the production-path `load_stdlib_module()` helper that
//! exercises the same embedded + sequential-prelude compilation path as
//! production. This mirrors the helper trio in `trajectory_stdlib_compile.rs`.

use reify_compiler::*;
use reify_test_support::compile_source_with_stdlib;
use reify_types::*;

// ─── helpers ──────────────────────────────────────────────────────────────────

/// Return the `std/fdm` CompiledModule from the production stdlib loader.
/// Exercises the exact same code path as production: embedded source,
/// sequential compilation with growing prelude, OnceLock caching.
///
/// Panics if the module is not found — which is the expected failure mode
/// until step-2 lands the .ri file and loader registration.
fn load_stdlib_module() -> &'static CompiledModule {
    stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/fdm")
        .unwrap_or_else(|| {
            panic!(
                "stdlib should contain std/fdm module; available paths: {:?}",
                stdlib_loader::load_stdlib()
                    .iter()
                    .map(|m| m.path.to_string())
                    .collect::<Vec<_>>()
            )
        })
}

/// Look up a structure template by name within the `std/fdm` module.
fn find_structure(name: &str) -> &'static TopologyTemplate {
    let module = load_stdlib_module();
    module
        .templates
        .iter()
        .find(|t| t.name == name && t.entity_kind == EntityKind::Structure)
        .unwrap_or_else(|| {
            panic!(
                "expected `structure def {}` template in std/fdm, got templates: {:?}",
                name,
                module
                    .templates
                    .iter()
                    .map(|t| (&t.name, &t.entity_kind))
                    .collect::<Vec<_>>()
            )
        })
}

/// Look up an enum definition by name within the `std/fdm` module.
fn find_enum(name: &str) -> &'static EnumDef {
    let module = load_stdlib_module();
    module
        .enum_defs
        .iter()
        .find(|e| e.name == name)
        .unwrap_or_else(|| {
            panic!(
                "expected `enum {}` in std/fdm, got enum_defs: {:?}",
                name,
                module
                    .enum_defs
                    .iter()
                    .map(|e| &e.name)
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

// ─── step-1: module loads with zero error diagnostics ────────────────────────

/// The std/fdm module must load through the production stdlib path with zero
/// error-severity diagnostics. The loader-level `assert!` already fails fast
/// on Error diagnostics during init, but this test independently asserts the
/// post-init invariant so a regression is caught at the test boundary rather
/// than at first stdlib touch.
#[test]
fn std_fdm_module_loads_with_no_errors() {
    let module = load_stdlib_module();

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected error diagnostics in fdm.ri: {:?}",
        errors
    );
}

// ─── step-3: InfillPattern enum ──────────────────────────────────────────────

/// `InfillPattern` selects which infill geometry the slicer produces for a
/// printed part. The canonical order is near-isotropic patterns first
/// (Gyroid, Cubic), then directional patterns (Grid, Triangular, Honeycomb),
/// matching the PRD §"Built-in property correlations" grouping:
/// "gyroid/cubic treated as near-isotropic; grid/triangular/honeycomb get
/// directional factors."
///
/// Test pins the variant vector exactly (order-sensitive) — mirrors the
/// discipline in `trajectory_stdlib_compile.rs::spline_kind_enum_has_cubic_
/// and_quintic_variants`.
#[test]
fn infill_pattern_enum_has_five_variants_in_canonical_order() {
    let enum_def = find_enum("InfillPattern");

    assert_eq!(
        enum_def.variants,
        vec![
            "Gyroid".to_string(),
            "Cubic".to_string(),
            "Grid".to_string(),
            "Triangular".to_string(),
            "Honeycomb".to_string(),
        ],
        "InfillPattern variants must match the PRD §\"Built-in property correlations\" \
         spec exactly (order-sensitive: near-isotropic first, directional second); \
         got: {:?}",
        enum_def.variants
    );
}

// ─── step-5: FDMProcess 14-cell shape ────────────────────────────────────────

/// `FDMProcess` carries seven mechanically-relevant process-parameter fields
/// plus seven parallel `..._provenance : MaterialPropertyProvenance` slots —
/// one per logical param — mirroring the `Steel_AISI_1045` / `ABS_Plastic`
/// per-property-provenance convention in `materials_fea.ri`.
///
/// FDMProcess is a concrete process-parameters record, not a trait-bound type.
/// It has no `trait_bounds` (mirrors `ElasticOptions` in `solver_elastic.ri`).
///
/// Test pins: (a) the 14-cell shape (7 logical + 7 provenance) in canonical
/// declaration order, (b) the expected (name, type) pair for each cell, (c)
/// `template.trait_bounds.is_empty()` — concrete options record, no trait.
#[test]
fn fdm_process_structure_has_seven_params_plus_seven_provenance_slots() {
    let template = find_structure("FDMProcess");

    // FDMProcess is a concrete options record — no trait bound.
    assert!(
        template.trait_bounds.is_empty(),
        "FDMProcess should declare no trait bounds (concrete options record, \
         not a trait refinement); got: {:?}",
        template.trait_bounds
    );

    let params = param_cells(template);
    let names: Vec<&str> = params.iter().map(|vc| vc.id.member.as_str()).collect();

    assert_eq!(
        params.len(),
        14,
        "FDMProcess should have exactly 14 param cells (7 logical + 7 provenance), \
         got: {:?}",
        names
    );

    let provenance_ty = Type::StructureRef("MaterialPropertyProvenance".to_string());
    let expected: &[(&str, Type)] = &[
        (
            "build_direction",
            Type::Vector {
                n: 3,
                quantity: Box::new(Type::Scalar {
                    dimension: DimensionVector::LENGTH,
                }),
            },
        ),
        (
            "layer_height",
            Type::Scalar {
                dimension: DimensionVector::LENGTH,
            },
        ),
        ("walls", Type::Int),
        ("top_bottom_layers", Type::Int),
        ("infill_density", Type::Real),
        ("infill_pattern", Type::Enum("InfillPattern".to_string())),
        (
            "material",
            Type::TraitObject("ElasticMaterial".to_string()),
        ),
        ("build_direction_provenance", provenance_ty.clone()),
        ("layer_height_provenance", provenance_ty.clone()),
        ("walls_provenance", provenance_ty.clone()),
        ("top_bottom_layers_provenance", provenance_ty.clone()),
        ("infill_density_provenance", provenance_ty.clone()),
        ("infill_pattern_provenance", provenance_ty.clone()),
        ("material_provenance", provenance_ty),
    ];

    // Param declaration order is part of the contract — pin it explicitly
    // (mirrors the discipline in `trajectory_stdlib_compile.rs`).
    let expected_names: Vec<&str> = expected.iter().map(|(m, _)| *m).collect();
    assert_eq!(
        names, expected_names,
        "FDMProcess params must be declared in canonical order \
         (7 logical params then 7 provenance slots); got: {:?}",
        names
    );

    for (member, expected_ty) in expected {
        let cell = params
            .iter()
            .find(|vc| vc.id.member == *member)
            .unwrap_or_else(|| {
                panic!(
                    "FDMProcess missing required param '{}'; got: {:?}",
                    member, names
                )
            });
        assert_eq!(
            cell.cell_type, *expected_ty,
            "FDMProcess.{} should be {:?}, got {:?}",
            member, expected_ty, cell.cell_type
        );
    }
}

// ─── step-7: FDMProcess defaults SI values + provenance constructors ─────────

/// `FDMProcess` defaults must match the PRD §"Motivating end-to-end" values
/// exactly. This test checks both the type-level dimension and the SI
/// magnitude of each default expression, guarding against unit-prefix typos
/// (e.g. `0.2m` vs `0.2mm`).
///
/// For the `build_direction = vec3(0mm, 0mm, 1mm)` default, the test checks
/// the presence and type of the FunctionCall node rather than attempting to
/// numerically reduce a runtime builtin call at compile time.
///
/// For the seven `*_provenance` slots, presence of a default (plus the
/// correct `StructureRef` cell type) is the contract: the provenance
/// constructor's exact string content is not pinned here (that would be
/// fragile to rewording) — only that it exists and has the right type.
///
/// Mirrors `materials_fea_tests.rs::assert_property_si_value` discipline.
#[test]
fn fdm_process_defaults_have_expected_si_values_and_provenance_constructors() {
    let template = find_structure("FDMProcess");

    // layer_height = 0.2mm → 0.0002 m SI
    assert_scalar_default(
        template,
        "layer_height",
        DimensionVector::LENGTH,
        0.2e-3,
    );

    // infill_density = 0.2 (dimensionless Real)
    assert_real_default(template, "infill_density", 0.2);

    // walls = 3 (Int)
    assert_int_default(template, "walls", 3);

    // top_bottom_layers = 4 (Int)
    assert_int_default(template, "top_bottom_layers", 4);

    // infill_pattern = InfillPattern.Gyroid → Literal(Value::Enum { .. })
    {
        let cell = template
            .value_cells
            .iter()
            .find(|vc| vc.id.member == "infill_pattern")
            .expect("FDMProcess missing 'infill_pattern' cell");
        let expr = cell
            .default_expr
            .as_ref()
            .expect("FDMProcess.infill_pattern missing default_expr");
        match &expr.kind {
            CompiledExprKind::Literal(Value::Enum { type_name, variant }) => {
                assert_eq!(type_name, "InfillPattern", "infill_pattern default enum type_name");
                assert_eq!(variant, "Gyroid", "infill_pattern default enum variant");
            }
            other => panic!(
                "FDMProcess.infill_pattern default should be \
                 Literal(Value::Enum {{ type_name: \"InfillPattern\", variant: \"Gyroid\" }}), \
                 got: {:?}",
                other
            ),
        }
    }

    // material = ABS_Plastic() → StructureInstanceCtor { type_name: "ABS_Plastic" }
    {
        let cell = template
            .value_cells
            .iter()
            .find(|vc| vc.id.member == "material")
            .expect("FDMProcess missing 'material' cell");
        let expr = cell
            .default_expr
            .as_ref()
            .expect("FDMProcess.material missing default_expr");
        match &expr.kind {
            CompiledExprKind::StructureInstanceCtor { type_name, .. } => {
                assert_eq!(
                    type_name, "ABS_Plastic",
                    "FDMProcess.material default should be ABS_Plastic(), got type_name: {}",
                    type_name
                );
            }
            other => panic!(
                "FDMProcess.material default should be \
                 StructureInstanceCtor {{ type_name: \"ABS_Plastic\", .. }}, \
                 got: {:?}",
                other
            ),
        }
    }

    // build_direction = vec3(0mm, 0mm, 1mm) → FunctionCall { function.name: "vec3" }
    // Note: the compiled default_expr.result_type for a vec3(...) call is
    // the common scalar dimension of the args (Scalar<Length>), not Vector3<Length>.
    // The Vector3<Length> type is pinned by the cell's declared cell_type (step-5).
    // Here we verify the FunctionCall structure and the component SI values.
    {
        let cell = template
            .value_cells
            .iter()
            .find(|vc| vc.id.member == "build_direction")
            .expect("FDMProcess missing 'build_direction' cell");
        // The declared cell type must be Vector3<Length> (pinned in step-5)
        assert_eq!(
            cell.cell_type,
            Type::Vector {
                n: 3,
                quantity: Box::new(Type::Scalar {
                    dimension: DimensionVector::LENGTH,
                }),
            },
            "FDMProcess.build_direction cell_type should be Vector3<Length>"
        );
        let expr = cell
            .default_expr
            .as_ref()
            .expect("FDMProcess.build_direction missing default_expr");
        // The expression must be a FunctionCall to "vec3"
        match &expr.kind {
            CompiledExprKind::FunctionCall { function, args } => {
                assert_eq!(
                    function.name, "vec3",
                    "FDMProcess.build_direction default should call 'vec3', \
                     got function.name: {}",
                    function.name
                );
                assert_eq!(
                    args.len(),
                    3,
                    "vec3 call in build_direction default should have 3 args, got {}",
                    args.len()
                );
                // z-component (index 2) should be 1mm = 0.001 m SI
                let z_si = extract_scalar_si(&args[2]);
                let tol = 1e-9_f64;
                assert!(
                    (z_si - 1e-3).abs() <= tol,
                    "build_direction z-component (vec3 arg[2]) should be 1mm = 0.001 m, \
                     got {} m",
                    z_si
                );
                // x and y components should be 0
                let x_si = extract_scalar_si(&args[0]);
                let y_si = extract_scalar_si(&args[1]);
                assert!(
                    x_si.abs() <= tol,
                    "build_direction x-component should be 0 m, got {} m",
                    x_si
                );
                assert!(
                    y_si.abs() <= tol,
                    "build_direction y-component should be 0 m, got {} m",
                    y_si
                );
            }
            other => panic!(
                "FDMProcess.build_direction default should be FunctionCall {{ name: \"vec3\", .. }}, \
                 got: {:?}",
                other
            ),
        }
    }

    // All seven *_provenance slots must carry a default StructureInstanceCtor
    // for MaterialPropertyProvenance.
    let provenance_fields = &[
        "build_direction_provenance",
        "layer_height_provenance",
        "walls_provenance",
        "top_bottom_layers_provenance",
        "infill_density_provenance",
        "infill_pattern_provenance",
        "material_provenance",
    ];
    for prov_name in provenance_fields {
        let cell = template
            .value_cells
            .iter()
            .find(|vc| vc.id.member == *prov_name)
            .unwrap_or_else(|| panic!("FDMProcess missing '{}' cell", prov_name));
        // Cell type must be MaterialPropertyProvenance
        assert_eq!(
            cell.cell_type,
            Type::StructureRef("MaterialPropertyProvenance".to_string()),
            "FDMProcess.{} cell_type should be StructureRef(MaterialPropertyProvenance)",
            prov_name
        );
        // Default must be present and a StructureInstanceCtor for MaterialPropertyProvenance
        let expr = cell
            .default_expr
            .as_ref()
            .unwrap_or_else(|| panic!("FDMProcess.{} missing default_expr", prov_name));
        match &expr.kind {
            CompiledExprKind::StructureInstanceCtor { type_name, .. } => {
                assert_eq!(
                    type_name, "MaterialPropertyProvenance",
                    "FDMProcess.{} default should be MaterialPropertyProvenance(..), \
                     got type_name: {}",
                    prov_name, type_name
                );
            }
            other => panic!(
                "FDMProcess.{} default should be \
                 StructureInstanceCtor {{ type_name: \"MaterialPropertyProvenance\", .. }}, \
                 got: {:?}",
                prov_name, other
            ),
        }
    }
}

// ─── step-7 helpers ───────────────────────────────────────────────────────────

/// Extract the SI scalar value from a compiled expression. Handles:
///   - `Literal(Value::Scalar { si_value, .. })` — dimensioned quantity literals
///   - `Literal(Value::Real(v))` — bare numbers
///   - `Literal(Value::Int(v))` — bare integers
///   - `BinOp { Mul | Div | Add | Sub }` — compositional forms
///   - `OptionSome(inner)` — some(...)
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
                    "compute_si_value: unsupported BinOp {:?} in FDMProcess default expr",
                    other
                ),
            }
        }
        CompiledExprKind::OptionSome(inner) => compute_si_value(inner),
        other => panic!(
            "compute_si_value: unsupported expression kind in FDMProcess default: {:?}",
            other
        ),
    }
}

/// Extract the SI scalar magnitude from a dimensioned literal expression.
/// Only accepts `Literal(Value::Scalar { si_value, .. })` directly.
fn extract_scalar_si(expr: &CompiledExpr) -> f64 {
    match &expr.kind {
        CompiledExprKind::Literal(Value::Scalar { si_value, .. }) => *si_value,
        other => panic!(
            "extract_scalar_si: expected Literal(Value::Scalar), got: {:?}",
            other
        ),
    }
}

/// Assert that the named param cell on `template` carries a scalar default
/// with the expected dimension and SI magnitude (1e-6 relative tolerance).
fn assert_scalar_default(
    template: &TopologyTemplate,
    member: &str,
    expected_dim: DimensionVector,
    expected_si: f64,
) {
    let cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == member)
        .unwrap_or_else(|| panic!("FDMProcess missing param '{}'", member));
    let expr = cell
        .default_expr
        .as_ref()
        .unwrap_or_else(|| panic!("FDMProcess.{} missing default_expr", member));

    let actual_dim = match &expr.result_type {
        Type::Scalar { dimension } => *dimension,
        other => panic!(
            "FDMProcess.{} default result_type should be Scalar, got: {:?}",
            member, other
        ),
    };
    assert_eq!(
        actual_dim, expected_dim,
        "FDMProcess.{} default dimension should be {:?}, got {:?}",
        member, expected_dim, actual_dim
    );

    let actual_si = compute_si_value(expr);
    let tol = 1e-6 * expected_si.abs().max(1.0);
    assert!(
        (actual_si - expected_si).abs() <= tol,
        "FDMProcess.{} default SI value should be {} (within {}), got {}",
        member,
        expected_si,
        tol,
        actual_si
    );
}

/// Assert that the named param cell on `template` carries a Real default
/// with the expected value (1e-9 absolute tolerance for dimensionless).
fn assert_real_default(template: &TopologyTemplate, member: &str, expected: f64) {
    let cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == member)
        .unwrap_or_else(|| panic!("FDMProcess missing param '{}'", member));
    let expr = cell
        .default_expr
        .as_ref()
        .unwrap_or_else(|| panic!("FDMProcess.{} missing default_expr", member));

    assert_eq!(
        expr.result_type,
        Type::Real,
        "FDMProcess.{} default result_type should be Real, got: {:?}",
        member,
        expr.result_type
    );

    let actual = compute_si_value(expr);
    let tol = 1e-9_f64;
    assert!(
        (actual - expected).abs() <= tol,
        "FDMProcess.{} default value should be {} (within {}), got {}",
        member,
        expected,
        tol,
        actual
    );
}

/// Assert that the named param cell on `template` carries an Int default
/// with the expected integer value.
fn assert_int_default(template: &TopologyTemplate, member: &str, expected: i64) {
    let cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == member)
        .unwrap_or_else(|| panic!("FDMProcess missing param '{}'", member));
    let expr = cell
        .default_expr
        .as_ref()
        .unwrap_or_else(|| panic!("FDMProcess.{} missing default_expr", member));

    assert_eq!(
        expr.result_type,
        Type::Int,
        "FDMProcess.{} default result_type should be Int, got: {:?}",
        member,
        expr.result_type
    );

    match &expr.kind {
        CompiledExprKind::Literal(Value::Int(v)) => {
            assert_eq!(
                *v, expected,
                "FDMProcess.{} default value should be {}, got {}",
                member, expected, v
            );
        }
        other => panic!(
            "FDMProcess.{} default should be Literal(Value::Int({})), got: {:?}",
            member, expected, other
        ),
    }
}

// ─── step-9a: PRD motivating-example ctor compiles cleanly ───────────────────

/// The PRD §"Motivating end-to-end (Rung-0, decompose-ready)" named-arg
/// constructor form must compile without error diagnostics through the
/// stdlib prelude path. This is the user-observable signal for α: the
/// FDMProcess, InfillPattern, and ABS_Plastic names all resolve without
/// inline redeclaration.
///
/// Mirrors `materials_chemical_tests.rs::titanium_implant_conforms_without_
/// inline_enum_redeclarations` discipline.
#[test]
fn prd_motivating_example_named_arg_ctor_compiles_cleanly() {
    // PRD §"Motivating end-to-end" lines 12-20 wrapped in a shell structure.
    let source = r#"
structure def TestFDM {
    let proc = FDMProcess(
        build_direction: vec3(0mm, 0mm, 1mm),
        layer_height: 0.2mm,
        walls: 3,
        top_bottom_layers: 4,
        infill_density: 0.2,
        infill_pattern: InfillPattern.Gyroid,
        material: ABS_Plastic()
    )
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
        "PRD motivating-example FDMProcess ctor should compile without errors; \
         got: {:?}",
        errors
    );
}

// ─── step-9b: module summary cardinality lock ─────────────────────────────────

/// Final regression that locks in the std/fdm module's cardinality:
/// exactly one enum (`InfillPattern`), exactly one top-level structure
/// (`FDMProcess`), zero error diagnostics. Any silent future expansion
/// of `fdm.ri` fails this test, which is the intended behaviour — deliberate
/// updates require an explicit test change.
///
/// Mirrors `materials_fea_tests.rs::std_materials_fea_module_summary_has_one_
/// trait_one_provenance_struct_and_four_materials` discipline.
#[test]
fn std_fdm_module_summary_has_one_enum_one_structure_zero_errors() {
    let module = load_stdlib_module();

    // Zero error diagnostics.
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "std/fdm should have zero error diagnostics, got: {:?}",
        errors
    );

    // Exactly one enum: InfillPattern.
    let enum_names: Vec<&str> = module.enum_defs.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(
        module.enum_defs.len(),
        1,
        "std/fdm should declare exactly 1 enum (InfillPattern), got: {:?}",
        enum_names
    );
    assert!(
        module.enum_defs.iter().any(|e| e.name == "InfillPattern"),
        "std/fdm should contain the 'InfillPattern' enum, got: {:?}",
        enum_names
    );

    // Exactly one structure: FDMProcess.
    let structure_names: Vec<&str> = module
        .templates
        .iter()
        .filter(|t| t.entity_kind == EntityKind::Structure)
        .map(|t| t.name.as_str())
        .collect();
    assert_eq!(
        structure_names.len(),
        1,
        "std/fdm should declare exactly 1 structure (FDMProcess), got: {:?}",
        structure_names
    );
    assert!(
        module
            .templates
            .iter()
            .any(|t| t.name == "FDMProcess" && t.entity_kind == EntityKind::Structure),
        "std/fdm should contain the 'FDMProcess' structure, got: {:?}",
        structure_names
    );
}
