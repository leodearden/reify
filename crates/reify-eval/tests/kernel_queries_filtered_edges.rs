//! End-to-end integration test for KGQ-θ filtered selectors
//! `edges_by_length`, `faces_by_area`, and `edges_at_height` (task 3617,
//! PRD `docs/prds/v0_3/kernel-geometry-queries.md` §9 Phase 3).
//!
//! Fixture: `examples/kernel_queries/filtered_edges.ri`
//!
//! ```ri
//! structure def FilteredEdges {
//!     let b = box(10mm, 20mm, 30mm)
//!     let len_range  = 15mm..25mm
//!     let mid_edges  = edges_by_length(b, len_range)
//!     let area_range = 14mm*14mm..15mm*15mm
//!     let small_faces = faces_by_area(b, area_range)
//!     let top_z   = 15mm
//!     let top_tol = 0.001mm
//!     let top_edges = edges_at_height(b, top_z, top_tol)
//! }
//! ```
//!
//! Two assertions:
//!
//! 1. **COMPILE-LEVEL** (always) — fixture parses + compiles with no errors,
//!    pinning fixture presence and the grammar/type-system registration for
//!    `edges_by_length`, `faces_by_area`, and `edges_at_height` on every CI
//!    runner.
//!
//! 2. **OCCT-BACKED RUNTIME** (gated on `reify_kernel_occt::OCCT_AVAILABLE`) —
//!    Task 4118 (γ) re-typed the predicate selector constructors: these cells now
//!    hold a kernel-FREE `Value::Selector(kind)` (the typed leaf), NOT an eager
//!    `Value::List<GeometryHandle>`.
//!    - `FilteredEdges.mid_edges` is `Value::Selector(Edge)` with a
//!      `ByLength { 15..25mm }` leaf.
//!    - `FilteredEdges.small_faces` is `Value::Selector(Face)` with a
//!      `ByArea { 196..225mm² }` leaf.
//!    - `FilteredEdges.top_edges` is `Value::Selector(Edge)` with a
//!      `ByHeight { z=15mm, tol=0.001mm }` leaf.
//!    The handle COUNTS (4 y-edges / 2 z-faces / 4 top edges) are verified through
//!    `topology_selectors::resolve` by the resolve() unit tests and the
//!    `single(faces_by_normal(...))` golden (`selector_coercion_golden.rs`).
//!
//! Modelled on `kernel_queries_directional_selectors.rs` (task 3618).

use reify_constraints::SimpleConstraintChecker;
use reify_core::identity::ValueCellId;
use reify_core::ty::SelectorKind;
use reify_eval::Engine;
use reify_ir::value::{LeafQuery, SelectorNode};
use reify_ir::{ExportFormat, Value};
use reify_test_support::{errors_only, parse_and_compile_with_stdlib};

const FIXTURE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/kernel_queries/filtered_edges.ri"
);

/// End-to-end pin for KGQ-θ re-typed by task 4118 (γ): `edges_by_length`,
/// `faces_by_area`, and `edges_at_height` on a box each build a kernel-free
/// `Value::Selector(kind)` (typed leaf), the `Selector → List<Geometry>`
/// resolution being deferred to `topology_selectors::resolve`.
#[test]
fn filtered_edges_compile_and_return_geometry_handles() {
    // ── assertion 1: fixture exists and compiles cleanly (unconditional) ──────

    let source = std::fs::read_to_string(FIXTURE_PATH)
        .expect("examples/kernel_queries/filtered_edges.ri should exist (task 3617)");
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "filtered_edges.ri should compile with no error diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );

    // ── assertion 2: OCCT-backed runtime (gated) ──────────────────────────────

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping filtered_edges OCCT assertions: OCCT not available");
        return;
    }

    let checker = SimpleConstraintChecker;
    let kernel: Box<dyn reify_ir::GeometryKernel> =
        Box::new(reify_kernel_occt::OcctKernelHandle::spawn());
    let mut engine = Engine::new(Box::new(checker), Some(kernel));
    let result = engine.build(&compiled, ExportFormat::Step);

    // Task 4118 (γ): the 3 filtered selector constructors now build a kernel-FREE
    // `Value::Selector(kind)` (typed leaf), NOT an eager `Value::List<Geometry>`.
    // The handle COUNTS (4 y-edges / 2 z-faces / 4 top edges) are verified through
    // `topology_selectors::resolve` by the resolve() unit tests and the
    // `single(faces_by_normal(...))` golden (`selector_coercion_golden.rs`).

    // ── edges_by_length: Value::Selector(Edge), ByLength{15..25mm} ────────────

    let mid_cell = ValueCellId::new("FilteredEdges", "mid_edges");
    assert_selector_leaf(
        result.values.get(&mid_cell),
        "FilteredEdges.mid_edges",
        SelectorKind::Edge,
        |query| match query {
            LeafQuery::ByLength { min_m, max_m } => {
                assert!(
                    (*min_m - 0.015).abs() < 1e-9 && (*max_m - 0.025).abs() < 1e-9,
                    "mid_edges leaf ByLength must be 15..25mm (0.015..0.025 m), got {min_m}..{max_m}"
                );
            }
            other => panic!("mid_edges must be a ByLength leaf, got: {other:?}"),
        },
    );

    // ── faces_by_area: Value::Selector(Face), ByArea{196..225mm²} ─────────────

    let sf_cell = ValueCellId::new("FilteredEdges", "small_faces");
    assert_selector_leaf(
        result.values.get(&sf_cell),
        "FilteredEdges.small_faces",
        SelectorKind::Face,
        |query| match query {
            LeafQuery::ByArea { min_m2, max_m2 } => {
                assert!(
                    (*min_m2 - 0.000196).abs() < 1e-12 && (*max_m2 - 0.000225).abs() < 1e-12,
                    "small_faces leaf ByArea must be 196..225mm² (1.96e-4..2.25e-4 m²), \
                     got {min_m2}..{max_m2}"
                );
            }
            other => panic!("small_faces must be a ByArea leaf, got: {other:?}"),
        },
    );

    // ── edges_at_height: Value::Selector(Edge), ByHeight{z=15mm,tol=0.001mm} ──

    let te_cell = ValueCellId::new("FilteredEdges", "top_edges");
    assert_selector_leaf(
        result.values.get(&te_cell),
        "FilteredEdges.top_edges",
        SelectorKind::Edge,
        |query| match query {
            LeafQuery::ByHeight { z_m, tol_m } => {
                assert!(
                    (*z_m - 0.015).abs() < 1e-9,
                    "top_edges leaf ByHeight z_m must be 15mm (0.015 m), got {z_m}"
                );
                assert!(
                    (*tol_m - 0.000001).abs() < 1e-12,
                    "top_edges leaf ByHeight tol_m must be 0.001mm (1e-6 m), got {tol_m}"
                );
            }
            other => panic!("top_edges must be a ByHeight leaf, got: {other:?}"),
        },
    );
}

/// Assert a cell holds a kernel-free `Value::Selector` whose node is a single
/// `Leaf` of the expected `kind`, then run `check_query` against the leaf's
/// `LeafQuery` (task 4118 γ). Mirrors the helper in
/// `topology_selector_runtime.rs`.
fn assert_selector_leaf(
    cell_value: Option<&Value>,
    label: &str,
    kind: SelectorKind,
    check_query: impl FnOnce(&LeafQuery),
) {
    let sv = match cell_value {
        Some(Value::Selector(sv)) => sv,
        other => panic!(
            "{label} must be a kernel-free Value::Selector (task 4118 γ; BT7), got: {other:?}"
        ),
    };
    assert_eq!(sv.kind, kind, "{label}: selector kind");
    match &sv.node {
        SelectorNode::Leaf { query, .. } => check_query(query),
        other => panic!("{label} must be a Leaf selector node, got: {other:?}"),
    }
}
