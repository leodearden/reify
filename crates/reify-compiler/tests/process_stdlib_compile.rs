//! Tests for `crates/reify-compiler/stdlib/process.ri` —
//! `std.process` module: DFMSeverity enum, Process base trait,
//! seven process-category marker traits (Subtracting, Adding, Forming,
//! Joining, Parting, SurfaceTreating, HeatTreating), and DFMRule trait.
//!
//! Reconstructs the lost std.process stdlib module per PRD
//! docs/prds/v0_6/stdlib-reconstruction.md §Slice B.
//!
//! Trait surface only — DFM-rule evaluation (running a rule against geometry)
//! is out of scope and not tested here.
//!
//! Tests use the production-path `load_stdlib()` helper, modeled on
//! `io_traits_tests.rs` + `fdm_stdlib_compile.rs`.

use reify_compiler::{CompiledTrait, EntityKind, RequirementKind, stdlib_loader};
use reify_core::{DimensionVector, Severity, Type};
use reify_ir::EnumDef;
use reify_test_support::compile_source_with_stdlib;

// ─── helpers ─────────────────────────────────────────────────────────────────

/// Return the `std/process` CompiledModule from the production stdlib loader.
/// Panics if absent — which is the expected failure mode until step-2 registers
/// the module.
fn load_stdlib_module() -> &'static reify_compiler::CompiledModule {
    stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/process")
        .unwrap_or_else(|| {
            panic!(
                "stdlib should contain std/process module; available paths: {:?}",
                stdlib_loader::load_stdlib()
                    .iter()
                    .map(|m| m.path.to_string())
                    .collect::<Vec<_>>()
            )
        })
}

/// Look up an enum definition by name within the `std/process` module.
fn find_enum(name: &str) -> &'static EnumDef {
    let module = load_stdlib_module();
    module
        .enum_defs
        .iter()
        .find(|e| e.name == name)
        .unwrap_or_else(|| {
            panic!(
                "expected `enum {}` in std/process, got enum_defs: {:?}",
                name,
                module.enum_defs.iter().map(|e| &e.name).collect::<Vec<_>>()
            )
        })
}

/// Find a trait by name within the `std/process` module.
fn find_trait(name: &str) -> &'static CompiledTrait {
    let module = load_stdlib_module();
    module
        .trait_defs
        .iter()
        .find(|t| t.name == name)
        .unwrap_or_else(|| {
            panic!(
                "std/process should contain trait '{}'; found: {:?}",
                name,
                module
                    .trait_defs
                    .iter()
                    .map(|t| &t.name)
                    .collect::<Vec<_>>()
            )
        })
}

/// Get the Param type for a named required member of a trait or panic.
fn param_type(trait_name: &str, member: &str) -> Type {
    let t = find_trait(trait_name);
    let req = t
        .required_members
        .iter()
        .find(|r| r.name == member)
        .unwrap_or_else(|| {
            panic!(
                "trait '{}' should have required member '{}'; found: {:?}",
                trait_name,
                member,
                t.required_members
                    .iter()
                    .map(|r| &r.name)
                    .collect::<Vec<_>>()
            )
        });
    match &req.kind {
        RequirementKind::Param(ty) => ty.clone(),
        other => panic!(
            "trait '{}' member '{}' should be RequirementKind::Param, got {:?}",
            trait_name, member, other
        ),
    }
}

// ─── step-1: module loads + DFMSeverity enum ─────────────────────────────────

/// The std/process module must load through the production stdlib path with zero
/// error-severity diagnostics, and enum DFMSeverity must have exactly the three
/// variants [Info, Warning, Error] in that order.
#[test]
fn std_process_loads_with_no_errors_and_dfmseverity_enum() {
    let module = load_stdlib_module();

    // Zero Error diagnostics.
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected error diagnostics in process.ri: {:?}",
        errors
    );

    // DFMSeverity must be present with exactly [Info, Warning, Error].
    let enum_def = find_enum("DFMSeverity");
    assert_eq!(
        enum_def.variants,
        vec![
            "Info".to_string(),
            "Warning".to_string(),
            "Error".to_string(),
        ],
        "DFMSeverity variants must be [Info, Warning, Error] in order; got: {:?}",
        enum_def.variants
    );
}

// ─── step-3: Process base trait ──────────────────────────────────────────────

/// Process base trait has no refinements and exactly two required params:
/// duration : Time and cost : Money (order-pinned).
#[test]
fn process_base_trait_requires_duration_time_and_cost_money() {
    let t = find_trait("Process");

    assert!(
        t.refinements.is_empty(),
        "Process should have no refinements, got: {:?}",
        t.refinements
    );

    // Exactly two required members, in declaration order.
    assert_eq!(
        t.required_members.len(),
        2,
        "Process should have exactly 2 required members (duration, cost); got: {:?}",
        t.required_members
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        t.required_members[0].name, "duration",
        "Process required_members[0] should be 'duration', got '{}'",
        t.required_members[0].name
    );
    assert_eq!(
        t.required_members[1].name, "cost",
        "Process required_members[1] should be 'cost', got '{}'",
        t.required_members[1].name
    );

    assert_eq!(
        param_type("Process", "duration"),
        Type::Scalar {
            dimension: DimensionVector::TIME
        },
        "Process.duration must have DimensionVector::TIME"
    );
    assert_eq!(
        param_type("Process", "cost"),
        Type::Scalar {
            dimension: DimensionVector::MONEY
        },
        "Process.cost must have DimensionVector::MONEY"
    );
}

// ─── step-5: process-category traits ─────────────────────────────────────────

/// Each of the seven process-category traits must refine exactly [Process] and
/// expose exactly the §8 capability params as own required_members, in declaration order.
/// (`CompiledTrait.required_members` holds only a trait's OWN declared members;
/// inherited `duration`/`cost` from Process are NOT listed there.)
#[test]
fn process_category_traits_each_refine_process() {
    let length = Type::Scalar {
        dimension: DimensionVector::LENGTH,
    };
    let angle = Type::Scalar {
        dimension: DimensionVector::ANGLE,
    };
    let pressure = Type::Scalar {
        dimension: DimensionVector::PRESSURE,
    };
    let temperature = Type::Scalar {
        dimension: DimensionVector::TEMPERATURE,
    };
    let time_t = Type::Scalar {
        dimension: DimensionVector::TIME,
    };

    // (trait_name, [(member_name, expected_type)])
    let categories: Vec<(&str, Vec<(&str, Type)>)> = vec![
        (
            "Subtracting",
            vec![
                ("tool_access", Type::Geometry),
                ("min_feature_size", length.clone()),
                ("achievable_finish", length.clone()),
            ],
        ),
        (
            "Adding",
            vec![
                ("layer_thickness", length.clone()),
                ("min_feature_size", length.clone()),
                ("build_volume", Type::Geometry),
                ("max_overhang_angle", angle.clone()),
            ],
        ),
        (
            "Forming",
            vec![
                ("min_bend_radius", length.clone()),
                ("max_draw_depth", length.clone()),
                ("draft_angle", angle.clone()),
            ],
        ),
        (
            "Joining",
            vec![
                ("joint_strength", pressure.clone()),
                ("reversible", Type::Bool),
            ],
        ),
        (
            "Parting",
            vec![
                ("kerf_width", length.clone()),
                ("min_feature_size", length.clone()),
            ],
        ),
        (
            "SurfaceTreating",
            vec![
                ("coating_thickness", length.clone()),
                ("achievable_finish", length.clone()),
            ],
        ),
        (
            "HeatTreating",
            vec![
                ("treatment_temperature", temperature.clone()),
                ("hold_duration", time_t.clone()),
            ],
        ),
    ];

    for (name, expected_members) in &categories {
        let t = find_trait(name);

        assert_eq!(
            t.refinements.as_slice(),
            ["Process".to_string()].as_slice(),
            "trait '{}' should refine exactly [Process], got: {:?}",
            name,
            t.refinements
        );

        assert_eq!(
            t.required_members.len(),
            expected_members.len(),
            "trait '{}' should have exactly {} own required_members; got: {:?}",
            name,
            expected_members.len(),
            t.required_members
                .iter()
                .map(|r| &r.name)
                .collect::<Vec<_>>()
        );

        // Declaration order is a compiler contract: required_members preserves source order.
        for (i, (member_name, expected_type)) in expected_members.iter().enumerate() {
            assert_eq!(
                t.required_members[i].name, *member_name,
                "trait '{}' required_members[{}] should be '{}', got '{}'",
                name, i, member_name, t.required_members[i].name
            );
            assert_eq!(
                param_type(name, member_name),
                *expected_type,
                "trait '{}' member '{}' should have type {:?}",
                name, member_name, expected_type
            );
        }
    }
}

// ─── step-7: DFMRule trait surface ───────────────────────────────────────────

/// DFMRule has no refinements and exactly four required params in order:
/// rule_name : String, severity : DFMSeverity, applies_to : Process,
/// subject : Solid (resolves to Type::Geometry).
#[test]
fn dfmrule_trait_surface_has_rule_name_severity_and_process_applicability() {
    let t = find_trait("DFMRule");

    assert!(
        t.refinements.is_empty(),
        "DFMRule should have no refinements, got: {:?}",
        t.refinements
    );

    // Exactly four required members, in declaration order.
    assert_eq!(
        t.required_members.len(),
        4,
        "DFMRule should have exactly 4 required members; got: {:?}",
        t.required_members
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        t.required_members[0].name, "rule_name",
        "DFMRule required_members[0] should be 'rule_name'"
    );
    assert_eq!(
        t.required_members[1].name, "severity",
        "DFMRule required_members[1] should be 'severity'"
    );
    assert_eq!(
        t.required_members[2].name, "applies_to",
        "DFMRule required_members[2] should be 'applies_to'"
    );
    assert_eq!(
        t.required_members[3].name, "subject",
        "DFMRule required_members[3] should be 'subject'"
    );

    assert_eq!(
        param_type("DFMRule", "rule_name"),
        Type::String,
        "DFMRule.rule_name must be Type::String"
    );
    assert_eq!(
        param_type("DFMRule", "severity"),
        Type::Enum("DFMSeverity".into()),
        "DFMRule.severity must be Type::Enum(\"DFMSeverity\")"
    );
    assert_eq!(
        param_type("DFMRule", "applies_to"),
        Type::TraitObject("Process".into()),
        "DFMRule.applies_to must be Type::TraitObject(\"Process\")"
    );
    assert_eq!(
        param_type("DFMRule", "subject"),
        Type::Geometry,
        "DFMRule.subject must be Type::Geometry (Solid resolves to Type::Geometry)"
    );
}

// ─── task-4407 step-3: DFMRule.subject RED tests ─────────────────────────────

/// β conformance lock — DFMRule.subject (negative):
/// A DFMRule conformer omitting `subject` must emit a missing-required-member
/// Error diagnostic.
#[test]
fn dfmrule_conformer_missing_subject_emits_missing_member_error() {
    let source = r#"
import std.process

structure def MinWallCheck : DFMRule {
    param rule_name  : String      = "min_wall"
    param severity   : DFMSeverity = DFMSeverity.Warning
    param applies_to : Process     = MilledPart()
}

structure def MilledPart : Subtracting {
    param duration         : Time   = 30min
    param cost             : Money  = 50USD
    param tool_access      : Solid  = box(200mm, 150mm, 100mm)
    param min_feature_size : Length = 0.5mm
    param achievable_finish: Length = 0.0016mm
}
"#;
    // Note: `subject` is intentionally omitted from MinWallCheck.

    let compiled = compile_source_with_stdlib(source);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    assert!(
        !errors.is_empty(),
        "MinWallCheck omitting 'subject' should emit an error diagnostic"
    );

    let error_msg = format!("{:?}", errors);
    assert!(
        error_msg.contains("missing required member") && error_msg.contains("subject"),
        "error should mention 'missing required member' and 'subject', got: {}",
        error_msg
    );
}

/// β conformance lock — DFMRule.subject (positive):
/// A complete DFMRule conformer supplying all four members including
/// `param subject : Solid = box(...)` compiles with zero Error diagnostics
/// and exposes a `subject` value cell of type Type::Geometry (the cell γ reads).
#[test]
fn dfmrule_conformer_with_subject_compiles_clean_and_exposes_subject() {
    let source = r#"
import std.process

structure def MilledPart : Subtracting {
    param duration           : Time   = 30min
    param cost               : Money  = 50USD
    param tool_access        : Solid  = box(200mm, 150mm, 100mm)
    param min_feature_size   : Length = 0.5mm
    param achievable_finish  : Length = 0.0016mm
    param max_overhang_angle : Angle  = 45deg
}

structure def MinWallCheck : DFMRule {
    param rule_name  : String      = "min_wall"
    param severity   : DFMSeverity = DFMSeverity.Warning
    param applies_to : Process     = MilledPart()
    param subject    : Solid       = box(50mm, 30mm, 20mm)
}
"#;

    let compiled = compile_source_with_stdlib(source);

    // (a-1) Zero error-severity diagnostics.
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "MinWallCheck with all DFMRule params should compile clean; errors: {:?}",
        errors
    );

    // (a-2) The compiled MinWallCheck template exposes a `subject` value cell
    //       with the correct type (Type::Geometry — Solid resolves to Type::Geometry).
    let rule = compiled
        .templates
        .iter()
        .find(|t| t.name == "MinWallCheck")
        .expect("MinWallCheck template should be present after clean compile");

    let subject_cell = rule
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "subject")
        .unwrap_or_else(|| {
            panic!(
                "MinWallCheck should have a 'subject' value cell; found: {:?}",
                rule.value_cells
                    .iter()
                    .map(|vc| &vc.id.member)
                    .collect::<Vec<_>>()
            )
        });

    assert_eq!(
        subject_cell.cell_type,
        Type::Geometry,
        "MinWallCheck.subject cell_type should be Type::Geometry"
    );
}

// ─── step-9: cardinality lock + example compile (split into two tests) ────────

/// Locks the std/process module cardinality: exactly 1 enum (DFMSeverity),
/// exactly 9 traits (Process + 7 categories + DFMRule), 0 structures.
/// Any silent expansion of process.ri fails this test — deliberate updates
/// require an explicit test change.
///
/// The zero-error invariant for std/process is already covered by
/// `std_process_loads_with_no_errors_and_dfmseverity_enum` (step-1) and the
/// central `stdlib_loader_tests::all_stdlib_modules_have_no_errors`; it is
/// intentionally not re-checked here to avoid maintenance-surface duplication.
#[test]
fn std_process_module_cardinality_locked() {
    let module = load_stdlib_module();

    // Exactly 1 enum: DFMSeverity.
    let enum_names: Vec<&str> = module.enum_defs.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(
        module.enum_defs.len(),
        1,
        "std/process should declare exactly 1 enum (DFMSeverity), got: {:?}",
        enum_names
    );
    assert!(
        module.enum_defs.iter().any(|e| e.name == "DFMSeverity"),
        "std/process should contain the 'DFMSeverity' enum, got: {:?}",
        enum_names
    );

    // Exactly 9 traits: Process + 7 categories + DFMRule.
    let trait_names: Vec<&str> = module
        .trait_defs
        .iter()
        .map(|t| t.name.as_str())
        .collect();
    assert_eq!(
        module.trait_defs.len(),
        9,
        "std/process should declare exactly 9 traits \
         (Process + 7 categories + DFMRule), got: {:?}",
        trait_names
    );

    // Exactly 0 structures.
    let structure_names: Vec<&str> = module
        .templates
        .iter()
        .filter(|t| t.entity_kind == EntityKind::Structure)
        .map(|t| t.name.as_str())
        .collect();
    assert_eq!(
        structure_names.len(),
        0,
        "std/process should declare 0 structures, got: {:?}",
        structure_names
    );
}

/// examples/stdlib/process.ri must compile without errors and structurally
/// declare a `MilledPart : Subtracting` conformer and a `: DFMRule` conformer.
///
/// Guards are asserted structurally on the compiled module (via
/// `TopologyTemplate.trait_bounds`) rather than on the raw source text, so
/// comment-text drift or header edits cannot silently defeat them.
#[test]
fn example_process_ri_compiles_clean() {
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let example_path = manifest_dir
        .join("../../examples/stdlib/process.ri")
        .canonicalize()
        .expect("examples/stdlib/process.ri should exist on disk");

    let source =
        std::fs::read_to_string(&example_path).expect("failed to read examples/stdlib/process.ri");

    let compiled = compile_source_with_stdlib(&source);

    let example_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        example_errors.is_empty(),
        "examples/stdlib/process.ri should compile without errors; got: {:?}",
        example_errors
    );

    // Guard: example must declare a MilledPart structure conforming to Subtracting.
    // Structural assertion on `trait_bounds` — immune to comment-text drift.
    assert!(
        compiled.templates.iter().any(|t| {
            t.name == "MilledPart"
                && t.entity_kind == EntityKind::Structure
                && t.trait_bounds.contains(&"Subtracting".to_string())
        }),
        "examples/stdlib/process.ri should declare \
         'structure def MilledPart : Subtracting'; \
         found templates: {:?}",
        compiled
            .templates
            .iter()
            .map(|t| (&t.name, &t.trait_bounds))
            .collect::<Vec<_>>()
    );

    // Guard: example must declare a structure conforming to DFMRule.
    assert!(
        compiled.templates.iter().any(|t| {
            t.entity_kind == EntityKind::Structure
                && t.trait_bounds.contains(&"DFMRule".to_string())
        }),
        "examples/stdlib/process.ri should declare a structure conforming to 'DFMRule'; \
         found templates: {:?}",
        compiled
            .templates
            .iter()
            .map(|t| (&t.name, &t.trait_bounds))
            .collect::<Vec<_>>()
    );
}

// ─── step-3 (task-4273): β conformance signal ─────────────────────────────────

/// β conformance lock — part (a): a complete Subtracting conformer compiles clean
/// and its `min_feature_size` value cell is readable with the correct type.
///
/// Uses `compile_source_with_stdlib` at the same altitude as
/// `example_process_ri_compiles_clean`. Numeric evaluation of capability
/// members is exercised downstream by γ/δ via `reify check`/`eval` on examples.
#[test]
fn subtracting_conformer_with_all_params_compiles_clean_and_exposes_min_feature_size() {
    let source = r#"
import std.process

structure def MilledBracket : Subtracting {
    param duration : Time = 30min
    param cost : Money = 50USD
    param tool_access : Solid = box(10mm, 20mm, 30mm)
    param min_feature_size : Length = 1mm
    param achievable_finish : Length = 0.01mm
}
"#;

    let compiled = compile_source_with_stdlib(source);

    // (a-1) Zero error-severity diagnostics.
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "MilledBracket with all Subtracting params should compile clean; errors: {:?}",
        errors
    );

    // (a-2) The compiled MilledBracket template exposes a `min_feature_size` value cell
    //       with the correct type (Type::Scalar{LENGTH}).
    let bracket = compiled
        .templates
        .iter()
        .find(|t| t.name == "MilledBracket")
        .expect("MilledBracket template should be present after clean compile");

    let mfs_cell = bracket
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "min_feature_size")
        .unwrap_or_else(|| {
            panic!(
                "MilledBracket should have a 'min_feature_size' value cell; found: {:?}",
                bracket
                    .value_cells
                    .iter()
                    .map(|vc| &vc.id.member)
                    .collect::<Vec<_>>()
            )
        });

    assert_eq!(
        mfs_cell.cell_type,
        Type::Scalar {
            dimension: DimensionVector::LENGTH
        },
        "MilledBracket.min_feature_size cell_type should be Type::Scalar{{LENGTH}}"
    );
}

/// β conformance lock — part (b): a Subtracting conformer OMITTING `min_feature_size`
/// emits a `missing required member 'min_feature_size'` error diagnostic.
///
/// This is the durable lock for the conformance contract consumed by γ.
#[test]
fn subtracting_conformer_missing_min_feature_size_emits_missing_member_error() {
    let source = r#"
import std.process

structure def IncompleteMilled : Subtracting {
    param duration : Time = 30min
    param cost : Money = 50USD
    param tool_access : Solid = box(10mm, 20mm, 30mm)
    param achievable_finish : Length = 0.01mm
}
"#;
    // Note: `min_feature_size` is intentionally omitted.

    let compiled = compile_source_with_stdlib(source);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    assert!(
        !errors.is_empty(),
        "IncompleteMilled omitting 'min_feature_size' should emit an error diagnostic"
    );

    let error_msg = format!("{:?}", errors);
    assert!(
        error_msg.contains("missing required member") && error_msg.contains("min_feature_size"),
        "error should mention 'missing required member' and 'min_feature_size', got: {}",
        error_msg
    );
}

// ─── step-5 (task-4274): FeatureManufacturable compile-clean ─────────────────

/// γ compile-clean lock — `FeatureManufacturable` over a full Subtracting conformer.
///
/// A `MilledBracket` conformer (all five Subtracting params supplied) is bound
/// via `let proc = MilledBracket()` inside `CheckedPart`, which applies
/// `constraint FeatureManufacturable(proc: proc, feature: wall)`.
/// Since `FeatureManufacturable` reads `proc.min_feature_size` (a Length on
/// Subtracting), the compiler must resolve the member access on a trait-typed
/// let-binding — the test confirms this type-checks with zero Error diagnostics.
///
/// Compile-only (no kernel/eval) because `MilledBracket.tool_access` is a Solid
/// and `check_source_with_stdlib` uses a no-kernel engine — the runtime
/// OK→VIOLATED flip for `feature >= proc.min_feature_size` is left to δ.
///
/// RED: `FeatureManufacturable` does not exist yet → "unknown constraint def" error.
#[test]
fn feature_manufacturable_over_subtracting_conformer_compiles_clean() {
    let source = r#"
import std.process

structure def MilledBracket : Subtracting {
    param duration         : Time   = 30min
    param cost             : Money  = 50USD
    param tool_access      : Solid  = box(10mm, 20mm, 30mm)
    param min_feature_size : Length = 1mm
    param achievable_finish: Length = 0.01mm
}

structure def CheckedPart {
    let proc  = MilledBracket()
    param wall : Length = 0.5mm
    constraint FeatureManufacturable(proc: proc, feature: wall)
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
        "FeatureManufacturable over Subtracting conformer should compile clean; errors: {:?}",
        errors
    );
}

// Shared FdmPrinter : Adding fixture (all required params including max_overhang_angle).
// Used by `fits_build_volume_over_adding_conformer_compiles_clean` and
// `adding_conformer_with_all_params_compiles_clean_and_exposes_max_overhang_angle`
// so both tests stay in sync when the Adding trait surface changes.
const FDMPRINTER_ALL_PARAMS_SOURCE: &str = r#"
import std.process

structure def FdmPrinter : Adding {
    param duration           : Time   = 60min
    param cost               : Money  = 10USD
    param layer_thickness    : Length = 0.2mm
    param min_feature_size   : Length = 0.4mm
    param build_volume       : Solid  = box(200mm, 200mm, 200mm)
    param max_overhang_angle : Angle  = 45deg
}
"#;

// ─── step-7 (task-4274): FitsBuildVolume compile-clean + cardinality lock ─────

/// γ compile-clean lock — `FitsBuildVolume` over a full Adding conformer.
///
/// An `FdmPrinter` conformer (all five Adding params supplied, including
/// `build_volume : Solid`) is bound via `let proc = FdmPrinter()` inside
/// `SmallPart`, which applies
/// `constraint FitsBuildVolume(proc: proc, part: part)`.
/// The predicate `fits_build_volume(bounding_box(part), bounding_box(proc.build_volume))`
/// is geometry-backed (both args are Solids resolved to BoundingBox by
/// `bounding_box(...)`); compile-clean means the type-checker accepts it.
///
/// Compile-only (no kernel/eval) — runtime OK→VIOLATED flip is δ's.
///
/// RED: `FitsBuildVolume` does not exist yet → "unknown constraint def" error.
#[test]
fn fits_build_volume_over_adding_conformer_compiles_clean() {
    let source = format!(
        r#"{}
structure def SmallPart {{
    let proc = FdmPrinter()
    param part : Solid = box(50mm, 50mm, 50mm)
    constraint FitsBuildVolume(proc: proc, part: part)
}}"#,
        FDMPRINTER_ALL_PARAMS_SOURCE
    );

    let compiled = compile_source_with_stdlib(&source);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "FitsBuildVolume over Adding conformer should compile clean; errors: {:?}",
        errors
    );
}

// ─── task-4407 step-1: Adding.max_overhang_angle RED tests ───────────────────

/// β conformance lock — Adding.max_overhang_angle (negative):
/// An FdmPrinter : Adding conformer omitting `max_overhang_angle` must
/// emit a missing-required-member Error diagnostic.
#[test]
fn adding_conformer_missing_max_overhang_angle_emits_missing_member_error() {
    let source = r#"
import std.process

structure def FdmPrinter : Adding {
    param duration         : Time   = 60min
    param cost             : Money  = 10USD
    param layer_thickness  : Length = 0.2mm
    param min_feature_size : Length = 0.4mm
    param build_volume     : Solid  = box(200mm, 200mm, 200mm)
}
"#;
    // Note: `max_overhang_angle` is intentionally omitted.

    let compiled = compile_source_with_stdlib(source);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    assert!(
        !errors.is_empty(),
        "FdmPrinter omitting 'max_overhang_angle' should emit an error diagnostic"
    );

    let error_msg = format!("{:?}", errors);
    assert!(
        error_msg.contains("missing required member") && error_msg.contains("max_overhang_angle"),
        "error should mention 'missing required member' and 'max_overhang_angle', got: {}",
        error_msg
    );
}

/// β conformance lock — Adding.max_overhang_angle (positive):
/// A complete FdmPrinter : Adding conformer supplying all params including
/// `param max_overhang_angle : Angle = 45deg` compiles with zero Error
/// diagnostics and exposes a `max_overhang_angle` value cell of type
/// Type::Scalar{ANGLE} (the cell γ reads).
#[test]
fn adding_conformer_with_all_params_compiles_clean_and_exposes_max_overhang_angle() {
    let compiled = compile_source_with_stdlib(FDMPRINTER_ALL_PARAMS_SOURCE);

    // (a-1) Zero error-severity diagnostics.
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "FdmPrinter with all Adding params should compile clean; errors: {:?}",
        errors
    );

    // (a-2) The compiled FdmPrinter template exposes a `max_overhang_angle` value cell
    //       with the correct type (Type::Scalar{ANGLE}).
    let printer = compiled
        .templates
        .iter()
        .find(|t| t.name == "FdmPrinter")
        .expect("FdmPrinter template should be present after clean compile");

    let moa_cell = printer
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "max_overhang_angle")
        .unwrap_or_else(|| {
            panic!(
                "FdmPrinter should have a 'max_overhang_angle' value cell; found: {:?}",
                printer
                    .value_cells
                    .iter()
                    .map(|vc| &vc.id.member)
                    .collect::<Vec<_>>()
            )
        });

    assert_eq!(
        moa_cell.cell_type,
        Type::Scalar {
            dimension: DimensionVector::ANGLE
        },
        "FdmPrinter.max_overhang_angle cell_type should be Type::Scalar{{ANGLE}}"
    );
}

// ─── cardinality lock + example compile ────────────────────────────────────────

/// γ name-presence lock — the std/process module must contain each of the six
/// named DFM constraint defs:
/// Manufacturable, FeatureManufacturable, BendManufacturable,
/// DrawManufacturable, DraftManufacturable, FitsBuildVolume.
///
/// Presence is checked by name (not by exact count) so that future tasks adding
/// additional DFM constraint defs do not spuriously fail this test.
///
/// RED (before step-8): `FitsBuildVolume` absent → the name check fails.
#[test]
fn process_module_has_exactly_six_dfm_constraint_defs() {
    let module = load_stdlib_module();

    let expected_names = [
        "Manufacturable",
        "FeatureManufacturable",
        "BendManufacturable",
        "DrawManufacturable",
        "DraftManufacturable",
        "FitsBuildVolume",
    ];

    let found_names: Vec<&str> = module
        .constraint_defs
        .iter()
        .map(|c| c.name.as_str())
        .collect();

    for name in &expected_names {
        assert!(
            found_names.contains(name),
            "std/process constraint_defs should contain '{}'; found: {:?}",
            name,
            found_names
        );
    }
}
