//! Integration tests for `OcctKernel::surface_normal_at` and `OcctKernel::curvature_at`.
//!
//! These primitives expose local differential properties of a face at a
//! (u, v) parameter point:
//!
//! - `surface_normal_at(face, u, v) -> [f64; 3]` — outward unit normal.
//! - `curvature_at(face, u, v) -> Curvature` — Gaussian + mean + principal curvatures.
//!
//! Analytic ground truth:
//! - **Sphere r=5**: K = 1/25, H = 1/5, κ_min = κ_max = 1/5 (umbilical).
//! - **Cylinder r=5**: curved side → K = 0, H = 1/10, κ_min = 0, κ_max = 1/5;
//!   flat caps → K = H = κ_min = κ_max = 0.

#![cfg(all(has_occt, feature = "test-fixtures"))]

use std::f64::consts::PI;

use reify_kernel_occt::OcctKernel;
use reify_ir::{GeometryHandleId, GeometryOp, GeometryQuery, QueryError, Value};

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

/// Build a kernel with a sphere of the given radius.
/// Returns `(kernel, sphere_handle_id)`.
fn sphere_kernel(radius: f64) -> (OcctKernel, GeometryHandleId) {
    let mut kernel = OcctKernel::new();
    let handle = kernel
        .execute(&GeometryOp::Sphere {
            radius: Value::Real(radius),
        })
        .expect("sphere creation should succeed");
    (kernel, handle.id)
}

/// Build a kernel with a cylinder of the given radius and height.
/// Returns `(kernel, cylinder_handle_id)`.
fn cylinder_kernel(radius: f64, height: f64) -> (OcctKernel, GeometryHandleId) {
    let mut kernel = OcctKernel::new();
    let handle = kernel
        .execute(&GeometryOp::Cylinder {
            radius: Value::Real(radius),
            height: Value::Real(height),
        })
        .expect("cylinder creation should succeed");
    (kernel, handle.id)
}

// ---------------------------------------------------------------------------
// surface_normal_at — happy-path: sphere
// ---------------------------------------------------------------------------

/// `surface_normal_at` on a sphere face at (u=π, v=0) returns the outward
/// unit radial normal.
///
/// At (u=π, v=0) in OCCT's sphere parametrization the surface point is
/// (-r, 0, 0), so the outward normal is (-1, 0, 0). This point is safely
/// interior and away from the seam at u=0.
#[test]
fn surface_normal_at_on_sphere_face_yields_unit_outward_radial_normal() {
    let (mut kernel, sphere_id) = sphere_kernel(5.0);
    let faces = kernel
        .extract_faces(sphere_id)
        .expect("extract_faces should succeed for sphere");
    assert!(!faces.is_empty(), "sphere should have at least one face");
    let face = faces[0];

    // Query at (u=π, v=0): point on equator at −X direction.
    let n = kernel
        .surface_normal_at(face, PI, 0.0)
        .expect("surface_normal_at should succeed for sphere face");

    // (a) Unit length.
    let mag_sq = n[0] * n[0] + n[1] * n[1] + n[2] * n[2];
    assert!(
        (mag_sq - 1.0).abs() < 1e-9,
        "surface_normal_at should return a unit vector: |n|² = {mag_sq}"
    );

    // (b) At (u=π, v=0), the outward normal is (−1, 0, 0).
    assert!(
        (n[0] + 1.0).abs() < 1e-6 && n[1].abs() < 1e-6 && n[2].abs() < 1e-6,
        "surface_normal_at(sphere, π, 0) expected (−1, 0, 0), got {n:?}"
    );
}

// ---------------------------------------------------------------------------
// surface_normal_at — edge cases
// ---------------------------------------------------------------------------

/// `surface_normal_at` on the curved side face of a cylinder is radial and
/// perpendicular to the Z axis.
///
/// At (u=π/2, v=5) on a cylinder of radius 5 and height 10, the analytic
/// outward normal is (0, 1, 0) — radially outward in the +Y direction.
#[test]
fn surface_normal_at_on_cylinder_side_face_is_radial_perpendicular_to_z() {
    let (mut kernel, cyl_id) = cylinder_kernel(5.0, 10.0);
    let faces = kernel
        .extract_faces(cyl_id)
        .expect("extract_faces should succeed for cylinder");
    assert_eq!(faces.len(), 3, "cylinder should have 3 faces");

    // Identify the curved side face: it is the one whose centroid normal has |n_z| < 0.5.
    let side_face = faces
        .iter()
        .copied()
        .find(|&f| {
            kernel
                .face_outward_unit_normal_for_test(f)
                .expect("FaceNormal should succeed for cylinder face")[2]
                .abs()
                < 0.5
        })
        .expect("cylinder should have a curved side face with |n_z| < 0.5");

    // At (u=π/2, v=5): OCCT cylinder parametrization P(u,v) = (r·cos u, r·sin u, v).
    // So P(π/2, 5) = (0, 5, 5) on a r=5, h=10 cylinder. The outward normal is (0, 1, 0).
    let n = kernel
        .surface_normal_at(side_face, PI / 2.0, 5.0)
        .expect("surface_normal_at should succeed for cylinder side face");

    // Overall unit length.
    let mag_sq = n[0] * n[0] + n[1] * n[1] + n[2] * n[2];
    assert!(
        (mag_sq - 1.0).abs() < 1e-9,
        "surface_normal_at cylinder side: |n|² = {mag_sq}, expected 1.0"
    );

    // n_z ≈ 0 (perpendicular to the cylinder axis).
    assert!(
        n[2].abs() < 1e-9,
        "surface_normal_at cylinder side: n_z = {}, expected ≈ 0",
        n[2]
    );

    // x² + y² ≈ 1 (unit in the radial plane).
    let radial_sq = n[0] * n[0] + n[1] * n[1];
    assert!(
        (radial_sq - 1.0).abs() < 1e-9,
        "surface_normal_at cylinder side: radial |n|² = {radial_sq}, expected ≈ 1"
    );
}

/// Passing an unknown handle returns `QueryError::InvalidHandle`.
#[test]
fn surface_normal_at_unknown_handle_returns_invalid_handle() {
    let (kernel, _) = sphere_kernel(5.0);
    let unknown = GeometryHandleId(9999);
    match kernel.surface_normal_at(unknown, 0.0, 0.0) {
        Err(QueryError::InvalidHandle(id)) if id == unknown => {}
        Err(QueryError::InvalidHandle(id)) => {
            panic!("expected InvalidHandle({unknown:?}), got InvalidHandle({id:?})")
        }
        other => panic!("expected Err(InvalidHandle({unknown:?})), got {other:?}"),
    }
}

/// Passing a solid handle (not a face) returns `QueryError::QueryFailed` with
/// a message containing "not a face".
#[test]
fn surface_normal_at_non_face_shape_returns_query_failed_with_not_a_face() {
    let (kernel, cyl_id) = cylinder_kernel(5.0, 10.0);
    // cyl_id is the solid handle, not a face.
    match kernel.surface_normal_at(cyl_id, 0.0, 0.0) {
        Err(QueryError::QueryFailed(msg)) => {
            assert!(
                msg.contains("not a face"),
                "expected error containing 'not a face', got: {msg}"
            );
        }
        other => panic!(
            "expected Err(QueryFailed(\"...not a face...\")) for solid handle, got {other:?}"
        ),
    }
}

// ---------------------------------------------------------------------------
// face_outward_unit_normal_for_test — typed centroid normal
// ---------------------------------------------------------------------------

/// `face_outward_unit_normal_for_test` returns a unit outward normal as a
/// typed `[f64; 3]` and agrees numerically with `kernel.query(FaceNormal)`.
///
/// Cross-check assertion (c) is the contract that the typed test helper
/// does not drift from the JSON-encoded production query path. After this
/// test, the `serde_json` dependency disappears from every other test in
/// this file (see step-5's removal of `parse_face_normal{,_z}`).
#[test]
fn face_outward_unit_normal_for_test_returns_unit_outward_normal_for_sphere_face() {
    let (mut kernel, sphere_id) = sphere_kernel(5.0);
    let faces = kernel
        .extract_faces(sphere_id)
        .expect("extract_faces should succeed for sphere");
    let face = faces[0];

    let n = kernel
        .face_outward_unit_normal_for_test(face)
        .expect("face_outward_unit_normal_for_test should succeed for sphere face");

    // (a) Result is [f64; 3] — implied by the type signature; no runtime check.

    // (b) Unit length.
    let mag_sq = n[0] * n[0] + n[1] * n[1] + n[2] * n[2];
    assert!(
        (mag_sq - 1.0).abs() < 1e-9,
        "face_outward_unit_normal_for_test sphere: |n|² = {mag_sq}, expected 1.0"
    );

    // (c) Agrees with JSON-encoded query path within float tolerance.
    let json = match kernel.query(&GeometryQuery::FaceNormal(face)) {
        Ok(Value::String(s)) => s,
        other => panic!("FaceNormal returned unexpected value: {other:?}"),
    };
    let v: serde_json::Value = serde_json::from_str(&json)
        .unwrap_or_else(|e| panic!("FaceNormal JSON parse failed: {e}; raw = {json:?}"));
    let from_json = [
        v["x"].as_f64().expect("FaceNormal JSON missing 'x'"),
        v["y"].as_f64().expect("FaceNormal JSON missing 'y'"),
        v["z"].as_f64().expect("FaceNormal JSON missing 'z'"),
    ];
    for i in 0..3 {
        assert!(
            (n[i] - from_json[i]).abs() < 1e-9,
            "typed helper component {i} = {} disagrees with JSON-encoded path = {}",
            n[i],
            from_json[i]
        );
    }
}

#[test]
fn face_outward_unit_normal_for_test_unknown_handle_returns_invalid_handle() {
    let (kernel, _) = sphere_kernel(5.0);
    let unknown = GeometryHandleId(9999);
    match kernel.face_outward_unit_normal_for_test(unknown) {
        Err(QueryError::InvalidHandle(id)) if id == unknown => {}
        Err(QueryError::InvalidHandle(id)) => {
            panic!("expected InvalidHandle({unknown:?}), got InvalidHandle({id:?})")
        }
        other => panic!("expected Err(InvalidHandle({unknown:?})), got {other:?}"),
    }
}

#[test]
fn face_outward_unit_normal_for_test_non_face_shape_returns_query_failed() {
    let (kernel, cyl_id) = cylinder_kernel(5.0, 10.0);
    // cyl_id is the solid handle, not a face.
    match kernel.face_outward_unit_normal_for_test(cyl_id) {
        Err(QueryError::QueryFailed(_)) => {}
        other => panic!("expected Err(QueryFailed(_)) for solid handle, got {other:?}"),
    }
}

/// Cross-API agreement: `surface_normal_at` at the centroid's (u, v) agrees
/// in direction with `query_face_normal` at the centroid (dot product ≈ 1).
///
/// Both APIs use the same Du × Dv algorithm. `query_face_normal` returns the
/// outward normal at the face's physical centroid. For a sphere, the outward
/// normal is also the unit radial direction, so we can invert the
/// parametrization to recover which (u, v) the centroid corresponds to, then
/// call `surface_normal_at` at that same (u, v). Both calls must agree.
///
/// A disagreement (dot < 0.99) would indicate a regression in orientation
/// handling or the Du × Dv computation in one of the two APIs.
#[test]
fn surface_normal_at_matches_query_face_normal_at_centroid() {
    let (mut kernel, sphere_id) = sphere_kernel(5.0);
    let faces = kernel
        .extract_faces(sphere_id)
        .expect("extract_faces should succeed for sphere");
    let face = faces[0];

    // FaceNormal returns the outward unit normal at the physical centroid.
    let qfn = kernel
        .face_outward_unit_normal_for_test(face)
        .expect("FaceNormal should succeed for sphere face");

    // For a sphere, the outward normal direction determines (u, v) uniquely:
    //   normal = (cos(v)·cos(u), cos(v)·sin(u), sin(v))
    // Invert: v = arcsin(qfn[2]),  u = atan2(qfn[1], qfn[0]).
    // OCCT sphere uses u ∈ [0, 2π], so shift negative u by 2π.
    let v_at_centroid = qfn[2].asin();
    let u_at_centroid = {
        let u = qfn[1].atan2(qfn[0]);
        if u < 0.0 { u + 2.0 * PI } else { u }
    };

    // surface_normal_at at the recovered centroid (u, v) must agree with qfn.
    let n = kernel
        .surface_normal_at(face, u_at_centroid, v_at_centroid)
        .expect("surface_normal_at should succeed at the centroid (u, v)");

    let dot = n[0] * qfn[0] + n[1] * qfn[1] + n[2] * qfn[2];
    assert!(
        dot > 0.99,
        "surface_normal_at and query_face_normal disagree at the sphere centroid: \
         dot = {dot}, n = {n:?}, qfn = {qfn:?}, u = {u_at_centroid}, v = {v_at_centroid}"
    );
}

// ---------------------------------------------------------------------------
// curvature_at — happy-path: sphere
// ---------------------------------------------------------------------------

/// `curvature_at` on a sphere of radius 5 returns the analytic constant
/// curvature values (umbilical surface).
///
/// OCCT sign convention: mean curvature H and principal curvatures κᵢ are
/// negative for convex surfaces when the outward (surface) normal points away
/// from the centre of curvature. For a sphere of radius r with outward normal:
///   K = 1/r² = 0.04   (Gaussian, always positive for a sphere),
///   H = -1/r = -0.2   (negative: centre of curvature is inward),
///   κ_min = κ_max = -1/r = -0.2  (umbilical: all principal curvatures equal).
///
/// Principal directions are unit tangent vectors at the surface (tangent-plane
/// property verified by near-zero dot product with the face outward normal).
#[test]
fn curvature_at_on_sphere_face_yields_constant_k_and_h() {
    use reify_kernel_occt::Curvature;

    let (mut kernel, sphere_id) = sphere_kernel(5.0);
    let faces = kernel
        .extract_faces(sphere_id)
        .expect("extract_faces should succeed for sphere");
    let face = faces[0];

    // (π, 0): safe interior point, away from parametric poles (v = ±π/2).
    let c: Curvature = kernel
        .curvature_at(face, PI, 0.0)
        .expect("curvature_at should succeed for sphere face");

    let tol = 1e-9;
    // Gaussian K = 1/r² — invariant, always positive.
    assert!(
        (c.gaussian - 0.04).abs() < tol,
        "sphere Gaussian curvature: expected 0.04, got {}",
        c.gaussian
    );
    // Mean H = -1/r (negative: convex sphere, outward normal, OCCT sign convention).
    assert!(
        (c.mean + 0.2).abs() < tol,
        "sphere mean curvature: expected -0.2, got {}",
        c.mean
    );
    // κ_min = κ_max = -1/r (umbilical, both equal).
    assert!(
        (c.kappa_min + 0.2).abs() < tol,
        "sphere κ_min: expected -0.2, got {}",
        c.kappa_min
    );
    assert!(
        (c.kappa_max + 0.2).abs() < tol,
        "sphere κ_max: expected -0.2, got {}",
        c.kappa_max
    );

    // Principal directions must lie in the tangent plane: near-zero dot with
    // the face outward normal at (π, 0). On a sphere umbilical point, OCCT
    // picks an arbitrary orthonormal pair in the tangent plane.
    let n = kernel
        .surface_normal_at(face, PI, 0.0)
        .expect("surface_normal_at should succeed");
    let dot_min = c.dir_min[0] * n[0] + c.dir_min[1] * n[1] + c.dir_min[2] * n[2];
    let dot_max = c.dir_max[0] * n[0] + c.dir_max[1] * n[1] + c.dir_max[2] * n[2];
    assert!(
        dot_min.abs() < 1e-9,
        "dir_min should be in the tangent plane: dot(dir_min, n) = {dot_min}"
    );
    assert!(
        dot_max.abs() < 1e-9,
        "dir_max should be in the tangent plane: dot(dir_max, n) = {dot_max}"
    );

    // Principal directions must be unit length (per the Curvature doc contract).
    let dir_min_mag_sq =
        c.dir_min[0] * c.dir_min[0] + c.dir_min[1] * c.dir_min[1] + c.dir_min[2] * c.dir_min[2];
    let dir_max_mag_sq =
        c.dir_max[0] * c.dir_max[0] + c.dir_max[1] * c.dir_max[1] + c.dir_max[2] * c.dir_max[2];
    assert!(
        (dir_min_mag_sq - 1.0).abs() < 1e-9,
        "sphere dir_min should be unit length: |dir_min|² = {dir_min_mag_sq}"
    );
    assert!(
        (dir_max_mag_sq - 1.0).abs() < 1e-9,
        "sphere dir_max should be unit length: |dir_max|² = {dir_max_mag_sq}"
    );

    // Principal directions must be mutually orthogonal (OCCT picks an
    // orthonormal pair at umbilical points; both lie in the tangent plane).
    let dot_dirs =
        c.dir_min[0] * c.dir_max[0] + c.dir_min[1] * c.dir_max[1] + c.dir_min[2] * c.dir_max[2];
    assert!(
        dot_dirs.abs() < 1e-9,
        "sphere dir_min and dir_max should be mutually orthogonal: dot = {dot_dirs}"
    );
}

// ---------------------------------------------------------------------------
// curvature_at — edge cases
// ---------------------------------------------------------------------------

/// `curvature_at` on the curved side face of a cylinder is developable.
///
/// OCCT sign convention (negative for convex surfaces with outward normal):
///   K = 0     (developable: one zero principal curvature),
///   H = -1/(2r) = -0.1,
///   κ_min = -1/r = -0.2  (circumferential direction, most curved),
///   κ_max = 0            (axial direction, no curvature along Z).
///
/// Note: κ_min corresponds to the circumferential direction because
/// `GeomLProp_SLProps::MinCurvature` returns the smallest (most negative)
/// signed value. The re-sort in `curvature_at` preserves this ordering.
#[test]
fn curvature_at_on_cylinder_side_face_yields_developable_curvature() {
    let (mut kernel, cyl_id) = cylinder_kernel(5.0, 10.0);
    let faces = kernel
        .extract_faces(cyl_id)
        .expect("extract_faces should succeed for cylinder");
    assert_eq!(faces.len(), 3, "cylinder should have 3 faces");

    let side_face = faces
        .iter()
        .copied()
        .find(|&f| {
            kernel
                .face_outward_unit_normal_for_test(f)
                .expect("FaceNormal should succeed for cylinder face")[2]
                .abs()
                < 0.5
        })
        .expect("cylinder should have a curved side face");

    let c = kernel
        .curvature_at(side_face, PI / 2.0, 5.0)
        .expect("curvature_at should succeed for cylinder side face");

    let tol = 1e-9;

    // Cylinder is developable: K = 0.
    assert!(
        c.gaussian.abs() < tol,
        "cylinder side Gaussian: expected 0, got {}",
        c.gaussian
    );
    // H = -1/(2r) = -0.1 (negative: convex cylinder, OCCT sign convention).
    assert!(
        (c.mean + 0.1).abs() < tol,
        "cylinder side mean: expected -0.1, got {}",
        c.mean
    );
    // κ_min = -1/r = -0.2 (circumferential direction — most curved, most negative).
    assert!(
        (c.kappa_min + 0.2).abs() < tol,
        "cylinder side κ_min: expected -0.2, got {}",
        c.kappa_min
    );
    // κ_max = 0 (axial direction — no curvature along Z).
    assert!(
        c.kappa_max.abs() < tol,
        "cylinder side κ_max: expected 0, got {}",
        c.kappa_max
    );

    // dir_min corresponds to κ_min = -0.2 (circumferential direction ⊥ Z).
    // dir_max corresponds to κ_max = 0 (axial direction ≈ ±Z).
    // Accept both +Z and −Z for dir_max since OCCT's sign convention may vary.
    assert!(
        c.dir_min[2].abs() < 1e-9,
        "cylinder dir_min (circumferential) should be ⊥ Z, got {:?}",
        c.dir_min
    );

    let axial_dot = c.dir_max[2].abs();
    assert!(
        (axial_dot - 1.0).abs() < 1e-9,
        "cylinder dir_max (axial) should be ≈ ±Z, got {:?}",
        c.dir_max
    );

    // Both principal directions must be unit length (per the Curvature doc contract).
    let dir_min_mag_sq =
        c.dir_min[0] * c.dir_min[0] + c.dir_min[1] * c.dir_min[1] + c.dir_min[2] * c.dir_min[2];
    let dir_max_mag_sq =
        c.dir_max[0] * c.dir_max[0] + c.dir_max[1] * c.dir_max[1] + c.dir_max[2] * c.dir_max[2];
    assert!(
        (dir_min_mag_sq - 1.0).abs() < 1e-9,
        "cylinder dir_min should be unit length: |dir_min|² = {dir_min_mag_sq}"
    );
    assert!(
        (dir_max_mag_sq - 1.0).abs() < 1e-9,
        "cylinder dir_max should be unit length: |dir_max|² = {dir_max_mag_sq}"
    );

    // Mutually orthogonal.
    let dot_dirs =
        c.dir_min[0] * c.dir_max[0] + c.dir_min[1] * c.dir_max[1] + c.dir_min[2] * c.dir_max[2];
    assert!(
        dot_dirs.abs() < 1e-9,
        "cylinder dir_min and dir_max should be mutually orthogonal: dot = {dot_dirs}"
    );
}

/// `curvature_at` on a flat cylinder cap returns all-zero curvatures (planar face).
#[test]
fn curvature_at_on_cylinder_cap_face_yields_zero_curvature() {
    let (mut kernel, cyl_id) = cylinder_kernel(5.0, 10.0);
    let faces = kernel
        .extract_faces(cyl_id)
        .expect("extract_faces should succeed for cylinder");
    assert_eq!(faces.len(), 3, "cylinder should have 3 faces");

    // Pick either cap (|n_z| ≈ 1).
    let cap_face = faces
        .iter()
        .copied()
        .find(|&f| {
            kernel
                .face_outward_unit_normal_for_test(f)
                .expect("FaceNormal should succeed for cylinder face")[2]
                .abs()
                > 0.99
        })
        .expect("cylinder should have a flat cap face");

    // The cap is a disk; query at (0, 0) as a stable interior parameter.
    let c = kernel
        .curvature_at(cap_face, 0.0, 0.0)
        .expect("curvature_at should succeed for cylinder cap face");

    let tol = 1e-9;
    assert!(
        c.gaussian.abs() < tol,
        "cap Gaussian: expected 0, got {}",
        c.gaussian
    );
    assert!(c.mean.abs() < tol, "cap mean: expected 0, got {}", c.mean);
    assert!(
        c.kappa_min.abs() < tol,
        "cap κ_min: expected 0, got {}",
        c.kappa_min
    );
    assert!(
        c.kappa_max.abs() < tol,
        "cap κ_max: expected 0, got {}",
        c.kappa_max
    );
}

/// `curvature_at` on a `TopAbs_REVERSED` face whose principal curvatures
/// differ in absolute value (non-developable case) keeps `(κ_min, dir_min)`
/// and `(κ_max, dir_max)` correctly paired after the orientation-flip swap.
///
/// Regression for the bug where `REVERSED` faces had their principal-curvature
/// values negated and re-sorted but the corresponding direction vectors were
/// NOT swapped — so `dir_min` could end up paired with `κ_max` (and vice
/// versa) whenever the sign flip reversed the value ordering.
///
/// Construction: a hollow cylinder = outer cylinder (R = 10) − inner cylinder
/// (r = 5). The Boolean cut leaves the inner cylindrical face oriented
/// `REVERSED` so its outward normal points toward the axis. That inner face
/// has principal curvatures `+1/r` (circumferential, paired with a tangent
/// vector ⊥ Z) and `0` (axial, paired with ±Z) — different absolute values,
/// so the (κ, direction) pairing matters.
///
/// The earlier sphere/cylinder happy-path tests do not catch this because:
///   - sphere is umbilical (`dir_min == dir_max` up to sign),
///   - the regular cylinder side face is `FORWARD` (no flip taken).
#[test]
fn curvature_at_on_reversed_inner_cylinder_face_pairs_directions_with_min_max() {
    use reify_kernel_occt::Curvature;
    let mut kernel = OcctKernel::new();
    let outer = kernel
        .execute(&GeometryOp::Cylinder {
            radius: Value::Real(10.0),
            height: Value::Real(10.0),
        })
        .expect("outer cylinder creation should succeed");
    let inner = kernel
        .execute(&GeometryOp::Cylinder {
            radius: Value::Real(5.0),
            height: Value::Real(10.0),
        })
        .expect("inner cylinder creation should succeed");
    let hollow = kernel
        .execute(&GeometryOp::Difference {
            left: outer.id,
            right: inner.id,
        })
        .expect("hollow-cylinder difference should succeed");

    let faces = kernel
        .extract_faces(hollow.id)
        .expect("extract_faces should succeed for hollow cylinder");

    // Locate the inner cylindrical face: its `surface_normal_at` succeeds at
    // (u = π/2, v = 5) AND the returned outward normal points inward
    // (negative radial — i.e. n_y < 0 at this parametric point) AND it is
    // perpendicular to Z (curved side, not a cap).
    let inner_side = faces
        .iter()
        .copied()
        .find(|&f| {
            kernel
                .surface_normal_at(f, PI / 2.0, 5.0)
                .is_ok_and(|n| n[2].abs() < 0.5 && n[1] < -0.5)
        })
        .expect(
            "hollow cylinder should have an inner cylindrical face whose \
             outward normal at (π/2, 5) points inward",
        );

    let c: Curvature = kernel
        .curvature_at(inner_side, PI / 2.0, 5.0)
        .expect("curvature_at should succeed for the inner cylindrical face");

    let tol = 1e-9;

    // Sanity: K = 0 (still developable), H = +1/(2r) = +0.1, κ_min = 0,
    // κ_max = +1/r = +0.2 (positive sign because the post-orientation outward
    // normal points toward the centre of curvature on this concave face).
    assert!(
        c.gaussian.abs() < tol,
        "inner side Gaussian: expected 0, got {}",
        c.gaussian
    );
    assert!(
        (c.mean - 0.1).abs() < tol,
        "inner side mean: expected +0.1, got {}",
        c.mean
    );
    assert!(
        c.kappa_min.abs() < tol,
        "inner side κ_min: expected 0, got {}",
        c.kappa_min
    );
    assert!(
        (c.kappa_max - 0.2).abs() < tol,
        "inner side κ_max: expected +0.2, got {}",
        c.kappa_max
    );

    // CRITICAL — the (κ, direction) pairing under REVERSED:
    //   κ_min = 0       must be paired with the axial direction (≈ ±Z),
    //   κ_max = +0.2    must be paired with the circumferential direction (⊥ Z).
    // If the directions were not swapped together with the curvature labels,
    // these dot-product assertions would flip.
    let dir_min_axial_dot = c.dir_min[2].abs();
    assert!(
        (dir_min_axial_dot - 1.0).abs() < 1e-9,
        "inner side dir_min (paired with κ_min = 0) should be axial (≈ ±Z), \
         got dir_min = {:?}, |dir_min · Z| = {dir_min_axial_dot}",
        c.dir_min
    );
    assert!(
        c.dir_max[2].abs() < 1e-9,
        "inner side dir_max (paired with κ_max = +0.2) should be \
         circumferential (⊥ Z), got dir_max = {:?}, dir_max[2] = {}",
        c.dir_max,
        c.dir_max[2]
    );
}

/// Passing an unknown handle returns `QueryError::InvalidHandle`.
#[test]
fn curvature_at_unknown_handle_returns_invalid_handle() {
    let (kernel, _) = sphere_kernel(5.0);
    let unknown = GeometryHandleId(9999);
    match kernel.curvature_at(unknown, 0.0, 0.0) {
        Err(QueryError::InvalidHandle(id)) if id == unknown => {}
        Err(QueryError::InvalidHandle(id)) => {
            panic!("expected InvalidHandle({unknown:?}), got InvalidHandle({id:?})")
        }
        other => panic!("expected Err(InvalidHandle({unknown:?})), got {other:?}"),
    }
}

/// Passing a solid handle (not a face) returns `QueryError::QueryFailed` with
/// a message containing "not a face".
#[test]
fn curvature_at_non_face_shape_returns_query_failed_with_not_a_face() {
    let (kernel, cyl_id) = cylinder_kernel(5.0, 10.0);
    match kernel.curvature_at(cyl_id, 0.0, 0.0) {
        Err(QueryError::QueryFailed(msg)) => {
            assert!(
                msg.contains("not a face"),
                "expected error containing 'not a face', got: {msg}"
            );
        }
        other => panic!(
            "expected Err(QueryFailed(\"...not a face...\")) for solid handle, got {other:?}"
        ),
    }
}

// ---------------------------------------------------------------------------
// Placed-face cross-API agreement (architectural-unification contract)
// ---------------------------------------------------------------------------

/// Cross-API agreement on a placed (non-identity-location) cylinder side face.
///
/// The cylinder (radius=5, height=10) is rotated around the X-axis by π/4 and
/// translated to (2, 3, -1) using `BRepBuilderAPI_Transform(..., Copy=false)`,
/// so its faces have a non-identity `TopLoc_Location`. The rotation tilts the
/// cylinder axis from (0,0,1) to (0, −sin(π/4), cos(π/4)) ≈ (0, −0.707, 0.707).
///
/// Asserts on the curved side face at (u=π/2, v=5):
///   (a) K = 0 (developable),
///   (b) H = −0.1 (= −1/(2r)),
///   (c) κ_min = −0.2 (circumferential),  κ_max = 0 (axial),
///   (d) dir_max — paired with κ_max=0 — has |dir_max · rotated_axis| ≈ 1,
///       i.e. the ROTATED (world-frame) axis, NOT world Z. This is the key
///       cross-API check: if curvature_at used a different abstraction, it
///       would return world-Z for dir_max instead of the rotated axis.
///   (e) dir_min · n ≈ 0 and dir_max · n ≈ 0 (tangent-plane membership),
///   (f) dir_min and dir_max are unit length and mutually orthogonal.
///
/// This exercises the eigenvalue solver in the non-umbilical case.
#[test]
fn curvature_at_on_placed_cylinder_side_axial_principal_direction_aligns_with_rotated_axis() {
    use reify_kernel_occt::Curvature;

    let r = 5.0_f64;
    let mut kernel = OcctKernel::new();
    let cyl = kernel
        .execute(&GeometryOp::Cylinder {
            radius: Value::Real(r),
            height: Value::Real(10.0),
        })
        .expect("cylinder creation should succeed");

    // Rotate around X-axis by π/4, then translate to (2, 3, -1). Copy=false.
    let placed = kernel.store_placed_for_test(
        cyl.id,
        1.0,
        0.0,
        0.0,      // rotation axis: X
        PI / 4.0, // rotation angle
        2.0,
        3.0,
        -1.0, // translation
    );

    let faces = kernel
        .extract_faces(placed)
        .expect("extract_faces should succeed for placed cylinder");
    assert_eq!(faces.len(), 3, "placed cylinder should have 3 faces");

    // The rotated cylinder axis is the world-frame image of (0,0,1) under the
    // X-rotation: (0, -sin(π/4), cos(π/4)).
    let rot_axis = [0.0_f64, -(PI / 4.0).sin(), (PI / 4.0).cos()];

    // Identify the curved side face: its centroid normal is radially outward
    // from the (tilted) cylinder axis, so |n · rotated_axis| < 0.5.
    let side_face = faces
        .iter()
        .copied()
        .find(|&f| {
            let n = kernel
                .face_outward_unit_normal_for_test(f)
                .expect("FaceNormal should succeed for placed cylinder face");
            let dot = n[0] * rot_axis[0] + n[1] * rot_axis[1] + n[2] * rot_axis[2];
            dot.abs() < 0.5
        })
        .expect(
            "placed cylinder should have a curved side face whose centroid normal \
             is perpendicular to the rotated axis",
        );

    // Query at (u=π/2, v=5): OCCT cylinder P(u,v) = (r·cos u, r·sin u, v).
    let n = kernel
        .surface_normal_at(side_face, PI / 2.0, 5.0)
        .expect("surface_normal_at should succeed on placed cylinder side face");
    let c: Curvature = kernel
        .curvature_at(side_face, PI / 2.0, 5.0)
        .expect("curvature_at should succeed on placed cylinder side face");

    let tol = 1e-6;

    // (a) Developable: K = 0.
    assert!(
        c.gaussian.abs() < tol,
        "placed cylinder side K: expected 0, got {}",
        c.gaussian
    );
    // (b) H = -1/(2r).
    assert!(
        (c.mean + 1.0 / (2.0 * r)).abs() < tol,
        "placed cylinder side H: expected {}, got {}",
        -1.0 / (2.0 * r),
        c.mean
    );
    // (c) κ_min = -1/r (circumferential), κ_max = 0 (axial).
    assert!(
        (c.kappa_min + 1.0 / r).abs() < tol,
        "placed cylinder side κ_min: expected {}, got {}",
        -1.0 / r,
        c.kappa_min
    );
    assert!(
        c.kappa_max.abs() < tol,
        "placed cylinder side κ_max: expected 0, got {}",
        c.kappa_max
    );

    // (d) dir_max (paired with κ_max=0) must align with the ROTATED cylinder
    //     axis — not world Z. This is the architectural-unification check:
    //     if curvature_at used the old BRep_Tool::Surface path with baked
    //     geometry, the axis direction would agree, but using non-baked
    //     (Copy=false) placement ensures we exercise TopoLoc_Location.
    let dot_axial =
        c.dir_max[0] * rot_axis[0] + c.dir_max[1] * rot_axis[1] + c.dir_max[2] * rot_axis[2];
    assert!(
        (dot_axial.abs() - 1.0).abs() < tol,
        "placed cylinder dir_max should align with rotated axis (0, -{:.4}, {:.4}), \
         got dir_max = {:?}, |dot| = {dot_axial:.9}",
        (PI / 4.0).sin(),
        (PI / 4.0).cos(),
        c.dir_max
    );

    // (e) Both principal directions lie in the tangent plane.
    let dot_min_n = c.dir_min[0] * n[0] + c.dir_min[1] * n[1] + c.dir_min[2] * n[2];
    let dot_max_n = c.dir_max[0] * n[0] + c.dir_max[1] * n[1] + c.dir_max[2] * n[2];
    assert!(
        dot_min_n.abs() < tol,
        "placed cylinder dir_min not in tangent plane: dot(dir_min, n) = {dot_min_n}"
    );
    assert!(
        dot_max_n.abs() < tol,
        "placed cylinder dir_max not in tangent plane: dot(dir_max, n) = {dot_max_n}"
    );

    // (f) Unit length and mutually orthogonal.
    let dmin_mag_sq =
        c.dir_min[0] * c.dir_min[0] + c.dir_min[1] * c.dir_min[1] + c.dir_min[2] * c.dir_min[2];
    let dmax_mag_sq =
        c.dir_max[0] * c.dir_max[0] + c.dir_max[1] * c.dir_max[1] + c.dir_max[2] * c.dir_max[2];
    assert!(
        (dmin_mag_sq - 1.0).abs() < 1e-9,
        "placed cylinder dir_min not unit length: |dir_min|² = {dmin_mag_sq}"
    );
    assert!(
        (dmax_mag_sq - 1.0).abs() < 1e-9,
        "placed cylinder dir_max not unit length: |dir_max|² = {dmax_mag_sq}"
    );
    let dot_dirs =
        c.dir_min[0] * c.dir_max[0] + c.dir_min[1] * c.dir_max[1] + c.dir_min[2] * c.dir_max[2];
    assert!(
        dot_dirs.abs() < 1e-9,
        "placed cylinder dir_min and dir_max should be orthogonal: dot = {dot_dirs}"
    );
}

/// Cross-API agreement on a placed (non-identity-location) sphere.
///
/// The sphere is rotated around the Y-axis by π/3 and translated to (10, 2, -3)
/// using `BRepBuilderAPI_Transform(..., Copy=false)`, so its faces have a
/// non-identity `TopLoc_Location`. Both `surface_normal_at` and `curvature_at`
/// must agree that:
///   (a) the outward normal is unit length,
///   (b) K = 1/r² = 1/25,  H = -1/r = -1/5,  κ_min = κ_max = -1/5 (umbilical),
///   (c) principal directions dir_min and dir_max are unit length, mutually
///       orthogonal, and lie in the tangent plane (dot with world normal ≈ 0).
///
/// The tangent-plane assertion (c) is the key architectural-unification check:
/// if `curvature_at` used a different surface abstraction than `surface_normal_at`
/// the principal directions might not be perpendicular to the `surface_normal_at`
/// world normal. Both now use `BRepAdaptor_Surface`, so they must agree.
///
/// This exercises the umbilical fallback path (sphere is umbilical at every point).
#[test]
fn curvature_at_on_placed_sphere_principal_directions_perpendicular_to_world_normal() {
    use reify_kernel_occt::Curvature;

    let r = 5.0_f64;
    let mut kernel = OcctKernel::new();
    let sphere = kernel
        .execute(&GeometryOp::Sphere {
            radius: Value::Real(r),
        })
        .expect("sphere creation should succeed");

    // Place the sphere with a non-identity location (rotation around Y by π/3,
    // then translate to (10, 2, -3)). Copy=false → TopLoc_Location, not baked.
    let placed = kernel.store_placed_for_test(
        sphere.id,
        0.0,
        1.0,
        0.0,      // rotation axis: Y
        PI / 3.0, // rotation angle
        10.0,
        2.0,
        -3.0, // translation
    );

    let faces = kernel
        .extract_faces(placed)
        .expect("extract_faces should succeed for placed sphere");
    assert!(
        !faces.is_empty(),
        "placed sphere should have at least one face"
    );
    let face = faces[0];

    // At (u=π, v=0) — safe interior point, away from the poles.
    let n = kernel
        .surface_normal_at(face, PI, 0.0)
        .expect("surface_normal_at should succeed on placed sphere");
    let c: Curvature = kernel
        .curvature_at(face, PI, 0.0)
        .expect("curvature_at should succeed on placed sphere");

    // (a) Normal unit length.
    let mag_sq = n[0] * n[0] + n[1] * n[1] + n[2] * n[2];
    assert!(
        (mag_sq - 1.0).abs() < 1e-9,
        "placed sphere: surface_normal_at should return a unit vector, |n|² = {mag_sq}"
    );

    // (b) Intrinsic curvature values (looser tolerance to absorb rotation FP noise).
    let tol = 1e-6;
    assert!(
        (c.gaussian - 1.0 / (r * r)).abs() < tol,
        "placed sphere K: expected {}, got {}",
        1.0 / (r * r),
        c.gaussian
    );
    assert!(
        (c.mean + 1.0 / r).abs() < tol,
        "placed sphere H: expected {}, got {}",
        -1.0 / r,
        c.mean
    );
    assert!(
        (c.kappa_min + 1.0 / r).abs() < tol,
        "placed sphere κ_min: expected {}, got {}",
        -1.0 / r,
        c.kappa_min
    );
    assert!(
        (c.kappa_max + 1.0 / r).abs() < tol,
        "placed sphere κ_max: expected {}, got {}",
        -1.0 / r,
        c.kappa_max
    );

    // (c) Principal directions unit length.
    let dmin_mag_sq =
        c.dir_min[0] * c.dir_min[0] + c.dir_min[1] * c.dir_min[1] + c.dir_min[2] * c.dir_min[2];
    let dmax_mag_sq =
        c.dir_max[0] * c.dir_max[0] + c.dir_max[1] * c.dir_max[1] + c.dir_max[2] * c.dir_max[2];
    assert!(
        (dmin_mag_sq - 1.0).abs() < 1e-9,
        "placed sphere dir_min not unit length: |dir_min|² = {dmin_mag_sq}"
    );
    assert!(
        (dmax_mag_sq - 1.0).abs() < 1e-9,
        "placed sphere dir_max not unit length: |dir_max|² = {dmax_mag_sq}"
    );

    // (c) Principal directions mutually orthogonal.
    let dot_dirs =
        c.dir_min[0] * c.dir_max[0] + c.dir_min[1] * c.dir_max[1] + c.dir_min[2] * c.dir_max[2];
    assert!(
        dot_dirs.abs() < 1e-9,
        "placed sphere dir_min and dir_max should be orthogonal: dot = {dot_dirs}"
    );

    // (c) Principal directions lie in the tangent plane: dot with world normal ≈ 0.
    // This is the key architectural-unification check: both APIs must use the same
    // surface abstraction (BRepAdaptor_Surface) to agree on the world-frame normal.
    let dot_min_n = c.dir_min[0] * n[0] + c.dir_min[1] * n[1] + c.dir_min[2] * n[2];
    let dot_max_n = c.dir_max[0] * n[0] + c.dir_max[1] * n[1] + c.dir_max[2] * n[2];
    assert!(
        dot_min_n.abs() < 1e-6,
        "placed sphere dir_min not in tangent plane: dot(dir_min, n) = {dot_min_n}"
    );
    assert!(
        dot_max_n.abs() < 1e-6,
        "placed sphere dir_max not in tangent plane: dot(dir_max, n) = {dot_max_n}"
    );
}

// ---------------------------------------------------------------------------
// NaN / Inf input rejection (validate_uv_finite guard)
// ---------------------------------------------------------------------------

/// Bad-input matrix shared by both NaN/Inf rejection tests.
///
/// Covers each axis individually (NaN-u, NaN-v, +Inf-u, −Inf-v) plus the
/// "both bad" case (NaN, NaN). The `validate_uv_finite` helper short-circuits
/// on the first non-finite component, so (NaN, NaN) is redundant from a
/// *coverage* standpoint but makes the invariant explicit for readers.
const NON_FINITE_UV: &[(f64, f64)] = &[
    (f64::NAN, 0.0),
    (0.0, f64::NAN),
    (f64::INFINITY, 0.0),
    (0.0, f64::NEG_INFINITY),
    (f64::NAN, f64::NAN),
];

/// `surface_normal_at` rejects non-finite (u, v) inputs with
/// `QueryError::NonFiniteParameter { u, v }` echoing the bad input.
///
/// The guard must fire before the FFI call so that NaN/Inf parametric
/// coordinates never reach the C++ wrapper. The bad-input cases cover:
/// NaN-u, NaN-v, +Inf-u, -Inf-v, and both-NaN — see [`NON_FINITE_UV`].
#[test]
fn surface_normal_at_rejects_non_finite_uv() {
    let (mut kernel, sphere_id) = sphere_kernel(5.0);
    let faces = kernel
        .extract_faces(sphere_id)
        .expect("extract_faces should succeed for sphere");
    let face = faces[0];

    for &(u, v) in NON_FINITE_UV {
        match kernel.surface_normal_at(face, u, v) {
            Err(QueryError::NonFiniteParameter { u: eu, v: ev }) => {
                // Pin that the variant carries the same (u, v) the test passed in.
                // bit-equality on NaN is false, so use is_nan-aware comparison:
                let bit_eq = |a: f64, b: f64| (a.is_nan() && b.is_nan()) || a == b;
                assert!(
                    bit_eq(eu, u) && bit_eq(ev, v),
                    "surface_normal_at(u={u}, v={v}): NonFiniteParameter {{ u: {eu}, v: {ev} }} \
                     did not echo input"
                );
            }
            other => panic!(
                "surface_normal_at(u={u}, v={v}): expected \
                 Err(NonFiniteParameter {{ ... }}), got {other:?}"
            ),
        }
    }
}

/// `curvature_at` rejects non-finite (u, v) inputs with
/// `QueryError::NonFiniteParameter { u, v }` echoing the bad input.
///
/// Mirrors `surface_normal_at_rejects_non_finite_uv` for the `curvature_at`
/// entrypoint — both share the same `validate_uv_finite` helper.
#[test]
fn curvature_at_rejects_non_finite_uv() {
    let (mut kernel, sphere_id) = sphere_kernel(5.0);
    let faces = kernel
        .extract_faces(sphere_id)
        .expect("extract_faces should succeed for sphere");
    let face = faces[0];

    for &(u, v) in NON_FINITE_UV {
        match kernel.curvature_at(face, u, v) {
            Err(QueryError::NonFiniteParameter { u: eu, v: ev }) => {
                // Pin that the variant carries the same (u, v) the test passed in.
                // bit-equality on NaN is false, so use is_nan-aware comparison:
                let bit_eq = |a: f64, b: f64| (a.is_nan() && b.is_nan()) || a == b;
                assert!(
                    bit_eq(eu, u) && bit_eq(ev, v),
                    "curvature_at(u={u}, v={v}): NonFiniteParameter {{ u: {eu}, v: {ev} }} \
                     did not echo input"
                );
            }
            other => panic!(
                "curvature_at(u={u}, v={v}): expected \
                 Err(NonFiniteParameter {{ ... }}), got {other:?}"
            ),
        }
    }
}

/// Cross-API agreement on a placed (non-identity-location) REVERSED face.
///
/// Covers the quadrant missing from the other placed tests: REVERSED orientation
/// combined with a non-identity `TopLoc_Location`. The existing
/// `curvature_at_on_reversed_inner_cylinder_face_pairs_directions_with_min_max`
/// exercises REVERSED on an identity-location face; the placed tests above cover
/// FORWARD orientation only.
///
/// Construction: hollow cylinder (outer R=10, inner r=5) placed with a rotation
/// around the Y-axis by π/4 and translation (3, −2, 5) via `store_placed_for_test`
/// (Copy=false → TopLoc_Location, not baked). The inner cylindrical face retains
/// `TopAbs_REVERSED` topology; its outward normal points toward the axis.
///
/// The rotated cylinder axis (world-frame image of Z under Y-rotation by π/4):
///   `rotated_axis = (sin(π/4), 0, cos(π/4)) ≈ (0.707, 0, 0.707)`.
///
/// Asserts on the inner cylindrical face at (u=π/2, v=5):
///   (a) K = 0 (developable),
///   (b) H = +0.1 = +1/(2r) (positive because REVERSED → normal toward axis),
///   (c) κ_min = 0 (axial),  κ_max = +0.2 = +1/r (circumferential),
///   (d) dir_min (paired with κ_min=0) has |dir_min · rotated_axis| ≈ 1,
///       confirming the axial direction tracks the ROTATED (world-frame) axis,
///   (e) both principal directions lie in the tangent plane and are unit/orthogonal.
#[test]
fn curvature_at_on_placed_reversed_inner_cylinder_face_pairs_directions_with_rotated_axis() {
    use reify_kernel_occt::Curvature;

    let r_inner = 5.0_f64;
    let mut kernel = OcctKernel::new();
    let outer = kernel
        .execute(&GeometryOp::Cylinder {
            radius: Value::Real(10.0),
            height: Value::Real(10.0),
        })
        .expect("outer cylinder creation should succeed");
    let inner = kernel
        .execute(&GeometryOp::Cylinder {
            radius: Value::Real(r_inner),
            height: Value::Real(10.0),
        })
        .expect("inner cylinder creation should succeed");
    let hollow = kernel
        .execute(&GeometryOp::Difference {
            left: outer.id,
            right: inner.id,
        })
        .expect("hollow-cylinder difference should succeed");

    // Rotate around Y-axis by π/4, then translate to (3, -2, 5). Copy=false.
    let placed = kernel.store_placed_for_test(
        hollow.id,
        0.0,
        1.0,
        0.0,      // rotation axis: Y
        PI / 4.0, // rotation angle
        3.0,
        -2.0,
        5.0, // translation
    );

    let faces = kernel
        .extract_faces(placed)
        .expect("extract_faces should succeed for placed hollow cylinder");

    // The rotated cylinder axis is the world-frame image of (0,0,1) under
    // Y-rotation by π/4: (sin(π/4), 0, cos(π/4)).
    let rot_axis = [(PI / 4.0).sin(), 0.0_f64, (PI / 4.0).cos()];

    // Locate the inner cylindrical face: at (u=π/2, v=5), its effective outward
    // normal points toward the axis — n[1] < -0.5 (Y component unchanged by
    // Y-axis rotation) — and the face is curved (|n[2]| < 0.5 rules out caps
    // whose world-frame normals have |n_z| ≈ cos(π/4) ≈ 0.707 after rotation).
    let inner_side = faces
        .iter()
        .copied()
        .find(|&f| {
            kernel
                .surface_normal_at(f, PI / 2.0, 5.0)
                .is_ok_and(|n| n[2].abs() < 0.5 && n[1] < -0.5)
        })
        .expect(
            "placed hollow cylinder should have an inner cylindrical face whose \
             outward normal at (π/2, 5) points inward (n[1] < -0.5)",
        );

    let c: Curvature = kernel
        .curvature_at(inner_side, PI / 2.0, 5.0)
        .expect("curvature_at should succeed for the placed inner cylindrical face");

    let tol = 1e-6;

    // (a–c) Intrinsic curvature values — same as the unplaced inner face.
    assert!(
        c.gaussian.abs() < tol,
        "placed reversed cylinder K: expected 0, got {}",
        c.gaussian
    );
    assert!(
        (c.mean - 0.1).abs() < tol,
        "placed reversed cylinder H: expected +0.1, got {}",
        c.mean
    );
    assert!(
        c.kappa_min.abs() < tol,
        "placed reversed cylinder κ_min: expected 0, got {}",
        c.kappa_min
    );
    assert!(
        (c.kappa_max - 0.2).abs() < tol,
        "placed reversed cylinder κ_max: expected +0.2, got {}",
        c.kappa_max
    );

    // (d) dir_min (paired with κ_min=0, axial) must align with the ROTATED axis —
    //     not world Z. This is the key check for REVERSED + non-identity location.
    let dot_axial =
        c.dir_min[0] * rot_axis[0] + c.dir_min[1] * rot_axis[1] + c.dir_min[2] * rot_axis[2];
    assert!(
        (dot_axial.abs() - 1.0).abs() < tol,
        "placed reversed cylinder dir_min (κ_min=0) should align with rotated axis \
         ({:.4}, 0, {:.4}), got dir_min = {:?}, |dot| = {dot_axial:.9}",
        (PI / 4.0).sin(),
        (PI / 4.0).cos(),
        c.dir_min
    );

    // (e) Both principal directions lie in the tangent plane.
    let n = kernel
        .surface_normal_at(inner_side, PI / 2.0, 5.0)
        .expect("surface_normal_at should succeed on placed inner cylindrical face");
    let dot_min_n = c.dir_min[0] * n[0] + c.dir_min[1] * n[1] + c.dir_min[2] * n[2];
    let dot_max_n = c.dir_max[0] * n[0] + c.dir_max[1] * n[1] + c.dir_max[2] * n[2];
    assert!(
        dot_min_n.abs() < tol,
        "placed reversed cylinder dir_min not in tangent plane: dot(dir_min, n) = {dot_min_n}"
    );
    assert!(
        dot_max_n.abs() < tol,
        "placed reversed cylinder dir_max not in tangent plane: dot(dir_max, n) = {dot_max_n}"
    );

    // (e) Unit length and mutually orthogonal.
    let dmin_mag_sq =
        c.dir_min[0] * c.dir_min[0] + c.dir_min[1] * c.dir_min[1] + c.dir_min[2] * c.dir_min[2];
    let dmax_mag_sq =
        c.dir_max[0] * c.dir_max[0] + c.dir_max[1] * c.dir_max[1] + c.dir_max[2] * c.dir_max[2];
    assert!(
        (dmin_mag_sq - 1.0).abs() < 1e-9,
        "placed reversed cylinder dir_min not unit length: |dir_min|² = {dmin_mag_sq}"
    );
    assert!(
        (dmax_mag_sq - 1.0).abs() < 1e-9,
        "placed reversed cylinder dir_max not unit length: |dir_max|² = {dmax_mag_sq}"
    );
    let dot_dirs =
        c.dir_min[0] * c.dir_max[0] + c.dir_min[1] * c.dir_max[1] + c.dir_min[2] * c.dir_max[2];
    assert!(
        dot_dirs.abs() < 1e-9,
        "placed reversed cylinder dir_min and dir_max should be orthogonal: dot = {dot_dirs}"
    );
}

// ---------------------------------------------------------------------------
// surface_normal_at_point — at-point normal (KGQ-ζ, task 3615)
// ---------------------------------------------------------------------------

/// Helper: build a kernel with a centred box(10mm × 10mm × 10mm).
/// Returns `(kernel, box_handle_id)`.
fn box10mm_kernel() -> (OcctKernel, GeometryHandleId) {
    let mut kernel = OcctKernel::new();
    let handle = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(0.01),
            height: Value::Real(0.01),
            depth: Value::Real(0.01),
        })
        .expect("box creation should succeed");
    (kernel, handle.id)
}

/// `surface_normal_at_point` on the +Z face of a box(10mm) centred at origin
/// returns [0, 0, 1] within 1e-9.
///
/// Analytic ground truth: the +Z face is the plane z = +0.005 m with constant
/// outward normal (0, 0, 1).  `ValueOfUV` projects (0, 0, 0.005) exactly onto
/// that plane; `face_outward_unit_normal_at_uv` returns (0, 0, 1) to ~1e-15
/// (planar face → constant Du × Dv; REVERSED-flip gives outward direction).
///
/// The query point px=0, py=0, pz=0.005 lies on the face, so the projection
/// is exact (closed-form, no approximation error).
#[test]
fn surface_normal_at_point_on_top_face_of_box_yields_outward_z_normal() {
    let (mut kernel, box_id) = box10mm_kernel();
    let faces = kernel
        .extract_faces(box_id)
        .expect("extract_faces should succeed for box");
    assert_eq!(faces.len(), 6, "box should have 6 faces");

    // Select the +Z face: centroid normal n_z ≈ +1.
    let top_face = faces
        .iter()
        .copied()
        .find(|&f| {
            kernel
                .face_outward_unit_normal_for_test(f)
                .expect("face_outward_unit_normal_for_test should succeed")[2]
                > 0.9
        })
        .expect("box should have a +Z face");

    // Query point on the +Z face: (0, 0, +5mm = 0.005 m).
    let n = kernel
        .surface_normal_at_point(top_face, 0.0, 0.0, 0.005)
        .expect("surface_normal_at_point should succeed on +Z face of box");

    // (a) Unit length.
    let mag_sq = n[0] * n[0] + n[1] * n[1] + n[2] * n[2];
    assert!(
        (mag_sq - 1.0).abs() < 1e-9,
        "surface_normal_at_point: |n|² = {mag_sq}, expected 1.0"
    );

    // (b) Analytic: outward normal of +Z face is (0, 0, 1).
    assert!(
        n[0].abs() < 1e-9 && n[1].abs() < 1e-9 && (n[2] - 1.0).abs() < 1e-9,
        "surface_normal_at_point on +Z face: expected (0, 0, 1), got {n:?}"
    );
}

/// `GeometryQuery::FaceNormalAt` on the +Z face of a box(10mm) returns
/// `Value::String` parseable as `{"x":≈0, "y":≈0, "z":≈1}`.
///
/// This exercises the `OcctKernel::query()` dispatch arm that wraps
/// `surface_normal_at_point` and encodes the result as the same JSON-Point3
/// wire format used by `FaceNormal` / `EdgeTangent` / `ClosestPointOnShape`.
#[test]
fn geometry_query_face_normal_at_on_top_face_of_box_encodes_z_normal() {
    let (mut kernel, box_id) = box10mm_kernel();
    let faces = kernel
        .extract_faces(box_id)
        .expect("extract_faces should succeed for box");

    // Select the +Z face.
    let top_face = faces
        .iter()
        .copied()
        .find(|&f| {
            kernel
                .face_outward_unit_normal_for_test(f)
                .expect("face_outward_unit_normal_for_test should succeed")[2]
                > 0.9
        })
        .expect("box should have a +Z face");

    // Drive through the GeometryQuery trait path.
    let reply = kernel
        .query(&GeometryQuery::FaceNormalAt {
            handle: top_face,
            px: 0.0,
            py: 0.0,
            pz: 0.005,
        })
        .expect("GeometryQuery::FaceNormalAt should succeed on +Z face");

    // Decode the JSON-Point3 string.
    let json = match reply {
        Value::String(s) => s,
        other => panic!("FaceNormalAt reply should be Value::String, got {other:?}"),
    };
    let parsed: serde_json::Value =
        serde_json::from_str(&json).expect("FaceNormalAt reply should be valid JSON");
    let x = parsed["x"].as_f64().expect("reply JSON should have 'x'");
    let y = parsed["y"].as_f64().expect("reply JSON should have 'y'");
    let z = parsed["z"].as_f64().expect("reply JSON should have 'z'");

    assert!(
        x.abs() < 1e-9 && y.abs() < 1e-9 && (z - 1.0).abs() < 1e-9,
        "FaceNormalAt on +Z face: expected {{x:0, y:0, z:1}}, got {{x:{x}, y:{y}, z:{z}}}"
    );
}
