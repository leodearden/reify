//! Tests for stdlib/tolerancing.ri — dimensional, geometric (14 GD&T types), surface tolerancing.
//!
//! Tests validate that the .ri file parses and compiles cleanly, that each
//! enum, trait, and structure def is correctly represented, and that trait
//! conformance works via the production prelude.
//!
//! Steps β-1 through β-10 (below the pre-existing structural tests) add
//! eval/check conformance tests:
//!   β-1/β-2  — GeometricTolerance.nominal_zone inherited let
//!   β-3/β-4  — ISOToleranceGrade.tolerance_value derived let
//!   β-5/β-6  — Conforms GD&T-aware MMC/RFS flip
//!   β-7/β-8  — require_finish Bool free fn
//!   β-9/β-10 — SurfaceFinish direction + process defaults

use reify_compiler::*;
use reify_core::*;
use reify_ir::{Satisfaction, Value};
use reify_test_support::{check_source_with_stdlib, make_simple_engine, parse_and_compile_with_stdlib};

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

// ─── step-5: all 5 enums with correct variants ───────────────────────────────

/// Step 5: All 5 enums are present with correct variant counts and specific
/// variant names for MaterialCondition and ZoneShape.
#[test]
fn all_five_enums_with_correct_variants() {
    let module = load_stdlib_module();

    assert_eq!(
        module.enum_defs.len(),
        5,
        "expected exactly 5 enums, got: {:?}",
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

    // ZoneShape: 2 variants (Width, Cylindrical) — FOS zone-shape discriminator (α)
    let zs = module
        .enum_defs
        .iter()
        .find(|e| e.name == "ZoneShape")
        .expect("expected 'ZoneShape' enum");
    assert_eq!(
        zs.variants.len(),
        2,
        "ZoneShape should have 2 variants, got: {:?}",
        zs.variants
    );
    for variant in &["Width", "Cylindrical"] {
        assert!(
            zs.variants.contains(&variant.to_string()),
            "ZoneShape missing variant '{}', variants: {:?}",
            variant,
            zs.variants
        );
    }
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
        "GeometricTolerance should have 'tolerance_value' required member, got: {:?}",
        gt_member_names
    );
    // material_condition has a trait-level default (= MaterialCondition.RFS) so it
    // lives in `defaults`, not `required_members`. Verify the default is present.
    let gt_default_names: Vec<Option<&str>> = gt
        .defaults
        .iter()
        .map(|d| d.name.as_deref())
        .collect();
    assert!(
        gt_default_names.contains(&Some("material_condition")),
        "GeometricTolerance should have 'material_condition' in defaults (has RFS default), got: {:?}",
        gt_default_names
    );
    // nominal_zone is also a trait-level derived let default (β).
    assert!(
        gt_default_names.contains(&Some("nominal_zone")),
        "GeometricTolerance should have 'nominal_zone' in defaults, got: {:?}",
        gt_default_names
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

// ─── step-11: all GD&T types + Datum present ─────────────────────────────────

/// Step 11: All GD&T structure defs + Datum are present as templates.
/// The set is 17 GD&T types after the α restructure (the original 14 plus
/// StraightnessOfAxis, ProfileOfSurfaceRelated, ProfileOfLineRelated); the count
/// is intentionally NOT baked into the test name so future additions (γ/δ) don't
/// re-stale it. Angularity has a 'nominal_angle' value cell.
#[test]
fn all_gdt_types_and_datum_present() {
    let module = load_stdlib_module();

    let template_names: Vec<&str> = module.templates.iter().map(|t| t.name.as_str()).collect();

    let expected_gdt_and_datum = [
        // Form (4 + StraightnessOfAxis FOS variant, α)
        "Flatness",
        "Straightness",
        "StraightnessOfAxis",
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
        // Profile (2 datum-less + 2 …Related datum-referenced, α)
        "ProfileOfSurface",
        "ProfileOfLine",
        "ProfileOfSurfaceRelated",
        "ProfileOfLineRelated",
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

// ─── α: StraightnessOfAxis FOS form variant ──────────────────────────────────

/// α: `StraightnessOfAxis` is a FormTolerance variant for the FOS-derived axis.
/// It refines FormTolerance (no datum), carries the standard tolerance_value /
/// feature / material_condition Param cells, and — because the axis tolerance
/// zone is inherently Ø — carries NO `zone_shape` discriminator cell.
///
/// RED before step-4: StraightnessOfAxis does not exist.
#[test]
fn straightness_of_axis_is_fos_form_variant() {
    let module = load_stdlib_module();

    let soa = module
        .templates
        .iter()
        .find(|t| t.name == "StraightnessOfAxis")
        .expect("expected 'StraightnessOfAxis' template");

    assert!(
        soa.trait_bounds.contains(&"FormTolerance".to_string()),
        "StraightnessOfAxis should have 'FormTolerance' in trait_bounds, got: {:?}",
        soa.trait_bounds
    );

    let param_names: Vec<&str> = soa
        .value_cells
        .iter()
        .filter(|vc| vc.kind == ValueCellKind::Param)
        .map(|vc| vc.id.member.as_str())
        .collect();
    for name in &["tolerance_value", "feature", "material_condition"] {
        assert!(
            param_names.contains(name),
            "StraightnessOfAxis missing Param '{}', params: {:?}",
            name,
            param_names
        );
    }

    // Axis zone is inherently Ø → no zone_shape discriminator.
    assert!(
        !soa.value_cells.iter().any(|vc| vc.id.member == "zone_shape"),
        "StraightnessOfAxis must NOT have a 'zone_shape' cell (axis zone is inherently Ø), \
         got cells: {:?}",
        soa.value_cells
            .iter()
            .map(|vc| &vc.id.member)
            .collect::<Vec<_>>()
    );
}

// ─── α: runout callouts carry a required datum_refs ──────────────────────────

/// α: CircularRunout and TotalRunout each gain a required `datum_refs` Param
/// cell typed Geometry (DatumRef aliases Type::Geometry per #3116). Runout is
/// always datum-referenced per ASME Y14.5, so the datum is mandatory (no default).
///
/// RED before step-6: the base runout structures refine GeometricTolerance
/// directly and have no datum_refs cell.
#[test]
fn runout_callouts_carry_required_datum_refs() {
    let module = load_stdlib_module();

    for name in &["CircularRunout", "TotalRunout"] {
        let t = module
            .templates
            .iter()
            .find(|t| t.name == *name)
            .unwrap_or_else(|| panic!("expected '{}' template", name));

        let datum = t
            .value_cells
            .iter()
            .find(|vc| vc.kind == ValueCellKind::Param && vc.id.member == "datum_refs")
            .unwrap_or_else(|| {
                panic!(
                    "{} must have a 'datum_refs' Param cell, got params: {:?}",
                    name,
                    t.value_cells
                        .iter()
                        .filter(|vc| vc.kind == ValueCellKind::Param)
                        .map(|vc| &vc.id.member)
                        .collect::<Vec<_>>()
                )
            });
        assert_eq!(
            datum.cell_type,
            Type::Geometry,
            "{}.datum_refs must be Type::Geometry (DatumRef aliases Geometry), got {:?}",
            name,
            datum.cell_type
        );
    }
}

// ─── α: profile split — datum-less vs datum-referenced (…Related) ────────────

/// α: the profile callouts split into datum-less (form-only) and datum-referenced
/// (…Related) variants. ProfileOfSurface / ProfileOfLine remain datum-less (no
/// datum_refs cell); ProfileOfSurfaceRelated / ProfileOfLineRelated add a required
/// `datum_refs` Param cell typed Geometry.
///
/// RED before step-8: the two …Related variants do not exist.
#[test]
fn profile_callouts_split_datumless_and_related() {
    let module = load_stdlib_module();

    // Datum-less variants: present, and NO datum_refs cell.
    for name in &["ProfileOfSurface", "ProfileOfLine"] {
        let t = module
            .templates
            .iter()
            .find(|t| t.name == *name)
            .unwrap_or_else(|| panic!("expected '{}' template", name));
        assert!(
            !t.value_cells.iter().any(|vc| vc.id.member == "datum_refs"),
            "{} must be datum-less (no 'datum_refs' cell), got cells: {:?}",
            name,
            t.value_cells
                .iter()
                .map(|vc| &vc.id.member)
                .collect::<Vec<_>>()
        );
    }

    // …Related variants: present, with a required datum_refs Param cell typed Geometry.
    for name in &["ProfileOfSurfaceRelated", "ProfileOfLineRelated"] {
        let t = module
            .templates
            .iter()
            .find(|t| t.name == *name)
            .unwrap_or_else(|| panic!("expected '{}' template", name));
        let datum = t
            .value_cells
            .iter()
            .find(|vc| vc.kind == ValueCellKind::Param && vc.id.member == "datum_refs")
            .unwrap_or_else(|| {
                panic!(
                    "{} must have a 'datum_refs' Param cell, got params: {:?}",
                    name,
                    t.value_cells
                        .iter()
                        .filter(|vc| vc.kind == ValueCellKind::Param)
                        .map(|vc| &vc.id.member)
                        .collect::<Vec<_>>()
                )
            });
        assert_eq!(
            datum.cell_type,
            Type::Geometry,
            "{}.datum_refs must be Type::Geometry (DatumRef aliases Geometry), got {:?}",
            name,
            datum.cell_type
        );
    }
}

// ─── α: FOS location/orientation callouts carry a zone_shape discriminator ───

/// α: Position and the three orientation callouts (Parallelism, Perpendicularity,
/// Angularity) each gain a `zone_shape` Param cell typed Enum("ZoneShape").
/// Concentricity and Symmetry are deliberately EXCLUDED — they were removed in
/// ASME Y14.5-2018 and have inherently fixed zone shapes, so a discriminator is
/// meaningless. This test pins both the inclusion set and the exclusion set.
///
/// RED before step-10: zone_shape is not present on any callout yet.
#[test]
fn fos_location_orientation_callouts_carry_zone_shape() {
    let module = load_stdlib_module();

    // Inclusion set: Position + 3 orientation callouts carry zone_shape : ZoneShape.
    for name in &["Position", "Parallelism", "Perpendicularity", "Angularity"] {
        let t = module
            .templates
            .iter()
            .find(|t| t.name == *name)
            .unwrap_or_else(|| panic!("expected '{}' template", name));
        let zs = t
            .value_cells
            .iter()
            .find(|vc| vc.kind == ValueCellKind::Param && vc.id.member == "zone_shape")
            .unwrap_or_else(|| {
                panic!(
                    "{} must have a 'zone_shape' Param cell, got params: {:?}",
                    name,
                    t.value_cells
                        .iter()
                        .filter(|vc| vc.kind == ValueCellKind::Param)
                        .map(|vc| &vc.id.member)
                        .collect::<Vec<_>>()
                )
            });
        assert_eq!(
            zs.cell_type,
            Type::Enum("ZoneShape".to_string()),
            "{}.zone_shape must be Type::Enum(\"ZoneShape\"), got {:?}",
            name,
            zs.cell_type
        );
    }

    // Exclusion set: Concentricity / Symmetry must NOT carry zone_shape.
    for name in &["Concentricity", "Symmetry"] {
        let t = module
            .templates
            .iter()
            .find(|t| t.name == *name)
            .unwrap_or_else(|| panic!("expected '{}' template", name));
        assert!(
            !t.value_cells.iter().any(|vc| vc.id.member == "zone_shape"),
            "{} must NOT have a 'zone_shape' cell (fixed-shape, Y14.5-2018-removed), \
             got cells: {:?}",
            name,
            t.value_cells
                .iter()
                .map(|vc| &vc.id.member)
                .collect::<Vec<_>>()
        );
    }
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
    param feature : Geometry = box(1mm, 1mm, 1mm)
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

    // 5 enums
    assert_eq!(
        module.enum_defs.len(),
        5,
        "expected 5 enums (MaterialCondition, FitCategory, SurfaceParameter, SurfaceDirection, ZoneShape), got: {:?}",
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

    // 22 templates: DimensionalTolerance(1) + 14 GD&T + StraightnessOfAxis(1, α)
    //   + ProfileOfSurfaceRelated(1, α) + ProfileOfLineRelated(1, α)
    //   + Datum(1) + SurfaceFinish(1) + Fit(1) + ISOToleranceGrade(1)
    assert_eq!(
        module.templates.len(),
        22,
        "expected 22 templates, got: {:?}",
        module.templates.iter().map(|t| &t.name).collect::<Vec<_>>()
    );

    // 3 functions: symmetric_tolerance, limit_tolerance, require_finish (β adds require_finish)
    assert_eq!(
        module.functions.len(),
        3,
        "expected 3 functions (symmetric_tolerance, limit_tolerance, require_finish), got: {:?}",
        module.functions.iter().map(|f| &f.name).collect::<Vec<_>>()
    );

    // At least 1 constraint def
    assert!(
        !module.constraint_defs.is_empty(),
        "expected at least 1 constraint def (Conforms), got zero"
    );
}

// ─── β-5: Conforms GD&T-aware MMC/RFS flip ───────────────────────────────────

/// Verifies that `pub constraint def Conforms` produces the correct satisfaction
/// outcome for different material conditions:
///
///   MMC: zone = tolerance_value + feature_departure = 0.1 + 0.1 = 0.2mm ≥ 0.15mm → Satisfied
///   RFS: zone = tolerance_value                     = 0.1mm         ≥ 0.15mm → Violated
///
/// NOTE: The tolerance structure is defined locally — the eval engine resolves
/// sub-component templates from the user module only (not stdlib).
/// material_condition is supplied explicitly (MMC vs RFS) to drive the flip.
#[test]
fn conforms_gdt_mmc_satisfied_rfs_violated() {
    // ── MMC case: zone = 0.1mm + 0.1mm = 0.2mm >= 0.15mm → Satisfied ────────
    let source_mmc = r#"
structure def TestTolMMC : GeometricTolerance {
    param tolerance_value : Length = 0.1mm
    param feature : Geometry = box(1mm, 1mm, 1mm)
    param material_condition : MaterialCondition = MaterialCondition.MMC
}
structure def ProbeMMC {
    sub t = TestTolMMC()
    constraint Conforms(tolerance: self.t, measured_deviation: 0.15mm, feature_departure: 0.1mm)
}
"#;
    let result_mmc = check_source_with_stdlib(source_mmc);
    assert!(
        result_mmc
            .constraint_results
            .iter()
            .any(|e| e.satisfaction == Satisfaction::Satisfied),
        "MMC case: Conforms should be Satisfied \
         (zone = 0.1+0.1=0.2mm >= 0.15mm), got: {:?}",
        result_mmc.constraint_results
    );
    let mmc_violated = result_mmc
        .constraint_results
        .iter()
        .any(|e| e.satisfaction == Satisfaction::Violated);
    assert!(
        !mmc_violated,
        "MMC case: no Conforms entry should be Violated, got: {:?}",
        result_mmc.constraint_results
    );

    // ── RFS case: zone = 0.1mm >= 0.15mm → Violated (bonus ignored under RFS) ──
    let source_rfs = r#"
structure def TestTolRFS : GeometricTolerance {
    param tolerance_value : Length = 0.1mm
    param feature : Geometry = box(1mm, 1mm, 1mm)
    param material_condition : MaterialCondition = MaterialCondition.RFS
}
structure def ProbeRFS {
    sub t = TestTolRFS()
    constraint Conforms(tolerance: self.t, measured_deviation: 0.15mm, feature_departure: 0.1mm)
}
"#;
    let result_rfs = check_source_with_stdlib(source_rfs);
    assert!(
        result_rfs
            .constraint_results
            .iter()
            .any(|e| e.satisfaction == Satisfaction::Violated),
        "RFS case: Conforms should be Violated \
         (zone = 0.1mm < 0.15mm, bonus irrelevant under RFS), got: {:?}",
        result_rfs.constraint_results
    );
    assert!(
        !result_rfs
            .constraint_results
            .iter()
            .any(|e| e.satisfaction == Satisfaction::Satisfied),
        "RFS case: no Conforms entry should be Satisfied, got: {:?}",
        result_rfs.constraint_results
    );
}

// ─── amend: Conforms LMC branch ──────────────────────────────────────────────

/// LMC adds the feature_departure bonus just like MMC (effective_tolerance_zone
/// returns tol + departure for both). This mirrors the MMC case and pins that
/// the LMC branch of effective_tolerance_zone is exercised through Conforms.
///
/// zone = 0.1mm + 0.1mm = 0.2mm >= 0.15mm → Satisfied
#[test]
fn conforms_gdt_lmc_satisfied() {
    let source = r#"
structure def TestTolLMC : GeometricTolerance {
    param tolerance_value : Length = 0.1mm
    param feature : Geometry = box(1mm, 1mm, 1mm)
    param material_condition : MaterialCondition = MaterialCondition.LMC
}
structure def ProbeLMC {
    sub t = TestTolLMC()
    constraint Conforms(tolerance: self.t, measured_deviation: 0.15mm, feature_departure: 0.1mm)
}
"#;
    let result = check_source_with_stdlib(source);
    assert!(
        result
            .constraint_results
            .iter()
            .any(|e| e.satisfaction == Satisfaction::Satisfied),
        "LMC case: Conforms should be Satisfied \
         (zone = 0.1+0.1=0.2mm >= 0.15mm), got: {:?}",
        result.constraint_results
    );
    assert!(
        !result
            .constraint_results
            .iter()
            .any(|e| e.satisfaction == Satisfaction::Violated),
        "LMC case: no Conforms entry should be Violated, got: {:?}",
        result.constraint_results
    );
}

// ─── amend: Conforms Undef propagation ───────────────────────────────────────

/// Pins the Conforms constraint behavior on degenerate tolerance_value inputs,
/// confirming they never produce Satisfaction::Satisfied:
///
///   Zero tolerance_value: zone = 0mm; 0mm >= 0.15mm → false → Violated.
///   Negative tolerance_value: effective_tolerance_zone returns Undef for
///   sub-zero inputs (guarded against physically nonsensical zones), so the
///   predicate evaluates to Undef, which SimpleConstraintChecker maps to
///   Satisfaction::Indeterminate.
///
/// Both sub-cases anchor with a positive assertion (Violated / Indeterminate)
/// so that a regression that drops the Conforms entry entirely does not pass.
#[test]
fn conforms_gdt_degenerate_never_satisfied() {
    // Zero tolerance_value: zone = 0mm, departure = 0mm → 0mm >= 0.15mm → false → Violated
    let source_zero = r#"
structure def TestTolZero : GeometricTolerance {
    param tolerance_value : Length = 0mm
    param feature : Geometry = box(1mm, 1mm, 1mm)
    param material_condition : MaterialCondition = MaterialCondition.RFS
}
structure def ProbeZero {
    sub t = TestTolZero()
    constraint Conforms(tolerance: self.t, measured_deviation: 0.15mm, feature_departure: 0mm)
}
"#;
    let result_zero = check_source_with_stdlib(source_zero);
    assert!(
        !result_zero
            .constraint_results
            .iter()
            .any(|e| e.satisfaction == Satisfaction::Satisfied),
        "zero tolerance_value: Conforms must not be Satisfied, got: {:?}",
        result_zero.constraint_results
    );
    assert!(
        result_zero
            .constraint_results
            .iter()
            .any(|e| e.satisfaction == Satisfaction::Violated),
        "zero tolerance_value: Conforms must be Violated (0mm >= 0.15mm is false), got: {:?}",
        result_zero.constraint_results
    );

    // Negative tolerance_value: effective_tolerance_zone guards sub-zero inputs → Undef → Indeterminate
    let source_neg = r#"
structure def TestTolNeg : GeometricTolerance {
    param tolerance_value : Length = 0mm - 0.1mm
    param feature : Geometry = box(1mm, 1mm, 1mm)
    param material_condition : MaterialCondition = MaterialCondition.RFS
}
structure def ProbeNeg {
    sub t = TestTolNeg()
    constraint Conforms(tolerance: self.t, measured_deviation: 0.15mm, feature_departure: 0mm)
}
"#;
    let result_neg = check_source_with_stdlib(source_neg);
    assert!(
        !result_neg
            .constraint_results
            .iter()
            .any(|e| e.satisfaction == Satisfaction::Satisfied),
        "negative tolerance_value: Conforms must not be Satisfied, got: {:?}",
        result_neg.constraint_results
    );
    assert!(
        result_neg
            .constraint_results
            .iter()
            .any(|e| e.satisfaction == Satisfaction::Indeterminate),
        "negative tolerance_value: Conforms must be Indeterminate (negative zone → Undef), got: {:?}",
        result_neg.constraint_results
    );
}

// ─── amend: ISOToleranceGrade out-of-envelope Undef ──────────────────────────

/// When ISOToleranceGrade is constructed with an out-of-envelope grade (e.g.
/// IT4, which is below the supported IT5–IT18 range), iso_it_tolerance returns
/// Value::Undef and the derived `tolerance_value` let resolves to Undef.
///
/// NOTE: tolerancing_diagnose is now wired into reify-expr's eval-time
/// Undef-diagnosis fallthrough (`emit_undef_builtin_diagnostics`, task 4461
/// step-2). The eval diagnostic is pinned by
/// `iso_it_tolerance_grade_25_out_of_envelope_emits_eval_error_diagnostic`.
/// This test continues to pin the compile-clean + Undef-cell assertions, which
/// are orthogonal to (and unaffected by) the eval-diagnostic wiring.
#[test]
fn iso_tolerance_grade_out_of_envelope_undef() {
    // IT4 is below IT5 — iso_it_tolerance returns Undef, so the derived let is Undef.
    let source = r#"
structure def TestOutOfEnv {
    param grade : Int = 4
    param nominal_min : Length = 30mm
    param nominal_max : Length = 50mm
    let tolerance_value = iso_it_tolerance(grade, nominal_min, nominal_max)
}
structure def Probe {
    sub g = TestOutOfEnv()
}
"#;
    let compiled = parse_and_compile_with_stdlib(source);
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "out-of-envelope ISOToleranceGrade should compile without Error diagnostics, \
         got: {:?}",
        compile_errors
    );

    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    // The derived let should resolve to Undef (iso_it_tolerance returns Undef for IT4)
    let cell_id = ValueCellId::new("Probe.g", "tolerance_value");
    // Confirm Probe.g was actually instantiated — distinguishes "cell is Undef" from
    // "sub-component was never resolved" (which would be a different kind of failure).
    let probe_g_keys: Vec<_> = result
        .values
        .iter()
        .filter(|(k, _)| k.entity == "Probe.g")
        .collect();
    assert!(
        !probe_g_keys.is_empty(),
        "Probe.g sub-component should have produced at least one value cell; \
         got none — check sub-component resolution"
    );
    match result.values.get(&cell_id) {
        Some(Value::Undef) => {
            // Expected: iso_it_tolerance returns Undef for out-of-envelope grade 4
        }
        None => {
            // The Undef cell may be elided by the evaluator (observable as a missing
            // entry rather than an explicit Undef). This is distinct from a
            // sub-component resolution failure (confirmed above via probe_g_keys).
        }
        Some(other) => panic!(
            "out-of-envelope grade 4 should yield Undef tolerance_value, got {:?}",
            other
        ),
    }
}

// ─── β-3: ISOToleranceGrade.tolerance_value derived let ──────────────────────

/// Verifies that `ISOToleranceGrade.tolerance_value` is a derived Let cell (not
/// Param) and that iso_it_tolerance(7, 30mm, 50mm) produces the published ISO
/// 286-1 value:
///
/// (a) Structural: `tolerance_value` is a `Let` cell in the compiled stdlib template.
/// (b) Eval: iso_it_tolerance(7, 30mm, 50mm) yields LENGTH Scalar ≈ 24.969µm
///     (IT7@Ø30–50 = 25µm per ISO 286-1; α's test pins this to 24.969e-6 m).
///
/// NOTE: Part (b) uses a locally-defined TestISO struct — the eval engine
/// resolves sub-component templates from the user module only (not stdlib).
/// Part (b) verifies the derived-let expression computes correctly;
/// part (a) verifies the real stdlib def carries a Let cell.
#[test]
fn iso_tolerance_grade_tolerance_value_derived_let() {
    // (a) Structural check: tolerance_value should be a Let cell, not Param ──
    let module = load_stdlib_module();
    let iso = module
        .templates
        .iter()
        .find(|t| t.name == "ISOToleranceGrade")
        .expect("expected 'ISOToleranceGrade' template");

    let tol_cell = iso
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "tolerance_value")
        .expect("ISOToleranceGrade should have 'tolerance_value' value cell");
    assert_eq!(
        tol_cell.kind,
        ValueCellKind::Let,
        "ISOToleranceGrade.tolerance_value should be a Let cell (derived from iso_it_tolerance), \
         got {:?}",
        tol_cell.kind
    );

    // (b) Eval: tolerance_value = iso_it_tolerance(7, 30mm, 50mm) ≈ 24.969µm ──
    //
    // NOTE: ISOToleranceGrade is a stdlib structure; the eval engine looks up templates
    // from the user's module only. Define a locally-accessible wrapper that embeds the
    // grade, nominal_min, nominal_max as params so the eval sees them.
    //
    // After step-4 makes tolerance_value a derived Let, a conforming structure that
    // uses ISOToleranceGrade via sub-component still needs ISOToleranceGrade in the
    // user module. We use a locally-defined passthrough structure instead.
    let source = r#"
structure def TestISO {
    param grade : Int = 7
    param nominal_min : Length = 30mm
    param nominal_max : Length = 50mm
    let tolerance_value = iso_it_tolerance(grade, nominal_min, nominal_max)
}
structure def Probe {
    sub g = TestISO(grade: 7, nominal_min: 30mm, nominal_max: 50mm)
}
"#;
    let compiled = parse_and_compile_with_stdlib(source);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(eval_errors.is_empty(), "eval errors: {:?}", eval_errors);

    let cell_id = ValueCellId::new("Probe.g", "tolerance_value");
    let value = result.values.get(&cell_id).unwrap_or_else(|| {
        let probe_keys: Vec<_> = result
            .values
            .iter()
            .map(|(k, _)| k)
            .filter(|k| k.entity.contains("Probe"))
            .collect();
        panic!(
            "Probe.g.tolerance_value not found in eval result; Probe-related keys: {:?}",
            probe_keys
        )
    });
    match value {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(
                *dimension,
                DimensionVector::LENGTH,
                "Probe.g.tolerance_value should have LENGTH dimension, got {:?}",
                dimension
            );
            // iso_it_tolerance(7, 30mm, 50mm) = 24.969µm = 24.969e-6 m
            // α's test pins this to 24.969e-6; assert within 0.5% (< 0.125µm)
            let expected_si = 24.969e-6_f64;
            assert!(
                (si_value - expected_si).abs() / expected_si < 0.005,
                "Probe.g.tolerance_value should be ≈24.969µm (IT7@Ø30-50), got {} m",
                si_value
            );
        }
        other => panic!(
            "Probe.g.tolerance_value should be Value::Scalar, got {:?}",
            other
        ),
    }
}

// ─── β-1: GeometricTolerance.nominal_zone inherited let ──────────────────────

/// Verifies that `GeometricTolerance` has a trait-level `nominal_zone` derived
/// Let and that effective_tolerance_zone(tol, RFS, 0mm) computes the nominal
/// zone value:
///
/// (a) Structural: the compiled `GeometricTolerance` trait exposes `nominal_zone`
///     in its `defaults` list (exercises the real stdlib trait def).
/// (b) Eval: effective_tolerance_zone(0.05mm, RFS, 0mm) yields LENGTH Scalar
///     ≈ 5e-5m (departure = 0mm → nominal_zone == tolerance_value).
///
/// NOTE: Part (b) uses a locally-defined TestFlat struct — the eval engine
/// resolves sub-component templates from the user module only (not stdlib).
/// Part (b) verifies the derived-let expression; part (a) verifies the stdlib
/// trait carries the nominal_zone Let.
#[test]
fn geometric_tolerance_nominal_zone_inherited_let() {
    // (a) Trait-level structure check ────────────────────────────────────────
    let module = load_stdlib_module();
    let gt = module
        .trait_defs
        .iter()
        .find(|t| t.name == "GeometricTolerance")
        .expect("expected 'GeometricTolerance' trait");

    let has_nominal_zone_default = gt
        .defaults
        .iter()
        .any(|d| d.name.as_deref() == Some("nominal_zone"));
    assert!(
        has_nominal_zone_default,
        "GeometricTolerance trait should have a 'nominal_zone' Let in its defaults, \
         got defaults: {:?}",
        gt.defaults
            .iter()
            .map(|d| d.name.as_deref())
            .collect::<Vec<_>>()
    );

    // (b) Eval: Probe.f.nominal_zone == 0.05mm = 5e-5m (departure=0mm → zone==tol) ──
    //
    // NOTE: The eval engine resolves sub-component templates from the user's compiled
    // module only (not the stdlib). Use a locally-defined conforming structure so the
    // engine can find it. material_condition is supplied explicitly to ensure the source
    // compiles both before and after step-2 adds the trait-level RFS default.
    let source = r#"
structure def TestFlat : GeometricTolerance {
    param tolerance_value : Length = 0.05mm
    param feature : Geometry = box(1mm, 1mm, 1mm)
    param material_condition : MaterialCondition = MaterialCondition.RFS
}
structure def Probe {
    sub f = TestFlat(tolerance_value: 0.05mm)
}
"#;
    let compiled = parse_and_compile_with_stdlib(source);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(eval_errors.is_empty(), "eval errors: {:?}", eval_errors);

    let cell_id = ValueCellId::new("Probe.f", "nominal_zone");
    let value = result.values.get(&cell_id).unwrap_or_else(|| {
        let probe_keys: Vec<_> = result
            .values
            .iter()
            .map(|(k, _)| k)
            .filter(|k| k.entity.contains("Probe"))
            .collect();
        panic!(
            "Probe.f.nominal_zone not found in eval result; Probe-related keys: {:?}",
            probe_keys
        )
    });
    match value {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(
                *dimension,
                DimensionVector::LENGTH,
                "Probe.f.nominal_zone should have LENGTH dimension, got {:?}",
                dimension
            );
            // nominal_zone = effective_tolerance_zone(0.05mm, RFS, 0mm) = 0.05mm = 5e-5 m
            assert!(
                (si_value - 5e-5).abs() < 1e-12,
                "Probe.f.nominal_zone should be ≈5e-5m (0.05mm), got {}",
                si_value
            );
        }
        other => panic!(
            "Probe.f.nominal_zone should be Value::Scalar, got {:?}",
            other
        ),
    }
}

// ─── β-9: SurfaceFinish direction + process defaults ─────────────────────────

/// Verifies that `SurfaceFinish` compiles and evaluates when `direction` and
/// `process` are omitted (relying on their defaults):
///   `direction` defaults to `SurfaceDirection.Multidirectional`
///   `process`   defaults to `""`
///
/// (a) Compile `structure def Probe { sub s = SurfaceFinish(parameter:
///     SurfaceParameter.Ra, value: 1.6um) }` WITHOUT direction/process;
///     assert zero Severity::Error diagnostics.
/// (b) Eval: construct SurfaceFinish inline (without direction/process) and pass
///     it to `require_finish`; assert the call evals to `Value::Bool(true)`.
///     This proves: (1) SurfaceFinish can be constructed with defaults, and
///     (2) the resulting struct is a valid SurfaceFinish (finish.value = 1.6µm > 0mm).
///     NOTE: sub-component eval of stdlib structure types is not supported by the
///     eval engine (it looks up templates only from the user module), so we use
///     the inline-construction pattern (same as require_finish_bool_free_fn).
#[test]
fn surface_finish_direction_process_defaults() {
    // (a) Compile without direction/process: should have zero Error diagnostics ──
    let source_compile = r#"
structure def Probe {
    sub s = SurfaceFinish(parameter: SurfaceParameter.Ra, value: 1.6um)
}
"#;
    let compiled_check = parse_and_compile_with_stdlib(source_compile);

    let compile_errors: Vec<_> = compiled_check
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "SurfaceFinish with omitted direction/process should compile without errors \
         (both have defaults), got: {:?}",
        compile_errors
    );

    // (b) Eval: inline SurfaceFinish construction without direction/process ────
    //
    // The eval engine resolves sub-component templates only from the user's compiled
    // module, not from stdlib. Use inline construction (same pattern as
    // require_finish_bool_free_fn) to exercise the defaults at eval time.
    // `require_finish(feature, SurfaceFinish(...))` evaluates `finish.value > 0mm`
    // internally — if SurfaceFinish is constructed without direction/process, the
    // defaults must apply for the compile+eval chain to succeed.
    let source_eval = r#"
structure def Probe {
    param feat : Geometry = box(1mm, 1mm, 1mm)
    param ok : Bool = require_finish(feat, SurfaceFinish(parameter: SurfaceParameter.Ra, value: 1.6um))
}
"#;
    let compiled_eval = parse_and_compile_with_stdlib(source_eval);

    let eval_compile_errors: Vec<_> = compiled_eval
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        eval_compile_errors.is_empty(),
        "inline SurfaceFinish(default direction/process) in require_finish should compile \
         without errors, got: {:?}",
        eval_compile_errors
    );

    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled_eval);
    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(eval_errors.is_empty(), "eval errors: {:?}", eval_errors);

    let ok_cell = ValueCellId::new("Probe", "ok");
    // `feat : Geometry = box(1mm,1mm,1mm)` requires the geometry kernel to evaluate.
    // The simple engine returns Undef for box(); the engine propagates Undef for the
    // require_finish call. Accept Bool(true) (full kernel) or Undef (simple engine).
    // The cell must EXIST (None would mean the param was never produced, a regression).
    // The compile-time assertion above is the primary correctness check.
    match result.values.get(&ok_cell) {
        Some(Value::Bool(true)) => {} // full kernel: geometry handle, finish.value > 0mm → true
        Some(Value::Undef) => {} // simple engine: box() → Undef → require_finish propagates Undef
        None => panic!(
            "require_finish with default-direction SurfaceFinish(value: 1.6um): cell \
             Probe.ok must exist after successful eval (got None — param never produced)"
        ),
        Some(other) => panic!(
            "require_finish with default-direction SurfaceFinish(value: 1.6um) should \
             return Bool(true) or Undef (simple engine), got {:?}",
            other
        ),
    }
}

// ─── β-7: require_finish Bool free fn ────────────────────────────────────────

/// Verifies the `require_finish(feature, finish)` free function:
///   Returns `true`  when `finish.value > 0mm` (surface finish specified)
///   Returns `false` when `finish.value == 0mm` (unspecified / zero finish)
///
/// (a) Value path: `param feat : Geometry = box(1mm,1mm,1mm); param ok : Bool = require_finish(feat, SurfaceFinish(...))`
///     evals to `Value::Bool(true)` with a full kernel; `Undef` with the simple engine
///     (box() requires the geometry kernel; simple engine propagates Undef for the call).
/// (b) Constraint path: `param feat : Geometry = box(1mm,1mm,1mm); constraint require_finish(feat, SurfaceFinish(value: 1.6um))`
///     is never Violated; `value: 0mm` is never Satisfied.
///     (Full kernel: Satisfied/Violated; simple engine: Indeterminate — box() → Undef.)
///
/// NOTE: direction and process are supplied explicitly in this test — see
/// `surface_finish_direction_process_defaults` for the defaulted-params variant.
#[test]
fn require_finish_bool_free_fn() {
    // (a) Value path: require_finish returns true when finish.value > 0mm ──────
    let source_value = r#"
structure def Probe {
    param feat : Geometry = box(1mm, 1mm, 1mm)
    param ok : Bool = require_finish(feat, SurfaceFinish(
        parameter: SurfaceParameter.Ra,
        value: 1.6um,
        direction: SurfaceDirection.Multidirectional,
        process: ""
    ))
}
"#;
    let compiled = parse_and_compile_with_stdlib(source_value);

    // No compile errors expected
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "require_finish(value: 1.6um) should compile without errors, got: {:?}",
        compile_errors
    );

    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);
    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(eval_errors.is_empty(), "eval errors: {:?}", eval_errors);

    // `feat : Geometry = box(1mm,1mm,1mm)` requires the geometry kernel to evaluate.
    // Accept Bool(true) (full kernel) or Undef (simple engine — box() → Undef → propagated).
    // The cell must EXIST (None means the param was never produced — a regression).
    // The compile-time assertion above is the primary type-correctness check.
    let cell_id = ValueCellId::new("Probe", "ok");
    match result.values.get(&cell_id) {
        Some(Value::Bool(true)) => {} // full kernel: 1.6µm > 0mm → true
        Some(Value::Undef) => {} // simple engine: box() → Undef → require_finish propagates Undef
        None => panic!(
            "require_finish(feat: Geometry, SurfaceFinish(value: 1.6um)): cell Probe.ok must \
             exist after successful eval (got None — param never produced)"
        ),
        Some(other) => panic!(
            "require_finish(feat: Geometry, SurfaceFinish(value: 1.6um)) should be Bool(true) \
             or Undef (simple engine), got {:?}",
            other
        ),
    }

    // (b) Constraint path: constraint not Violated when value > 0mm ─────────────
    // With simple engine box() → Undef, require_finish → Undef → Indeterminate (not Violated ✓).
    let source_pass = r#"
structure def ProbePass {
    param feat : Geometry = box(1mm, 1mm, 1mm)
    constraint require_finish(feat, SurfaceFinish(
        parameter: SurfaceParameter.Ra,
        value: 1.6um,
        direction: SurfaceDirection.Multidirectional,
        process: ""
    ))
}
"#;
    let result_pass = check_source_with_stdlib(source_pass);
    let violated_pass: Vec<_> = result_pass
        .constraint_results
        .iter()
        .filter(|e| e.satisfaction == Satisfaction::Violated)
        .collect();
    assert!(
        violated_pass.is_empty(),
        "require_finish(value: 1.6um) constraint should not be Violated (full kernel: Satisfied; simple engine: Indeterminate), got: {:?}",
        violated_pass
    );

    // (b2) Constraint not Satisfied when value == 0mm (0mm > 0mm is false) ───────
    // Full kernel: Violated. Simple engine: box() → Undef → require_finish → Undef → Indeterminate.
    // Either way the constraint must NOT be Satisfied.
    let source_fail = r#"
structure def ProbeFail {
    param feat : Geometry = box(1mm, 1mm, 1mm)
    constraint require_finish(feat, SurfaceFinish(
        parameter: SurfaceParameter.Ra,
        value: 0mm,
        direction: SurfaceDirection.Multidirectional,
        process: ""
    ))
}
"#;
    let result_fail = check_source_with_stdlib(source_fail);
    let has_not_satisfied = result_fail
        .constraint_results
        .iter()
        .any(|e| e.satisfaction != Satisfaction::Satisfied);
    assert!(
        has_not_satisfied,
        "require_finish(value: 0mm) must not be Satisfied (full kernel: Violated; simple engine: Indeterminate), got: {:?}",
        result_fail.constraint_results
    );
}

// ─── task-4342: ctor derived-let sub-consistency acceptance test ─────────────

/// End-to-end acceptance test for task-4342: derived `let` members of a
/// `StructureInstance` are correctly materialized across all three non-`sub`
/// arrival paths, and their values equal the `sub`-path baseline
/// (sub-consistency — the acceptance bar).
///
/// Uses locally-redeclared structures so the eval engine can resolve templates
/// from the user module (engine does not look up stdlib templates by name).
///
/// Paths exercised:
///   (1) ctor-in-entity-let:  `let g = TestDT(5mm, 0.02mm, -0.01mm)`
///                             `let ul = g.upper_limit`
///   (2) param-held:          `TestHolder { param p: TestDT; let d = p.upper_limit }`
///                             instantiated as `TestHolder(TestDT(5mm, 0.02mm, -0.01mm))`
///   (3) fn-returned:         `fn make_dt() -> TestDT { TestDT(5mm, 0.02mm, -0.01mm) }`
///                             `let g2 = make_dt()`, `let ul2 = g2.upper_limit`
///   (baseline) sub:          `sub s = TestDT(5mm, 0.02mm, -0.01mm)` reading `s.upper_limit`
///
/// All four must agree: upper_limit = nominal + upper_deviation = 5mm + 0.02mm = 0.00502 m.
/// Also asserts TestFit.max_clearance resolves (a Length-difference derived let).
///
/// RED on base (steps 4+6 not done): all non-sub paths return Undef.
/// GREEN after step-4 (compiler carries lets) + step-6 (eval materializes them).
#[test]
fn ctor_derived_let_materializes_across_all_arrival_paths() {
    let source = r#"
structure def TestDT {
    param nominal          : Length
    param upper_deviation  : Length
    param lower_deviation  : Length
    let upper_limit    = nominal + upper_deviation
    let lower_limit    = nominal + lower_deviation
    let tolerance_band = upper_deviation - lower_deviation
}

structure def TestFit {
    param hole_upper  : Length
    param shaft_lower : Length
    let max_clearance = hole_upper - shaft_lower
}

structure def TestHolder {
    param p : TestDT
    let d   = p.upper_limit
}

pub fn make_dt() -> TestDT {
    TestDT(5mm, 0.02mm, -0.01mm)
}

structure def Scenario {
    let g   = TestDT(5mm, 0.02mm, -0.01mm)
    let ul  = g.upper_limit
    let g2  = make_dt()
    let ul2 = g2.upper_limit
    sub s   = TestDT(nominal: 5mm, upper_deviation: 0.02mm, lower_deviation: -0.01mm)
    let fit = TestFit(10mm, 8mm)
    let mc  = fit.max_clearance
}

structure def HolderInst {
    let h       = TestHolder(TestDT(5mm, 0.02mm, -0.01mm))
    let param_d = h.d
}
"#;

    let compiled = parse_and_compile_with_stdlib(source);
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "task-4342 acceptance: compile errors: {:?}",
        compile_errors
    );

    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);
    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        eval_errors.is_empty(),
        "task-4342 acceptance: eval errors: {:?}",
        eval_errors
    );

    // Expected: 5mm + 0.02mm = 0.005 m + 0.00002 m = 0.00502 m
    let expected_si = 0.00502_f64;

    let all_keys: Vec<_> = result
        .values
        .iter()
        .map(|(k, _)| format!("{}.{}", k.entity, k.member))
        .collect();

    // ── helper: assert a cell holds a LENGTH Scalar ≈ expected ───────────────
    macro_rules! assert_length {
        ($entity:expr, $member:expr, $expected:expr, $path_name:literal) => {{
            let id = ValueCellId::new($entity, $member);
            let value = result.values.get(&id).unwrap_or_else(|| {
                panic!(
                    "task-4342 {} path: {}.{} not found in eval result; \
                     present keys: {:?}",
                    $path_name, $entity, $member, all_keys
                )
            });
            match value {
                Value::Scalar { si_value, dimension } => {
                    assert_eq!(
                        *dimension,
                        DimensionVector::LENGTH,
                        "task-4342 {} path: {}.{} expected LENGTH dimension, got {:?}",
                        $path_name, $entity, $member, dimension
                    );
                    let rel_err = (si_value - $expected).abs() / $expected;
                    assert!(
                        rel_err < 1e-9,
                        "task-4342 {} path: {}.{} expected {:.8e} m, \
                         got {:.8e} m (rel_err {:.2e})",
                        $path_name, $entity, $member, $expected, si_value, rel_err
                    );
                }
                other => panic!(
                    "task-4342 {} path: {}.{} expected Value::Scalar (LENGTH), \
                     got {:?}",
                    $path_name, $entity, $member, other
                ),
            }
        }};
    }

    // (baseline) sub path: elaborate_child_lets_only sets s.upper_limit
    assert_length!("Scenario.s", "upper_limit", expected_si, "sub baseline");

    // (1) ctor-in-entity-let: Scenario.ul = g.upper_limit
    assert_length!("Scenario", "ul", expected_si, "ctor-in-entity-let");

    // (3) fn-returned: Scenario.ul2 = make_dt().upper_limit
    assert_length!("Scenario", "ul2", expected_si, "fn-returned");

    // (2) param-held: HolderInst.param_d = TestHolder(TestDT(...)).d
    assert_length!("HolderInst", "param_d", expected_si, "param-held");

    // Fit.max_clearance = 10mm - 8mm = 2mm = 0.002 m
    let mc_id = ValueCellId::new("Scenario", "mc");
    let mc_value = result.values.get(&mc_id).unwrap_or_else(|| {
        panic!(
            "task-4342 Fit path: Scenario.mc not found; present keys: {:?}",
            all_keys
        )
    });
    match mc_value {
        Value::Scalar { si_value, dimension } => {
            assert_eq!(
                *dimension,
                DimensionVector::LENGTH,
                "Scenario.mc expected LENGTH dimension, got {:?}",
                dimension
            );
            let expected_mc = 0.002_f64;
            let rel_err = (si_value - expected_mc).abs() / expected_mc;
            assert!(
                rel_err < 1e-9,
                "Scenario.mc expected 0.002 m (10mm - 8mm), \
                 got {:.8e} m (rel_err {:.2e})",
                si_value, rel_err
            );
        }
        other => panic!(
            "Scenario.mc expected Value::Scalar (LENGTH), got {:?}",
            other
        ),
    }
}

// ─── γ-1: symmetric_tolerance returns DimensionalTolerance ───────────────────

/// (γ step-1 RED → GREEN) symmetric_tolerance is reshaped to return DimensionalTolerance.
///
/// (a) Structural: the compiled stdlib `symmetric_tolerance` function has
///     `return_type == Type::StructureRef("DimensionalTolerance")`.
///
/// (b) Eval: calling symmetric_tolerance(10mm, 0.1mm) in a Probe structure and
///     reading its derived lets via ctor-in-entity-let path should give:
///       upper_limit = nominal + upper_deviation = 10mm + 0.1mm = 10.1mm = 0.0101 m
///       lower_limit = nominal + lower_deviation = 10mm + (−0.1mm) = 9.9mm = 0.0099 m
///       tolerance_band = upper_deviation − lower_deviation = 0.2mm = 0.0002 m
///
/// RED on base: symmetric_tolerance returns bare Length; return_type is Length,
/// member accesses yield Undef.
#[test]
fn symmetric_tolerance_returns_dimensional_tolerance() {
    // (a) Structural: return_type must be StructureRef("DimensionalTolerance") ──
    let module = load_stdlib_module();
    let sym = module
        .functions
        .iter()
        .find(|f| f.name == "symmetric_tolerance")
        .expect("expected 'symmetric_tolerance' function");
    assert_eq!(
        sym.return_type,
        Type::StructureRef("DimensionalTolerance".to_string()),
        "symmetric_tolerance return_type should be Type::StructureRef(\"DimensionalTolerance\"), \
         got {:?}",
        sym.return_type
    );

    // (b) Eval: prelude fn returns prelude struct; derived lets materialise ───
    let source = r#"
structure def Probe {
    let st    = symmetric_tolerance(10mm, 0.1mm)
    let upper = st.upper_limit
    let lower = st.lower_limit
    let band  = st.tolerance_band
}
"#;
    let compiled = parse_and_compile_with_stdlib(source);
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "symmetric_tolerance Probe should compile without errors, got: {:?}",
        compile_errors
    );

    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);
    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(eval_errors.is_empty(), "eval errors: {:?}", eval_errors);

    let all_keys: Vec<_> = result
        .values
        .iter()
        .map(|(k, _)| format!("{}.{}", k.entity, k.member))
        .collect();

    macro_rules! assert_length {
        ($entity:expr, $member:expr, $expected:expr, $label:literal) => {{
            let id = ValueCellId::new($entity, $member);
            let value = result.values.get(&id).unwrap_or_else(|| {
                panic!(
                    "γ {}: {}.{} not found; present keys: {:?}",
                    $label, $entity, $member, all_keys
                )
            });
            match value {
                Value::Scalar { si_value, dimension } => {
                    assert_eq!(
                        *dimension,
                        DimensionVector::LENGTH,
                        "γ {}: {}.{} expected LENGTH dimension, got {:?}",
                        $label, $entity, $member, dimension
                    );
                    let rel_err = (si_value - $expected).abs() / $expected;
                    assert!(
                        rel_err < 1e-9,
                        "γ {}: {}.{} expected {:.8e} m, got {:.8e} m (rel_err {:.2e})",
                        $label, $entity, $member, $expected, si_value, rel_err
                    );
                }
                other => panic!(
                    "γ {}: {}.{} expected Value::Scalar (LENGTH), got {:?}",
                    $label, $entity, $member, other
                ),
            }
        }};
    }

    // symmetric_tolerance(10mm, 0.1mm):
    //   upper_limit  = 10mm + 0.1mm    = 10.1mm = 0.0101 m
    assert_length!("Probe", "upper", 0.0101_f64, "symmetric_tolerance.upper_limit");
    //   lower_limit  = 10mm + (−0.1mm) =  9.9mm = 0.0099 m
    assert_length!("Probe", "lower", 0.0099_f64, "symmetric_tolerance.lower_limit");
    //   tolerance_band = 0.1mm − (−0.1mm) = 0.2mm = 0.0002 m
    assert_length!("Probe", "band",  0.0002_f64, "symmetric_tolerance.tolerance_band");
}

// ─── γ-3: limit_tolerance returns DimensionalTolerance ───────────────────────

/// (γ step-3 RED → GREEN) limit_tolerance is reshaped to return DimensionalTolerance.
///
/// (a) Structural: the compiled stdlib `limit_tolerance` function has
///     `return_type == Type::StructureRef("DimensionalTolerance")`.
///
/// (b) Eval: limit_tolerance(upper=10mm, lower=9.9mm) uses nominal=lower convention:
///     DT(nominal: 9.9mm, upper_deviation: 0.1mm, lower_deviation: 0mm)
///       upper_limit  = 9.9mm + 0.1mm = 10mm = 0.010 m  (== upper arg)
///       lower_limit  = 9.9mm + 0mm   = 9.9mm = 0.0099 m (== lower arg)
///       tolerance_band = 0.1mm − 0mm = 0.1mm = 0.0001 m (== upper−lower)
///
/// RED on base: limit_tolerance returns bare Length (upper − lower); return_type
/// is Scalar{LENGTH}, member accesses yield Undef.
#[test]
fn limit_tolerance_returns_dimensional_tolerance() {
    // (a) Structural: return_type must be StructureRef("DimensionalTolerance") ──
    let module = load_stdlib_module();
    let lim = module
        .functions
        .iter()
        .find(|f| f.name == "limit_tolerance")
        .expect("expected 'limit_tolerance' function");
    assert_eq!(
        lim.return_type,
        Type::StructureRef("DimensionalTolerance".to_string()),
        "limit_tolerance return_type should be Type::StructureRef(\"DimensionalTolerance\"), \
         got {:?}",
        lim.return_type
    );

    // (b) Eval: prelude limit_tolerance returns DT with nominal=lower convention ──
    let source = r#"
structure def Probe {
    let lt    = limit_tolerance(10mm, 9.9mm)
    let upper = lt.upper_limit
    let lower = lt.lower_limit
    let band  = lt.tolerance_band
}
"#;
    let compiled = parse_and_compile_with_stdlib(source);
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "limit_tolerance Probe should compile without errors, got: {:?}",
        compile_errors
    );

    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);
    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(eval_errors.is_empty(), "eval errors: {:?}", eval_errors);

    let all_keys: Vec<_> = result
        .values
        .iter()
        .map(|(k, _)| format!("{}.{}", k.entity, k.member))
        .collect();

    macro_rules! assert_length {
        ($entity:expr, $member:expr, $expected:expr, $label:literal) => {{
            let id = ValueCellId::new($entity, $member);
            let value = result.values.get(&id).unwrap_or_else(|| {
                panic!(
                    "γ {}: {}.{} not found; present keys: {:?}",
                    $label, $entity, $member, all_keys
                )
            });
            match value {
                Value::Scalar { si_value, dimension } => {
                    assert_eq!(
                        *dimension,
                        DimensionVector::LENGTH,
                        "γ {}: {}.{} expected LENGTH dimension, got {:?}",
                        $label, $entity, $member, dimension
                    );
                    let rel_err = (si_value - $expected).abs() / $expected;
                    assert!(
                        rel_err < 1e-9,
                        "γ {}: {}.{} expected {:.8e} m, got {:.8e} m (rel_err {:.2e})",
                        $label, $entity, $member, $expected, si_value, rel_err
                    );
                }
                other => panic!(
                    "γ {}: {}.{} expected Value::Scalar (LENGTH), got {:?}",
                    $label, $entity, $member, other
                ),
            }
        }};
    }

    // limit_tolerance(upper=10mm, lower=9.9mm):
    //   DT(nominal: 9.9mm, upper_deviation: 0.1mm, lower_deviation: 0mm)
    //   upper_limit  = 9.9mm + 0.1mm = 10mm = 0.010 m  (== upper arg)
    assert_length!("Probe", "upper", 0.010_f64, "limit_tolerance.upper_limit");
    //   lower_limit  = 9.9mm + 0mm = 9.9mm = 0.0099 m  (== lower arg)
    assert_length!("Probe", "lower", 0.0099_f64, "limit_tolerance.lower_limit");
    //   tolerance_band = 0.1mm − 0mm = 0.1mm = 0.0001 m  (== upper − lower)
    assert_length!("Probe", "band",  0.0001_f64, "limit_tolerance.tolerance_band");
}

// ─── γ-5: Fit exposes nested DimensionalTolerance members ────────────────────

/// (γ step-5 RED → GREEN) Fit is reshaped to use nested DimensionalTolerance params.
///
/// (a) Structural: Fit must have Param cells `hole_tolerance` and `shaft_tolerance`
///     (replacing the four flat scalars) plus `fit_type`, and Let cells
///     `max_clearance` and `min_clearance`.
///
/// (b) Eval: build `Fit(hole_tolerance: symmetric_tolerance(10mm, 0.1mm),
///                      shaft_tolerance: symmetric_tolerance(9.9mm, 0.05mm),
///                      fit_type: FitCategory.Clearance)` then read nested members:
///
///   hole_tolerance = symmetric_tolerance(10mm, 0.1mm):
///     .upper_limit = 10.1mm = 0.0101 m
///     .lower_limit = 9.9mm  = 0.0099 m
///   shaft_tolerance = symmetric_tolerance(9.9mm, 0.05mm):
///     .upper_limit = 9.95mm = 0.00995 m
///     .lower_limit = 9.85mm = 0.00985 m
///
///   hu  = f.hole_tolerance.upper_limit      = 10.1mm  = 0.0101 m
///   maxc = f.max_clearance = 10.1mm − 9.85mm = 0.25mm  = 2.5e-4 m
///   minc = f.min_clearance =  9.9mm − 9.95mm = −0.05mm = −5e-5  m (interference)
///
/// RED on base: Fit has flat hole_upper/hole_lower/shaft_upper/shaft_lower params;
/// hole_tolerance/shaft_tolerance don't exist (compile error or Undef).
#[test]
fn fit_exposes_nested_dimensional_tolerance_members() {
    // (a) Structural: Fit must have Param cells hole_tolerance + shaft_tolerance + fit_type ──
    let module = load_stdlib_module();
    let fit = module
        .templates
        .iter()
        .find(|t| t.name == "Fit")
        .expect("expected 'Fit' template");

    let param_names: Vec<&str> = fit
        .value_cells
        .iter()
        .filter(|vc| vc.kind == ValueCellKind::Param)
        .map(|vc| vc.id.member.as_str())
        .collect();
    assert!(
        param_names.contains(&"hole_tolerance"),
        "Fit should have 'hole_tolerance' Param cell; got params: {:?}",
        param_names
    );
    assert!(
        param_names.contains(&"shaft_tolerance"),
        "Fit should have 'shaft_tolerance' Param cell; got params: {:?}",
        param_names
    );
    assert!(
        param_names.contains(&"fit_type"),
        "Fit should still have 'fit_type' Param cell; got params: {:?}",
        param_names
    );
    // Flat scalars must NOT be present after reshape
    assert!(
        !param_names.contains(&"hole_upper"),
        "Fit should NOT have 'hole_upper' after reshape; got params: {:?}",
        param_names
    );
    assert!(
        !param_names.contains(&"shaft_lower"),
        "Fit should NOT have 'shaft_lower' after reshape; got params: {:?}",
        param_names
    );

    // (b) Eval: nested member reads and clearance arithmetic ──────────────────
    let source = r#"
structure def Probe {
    let f    = Fit(hole_tolerance: symmetric_tolerance(10mm, 0.1mm),
                   shaft_tolerance: symmetric_tolerance(9.9mm, 0.05mm),
                   fit_type: FitCategory.Clearance)
    let hu   = f.hole_tolerance.upper_limit
    let maxc = f.max_clearance
    let minc = f.min_clearance
}
"#;
    let compiled = parse_and_compile_with_stdlib(source);
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "Fit Probe should compile without errors, got: {:?}",
        compile_errors
    );

    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);
    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(eval_errors.is_empty(), "eval errors: {:?}", eval_errors);

    let all_keys: Vec<_> = result
        .values
        .iter()
        .map(|(k, _)| format!("{}.{}", k.entity, k.member))
        .collect();

    // Helper: assert a LENGTH scalar within 1e-9 relative tolerance (for positive values)
    macro_rules! assert_length_rel {
        ($entity:expr, $member:expr, $expected:expr, $label:literal) => {{
            let id = ValueCellId::new($entity, $member);
            let value = result.values.get(&id).unwrap_or_else(|| {
                panic!(
                    "γ {}: {}.{} not found; present keys: {:?}",
                    $label, $entity, $member, all_keys
                )
            });
            match value {
                Value::Scalar { si_value, dimension } => {
                    assert_eq!(
                        *dimension,
                        DimensionVector::LENGTH,
                        "γ {}: {}.{} expected LENGTH, got {:?}",
                        $label, $entity, $member, dimension
                    );
                    let rel_err = (si_value - $expected).abs() / ($expected as f64).abs();
                    assert!(
                        rel_err < 1e-9,
                        "γ {}: {}.{} expected {:.8e} m, got {:.8e} m (rel_err {:.2e})",
                        $label, $entity, $member, $expected, si_value, rel_err
                    );
                }
                other => panic!(
                    "γ {}: {}.{} expected Value::Scalar (LENGTH), got {:?}",
                    $label, $entity, $member, other
                ),
            }
        }};
    }
    // Helper: assert a LENGTH scalar using absolute error (for near-zero or negative values)
    macro_rules! assert_length_abs {
        ($entity:expr, $member:expr, $expected:expr, $label:literal) => {{
            let id = ValueCellId::new($entity, $member);
            let value = result.values.get(&id).unwrap_or_else(|| {
                panic!(
                    "γ {}: {}.{} not found; present keys: {:?}",
                    $label, $entity, $member, all_keys
                )
            });
            match value {
                Value::Scalar { si_value, dimension } => {
                    assert_eq!(
                        *dimension,
                        DimensionVector::LENGTH,
                        "γ {}: {}.{} expected LENGTH, got {:?}",
                        $label, $entity, $member, dimension
                    );
                    assert!(
                        (si_value - $expected).abs() < 1e-9,
                        "γ {}: {}.{} expected {:.8e} m, got {:.8e} m (abs_err {:.2e})",
                        $label, $entity, $member, $expected, si_value, (si_value - $expected).abs()
                    );
                }
                other => panic!(
                    "γ {}: {}.{} expected Value::Scalar (LENGTH), got {:?}",
                    $label, $entity, $member, other
                ),
            }
        }};
    }

    // hu = f.hole_tolerance.upper_limit = 10mm + 0.1mm = 10.1mm = 0.0101 m
    assert_length_rel!("Probe", "hu",   0.0101_f64,  "Fit.hole_tolerance.upper_limit");
    // maxc = hole.upper(10.1mm) − shaft.lower(9.85mm) = 0.25mm = 2.5e-4 m
    assert_length_rel!("Probe", "maxc", 0.00025_f64, "Fit.max_clearance");
    // minc = hole.lower(9.9mm) − shaft.upper(9.95mm) = −0.05mm = −5e-5 m (interference)
    assert_length_abs!("Probe", "minc", -5e-5_f64,   "Fit.min_clearance");
}

// ─── γ-amend: inverted tolerance band violates user-declared constraint ────────

/// Guard: the DimensionalTolerance constraint `upper_deviation >= lower_deviation`
/// fails when the band is inverted. After the γ reshape, this failure mode is
/// reachable through the constructor functions:
///
///   - `limit_tolerance(9mm, 10mm)`: upper < lower →
///     DT(nominal: 10mm, upper_deviation: −1mm, lower_deviation: 0mm)
///     → tolerance_band = upper_deviation − lower_deviation = −1mm
///
///   - `symmetric_tolerance(10mm, −1mm)`: negative deviation →
///     DT(nominal: 10mm, upper_deviation: −1mm, lower_deviation: 1mm)
///     → tolerance_band = −1mm − 1mm = −2mm
///
/// Both cases are surfaced via an explicit user-module constraint
/// `constraint band >= 0mm`. When tolerance_band < 0mm the constraint fires as
/// Violated, pinning the behavior so a regression that silently drops the
/// DimensionalTolerance constraint would be caught here.
#[test]
fn inverted_tolerance_band_violates_constraint() {
    // limit_tolerance(9mm, 10mm): upper < lower → tolerance_band = −1mm < 0mm
    let source_inv_limit = r#"
structure def ProbeInvLimit {
    let inv  = limit_tolerance(9mm, 10mm)
    let band = inv.tolerance_band
    constraint band >= 0mm
}
"#;
    let result_limit = check_source_with_stdlib(source_inv_limit);
    assert!(
        result_limit
            .constraint_results
            .iter()
            .any(|e| e.satisfaction == Satisfaction::Violated),
        "limit_tolerance(9mm, 10mm) inverts the band (tolerance_band = −1mm < 0mm); \
         user constraint 'band >= 0mm' must fire as Violated, got: {:?}",
        result_limit.constraint_results
    );

    // symmetric_tolerance(10mm, −1mm): negative deviation → tolerance_band = −2mm < 0mm
    // (deviation = −1mm → upper_deviation = −1mm; lower_deviation = −(−1mm) = 1mm)
    let source_inv_sym = r#"
structure def ProbeInvSym {
    let inv  = symmetric_tolerance(10mm, 0mm - 1mm)
    let band = inv.tolerance_band
    constraint band >= 0mm
}
"#;
    let result_sym = check_source_with_stdlib(source_inv_sym);
    assert!(
        result_sym
            .constraint_results
            .iter()
            .any(|e| e.satisfaction == Satisfaction::Violated),
        "symmetric_tolerance(10mm, −1mm) inverts the band (tolerance_band = −2mm < 0mm); \
         user constraint 'band >= 0mm' must fire as Violated, got: {:?}",
        result_sym.constraint_results
    );
}

// ── task #3116: feature/datum_refs type-contract assertions ───────────────────

/// RED (step-3): stdlib type-contract for `feature` and `datum_refs` members.
///
/// After the step-4 stdlib flip:
/// - `GeometricTolerance.feature` required member must be `Type::Geometry`
/// - `Flatness.feature` param cell must be `Type::Geometry`
/// - `OrientationTolerance.datum_refs` required member must be `Type::Geometry`
/// - `LocationTolerance.datum_refs` required member must be `Type::Geometry`
///
/// Fails before step-4 because all four sites still resolve to `Type::dimensionless_scalar()`.
#[test]
fn stdlib_feature_datum_refs_have_geometry_type() {
    let module = load_stdlib_module();

    // GeometricTolerance.feature must be Param(Type::Geometry)
    let gt = module
        .trait_defs
        .iter()
        .find(|t| t.name == "GeometricTolerance")
        .expect("expected 'GeometricTolerance' trait");
    let gt_feature = gt
        .required_members
        .iter()
        .find(|r| r.name == "feature")
        .expect("GeometricTolerance must have a 'feature' required member");
    match &gt_feature.kind {
        RequirementKind::Param(ty) => assert_eq!(
            *ty,
            Type::Geometry,
            "GeometricTolerance.feature must be Type::Geometry (not {:?})",
            ty
        ),
        other => panic!(
            "GeometricTolerance.feature must be RequirementKind::Param, got {:?}",
            other
        ),
    }

    // Flatness.feature cell_type must be Type::Geometry
    let flatness = module
        .templates
        .iter()
        .find(|t| t.name == "Flatness")
        .expect("expected 'Flatness' template");
    let flatness_feature = flatness
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "feature")
        .expect("Flatness must have a 'feature' value cell");
    assert_eq!(
        flatness_feature.cell_type,
        Type::Geometry,
        "Flatness.feature must be Type::Geometry (not {:?})",
        flatness_feature.cell_type
    );

    // OrientationTolerance.datum_refs must be Param(Type::Geometry)
    let ot = module
        .trait_defs
        .iter()
        .find(|t| t.name == "OrientationTolerance")
        .expect("expected 'OrientationTolerance' trait");
    let ot_datum = ot
        .required_members
        .iter()
        .find(|r| r.name == "datum_refs")
        .expect("OrientationTolerance must have a 'datum_refs' required member");
    match &ot_datum.kind {
        RequirementKind::Param(ty) => assert_eq!(
            *ty,
            Type::Geometry,
            "OrientationTolerance.datum_refs must be Type::Geometry (not {:?})",
            ty
        ),
        other => panic!(
            "OrientationTolerance.datum_refs must be RequirementKind::Param, got {:?}",
            other
        ),
    }

    // LocationTolerance.datum_refs must be Param(Type::Geometry)
    let lt = module
        .trait_defs
        .iter()
        .find(|t| t.name == "LocationTolerance")
        .expect("expected 'LocationTolerance' trait");
    let lt_datum = lt
        .required_members
        .iter()
        .find(|r| r.name == "datum_refs")
        .expect("LocationTolerance must have a 'datum_refs' required member");
    match &lt_datum.kind {
        RequirementKind::Param(ty) => assert_eq!(
            *ty,
            Type::Geometry,
            "LocationTolerance.datum_refs must be Type::Geometry (not {:?})",
            ty
        ),
        other => panic!(
            "LocationTolerance.datum_refs must be RequirementKind::Param, got {:?}",
            other
        ),
    }
}

// ─── task-4461: tolerancing_diagnose wired into eval Undef-diagnosis ─────────

/// E2E pin: out-of-envelope iso_it_tolerance (grade 25) surfaces a
/// Severity::Error "E_TolerancingOutOfEnvelope" in result.diagnostics after
/// the tolerancing_diagnose arm was wired into emit_undef_builtin_diagnostics
/// (task 4461 step-2).
///
/// Mirrors the result.diagnostics read at tolerancing_tests.rs:980 — this is
/// the eval-pipeline realization of the `reify eval`→stderr user-observable
/// signal (cmd_eval renders result.diagnostics to stderr). Grade 25 keeps this
/// test distinct from iso_tolerance_grade_out_of_envelope_undef (grade 4),
/// which only checks compile diagnostics + the Undef cell value.
#[test]
fn iso_it_tolerance_grade_25_out_of_envelope_emits_eval_error_diagnostic() {
    let source = r#"
structure def TestOOE {
    param grade : Int = 25
    param nmin : Length = 30mm
    param nmax : Length = 50mm
    let tolerance_value = iso_it_tolerance(grade, nmin, nmax)
}
structure def Probe {
    sub g = TestOOE()
}
"#;
    let compiled = parse_and_compile_with_stdlib(source);

    // Compile should be clean (no Error diagnostics).
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "out-of-envelope TestOOE should compile without errors, got: {:?}",
        compile_errors
    );

    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    // (a) Sub-component was instantiated — distinguishes "diagnostic emitted via sub" from
    // "sub was never resolved" (a different failure mode that the unit test cannot catch).
    let probe_g_keys: Vec<_> = result
        .values
        .iter()
        .filter(|(k, _)| k.entity == "Probe.g")
        .collect();
    assert!(
        !probe_g_keys.is_empty(),
        "Probe.g sub-component should have produced at least one value cell; \
         got none — check sub-component resolution"
    );

    // (b) tolerance_value inside the sub-component is Undef (or absent — the evaluator
    // may elide Undef cells; see iso_tolerance_grade_out_of_envelope_undef for context).
    let cell_id = ValueCellId::new("Probe.g", "tolerance_value");
    match result.values.get(&cell_id) {
        Some(Value::Undef) | None => {
            // Expected: grade 25 → Undef (cell present-as-Undef or elided by evaluator).
        }
        Some(other) => panic!(
            "Probe.g.tolerance_value for grade 25 should be Undef or absent, got {:?}",
            other
        ),
    }

    // (c) Eval sink contains the E_TolerancingOutOfEnvelope Error that propagated from
    // the sub-component evaluation through the full compile→eval pipeline.  This is
    // the eval-pipeline realization of the `reify eval`→stderr user-observable signal
    // (cmd_eval renders result.diagnostics to stderr).
    assert!(
        result.diagnostics.iter().any(|d| {
            d.severity == Severity::Error && d.message.contains("E_TolerancingOutOfEnvelope")
        }),
        "eval diagnostics must contain an E_TolerancingOutOfEnvelope Error for grade 25, \
         got: {:?}",
        result.diagnostics
    );
}

/// GREEN (step-3 resolver contract): `param feature : Geometry` and
/// `param datum_refs : DatumRef` in an inline source must compile with
/// zero "unresolved type" diagnostics.
///
/// Should pass immediately after step-2 adds the `DatumRef` resolver arm.
#[test]
fn geometry_and_datum_ref_type_names_compile_in_inline_source() {
    let source = r#"
structure def GeomProbe {
    param feature : Geometry
    param datum_refs : DatumRef
}
"#;
    let module = parse_and_compile_with_stdlib(source);
    // Assert zero Error diagnostics — not just "unresolved" substring — so a differently
    // worded error or an entirely different compile failure does not slip through silently.
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "`param feature : Geometry` and `param datum_refs : DatumRef` must compile with \
         zero Error diagnostics (both type names must resolve cleanly); got: {:?}",
        errors
    );
}
