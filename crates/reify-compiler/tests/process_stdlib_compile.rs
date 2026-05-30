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
/// have an empty own required_members (requirements are inherited via refinement).
#[test]
fn process_category_traits_each_refine_process() {
    let categories = [
        "Subtracting",
        "Adding",
        "Forming",
        "Joining",
        "Parting",
        "SurfaceTreating",
        "HeatTreating",
    ];

    for name in &categories {
        let t = find_trait(name);

        assert_eq!(
            t.refinements.as_slice(),
            ["Process".to_string()].as_slice(),
            "trait '{}' should refine exactly [Process], got: {:?}",
            name,
            t.refinements
        );

        assert!(
            t.required_members.is_empty(),
            "trait '{}' should have no own required_members (inherited via refinement), \
             got: {:?}",
            name,
            t.required_members
                .iter()
                .map(|r| &r.name)
                .collect::<Vec<_>>()
        );
    }
}

// ─── step-7: DFMRule trait surface ───────────────────────────────────────────

/// DFMRule has no refinements and exactly three required params in order:
/// rule_name : String, severity : DFMSeverity, applies_to : Process.
#[test]
fn dfmrule_trait_surface_has_rule_name_severity_and_process_applicability() {
    let t = find_trait("DFMRule");

    assert!(
        t.refinements.is_empty(),
        "DFMRule should have no refinements, got: {:?}",
        t.refinements
    );

    // Exactly three required members, in declaration order.
    assert_eq!(
        t.required_members.len(),
        3,
        "DFMRule should have exactly 3 required members; got: {:?}",
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
}

// ─── step-9: capstone — example compiles clean + cardinality locked ───────────

/// examples/stdlib/process.ri must compile without errors, contain
/// a MilledPart : Subtracting conformer and a : DFMRule conformer.
/// Also locks std/process cardinality: exactly 1 enum, 9 traits, 0 structures.
#[test]
fn example_compiles_clean_and_module_cardinality_locked() {
    // ── Cardinality lock on std/process ──────────────────────────────────────
    let module = load_stdlib_module();

    // Zero error diagnostics.
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "std/process should have zero error diagnostics, got: {:?}",
        errors
    );

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

    // ── examples/stdlib/process.ri compiles clean ────────────────────────────
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

    // Guard: example must declare MilledPart : Subtracting and a : DFMRule conformer.
    assert!(
        source.contains("MilledPart"),
        "examples/stdlib/process.ri should declare a MilledPart structure"
    );
    assert!(
        source.contains(": Subtracting"),
        "examples/stdlib/process.ri should have a : Subtracting conformer"
    );
    assert!(
        source.contains(": DFMRule"),
        "examples/stdlib/process.ri should have a : DFMRule conformer"
    );
}
