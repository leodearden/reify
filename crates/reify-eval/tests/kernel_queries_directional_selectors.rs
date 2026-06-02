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
//!    - `DirectionalSelectors.top` is `Value::List` of exactly **1**
//!      `Value::GeometryHandle` (the +z top face; sign-sensitive).
//!    - `DirectionalSelectors.vert` is `Value::List` of exactly **4**
//!      `Value::GeometryHandle` (the 4 z-parallel vertical edges; sign-tolerant),
//!      with pairwise-distinct `upstream_values_hash` (PRD §4 iii).
//!
//! Modelled on `topology_selectors_tests.rs::box_faces_integration_test` and
//! `kernel_queries_normal_smoke.rs`.

use reify_constraints::SimpleConstraintChecker;
use reify_core::identity::ValueCellId;
use reify_eval::Engine;
use reify_ir::{ExportFormat, Value};
use reify_test_support::{errors_only, parse_and_compile_with_stdlib};

const FIXTURE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/kernel_queries/directional_selectors.ri"
);

/// End-to-end pin for KGQ-ι: `faces_by_normal` (1 top face) and
/// `edges_parallel_to` (4 vertical edges) on a box, both returning
/// `Value::List([Value::GeometryHandle])` with distinct hashes.
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

    // ── faces_by_normal: exactly 1 face (+z top face) ────────────────────────

    let top_cell = ValueCellId::new("DirectionalSelectors", "top");
    let top_list = match result.values.get(&top_cell) {
        Some(Value::List(elems)) => elems.clone(),
        other => panic!(
            "DirectionalSelectors.top must be Value::List of Value::GeometryHandle \
             (PRD §4 KGQ-ι), got: {other:?}"
        ),
    };
    assert_eq!(
        top_list.len(),
        1,
        "faces_by_normal(box(10,10,10), +z, 1°) must return exactly 1 face \
         (sign-sensitive: only the top face); got {} elements",
        top_list.len()
    );
    match &top_list[0] {
        Value::GeometryHandle {
            upstream_values_hash,
            ..
        } => {
            assert_ne!(
                upstream_values_hash, &[0u8; 32],
                "top[0] upstream_values_hash must be non-zero (PRD §4 i)"
            );
        }
        other => panic!("top[0] must be Value::GeometryHandle, got: {other:?}"),
    }

    // ── edges_parallel_to: exactly 4 edges (z-parallel verticals) ────────────

    let vert_cell = ValueCellId::new("DirectionalSelectors", "vert");
    let vert_list = match result.values.get(&vert_cell) {
        Some(Value::List(elems)) => elems.clone(),
        other => panic!(
            "DirectionalSelectors.vert must be Value::List of Value::GeometryHandle \
             (PRD §4 KGQ-ι), got: {other:?}"
        ),
    };
    assert_eq!(
        vert_list.len(),
        4,
        "edges_parallel_to(box(10,20,30), +z, 1°) must return exactly 4 edges \
         (sign-tolerant: 4 z-parallel verticals); got {} elements",
        vert_list.len()
    );

    // Collect upstream_values_hashes and verify pairwise distinctness (PRD §4 iii).
    let mut hashes: Vec<[u8; 32]> = Vec::new();
    for (i, elem) in vert_list.iter().enumerate() {
        match elem {
            Value::GeometryHandle {
                upstream_values_hash,
                ..
            } => {
                assert_ne!(
                    upstream_values_hash, &[0u8; 32],
                    "vert[{i}] upstream_values_hash must be non-zero (PRD §4 i)"
                );
                hashes.push(*upstream_values_hash);
            }
            other => {
                panic!("vert[{i}] must be Value::GeometryHandle (PRD §4 KGQ-ι), got: {other:?}")
            }
        }
    }
    for i in 0..hashes.len() {
        for j in (i + 1)..hashes.len() {
            assert_ne!(
                hashes[i], hashes[j],
                "vert[{i}] and vert[{j}] must have distinct upstream_values_hashes (PRD §4 iii)"
            );
        }
    }
}
