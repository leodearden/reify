//! Unit tests for `seed_template_literal_params` helper (task 4599).
//!
//! # What this file covers
//!
//! Helper unit tests — build a synthetic parameterized `TopologyTemplate`
//! and assert that `seed_template_literal_params` extracts exactly the
//! direct-literal-default cells, keyed by each cell's **own** `id`
//! (entity = template name, e.g. `ValueCellId::new("Bearing","bore_radius")`).
//!
//! # RED state
//!
//! - **Step 1 (RED):** `seed_template_literal_params` does not yet exist —
//!   this file fails to compile at the `use ...::seed_template_literal_params`
//!   import line.
//!
//! - **Step 2 (GREEN):** `seed_template_literal_params` is added to
//!   `auto_type_param.rs`; all tests in this file pass.

use reify_compiler::auto_type_param::seed_template_literal_params;
use reify_core::{DimensionVector, Type, ValueCellId};
use reify_ir::{CompiledExpr, Value};
use reify_test_support::TopologyTemplateBuilder;

// ─── helper: a length Value for n millimetres ────────────────────────────────

fn mm(n: f64) -> Value {
    Value::Scalar {
        si_value: n * 1e-3,
        dimension: DimensionVector::LENGTH,
    }
}

// ─── helper: a non-literal CompiledExpr (ValueRef) ──────────────────────────

fn non_literal_expr() -> CompiledExpr {
    // Any non-Literal kind is fine; ValueRef is the simplest to construct.
    CompiledExpr::value_ref(
        ValueCellId::new("Bearing", "bore_radius"),
        Type::length(),
    )
}

// ─── synthetic parameterized template ────────────────────────────────────────

/// Build a synthetic "Bearing" parameterized `TopologyTemplate` with:
/// - `bore_radius : Length = 10mm` — literal → seeded as `("Bearing","bore_radius")`
/// - `enabled     : Bool  = true`  — Bool literal → seeded as `("Bearing","enabled")`
///   (pins Gap B: value-kind-agnostic seeding)
/// - `seal : TypeParam("T1")` (no default) — must be skipped
/// - `color : Real` (no default)           — must be skipped
/// - `computed : Length = <ValueRef>`      — non-literal → must be skipped
fn bearing_template() -> reify_compiler::TopologyTemplate {
    TopologyTemplateBuilder::new("Bearing")
        .param(
            "Bearing",
            "bore_radius",
            Type::length(),
            Some(CompiledExpr::literal(mm(10.0), Type::length())),
        )
        .param(
            "Bearing",
            "enabled",
            Type::Bool,
            Some(CompiledExpr::literal(Value::Bool(true), Type::Bool)),
        )
        .param(
            "Bearing",
            "seal",
            Type::TypeParam("T1".to_string()),
            None,
        )
        .param("Bearing", "color", Type::dimensionless_scalar(), None)
        .param(
            "Bearing",
            "computed",
            Type::length(),
            Some(non_literal_expr()),
        )
        .build()
}

// ─── tests ───────────────────────────────────────────────────────────────────

/// `seed_template_literal_params` must:
///
/// (a) Seed literal-default cells keyed by their OWN `cell.id`
///     (entity = template name, member = param name).
/// (b) Skip `TypeParam` cells with `default_expr = None`.
/// (c) Skip `None`-default cells and non-literal-default cells.
/// (d) Be value-kind-agnostic — a `Value::Bool(true)` literal is seeded
///     alongside a `Value::Scalar` (Length) literal (pins Gap B).
/// (e) The map length equals exactly the count of literal-default cells.
#[test]
fn seed_extracts_literal_defaults_keyed_by_own_cell_id() {
    let tmpl = bearing_template();
    let map = seed_template_literal_params(&tmpl);

    // (a) + (d) literal-default cells are present with their own entity name as key
    let bore_key = ValueCellId::new("Bearing", "bore_radius");
    let enabled_key = ValueCellId::new("Bearing", "enabled");

    assert_eq!(
        map.get(&bore_key),
        Some(&mm(10.0)),
        "bore_radius must be seeded with the 10mm literal, keyed as (\"Bearing\",\"bore_radius\")"
    );
    assert_eq!(
        map.get(&enabled_key),
        Some(&Value::Bool(true)),
        "enabled (Bool literal) must be seeded — value-kind-agnostic (Gap B)"
    );

    // (b) TypeParam cell with None default must be absent
    let seal_key = ValueCellId::new("Bearing", "seal");
    assert!(
        map.get(&seal_key).is_none(),
        "seal (TypeParam/None) must NOT appear in the seeded ValueMap"
    );

    // (c) None-default and non-literal-default cells must be absent
    let color_key = ValueCellId::new("Bearing", "color");
    let computed_key = ValueCellId::new("Bearing", "computed");

    assert!(
        map.get(&color_key).is_none(),
        "color (no default) must NOT appear in the seeded ValueMap"
    );
    assert!(
        map.get(&computed_key).is_none(),
        "computed (non-literal ValueRef default) must NOT appear"
    );

    // (e) exactly 2 entries — bore_radius and enabled
    assert_eq!(
        map.len(),
        2,
        "map must contain exactly the two literal-default cells (bore_radius + enabled)"
    );
}

/// `seed_template_literal_params` returns an empty map when all cells have
/// `default_expr = None`.
#[test]
fn seed_returns_empty_map_for_template_with_no_literal_defaults() {
    let tmpl = TopologyTemplateBuilder::new("BareTemplate")
        .param("BareTemplate", "x", Type::dimensionless_scalar(), None)
        .param("BareTemplate", "y", Type::TypeParam("T".to_string()), None)
        .build();

    let map = seed_template_literal_params(&tmpl);
    assert!(
        map.is_empty(),
        "expected empty ValueMap when the template has no literal-default cells"
    );
}

/// `seed_template_literal_params` keys entries under the template's OWN
/// entity name, NOT under any external `param_member` string.
///
/// This is the inverse of `seed_candidate_value_map`, which re-keys under
/// a `param_member` prefix. The new helper must use `cell.id` directly.
#[test]
fn seed_keys_entries_under_own_entity_name_not_a_param_member() {
    let tmpl = TopologyTemplateBuilder::new("MySeal")
        .param(
            "MySeal",
            "thickness",
            Type::length(),
            Some(CompiledExpr::literal(mm(3.0), Type::length())),
        )
        .build();

    let map = seed_template_literal_params(&tmpl);

    // Key must use "MySeal" as the entity half (the cell's own id).
    let correct_key = ValueCellId::new("MySeal", "thickness");
    // A candidate-style re-key (as seed_candidate_value_map would produce)
    // must NOT appear.
    let wrong_key = ValueCellId::new("some_param_member", "thickness");

    assert!(
        map.get(&correct_key).is_some(),
        "key must use the template entity name 'MySeal' (the cell's own entity)"
    );
    assert!(
        map.get(&wrong_key).is_none(),
        "key must NOT use a param_member prefix — own-cell-id keying only"
    );
}
