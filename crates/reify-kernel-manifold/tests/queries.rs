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

/// Sub-face `SurfaceArea` and `FaceNormal` over the unit cube's 12 facets.
///
/// Each `unit_cube_mesh` facet is a right triangle with legs 1 and 1, so its
/// area is `1/2·1·1 = 0.5` and the 12 facets sum to the cube's total surface
/// area `6.0`. Every facet of an axis-aligned cube has an axis-aligned unit
/// normal (both triangles of a cube face share that face's normal), so the
/// FaceNormal check is triangle-order-independent; we additionally confirm a
/// ±Z facet exists to exercise the Z axis explicitly (plan step-5(c)). Sign
/// is accepted either way per the FaceNormal contract.
///
/// RED (step-5): `ManifoldKernel::query` returns `Err(QueryFailed(STUB_MSG))`
/// for `SurfaceArea`/`FaceNormal`. GREEN is step-6.
#[test]
fn query_sub_face_surface_area_and_normal_unit_cube() {
    let mut kernel = ManifoldKernel::new();
    let handle = ingest(&mut kernel, [0.0, 0.0, 0.0]);
    let faces = kernel
        .extract_faces(handle)
        .expect("extract_faces must succeed");
    assert_eq!(faces.len(), 12, "unit cube must have 12 facets");

    let mut area_sum = 0.0;
    let mut saw_z_facet = false;
    for (i, &f) in faces.iter().enumerate() {
        // (a) per-facet area == 0.5.
        let area = match kernel.query(&GeometryQuery::SurfaceArea(f)) {
            Ok(Value::Real(a)) => a,
            other => panic!(
                "SurfaceArea(face[{i}]) must return Ok(Value::Real(_)); got {other:?}"
            ),
        };
        assert!(
            (area - 0.5).abs() < 1e-6,
            "unit-cube facet [{i}] is a right triangle (legs 1,1) => area 0.5; got {area}",
        );
        area_sum += area;

        // (c) per-facet normal is a unit, axis-aligned vector.
        let n = match kernel.query(&GeometryQuery::FaceNormal(f)) {
            Ok(Value::String(s)) => parse_xyz(&s),
            other => panic!(
                "FaceNormal(face[{i}]) must return Ok(Value::String(_)); got {other:?}"
            ),
        };
        assert_unit_axis_aligned(n, &format!("FaceNormal(face[{i}])"));
        if (n[2].abs() - 1.0).abs() < 1e-6 {
            saw_z_facet = true;
        }
    }

    // (b) sum of all 12 facet areas == total cube surface area 6.0.
    assert!(
        (area_sum - 6.0).abs() < 1e-6,
        "sum of 12 unit-cube facet areas must be 6.0; got {area_sum}",
    );
    assert!(
        saw_z_facet,
        "unit cube must have at least one facet whose normal is ≈ ±Z",
    );
}

/// Parse the OCCT-compatible `{"xmin":_,...,"zmax":_}` BoundingBox JSON wire
/// format into `[xmin, ymin, zmin, xmax, ymax, zmax]`.
fn parse_bbox(s: &str) -> [f64; 6] {
    let mut out = [f64::NAN; 6];
    let trimmed = s.trim().trim_start_matches('{').trim_end_matches('}');
    for field in trimmed.split(',') {
        let (key, val) = field
            .split_once(':')
            .unwrap_or_else(|| panic!("malformed bbox field {field:?} in {s:?}"));
        let key = key.trim().trim_matches('"');
        let val: f64 = val
            .trim()
            .parse()
            .unwrap_or_else(|_| panic!("non-numeric value in field {field:?} of {s:?}"));
        let idx = match key {
            "xmin" => 0,
            "ymin" => 1,
            "zmin" => 2,
            "xmax" => 3,
            "ymax" => 4,
            "zmax" => 5,
            other => panic!("unexpected key {other:?} in bbox JSON {s:?}"),
        };
        out[idx] = val;
    }
    for (i, c) in out.iter().enumerate() {
        assert!(!c.is_nan(), "bbox JSON {s:?} missing component {i}");
    }
    out
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
///
/// RED (step-7): `ManifoldKernel::query` returns `Err(QueryFailed(STUB_MSG))`
/// for `EdgeTangent`/`BoundingBox`. GREEN is step-8.
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

/// Decode an `Ok(Value::List)` of `Value::Int` into `Vec<i64>`, panicking with
/// a descriptive message on any other shape. Shared by the `AdjacentFaces` and
/// `SharedEdges` tests (both mirror OCCT's `Value::List<Value::Int>` wire
/// format: `crates/reify-kernel-occt/src/lib.rs`).
fn query_int_list(kernel: &ManifoldKernel, q: &GeometryQuery) -> Vec<i64> {
    match kernel.query(q) {
        Ok(Value::List(items)) => items
            .iter()
            .map(|v| match v {
                Value::Int(i) => *i,
                other => panic!("AdjacentFaces/SharedEdges list must hold Value::Int; got {other:?}"),
            })
            .collect(),
        other => panic!("query({q:?}) must return Ok(Value::List(_)); got {other:?}"),
    }
}

/// `AdjacentFaces` over the unit cube: every mesh triangle has exactly 3
/// edge-adjacent neighbours.
///
/// Closed-2-manifold invariant: the cube mesh has 12 triangles and 18 edges,
/// each edge shared by exactly 2 triangles (`12·3 = 36 = 18·2`). Two distinct
/// triangles cannot share two edges without being identical, so each triangle's
/// 3 edges reach 3 *distinct* neighbours — exactly 3, for **every** triangle
/// index (the assertion is therefore triangle-order-independent). The query
/// excludes the triangle itself and returns the neighbour indices as an
/// ascending `Value::List<Value::Int>`, mirroring OCCT's wire format.
///
/// RED (step-9): `query()` returns `Err(QueryFailed(STUB_MSG))` for
/// `AdjacentFaces`. GREEN is step-10.
#[test]
fn query_adjacent_faces_unit_cube_exactly_three_neighbours() {
    let mut kernel = ManifoldKernel::new();
    let shape = ingest(&mut kernel, [0.0, 0.0, 0.0]);
    // Pin the 12-triangle face count the `0..12` range below rests on.
    let faces = kernel
        .extract_faces(shape)
        .expect("extract_faces must succeed");
    assert_eq!(faces.len(), 12, "unit cube must have 12 mesh triangles");

    // A spread of triangle indices — the "exactly 3" invariant holds for all.
    for &face_index in &[0usize, 1, 6, 11] {
        let neighbours =
            query_int_list(&kernel, &GeometryQuery::AdjacentFaces { shape, face_index });
        assert_eq!(
            neighbours.len(),
            3,
            "triangle {face_index} must have exactly 3 edge-adjacent neighbours \
             (closed-2-manifold); got {neighbours:?}",
        );
        // Self excluded.
        assert!(
            !neighbours.contains(&(face_index as i64)),
            "AdjacentFaces({face_index}) must exclude the queried triangle; got {neighbours:?}",
        );
        // All entries are valid triangle indices in 0..12.
        for &n in &neighbours {
            assert!(
                (0..12).contains(&n),
                "neighbour {n} of triangle {face_index} out of range 0..12",
            );
        }
        // Ascending and distinct.
        let mut sorted = neighbours.clone();
        sorted.sort_unstable();
        assert_eq!(
            neighbours, sorted,
            "AdjacentFaces({face_index}) must be ascending; got {neighbours:?}",
        );
        let mut deduped = sorted.clone();
        deduped.dedup();
        assert_eq!(
            deduped.len(),
            neighbours.len(),
            "AdjacentFaces({face_index}) must have distinct entries; got {neighbours:?}",
        );
    }
}

/// `SharedEdges` over the unit cube, cross-validated against `AdjacentFaces`.
///
/// Two distinct triangles cannot share two undirected edges without coinciding,
/// so every *edge-adjacent* pair shares **exactly one** canonical edge, and
/// every *non-adjacent* pair shares **none**. Driving the pairs off
/// `AdjacentFaces(0)` keeps the test independent of the fixture's triangle
/// winding/order while pinning the SharedEdges contract:
/// - (a) each of triangle 0's 3 neighbours shares exactly one edge, a valid
///   index in `0..18` (the canonical `extract_edges` enumeration), and the
///   three shared indices are triangle 0's own 3 distinct edges;
/// - (b) `face_a == face_b` returns an empty list (design decision);
/// - (c) a non-adjacent triangle shares no edge (empty list).
///
/// RED (step-11): `query()` returns `Err(QueryFailed(STUB_MSG))` for
/// `SharedEdges`. GREEN is step-12.
#[test]
fn query_shared_edges_unit_cube() {
    let mut kernel = ManifoldKernel::new();
    let shape = ingest(&mut kernel, [0.0, 0.0, 0.0]);
    // 12 triangles; the SharedEdges index space is the 18 canonical edges.
    let faces = kernel
        .extract_faces(shape)
        .expect("extract_faces must succeed");
    assert_eq!(faces.len(), 12, "unit cube must have 12 mesh triangles");

    // (a) Every edge-adjacent pair shares exactly one canonical edge.
    let neighbours =
        query_int_list(&kernel, &GeometryQuery::AdjacentFaces { shape, face_index: 0 });
    assert_eq!(neighbours.len(), 3, "triangle 0 must have 3 neighbours");
    let mut shared_ids = Vec::new();
    for &nb in &neighbours {
        let shared = query_int_list(
            &kernel,
            &GeometryQuery::SharedEdges {
                shape,
                face_a: 0,
                face_b: nb as usize,
            },
        );
        assert_eq!(
            shared.len(),
            1,
            "edge-adjacent triangles 0 and {nb} must share exactly one edge; got {shared:?}",
        );
        let e = shared[0];
        assert!(
            (0..18).contains(&e),
            "shared edge index {e} (triangles 0,{nb}) out of range 0..18",
        );
        shared_ids.push(e);
    }
    // Triangle 0's three shared edges are its own three *distinct* edges.
    shared_ids.sort_unstable();
    let before = shared_ids.len();
    shared_ids.dedup();
    assert_eq!(
        shared_ids.len(),
        before,
        "triangle 0's three shared edges must be distinct; got {shared_ids:?}",
    );

    // (b) face_a == face_b => empty list (design decision).
    let self_shared = query_int_list(
        &kernel,
        &GeometryQuery::SharedEdges {
            shape,
            face_a: 3,
            face_b: 3,
        },
    );
    assert!(
        self_shared.is_empty(),
        "SharedEdges(f, f) must be empty; got {self_shared:?}",
    );

    // (c) A non-adjacent triangle (neither 0 nor a neighbour of 0) shares no
    //     edge — 3 neighbours < 11 others, so one always exists.
    let non_adjacent = (1..12i64)
        .find(|i| !neighbours.contains(i))
        .expect("a non-adjacent triangle must exist");
    let none_shared = query_int_list(
        &kernel,
        &GeometryQuery::SharedEdges {
            shape,
            face_a: 0,
            face_b: non_adjacent as usize,
        },
    );
    assert!(
        none_shared.is_empty(),
        "non-adjacent triangles 0 and {non_adjacent} must share no edge; got {none_shared:?}",
    );
}
