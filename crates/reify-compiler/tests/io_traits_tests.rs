//! Tests for the std.io stdlib module.
//!
//! Exercises: marker traits (Source/Sink), enums (DiscardReason/DisposalMethod/
//! OutputFormat), Provenance structure, and the four refining traits (Input,
//! Buy, Output, Discard) including Buy.unit_cost having Money dimension.
//!
//! File-stem `io_traits` matches the `cargo test -p reify-compiler -- io_traits`
//! filter used in the task testStrategy.
//!
//! Coverage: "no error diagnostics" is centrally checked by
//! `stdlib_loader_tests.rs::all_stdlib_modules_have_no_errors`; module
//! presence is implicitly exercised by the `io_module()` `expect(...)` panic.

use reify_compiler::{RequirementKind, stdlib_loader};
use reify_core::{DimensionVector, Type};

// Helper: find the std/io module (panics with a clear message if absent).
fn io_module() -> &'static reify_compiler::CompiledModule {
    stdlib_loader::load_stdlib()
        .iter()
        .find(|m| format!("{}", m.path) == "std/io")
        .expect("std.io module should be present in the stdlib")
}

// ─── step-9: refining traits ─────────────────────────────────────────────────

/// Input, Buy, Output, Discard have correct refinements and param types.
/// The critical assertion is Buy.unit_cost having DimensionVector::MONEY.
#[test]
fn io_refining_traits_with_correct_params_and_dimensions() {
    let module = io_module();

    // Helper: find a trait by name or panic.
    let find_trait = |name: &str| {
        module
            .trait_defs
            .iter()
            .find(|t| t.name == name)
            .unwrap_or_else(|| {
                panic!(
                    "std.io should contain trait '{}'; found: {:?}",
                    name,
                    module
                        .trait_defs
                        .iter()
                        .map(|t| &t.name)
                        .collect::<Vec<_>>()
                )
            })
    };

    // Helper: get the Param type for a named required member or panic.
    let param_type = |trait_name: &str, member: &str| -> Type {
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
    };

    // Input : Source — source: String, provenance: StructureRef("Provenance")
    let refinements = &find_trait("Input").refinements;
    assert_eq!(
        refinements.as_slice(),
        ["Source".to_string()].as_slice(),
        "Input should refine exactly [Source], got: {:?}",
        refinements
    );
    assert_eq!(param_type("Input", "source"), Type::String);
    assert_eq!(
        param_type("Input", "provenance"),
        Type::StructureRef("Provenance".into())
    );

    // Buy : Source — supplier, part_number: String; unit_cost: Money; lead_time: Time
    let refinements = &find_trait("Buy").refinements;
    assert_eq!(
        refinements.as_slice(),
        ["Source".to_string()].as_slice(),
        "Buy should refine exactly [Source], got: {:?}",
        refinements
    );
    assert_eq!(param_type("Buy", "supplier"), Type::String);
    assert_eq!(param_type("Buy", "part_number"), Type::String);
    // Critical: Buy.unit_cost must have Money dimension.
    assert_eq!(
        param_type("Buy", "unit_cost"),
        Type::Scalar {
            dimension: DimensionVector::MONEY
        },
        "Buy.unit_cost must have DimensionVector::MONEY"
    );
    assert_eq!(
        param_type("Buy", "lead_time"),
        Type::Scalar {
            dimension: DimensionVector::TIME
        },
        "Buy.lead_time must have DimensionVector::TIME"
    );

    // Output : Sink — format: Enum("OutputFormat")
    let refinements = &find_trait("Output").refinements;
    assert_eq!(
        refinements.as_slice(),
        ["Sink".to_string()].as_slice(),
        "Output should refine exactly [Sink], got: {:?}",
        refinements
    );
    assert_eq!(
        param_type("Output", "format"),
        Type::Enum("OutputFormat".into())
    );

    // Discard : Sink — reason: Enum("DiscardReason"), disposal_method: Enum("DisposalMethod")
    let refinements = &find_trait("Discard").refinements;
    assert_eq!(
        refinements.as_slice(),
        ["Sink".to_string()].as_slice(),
        "Discard should refine exactly [Sink], got: {:?}",
        refinements
    );
    assert_eq!(
        param_type("Discard", "reason"),
        Type::Enum("DiscardReason".into())
    );
    assert_eq!(
        param_type("Discard", "disposal_method"),
        Type::Enum("DisposalMethod".into())
    );
}

// ─── step-7: Provenance structure ────────────────────────────────────────────

/// The Provenance structure is present in std/io with the four expected
/// value cells: source_tool: String, source_version: String,
/// timestamp: String, tolerance_guarantee: Length.
#[test]
fn provenance_structure_present_with_correct_fields() {
    let module = io_module();

    let provenance = module
        .templates
        .iter()
        .find(|t| t.name == "Provenance")
        .expect("std.io should contain a Provenance structure");

    // Helper closure: find a value cell by member name.
    let find_cell = |member: &str| {
        provenance
            .value_cells
            .iter()
            .find(|vc| vc.id.member == member)
            .unwrap_or_else(|| {
                panic!(
                    "Provenance should have a '{}' cell; found members: {:?}",
                    member,
                    provenance
                        .value_cells
                        .iter()
                        .map(|vc| &vc.id.member)
                        .collect::<Vec<_>>()
                )
            })
    };

    assert_eq!(find_cell("source_tool").cell_type, Type::String);
    assert_eq!(find_cell("source_version").cell_type, Type::String);
    assert_eq!(find_cell("timestamp").cell_type, Type::String);
    assert_eq!(
        find_cell("tolerance_guarantee").cell_type,
        Type::Scalar {
            dimension: DimensionVector::LENGTH
        }
    );
}

// ─── step-5: enums ───────────────────────────────────────────────────────────

/// DiscardReason, DisposalMethod, and OutputFormat are present with the variant
/// set from docs/reify-stdlib-reference.md §9.
#[test]
fn io_enums_present_with_expected_variants() {
    let module = io_module();

    struct EnumSpec {
        name: &'static str,
        variants: &'static [&'static str],
    }

    let specs = [
        EnumSpec {
            name: "DiscardReason",
            variants: &["Offcut", "Scrap", "FailedInspection", "Waste"],
        },
        EnumSpec {
            name: "DisposalMethod",
            variants: &["Recycle", "Landfill", "Reprocess"],
        },
        EnumSpec {
            name: "OutputFormat",
            variants: &["STEP", "STL", "ThreeMF", "Display"],
        },
    ];

    for spec in &specs {
        let enum_def = module
            .enum_defs
            .iter()
            .find(|e| e.name == spec.name)
            .unwrap_or_else(|| {
                panic!(
                    "std.io should contain enum '{}'; found: {:?}",
                    spec.name,
                    module.enum_defs.iter().map(|e| &e.name).collect::<Vec<_>>()
                )
            });

        assert_eq!(
            enum_def.variants.len(),
            spec.variants.len(),
            "enum '{}' should have {} variants, got {}: {:?}",
            spec.name,
            spec.variants.len(),
            enum_def.variants.len(),
            enum_def.variants
        );

        for variant in spec.variants {
            assert!(
                enum_def.variants.contains(&variant.to_string()),
                "enum '{}' should contain variant '{}', found: {:?}",
                spec.name,
                variant,
                enum_def.variants
            );
        }
    }
}

// ─── step-3: marker traits ───────────────────────────────────────────────────

/// Source and Sink are pure marker traits: no refinements, no required members,
/// no defaults — matching the geometry_traits convention for marker traits.
#[test]
fn io_source_and_sink_marker_traits_present() {
    let module = io_module();

    for trait_name in &["Source", "Sink"] {
        let t = module
            .trait_defs
            .iter()
            .find(|t| t.name == *trait_name)
            .unwrap_or_else(|| {
                panic!(
                    "std.io should contain trait '{}'; found: {:?}",
                    trait_name,
                    module
                        .trait_defs
                        .iter()
                        .map(|t| &t.name)
                        .collect::<Vec<_>>()
                )
            });

        assert!(
            t.refinements.is_empty(),
            "trait '{}' should have no refinements, got: {:?}",
            trait_name,
            t.refinements
        );
        assert!(
            t.required_members.is_empty(),
            "trait '{}' should have no required members, got: {:?}",
            trait_name,
            t.required_members
                .iter()
                .map(|r| &r.name)
                .collect::<Vec<_>>()
        );
        assert!(
            t.defaults.is_empty(),
            "trait '{}' should have no defaults, got {} entries",
            trait_name,
            t.defaults.len()
        );
    }
}
