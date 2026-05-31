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
use reify_eval::Engine;
use reify_eval::cache::NodeId;
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

// ── Integration test: edges(Solid) ──────────────────────────────────────────

/// End-to-end acceptance pin for KGQ-η `edges(Solid)` sub-handle dispatch
/// (PRD §4/§9, task 3616 step-8).
///
/// Builds `box_edges.ri` with the real OCCT kernel and asserts that the
/// `BoxEdges.es` cell is a `Value::List` of exactly 12 `Value::GeometryHandle`
/// elements (a 10×20×30 mm box has 12 edges), with:
///
/// - every element being `Value::GeometryHandle` (PRD §8.2: es[0] non-Undef)
/// - all 12 `upstream_values_hash` values pairwise distinct (PRD §4 iii)
/// - all 12 elements sharing one `realization_ref` (PRD §4 — same parent)
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

    let cell = ValueCellId::new("BoxEdges", "es");
    let list = match result.values.get(&cell) {
        Some(Value::List(elems)) => elems.clone(),
        other => panic!(
            "BoxEdges.es must be Value::List of Value::GeometryHandle sub-handles \
             (PRD §4 KGQ-η), got: {other:?}"
        ),
    };

    assert_eq!(
        list.len(),
        12,
        "a 10×20×30 mm box must have exactly 12 edges; BoxEdges.es has {} elements",
        list.len()
    );

    // Collect realization_refs and upstream_values_hashes from all elements.
    let mut hashes: Vec<[u8; 32]> = Vec::new();
    let mut realization_refs: Vec<RealizationNodeId> = Vec::new();

    for (i, elem) in list.iter().enumerate() {
        match elem {
            Value::GeometryHandle { realization_ref, upstream_values_hash, .. } => {
                assert!(
                    *upstream_values_hash != [0u8; 32],
                    "es[{i}] upstream_values_hash must be non-zero (PRD §4 i)"
                );
                hashes.push(*upstream_values_hash);
                realization_refs.push(realization_ref.clone());
            }
            other => panic!(
                "BoxEdges.es[{i}] must be Value::GeometryHandle (PRD §8.2 es[0] non-Undef), got: {other:?}"
            ),
        }
    }

    // All 12 upstream_values_hashes must be pairwise distinct (PRD §4 iii).
    for i in 0..hashes.len() {
        for j in (i + 1)..hashes.len() {
            assert_ne!(
                hashes[i], hashes[j],
                "edges {i} and {j} must have distinct upstream_values_hashes (PRD §4 iii)"
            );
        }
    }

    // All 12 elements share the same parent realization_ref (PRD §4 — same parent).
    let first_ref = &realization_refs[0];
    for (i, r) in realization_refs.iter().enumerate() {
        assert_eq!(
            r, first_ref,
            "es[{i}] realization_ref must equal the parent BoxEdges.b realization"
        );
    }
}

// ── Integration test: faces(Solid) ──────────────────────────────────────────

/// End-to-end acceptance pin for KGQ-η `faces(Solid)` sub-handle dispatch
/// (PRD §4/§9, task 3616 step-8).
///
/// Builds `box_faces.ri` with the real OCCT kernel and asserts that the
/// `BoxFaces.fs` cell is a `Value::List` of exactly 6 `Value::GeometryHandle`
/// elements (a 10×20×30 mm box has 6 faces), with:
///
/// - every element being `Value::GeometryHandle` (PRD §8.2 non-Undef)
/// - all 6 `upstream_values_hash` values pairwise distinct (PRD §4 iii)
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

    let cell = ValueCellId::new("BoxFaces", "fs");
    let list = match result.values.get(&cell) {
        Some(Value::List(elems)) => elems.clone(),
        other => panic!(
            "BoxFaces.fs must be Value::List of Value::GeometryHandle sub-handles \
             (PRD §4 KGQ-η), got: {other:?}"
        ),
    };

    assert_eq!(
        list.len(),
        6,
        "a 10×20×30 mm box must have exactly 6 faces; BoxFaces.fs has {} elements",
        list.len()
    );

    // All 6 upstream_values_hashes must be pairwise distinct (PRD §4 iii).
    let mut hashes: Vec<[u8; 32]> = Vec::new();
    for (i, elem) in list.iter().enumerate() {
        match elem {
            Value::GeometryHandle { upstream_values_hash, .. } => {
                assert!(
                    *upstream_values_hash != [0u8; 32],
                    "fs[{i}] upstream_values_hash must be non-zero (PRD §4 i)"
                );
                hashes.push(*upstream_values_hash);
            }
            other => panic!(
                "BoxFaces.fs[{i}] must be Value::GeometryHandle (PRD §8.2 non-Undef), got: {other:?}"
            ),
        }
    }
    for i in 0..hashes.len() {
        for j in (i + 1)..hashes.len() {
            assert_ne!(
                hashes[i], hashes[j],
                "faces {i} and {j} must have distinct upstream_values_hashes (PRD §4 iii)"
            );
        }
    }
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
/// "12 edge cells Pending" is realized at list-cell granularity: the single
/// `es` cell holding all 12 sub-handles goes Pending; the next read re-mints
/// all 12 sub-handles.
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
        matches!(result.values.get(&b_cell_id), Some(Value::GeometryHandle { .. })),
        "BoxEdges.b must hydrate to Value::GeometryHandle after build"
    );

    // Sanity: the `es` cell must be a 12-element list of sub-handles.
    let es_cell_id = ValueCellId::new("BoxEdges", "es");
    match result.values.get(&es_cell_id) {
        Some(Value::List(elems)) if elems.len() == 12 => {}
        other => panic!(
            "BoxEdges.es must be a 12-element Value::List of sub-handles before cascade; got: {other:?}"
        ),
    }

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
        matches!(engine.cache_store().freshness(&r0_node), Freshness::Pending { .. }),
        "BoxEdges Realization#0 (R0) must be Pending after width is dirtied; \
         got {:?}",
        engine.cache_store().freshness(&r0_node)
    );

    // 2. R0 → b: the geometry cell folds R0's Pending via realization_reads.
    assert!(
        matches!(engine.cache_store().freshness(&b_node), Freshness::Pending { .. }),
        "BoxEdges.b must be Pending via the Realization→ValueCell edge (GHR-δ §5); \
         got {:?}",
        engine.cache_store().freshness(&b_node)
    );

    // 3. b → es: the selector cell folds b's Pending via VC→VC reads edge.
    assert!(
        matches!(engine.cache_store().freshness(&es_node), Freshness::Pending { .. }),
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
