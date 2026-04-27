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

use reify_compiler::{DefaultKind, RequirementKind, stdlib_loader};
use reify_types::{DimensionVector, Severity, Type};

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

// ─── step-3: Costed exposes line_cost let-default with Money dimension ───────

/// `Costed` must provide `let line_cost : Money = unit_cost * quantity_produced`
/// as a `DefaultKind::Let` with `cell_type == Some(Scalar<MONEY>)`.
///
/// This locks the trait's promise that conforming structures inherit a
/// money-typed `line_cost` cell. Without the explicit `Money` annotation, the
/// trait-let cell_type would be `None` and the contract would only be
/// exercised through type inference at conformance sites.
///
/// RED before step-4: step-2 added only the param, not the let-default; the
/// `defaults` vec is empty so `find` returns None.
#[test]
fn cost_aggregation_costed_exposes_line_cost_let_default_with_money_dim() {
    let module = io_module();

    let costed = module
        .trait_defs
        .iter()
        .find(|t| t.name == "Costed")
        .expect("std.io should contain trait 'Costed'");

    let line_cost_default = costed
        .defaults
        .iter()
        .find(|d| d.name.as_deref() == Some("line_cost"))
        .unwrap_or_else(|| {
            panic!(
                "Costed should provide a let-default named 'line_cost'; found defaults: {:?}",
                costed.defaults.iter().map(|d| &d.name).collect::<Vec<_>>()
            )
        });

    match &line_cost_default.kind {
        DefaultKind::Let { cell_type, .. } => {
            assert_eq!(
                *cell_type,
                Some(Type::Scalar { dimension: DimensionVector::MONEY }),
                "Costed.line_cost should be DefaultKind::Let with cell_type Some(Scalar<MONEY>), got cell_type = {:?}",
                cell_type
            );
        }
        other => panic!(
            "Costed.line_cost should be DefaultKind::Let, got {:?}",
            other
        ),
    }
}

// ─── step-5: user structure conforming to Costed compiles clean ──────────────

/// A user `structure def CapScrew : Costed { ... }` with concrete defaults
/// for all four `Buy` params + `quantity_produced` must compile clean under
/// the stdlib prelude, and the resulting template must carry an inherited
/// `line_cost` value cell whose type is `Scalar<MONEY>`.
///
/// This is the conformance acceptance gate: it pins that the trait-let
/// default injection path correctly produces a money-typed cell on
/// conforming structures (the same machinery that lets
/// `examples/large_assembly.ri:252+` access `self.b01.mass` on a
/// `Physical : MaterialSpec`-conforming structure).
///
/// RED before step-4: any reference to `Costed` would resolve to "unknown
/// trait" and conformance would fail. After step-4: the structure conforms
/// and the trait-let cell is injected — test goes GREEN with no further
/// code change.
#[test]
fn cost_aggregation_user_structure_conforming_to_costed_compiles_clean_under_stdlib() {
    let source = r#"
structure def CapScrew : Costed {
    param supplier         : String = "McMaster-Carr"
    param part_number      : String = "91251A190"
    param unit_cost        : Money  = 0.12USD
    param lead_time        : Time   = 24h
    param quantity_produced : Real  = 24.0
}
"#;
    let module = common::compile_with_stdlib_helper(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected zero Error diagnostics compiling CapScrew : Costed, got:\n{:#?}",
        errors
    );

    let cap_screw = module
        .templates
        .iter()
        .find(|t| t.name == "CapScrew")
        .expect("CapScrew template should be present in compiled module");

    let line_cost_cell = cap_screw
        .value_cells
        .iter()
        .find(|c| c.id.member == "line_cost")
        .unwrap_or_else(|| {
            panic!(
                "CapScrew should inherit a 'line_cost' value cell from Costed; \
                 found cells: {:?}",
                cap_screw
                    .value_cells
                    .iter()
                    .map(|c| &c.id.member)
                    .collect::<Vec<_>>()
            )
        });

    assert_eq!(
        line_cost_cell.cell_type,
        Type::Scalar { dimension: DimensionVector::MONEY },
        "CapScrew.line_cost should have type Scalar<MONEY>, got {:?}",
        line_cost_cell.cell_type
    );
}
