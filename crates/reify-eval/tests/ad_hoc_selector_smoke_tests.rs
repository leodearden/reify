//! Smoke tests for the `try_eval_ad_hoc_selector` function AND the
//! engine-level `post_process_ad_hoc_selectors` wiring (task 3463).
//!
//! Layer-1 (function-level) tests — steps 5/6:
//! 1. Resolves `@face("top")` against a Cylinder body (manually-seeded table)
//!    to `Some(Value::Frame { .. })` — the happy path.
//! 2. Returns `Some(Value::Undef)` with a `TopologyAttributeStale` Warning for
//!    an unresolvable label ("nonexistent") — the diagnostic degradation path.
//! 3. Returns `None` for expressions that are not `AdHocSelector` nodes.
//!
//! Layer-2 (engine-level) tests — steps 7/8:
//! 4. `engine_build_post_processes_ad_hoc_face_selector_to_frame` — full
//!    `engine.build()` pipeline patches `body @ face("top")` to `Value::Frame`.
//! 5. `engine_build_emits_warning_on_unresolved_face_name` — full pipeline
//!    leaves `body @ face("nonexistent")` at `Value::Undef` with a Warning.
//!
//! Tests 4 and 5 are RED until step-8 wires `post_process_ad_hoc_selectors`
//! into `engine_build.rs`.
//!
//! Naming convention: function-level tests begin with `try_eval_ad_hoc_selector_*`;
//! engine-level tests begin with `engine_build_*`.

use std::collections::HashMap;

use reify_constraints::SimpleConstraintChecker;
use reify_core::{DiagnosticCode, Severity, SourceSpan, Type, ValueCellId};
use reify_eval::try_eval_ad_hoc_selector;
use reify_ir::{
    CapKind, CompiledExpr, ExportFormat, FeatureId, GeometryHandleId, KernelHandle, KernelId,
    QueryError, Role, SelectorKind, TopologyAttribute, TopologyAttributeTable, Value,
};
use reify_test_support::{
    MockGeometryKernel, compile_source, errors_only, parse_and_compile_with_stdlib,
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
/// An edge handle for the edge-selector smoke test.
const EDGE_HANDLE: GeometryHandleId = GeometryHandleId(20);

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

/// Wrap a bare `GeometryHandleId` in a `KernelHandle` (Occt kernel).
///
/// Used by test fixtures that migrate from `HashMap<String, GeometryHandleId>`
/// to `HashMap<String, KernelHandle>`.
fn kh(id: GeometryHandleId) -> KernelHandle {
    KernelHandle {
        kernel: KernelId::Occt,
        id,
    }
}

/// Build a named-steps map mapping the string `"body"` to `BODY_HANDLE`.
///
/// This is what the engine populates for `let body = cylinder(...)` before
/// calling `post_process_ad_hoc_selectors`.
fn named_steps_with_body() -> HashMap<String, KernelHandle> {
    let mut m = HashMap::new();
    m.insert("body".to_string(), kh(BODY_HANDLE));
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

    let result = try_eval_ad_hoc_selector(
        &expr,
        &named_steps,
        &mut kernel,
        &table,
        SourceSpan::empty(0),
        &mut diagnostics,
    );

    // ── Verify exact Frame contents ──────────────────────────────────────────
    // Kernel returns centroid {"x":0.0,"y":0.0,"z":0.01} → origin at (0m, 0m, 0.01m)
    // and normal {"x":0.0,"y":0.0,"z":1.0} → +Z → +Z = identity quaternion
    // (quaternion_from_z_to_axis(0,0,1): w_unnorm=2, len=2, w=1 — exact IEEE 754).
    let Some(Value::Frame {
        ref origin,
        ref basis,
    }) = result
    else {
        panic!(
            "@face(\"top\") against a seeded cylinder should resolve to Some(Value::Frame {{ .. }}), \
             got {:?}",
            result
        );
    };
    assert_eq!(
        **origin,
        Value::Point(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(0.01),
        ]),
        "@face(\"top\") origin should be (0m, 0m, 0.01m)"
    );
    assert_eq!(
        **basis,
        Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0
        },
        "@face(\"top\") basis should be identity (normal +Z → +Z is zero rotation)"
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

    let result = try_eval_ad_hoc_selector(
        &expr,
        &named_steps,
        &mut kernel,
        &table,
        SourceSpan::empty(0),
        &mut diagnostics,
    );

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
    let result_a = try_eval_ad_hoc_selector(
        &bool_expr,
        &named_steps,
        &mut kernel,
        &table,
        SourceSpan::empty(0),
        &mut diagnostics,
    );
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
        SourceSpan::empty(0),
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

// ─────────────────────────────────────────────────────────────────────────────
// Engine-level tests (step-7 / step-8)
//
// These tests exercise the FULL `engine.build()` pipeline — parse → compile →
// execute kernel ops → seed topology_attribute_table → eval_expr → post-process.
// They are RED on HEAD because `Engine::post_process_ad_hoc_selectors` does not
// exist yet; they turn GREEN when step-8 wires it into `engine_build.rs`.
//
// Handle numbering discipline:
//   `MockGeometryKernel.next_id` starts at 1. The cylinder is the first
//   `execute()` call → BODY_HANDLE = GeometryHandleId(1) (same constant as
//   above). The sub-shape face handles (10/11/12) are configured manually via
//   `with_extracted_faces` and are never allocated by `execute()`, so there
//   is no conflict.
//
// Kernel configuration for the engine-level tests:
//   The seeder (`seed_primitive_attributes_for_handle`) calls:
//     - `kernel.extract_faces(BODY_HANDLE)` → [TOP_FACE, BOTTOM_FACE, SIDE_FACE]
//     - `kernel.extract_edges(BODY_HANDLE)` → [] (empty; seeder ok with no edges)
//     - `kernel.query(FaceNormal(f))` for each face → z-component classifies role
//   Then `post_process_ad_hoc_selectors` (step-8) calls:
//     - `kernel.extract_faces(BODY_HANDLE)` again (safe: MockKernel caches it)
//     - `kernel.query(Centroid(TOP_FACE))` → Frame origin
//     - `kernel.query(FaceNormal(TOP_FACE))` → Frame basis
// ─────────────────────────────────────────────────────────────────────────────

/// Build a `MockGeometryKernel` configured for the full engine-level pipeline:
/// seeder classification AND post-process Frame construction.
///
/// Differences from `configured_kernel()` (used by function-level tests):
///   - Adds `with_extracted_edges(BODY_HANDLE, vec![])` so the seeder
///     does not fail at `extract_edges`.
///   - Adds `with_face_normal_result` for BOTTOM_FACE and SIDE_FACE so the
///     seeder can classify all three faces of the cylinder.
///   - The TOP_FACE centroid and normal are already included (same as the
///     function-level fixture) for the post-process Frame construction.
fn configured_engine_kernel() -> MockGeometryKernel {
    let centroid_json = Value::String(r#"{"x":0.0,"y":0.0,"z":0.01}"#.to_string());
    // +Z normal → Role::Cap(CapKind::Top)
    let top_normal_json = Value::String(r#"{"x":0.0,"y":0.0,"z":1.0}"#.to_string());
    // −Z normal → Role::Cap(CapKind::Bottom)
    let bottom_normal_json = Value::String(r#"{"x":0.0,"y":0.0,"z":-1.0}"#.to_string());
    // Horizontal normal (z≈0) → Role::Side
    let side_normal_json = Value::String(r#"{"x":1.0,"y":0.0,"z":0.0}"#.to_string());

    MockGeometryKernel::new()
        // Sub-shape extraction — used by both the seeder and the post-process.
        .with_extracted_faces(BODY_HANDLE, vec![TOP_FACE, BOTTOM_FACE, SIDE_FACE])
        .with_extracted_edges(BODY_HANDLE, vec![]) // empty is fine for the seeder
        // Per-face normals consumed by the seeder for role classification.
        .with_face_normal_result(TOP_FACE, top_normal_json.clone())
        .with_face_normal_result(BOTTOM_FACE, bottom_normal_json)
        .with_face_normal_result(SIDE_FACE, side_normal_json)
        // Centroid of TOP_FACE for Frame origin in post_process_ad_hoc_selectors.
        .with_centroid_result(TOP_FACE, centroid_json)
    // The TOP_FACE FaceNormal is already registered above (top_normal_json).
    // MockGeometryKernel reuses the same entry for both the seeder query
    // and the post-process basis-derivation query.
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 4: engine.build() patches @face("top") → Value::Frame
// ─────────────────────────────────────────────────────────────────────────────

/// `Engine::build()` must patch the `top_frame` cell from `Value::Undef`
/// (produced by `eval_expr` for `SelectorKind::Face`) to `Value::Frame { .. }`
/// via `post_process_ad_hoc_selectors` + `try_eval_ad_hoc_selector`.
///
/// End-to-end pipeline:
///   1. `cylinder(10mm, 20mm)` → kernel execute → `BODY_HANDLE`
///   2. Seeder: seeds `TOP_FACE → Role::Cap(Top)`, etc. into the attribute table
///   3. `eval_expr` for `top_frame = body @ face("top")` → `Value::Undef` (Face arm)
///   4. `post_process_ad_hoc_selectors` (step-8) →
///      `try_eval_ad_hoc_selector` →
///      `resolve_unique_by_attribute` → `Resolved(TOP_FACE)` →
///      `Centroid(TOP_FACE)` + `FaceNormal(TOP_FACE)` →
///      `Value::Frame { origin: Point([0m, 0m, 0.01m]), basis: .. }`
///   5. Cell is patched from `Undef` to `Frame`.
///
/// RED until step-8 wires `post_process_ad_hoc_selectors` into `engine_build.rs`.
#[test]
fn engine_build_post_processes_ad_hoc_face_selector_to_frame() {
    let source = r#"structure AdHocCylinder {
    let body = cylinder(10mm, 20mm)
    let top_frame = body @ face("top")
}"#;

    let compiled = compile_source(source);
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "expected no compile-time Error diagnostics; got: {:#?}",
        compile_errors
    );

    let checker = SimpleConstraintChecker;
    let kernel = configured_engine_kernel();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&compiled, ExportFormat::Step);

    let top_frame_id = ValueCellId::new("AdHocCylinder", "top_frame");
    let top_frame_val = result
        .values
        .get(&top_frame_id)
        .unwrap_or_else(|| panic!("AdHocCylinder.top_frame not found in build result values"));

    // ── Verify exact Frame contents ──────────────────────────────────────────
    // Same kernel config as test 1: centroid {"x":0.0,"y":0.0,"z":0.01},
    // normal {"x":0.0,"y":0.0,"z":1.0} → identity basis.
    // `top_frame_val` is `&Value`; match ergonomics auto-borrows, so `ref`
    // must be omitted (origin: &Box<Value>, basis: &Box<Value>).
    let Value::Frame { origin, basis } = top_frame_val else {
        panic!(
            "AdHocCylinder.top_frame should resolve to Value::Frame {{ .. }} after \
             post_process_ad_hoc_selectors wires @face(\"top\"); got {:?}",
            top_frame_val
        );
    };
    assert_eq!(
        **origin,
        Value::Point(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(0.01),
        ]),
        "engine-level @face(\"top\") origin should be (0m, 0m, 0.01m)"
    );
    assert_eq!(
        **basis,
        Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0
        },
        "engine-level @face(\"top\") basis should be identity"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 5: engine.build() leaves @face("nonexistent") at Undef + Warning
// ─────────────────────────────────────────────────────────────────────────────

/// `Engine::build()` must leave a `body @ face("nonexistent")` cell at
/// `Value::Undef` AND emit a `Severity::Warning` with
/// `DiagnosticCode::TopologyAttributeStale` when the label does not match
/// any entry in the attribute table.
///
/// This mirrors the function-level test
/// `try_eval_ad_hoc_selector_face_unresolved_name_returns_undef_with_warning`
/// but exercises the full engine pipeline including seeder and wiring.
///
/// RED until step-8 wires `post_process_ad_hoc_selectors` into `engine_build.rs`.
#[test]
fn engine_build_emits_warning_on_unresolved_face_name() {
    let source = r#"structure AdHocCylinderUnresolved {
    let body = cylinder(10mm, 20mm)
    let mystery_frame = body @ face("nonexistent")
}"#;

    let compiled = compile_source(source);
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "expected no compile-time Error diagnostics; got: {:#?}",
        compile_errors
    );

    let checker = SimpleConstraintChecker;
    // The seeder still needs FaceNormal results to populate the table
    // (so the resolver has candidates to enumerate). The centroid/normal
    // for the post-process Frame construction are never reached because the
    // label resolves to Unresolved.
    let kernel = configured_engine_kernel();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&compiled, ExportFormat::Step);

    let mystery_id = ValueCellId::new("AdHocCylinderUnresolved", "mystery_frame");
    let mystery_val = result.values.get(&mystery_id).unwrap_or_else(|| {
        panic!("AdHocCylinderUnresolved.mystery_frame not found in build result values")
    });

    assert!(
        matches!(mystery_val, Value::Undef),
        "@face(\"nonexistent\") should leave the cell at Value::Undef on Unresolved; \
         got {:?}",
        mystery_val
    );

    assert!(
        result.diagnostics.iter().any(|d| {
            d.code == Some(DiagnosticCode::TopologyAttributeStale)
                && d.severity == Severity::Warning
        }),
        "expected a Severity::Warning with code TopologyAttributeStale on unresolved \
         @face(\"nonexistent\"); got: {:?}",
        result.diagnostics
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 6: .ri example file compiles and evaluates @face("top") to Frame
// ─────────────────────────────────────────────────────────────────────────────

/// Path to the user-observable `.ri` witness for task 3463.
///
/// RED until step-10 creates `examples/ad_hoc_face_selector.ri`.
const AD_HOC_FACE_SELECTOR_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/ad_hoc_face_selector.ri"
);

/// `examples/ad_hoc_face_selector.ri` must compile with no error-severity
/// diagnostics AND the `top_frame` cell in `TopCapBracket` must evaluate
/// to `Value::Frame { .. }` through the full `engine.build()` pipeline.
///
/// Pipeline under test (mirrors `engine_build_post_processes_ad_hoc_face_selector_to_frame`
/// but reads a real `.ri` file compiled with stdlib, not an inline source string):
///   1. `std::fs::read_to_string(AD_HOC_FACE_SELECTOR_PATH)` — reads the file.
///   2. `parse_and_compile_with_stdlib(&source)` — compiles with stdlib so that
///      `cylinder`, `transform3`, `orient_identity`, `vec3` etc. are resolved.
///   3. `errors_only(&compiled).is_empty()` — no compile-time Error diagnostics.
///   4. `engine.build(&compiled, ExportFormat::Step)` with `configured_engine_kernel()` —
///      seeder populates `TopologyAttributeTable`, then
///      `post_process_ad_hoc_selectors` patches `top_frame` from `Value::Undef`
///      (Face arm from `eval_expr`) to `Value::Frame { .. }`.
///   5. Asserts `TopCapBracket.top_frame` is `Value::Frame { .. }`.
///
/// Mirrors the smoke-test pattern from
/// `tests/topology_selector_smoke_tests.rs::block_inertia_compiles_with_stdlib_no_errors`
/// (compile-side assertion) plus the engine-level assertion from
/// `engine_build_post_processes_ad_hoc_face_selector_to_frame` (value assertion).
///
/// RED until step-10 creates `examples/ad_hoc_face_selector.ri`.
#[test]
fn face_selector_ri_example_compiles_and_evaluates_to_frame() {
    let source = std::fs::read_to_string(AD_HOC_FACE_SELECTOR_PATH)
        .expect("examples/ad_hoc_face_selector.ri should exist (created by step-10)");

    // Compile with stdlib so cylinder, transform3, orient_identity, vec3 resolve.
    let compiled = parse_and_compile_with_stdlib(&source);

    // `parse_and_compile_with_stdlib` already panics on compile errors; this
    // explicit assertion surfaces a clearer failure message in case of a
    // Warning-promoted-to-Error regression in the future.
    let compile_errors = errors_only(&compiled);
    assert!(
        compile_errors.is_empty(),
        "examples/ad_hoc_face_selector.ri should compile with no error-severity diagnostics; \
         got:\n{:#?}",
        compile_errors
    );

    // Build with the same kernel fixture used by the engine-level tests —
    // `configured_engine_kernel()` configures extract_faces, FaceNormal for role
    // classification (seeder), and Centroid + FaceNormal for TOP_FACE post-process.
    let checker = SimpleConstraintChecker;
    let kernel = configured_engine_kernel();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&compiled, ExportFormat::Step);

    // Assert the primary user-observable witness: @face("top") → Value::Frame.
    let top_frame_id = ValueCellId::new("TopCapBracket", "top_frame");
    let top_frame_val = result
        .values
        .get(&top_frame_id)
        .unwrap_or_else(|| panic!("TopCapBracket.top_frame not found in build result values"));

    // ── Verify exact Frame contents ──────────────────────────────────────────
    // Same configured_engine_kernel() fixture: centroid {"x":0.0,"y":0.0,"z":0.01},
    // normal {"x":0.0,"y":0.0,"z":1.0} → +Z → +Z = identity quaternion.
    // `top_frame_val` is `&Value`; match ergonomics auto-borrows, so `ref`
    // must be omitted (origin: &Box<Value>, basis: &Box<Value>).
    let Value::Frame { origin, basis } = top_frame_val else {
        panic!(
            "TopCapBracket.top_frame should resolve to Value::Frame {{ .. }} via \
             post_process_ad_hoc_selectors wiring @face(\"top\"); got {:?}",
            top_frame_val
        );
    };
    assert_eq!(
        **origin,
        Value::Point(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(0.01),
        ]),
        ".ri example @face(\"top\") origin should be (0m, 0m, 0.01m)"
    );
    assert_eq!(
        **basis,
        Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0
        },
        ".ri example @face(\"top\") basis should be identity"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 7: @edge("top_edge") resolves via user_label to Some(Value::Frame)
// (Suggestion 2 — edge convention smoke test)
// ─────────────────────────────────────────────────────────────────────────────

/// Build a `TopologyAttributeTable` with a single edge entry whose
/// `user_label = Some("top_edge")`.  The resolver will match via user_label
/// since there is no edge-role entry in `cap_kind_translation`.
fn seeded_edge_table() -> TopologyAttributeTable {
    let feature_id = cylinder_feature_id();
    let mut table = TopologyAttributeTable::default();
    table.record(
        EDGE_HANDLE,
        TopologyAttribute {
            feature_id,
            role: Role::NewEdge,
            local_index: 0,
            user_label: Some("top_edge".to_string()),
            mod_history: Vec::new(),
        },
    );
    table
}

/// Build a `MockGeometryKernel` for the edge smoke test:
///   - `extract_edges(BODY_HANDLE)` → `[EDGE_HANDLE]`
///   - `Centroid(EDGE_HANDLE)` → centroid at `{"x":0.0,"y":0.0,"z":0.005}`
///     (an arbitrary point 5mm up the cylinder axis)
///   - `EdgeTangent(EDGE_HANDLE)` → `{"x":0.0,"y":0.0,"z":1.0}` (+Z direction)
///
/// The +Z tangent is chosen to produce an identity basis quaternion, making
/// the assertion exact.  See `construct_frame_from_kernel` doc comment for
/// the convention: the edge tangent maps to the frame's **+Z** axis.
fn configured_edge_kernel() -> MockGeometryKernel {
    let centroid_json = Value::String(r#"{"x":0.0,"y":0.0,"z":0.005}"#.to_string());
    let tangent_json = Value::String(r#"{"x":0.0,"y":0.0,"z":1.0}"#.to_string());
    MockGeometryKernel::new()
        .with_extracted_edges(BODY_HANDLE, vec![EDGE_HANDLE])
        .with_centroid_result(EDGE_HANDLE, centroid_json)
        .with_edge_tangent_result(EDGE_HANDLE, tangent_json)
}

/// `try_eval_ad_hoc_selector` must resolve `@edge("top_edge")` against an
/// edge whose `user_label = "top_edge"` to `Some(Value::Frame { .. })`.
///
/// This test pins the **edge frame convention**: the edge tangent maps to the
/// frame's **+Z** axis (`construct_frame_from_kernel` doc comment).  With a
/// +Z tangent the expected basis is the identity quaternion — exact IEEE 754.
///
/// The test also verifies that `@edge` expressions are dispatched by the
/// `SelectorKind::Edge` arm (confirming `try_eval_ad_hoc_selector` handles
/// both Face and Edge selectors).
#[test]
fn try_eval_ad_hoc_selector_edge_resolves_to_frame_via_user_label() {
    // `@edge("top_edge")` expression: base="body", kind=Edge, args=["top_edge"]
    let expr = CompiledExpr::ad_hoc_selector(
        CompiledExpr::literal(Value::String("body".to_string()), Type::String),
        SelectorKind::Edge,
        vec![CompiledExpr::literal(
            Value::String("top_edge".to_string()),
            Type::String,
        )],
    );

    let named_steps = named_steps_with_body();
    let table = seeded_edge_table();
    let mut kernel = configured_edge_kernel();
    let mut diagnostics = Vec::new();

    let result = try_eval_ad_hoc_selector(
        &expr,
        &named_steps,
        &mut kernel,
        &table,
        SourceSpan::empty(0),
        &mut diagnostics,
    );

    // ── Verify exact Frame contents ──────────────────────────────────────────
    // Edge tangent {"x":0.0,"y":0.0,"z":1.0} → +Z → +Z = identity quaternion.
    // Edge centroid {"x":0.0,"y":0.0,"z":0.005} → origin at (0m, 0m, 0.005m).
    // Convention: tangent aligns to frame +Z (documented in construct_frame_from_kernel).
    let Some(Value::Frame {
        ref origin,
        ref basis,
    }) = result
    else {
        panic!(
            "@edge(\"top_edge\") against a user-labelled edge should resolve to \
             Some(Value::Frame {{ .. }}), got {:?}",
            result
        );
    };
    assert_eq!(
        **origin,
        Value::Point(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(0.005),
        ]),
        "@edge(\"top_edge\") origin should be (0m, 0m, 0.005m)"
    );
    assert_eq!(
        **basis,
        Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0
        },
        "@edge(\"top_edge\") basis should be identity (tangent +Z → +Z is zero rotation)"
    );
    assert!(
        diagnostics.is_empty(),
        "no diagnostic expected on a clean resolution; got {:?}",
        diagnostics
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 8: @face("side") resolves via cap_kind_translation to Some(Value::Frame)
// (Suggestion 4 — "side" added to cap_kind_translation)
// ─────────────────────────────────────────────────────────────────────────────

/// Build a `MockGeometryKernel` for the side-face smoke test:
///   - `extract_faces(BODY_HANDLE)` → `[TOP_FACE, BOTTOM_FACE, SIDE_FACE]`
///   - `Centroid(SIDE_FACE)` → `{"x":0.0,"y":0.0,"z":0.0}` (world origin)
///   - `FaceNormal(SIDE_FACE)` → `{"x":0.0,"y":0.0,"z":1.0}` (+Z, mock-only)
///
/// Note: the +Z normal is geometrically incorrect for a Cylinder's side face
/// (which normally has a radial normal in XY).  The mock value is chosen for
/// exact-assertion simplicity: `quaternion_from_z_to_axis(0,0,1) = identity`.
fn configured_side_kernel() -> MockGeometryKernel {
    let centroid_json = Value::String(r#"{"x":0.0,"y":0.0,"z":0.0}"#.to_string());
    let normal_json = Value::String(r#"{"x":0.0,"y":0.0,"z":1.0}"#.to_string());
    MockGeometryKernel::new()
        .with_extracted_faces(BODY_HANDLE, vec![TOP_FACE, BOTTOM_FACE, SIDE_FACE])
        .with_centroid_result(SIDE_FACE, centroid_json)
        .with_face_normal_result(SIDE_FACE, normal_json)
}

/// `try_eval_ad_hoc_selector` must resolve `@face("side")` against a Cylinder
/// body (whose side face is seeded with `Role::Side`) to `Some(Value::Frame { .. })`
/// after `"side"` is added to `cap_kind_translation`.
///
/// Resolution path: `cap_kind_translation("side")` → `Some((Role::Side, 0))` →
/// `role_and_index` match against the table entry `SIDE_FACE { role: Role::Side,
/// local_index: 0 }` → `Resolved(SIDE_FACE)` → Frame construction.
///
/// This test verifies that the `cap_kind_translation` vocabulary extension
/// (suggestion 4) closes the `@face("side")` → `TopologyAttributeStale`
/// regression for Cylinder users.
#[test]
fn try_eval_ad_hoc_selector_face_side_resolves_via_cap_kind_translation() {
    // `@face("side")` expression: base="body", kind=Face, args=["side"]
    let expr = CompiledExpr::ad_hoc_selector(
        CompiledExpr::literal(Value::String("body".to_string()), Type::String),
        SelectorKind::Face,
        vec![CompiledExpr::literal(
            Value::String("side".to_string()),
            Type::String,
        )],
    );

    let named_steps = named_steps_with_body();
    // seeded_cylinder_table() contains SIDE_FACE with Role::Side, local_index=0,
    // user_label=None — the resolver matches via role_and_index only.
    let table = seeded_cylinder_table();
    let mut kernel = configured_side_kernel();
    let mut diagnostics = Vec::new();

    let result = try_eval_ad_hoc_selector(
        &expr,
        &named_steps,
        &mut kernel,
        &table,
        SourceSpan::empty(0),
        &mut diagnostics,
    );

    // ── Verify exact Frame contents ──────────────────────────────────────────
    // centroid {"x":0.0,"y":0.0,"z":0.0} → origin at world origin.
    // normal {"x":0.0,"y":0.0,"z":1.0} → +Z → +Z = identity quaternion.
    let Some(Value::Frame {
        ref origin,
        ref basis,
    }) = result
    else {
        panic!(
            "@face(\"side\") against a Cylinder side face should resolve to \
             Some(Value::Frame {{ .. }}) after cap_kind_translation adds \"side\", \
             got {:?}",
            result
        );
    };
    assert_eq!(
        **origin,
        Value::Point(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(0.0),
        ]),
        "@face(\"side\") origin should be (0m, 0m, 0m)"
    );
    assert_eq!(
        **basis,
        Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0
        },
        "@face(\"side\") basis should be identity for mock +Z normal"
    );
    assert!(
        diagnostics.is_empty(),
        "no diagnostic expected on a clean resolution; got {:?}",
        diagnostics
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 9: extract_faces kernel error → Warning + Some(Undef)
// Characterisation test for the Err arm at geometry_ops.rs:2157-2161.
// ─────────────────────────────────────────────────────────────────────────────

/// `try_eval_ad_hoc_selector` must return `Some(Value::Undef)` and emit exactly
/// one `Severity::Warning` that surfaces the kernel error message when
/// `kernel.extract_faces` returns an error.
///
/// Asserts on the propagated kernel error text ("mock face extraction failure")
/// rather than the incidental function-name/verb wording, so a benign message
/// reword won't break this test.
///
/// Pins the Warning+Some(Undef) wiring at geometry_ops.rs:2157-2161.
/// Passes on HEAD — characterisation test for already-implemented behaviour.
#[test]
fn try_eval_ad_hoc_selector_face_kernel_error_returns_undef_with_warning() {
    // `@face("top")` with a kernel that errors on extract_faces.
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
    let mut kernel = MockGeometryKernel::new().with_extract_faces_error(
        BODY_HANDLE,
        QueryError::QueryFailed("mock face extraction failure".into()),
    );
    let mut diagnostics = Vec::new();

    let result = try_eval_ad_hoc_selector(
        &expr,
        &named_steps,
        &mut kernel,
        &table,
        SourceSpan::empty(0),
        &mut diagnostics,
    );

    assert!(
        matches!(result, Some(Value::Undef)),
        "extract_faces error should return Some(Value::Undef), got {:?}",
        result
    );

    let warnings: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .collect();
    assert_eq!(
        warnings.len(),
        1,
        "expected exactly one Warning diagnostic, got {} total diagnostics: {:#?}",
        diagnostics.len(),
        diagnostics
    );
    assert!(
        warnings[0].message.contains("mock face extraction failure"),
        "warning message should propagate the kernel error text; got {:?}",
        warnings[0].message
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 10: extract_edges kernel error → Warning + Some(Undef)
// Characterisation test for the Err arm at geometry_ops.rs:2166-2170.
// ─────────────────────────────────────────────────────────────────────────────

/// `try_eval_ad_hoc_selector` must return `Some(Value::Undef)` and emit exactly
/// one `Severity::Warning` that surfaces the kernel error message when
/// `kernel.extract_edges` returns an error.
///
/// Asserts on the propagated kernel error text ("mock edge extraction failure")
/// rather than the incidental function-name/verb wording, so a benign message
/// reword won't break this test.
///
/// Pins the Warning+Some(Undef) wiring at geometry_ops.rs:2166-2170.
/// Passes on HEAD — characterisation test for already-implemented behaviour.
#[test]
fn try_eval_ad_hoc_selector_edge_kernel_error_returns_undef_with_warning() {
    // `@edge("top_edge")` with a kernel that errors on extract_edges.
    let expr = CompiledExpr::ad_hoc_selector(
        CompiledExpr::literal(Value::String("body".to_string()), Type::String),
        SelectorKind::Edge,
        vec![CompiledExpr::literal(
            Value::String("top_edge".to_string()),
            Type::String,
        )],
    );

    let named_steps = named_steps_with_body();
    let table = seeded_edge_table();
    let mut kernel = MockGeometryKernel::new().with_extract_edges_error(
        BODY_HANDLE,
        QueryError::QueryFailed("mock edge extraction failure".into()),
    );
    let mut diagnostics = Vec::new();

    let result = try_eval_ad_hoc_selector(
        &expr,
        &named_steps,
        &mut kernel,
        &table,
        SourceSpan::empty(0),
        &mut diagnostics,
    );

    assert!(
        matches!(result, Some(Value::Undef)),
        "extract_edges error should return Some(Value::Undef), got {:?}",
        result
    );

    let warnings: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .collect();
    assert_eq!(
        warnings.len(),
        1,
        "expected exactly one Warning diagnostic, got {} total diagnostics: {:#?}",
        diagnostics.len(),
        diagnostics
    );
    assert!(
        warnings[0].message.contains("mock edge extraction failure"),
        "warning message should propagate the kernel error text; got {:?}",
        warnings[0].message
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 11: non-Literal args[0] → None early return, no diagnostics
// Characterisation test for the resolve_string_literal_arg(a)? arm at
// geometry_ops.rs:2142.
// ─────────────────────────────────────────────────────────────────────────────

/// `try_eval_ad_hoc_selector` must return `None` (not applicable) and emit no
/// diagnostics when `args[0]` is not a `Literal(String(..))` — e.g. a `ValueRef`.
///
/// The base remains `Literal(String("body"))` so the function passes step (3)
/// and reaches the `resolve_string_literal_arg(a)?` call at step (4).  The
/// `ValueRef` causes `resolve_string_literal_arg` to return `None`, triggering
/// the early return via `?`.
///
/// Pins the contract at geometry_ops.rs:2142.
/// Passes on HEAD — characterisation test for already-implemented behaviour.
#[test]
fn try_eval_ad_hoc_selector_non_literal_arg_returns_none() {
    // `@face(<dynamic_expr>)` — args[0] is a ValueRef, not a Literal(String).
    // The base is a Literal so the function reaches the args[0] check.
    let expr = CompiledExpr::ad_hoc_selector(
        CompiledExpr::literal(Value::String("body".to_string()), Type::String),
        SelectorKind::Face,
        vec![CompiledExpr::value_ref(
            ValueCellId::new("Body", "dynamic_label"),
            Type::String,
        )],
    );

    let named_steps = named_steps_with_body();
    let table = seeded_cylinder_table();
    let mut kernel = configured_kernel();
    let mut diagnostics = Vec::new();

    let result = try_eval_ad_hoc_selector(
        &expr,
        &named_steps,
        &mut kernel,
        &table,
        SourceSpan::empty(0),
        &mut diagnostics,
    );

    assert!(
        result.is_none(),
        "non-Literal args[0] should return None (not applicable), got {:?}",
        result
    );
    assert!(
        diagnostics.is_empty(),
        "None-path should emit no diagnostics; got {:?}",
        diagnostics
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Test 12: engine-level TopologyAttributeStale warning carries a non-empty span
// RED on HEAD (geometry_ops.rs:2153 hardcodes SourceSpan::empty(0)).
// Becomes GREEN when step-5 threads cell.span through try_eval_ad_hoc_selector.
// ─────────────────────────────────────────────────────────────────────────────

/// The `TopologyAttributeStale` warning emitted by `engine.build()` for an
/// unresolved `@face` selector must carry a primary label whose source span is
/// non-empty — i.e. it must point at a real byte range in the source, not at
/// the synthetic empty span `{ start: 0, end: 0 }`.
///
/// **RED on HEAD**: `try_eval_ad_hoc_selector` passes `SourceSpan::empty(0)` to
/// `resolve_unique_by_attribute` (geometry_ops.rs:2153), so the label span has
/// `start == end == 0` and `is_empty()` returns `true`.
///
/// **GREEN after step-5**: the `cell.span` from `ValueCellDecl` (populated by
/// the compiler from the `let`-declaration's byte range) is threaded through as
/// `selector_span`, giving the warning a real source location.
#[test]
fn engine_build_topology_stale_warning_carries_nonzero_source_span() {
    let source = r#"structure SpanCheckCylinder {
    let body = cylinder(10mm, 20mm)
    let mystery_frame = body @ face("nonexistent")
}"#;

    let compiled = compile_source(source);
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "expected no compile-time Error diagnostics; got: {:#?}",
        compile_errors
    );

    let checker = SimpleConstraintChecker;
    let kernel = configured_engine_kernel();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&compiled, ExportFormat::Step);

    // Locate the TopologyAttributeStale warning — should be exactly one.
    let stale_warnings: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| {
            d.code == Some(DiagnosticCode::TopologyAttributeStale)
                && d.severity == Severity::Warning
        })
        .collect();
    assert_eq!(
        stale_warnings.len(),
        1,
        "expected exactly one TopologyAttributeStale Warning; got: {:#?}",
        result.diagnostics
    );

    let diag = stale_warnings[0];

    // The warning must have at least one label.
    assert!(
        !diag.labels.is_empty(),
        "TopologyAttributeStale warning must have at least one label; got none. \
         Diagnostic: {:#?}",
        diag
    );

    // The primary label must carry a real (non-empty) source span.
    // RED on HEAD: SourceSpan::empty(0) has start == end == 0 → is_empty() == true.
    // GREEN after step-5: cell.span from the let-decl has start < end → is_empty() == false.
    let primary_span = diag.labels[0].span;
    assert!(
        !primary_span.is_empty(),
        "TopologyAttributeStale warning's primary label should carry a real source span \
         (from the let-decl), but got an empty span {:?}. \
         The span-plumbing impl (step-5) must thread cell.span through \
         try_eval_ad_hoc_selector.",
        primary_span
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Regression pin: @point selector → None (Layer-1 / Layer-2 split)
// ─────────────────────────────────────────────────────────────────────────────

/// Pins the Layer-1 / Layer-2 split contract: `@point` selectors must
/// short-circuit to `None` without touching the kernel.
///
/// `@point` selectors are resolved by Layer-1 (`eval_expr`) directly from
/// literal coordinate arguments; Layer-2 (`try_eval_ad_hoc_selector`) must
/// be a no-op for Point and return `None` with an empty diagnostics list.
#[test]
fn try_eval_ad_hoc_selector_point_returns_none() {
    // `@point(0m, 0m, 0m)` expression: base="body", kind=Point, args=[0m,0m,0m]
    // (Layer-1 resolves @point from its literal coordinate args; Layer-2 must
    // be a no-op and return None without touching the kernel.)
    let expr = CompiledExpr::ad_hoc_selector(
        CompiledExpr::literal(Value::String("body".to_string()), Type::String),
        SelectorKind::Point,
        vec![
            CompiledExpr::literal(Value::length(0.0), Type::length()),
            CompiledExpr::literal(Value::length(0.0), Type::length()),
            CompiledExpr::literal(Value::length(0.0), Type::length()),
        ],
    );

    let named_steps = named_steps_with_body();
    let table = seeded_cylinder_table();
    // MockGeometryKernel with no results configured — any accidental kernel
    // query would return an error, making a spurious Some(Value::Undef) visible.
    let mut kernel = MockGeometryKernel::new();
    let mut diagnostics = Vec::new();

    let result = try_eval_ad_hoc_selector(
        &expr,
        &named_steps,
        &mut kernel,
        &table,
        SourceSpan::empty(0),
        &mut diagnostics,
    );

    assert!(
        result.is_none(),
        "try_eval_ad_hoc_selector with SelectorKind::Point must return None — \
         @point selectors are handled by Layer-1 eval_expr and Layer-2 is a no-op; \
         got {:?}",
        result
    );
    assert!(
        diagnostics.is_empty(),
        "no diagnostics expected for a Point selector (Layer-2 early-returns None \
         without emitting any warning); got {:?}",
        diagnostics
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Task 4142 Cluster B RED contract test (ad-hoc selector leaf)
// ─────────────────────────────────────────────────────────────────────────────

/// Contract test (task 4142, Cluster B RED — ad-hoc selector leaf):
/// `try_eval_ad_hoc_selector` resolves the base body via `KernelHandle.id`,
/// ignoring `KernelHandle.kernel`.
///
/// Uses `KernelId::Manifold` for the body handle (deliberately non-default)
/// to prove the leaf at geometry_ops.rs:4221 keys only off `.id` and never
/// consults `.kernel`.
///
/// RED on current main (after step-2): `try_eval_ad_hoc_selector` still takes
/// `&HashMap<String, GeometryHandleId>` → E0308 type mismatch on `&named_steps`.
/// GREEN after step-4: signature changed + leaf projection updated.
///
/// NOTE: Pins the leaf-projection contract only (`.kernel` unused in the current
/// single-kernel-per-build design). When cross-kernel handle resolution lands,
/// update to assert per-kernel dispatch rather than treating `.kernel` as ignored.
#[test]
fn try_eval_ad_hoc_selector_resolves_base_via_kernel_handle_id() {
    // Map "body" to a KernelHandle with deliberately non-default kernel.
    let mut named_steps: HashMap<String, KernelHandle> = HashMap::new();
    named_steps.insert(
        "body".to_string(),
        KernelHandle {
            kernel: KernelId::Manifold, // non-default: must be ignored
            id: BODY_HANDLE,
        },
    );

    // Same @face("top") expression as the happy-path test above.
    let expr = CompiledExpr::ad_hoc_selector(
        CompiledExpr::literal(Value::String("body".to_string()), Type::String),
        SelectorKind::Face,
        vec![CompiledExpr::literal(
            Value::String("top".to_string()),
            Type::String,
        )],
    );

    let table = seeded_cylinder_table();
    let mut kernel = configured_kernel(); // set up for BODY_HANDLE (.id)
    let mut diagnostics = Vec::new();

    let result = try_eval_ad_hoc_selector(
        &expr,
        &named_steps,
        &mut kernel,
        &table,
        SourceSpan::empty(0),
        &mut diagnostics,
    );

    // Kernel is set up for BODY_HANDLE; `.kernel` (Manifold) must be ignored.
    let Some(Value::Frame {
        ref origin,
        ref basis,
    }) = result
    else {
        panic!(
            "@face(\"top\") with KernelHandle{{Manifold, BODY_HANDLE}} should resolve to \
             Some(Value::Frame {{ .. }}), got {:?}",
            result
        );
    };
    assert_eq!(
        **origin,
        Value::Point(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(0.01),
        ]),
        "@face(\"top\") origin should be (0m, 0m, 0.01m)"
    );
    assert_eq!(
        **basis,
        Value::Orientation {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0
        },
        "@face(\"top\") basis should be identity (normal +Z → +Z is zero rotation)"
    );
    assert!(
        diagnostics.is_empty(),
        "no diagnostic expected on a clean resolution; got {:?}",
        diagnostics
    );
}
