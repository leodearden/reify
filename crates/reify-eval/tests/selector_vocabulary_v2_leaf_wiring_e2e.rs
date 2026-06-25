//! Capstone end-to-end wiring test for the 6 selector_vocabulary_v2 leaf
//! constructors registered by task 3523 (faces_perpendicular_to,
//! edges_perpendicular_to, faces_by_surface_kind, edges_by_curve_kind,
//! extremal_by_bbox, extremal_by_centroid).
//!
//! Fixture: `examples/selectors/selector_vocabulary_v2_leaves.ri`
//!
//! Three layers, mirroring `kernel_queries_directional_selectors.rs`
//! (compile + minting) and `selector_vocabulary_v2_e2e.rs` (OCCT counts):
//!
//! 1. **COMPILE-LEVEL** (always) — the fixture parses + compiles with no error
//!    diagnostics, pinning name registration (units.rs
//!    `GEOMETRY_TOPOLOGY_SELECTOR_NAMES` + `topology_selector_result_type`),
//!    the `is_selector_expr` value-typing route (geometry.rs), and the
//!    ANGLE-tol arg slots (builtin_signatures.rs) on every CI runner.
//!
//! 2. **OCCT-BACKED MINTING** (gated on `reify_kernel_occt::OCCT_AVAILABLE`) —
//!    each cell holds a kernel-FREE `Value::Selector(kind)` whose `LeafQuery`
//!    node matches the constructor (4119 typed-selector value model): e.g.
//!    `faces_perp` is `Selector(Face)` / `ByPerpendicular{+Z, 1°}`,
//!    `top_bbox` is `Selector(Face)` / `ByExtremalBbox{axis_index:2, max:true}`.
//!
//! 3. **OCCT-BACKED COUNTS** (gated) — build a real 10mm box, construct the
//!    same `SelectorValue::leaf` for each predicate over the box handle, and
//!    resolve it through the public `topology_selectors::resolve`, asserting
//!    the analytically-derived element counts.

use reify_constraints::SimpleConstraintChecker;
use reify_core::identity::{RealizationNodeId, ValueCellId};
use reify_core::ty::SelectorKind;
use reify_eval::{Engine, topology_selectors};
use reify_ir::value::{GeometryHandleRef, LeafQuery, SelectorNode, SelectorValue};
use reify_ir::{EdgeCurveKind, ExportFormat, FaceSurfaceKind, GeometryOp, Value};
use reify_test_support::{errors_only, parse_and_compile_with_stdlib};

const FIXTURE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/selectors/selector_vocabulary_v2_leaves.ri"
);

/// 10×10×10 mm box, expressed in SI metres at the kernel boundary
/// (matches `selector_vocabulary_v2_e2e.rs`).
const BOX_SIDE_M: f64 = 10.0e-3;

fn ten_mm_box_op() -> GeometryOp {
    GeometryOp::Box {
        width: Value::Real(BOX_SIDE_M),
        height: Value::Real(BOX_SIDE_M),
        depth: Value::Real(BOX_SIDE_M),
    }
}

const STRUCT: &str = "SelectorVocabularyV2Leaves";

// ─────────────────────────────────────────────────────────────────────────────
// Layer 1 (unconditional) + Layer 2 (OCCT-gated minting)
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn selector_vocabulary_v2_leaves_compile_and_mint_typed_selectors() {
    // ── Layer 1: fixture exists and compiles cleanly (unconditional) ──────────
    let source = std::fs::read_to_string(FIXTURE_PATH)
        .expect("examples/selectors/selector_vocabulary_v2_leaves.ri should exist (task 3523)");
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "selector_vocabulary_v2_leaves.ri should compile with no error diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );

    // ── Layer 2: OCCT-backed minting (gated) ──────────────────────────────────
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping v2-leaf minting OCCT assertions: OCCT not available");
        return;
    }

    let checker = SimpleConstraintChecker;
    let kernel: Box<dyn reify_ir::GeometryKernel> =
        Box::new(reify_kernel_occt::OcctKernelHandle::spawn());
    let mut engine = Engine::new(Box::new(checker), Some(kernel));
    let result = engine.build(&compiled, ExportFormat::Step);

    // faces_perp → Selector(Face), ByPerpendicular{+Z, 1°}
    assert_selector_leaf(
        result.values.get(&ValueCellId::new(STRUCT, "faces_perp")),
        "faces_perp",
        SelectorKind::Face,
        |query| match query {
            LeafQuery::ByPerpendicular { axis, tol_rad } => {
                assert_eq!(*axis, [0.0, 0.0, 1.0], "faces_perp ByPerpendicular axis must be +Z");
                assert!(*tol_rad > 0.0, "faces_perp tol_rad must be positive (1°), got {tol_rad}");
            }
            other => panic!("faces_perp must be a ByPerpendicular leaf, got: {other:?}"),
        },
    );

    // edges_perp → Selector(Edge), ByPerpendicular{+Z, 1°}
    assert_selector_leaf(
        result.values.get(&ValueCellId::new(STRUCT, "edges_perp")),
        "edges_perp",
        SelectorKind::Edge,
        |query| match query {
            LeafQuery::ByPerpendicular { axis, tol_rad } => {
                assert_eq!(*axis, [0.0, 0.0, 1.0], "edges_perp ByPerpendicular axis must be +Z");
                assert!(*tol_rad > 0.0, "edges_perp tol_rad must be positive (1°), got {tol_rad}");
            }
            other => panic!("edges_perp must be a ByPerpendicular leaf, got: {other:?}"),
        },
    );

    // faces_planar → Selector(Face), BySurfaceKind(Plane)
    assert_selector_leaf(
        result.values.get(&ValueCellId::new(STRUCT, "faces_planar")),
        "faces_planar",
        SelectorKind::Face,
        |query| match query {
            LeafQuery::BySurfaceKind(kind) => {
                assert_eq!(*kind, FaceSurfaceKind::Plane, "faces_planar must filter on Plane");
            }
            other => panic!("faces_planar must be a BySurfaceKind leaf, got: {other:?}"),
        },
    );

    // edges_linear → Selector(Edge), ByCurveKind(Line)
    assert_selector_leaf(
        result.values.get(&ValueCellId::new(STRUCT, "edges_linear")),
        "edges_linear",
        SelectorKind::Edge,
        |query| match query {
            LeafQuery::ByCurveKind(kind) => {
                assert_eq!(*kind, EdgeCurveKind::Line, "edges_linear must filter on Line");
            }
            other => panic!("edges_linear must be a ByCurveKind leaf, got: {other:?}"),
        },
    );

    // top_bbox → Selector(Face), ByExtremalBbox{axis_index:2 (Z), max:true}
    assert_selector_leaf(
        result.values.get(&ValueCellId::new(STRUCT, "top_bbox")),
        "top_bbox",
        SelectorKind::Face,
        |query| match query {
            LeafQuery::ByExtremalBbox { axis_index, max, tol_m } => {
                assert_eq!(*axis_index, 2, "top_bbox axis must be Z (index 2)");
                assert!(*max, "top_bbox sense must be Max");
                assert!(*tol_m >= 0.0, "top_bbox tol_m must be non-negative, got {tol_m}");
            }
            other => panic!("top_bbox must be a ByExtremalBbox leaf, got: {other:?}"),
        },
    );

    // top_cent → Selector(Face), ByExtremalCentroid{axis_index:2 (Z), max:true}
    assert_selector_leaf(
        result.values.get(&ValueCellId::new(STRUCT, "top_cent")),
        "top_cent",
        SelectorKind::Face,
        |query| match query {
            LeafQuery::ByExtremalCentroid { axis_index, max, tol_m } => {
                assert_eq!(*axis_index, 2, "top_cent axis must be Z (index 2)");
                assert!(*max, "top_cent sense must be Max");
                assert!(*tol_m >= 0.0, "top_cent tol_m must be non-negative, got {tol_m}");
            }
            other => panic!("top_cent must be a ByExtremalCentroid leaf, got: {other:?}"),
        },
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Layer 3 (OCCT-gated counts) — resolve each predicate over a real 10mm box.
//
// Analytic box facts (origin-corner box spanning [0,10mm]^3):
//   * 6 planar faces, 12 line edges.
//   * 4 faces have normals ⊥ to +Z (the ±X / ±Y side faces); the ±Z caps don't.
//   * 8 edges have tangents ⊥ to +Z (the top + bottom rings); the 4 verticals don't.
//   * extremal-by-centroid +Z Max → the single top cap (centroid z = 10mm,
//     sides at 5mm, bottom at 0).
//   * extremal-by-bbox +Z Max → every face whose bbox reaches z=10mm: the top
//     cap PLUS all 4 side faces (which span the full height) = 5 faces.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn selector_vocabulary_v2_leaves_resolve_to_expected_counts() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping v2-leaf count OCCT assertions: OCCT not available");
        return;
    }

    let mut kernel = reify_kernel_occt::OcctKernelHandle::spawn();
    let box_id = kernel
        .execute(&ten_mm_box_op())
        .expect("10mm box should build via OCCT")
        .id;

    let one_deg = 1f64.to_radians();

    assert_eq!(
        resolve_count(
            &mut kernel,
            box_id,
            SelectorKind::Face,
            LeafQuery::BySurfaceKind(FaceSurfaceKind::Plane),
        ),
        6,
        "faces_by_surface_kind(Plane) must select all 6 planar box faces"
    );
    assert_eq!(
        resolve_count(
            &mut kernel,
            box_id,
            SelectorKind::Face,
            LeafQuery::ByPerpendicular { axis: [0.0, 0.0, 1.0], tol_rad: one_deg },
        ),
        4,
        "faces_perpendicular_to(+Z) must select the 4 side faces"
    );
    assert_eq!(
        resolve_count(
            &mut kernel,
            box_id,
            SelectorKind::Edge,
            LeafQuery::ByPerpendicular { axis: [0.0, 0.0, 1.0], tol_rad: one_deg },
        ),
        8,
        "edges_perpendicular_to(+Z) must select the 8 horizontal-ring edges"
    );
    assert_eq!(
        resolve_count(
            &mut kernel,
            box_id,
            SelectorKind::Edge,
            LeafQuery::ByCurveKind(EdgeCurveKind::Line),
        ),
        12,
        "edges_by_curve_kind(Line) must select all 12 box edges"
    );
    assert_eq!(
        resolve_count(
            &mut kernel,
            box_id,
            SelectorKind::Face,
            LeafQuery::ByExtremalBbox { axis_index: 2, max: true, tol_m: 1e-6 },
        ),
        1,
        "extremal_by_bbox(Z,Max) must select the single top face"
    );
    assert_eq!(
        resolve_count(
            &mut kernel,
            box_id,
            SelectorKind::Face,
            LeafQuery::ByExtremalCentroid { axis_index: 2, max: true, tol_m: 1e-6 },
        ),
        1,
        "extremal_by_centroid(Z,Max) must select the single top face"
    );
}

/// Build a leaf selector over the real box handle and resolve it through the
/// public `topology_selectors::resolve`, returning the resolved element count.
fn resolve_count(
    kernel: &mut reify_kernel_occt::OcctKernelHandle,
    box_id: reify_ir::GeometryHandleId,
    kind: SelectorKind,
    query: LeafQuery,
) -> usize {
    let target = GeometryHandleRef {
        realization_ref: RealizationNodeId::new("box", 0),
        upstream_values_hash: [0u8; 32],
        kernel_handle: Some(box_id),
    };
    let sv = SelectorValue::leaf(kind, target, query).expect("leaf construction");
    let mut diags = Vec::new();
    let got = topology_selectors::resolve(&sv, kernel, &mut diags).expect("resolve must succeed");
    got.len()
}

/// Assert a cell holds a kernel-free `Value::Selector` whose node is a single
/// `Leaf` of the expected `kind`, then run `check_query` against the leaf's
/// `LeafQuery`. Mirrors the helper in `kernel_queries_directional_selectors.rs`.
fn assert_selector_leaf(
    cell_value: Option<&Value>,
    label: &str,
    kind: SelectorKind,
    check_query: impl FnOnce(&LeafQuery),
) {
    let sv = match cell_value {
        Some(Value::Selector(sv)) => sv,
        other => panic!("{label} must be a kernel-free Value::Selector (4119 value model), got: {other:?}"),
    };
    assert_eq!(sv.kind, kind, "{label}: selector kind");
    match &sv.node {
        SelectorNode::Leaf { query, .. } => check_query(query),
        other => panic!("{label} must be a Leaf selector node, got: {other:?}"),
    }
}
