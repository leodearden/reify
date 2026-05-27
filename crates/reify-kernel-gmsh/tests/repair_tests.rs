//! Tests for the v0.3 surface-mesh repair pre-stage.
//!
//! Per the v0.3 FEA PRD, raw OCCT BRepMesh output causes Gmsh to fail on
//! tight features — sliver triangles below an area threshold and pairs of
//! near-coincident vertices both trigger Gmsh failures. The repair stage is
//! pure-Rust, kernel-FFI-free, and runs before the surface mesh is handed
//! to Gmsh.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use reify_kernel_gmsh::repair::{RepairConfig, repair_surface_mesh};
use reify_ir::Mesh;

/// Sliver triangles below the area threshold are collapsed. The output
/// mesh's `indices` array drops the three sliver indices, leaving only the
/// remaining well-formed triangles.
#[test]
fn sliver_triangles_below_area_threshold_are_collapsed() {
    // Triangle 0: equilateral-ish with edge length ~1.0; area ~0.43, well above threshold.
    // Triangle 1: three near-collinear vertices, area ~5e-9, below threshold 1e-6.
    let mesh = Mesh {
        vertices: vec![
            // Triangle 0 corners
            0.0, 0.0, 0.0, // v0
            1.0, 0.0, 0.0, // v1
            0.5, 0.866, 0.0, // v2
            // Triangle 1 corners (collinear-ish along x-axis with tiny y deviation)
            5.0, 0.0, 0.0, // v3
            6.0, 0.0, 0.0, // v4
            5.5, 1e-8, 0.0, // v5 — sliver
        ],
        indices: vec![
            0, 1, 2, // good triangle
            3, 4, 5, // sliver
        ],
        normals: None,
    };
    let cfg = RepairConfig {
        sliver_area_threshold: 1e-6,
        vertex_merge_epsilon: 1e-9,
    };
    let repaired = repair_surface_mesh(&mesh, cfg);
    assert_eq!(
        repaired.indices.len(),
        3,
        "exactly one triangle should survive (the sliver gets dropped); \
         got {} indices = {} triangles",
        repaired.indices.len(),
        repaired.indices.len() / 3
    );
    // Identity check: the surviving triangle must be the WELL-FORMED one
    // (corners at (0,0,0), (1,0,0), (0.5,0.866,0)), not the sliver. A
    // length-only assertion would still pass if a regression accidentally
    // dropped the well-formed triangle and kept the sliver. Look up each
    // surviving index in the (compacted) vertices array and assert the
    // triple matches the well-formed triangle's coordinates.
    let expected: [(f32, f32, f32); 3] = [(0.0, 0.0, 0.0), (1.0, 0.0, 0.0), (0.5, 0.866, 0.0)];
    let tol: f32 = 1e-6;
    for (slot, &idx) in repaired.indices.iter().enumerate() {
        let off = (idx as usize) * 3;
        let got = (
            repaired.vertices[off],
            repaired.vertices[off + 1],
            repaired.vertices[off + 2],
        );
        let want = expected[slot];
        assert!(
            (got.0 - want.0).abs() < tol
                && (got.1 - want.1).abs() < tol
                && (got.2 - want.2).abs() < tol,
            "surviving index {} (slot {}) should reference the well-formed \
             triangle's corner {:?}; got {:?}",
            idx,
            slot,
            want,
            got
        );
    }
}

/// Vertices closer than `vertex_merge_epsilon` are merged into a single
/// vertex; triangles that referenced the merged vertex are re-indexed onto
/// the survivor.
#[test]
fn near_coincident_vertices_are_merged() {
    // Two vertices at distance 1e-12 (well below epsilon 1e-9), one far away.
    // Triangle uses all three; after repair the duplicate goes away and the
    // triangle's three indices include the survivor instead.
    let mesh = Mesh {
        vertices: vec![
            0.0, 0.0, 0.0, // v0
            1e-13, 0.0, 0.0, // v1 (1e-13 from v0 — should merge into v0)
            1.0, 0.0, 0.0, // v2 (far)
            0.5, 1.0, 0.0, // v3 (far)
        ],
        // Triangle (v1, v2, v3) — v1 should re-index to v0 after merging.
        indices: vec![1, 2, 3],
        normals: None,
    };
    let cfg = RepairConfig {
        sliver_area_threshold: 1e-9,
        vertex_merge_epsilon: 1e-9,
    };
    let repaired = repair_surface_mesh(&mesh, cfg);
    assert_eq!(
        repaired.vertices.len(),
        9,
        "one merged-away vertex means 3 surviving positions × 3 floats = 9; got {}",
        repaired.vertices.len()
    );
    // The triangle should reference 3 distinct surviving indices, none of
    // which equal the dropped one. Defence-in-depth: indices must be in range.
    for &idx in &repaired.indices {
        assert!(
            (idx as usize) * 3 < repaired.vertices.len(),
            "all surviving indices must be in range of the compacted vertices array"
        );
    }
}

/// A clean mesh with no slivers and no near-coincident vertices passes
/// through unchanged — vertex count, index count, and exact contents
/// preserved bit-for-bit.
#[test]
fn well_formed_mesh_passes_through_unchanged() {
    let mesh = Mesh {
        vertices: vec![
            0.0, 0.0, 0.0, //
            1.0, 0.0, 0.0, //
            0.5, 0.866, 0.0, //
            0.5, 0.289, 0.816, //
        ],
        indices: vec![0, 1, 2, 0, 1, 3, 0, 2, 3, 1, 2, 3],
        normals: None,
    };
    let cfg = RepairConfig {
        sliver_area_threshold: 1e-9,
        vertex_merge_epsilon: 1e-12,
    };
    let repaired = repair_surface_mesh(&mesh, cfg);
    assert_eq!(repaired.vertices, mesh.vertices, "vertices preserved");
    assert_eq!(repaired.indices, mesh.indices, "indices preserved");
    assert!(repaired.normals.is_none());
}

/// A mesh with 100_001 vertices (just above `LARGE_VERT_THRESHOLD = 100_000`)
/// must NOT crash the test binary — the debug_assert! (before fix) fires in
/// debug/test builds and causes a panic that never reaches the assertions.
/// After fix (step-8): `debug_assert!` is replaced by `tracing::warn!` and
/// the function runs to completion, emitting exactly one WARN event at the
/// `reify_kernel_gmsh::repair` target.
///
/// Performance note (baked into the test design): all 100_001 vertices are
/// placed at the origin (0, 0, 0). The inner first-match-wins-and-break merge
/// loop triggers on j=0 for every i ≥ 1, reducing the O(n²) scan to ≈100k
/// total iterations (O(n)), making this safe to run as a unit test. Empty
/// `indices` ensures the per-triangle step is a no-op — the test targets the
/// perf-canary code path only.
#[test]
fn large_vertex_count_emits_warn_does_not_panic() {
    // Prime the callsite cache so per-test with_default subscribers see events
    // even if a prior test thread hit the callsite with no subscriber active.
    reify_test_support::prime_tracing_callsite_cache();

    // 100_001 vertices all at origin: vertices.len() == 300_003.
    let vertices: Vec<f32> = vec![0.0_f32; 100_001 * 3];
    let mesh = Mesh {
        vertices,
        indices: vec![], // empty indices → triangle loop is a no-op
        normals: None,
    };
    let cfg = RepairConfig::default();

    let (subscriber, counters) = reify_test_support::CountingSubscriberBuilder::new()
        .count_level(tracing::Level::WARN)
        .target_prefix("reify_kernel_gmsh::repair")
        .build();
    let warn_arc = Arc::clone(&counters[&tracing::Level::WARN]);

    // (a) Must NOT panic — reaching the assertion below proves the function
    //     returned normally rather than crashing via debug_assert!.
    let _repaired =
        tracing::subscriber::with_default(subscriber, || repair_surface_mesh(&mesh, cfg));

    // (b) Exactly one WARN event must be emitted at the
    //     reify_kernel_gmsh::repair target with reason="large_mesh_perf".
    let warn_count = warn_arc.load(Ordering::Acquire);
    assert_eq!(
        warn_count, 1,
        "expected exactly 1 WARN event at reify_kernel_gmsh::repair; got {}",
        warn_count
    );
}

/// Verifies the TRANSITIVE (chain) merge property documented in `repair.rs`
/// lines 59-70. When vertices A↔B and B↔C are each within `vertex_merge_epsilon`
/// but A↔C is NOT directly within epsilon, the first-match-wins-and-break loop
/// still collapses all three into A's position via the chain:
///
///   i=1, j=0 → merge_map[1] = merge_map[0] = 0   (A is B's survivor)
///   i=2, j=0 misses (|A-C|² > eps_sq),
///   i=2, j=1 hits  → merge_map[2] = merge_map[1] = 0   (transitive: A is C's survivor)
///
/// Existing tests (`near_coincident_vertices_are_merged`) only cover the DIRECT
/// pair case. A refactor that compares `dist(i, merge_map[j])` instead of
/// `dist(i, j)` would break chain semantics and fail this test, since
/// `|C - merge_map[B]| = |C - A| > epsilon` — whereas the direct-pair test
/// would still pass. A spatial-hash refinement that drops chain semantics
/// altogether would also fail this test.
///
/// Numerical stability: vertex spacing 1e-9, epsilon 1.5e-9 →
///   eps_sq = 2.25e-18, |A-B|² = |B-C|² ≈ 1e-18 ≤ eps_sq (both merge),
///   |A-C|² ≈ 4e-18 > eps_sq (no direct merge).
///   f32-rounded literals give `(1e-9_f32 as f64).powi(2) ≈ 1.0e-18` and
///   `(2e-9_f32 as f64).powi(2) ≈ 4.0e-18` — both safely on opposite sides
///   of `eps_sq = 2.25e-18`.
#[test]
fn chain_merge_collapses_via_intermediate_vertex() {
    // A=(0,0,0), B=(1e-9,0,0), C=(2e-9,0,0): the three chain vertices.
    // D=(10,0,0), E=(10,10,0): far vertices used to form non-degenerate triangles.
    // Three triangles: (A,D,E), (B,D,E), (C,D,E) — each ~50 m² area, far above
    // the sliver_area_threshold of 1e-12.
    let mesh = Mesh {
        vertices: vec![
            0.0_f32, 0.0, 0.0, // v0 = A
            1e-9_f32, 0.0, 0.0, // v1 = B  (1e-9 from A, within epsilon 1.5e-9)
            2e-9_f32, 0.0, 0.0, // v2 = C  (1e-9 from B; 2e-9 from A: no direct A↔C merge)
            10.0_f32, 0.0, 0.0, // v3 = D
            10.0_f32, 10.0, 0.0, // v4 = E
        ],
        indices: vec![
            0, 3, 4, // triangle (A, D, E)
            1, 3, 4, // triangle (B, D, E)
            2, 3, 4, // triangle (C, D, E)
        ],
        normals: None,
    };
    let cfg = RepairConfig {
        sliver_area_threshold: 1e-12, // far below the ~50 m² triangle areas
        vertex_merge_epsilon: 1.5e-9,
    };
    let repaired = repair_surface_mesh(&mesh, cfg);

    // (1) Only A, D, E survive after compaction (B and C merged into A):
    //     3 vertices × 3 floats = 9.
    assert_eq!(
        repaired.vertices.len(),
        9,
        "only A, D, E should survive after chain-merge compaction; \
         got {} floats = {} vertices",
        repaired.vertices.len(),
        repaired.vertices.len() / 3
    );

    // (2) All three triangles survive — none are degenerate after merging
    //     (each maps to distinct compacted indices A_slot, D_slot, E_slot)
    //     and none are slivers (~50 m² >> 1e-12 threshold).
    //     Note: all three triangles map to the same (A,D,E) tuple post-merge;
    //     this test relies on `repair_surface_mesh` NOT deduping identical triangle tuples.
    assert_eq!(
        repaired.indices.len(),
        9,
        "all 3 triangles should survive (none degenerate, none slivers); \
         got {} indices = {} triangles",
        repaired.indices.len(),
        repaired.indices.len() / 3
    );

    // (3) All three triangles' first-corner indices must be EQUAL — proves that
    //     B chain-merged to A's survivor AND that C TRANSITIVELY chain-merged
    //     through B to A's survivor. A refactor comparing dist(i, merge_map[j])
    //     instead of dist(i, j) would fail to chain C → A (|C - A| > epsilon)
    //     and fail these checks.
    assert_eq!(
        repaired.indices[0], repaired.indices[3],
        "B should chain-merge to A's survivor \
         (indices[0] = triangle-(A,D,E) first-corner, \
          indices[3] = triangle-(B,D,E) first-corner after merge)"
    );
    assert_eq!(
        repaired.indices[0], repaired.indices[6],
        "C should TRANSITIVELY chain-merge to A's survivor via B \
         (indices[6] = triangle-(C,D,E) first-corner after merge)"
    );

    // (4) The survivor's position must be A=(0,0,0), pinning the lowest-index-wins
    //     semantic. A regression that picked a different survivor (e.g., averaged
    //     positions or chose the highest-index vertex) would fail here while still
    //     passing assertions (1)-(3).
    let survivor_slot = repaired.indices[0] as usize;
    let tol = 1e-6_f32;
    let sx = repaired.vertices[survivor_slot * 3];
    let sy = repaired.vertices[survivor_slot * 3 + 1];
    let sz = repaired.vertices[survivor_slot * 3 + 2];
    assert!(
        sx.abs() < tol && sy.abs() < tol && sz.abs() < tol,
        "chain-merge survivor must be A=(0,0,0) (lowest-index wins); \
         got ({}, {}, {})",
        sx,
        sy,
        sz
    );
}
