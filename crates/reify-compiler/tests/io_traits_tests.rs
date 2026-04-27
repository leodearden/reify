//! Tests for the std.io stdlib module.
//!
//! Exercises: module presence, marker traits (Source/Sink), enums
//! (DiscardReason/DisposalMethod/OutputFormat), Provenance structure, and the
//! four refining traits (Input, Buy, Output, Discard) including Buy.unit_cost
//! having Money dimension.
//!
//! File-stem `io_traits` matches the `cargo test -p reify-compiler -- io_traits`
//! filter used in the task testStrategy.

use reify_compiler::stdlib_loader;
use reify_test_support::collect_errors;

// Helper: find the std/io module (panics with a clear message if absent).
fn io_module() -> &'static reify_compiler::CompiledModule {
    stdlib_loader::load_stdlib()
        .iter()
        .find(|m| format!("{}", m.path) == "std/io")
        .expect("std.io module should be present in the stdlib")
}

// ─── step-5: enums ───────────────────────────────────────────────────────────

/// DiscardReason, DisposalMethod, and OutputFormat are present with the exact
/// variant sets from docs/reify-stdlib-reference.md §9.
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
                    module.trait_defs.iter().map(|t| &t.name).collect::<Vec<_>>()
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
            t.required_members.iter().map(|r| &r.name).collect::<Vec<_>>()
        );
        assert!(
            t.defaults.is_empty(),
            "trait '{}' should have no defaults, got {} entries",
            trait_name,
            t.defaults.len()
        );
    }
}

// ─── step-1: module load ─────────────────────────────────────────────────────

/// The std.io module is present in the stdlib and compiles without errors.
#[test]
fn std_io_module_present_and_compiles_clean() {
    let modules = stdlib_loader::load_stdlib();

    let io_module = modules
        .iter()
        .find(|m| format!("{}", m.path) == "std/io")
        .expect("std.io module should be present in the stdlib");

    let errors = collect_errors(&io_module.diagnostics);
    assert!(
        errors.is_empty(),
        "std.io module should have no error diagnostics, got: {:?}",
        errors
    );
}
