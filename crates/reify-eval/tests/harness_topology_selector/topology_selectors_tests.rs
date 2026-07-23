//! OCCT-gated end-to-end integration tests for KGQ-η (task 3616):
//! `edges(Solid)` and `faces(Solid)` sub-handle selectors.
//!
//! PRD: `docs/prds/v0_3/kernel-geometry-queries.md` §4/§9
//!
//! ## Fixtures
//!
//! - `examples/kernel_queries/box_edges.ri`:
//!   ```ri
//!   structure def BoxEdges {
//!       param width : Length = 10mm
//!       let b = box(width, 20mm, 30mm)
//!       let es = edges(b)
//!   }
//!   ```
//!   A 10×20×30 mm box has 12 edges. `param width` drives the freshness
//!   cascade test (PRD §4 invariant v).
//!
//! - `examples/kernel_queries/box_faces.ri`:
//!   ```ri
//!   structure def BoxFaces {
//!       let b = box(10mm, 20mm, 30mm)
//!       let fs = faces(b)
//!   }
//!   ```
//!   A 10×20×30 mm box has 6 faces.
//!
//! ## Test structure
//!
//! Each test reads the fixture unconditionally (fixture-presence CI contract)
//! and validates compilation. The OCCT kernel assertions skip cleanly when
//! OCCT is unavailable (`reify_kernel_occt::OCCT_AVAILABLE == false`).
//!
//! Modelled on `crates/reify-eval/tests/kernel_queries_moment_of_inertia_smoke.rs`
//! for the integration harness pattern and on
//! `crates/reify-eval/tests/geometry_handle_freshness.rs` for the freshness
//! cascade pattern.

use reify_constraints::SimpleConstraintChecker;
use reify_core::identity::{RealizationNodeId, ValueCellId};
use reify_core::ty::SelectorKind;
use reify_eval::Engine;
use reify_eval::cache::NodeId;
use reify_ir::value::{LeafQuery, SelectorNode};
use reify_ir::{ExportFormat, Freshness, Value};
use reify_test_support::{errors_only, parse_and_compile_with_stdlib};

const BOX_EDGES_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/kernel_queries/box_edges.ri"
);

const BOX_FACES_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/kernel_queries/box_faces.ri"
);

/// Assert a selector cell holds a kernel-free `Value::Selector(kind)` whose node
/// is an `All` leaf targeting the parent solid (task 4118 γ).
///
/// The 7 predicate/all selector constructors — `edges(b)` / `faces(b)` among
/// them — now evaluate to a typed `Value::Selector(kind)` instead of an eager
/// `Value::List<GeometryHandle>`. Construction is KERNEL-FREE (K2/BT7): the cell
/// holds the typed `All` leaf over the realized parent solid handle, and the
/// `Selector → List<Geometry>` resolution (the canonical 12-edge / 6-face counts)
/// is deferred to `topology_selectors::resolve` — exercised by the resolve() unit
/// tests in `topology_selectors.rs` and the `single(faces_by_normal(...))` golden
/// in `selector_coercion_golden.rs`.
///
/// `parent_realization` is the parent solid cell's `realization_ref`; the leaf
/// target must point at the same realization (PRD §4 — same parent).
fn assert_all_selector(
    cell_value: Option<&Value>,
    label: &str,
    kind: SelectorKind,
    parent_realization: &RealizationNodeId,
) {
    let sv = match cell_value {
        Some(Value::Selector(sv)) => sv,
        other => panic!(
            "{label} must be a kernel-free Value::Selector(kind) (task 4118 γ; BT7), got: {other:?}"
        ),
    };
    assert_eq!(sv.kind, kind, "{label}: selector kind");
    match &sv.node {
        SelectorNode::Leaf { target, query } => {
            assert_eq!(*query, LeafQuery::All, "{label}: edges/faces(b) → All leaf");
            assert_eq!(
                target.realization_ref, *parent_realization,
                "{label}: leaf target realization_ref must equal the parent solid realization \
                 (PRD §4 — same parent)"
            );
        }
        other => panic!("{label} must be a Leaf selector node, got: {other:?}"),
    }
}

/// Extract a `Value::GeometryHandle` cell's `realization_ref`, panicking with
/// `label` if the cell is missing or not a geometry handle.
fn parent_realization_of(cell_value: Option<&Value>, label: &str) -> RealizationNodeId {
    match cell_value {
        Some(Value::GeometryHandle {
            realization_ref, ..
        }) => realization_ref.clone(),
        other => {
            panic!("{label} must hydrate to Value::GeometryHandle after build, got: {other:?}")
        }
    }
}

// ── Integration test: edges(Solid) ──────────────────────────────────────────

/// End-to-end acceptance pin for KGQ-η `edges(Solid)` sub-handle dispatch
/// (PRD §4/§9, task 3616 step-8).
///
/// Builds `box_edges.ri` with the real OCCT kernel and asserts that the
/// `BoxEdges.es` cell holds a kernel-free `Value::Selector(Edge)` with an `All`
/// leaf targeting the realized `BoxEdges.b` solid (task 4118 γ re-typed `edges(b)`
/// from an eager `Value::List<GeometryHandle>` to the typed selector).
///
/// The end-to-end resolution to the 12 edge sub-handles (a 10×20×30 mm box has
/// 12 edges) is deferred to `topology_selectors::resolve` and verified by the
/// resolve() unit tests and the `single(faces_by_normal(...))` golden
/// (`selector_coercion_golden.rs`).
///
/// Fixture is read and compiled unconditionally so fixture absence / compile
/// regressions fail on every CI runner (not only OCCT-enabled ones).
#[test]
fn box_edges_integration_test() {
    // Read and compile unconditionally — fixture-presence CI contract.
    let source = std::fs::read_to_string(BOX_EDGES_PATH)
        .expect("examples/kernel_queries/box_edges.ri should exist (task 3616)");
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "examples/kernel_queries/box_edges.ri should compile with no error diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );

    // Skip OCCT-dependent assertions when the kernel lib is absent.
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping box_edges_integration_test: OCCT not available");
        return;
    }

    // Build with the real OCCT kernel.
    //
    // NOTE: OcctKernelHandle is passed directly (not via SingleKernelHolder)
    // because SingleKernelHolder does NOT override extract_edges / extract_faces
    // — its GeometryKernel impl delegates only execute/query/export/tessellate.
    // The extract_edges/faces dispatch in try_eval_topology_selector calls
    // kernel.extract_edges directly; wrapping in SingleKernelHolder would hit
    // the trait default and return Err("topology extraction not supported").
    // (Documented in topology_selector_runtime.rs lines 999–1012.)
    let checker = SimpleConstraintChecker;
    let kernel: Box<dyn reify_ir::GeometryKernel> =
        Box::new(reify_kernel_occt::OcctKernelHandle::spawn());
    let mut engine = Engine::new(Box::new(checker), Some(kernel));
    let result = engine.build(&compiled, ExportFormat::Step);

    // Task 4118 (γ): `edges(b)` now packages a kernel-free typed selector
    // (`All` leaf over the parent solid handle), not an eager list of sub-handles.
    let parent_realization = parent_realization_of(
        result.values.get(&ValueCellId::new("BoxEdges", "b")),
        "BoxEdges.b",
    );
    assert_all_selector(
        result.values.get(&ValueCellId::new("BoxEdges", "es")),
        "BoxEdges.es",
        SelectorKind::Edge,
        &parent_realization,
    );
}

// ── Integration test: faces(Solid) ──────────────────────────────────────────

/// End-to-end acceptance pin for KGQ-η `faces(Solid)` sub-handle dispatch
/// (PRD §4/§9, task 3616 step-8).
///
/// Builds `box_faces.ri` with the real OCCT kernel and asserts that the
/// `BoxFaces.fs` cell holds a kernel-free `Value::Selector(Face)` with an `All`
/// leaf targeting the realized `BoxFaces.b` solid (task 4118 γ re-typed `faces(b)`
/// from an eager `Value::List<GeometryHandle>` to the typed selector).
///
/// The end-to-end resolution to the 6 face sub-handles (a 10×20×30 mm box has 6
/// faces) is deferred to `topology_selectors::resolve` and verified by the
/// resolve() unit tests and the `single(faces_by_normal(...))` golden
/// (`selector_coercion_golden.rs`).
///
/// Fixture is read and compiled unconditionally so fixture absence / compile
/// regressions fail on every CI runner (not only OCCT-enabled ones).
#[test]
fn box_faces_integration_test() {
    // Read and compile unconditionally — fixture-presence CI contract.
    let source = std::fs::read_to_string(BOX_FACES_PATH)
        .expect("examples/kernel_queries/box_faces.ri should exist (task 3616)");
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "examples/kernel_queries/box_faces.ri should compile with no error diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );

    // Skip OCCT-dependent assertions when the kernel lib is absent.
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping box_faces_integration_test: OCCT not available");
        return;
    }

    // Build with the real OCCT kernel (OcctKernelHandle directly, not via
    // SingleKernelHolder — see box_edges_integration_test for rationale).
    let checker = SimpleConstraintChecker;
    let kernel: Box<dyn reify_ir::GeometryKernel> =
        Box::new(reify_kernel_occt::OcctKernelHandle::spawn());
    let mut engine = Engine::new(Box::new(checker), Some(kernel));
    let result = engine.build(&compiled, ExportFormat::Step);

    // Task 4118 (γ): `faces(b)` now packages a kernel-free typed selector
    // (`All` leaf over the parent solid handle), not an eager list of sub-handles.
    let parent_realization = parent_realization_of(
        result.values.get(&ValueCellId::new("BoxFaces", "b")),
        "BoxFaces.b",
    );
    assert_all_selector(
        result.values.get(&ValueCellId::new("BoxFaces", "fs")),
        "BoxFaces.fs",
        SelectorKind::Face,
        &parent_realization,
    );
}

// ── Freshness cascade test ───────────────────────────────────────────────────

/// OCCT-gated freshness-cascade pin for KGQ-η (PRD §4 invariant v, task 3616
/// step-9).
///
/// Fixture: `box_edges.ri` — `width → R0 → b (geometry cell) → es (selector
/// cell)` via:
///   - R0 reads `[width]` (standard VC→Realization deps.rs trace)
///   - `b` carries `realization_reads = [R0]` (GHR-δ §5, task 3606)
///   - `es` reads `[b]` (standard VC→VC edge: extract_dependency_trace)
///
/// Build the fixture, mark `width` Pending, run `propagate_freshness_only`,
/// and assert that ALL THREE nodes in the cascade chain become Pending:
///   1. `R0` — Realization Widget#0 (width → R0)
///   2. `b`  — geometry value cell (R0 → b via realization_reads fold)
///   3. `es` — selector list cell (b → es via VC→VC reads edge)
///
/// "12 edge cells Pending" is realized at selector-cell granularity: the single
/// `es` cell holding the typed `Value::Selector(Edge)` (task 4118 γ) goes
/// Pending; the next read re-resolves all 12 sub-handles via
/// `topology_selectors::resolve`.
///
/// Skips cleanly (early return) when OCCT is unavailable — both the build
/// and the cascade assertions require OCCT to hydrate `b` into a real
/// `Value::GeometryHandle` so `realization_reads` is wired.
#[test]
fn box_edges_freshness_cascade() {
    // Read and compile unconditionally — fixture-presence CI contract.
    let source = std::fs::read_to_string(BOX_EDGES_PATH)
        .expect("examples/kernel_queries/box_edges.ri should exist (task 3616)");
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "examples/kernel_queries/box_edges.ri should compile with no error diagnostics"
    );

    // Skip OCCT-dependent cascade assertions when the kernel lib is absent.
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping box_edges_freshness_cascade: OCCT not available");
        return;
    }

    // Build with real OCCT to establish the cache and hydrate all cells.
    // (OcctKernelHandle directly, not via SingleKernelHolder — see
    // box_edges_integration_test for rationale.)
    let checker = SimpleConstraintChecker;
    let kernel: Box<dyn reify_ir::GeometryKernel> =
        Box::new(reify_kernel_occt::OcctKernelHandle::spawn());
    let mut engine = Engine::new(Box::new(checker), Some(kernel));
    let result = engine.build(&compiled, ExportFormat::Step);

    // Sanity: the `b` cell must be a GeometryHandle (realization_reads wired).
    let b_cell_id = ValueCellId::new("BoxEdges", "b");
    assert!(
        matches!(
            result.values.get(&b_cell_id),
            Some(Value::GeometryHandle { .. })
        ),
        "BoxEdges.b must hydrate to Value::GeometryHandle after build"
    );

    // Sanity: the `es` cell must be the kernel-free typed selector (task 4118 γ
    // re-typed `edges(b)` from an eager `Value::List` to `Value::Selector(Edge)`).
    // The freshness cascade is unchanged: `es` still reads `[b]` (the selector
    // leaf targets `b`'s realized handle), so the VC→VC edge that propagates
    // Pending is preserved.
    let es_cell_id = ValueCellId::new("BoxEdges", "es");
    assert!(
        matches!(result.values.get(&es_cell_id), Some(Value::Selector(_))),
        "BoxEdges.es must be a kernel-free Value::Selector before cascade; got: {:?}",
        result.values.get(&es_cell_id)
    );

    // Define the three cascade nodes.
    let r0 = RealizationNodeId::new("BoxEdges", 0);
    let r0_node = NodeId::Realization(r0);
    let b_node = NodeId::Value(b_cell_id.clone());
    let es_node = NodeId::Value(es_cell_id.clone());
    let width_cell = ValueCellId::new("BoxEdges", "width");
    let generation = 1u64;

    // Dirty the upstream scalar param `width` that drives the realization.
    let marked = engine
        .cache_store_mut()
        .mark_pending(&NodeId::Value(width_cell.clone()));
    assert!(marked, "BoxEdges.width must be a cache node after build()");

    // Drive the freshness-only walk seeded from the changed width param.
    let updated = engine.propagate_freshness_only(std::iter::once(&width_cell), generation);

    // 1. width → R0: the realization re-derives Pending from dirty scalar input.
    assert!(
        matches!(
            engine.cache_store().freshness(&r0_node),
            Freshness::Pending { .. }
        ),
        "BoxEdges Realization#0 (R0) must be Pending after width is dirtied; \
         got {:?}",
        engine.cache_store().freshness(&r0_node)
    );

    // 2. R0 → b: the geometry cell folds R0's Pending via realization_reads.
    assert!(
        matches!(
            engine.cache_store().freshness(&b_node),
            Freshness::Pending { .. }
        ),
        "BoxEdges.b must be Pending via the Realization→ValueCell edge (GHR-δ §5); \
         got {:?}",
        engine.cache_store().freshness(&b_node)
    );

    // 3. b → es: the selector cell folds b's Pending via VC→VC reads edge.
    assert!(
        matches!(
            engine.cache_store().freshness(&es_node),
            Freshness::Pending { .. }
        ),
        "BoxEdges.es must be Pending via the VC→VC edge es reads=[b] (PRD §4 v); \
         got {:?}",
        engine.cache_store().freshness(&es_node)
    );

    // The updated set must include all three downstream nodes.
    assert!(
        updated.contains(&r0_node),
        "R0 must appear in the walk's updated set; got: {:?}",
        updated
    );
    assert!(
        updated.contains(&b_node),
        "BoxEdges.b must appear in the walk's updated set; got: {:?}",
        updated
    );
    assert!(
        updated.contains(&es_node),
        "BoxEdges.es must appear in the walk's updated set (PRD §4 v); got: {:?}",
        updated
    );
}
