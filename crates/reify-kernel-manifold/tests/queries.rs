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

use reify_ir::{GeometryHandleId, GeometryKernel, GeometryOp, GeometryQuery, QueryError, Value};
use reify_kernel_manifold::{kernel::ManifoldKernel, test_fixtures::unit_cube_mesh};

#[path = "support/query_wire.rs"]
mod query_wire;
use query_wire::parse_bbox;

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
// Topology extraction: extract_faces (task-4262 coplanar coalescing)
//
// After task-4262, extract_faces groups coplanar triangles into planar faces
// and emits ONE sub-handle per planar face. A unit cube has 6 planar faces
// (2 triangles each), so extract_faces returns 6 sub-handles — matching
// BRep parity (PRD Open Question §10.5 resolved: coalesced planar-face
// semantics).
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

/// `extract_faces` on the unit cube returns exactly 6 distinct, valid
/// sub-handles — one per coalesced planar face (BRep parity).
///
/// The unit cube tessellates to 12 triangles grouped into 6 planar faces
/// (2 coplanar triangles per face). After task-4262's coplanar coalescing,
/// `extract_faces` emits one sub-handle per planar face, matching OCCT's
/// BRep box face count (6). The `12 ≠ 6` semantic gap documented in PRD
/// Open Question §10.5 is resolved: both kernels now report 6 faces.
///
/// RED (step-1): extract_faces still returns 12 (one handle per triangle);
/// GREEN after step-2 (coalesce_coplanar_faces groups them into 6 planar faces).
#[test]
fn extract_faces_unit_cube_returns_6_planar_faces() {
    let mut kernel = ManifoldKernel::new();
    let handle = ingest(&mut kernel, [0.0, 0.0, 0.0]);

    let faces = kernel
        .extract_faces(handle)
        .expect("extract_faces on a stored unit cube must return Ok(Vec)");

    // (a) 6 planar faces — BRep parity. The unit cube has 6 planar surface
    // patches (2 coplanar triangles each); coalescing must yield exactly 6.
    assert_eq!(
        faces.len(),
        6,
        "unit cube must yield 6 planar-face handles after coplanar coalescing \
         (BRep parity: 6 == OCCT box faces)",
    );

    // (b) ids non-INVALID and pairwise distinct.
    assert_handles_valid_and_distinct(&faces, "face");
}

/// `extract_faces` on the same parent handle returns the **same** ids in the
/// **same order** on every call (per-parent memoization).
///
/// The per-parent idempotency contract mirrors OCCT's
/// (`crates/reify-kernel-occt/src/lib.rs:619-628` doc): given the same parent
/// handle, the returned `Vec<GeometryHandleId>` must be element-for-element
/// identical — both in id values and in order — across calls.
///
/// RED (step-3): after step-2 (coalescing), `store_sub_shape` mints FRESH
/// monotonic ids on every call, so the second `extract_faces` returns a
/// disjoint vec and `assert_eq!` fails.  GREEN after step-4 (memoization).
#[test]
fn extract_faces_is_idempotent_per_parent_handle() {
    let mut kernel = ManifoldKernel::new();
    let handle = ingest(&mut kernel, [0.0, 0.0, 0.0]);

    let first = kernel
        .extract_faces(handle)
        .expect("first extract_faces call must succeed");
    let second = kernel
        .extract_faces(handle)
        .expect("second extract_faces call must succeed");

    assert_eq!(
        first,
        second,
        "extract_faces must return identical ids in identical order for the same parent handle \
         (per-parent memoization contract, mirroring OCCT); \
         first={first:?}, second={second:?}",
    );
}

// ---------------------------------------------------------------------------
// Topology extraction: extract_edges (steps 3 + 4)
// ---------------------------------------------------------------------------

/// `extract_edges` on the unit cube returns exactly 18 distinct, valid
/// sub-handles — one per unique undirected mesh edge.
///
/// The closed cube mesh has 8 vertices and 12 triangles; by Euler's formula
/// for a genus-0 closed surface `V - E + F = 2` => `8 - E + 12 = 2` => `E =
/// 18`. This equals `Manifold::num_edge()` for the cube. The canonical
/// edge enumeration (deduped undirected vertex-index pairs) must therefore
/// yield 18 sub-handles.
///
/// RED (step-3): `ManifoldKernel` inherits the trait default for
/// `extract_edges`, which returns `Err(QueryError::QueryFailed("topology
/// extraction not supported by this kernel"))`. GREEN is step-4 (canonical
/// edge enumeration + `extract_edges` override).
#[test]
fn extract_edges_unit_cube_returns_18_distinct_handles() {
    let mut kernel = ManifoldKernel::new();
    let handle = ingest(&mut kernel, [0.0, 0.0, 0.0]);

    let edges = kernel
        .extract_edges(handle)
        .expect("extract_edges on a stored unit cube must return Ok(Vec)");

    // One sub-handle per unique undirected mesh edge: V - E + F = 2 =>
    // 8 - E + 12 = 2 => E = 18 (= Manifold::num_edge() for the cube).
    assert_eq!(
        edges.len(),
        18,
        "closed cube mesh has 18 unique edges (Euler V-E+F=2: 8-E+12=2); \
         extract_edges must return one sub-handle per edge",
    );

    assert_handles_valid_and_distinct(&edges, "edge");
}

/// `extract_edges` on the same parent handle returns the **same** ids in the
/// **same order** on every call (per-parent memoization).
///
/// The per-parent idempotency contract mirrors OCCT's
/// (`crates/reify-kernel-occt/src/lib.rs:619-628` doc) and
/// `extract_faces_is_idempotent_per_parent_handle` above: given the same
/// parent handle, the returned `Vec<GeometryHandleId>` must be
/// element-for-element identical — both in id values and in order — across
/// calls.
///
/// RED (task η step-1): `extract_edges` mints FRESH monotonic ids via
/// `store_sub_shape` on every call — no `extracted_edges` cache exists — a
/// dormant re-instance of the #4262 defect that `extract_faces` already had
/// fixed, so the second call returns a disjoint vec and `assert_eq!` fails.
/// GREEN after task η step-3 (mirrored `extracted_edges` memoization cache).
#[test]
fn extract_edges_is_idempotent_per_parent_handle() {
    let mut kernel = ManifoldKernel::new();
    let handle = ingest(&mut kernel, [0.0, 0.0, 0.0]);

    let first = kernel
        .extract_edges(handle)
        .expect("first extract_edges call must succeed");
    let second = kernel
        .extract_edges(handle)
        .expect("second extract_edges call must succeed");

    assert_eq!(
        first,
        second,
        "extract_edges must return identical ids in identical order for the same parent handle \
         (per-parent memoization contract, mirroring OCCT); \
         first={first:?}, second={second:?}",
    );
}

// ---------------------------------------------------------------------------
// Sub-element property queries: SurfaceArea + FaceNormal (steps 5 + 6),
// EdgeTangent + BoundingBox (steps 7 + 8)
// ---------------------------------------------------------------------------

/// Parse the OCCT-compatible `{"x":_,"y":_,"z":_}` JSON wire format emitted by
/// the FaceNormal / EdgeTangent / CenterOfMass query arms into `[x, y, z]`.
///
/// Mirrors OCCT's `parse_centroid_json` test decoder so both kernels' replies
/// are read identically (the cross-kernel parity contract).
fn parse_xyz(s: &str) -> [f64; 3] {
    let mut out = [f64::NAN; 3];
    let trimmed = s.trim().trim_start_matches('{').trim_end_matches('}');
    for field in trimmed.split(',') {
        let (key, val) = field
            .split_once(':')
            .unwrap_or_else(|| panic!("malformed xyz field {field:?} in {s:?}"));
        let key = key.trim().trim_matches('"');
        let val: f64 = val
            .trim()
            .parse()
            .unwrap_or_else(|_| panic!("non-numeric value in field {field:?} of {s:?}"));
        match key {
            "x" => out[0] = val,
            "y" => out[1] = val,
            "z" => out[2] = val,
            other => panic!("unexpected key {other:?} in xyz JSON {s:?}"),
        }
    }
    for (i, c) in out.iter().enumerate() {
        assert!(!c.is_nan(), "xyz JSON {s:?} missing component {i}");
    }
    out
}

/// Assert that `n` is a unit vector and axis-aligned (exactly one component
/// ≈ ±1, the other two ≈ 0). Every facet/edge of an axis-aligned unit cube
/// has this property, so the check is triangle-order-independent.
fn assert_unit_axis_aligned(n: [f64; 3], label: &str) {
    let mag = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    assert!(
        (mag - 1.0).abs() < 1e-6,
        "{label} must be a unit vector; got {n:?} (|v|={mag})",
    );
    let near_one = n.iter().filter(|c| (c.abs() - 1.0).abs() < 1e-6).count();
    let near_zero = n.iter().filter(|c| c.abs() < 1e-6).count();
    assert_eq!(
        (near_one, near_zero),
        (1, 2),
        "{label} of an axis-aligned cube must have one ±1 and two ≈0 components; got {n:?}",
    );
}

/// Sub-face `SurfaceArea` and `FaceNormal` over the unit cube's 6 coalesced planar faces.
///
/// After task-4262 coplanar coalescing, each planar face holds two coplanar
/// right triangles (legs 1, 1) → area = 0.5 + 0.5 = **1.0** per face.  The 6
/// faces sum to the cube's total surface area **6.0**.
///
/// Every planar face of an axis-aligned unit cube has an axis-aligned unit
/// normal. The six faces correspond to the ±X, ±Y, ±Z pairs, so all six
/// axis directions must appear **exactly once** among the returned normals.
/// Sign is accepted either way per the FaceNormal contract.
///
/// RED (step-1b): `extract_faces` still returns 12 one-triangle handles, so
/// `faces.len() == 6` fails. GREEN after step-2 (coalesce_coplanar_faces).
#[test]
fn query_sub_face_surface_area_and_normal_unit_cube() {
    let mut kernel = ManifoldKernel::new();
    let handle = ingest(&mut kernel, [0.0, 0.0, 0.0]);
    let faces = kernel
        .extract_faces(handle)
        .expect("extract_faces must succeed");
    // After coplanar coalescing: 6 planar faces (BRep parity).
    assert_eq!(faces.len(), 6, "unit cube must have 6 coalesced planar faces");

    let mut area_sum = 0.0;
    // Track which axis-direction normals we have seen (quantised to ±1 per axis).
    // Represented as a sorted Vec of (axis, sign) pairs so we can assert all 6.
    let mut axis_dirs: Vec<(usize, i8)> = Vec::new();
    for (i, &f) in faces.iter().enumerate() {
        // (a) per-planar-face area == 1.0 (two right triangles with legs 1,1).
        let area = match kernel.query(&GeometryQuery::SurfaceArea(f)) {
            Ok(Value::Real(a)) => a,
            other => panic!(
                "SurfaceArea(face[{i}]) must return Ok(Value::Real(_)); got {other:?}"
            ),
        };
        assert!(
            (area - 1.0).abs() < 1e-6,
            "unit-cube planar face [{i}] = two right triangles (legs 1,1) => area 1.0; got {area}",
        );
        area_sum += area;

        // (b) per-planar-face normal is a unit, axis-aligned vector.
        let n = match kernel.query(&GeometryQuery::FaceNormal(f)) {
            Ok(Value::String(s)) => parse_xyz(&s),
            other => panic!(
                "FaceNormal(face[{i}]) must return Ok(Value::String(_)); got {other:?}"
            ),
        };
        assert_unit_axis_aligned(n, &format!("FaceNormal(face[{i}])"));
        // Record which (axis, sign) this normal corresponds to.
        for (axis, &component) in n.iter().enumerate() {
            if (component.abs() - 1.0).abs() < 1e-6 {
                let sign = if component > 0.0 { 1i8 } else { -1i8 };
                axis_dirs.push((axis, sign));
            }
        }
    }

    // (c) sum of all 6 planar-face areas == total cube surface area 6.0.
    assert!(
        (area_sum - 6.0).abs() < 1e-6,
        "sum of 6 unit-cube planar-face areas must be 6.0; got {area_sum}",
    );

    // (d) all six axis directions (+/-X, +/-Y, +/-Z) appear exactly once.
    axis_dirs.sort_unstable();
    let expected: Vec<(usize, i8)> = {
        let mut v = vec![(0, -1), (0, 1), (1, -1), (1, 1), (2, -1), (2, 1)];
        v.sort_unstable();
        v
    };
    assert_eq!(
        axis_dirs,
        expected,
        "unit cube must have exactly one planar face per ±X/±Y/±Z direction; got {axis_dirs:?}",
    );
}

/// Sub-edge `EdgeTangent` and `BoundingBox` over the unit cube's 18 edges.
///
/// The closed cube mesh's 18 unique edges partition exactly into **12
/// axis-aligned unit edges** (the cube's geometric edges, length 1) and **6
/// face diagonals** (one per cube face, splitting it into two triangles,
/// length √2 spanning two axes). This partition is grounded in the fixture's
/// triangulation, so the test is triangle-order-independent.
///
/// Assertions:
/// - every edge's `EdgeTangent` is a unit vector (sign-agnostic per contract);
/// - every edge's `BoundingBox` parses with all 6 keys and `min ≤ max`;
/// - the 12 axis-aligned edges each span exactly one axis by 1.0 (the other
///   two degenerate, `min == max`) — the per-edge bbox the eval-side
///   `edges_at_height` Z-filter consumes — and their tangent is axis-aligned
///   along that spanned axis;
/// - the 6 diagonals each span exactly two axes by 1.0;
/// - the partition counts are exactly 12 + 6 = 18.
#[test]
fn query_sub_edge_tangent_and_bbox_unit_cube() {
    let mut kernel = ManifoldKernel::new();
    let handle = ingest(&mut kernel, [0.0, 0.0, 0.0]);
    let edges = kernel
        .extract_edges(handle)
        .expect("extract_edges must succeed");
    assert_eq!(edges.len(), 18, "unit cube must have 18 edges");

    let mut axis_aligned = 0usize;
    let mut diagonals = 0usize;
    for (i, &e) in edges.iter().enumerate() {
        // (a) EdgeTangent: a unit vector.
        let t = match kernel.query(&GeometryQuery::EdgeTangent(e)) {
            Ok(Value::String(s)) => parse_xyz(&s),
            other => panic!(
                "EdgeTangent(edge[{i}]) must return Ok(Value::String(_)); got {other:?}"
            ),
        };
        let tmag = (t[0] * t[0] + t[1] * t[1] + t[2] * t[2]).sqrt();
        assert!(
            (tmag - 1.0).abs() < 1e-6,
            "EdgeTangent(edge[{i}]) must be a unit vector; got {t:?} (|t|={tmag})",
        );

        // (b) BoundingBox: 6 keys, min <= max per axis.
        let bb = match kernel.query(&GeometryQuery::BoundingBox(e)) {
            Ok(Value::String(s)) => parse_bbox(&s),
            other => panic!(
                "BoundingBox(edge[{i}]) must return Ok(Value::String(_)); got {other:?}"
            ),
        };
        let span = [bb[3] - bb[0], bb[4] - bb[1], bb[5] - bb[2]];
        for (axis, &sp) in span.iter().enumerate() {
            assert!(
                sp >= -1e-9,
                "BoundingBox(edge[{i}]) min must be ≤ max on axis {axis}; got span {sp}",
            );
        }
        let spanning = span.iter().filter(|&&s| (s - 1.0).abs() < 1e-6).count();
        let degenerate = span.iter().filter(|&&s| s.abs() < 1e-6).count();

        if spanning == 1 && degenerate == 2 {
            // Axis-aligned unit edge: tangent axis-aligned and pointing along
            // the single spanned axis.
            axis_aligned += 1;
            assert_unit_axis_aligned(t, &format!("EdgeTangent(edge[{i}])"));
            let span_axis = span
                .iter()
                .position(|&s| (s - 1.0).abs() < 1e-6)
                .expect("a spanning axis exists");
            assert!(
                (t[span_axis].abs() - 1.0).abs() < 1e-6,
                "axis-aligned edge[{i}] tangent must point along its spanned axis \
                 {span_axis}; got {t:?}",
            );
        } else if spanning == 2 && degenerate == 1 {
            // Face diagonal (length √2): spans two axes by 1.0.
            diagonals += 1;
        } else {
            panic!(
                "edge[{i}] bbox span {span:?} is neither an axis-aligned unit edge \
                 (1 spanning, 2 degenerate) nor a face diagonal (2 spanning, 1 degenerate)",
            );
        }
    }

    assert_eq!(
        axis_aligned, 12,
        "unit cube must have 12 axis-aligned unit edges; got {axis_aligned}",
    );
    assert_eq!(
        diagonals, 6,
        "unit cube must have 6 face-diagonal edges; got {diagonals}",
    );
}

// ---------------------------------------------------------------------------
// Topology adjacency: AdjacentFaces (steps 9 + 10), SharedEdges (steps 11 + 12)
// ---------------------------------------------------------------------------

/// The indices an `AdjacentFaces` / `SharedEdges` query replies with,
/// panicking unless the reply is OCCT's `Value::List<Value::Int>` wire format
/// (`crates/reify-kernel-occt/src/lib.rs`).
fn query_index_list(kernel: &ManifoldKernel, q: &GeometryQuery) -> Vec<usize> {
    match kernel.query(q) {
        Ok(value) => query_wire::index_list(&value),
        Err(e) => panic!("query({q:?}) must succeed; got {e:?}"),
    }
}

/// Face indices `AdjacentFaces { shape, face_index }` reports, checked against
/// the wire contract every caller relies on (OCCT's `std::set` reply shape):
/// ascending, distinct, the queried face excluded, each a position in
/// `0..face_count` — i.e. an index into `extract_faces(shape)`.
fn adjacent_face_indices(
    kernel: &ManifoldKernel,
    shape: GeometryHandleId,
    face_index: usize,
    face_count: usize,
) -> Vec<usize> {
    let neighbours = query_index_list(kernel, &GeometryQuery::AdjacentFaces { shape, face_index });
    assert!(
        neighbours.windows(2).all(|w| w[0] < w[1]),
        "AdjacentFaces({face_index}) must be ascending and distinct; got {neighbours:?}",
    );
    assert!(
        !neighbours.contains(&face_index),
        "AdjacentFaces({face_index}) must exclude the queried face; got {neighbours:?}",
    );
    assert!(
        neighbours.iter().all(|&n| n < face_count),
        "AdjacentFaces({face_index}) must index extract_faces (0..{face_count}); \
         got {neighbours:?}",
    );
    neighbours
}

/// Edge indices `SharedEdges { shape, face_a, face_b }` reports, as positions
/// into `extract_edges(shape)`.
fn shared_edge_indices(
    kernel: &ManifoldKernel,
    shape: GeometryHandleId,
    face_a: usize,
    face_b: usize,
) -> Vec<usize> {
    query_index_list(
        kernel,
        &GeometryQuery::SharedEdges {
            shape,
            face_a,
            face_b,
        },
    )
}

/// Assert `query` is rejected with `QueryError::QueryFailed`.
fn assert_query_failed(kernel: &ManifoldKernel, query: &GeometryQuery) {
    match kernel.query(query) {
        Err(QueryError::QueryFailed(_)) => {}
        other => panic!("query({query:?}) must fail with QueryFailed; got {other:?}"),
    }
}

/// `FaceNormal` of every face handle, in `faces` order.
fn face_normals(kernel: &ManifoldKernel, faces: &[GeometryHandleId]) -> Vec<[f64; 3]> {
    faces
        .iter()
        .map(|&f| match kernel.query(&GeometryQuery::FaceNormal(f)) {
            Ok(Value::String(s)) => parse_xyz(&s),
            other => panic!("FaceNormal({f:?}) must return Ok(Value::String(_)); got {other:?}"),
        })
        .collect()
}

/// Per-axis extent `[dx, dy, dz]` of an edge handle's `BoundingBox`.
fn edge_span(kernel: &ManifoldKernel, edge: GeometryHandleId) -> [f64; 3] {
    let bb = match kernel.query(&GeometryQuery::BoundingBox(edge)) {
        Ok(Value::String(s)) => parse_bbox(&s),
        other => panic!("BoundingBox({edge:?}) must return Ok(Value::String(_)); got {other:?}"),
    };
    [bb[3] - bb[0], bb[4] - bb[1], bb[5] - bb[2]]
}

/// The axis an axis-aligned face normal points along.
fn normal_axis(n: [f64; 3]) -> usize {
    n.iter()
        .position(|c| (c.abs() - 1.0).abs() < AXIS_TOL)
        .unwrap_or_else(|| panic!("face normal {n:?} must be axis-aligned"))
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Every coordinate in these fixtures is an exact dyadic value (0, 0.5, 1,
/// 1.5) in both f32 and f64, so normals, spans and dot products are exact;
/// this is a representation margin, not a tuned bound.
const AXIS_TOL: f64 = 1e-9;

/// `AdjacentFaces` answers in `extract_faces`' coalesced planar-face space:
/// each of the unit cube's 6 faces touches exactly the 4 perpendicular faces
/// and not the opposite one, matching OCCT's BRep cube.
///
/// Index 6 is past the 6-face space, so it must be rejected — this also
/// discriminates the face space from the 12-triangle mesh space.
#[test]
fn adjacent_faces_unit_cube_four_neighbours_in_face_space() {
    let mut kernel = ManifoldKernel::new();
    let shape = ingest(&mut kernel, [0.0, 0.0, 0.0]);
    let faces = kernel
        .extract_faces(shape)
        .expect("extract_faces must succeed");
    assert_eq!(
        faces.len(),
        6,
        "unit cube must have 6 coalesced planar faces"
    );
    let normals = face_normals(&kernel, &faces);

    for face_index in 0..faces.len() {
        let neighbours = adjacent_face_indices(&kernel, shape, face_index, faces.len());
        assert_eq!(
            neighbours.len(),
            4,
            "cube face {face_index} must have exactly 4 adjacent faces; got {neighbours:?}",
        );
        for &j in &neighbours {
            assert!(
                dot(normals[face_index], normals[j]).abs() < AXIS_TOL,
                "neighbour {j} of face {face_index} must be perpendicular to it; \
                 normals {:?} vs {:?}",
                normals[face_index],
                normals[j],
            );
        }
        let rest: Vec<usize> = (0..faces.len())
            .filter(|&j| j != face_index && !neighbours.contains(&j))
            .collect();
        assert_eq!(
            rest.len(),
            1,
            "exactly one cube face is not adjacent to face {face_index}"
        );
        assert!(
            (dot(normals[face_index], normals[rest[0]]) + 1.0).abs() < AXIS_TOL,
            "the non-adjacent face {} must be opposite face {face_index}",
            rest[0],
        );
    }

    assert_query_failed(
        &kernel,
        &GeometryQuery::AdjacentFaces {
            shape,
            face_index: faces.len(),
        },
    );
}

/// `SharedEdges` answers in `extract_faces` face space with `extract_edges`
/// edge positions: two adjacent cube faces share exactly their common cube
/// edge — never one of the 6 face diagonals, which are interior triangulation
/// edges of a single coalesced face.
#[test]
fn shared_edges_unit_cube_one_boundary_edge_per_adjacent_pair() {
    let mut kernel = ManifoldKernel::new();
    let shape = ingest(&mut kernel, [0.0, 0.0, 0.0]);
    let faces = kernel
        .extract_faces(shape)
        .expect("extract_faces must succeed");
    let edges = kernel
        .extract_edges(shape)
        .expect("extract_edges must succeed");
    assert_eq!(edges.len(), 18, "unit cube must have 18 canonical edges");

    let neighbours = adjacent_face_indices(&kernel, shape, 0, faces.len());
    assert_eq!(
        neighbours.len(),
        4,
        "cube face 0 must have 4 adjacent faces"
    );
    let mut shared_ids = Vec::new();
    for &nb in &neighbours {
        let shared = shared_edge_indices(&kernel, shape, 0, nb);
        assert_eq!(
            shared.len(),
            1,
            "adjacent cube faces 0 and {nb} must share exactly one edge; got {shared:?}",
        );
        let e = shared[0];
        assert!(
            e < edges.len(),
            "shared edge {e} must index extract_edges (0..{})",
            edges.len()
        );
        let span = edge_span(&kernel, edges[e]);
        let spanning = span.iter().filter(|s| (*s - 1.0).abs() < AXIS_TOL).count();
        let degenerate = span.iter().filter(|s| s.abs() < AXIS_TOL).count();
        assert_eq!(
            (spanning, degenerate),
            (1, 2),
            "edge {e} shared by faces 0 and {nb} must be a cube edge (one unit axis), \
             not a face diagonal; bbox span {span:?}",
        );
        shared_ids.push(e);
    }
    shared_ids.sort_unstable();
    shared_ids.dedup();
    assert_eq!(
        shared_ids.len(),
        4,
        "face 0's four shared edges must be distinct"
    );

    assert!(
        shared_edge_indices(&kernel, shape, 3, 3).is_empty(),
        "SharedEdges(f, f) must be empty",
    );
    let opposite = (1..faces.len())
        .find(|j| !neighbours.contains(j))
        .expect("the face opposite face 0 exists");
    assert!(
        shared_edge_indices(&kernel, shape, 0, opposite).is_empty(),
        "opposite faces 0 and {opposite} must share no edge",
    );
    assert_query_failed(
        &kernel,
        &GeometryQuery::SharedEdges {
            shape,
            face_a: 0,
            face_b: faces.len(),
        },
    );
}

/// Adjacency on a boolean result stays in the coalesced face space even when
/// the mesh has far more triangles than faces.
///
/// `[0,1]³ ∪ [0.5,1.5]×[0,1]²` is the box `[0,1.5]×[0,1]²`: 6 coalesced faces
/// over a ~28-triangle mesh whose x-direction box edges are split into several
/// canonical segments at x = 0.5 and x = 1. So `SharedEdges` of two adjacent
/// faces is the set of segments on their common line, which together span the
/// box along the free axis — the count is deliberately not pinned.
#[test]
fn adjacency_on_boolean_union_box_answers_in_coalesced_face_space() {
    const BOX_EXTENT: [f64; 3] = [1.5, 1.0, 1.0];
    let mut kernel = ManifoldKernel::new();
    let left = ingest(&mut kernel, [0.0, 0.0, 0.0]);
    let right = ingest(&mut kernel, [0.5, 0.0, 0.0]);
    let body = kernel
        .execute(&GeometryOp::Union { left, right })
        .expect("union of two overlapping cubes must succeed")
        .id;
    let faces = kernel
        .extract_faces(body)
        .expect("extract_faces must succeed");
    assert_eq!(
        faces.len(),
        6,
        "the union box must have 6 coalesced planar faces"
    );
    let edges = kernel
        .extract_edges(body)
        .expect("extract_edges must succeed");
    let normals = face_normals(&kernel, &faces);

    for i in 0..faces.len() {
        let neighbours = adjacent_face_indices(&kernel, body, i, faces.len());
        assert_eq!(
            neighbours.len(),
            4,
            "union-box face {i} must have exactly 4 adjacent faces; got {neighbours:?}",
        );
        for &j in &neighbours {
            assert!(
                dot(normals[i], normals[j]).abs() < AXIS_TOL,
                "neighbour {j} of union-box face {i} must be perpendicular to it",
            );
            let (axis_i, axis_j) = (normal_axis(normals[i]), normal_axis(normals[j]));
            let free_axis = 3 - axis_i - axis_j;
            let shared = shared_edge_indices(&kernel, body, i, j);
            assert!(
                !shared.is_empty(),
                "adjacent union-box faces {i},{j} must share edges"
            );
            let mut covered = 0.0;
            for &e in &shared {
                assert!(e < edges.len(), "shared edge {e} must index extract_edges");
                let span = edge_span(&kernel, edges[e]);
                assert!(
                    span[axis_i].abs() < AXIS_TOL && span[axis_j].abs() < AXIS_TOL,
                    "edge {e} shared by faces {i},{j} must lie on their common line; \
                     bbox span {span:?}",
                );
                covered += span[free_axis];
            }
            assert!(
                (covered - BOX_EXTENT[free_axis]).abs() < AXIS_TOL,
                "edges shared by faces {i},{j} must cover the box along axis {free_axis} \
                 ({}); got {covered} from {shared:?}",
                BOX_EXTENT[free_axis],
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Mass properties via signed-tetrahedron mesh integration:
// CenterOfMass (steps 13 + 14), InertiaTensor (steps 15 + 16)
// ---------------------------------------------------------------------------

/// Query `CenterOfMass{handle, density}` and return the parsed `[x, y, z]`,
/// panicking on any non-`Ok(Value::String)` reply.
fn query_center_of_mass(
    kernel: &ManifoldKernel,
    handle: GeometryHandleId,
    density: f64,
) -> [f64; 3] {
    match kernel.query(&GeometryQuery::CenterOfMass { handle, density }) {
        Ok(Value::String(s)) => parse_xyz(&s),
        other => panic!(
            "CenterOfMass{{handle={handle:?}, density={density}}} must return \
             Ok(Value::String(_)); got {other:?}"
        ),
    }
}

/// `CenterOfMass` over the unit cube via signed-tetrahedron mesh integration.
///
/// `unit_cube_mesh([0,0,0])` spans `[0,1]³`, so its true volume centroid is
/// `(0.5, 0.5, 0.5)` — offset from the origin, which proves the arm performs
/// real integration rather than returning an origin default. Polyhedral
/// integration is exact on the cube's exactly-representable `{0,1}` vertices,
/// so the 1e-9 tolerance is derived, not tuned. Density is ignored
/// (uniform-density centroid == geometric centroid, matching OCCT), pinned by
/// querying ρ=1 vs ρ=100.
#[test]
fn query_center_of_mass_unit_cube() {
    let mut kernel = ManifoldKernel::new();
    let handle = ingest(&mut kernel, [0.0, 0.0, 0.0]);

    // (a) Volume centroid is the cube's geometric centre, (0.5, 0.5, 0.5).
    let c = query_center_of_mass(&kernel, handle, 1.0);
    let expected = [0.5, 0.5, 0.5];
    for axis in 0..3 {
        assert!(
            (c[axis] - expected[axis]).abs() < 1e-9,
            "CenterOfMass axis {axis} must be {} within 1e-9; got {c:?}",
            expected[axis],
        );
    }

    // (b) Density-invariance: ρ=1 and ρ=100 yield *identical* coordinates
    //     (the arm never reads density), so exact equality must hold.
    let c100 = query_center_of_mass(&kernel, handle, 100.0);
    assert_eq!(
        c, c100,
        "CenterOfMass must ignore density (uniform-density centroid); \
         ρ=1 gave {c:?}, ρ=100 gave {c100:?}",
    );
}

/// Query `InertiaTensor{handle, density}` and decode the `Value::List` of 3 row
/// `Value::List`s of 3 `Value::Real` into a row-major `[[f64;3];3]`, panicking
/// on any other shape (the wire-format contract is itself under test).
fn query_inertia_tensor(
    kernel: &ManifoldKernel,
    handle: GeometryHandleId,
    density: f64,
) -> [[f64; 3]; 3] {
    let rows = match kernel.query(&GeometryQuery::InertiaTensor { handle, density }) {
        Ok(Value::List(rows)) => rows,
        other => panic!(
            "InertiaTensor{{handle={handle:?}, density={density}}} must return \
             Ok(Value::List(_)); got {other:?}"
        ),
    };
    assert_eq!(rows.len(), 3, "inertia tensor must have 3 rows; got {}", rows.len());
    let mut m = [[0.0f64; 3]; 3];
    for (i, row) in rows.iter().enumerate() {
        let cols = match row {
            Value::List(cols) => cols,
            other => panic!("inertia row {i} must be Value::List; got {other:?}"),
        };
        assert_eq!(cols.len(), 3, "inertia row {i} must have 3 columns; got {}", cols.len());
        for (j, val) in cols.iter().enumerate() {
            m[i][j] = match val {
                Value::Real(r) => *r,
                other => panic!("inertia[{i}][{j}] must be Value::Real; got {other:?}"),
            };
        }
    }
    m
}

/// `InertiaTensor` over the unit cube via signed-tetrahedron mesh integration,
/// density-scaled and centroidal.
///
/// For a `1×1×1` cube of uniform density ρ (mass `m = ρ·V = ρ`, side `s = 1`),
/// the centroidal inertia about each axis is `I = m(s² + s²)/12 = ρ/6`, and the
/// products of inertia vanish (axis-aligned). Polyhedral integration is exact
/// on the cube's `{0,1}` vertices, so the 1e-9 tolerances are derived. Pins:
/// (a) diagonal == ρ/6; (b) off-diagonal ≈ 0; (c) symmetry; (d) linear density
/// scaling (ρ=1 is exactly half the ρ=2 tensor — density is a pure multiplier).
// 3×3 matrix assertions read most clearly with explicit (i, j) indices —
// especially the transpose access `t[j][i]` in the symmetry check, which an
// `.iter().enumerate()` rewrite cannot express cleanly. The sibling OCCT
// mass-property tests take the same exemption
// (crates/reify-kernel-occt/src/lib.rs:3279).
#[allow(clippy::needless_range_loop)]
#[test]
fn query_inertia_tensor_unit_cube_density_scaled() {
    let mut kernel = ManifoldKernel::new();
    let handle = ingest(&mut kernel, [0.0, 0.0, 0.0]);

    let rho = 2.0;
    let t = query_inertia_tensor(&kernel, handle, rho);

    // (a) Diagonal == ρ/6 (centroidal inertia of a unit cube).
    let expected_diag = rho / 6.0;
    for k in 0..3 {
        assert!(
            (t[k][k] - expected_diag).abs() < 1e-9,
            "inertia diagonal [{k}][{k}] must be ρ/6 = {expected_diag} within 1e-9; got {}",
            t[k][k],
        );
    }
    // (b) Off-diagonal ≈ 0 (axis-aligned cube ⇒ zero products of inertia).
    for i in 0..3 {
        for j in 0..3 {
            if i != j {
                assert!(
                    t[i][j].abs() < 1e-9,
                    "inertia off-diagonal [{i}][{j}] must be ≈ 0 within 1e-9; got {}",
                    t[i][j],
                );
            }
        }
    }
    // (c) Symmetric tensor.
    for i in 0..3 {
        for j in 0..3 {
            assert!(
                (t[i][j] - t[j][i]).abs() < 1e-12,
                "inertia tensor must be symmetric: [{i}][{j}]={} != [{j}][{i}]={}",
                t[i][j],
                t[j][i],
            );
        }
    }
    // (d) Linear density scaling: 2·I(ρ=1) == I(ρ=2), entrywise (density is a
    //     pure scalar multiplier over identical geometric integrals).
    let t1 = query_inertia_tensor(&kernel, handle, 1.0);
    for i in 0..3 {
        for j in 0..3 {
            assert!(
                (t1[i][j] * 2.0 - t[i][j]).abs() < 1e-12,
                "density scaling must be linear: 2·I(ρ=1)[{i}][{j}]={} != I(ρ=2)[{i}][{j}]={}",
                t1[i][j] * 2.0,
                t[i][j],
            );
        }
    }
}

/// `InertiaTensor` on an **asymmetric L-tromino** — pins the off-diagonal sign
/// convention the symmetric unit cube cannot reach.
///
/// `query_inertia_tensor_unit_cube_density_scaled` exercises an axis-aligned
/// cube, whose products of inertia are all *exactly zero*. That case cannot
/// distinguish OCCT's `Iᵢⱼ = −∫xᵢxⱼ dV` convention (which the implementation
/// hard-codes via `-pc[i][j]`) from the opposite sign, and cannot catch a
/// transposed / mis-slotted off-diagonal. This test uses an L-tromino — three
/// unit cubes unioned into an L in the xy-plane, extruded 1 unit in z — which
/// has a single non-zero, closed-form product of inertia, so both the sign and
/// the placement are pinned.
///
/// # Closed-form expected tensor (per unit density)
///
/// The L-tromino is the three unit cells `[0,1]³`, `[1,2]×[0,1]×[0,1]`,
/// `[0,1]×[1,2]×[0,1]` (volume `V = 3`), centroid `(5/6, 5/6, 1/2)`. Each cell's
/// own cross moment about its centre is zero (it is a box), so the product of
/// inertia is just `Σ dxᵢ·dyᵢ` over the cell-centroid offsets:
///
/// * `∫(x−cx)(y−cy) dV = 1/9 − 2/9 − 2/9 = −1/3`
/// * `∫(x−cx)(z−cz) dV = ∫(y−cy)(z−cz) dV = 0` (every cell has `zᵢ = cz`)
/// * `∫(x−cx)² dV = ∫(y−cy)² dV = 11/12`, `∫(z−cz)² dV = 1/4`
///
/// In OCCT's convention `Iₖₖ = ∫(x_l² + x_m²) dV`, `Iᵢⱼ = −∫xᵢxⱼ dV`:
///
/// ```text
///        ⎡  7/6   1/3    0  ⎤
///   I =  ⎢  1/3   7/6    0  ⎥   (× ρ)
///        ⎣   0     0   11/6 ⎦
/// ```
///
/// The off-diagonal `Iₓᵧ = −(−1/3) = +1/3` is the discriminating value: the
/// L's mass sits where `(x−cx)` and `(y−cy)` have *opposite* signs (the missing
/// fourth cell is the high-x/high-y one), so `∫xy dV < 0` and the OCCT-convention
/// product of inertia is **positive**. The opposite sign convention would yield
/// `−1/3`, and a transposed off-diagonal would move the non-zero term into
/// `Iₓz`/`Iᵧz`. Cross-kernel byte-parity against OCCT for the same solid is
/// KGQ-ρ's integration gate; here the values are pinned analytically.
///
/// The L is built by `Union`-ing face-sharing cubes, so its mesh may carry
/// Manifold's merge-tolerance-level coordinate noise (unlike the directly
/// ingested cube) — hence a 1e-6 tolerance rather than 1e-9, still four orders
/// of magnitude tighter than the ±1/3 sign gap under test.
// 3×3 matrix assertions read most clearly with explicit (i, j) indices — see
// the sibling cube test's note (and crates/reify-kernel-occt/src/lib.rs:3279).
#[allow(clippy::needless_range_loop)]
#[test]
fn query_inertia_tensor_l_tromino_pins_offdiagonal_sign() {
    let mut kernel = ManifoldKernel::new();
    let c1 = ingest(&mut kernel, [0.0, 0.0, 0.0]);
    let c2 = ingest(&mut kernel, [1.0, 0.0, 0.0]); // shares face x=1 with c1
    let c3 = ingest(&mut kernel, [0.0, 1.0, 0.0]); // shares face y=1 with c1

    // L = c1 ∪ c2 ∪ c3 — an L-tromino (3 unit cells, asymmetric in x/y).
    let bar = kernel
        .execute(&GeometryOp::Union {
            left: c1,
            right: c2,
        })
        .expect("union of two face-sharing cubes must succeed")
        .id;
    let ell = kernel
        .execute(&GeometryOp::Union {
            left: bar,
            right: c3,
        })
        .expect("union of the bar with the third cube must succeed")
        .id;

    let rho = 2.0;
    let t = query_inertia_tensor(&kernel, ell, rho);

    // Closed-form per-unit-density tensor (see doc comment), scaled by ρ.
    let expected = [
        [7.0 / 6.0 * rho, 1.0 / 3.0 * rho, 0.0],
        [1.0 / 3.0 * rho, 7.0 / 6.0 * rho, 0.0],
        [0.0, 0.0, 11.0 / 6.0 * rho],
    ];
    const TOL: f64 = 1e-6;
    for i in 0..3 {
        for j in 0..3 {
            assert!(
                (t[i][j] - expected[i][j]).abs() < TOL,
                "L-tromino inertia[{i}][{j}] must be {} within {TOL}; got {}",
                expected[i][j],
                t[i][j],
            );
        }
    }

    // The discriminating assertion, called out explicitly: the xy product of
    // inertia is *positive* (ρ/3) under OCCT's Iᵢⱼ = −∫xᵢxⱼ dV convention. A
    // positive value pins the sign; the opposite convention would give −ρ/3.
    assert!(
        t[0][1] > 0.0,
        "L-tromino product of inertia Iₓᵧ must be POSITIVE under the −∫xy dV \
         convention; the sign is the contract under test (got {})",
        t[0][1],
    );

    // Symmetry (independent of the closed-form magnitudes).
    for i in 0..3 {
        for j in 0..3 {
            assert!(
                (t[i][j] - t[j][i]).abs() < 1e-12,
                "L-tromino inertia tensor must be symmetric: [{i}][{j}]={} != [{j}][{i}]={}",
                t[i][j],
                t[j][i],
            );
        }
    }
}
