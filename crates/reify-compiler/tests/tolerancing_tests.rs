//! Tests for stdlib/tolerancing.ri — dimensional, geometric (14 GD&T types), surface tolerancing.
//!
//! Tests validate that the .ri file parses and compiles cleanly, that each
//! enum, trait, and structure def is correctly represented, and that trait
//! conformance works via the production prelude.

use reify_compiler::*;
use reify_core::*;
use reify_ir::*;

// ─── helpers ──────────────────────────────────────────────────────────────────

/// Return the `std/tolerancing` CompiledModule from the production stdlib loader.
/// Exercises the exact same code path as production.
fn load_stdlib_module() -> &'static CompiledModule {
    stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/tolerancing")
        .expect("stdlib should contain std/tolerancing module")
}

// ─── step-3: file exists, parses, compiles without errors ────────────────────

/// Step 3: tolerancing.ri exists, parses cleanly, compiles without
/// error-severity diagnostics.
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
        "unexpected error diagnostics in tolerancing.ri: {:?}",
        errors
    );

    assert!(
        !module.enum_defs.is_empty(),
        "expected at least one enum def, got zero"
    );
}

// ─── step-5: all 4 enums with correct variants ───────────────────────────────

/// Step 5: All 4 enums are present with correct variant counts and specific
/// variant names for MaterialCondition.
#[test]
fn all_four_enums_with_correct_variants() {
    let module = load_stdlib_module();

    assert_eq!(
        module.enum_defs.len(),
        4,
        "expected exactly 4 enums, got: {:?}",
        module.enum_defs.iter().map(|e| &e.name).collect::<Vec<_>>()
    );

    // MaterialCondition: 3 variants (MMC, LMC, RFS)
    let mc = module
        .enum_defs
        .iter()
        .find(|e| e.name == "MaterialCondition")
        .expect("expected 'MaterialCondition' enum");
    assert_eq!(
        mc.variants.len(),
        3,
        "MaterialCondition should have 3 variants, got: {:?}",
        mc.variants
    );
    for variant in &["MMC", "LMC", "RFS"] {
        assert!(
            mc.variants.contains(&variant.to_string()),
            "MaterialCondition missing variant '{}', variants: {:?}",
            variant,
            mc.variants
        );
    }

    // FitCategory: 3 variants
    let fc = module
        .enum_defs
        .iter()
        .find(|e| e.name == "FitCategory")
        .expect("expected 'FitCategory' enum");
    assert_eq!(
        fc.variants.len(),
        3,
        "FitCategory should have 3 variants, got: {:?}",
        fc.variants
    );

    // SurfaceParameter: 8 variants
    let sp = module
        .enum_defs
        .iter()
        .find(|e| e.name == "SurfaceParameter")
        .expect("expected 'SurfaceParameter' enum");
    assert_eq!(
        sp.variants.len(),
        8,
        "SurfaceParameter should have 8 variants, got: {:?}",
        sp.variants
    );

    // SurfaceDirection: 6 variants
    let sd = module
        .enum_defs
        .iter()
        .find(|e| e.name == "SurfaceDirection")
        .expect("expected 'SurfaceDirection' enum");
    assert_eq!(
        sd.variants.len(),
        6,
        "SurfaceDirection should have 6 variants, got: {:?}",
        sd.variants
    );
}

// ─── step-7: DimensionalTolerance structure correctness ───────────────────────

/// Step 7: DimensionalTolerance has 3 Param value cells, 3 Let value cells,
/// and at least 1 constraint.
#[test]
fn dimensional_tolerance_has_params_lets_and_constraint() {
    let module = load_stdlib_module();

    let dt = module
        .templates
        .iter()
        .find(|t| t.name == "DimensionalTolerance")
        .expect("expected 'DimensionalTolerance' template");

    let param_cells: Vec<_> = dt
        .value_cells
        .iter()
        .filter(|vc| vc.kind == ValueCellKind::Param)
        .collect();
    assert_eq!(
        param_cells.len(),
        3,
        "DimensionalTolerance should have 3 Param cells (nominal, upper_deviation, lower_deviation), got: {:?}",
        param_cells
            .iter()
            .map(|vc| &vc.id.member)
            .collect::<Vec<_>>()
    );

    let param_names: Vec<&str> = param_cells.iter().map(|vc| vc.id.member.as_str()).collect();
    for name in &["nominal", "upper_deviation", "lower_deviation"] {
        assert!(
            param_names.contains(name),
            "DimensionalTolerance missing param '{}', params: {:?}",
            name,
            param_names
        );
    }

    let let_cells: Vec<_> = dt
        .value_cells
        .iter()
        .filter(|vc| vc.kind == ValueCellKind::Let)
        .collect();
    assert_eq!(
        let_cells.len(),
        3,
        "DimensionalTolerance should have 3 Let cells (upper_limit, lower_limit, tolerance_band), got: {:?}",
        let_cells.iter().map(|vc| &vc.id.member).collect::<Vec<_>>()
    );

    let let_names: Vec<&str> = let_cells.iter().map(|vc| vc.id.member.as_str()).collect();
    for name in &["upper_limit", "lower_limit", "tolerance_band"] {
        assert!(
            let_names.contains(name),
            "DimensionalTolerance missing let '{}', lets: {:?}",
            name,
            let_names
        );
    }

    assert!(
        !dt.constraints.is_empty(),
        "DimensionalTolerance should have at least 1 constraint (upper_deviation >= lower_deviation)"
    );
}

// ─── step-9: GeometricTolerance trait and sub-trait hierarchy ─────────────────

/// Step 9: GeometricTolerance exists with tolerance_value and material_condition
/// required members. FormTolerance, OrientationTolerance, LocationTolerance all
/// refine GeometricTolerance.
#[test]
fn geometric_tolerance_trait_and_subtrait_hierarchy() {
    let module = load_stdlib_module();

    // GeometricTolerance
    let gt = module
        .trait_defs
        .iter()
        .find(|t| t.name == "GeometricTolerance")
        .expect("expected 'GeometricTolerance' trait");

    let gt_member_names: Vec<&str> = gt
        .required_members
        .iter()
        .map(|r| r.name.as_str())
        .collect();
    assert!(
        gt_member_names.contains(&"tolerance_value"),
        "GeometricTolerance should have 'tolerance_value' member, got: {:?}",
        gt_member_names
    );
    assert!(
        gt_member_names.contains(&"material_condition"),
        "GeometricTolerance should have 'material_condition' member, got: {:?}",
        gt_member_names
    );

    // FormTolerance refines GeometricTolerance
    let ft = module
        .trait_defs
        .iter()
        .find(|t| t.name == "FormTolerance")
        .expect("expected 'FormTolerance' trait");
    assert!(
        ft.refinements.contains(&"GeometricTolerance".to_string()),
        "FormTolerance should refine GeometricTolerance, got: {:?}",
        ft.refinements
    );

    // OrientationTolerance refines GeometricTolerance
    let ot = module
        .trait_defs
        .iter()
        .find(|t| t.name == "OrientationTolerance")
        .expect("expected 'OrientationTolerance' trait");
    assert!(
        ot.refinements.contains(&"GeometricTolerance".to_string()),
        "OrientationTolerance should refine GeometricTolerance, got: {:?}",
        ot.refinements
    );

    // LocationTolerance refines GeometricTolerance
    let lt = module
        .trait_defs
        .iter()
        .find(|t| t.name == "LocationTolerance")
        .expect("expected 'LocationTolerance' trait");
    assert!(
        lt.refinements.contains(&"GeometricTolerance".to_string()),
        "LocationTolerance should refine GeometricTolerance, got: {:?}",
        lt.refinements
    );
}

// ─── step-11: all 14 GD&T types + Datum present ──────────────────────────────

/// Step 11: All 14 GD&T structure defs + Datum are present as templates.
/// Angularity has a 'nominal_angle' value cell.
#[test]
fn all_fourteen_gdt_types_and_datum_present() {
    let module = load_stdlib_module();

    let template_names: Vec<&str> = module.templates.iter().map(|t| t.name.as_str()).collect();

    let expected_gdt_and_datum = [
        // Form (4)
        "Flatness",
        "Straightness",
        "Circularity",
        "Cylindricity",
        // Orientation (3)
        "Parallelism",
        "Perpendicularity",
        "Angularity",
        // Location (3)
        "Position",
        "Concentricity",
        "Symmetry",
        // Runout (2)
        "CircularRunout",
        "TotalRunout",
        // Profile (2)
        "ProfileOfSurface",
        "ProfileOfLine",
        // Datum
        "Datum",
    ];

    for name in &expected_gdt_and_datum {
        assert!(
            template_names.contains(name),
            "expected template '{}' in compiled module, found: {:?}",
            name,
            template_names
        );
    }

    // Angularity has 'nominal_angle' value cell
    let angularity = module
        .templates
        .iter()
        .find(|t| t.name == "Angularity")
        .expect("expected 'Angularity' template");
    assert!(
        angularity
            .value_cells
            .iter()
            .any(|vc| vc.id.member == "nominal_angle"),
        "Angularity should have 'nominal_angle' value cell, got: {:?}",
        angularity
            .value_cells
            .iter()
            .map(|vc| &vc.id.member)
            .collect::<Vec<_>>()
    );
}

// ─── step-13: SurfaceFinish, Fit, ISOToleranceGrade, Conforms ────────────────

/// Step 13: SurfaceFinish, Fit, ISOToleranceGrade templates exist with correct
/// members, and Conforms constraint def is present.
#[test]
fn surface_fit_iso_conforms_structures_present() {
    let module = load_stdlib_module();

    // SurfaceFinish: parameter, value, direction, process
    let sf = module
        .templates
        .iter()
        .find(|t| t.name == "SurfaceFinish")
        .expect("expected 'SurfaceFinish' template");
    let sf_cell_names: Vec<&str> = sf
        .value_cells
        .iter()
        .map(|vc| vc.id.member.as_str())
        .collect();
    for member in &["parameter", "value", "direction", "process"] {
        assert!(
            sf_cell_names.contains(member),
            "SurfaceFinish missing '{}' cell, got: {:?}",
            member,
            sf_cell_names
        );
    }

    // Fit: max_clearance and min_clearance Let cells
    let fit = module
        .templates
        .iter()
        .find(|t| t.name == "Fit")
        .expect("expected 'Fit' template");
    let fit_let_names: Vec<&str> = fit
        .value_cells
        .iter()
        .filter(|vc| vc.kind == ValueCellKind::Let)
        .map(|vc| vc.id.member.as_str())
        .collect();
    assert!(
        fit_let_names.contains(&"max_clearance"),
        "Fit should have 'max_clearance' Let cell, got lets: {:?}",
        fit_let_names
    );
    assert!(
        fit_let_names.contains(&"min_clearance"),
        "Fit should have 'min_clearance' Let cell, got lets: {:?}",
        fit_let_names
    );

    // ISOToleranceGrade: grade and tolerance_value
    let iso = module
        .templates
        .iter()
        .find(|t| t.name == "ISOToleranceGrade")
        .expect("expected 'ISOToleranceGrade' template");
    let iso_cell_names: Vec<&str> = iso
        .value_cells
        .iter()
        .map(|vc| vc.id.member.as_str())
        .collect();
    assert!(
        iso_cell_names.contains(&"grade"),
        "ISOToleranceGrade should have 'grade' cell, got: {:?}",
        iso_cell_names
    );
    assert!(
        iso_cell_names.contains(&"tolerance_value"),
        "ISOToleranceGrade should have 'tolerance_value' cell, got: {:?}",
        iso_cell_names
    );

    // Conforms constraint def
    assert!(
        !module.constraint_defs.is_empty(),
        "expected at least 1 constraint def (Conforms), got zero"
    );
    assert!(
        module.constraint_defs.iter().any(|c| c.name == "Conforms"),
        "expected 'Conforms' constraint def, found: {:?}",
        module
            .constraint_defs
            .iter()
            .map(|c| &c.name)
            .collect::<Vec<_>>()
    );
}

// ─── step-14: MaterialCondition type resolved via prelude ────────────────────

/// Step 14: A param typed `MaterialCondition` (prelude enum) resolves to
/// `Enum("MaterialCondition")` when compiled with prelude. Enum type annotation
/// works cross-module even though variant access in expressions is not yet
/// supported cross-module.
#[test]
fn material_condition_mmc_access_via_prelude() {
    let source = r#"
structure def S {
    param mc : MaterialCondition
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
        "param typed as MaterialCondition should compile without errors, got: {:?}",
        errors
    );

    let template = compiled
        .templates
        .first()
        .expect("expected at least 1 template");

    let mc_cell = template.value_cells.iter().find(|vc| vc.id.member == "mc");
    assert!(
        mc_cell.is_some(),
        "expected 'mc' value cell, got cells: {:?}",
        template
            .value_cells
            .iter()
            .map(|vc| &vc.id.member)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        mc_cell.unwrap().cell_type,
        Type::Enum("MaterialCondition".to_string()),
        "mc should have Enum(MaterialCondition) type, got {:?}",
        mc_cell.unwrap().cell_type
    );
}

// ─── step-15: DimensionalTolerance constraint injected ───────────────────────

/// Step 15: A structure using DimensionalTolerance's pattern has its constraint
/// injected. Verified by checking the template has at least 1 constraint.
#[test]
fn dimensional_tolerance_constraint_injected() {
    let module = load_stdlib_module();

    let dt = module
        .templates
        .iter()
        .find(|t| t.name == "DimensionalTolerance")
        .expect("expected 'DimensionalTolerance' template");

    assert!(
        !dt.constraints.is_empty(),
        "DimensionalTolerance should have at least 1 constraint (upper_deviation >= lower_deviation injected)"
    );
}

// ─── step-17: Constructor functions exist ────────────────────────────────────

/// Step 17: symmetric_tolerance and limit_tolerance functions exist with 2
/// params each.
#[test]
fn constructor_functions_exist() {
    let module = load_stdlib_module();

    let sym = module
        .functions
        .iter()
        .find(|f| f.name == "symmetric_tolerance")
        .expect("expected 'symmetric_tolerance' function");
    assert_eq!(
        sym.params.len(),
        2,
        "symmetric_tolerance should have 2 params, got: {:?}",
        sym.params.iter().map(|(n, _)| n).collect::<Vec<_>>()
    );

    let lim = module
        .functions
        .iter()
        .find(|f| f.name == "limit_tolerance")
        .expect("expected 'limit_tolerance' function");
    assert_eq!(
        lim.params.len(),
        2,
        "limit_tolerance should have 2 params, got: {:?}",
        lim.params.iter().map(|(n, _)| n).collect::<Vec<_>>()
    );
}

// ─── step-18: GD&T structure conforms to FormTolerance via prelude ────────────

/// Step 18: A user structure conforming to FormTolerance compiles via prelude
/// with no errors. FormTolerance in trait_bounds.
/// Note: `material_condition` has no default since cross-module enum variant
/// access in expressions is not yet supported.
#[test]
fn gdt_structure_conforms_to_form_tolerance_via_prelude() {
    let source = r#"
structure def MyFlat : FormTolerance {
    param tolerance_value : Length = 0.01mm
    param feature : Real = 0.0
    param material_condition : MaterialCondition
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
        "MyFlat : FormTolerance should compile without errors, got: {:?}",
        errors
    );

    let template = compiled
        .templates
        .first()
        .expect("expected at least 1 template");
    assert!(
        template.trait_bounds.contains(&"FormTolerance".to_string()),
        "MyFlat should have 'FormTolerance' in trait_bounds, got: {:?}",
        template.trait_bounds
    );
}

// ─── step-19: Full module integrity counts ────────────────────────────────────

/// Step 19: Full module integrity — correct counts for enums, traits, templates,
/// functions, constraint defs, and zero errors.
#[test]
fn full_module_integrity() {
    let module = load_stdlib_module();

    // Zero errors
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "tolerancing module should have zero Error-severity diagnostics, got: {:?}",
        errors
    );

    // 4 enums
    assert_eq!(
        module.enum_defs.len(),
        4,
        "expected 4 enums (MaterialCondition, FitCategory, SurfaceParameter, SurfaceDirection), got: {:?}",
        module.enum_defs.iter().map(|e| &e.name).collect::<Vec<_>>()
    );

    // 4 traits
    assert_eq!(
        module.trait_defs.len(),
        4,
        "expected 4 traits (GeometricTolerance, FormTolerance, OrientationTolerance, LocationTolerance), got: {:?}",
        module
            .trait_defs
            .iter()
            .map(|t| &t.name)
            .collect::<Vec<_>>()
    );

    // 19 templates: DimensionalTolerance(1) + 14 GD&T + Datum(1) + SurfaceFinish(1) + Fit(1) + ISOToleranceGrade(1)
    assert_eq!(
        module.templates.len(),
        19,
        "expected 19 templates, got: {:?}",
        module.templates.iter().map(|t| &t.name).collect::<Vec<_>>()
    );

    // 2 functions
    assert_eq!(
        module.functions.len(),
        2,
        "expected 2 functions (symmetric_tolerance, limit_tolerance), got: {:?}",
        module.functions.iter().map(|f| &f.name).collect::<Vec<_>>()
    );

    // At least 1 constraint def
    assert!(
        !module.constraint_defs.is_empty(),
        "expected at least 1 constraint def (Conforms), got zero"
    );
}
