//! Smoke tests for the `try_eval_ad_hoc_selector` function (task 3463).
//!
//! Tests that `try_eval_ad_hoc_selector` correctly:
//! 1. Resolves `@face("top")` against a Cylinder body (seeded table) to
//!    `Some(Value::Frame { .. })` — the happy path.
//! 2. Returns `Some(Value::Undef)` with a `TopologyAttributeStale` Warning for
//!    an unresolvable label ("nonexistent") — the diagnostic degradation path.
//! 3. Returns `None` for expressions that are not `AdHocSelector` nodes — the
//!    "not applicable" early-return.
//!
//! All three tests are RED on HEAD because `try_eval_ad_hoc_selector` does not
//! exist yet; they turn GREEN when step-6 implements the function and re-exports
//! it from `reify_eval::lib`.
//!
//! Test setup pattern:
//!   - Deterministic handle ids (no OCCT runtime required).
//!   - `TopologyAttributeTable` seeded by direct `table.record(...)` calls
//!     (mirrors the `test/topology_attribute_resolver_e2e.rs` pattern but
//!     without the OCCT kernel).
//!   - `MockGeometryKernel` configured with `with_extracted_faces`,
//!     `with_centroid_result`, and `with_face_normal_result`.
//!
//! Naming convention: every test name begins with the function it exercises
//! (`try_eval_ad_hoc_selector_*`) so `cargo test ad_hoc_selector` captures them all.

use std::collections::HashMap;

use reify_eval::try_eval_ad_hoc_selector;
use reify_test_support::MockGeometryKernel;
use reify_types::{
    CapKind, CompiledExpr, DiagnosticCode, FeatureId, GeometryHandleId, Role, SelectorKind,
    Severity, TopologyAttribute, TopologyAttributeTable, Type, Value,
};

// ─────────────────────────────────────────────────────────────────────────────
// Deterministic handle constants
//
// Convention: parent solid is id=1; sub-shape faces are id=10/11/12.
// Mirrors the `selector_vocabulary_v2_mock.rs` discipline of naming handles
// at the top of the file so the mapping is trivial to trace in test failures.
// ─────────────────────────────────────────────────────────────────────────────

/// The parent cylinder solid handle.
const BODY_HANDLE: GeometryHandleId = GeometryHandleId(1);
/// Top cap face handle (Role::Cap(CapKind::Top)).
const TOP_FACE: GeometryHandleId = GeometryHandleId(10);
/// Bottom cap face handle (Role::Cap(CapKind::Bottom)).
const BOTTOM_FACE: GeometryHandleId = GeometryHandleId(11);
/// Side face handle (Role::Side).
const SIDE_FACE: GeometryHandleId = GeometryHandleId(12);

// ─────────────────────────────────────────────────────────────────────────────
// Shared fixture helpers
// ─────────────────────────────────────────────────────────────────────────────

/// The `FeatureId` used for all seeded attributes in this test file.
fn cylinder_feature_id() -> FeatureId {
    FeatureId::new("Body#realization[0]")
}

/// Build a `TopologyAttributeTable` pre-seeded with Cylinder face attributes
/// (Top cap, Bottom cap, Side), mirroring the production
/// `seed_primitive_attributes(&..., &GeometryOp::Cylinder { .. })` outcome
/// but without needing the OCCT runtime.
///
/// The seeding shape pins the `cap_kind_translation` contract:
/// - `@face("top")`    → `Role::Cap(CapKind::Top)` → resolves to `TOP_FACE`
/// - `@face("bottom")` → `Role::Cap(CapKind::Bottom)` → resolves to `BOTTOM_FACE`
fn seeded_cylinder_table() -> TopologyAttributeTable {
    let feature_id = cylinder_feature_id();
    let mut table = TopologyAttributeTable::default();
    table.record(
        TOP_FACE,
        TopologyAttribute {
            feature_id: feature_id.clone(),
            role: Role::Cap(CapKind::Top),
            local_index: 0,
            user_label: None,
            mod_history: Vec::new(),
        },
    );
    table.record(
        BOTTOM_FACE,
        TopologyAttribute {
            feature_id: feature_id.clone(),
            role: Role::Cap(CapKind::Bottom),
            local_index: 0,
            user_label: None,
            mod_history: Vec::new(),
        },
    );
    table.record(
        SIDE_FACE,
        TopologyAttribute {
            feature_id,
            role: Role::Side,
            local_index: 0,
            user_label: None,
            mod_history: Vec::new(),
        },
    );
    table
}

/// Build a `MockGeometryKernel` configured so:
///   - `extract_faces(BODY_HANDLE)` → `[TOP_FACE, BOTTOM_FACE, SIDE_FACE]`
///   - `Centroid(TOP_FACE)` → JSON Point3 `{"x":0.0,"y":0.0,"z":0.01}`
///     (the centroid of a 20mm-high cylinder's top cap at +Z = 10mm = 0.01 m)
///   - `FaceNormal(TOP_FACE)` → JSON unit vector `{"x":0.0,"y":0.0,"z":1.0}`
///     (+Z normal, as expected for the top cap of an axis-aligned cylinder)
///
/// Only the `TOP_FACE` centroid/normal are configured because the happy-path
/// test (`@face("top")`) resolves to `TOP_FACE`; the unresolved-label test
/// never reaches the kernel queries.
fn configured_kernel() -> MockGeometryKernel {
    // JSON-Point3 format used by the OCCT kernel wire protocol and parsed by
    // `crate::topology_selectors::parse_xyz_value` on the eval side.
    let centroid_json = Value::String(r#"{"x":0.0,"y":0.0,"z":0.01}"#.to_string());
    let normal_json = Value::String(r#"{"x":0.0,"y":0.0,"z":1.0}"#.to_string());

    MockGeometryKernel::new()
        .with_extracted_faces(BODY_HANDLE, vec![TOP_FACE, BOTTOM_FACE, SIDE_FACE])
        .with_centroid_result(TOP_FACE, centroid_json)
        .with_face_normal_result(TOP_FACE, normal_json)
}

/// Build a named-steps map mapping the string `"body"` to `BODY_HANDLE`.
///
/// This is what the engine populates for `let body = cylinder(...)` before
/// calling `post_process_ad_hoc_selectors`.
fn named_steps_with_body() -> HashMap<String, GeometryHandleId> {
    let mut m = HashMap::new();
    m.insert("body".to_string(), BODY_HANDLE);
    m
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 1: @face("top") resolves to Some(Value::Frame { .. })
// ─────────────────────────────────────────────────────────────────────────────

/// `try_eval_ad_hoc_selector` must return `Some(Value::Frame { .. })` for an
/// `@face("top")` expression against a Cylinder body whose top-cap face is
/// seeded in the `TopologyAttributeTable` with `Role::Cap(CapKind::Top)`.
///
/// This is the nominal happy-path test that validates the full evaluation chain:
///   1. Extract base name → look up `"body"` in `named_steps` → `BODY_HANDLE`
///   2. Extract label → `"top"`
///   3. `kernel.extract_faces(BODY_HANDLE)` → `[TOP_FACE, BOTTOM_FACE, SIDE_FACE]`
///   4. `resolve_unique_by_attribute(table, candidates,
///       AttributeQuery { user_label: Some("top"), role_and_index: cap_kind_translation("top"),
///       feature_id: None }, ...)` → `Resolved(TOP_FACE)`
///   5. `kernel.query(Centroid(TOP_FACE))` + `kernel.query(FaceNormal(TOP_FACE))`
///      → `Value::Frame { origin: Point([0m, 0m, 0.01m]), basis: ... }`
///
/// RED on HEAD because `try_eval_ad_hoc_selector` does not exist yet.
#[test]
fn try_eval_ad_hoc_selector_face_top_resolves_to_frame_via_attribute_table() {
    // `@face("top")` expression: base="body", selector_kind=Face, args=["top"]
    let expr = CompiledExpr::ad_hoc_selector(
        CompiledExpr::literal(Value::String("body".to_string()), Type::String),
        SelectorKind::Face,
        vec![CompiledExpr::literal(
            Value::String("top".to_string()),
            Type::String,
        )],
    );

    let named_steps = named_steps_with_body();
    let table = seeded_cylinder_table();
    let mut kernel = configured_kernel();
    let mut diagnostics = Vec::new();

    let result = try_eval_ad_hoc_selector(&expr, &named_steps, &mut kernel, &table, &mut diagnostics);

    assert!(
        matches!(result, Some(Value::Frame { .. })),
        "@face(\"top\") against a seeded cylinder should resolve to Some(Value::Frame {{ .. }}), \
         got {:?}",
        result
    );
    assert!(
        diagnostics.is_empty(),
        "no diagnostic expected on a clean resolution; got {:?}",
        diagnostics
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 2: @face("nonexistent") returns Some(Value::Undef) + TopologyAttributeStale
// ─────────────────────────────────────────────────────────────────────────────

/// `try_eval_ad_hoc_selector` must return `Some(Value::Undef)` and emit a
/// `Severity::Warning` with `DiagnosticCode::TopologyAttributeStale` when the
/// attribute label has no match in the table.
///
/// Design decision: on `Unresolved`, the resolver pre-emits the Warning and the
/// dispatcher patches the cell with `Some(Undef)` (not `None`), so callers can
/// distinguish "dispatch ran, produced Undef" from "dispatch was not applicable
/// (returned None)". The Warning-with-stale-code is the structured user-facing
/// signal.
///
/// RED on HEAD because `try_eval_ad_hoc_selector` does not exist yet.
#[test]
fn try_eval_ad_hoc_selector_face_unresolved_name_returns_undef_with_warning() {
    // `@face("nonexistent")` expression — label does not match any seeded attribute.
    let expr = CompiledExpr::ad_hoc_selector(
        CompiledExpr::literal(Value::String("body".to_string()), Type::String),
        SelectorKind::Face,
        vec![CompiledExpr::literal(
            Value::String("nonexistent".to_string()),
            Type::String,
        )],
    );

    let named_steps = named_steps_with_body();
    let table = seeded_cylinder_table();
    // The unresolved path doesn't reach centroid/normal queries, so a minimal
    // kernel fixture is sufficient: just `extract_faces` so the resolver can
    // enumerate candidates.
    let mut kernel = MockGeometryKernel::new()
        .with_extracted_faces(BODY_HANDLE, vec![TOP_FACE, BOTTOM_FACE, SIDE_FACE]);
    let mut diagnostics = Vec::new();

    let result = try_eval_ad_hoc_selector(&expr, &named_steps, &mut kernel, &table, &mut diagnostics);

    assert!(
        matches!(result, Some(Value::Undef)),
        "@face(\"nonexistent\") should return Some(Value::Undef) on Unresolved, got {:?}",
        result
    );
    assert!(
        diagnostics.iter().any(|d| {
            d.code == Some(DiagnosticCode::TopologyAttributeStale)
                && d.severity == Severity::Warning
        }),
        "expected a Severity::Warning with code TopologyAttributeStale; got {:?}",
        diagnostics
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 3: Non-AdHocSelector expression returns None
// ─────────────────────────────────────────────────────────────────────────────

/// `try_eval_ad_hoc_selector` must return `None` for any expression that is
/// not a `CompiledExprKind::AdHocSelector` node — this is the "not applicable"
/// early-return that lets callers fall through to other dispatch arms.
///
/// Two sub-cases are checked:
///   (a) A bare `Literal(Bool(true))` — exercises the `_ => return None` arm.
///   (b) An empty `Literal(String("body"))` — also `Literal`, not AdHocSelector.
///
/// RED on HEAD because `try_eval_ad_hoc_selector` does not exist yet.
#[test]
fn try_eval_ad_hoc_selector_non_ad_hoc_expr_returns_none() {
    let named_steps = named_steps_with_body();
    let table = seeded_cylinder_table();
    let mut kernel = configured_kernel();

    // (a) Bool literal — not an AdHocSelector.
    let bool_expr = CompiledExpr::literal(Value::Bool(true), Type::Bool);
    let mut diagnostics = Vec::new();
    let result_a =
        try_eval_ad_hoc_selector(&bool_expr, &named_steps, &mut kernel, &table, &mut diagnostics);
    assert!(
        result_a.is_none(),
        "a Bool literal should return None (not applicable), got {:?}",
        result_a
    );
    assert!(
        diagnostics.is_empty(),
        "None-path should emit no diagnostics; got {:?}",
        diagnostics
    );

    // (b) String literal — also Literal, not AdHocSelector.
    let str_expr = CompiledExpr::literal(Value::String("body".to_string()), Type::String);
    let mut diagnostics_b = Vec::new();
    let result_b = try_eval_ad_hoc_selector(
        &str_expr,
        &named_steps,
        &mut kernel,
        &table,
        &mut diagnostics_b,
    );
    assert!(
        result_b.is_none(),
        "a String literal should return None (not applicable), got {:?}",
        result_b
    );
    assert!(
        diagnostics_b.is_empty(),
        "None-path should emit no diagnostics; got {:?}",
        diagnostics_b
    );
}
