//! User-observable-signal pin for **task 3603 / GHR-α (PRD §8 Phase 1)**.
//!
//! This file is the entry point for future readers tracing the
//! `geometry-handle-runtime` PRD Phase 1 wiring. It exercises:
//!
//!   1. Stdlib geometry-query helper calls (`volume`, `centroid`) typecheck
//!      to the correct return Type at compile-time. (Eval-time dispatch
//!      arrives in Phase 6 / GHR-ζ; Phase 1 produces `Value::Undef`.)
//!   2. Spec-shape `Physical` trait: a structure conforming to `Physical`
//!      via `param geometry : Solid` + `param material : Material` (instead
//!      of the legacy flat-scalar `param density / volume / centroid_x/y/z`
//!      params) compiles with NO error-severity diagnostics, gains
//!      `mass` and `centroid` value cells from the trait's let defaults,
//!      and pulls `material.density` via struct-member access (SIR-α).
//!
//! See `docs/prds/v0_3/geometry-handle-runtime.md` §1 + §8.

use reify_test_support::compile_source_with_stdlib;
use reify_core::{DimensionVector, Severity, Type};

/// `volume(my_box)` where `my_box : Solid` typechecks to `Scalar<Volume>`.
///
/// Pins the dispatch arm in `expr.rs::infer_type` that consults
/// `geometry_query_result_type` (added in step-8). Without that arm, the
/// inference falls through to the first-arg type (`Geometry`), which fails
/// `is_representable_cell_type`.
#[test]
fn spec_shape_volume_call_typechecks_to_scalar_volume() {
    let source = r#"
structure def MyBox {
    param my_box : Solid = box(10mm, 20mm, 30mm)
    let v = volume(my_box)
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
        "expected zero error-severity diagnostics, got: {:?}",
        errors
    );

    let template = compiled
        .templates
        .first()
        .expect("expected at least one template");
    let v_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "v")
        .expect("expected 'v' value cell from `let v = volume(my_box)`");
    assert_eq!(
        v_cell.cell_type,
        Type::Scalar {
            dimension: DimensionVector::VOLUME
        },
        "`volume(...)` must infer to Scalar<Volume>"
    );
}

/// `centroid(my_box)` where `my_box : Solid` typechecks to `Point3<Length>`.
///
/// Pins the second sample from the GHR-α §1 frozen list; matches the same
/// dispatch arm as the volume test above.
#[test]
fn spec_shape_centroid_call_typechecks_to_point3_length() {
    let source = r#"
structure def MyBox {
    param my_box : Solid = box(10mm, 20mm, 30mm)
    let c = centroid(my_box)
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
        "expected zero error-severity diagnostics, got: {:?}",
        errors
    );

    let template = compiled
        .templates
        .first()
        .expect("expected at least one template");
    let c_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "c")
        .expect("expected 'c' value cell from `let c = centroid(my_box)`");
    assert_eq!(
        c_cell.cell_type,
        Type::point3(Type::length()),
        "`centroid(...)` must infer to Point3<Length>"
    );
}

// ─── headline Phase-1 integration: spec-shape Bracket lowering pin ───────────

/// Spec-shape `Bracket : Physical` lowers `param geometry : Solid` to BOTH a
/// **realization** AND a **value cell** (`cell_type == Type::Geometry`).
///
/// After GHR-γ (task 3605, bypass retired), `param geometry : Solid` now emits
/// a `ValueCellDecl{cell_type: Type::Geometry, kind: Param}` alongside the
/// existing `RealizationDecl`.  The `ValueCellDecl` carries the compiled
/// geometry-call default expr; the `RealizationDecl` drives the kernel dispatch.
///
/// This is the unique coverage this file contributes over its sibling
/// `structural_physical_tests.rs`, which pins clean compilation + presence
/// of `mass` / `centroid` / `material` value cells + the `Physical` trait
/// bound. The "Solid-typed params lower to BOTH a value cell AND a realization"
/// invariant (post-GHR-γ) is pinned here as the cross-product check between
/// SIR-α struct-member access, the geometry-query dispatch arm, and the
/// realization-lowering path for `Solid` params.
///
/// Sibling test for the redundant trait-schema + `mass`/`centroid` checks:
/// see `physical_trait_has_geometry_and_material_params_only` +
/// `physical_trait_has_mass_and_centroid_lets` +
/// `bracket_conforms_to_physical_with_geometry_and_material` in
/// `structural_physical_tests.rs`.
#[test]
fn spec_shape_physical_bracket_lowers_geometry_to_realization() {
    let source = r#"
structure def Bracket : Physical {
    param geometry : Solid = box(10mm, 20mm, 30mm)
    param material : Material = Material(name: "steel", density: 7850kg/m^3, youngs_modulus: 200GPa)
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
        "spec-shape `Bracket : Physical` should compile clean (no errors); got: {:?}",
        errors
    );

    let bracket = compiled
        .templates
        .iter()
        .find(|t| t.name == "Bracket")
        .expect("Bracket template should be compiled");

    // After GHR-γ: `geometry : Solid` lowers to BOTH a RealizationDecl AND a
    // ValueCellDecl{cell_type: Type::Geometry} — the bypass that previously
    // skipped value-cell creation has been retired.
    assert!(
        !bracket.realizations.is_empty(),
        "Bracket should have at least one realization (from `param geometry : Solid = box(...)`); got none"
    );
    let geom_cells: Vec<_> = bracket
        .value_cells
        .iter()
        .filter(|vc| vc.id.member == "geometry")
        .collect();
    assert_eq!(
        geom_cells.len(),
        1,
        "After GHR-γ: Bracket MUST have exactly 1 ValueCellDecl for 'geometry' \
         (cell_type=Type::Geometry); got members: {:?}",
        bracket
            .value_cells
            .iter()
            .map(|vc| vc.id.member.as_str())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        geom_cells[0].cell_type,
        Type::Geometry,
        "expected cell_type=Type::Geometry for 'geometry' value cell, got {:?}",
        geom_cells[0].cell_type
    );
}

// ─── precedence pin: user fns shadow the geometry-query family ───────────────

/// **Dispatch-precedence regression** (reviewer #5 follow-up). The
/// geometry-query family added by GHR-α (PRD §1 — `volume` / `area` /
/// `length` / `contains` / `distance` / `angle` / `curvature` / …) overlaps
/// with names that are perfectly valid user-function identifiers. A user
/// `fn length(s: String) -> Int { … }` in scope MUST resolve to the
/// user-defined fn — not to the stdlib `length(curve) -> Scalar<Length>`
/// registration — and its return type MUST be the user's declared return,
/// not `Scalar<Length>`.
///
/// The precedence is enforced structurally by `resolve_function_overload`:
/// when any user fn matches the name, it returns `Resolved` / `Ambiguous` /
/// `NoMatch` before the `NoUserFunctions` arm where `is_geometry_query` is
/// consulted. This test pins that contract end-to-end.
///
/// Cross-reference: dispatch-precedence note in `expr.rs::infer_type`
/// (immediately before the `is_geometry_query_helper` chain in the
/// `NoUserFunctions` arm).
#[test]
fn user_defined_length_shadows_stdlib_geometry_query() {
    // User defines `length(s) -> Int` — a wholly different signature and
    // return type from the stdlib `length(curve) -> Scalar<Length>`. The
    // call `length("hello")` must dispatch to the user fn.
    let source = r#"
fn length(s: Real) -> Int {
    42
}

structure def StringHolder {
    param raw : Real = 3.14
    let measured = length(raw)
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
        "user-defined `fn length(Real) -> Int` should shadow stdlib geometry-query \
         `length` cleanly; got errors: {:?}",
        errors
    );

    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "StringHolder")
        .expect("StringHolder template should be compiled");

    let measured = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "measured")
        .expect("expected 'measured' value cell from `let measured = length(raw)`");

    // The user fn returns `Int`. The stdlib geometry-query `length` would
    // have returned `Scalar<Length>`. If the dispatch ordering ever inverted
    // (e.g. by moving the geometry-query arm above the user-fn resolution
    // step), this assertion would catch it: `measured` would be Scalar<Length>
    // instead of Int.
    assert_eq!(
        measured.cell_type,
        Type::Int,
        "`length(raw)` must resolve to the user-defined `fn length(Real) -> Int`, \
         NOT the stdlib geometry-query `length(curve) -> Scalar<Length>`; got cell_type {:?}",
        measured.cell_type
    );
}
