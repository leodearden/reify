//! Tests for the cost-aggregation stdlib idiom (task 2381).
//!
//! Locks the shape of `trait Costed : Buy` in `std/io` (helper trait) and the
//! `examples/cost_aggregation.ri` canonical example file. See
//! `docs/prds/money-dimension.md` §202–245 for the design rationale.
//!
//! File-stem `cost_aggregation` matches the
//! `cargo test -p reify-compiler -- cost_aggregation` filter used in this
//! task's testStrategy. Every test function name contains `cost_aggregation`
//! so that filter picks them up.

#[allow(dead_code)]
mod common;

use reify_compiler::{RequirementKind, stdlib_loader};
use reify_types::Type;

// ─── Helper: locate the std/io module ────────────────────────────────────────

fn io_module() -> &'static reify_compiler::CompiledModule {
    stdlib_loader::load_stdlib()
        .iter()
        .find(|m| format!("{}", m.path) == "std/io")
        .expect("std.io module should be present in the stdlib")
}

// ─── step-1: Costed trait shape — refinement + required quantity_produced ────

/// `Costed` must be present in `std/io`, refine exactly `[Buy]`, and require
/// `quantity_produced : Real` as a `RequirementKind::Param`.
///
/// Mirrors the `find_trait` / `param_type` closure pattern from
/// `io_traits_tests.rs::io_refining_traits_with_correct_params_and_dimensions`.
///
/// RED before step-2: `Costed` is not present in io.ri yet — `find_trait`
/// panics with "std.io should contain trait 'Costed'; found: [Source, Sink,
/// Input, Buy, Output, Discard]".
#[test]
fn cost_aggregation_costed_trait_present_in_std_io_with_required_quantity_produced() {
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
                    module.trait_defs.iter().map(|t| &t.name).collect::<Vec<_>>()
                )
            })
    };

    let costed = find_trait("Costed");

    // (a) refinements: exactly [Buy]
    assert_eq!(
        costed.refinements.as_slice(),
        ["Buy".to_string()].as_slice(),
        "Costed should refine exactly [Buy], got: {:?}",
        costed.refinements
    );

    // (b) required member quantity_produced : Real (RequirementKind::Param(Real))
    let req = costed
        .required_members
        .iter()
        .find(|r| r.name == "quantity_produced")
        .unwrap_or_else(|| {
            panic!(
                "Costed should have required member 'quantity_produced'; found: {:?}",
                costed.required_members.iter().map(|r| &r.name).collect::<Vec<_>>()
            )
        });
    match &req.kind {
        RequirementKind::Param(ty) => assert_eq!(
            *ty,
            Type::Real,
            "Costed.quantity_produced should be RequirementKind::Param(Type::Real), got Param({:?})",
            ty
        ),
        other => panic!(
            "Costed.quantity_produced should be RequirementKind::Param(Type::Real), got {:?}",
            other
        ),
    }
}
