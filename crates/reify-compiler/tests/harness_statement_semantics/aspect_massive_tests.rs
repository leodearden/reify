//! Tests for the `Massive` aspect trait in `std/io` and the
//! `examples/aspect_massive.ri` aggregation example (task 5019).
//!
//! `Massive` is an independent aspect trait, peer to `Costed`, establishing
//! an aspect vocabulary beyond cost. See
//! `docs/prds/v0_6/multi-aspect-objective-units-coherence.md` §D3: aspects
//! are independent traits with no common `Aspect` supertrait, avoiding the
//! "cost is special" trap.
//!
//! File-stem `aspect_massive` matches the
//! `cargo test -p reify-compiler -- aspect_massive` filter used in this
//! task's testStrategy. Every test function name contains `aspect_massive`
//! so that filter picks them up.

use reify_compiler::{RequirementKind, TopologyTemplate, stdlib_loader};
use reify_core::{DimensionVector, ModulePath, Severity, Type};

// ─── Helper: locate the std/io module ────────────────────────────────────────

fn io_module() -> &'static reify_compiler::CompiledModule {
    stdlib_loader::load_stdlib()
        .iter()
        .find(|m| format!("{}", m.path) == "std/io")
        .expect("std.io module should be present in the stdlib")
}

// ─── step-1: Massive trait shape — standalone + required mass ───────────────

/// `Massive` must be present in `std/io`, be STANDALONE (no refinements —
/// unlike `Costed : Buy`, `Massive` has no supertrait per PRD D3), and
/// require `mass : Mass` as a `RequirementKind::Param`.
///
/// Mirrors the `find_trait` / refinements-empty pattern from
/// `io_traits_tests.rs::io_source_and_sink_marker_traits_present` (for the
/// standalone-ness check) and the required-param assertion pattern from
/// `cost_aggregation_tests.rs`.
#[test]
fn aspect_massive_trait_present_in_std_io_standalone_with_required_mass() {
    let module = io_module();

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

    let massive = find_trait("Massive");

    // (a) standalone: no supertrait (contrast Costed : Buy).
    assert!(
        massive.refinements.is_empty(),
        "Massive should be a standalone trait with no refinements (PRD D3: aspects \
         are independent, no common supertrait), got: {:?}",
        massive.refinements
    );

    // (b) required member mass : Mass (RequirementKind::Param(Scalar<MASS>))
    let req = massive
        .required_members
        .iter()
        .find(|r| r.name == "mass")
        .unwrap_or_else(|| {
            panic!(
                "Massive should have required member 'mass'; found: {:?}",
                massive
                    .required_members
                    .iter()
                    .map(|r| &r.name)
                    .collect::<Vec<_>>()
            )
        });
    match &req.kind {
        RequirementKind::Param(ty) => assert_eq!(
            *ty,
            Type::Scalar {
                dimension: DimensionVector::MASS
            },
            "Massive.mass should be RequirementKind::Param(Type::Scalar {{ dimension: MASS }}), got Param({:?})",
            ty
        ),
        other => panic!(
            "Massive.mass should be RequirementKind::Param(Type::Scalar {{ dimension: MASS }}), got {:?}",
            other
        ),
    }
}

// ─── step-3: examples/aspect_massive.ri compiles clean under stdlib ─────────

/// The canonical example file `examples/aspect_massive.ri` must parse,
/// compile under the stdlib prelude with zero Error diagnostics, and expose:
///   - a `Bracket` template carrying a `line_cost` cell (from Costed) typed
///     `Scalar<MONEY>` and a `mass` cell (from Massive) typed `Scalar<MASS>`;
///   - an `AssemblyBracket` template carrying `total_cost` (`Scalar<MONEY>`)
///     and `total_mass` (`Scalar<MASS>`) value cells, aggregating BOTH
///     aspects via explicit named-sub member access.
///
/// Mirrors `cost_aggregation_example_compiles_under_stdlib_with_zero_errors`
/// (CARGO_MANIFEST_DIR-anchored path, parse + assert no parse errors,
/// compile_with_stdlib + filter to Severity::Error, assert empty).
///
/// RED: examples/aspect_massive.ri does not exist yet — read_to_string panics.
#[test]
fn aspect_massive_example_compiles_under_stdlib_with_zero_errors() {
    const EXAMPLE_PATH: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/aspect_massive.ri"
    );
    let src = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("failed to read examples/aspect_massive.ri — check CARGO_MANIFEST_DIR resolution");

    let parsed = reify_syntax::parse(&src, ModulePath::single("aspect_massive"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors in aspect_massive.ri: {:?}",
        parsed.errors
    );

    let module = reify_compiler::compile_with_stdlib(&parsed);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected zero Error diagnostics compiling aspect_massive.ri under stdlib, got:\n{:#?}",
        errors
    );

    // Helper: find a template by name or panic.
    let find_template = |name: &str| {
        module
            .templates
            .iter()
            .find(|t| t.name == name)
            .unwrap_or_else(|| {
                panic!(
                    "template '{}' should be present in compiled aspect_massive.ri; \
                     found templates: {:?}",
                    name,
                    module.templates.iter().map(|t| &t.name).collect::<Vec<_>>()
                )
            })
    };

    // Helper: find a value cell by member name on a template, or panic.
    let find_cell = |template: &TopologyTemplate, member: &str| {
        template
            .value_cells
            .iter()
            .find(|c| c.id.member == member)
            .unwrap_or_else(|| {
                panic!(
                    "template '{}' should carry a '{}' value cell; found cells: {:?}",
                    template.name,
                    member,
                    template
                        .value_cells
                        .iter()
                        .map(|c| &c.id.member)
                        .collect::<Vec<_>>()
                )
            })
            .cell_type
            .clone()
    };

    let bracket = find_template("Bracket");
    assert_eq!(
        find_cell(bracket, "line_cost"),
        Type::Scalar {
            dimension: DimensionVector::MONEY
        },
        "Bracket.line_cost should have type Scalar<MONEY>"
    );
    assert_eq!(
        find_cell(bracket, "mass"),
        Type::Scalar {
            dimension: DimensionVector::MASS
        },
        "Bracket.mass should have type Scalar<MASS>"
    );

    let assembly = find_template("AssemblyBracket");
    assert_eq!(
        find_cell(assembly, "total_cost"),
        Type::Scalar {
            dimension: DimensionVector::MONEY
        },
        "AssemblyBracket.total_cost should have type Scalar<MONEY>"
    );
    assert_eq!(
        find_cell(assembly, "total_mass"),
        Type::Scalar {
            dimension: DimensionVector::MASS
        },
        "AssemblyBracket.total_mass should have type Scalar<MASS>"
    );
}
