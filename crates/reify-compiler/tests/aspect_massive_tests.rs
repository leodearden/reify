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

use reify_compiler::{RequirementKind, stdlib_loader};
use reify_core::{DimensionVector, Type};

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
