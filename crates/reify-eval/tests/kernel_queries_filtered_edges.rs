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
//!    - `FilteredEdges.mid_edges` is `Value::List` of exactly **4**
//!      `Value::GeometryHandle` (the 20mm y-edges selected by [15,25]mm).
//!    - `FilteredEdges.small_faces` is `Value::List` of exactly **2**
//!      `Value::GeometryHandle` (the 200mm² 10×20 z-faces selected by [196,225]mm²).
//!    - `FilteredEdges.top_edges` is `Value::List` of exactly **4**
//!      `Value::GeometryHandle` (the boundary edges of the top z=+15mm face).
//!    - Every element's `upstream_values_hash` is non-zero (PRD §4 i) and,
//!      within each list, pairwise-distinct (PRD §4 iii).
//!
//! Modelled on `kernel_queries_directional_selectors.rs` (task 3618).

use reify_constraints::SimpleConstraintChecker;
use reify_core::identity::ValueCellId;
use reify_eval::Engine;
use reify_ir::{ExportFormat, Value};
use reify_test_support::{errors_only, parse_and_compile_with_stdlib};

const FIXTURE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/kernel_queries/filtered_edges.ri"
);

/// End-to-end pin for KGQ-θ: `edges_by_length` (4 y-edges), `faces_by_area`
/// (2 z-faces), and `edges_at_height` (4 top-plane edges) on a box, all
/// returning `Value::List([Value::GeometryHandle])` with distinct hashes.
#[test]
fn filtered_edges_compile_and_return_geometry_handles() {
    // ── assertion 1: fixture exists and compiles cleanly (unconditional) ──────

    let source = std::fs::read_to_string(FIXTURE_PATH).expect(
        "examples/kernel_queries/filtered_edges.ri should exist (task 3617)",
    );
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

    // ── edges_by_length: exactly 4 edges (20mm y-edges within [15,25]mm) ──────

    let mid_cell = ValueCellId::new("FilteredEdges", "mid_edges");
    let mid_list = match result.values.get(&mid_cell) {
        Some(Value::List(elems)) => elems.clone(),
        other => panic!(
            "FilteredEdges.mid_edges must be Value::List of Value::GeometryHandle \
             (PRD §4 KGQ-θ), got: {other:?}"
        ),
    };
    assert_eq!(
        mid_list.len(),
        4,
        "edges_by_length(box(10,20,30), 15..25mm) must return exactly 4 edges \
         (the 4 y-edges of length 20mm); got {} elements",
        mid_list.len()
    );

    let mut mid_hashes: Vec<[u8; 32]> = Vec::new();
    for (i, elem) in mid_list.iter().enumerate() {
        match elem {
            Value::GeometryHandle { upstream_values_hash, .. } => {
                assert_ne!(
                    upstream_values_hash,
                    &[0u8; 32],
                    "mid_edges[{i}] upstream_values_hash must be non-zero (PRD §4 i)"
                );
                mid_hashes.push(*upstream_values_hash);
            }
            other => panic!(
                "mid_edges[{i}] must be Value::GeometryHandle (PRD §4 KGQ-θ), got: {other:?}"
            ),
        }
    }
    for i in 0..mid_hashes.len() {
        for j in (i + 1)..mid_hashes.len() {
            assert_ne!(
                mid_hashes[i], mid_hashes[j],
                "mid_edges[{i}] and mid_edges[{j}] must have distinct upstream_values_hashes \
                 (PRD §4 iii)"
            );
        }
    }

    // ── faces_by_area: exactly 2 faces (200mm² z-faces within [196,225]mm²) ───

    let sf_cell = ValueCellId::new("FilteredEdges", "small_faces");
    let sf_list = match result.values.get(&sf_cell) {
        Some(Value::List(elems)) => elems.clone(),
        other => panic!(
            "FilteredEdges.small_faces must be Value::List of Value::GeometryHandle \
             (PRD §4 KGQ-θ), got: {other:?}"
        ),
    };
    assert_eq!(
        sf_list.len(),
        2,
        "faces_by_area(box(10,20,30), 196..225mm²) must return exactly 2 faces \
         (the two 10×20=200mm² z-faces); got {} elements",
        sf_list.len()
    );

    let mut sf_hashes: Vec<[u8; 32]> = Vec::new();
    for (i, elem) in sf_list.iter().enumerate() {
        match elem {
            Value::GeometryHandle { upstream_values_hash, .. } => {
                assert_ne!(
                    upstream_values_hash,
                    &[0u8; 32],
                    "small_faces[{i}] upstream_values_hash must be non-zero (PRD §4 i)"
                );
                sf_hashes.push(*upstream_values_hash);
            }
            other => panic!(
                "small_faces[{i}] must be Value::GeometryHandle (PRD §4 KGQ-θ), got: {other:?}"
            ),
        }
    }
    assert_ne!(
        sf_hashes[0], sf_hashes[1],
        "small_faces[0] and small_faces[1] must have distinct upstream_values_hashes \
         (PRD §4 iii)"
    );

    // ── edges_at_height: exactly 4 edges (top z=+15mm boundary edges) ──────────

    let te_cell = ValueCellId::new("FilteredEdges", "top_edges");
    let te_list = match result.values.get(&te_cell) {
        Some(Value::List(elems)) => elems.clone(),
        other => panic!(
            "FilteredEdges.top_edges must be Value::List of Value::GeometryHandle \
             (PRD §4 KGQ-θ), got: {other:?}"
        ),
    };
    assert_eq!(
        te_list.len(),
        4,
        "edges_at_height(box(10,20,30), 15mm, 0.001mm) must return exactly 4 edges \
         (the 4 top-face boundary edges at z=+15mm); got {} elements",
        te_list.len()
    );

    let mut te_hashes: Vec<[u8; 32]> = Vec::new();
    for (i, elem) in te_list.iter().enumerate() {
        match elem {
            Value::GeometryHandle { upstream_values_hash, .. } => {
                assert_ne!(
                    upstream_values_hash,
                    &[0u8; 32],
                    "top_edges[{i}] upstream_values_hash must be non-zero (PRD §4 i)"
                );
                te_hashes.push(*upstream_values_hash);
            }
            other => panic!(
                "top_edges[{i}] must be Value::GeometryHandle (PRD §4 KGQ-θ), got: {other:?}"
            ),
        }
    }
    for i in 0..te_hashes.len() {
        for j in (i + 1)..te_hashes.len() {
            assert_ne!(
                te_hashes[i], te_hashes[j],
                "top_edges[{i}] and top_edges[{j}] must have distinct upstream_values_hashes \
                 (PRD §4 iii)"
            );
        }
    }
}
