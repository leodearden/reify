//! End-to-end integration test for KGQ-ι directional selectors
//! `faces_by_normal` and `edges_parallel_to` (task 3618,
//! PRD `docs/prds/v0_3/kernel-geometry-queries.md` §9 Phase 3).
//!
//! Fixture: `examples/kernel_queries/directional_selectors.ri`
//!
//! ```ri
//! structure def DirectionalSelectors {
//!     let b1  = box(10mm, 10mm, 10mm)
//!     let dir = vec3(0.0, 0.0, 1.0)
//!     let tol = 1deg
//!     let top = faces_by_normal(b1, dir, tol)
//!
//!     let b2   = box(10mm, 20mm, 30mm)
//!     let vert = edges_parallel_to(b2, dir, tol)
//! }
//! ```
//!
//! Two assertions:
//!
//! 1. **COMPILE-LEVEL** (always) — fixture parses + compiles with no errors,
//!    pinning fixture presence and the grammar/type-system registration for
//!    `faces_by_normal` and `edges_parallel_to` on every CI runner.
//!
//! 2. **OCCT-BACKED RUNTIME** (gated on `reify_kernel_occt::OCCT_AVAILABLE`) —
//!    Task 4118 (γ) re-typed the predicate selector constructors: these cells now
//!    hold a kernel-FREE `Value::Selector(kind)` (the typed leaf), NOT an eager
//!    `Value::List<GeometryHandle>`.
//!    - `DirectionalSelectors.top` is `Value::Selector(Face)` with a
//!      `ByNormal { dir: +z, tol_rad: 1° }` leaf.
//!    - `DirectionalSelectors.vert` is `Value::Selector(Edge)` with a
//!      `ByParallel { axis: +z, tol_rad: 1° }` leaf.
//!
//!    The handle COUNTS (1 top face / 4 z-parallel edges) are verified through
//!    `topology_selectors::resolve` by the resolve() unit tests and the
//!    `single(faces_by_normal(...))` golden (`selector_coercion_golden.rs`).
//!
//! Modelled on `topology_selectors_tests.rs::box_faces_integration_test` and
//! `kernel_queries_normal_smoke.rs`.

use reify_constraints::SimpleConstraintChecker;
use reify_core::identity::ValueCellId;
use reify_core::ty::SelectorKind;
use reify_eval::Engine;
use reify_ir::value::{LeafQuery, SelectorNode};
use reify_ir::{ExportFormat, Value};
use reify_test_support::{errors_only, parse_and_compile_with_stdlib};

const FIXTURE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/kernel_queries/directional_selectors.ri"
);

/// End-to-end pin for KGQ-ι re-typed by task 4118 (γ): `faces_by_normal` and
/// `edges_parallel_to` on a box both build a kernel-free `Value::Selector(kind)`
/// (typed leaf), the `Selector → List<Geometry>` resolution being deferred to
/// `topology_selectors::resolve`.
#[test]
fn directional_selectors_compile_and_return_geometry_handles() {
    // ── assertion 1: fixture exists and compiles cleanly (unconditional) ──────

    let source = std::fs::read_to_string(FIXTURE_PATH)
        .expect("examples/kernel_queries/directional_selectors.ri should exist (task 3618)");
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "directional_selectors.ri should compile with no error diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );

    // ── assertion 2: OCCT-backed runtime (gated) ──────────────────────────────

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping directional_selectors OCCT assertions: OCCT not available");
        return;
    }

    let checker = SimpleConstraintChecker;
    let kernel: Box<dyn reify_ir::GeometryKernel> =
        Box::new(reify_kernel_occt::OcctKernelHandle::spawn());
    let mut engine = Engine::new(Box::new(checker), Some(kernel));
    let result = engine.build(&compiled, ExportFormat::Step);

    // ── faces_by_normal: kernel-free Value::Selector(Face), ByNormal{+z,1°} ───
    //
    // Task 4118 (γ): the 7 predicate/all selector constructors now evaluate to a
    // typed `Value::Selector(kind)` instead of an eager `Value::List<Geometry>`.
    // Construction is KERNEL-FREE (BT7): the cell holds the typed selector leaf
    // (parent solid handle + `ByNormal` predicate), and the `Selector →
    // List<Geometry>` resolution is deferred to `topology_selectors::resolve`
    // (exercised — with the +z single-face count — by the resolve() unit tests in
    // topology_selectors.rs and the `single(faces_by_normal(...))` golden in
    // selector_coercion_golden.rs).

    let top_cell = ValueCellId::new("DirectionalSelectors", "top");
    assert_selector_leaf(
        result.values.get(&top_cell),
        "DirectionalSelectors.top",
        SelectorKind::Face,
        |query| match query {
            LeafQuery::ByNormal { dir, tol_rad } => {
                assert_eq!(*dir, [0.0, 0.0, 1.0], "top leaf ByNormal dir must be +z");
                assert!(
                    *tol_rad > 0.0,
                    "top leaf ByNormal tol_rad must be positive (1°), got {tol_rad}"
                );
            }
            other => panic!("top must be a ByNormal leaf, got: {other:?}"),
        },
    );

    // ── edges_parallel_to: kernel-free Value::Selector(Edge), ByParallel{+z,1°} ─

    let vert_cell = ValueCellId::new("DirectionalSelectors", "vert");
    assert_selector_leaf(
        result.values.get(&vert_cell),
        "DirectionalSelectors.vert",
        SelectorKind::Edge,
        |query| match query {
            LeafQuery::ByParallel { axis, tol_rad } => {
                assert_eq!(
                    *axis, [0.0, 0.0, 1.0],
                    "vert leaf ByParallel axis must be +z"
                );
                assert!(
                    *tol_rad > 0.0,
                    "vert leaf ByParallel tol_rad must be positive (1°), got {tol_rad}"
                );
            }
            other => panic!("vert must be a ByParallel leaf, got: {other:?}"),
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
