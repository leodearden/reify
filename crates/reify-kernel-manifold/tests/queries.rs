//! Cross-crate integration test exercising the four BRepAndMesh-flagged
//! Phase-2 queries (Distance, Contains, Intersects-parity, GeoEquiv) through
//! the public [`ManifoldKernel::query`] API.
//!
//! # Purpose
//!
//! The in-crate `mod tests` unit tests in `src/kernel.rs` pin `Distance` for
//! the disjoint-cube case. This integration binary extends coverage to the
//! full KGQ-ο set:
//!
//! * **Distance** — generalised to surface-to-surface via `Manifold::min_gap`
//!   (0.0 for overlapping/touching; exact for disjoint axis-aligned cubes).
//! * **Intersects parity** — delivered through the Distance arm + the eval
//!   layer's `d ≤ 0` routing; no dedicated `GeometryQuery::Intersects` variant
//!   exists (see escalation esc-3624-169).
//! * **Contains** — point-in-solid via ray-cast crossing count.
//! * **GeoEquiv** — topology-signature + N=8 sampled-vertex comparison.
//!
//! # Compile-time feature guard
//!
//! Mirrors `boolean_ops_integration.rs:34-42`. If the `features =
//! ["test-fixtures"]` activation on the self-dev-dep in
//! `crates/reify-kernel-manifold/Cargo.toml:54` is dropped, this guard fires
//! at compile time with an actionable message.

#[cfg(not(feature = "test-fixtures"))]
compile_error!(
    "queries.rs requires the `test-fixtures` feature. \
     The self-dev-dep in crates/reify-kernel-manifold/Cargo.toml should \
     activate this feature for ALL integration test binaries — if you are \
     seeing this error, that activation has been dropped. Restore it via \
     `reify-kernel-manifold = { path = \".\", features = [\"test-fixtures\"] }` \
     in [dev-dependencies]."
);

use reify_ir::{GeometryHandleId, GeometryKernel, GeometryQuery, Value};
use reify_kernel_manifold::{kernel::ManifoldKernel, test_fixtures::unit_cube_mesh};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Ingest a `unit_cube_mesh(offset)` and return the stored `GeometryHandleId`.
fn ingest(kernel: &mut ManifoldKernel, offset: [f32; 3]) -> GeometryHandleId {
    kernel
        .ingest_mesh(&unit_cube_mesh(offset))
        .expect("unit_cube_mesh fixture must produce a valid manifold")
        .id
}

/// Call `kernel.query(Distance{from,to})` and return `Ok(Value::Real(d))`.
/// Panics with a descriptive message on any non-`Ok(Value::Real)` result.
fn query_distance(kernel: &ManifoldKernel, from: GeometryHandleId, to: GeometryHandleId) -> f64 {
    match kernel.query(&GeometryQuery::Distance { from, to }) {
        Ok(Value::Real(d)) => d,
        other => panic!(
            "query(Distance{{from={from:?},to={to:?}}}) must return Ok(Value::Real(_)); \
             got {other:?}"
        ),
    }
}

// ---------------------------------------------------------------------------
// Distance tests (steps 1 + 2)
// ---------------------------------------------------------------------------

/// Regression guard: two unit cubes with a 4-unit gap return distance ≈ 4.0.
///
/// `unit_cube_mesh([0,0,0])` spans `[0,1]³`; `unit_cube_mesh([5,0,0])` spans
/// `[5,6]×[0,1]²`.  Closest surface-to-surface distance = `5 − 1 = 4.0`.
/// This test passes both with the old vertex-to-vertex implementation (exact
/// vertex match) and with the new `Manifold::min_gap` (same result for
/// axis-aligned planar faces).
#[test]
fn distance_disjoint_cubes_returns_approx_4() {
    let mut kernel = ManifoldKernel::new();
    let from = ingest(&mut kernel, [0.0, 0.0, 0.0]);
    let to = ingest(&mut kernel, [5.0, 0.0, 0.0]);

    let d = query_distance(&kernel, from, to);

    assert!(
        (d - 4.0).abs() < 1e-6,
        "distance between disjoint cubes (4-unit gap) must be ≈ 4.0; got {d}"
    );
}

/// Distance between two overlapping cubes must be 0.0.
///
/// `unit_cube_mesh([0,0,0])` spans `[0,1]³`; `unit_cube_mesh([0.5,0,0])`
/// spans `[0.5,1.5]×[0,1]²`.  The solids interpenetrate — their surfaces
/// cross — so the true surface-to-surface distance is 0.0 (they are
/// already touching/intersecting).
///
/// **RED (step 1)**: the vertex-to-vertex loop returns 0.5 (minimum
/// pairwise corner distance), not 0.0.  Becomes GREEN in step 2 when
/// `queries::distance` switches to `Manifold::min_gap`.
#[test]
fn distance_overlapping_cubes_returns_zero() {
    let mut kernel = ManifoldKernel::new();
    let from = ingest(&mut kernel, [0.0, 0.0, 0.0]);
    let to = ingest(&mut kernel, [0.5, 0.0, 0.0]);

    let d = query_distance(&kernel, from, to);

    assert!(
        d.abs() < 1e-6,
        "distance between overlapping cubes must be 0.0 (surfaces cross); got {d}"
    );
}

/// Distance between two face-coincident cubes must be 0.0.
///
/// `unit_cube_mesh([0,0,0])` spans `[0,1]³`; `unit_cube_mesh([1,0,0])`
/// spans `[1,2]×[0,1]²`.  The cubes share exactly the `x = 1` face —
/// zero-volume touching.  The surface-to-surface distance is 0.0.
///
/// This case passes today (vertex-to-vertex = 0.0 since both meshes have
/// vertices at x=1) and must continue to pass after the min_gap migration.
#[test]
fn distance_face_coincident_cubes_returns_zero() {
    let mut kernel = ManifoldKernel::new();
    let from = ingest(&mut kernel, [0.0, 0.0, 0.0]);
    let to = ingest(&mut kernel, [1.0, 0.0, 0.0]);

    let d = query_distance(&kernel, from, to);

    assert!(
        d.abs() < 1e-6,
        "distance between face-coincident cubes must be 0.0 (touching at x=1); got {d}"
    );
}

// ---------------------------------------------------------------------------
// Intersects-parity tests (steps 1 + 2)
//
// The eval layer routes `intersects(a, b)` through `GeometryQuery::Distance`
// and classifies `d ≤ 0.0` as intersecting (matching the OCCT path).  These
// tests validate that semantics without needing a dedicated
// `GeometryQuery::Intersects` variant.
// ---------------------------------------------------------------------------

/// Overlapping cubes: distance must be ≤ 0.0 → classified as intersecting.
///
/// **RED (step 1)**: vertex-to-vertex returns 0.5 > 0.0 → intersects
/// classification would wrongly return false.  Becomes GREEN in step 2.
#[test]
fn intersects_overlapping_cubes_distance_le_zero() {
    let mut kernel = ManifoldKernel::new();
    let from = ingest(&mut kernel, [0.0, 0.0, 0.0]);
    let to = ingest(&mut kernel, [0.5, 0.0, 0.0]);

    let d = query_distance(&kernel, from, to);

    assert!(
        d <= 0.0 + 1e-6,
        "overlapping cubes: distance must be ≤ 0.0 so eval-layer classifies \
         as intersecting (d ≤ 0 semantics); got {d}"
    );
}

/// Disjoint cubes: distance must be > 0.0 → classified as non-intersecting.
///
/// This test passes both today (vertex-to-vertex = 4.0) and after step 2
/// (min_gap = 4.0).
#[test]
fn intersects_disjoint_cubes_distance_gt_zero() {
    let mut kernel = ManifoldKernel::new();
    let from = ingest(&mut kernel, [0.0, 0.0, 0.0]);
    let to = ingest(&mut kernel, [5.0, 0.0, 0.0]);

    let d = query_distance(&kernel, from, to);

    assert!(
        d > 0.0,
        "disjoint cubes: distance must be > 0.0 so eval-layer classifies \
         as non-intersecting (d > 0 semantics); got {d}"
    );
}

/// Face-coincident cubes: distance == 0.0 → classified as intersecting
/// (inclusive touching semantics, matching OCCT `d ≤ 0` path).
///
/// This test passes both today (vertex-to-vertex = 0.0) and after step 2
/// (min_gap = 0.0 for touching surfaces).
#[test]
fn intersects_face_coincident_cubes_distance_is_zero() {
    let mut kernel = ManifoldKernel::new();
    let from = ingest(&mut kernel, [0.0, 0.0, 0.0]);
    let to = ingest(&mut kernel, [1.0, 0.0, 0.0]);

    let d = query_distance(&kernel, from, to);

    assert!(
        d.abs() < 1e-6,
        "face-coincident cubes: distance must be 0.0 (touching at x=1 face); \
         OCCT-consistent d ≤ 0 => intersecting; got {d}"
    );
}

// ---------------------------------------------------------------------------
// Contains tests (steps 3 + 4)
// ---------------------------------------------------------------------------

/// Interior point of a unit cube must be classified as contained.
///
/// `unit_cube_mesh([0,0,0])` spans `[0,1]³`.  The point `(0.5, 0.5, 0.5)`
/// is the centroid — clearly inside.
///
/// **RED (step 3)**: `query()` returns `Err(STUB_MSG)` for the `Contains`
/// arm until step 4 wires it.
#[test]
fn contains_interior_point_returns_true() {
    let mut kernel = ManifoldKernel::new();
    let handle = ingest(&mut kernel, [0.0, 0.0, 0.0]);

    let result = kernel.query(&GeometryQuery::Contains {
        handle,
        px: 0.5,
        py: 0.5,
        pz: 0.5,
        tolerance: reify_ir::DEFAULT_CONTAINS_TOLERANCE_M,
    });

    match result {
        Ok(Value::Bool(true)) => {}
        other => panic!(
            "contains(unit_cube, (0.5,0.5,0.5)) must return Ok(Value::Bool(true)); \
             got {other:?}"
        ),
    }
}

/// Exterior point (far outside) must be classified as not contained.
///
/// Point `(5.0, 5.0, 5.0)` is well outside `[0,1]³`.
///
/// **RED (step 3)**: same stub issue; becomes GREEN in step 4.
#[test]
fn contains_exterior_point_returns_false() {
    let mut kernel = ManifoldKernel::new();
    let handle = ingest(&mut kernel, [0.0, 0.0, 0.0]);

    let result = kernel.query(&GeometryQuery::Contains {
        handle,
        px: 5.0,
        py: 5.0,
        pz: 5.0,
        tolerance: reify_ir::DEFAULT_CONTAINS_TOLERANCE_M,
    });

    match result {
        Ok(Value::Bool(false)) => {}
        other => panic!(
            "contains(unit_cube, (5.0,5.0,5.0)) must return Ok(Value::Bool(false)); \
             got {other:?}"
        ),
    }
}

/// Exterior point whose ray passes *through* the cube — 2 crossings, even count.
///
/// Validates the `hits.len() % 2 == 1` parity logic rather than just the
/// zero-crossing case.  A faulty `hits.len() > 0` guard would return `true`
/// here instead of `false`.
///
/// `unit_cube_mesh([0,0,0])` spans `[0,1]³`.  The `contains` implementation
/// casts a ray from the query point in the fixed direction
/// `d = normalize([0.7, 0.5, 0.3])`.  From the origin `(-5.0, -3.0, -2.0)`:
///
/// * **Entry** — at `t ≈ 6.51` the ray crosses the `x = 0` face at
///   `y ≈ 0.57`, `z ≈ 0.14` (both ∈ [0,1]).
/// * **Exit**  — at `t ≈ 7.29` the ray exits through the `y = 1` face at
///   `x ≈ 0.60`, `z ≈ 0.40` (both ∈ [0,1]).
///
/// That yields exactly **2 crossings** — even count → `false`.
#[test]
fn contains_exterior_2crossing_ray_returns_false() {
    let mut kernel = ManifoldKernel::new();
    let handle = ingest(&mut kernel, [0.0, 0.0, 0.0]);

    // (-5, -3, -2) is outside the cube; the ray in direction
    // normalize([0.7, 0.5, 0.3]) traverses the solid (enter x=0, exit y=1) →
    // 2 crossings → even → false.
    let result = kernel.query(&GeometryQuery::Contains {
        handle,
        px: -5.0,
        py: -3.0,
        pz: -2.0,
        tolerance: reify_ir::DEFAULT_CONTAINS_TOLERANCE_M,
    });

    match result {
        Ok(Value::Bool(false)) => {}
        other => panic!(
            "contains(unit_cube, (-5.0,-3.0,-2.0)) must return Ok(Value::Bool(false)); \
             ray passes through solid (2 crossings → even → outside); got {other:?}"
        ),
    }
}

// ---------------------------------------------------------------------------
// GeoEquiv tests (steps 5 + 6)
// ---------------------------------------------------------------------------

/// Two identically-built unit cubes must be geometrically equivalent.
///
/// Both are `unit_cube_mesh([0,0,0])` → same topology counts and same vertex
/// positions.  `geo_equiv` must return `true`.
///
/// **RED (step 5)**: `query()` returns `Err(STUB_MSG)` for the `GeoEquiv`
/// arm until step 6 wires it.
#[test]
fn geo_equiv_identical_cubes_returns_true() {
    let mut kernel = ManifoldKernel::new();
    let left = ingest(&mut kernel, [0.0, 0.0, 0.0]);
    let right = ingest(&mut kernel, [0.0, 0.0, 0.0]);

    let result = kernel.query(&GeometryQuery::GeoEquiv {
        left,
        right,
        tolerance: 1e-6,
    });

    match result {
        Ok(Value::Bool(true)) => {}
        other => panic!(
            "geo_equiv of two identical unit cubes must return Ok(Value::Bool(true)); \
             got {other:?}"
        ),
    }
}

/// A cube translated by 10 units must NOT be geo-equivalent to the origin cube.
///
/// `unit_cube_mesh([0,0,0])` vs `unit_cube_mesh([10,0,0])` — same topology
/// counts but all vertex positions differ by 10 in x, far exceeding `1e-6`.
///
/// **RED (step 5)**: same stub issue; becomes GREEN in step 6.
#[test]
fn geo_equiv_translated_cube_returns_false() {
    let mut kernel = ManifoldKernel::new();
    let left = ingest(&mut kernel, [0.0, 0.0, 0.0]);
    let right = ingest(&mut kernel, [10.0, 0.0, 0.0]);

    let result = kernel.query(&GeometryQuery::GeoEquiv {
        left,
        right,
        tolerance: 1e-6,
    });

    match result {
        Ok(Value::Bool(false)) => {}
        other => panic!(
            "geo_equiv(unit_cube, translated_cube_10) must return Ok(Value::Bool(false)); \
             vertices differ by 10 in x (>> 1e-6 tolerance); got {other:?}"
        ),
    }
}

/// A cube shifted by 1e-4 (sub-tolerance) must be geo-equivalent within 1e-3.
///
/// `unit_cube_mesh([1e-4,0,0])` — the offset is well within f32 precision
/// (~7 decimal digits) so every shifted vertex has a genuine non-zero
/// x-difference of ≈ 1e-4 after f32→f64 widening.  With tolerance `1e-3`,
/// each per-vertex Euclidean distance (≈ 1e-4) is below the threshold, so
/// `geo_equiv` must return `true`.  This exercises a real sub-tolerance
/// per-vertex difference, not a degenerate bit-identical case.
///
/// **RED (step 5)**: same stub issue; becomes GREEN in step 6.
#[test]
fn geo_equiv_within_tolerance_perturbation_returns_true() {
    let mut kernel = ManifoldKernel::new();
    let left = ingest(&mut kernel, [0.0, 0.0, 0.0]);
    // 1e-4 (= 0.0001) is representable in f32; all shifted vertices differ
    // by exactly 1e-4 in x after f32→f64 widening.  Tolerance 1e-3 > 1e-4.
    let right = ingest(&mut kernel, [1e-4_f32, 0.0, 0.0]);

    let result = kernel.query(&GeometryQuery::GeoEquiv {
        left,
        right,
        tolerance: 1e-3,
    });

    match result {
        Ok(Value::Bool(true)) => {}
        other => panic!(
            "geo_equiv(unit_cube, sub-tolerance-shifted cube (1e-4)) must return \
             Ok(Value::Bool(true)) within tolerance 1e-3; got {other:?}"
        ),
    }
}

// ---------------------------------------------------------------------------
// Topology extraction: extract_faces (steps 1 + 2)
//
// Manifold "face" = mesh triangle (NOT a BRep parametric surface patch).
// The unit cube tessellates to 12 triangles, so extract_faces returns 12
// sub-handles — pinning the Manifold-face=triangle semantic gap (PRD Open
// Question §10.5) as runtime behaviour: 12 != 6 (BRep's box face count).
// ---------------------------------------------------------------------------

/// Assert that every id in `handles` is non-INVALID and that the ids are
/// pairwise distinct. Shared by the `extract_faces` / `extract_edges`
/// sub-handle tests.
fn assert_handles_valid_and_distinct(handles: &[GeometryHandleId], label: &str) {
    for (i, h) in handles.iter().enumerate() {
        assert_ne!(
            *h,
            GeometryHandleId::INVALID,
            "{label} sub-handle [{i}] must be a real (non-INVALID) id",
        );
    }
    let mut sorted: Vec<u64> = handles.iter().map(|h| h.0).collect();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        handles.len(),
        "{label} sub-handles must all be distinct ids",
    );
}

/// `extract_faces` on the unit cube returns exactly 12 distinct, valid
/// sub-handles — one per mesh triangle.
///
/// The `unit_cube_mesh` fixture has 12 outward-wound triangles
/// (test_fixtures.rs), so the Manifold kernel — whose "face" is a mesh
/// triangle, not a coalesced parametric surface patch — returns 12
/// sub-handles. This is intentionally NOT 6 (the BRep box face count): the
/// `12 != 6` assertion pins the documented Manifold-face-vs-BRep-face
/// semantic gap (PRD Open Question §10.5) as observable runtime behaviour.
///
/// RED (step-1): `ManifoldKernel` inherits the trait default for
/// `extract_faces`, which returns `Err(QueryError::QueryFailed("topology
/// extraction not supported by this kernel"))`. GREEN is step-2 (sub-shape
/// store + `extract_faces` override).
#[test]
fn extract_faces_unit_cube_returns_12_distinct_handles() {
    let mut kernel = ManifoldKernel::new();
    let handle = ingest(&mut kernel, [0.0, 0.0, 0.0]);

    let faces = kernel
        .extract_faces(handle)
        .expect("extract_faces on a stored unit cube must return Ok(Vec)");

    // (a) one sub-handle per mesh triangle.
    assert_eq!(
        faces.len(),
        12,
        "unit cube tessellates to 12 triangles; extract_faces must return \
         one sub-handle per triangle",
    );

    // (b) 12 != 6 — pins the Manifold-face=triangle vs BRep-face=patch
    // semantic gap. A BRep box has 6 faces; the Manifold mesh has 12
    // triangles. This inequality is the runtime witness of PRD Open
    // Question §10.5.
    assert_ne!(
        faces.len(),
        6,
        "Manifold face count (mesh triangles = 12) must differ from the \
         BRep box face count (parametric patches = 6) — the semantic gap",
    );

    // (c) ids non-INVALID and distinct.
    assert_handles_valid_and_distinct(&faces, "face");
}
