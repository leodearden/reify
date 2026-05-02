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

#![cfg(has_occt)]

use std::f64::consts::PI;

use reify_kernel_occt::OcctKernel;
use reify_types::{GeometryHandleId, GeometryOp, GeometryQuery, QueryError, Value};

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

/// Parse the z-component out of a `FaceNormal` JSON result:
/// `{"x":<f>,"y":<f>,"z":<f>}`.
fn parse_face_normal_z(kernel: &OcctKernel, face_id: GeometryHandleId) -> f64 {
    match kernel.query(&GeometryQuery::FaceNormal(face_id)) {
        Ok(Value::String(s)) => s
            .split("\"z\":")
            .nth(1)
            .and_then(|tail| tail.trim_end_matches('}').parse::<f64>().ok())
            .unwrap_or(0.0),
        other => panic!("FaceNormal returned unexpected value: {other:?}"),
    }
}

/// Parse all three components out of a `FaceNormal` JSON result.
fn parse_face_normal(kernel: &OcctKernel, face_id: GeometryHandleId) -> [f64; 3] {
    match kernel.query(&GeometryQuery::FaceNormal(face_id)) {
        Ok(Value::String(s)) => {
            // Format: `{"x":<f>,"y":<f>,"z":<f>}`
            let x = s
                .split("\"x\":")
                .nth(1)
                .and_then(|t| t.split(',').next())
                .and_then(|t| t.parse::<f64>().ok())
                .unwrap_or(0.0);
            let y = s
                .split("\"y\":")
                .nth(1)
                .and_then(|t| t.split(',').next())
                .and_then(|t| t.parse::<f64>().ok())
                .unwrap_or(0.0);
            let z = s
                .split("\"z\":")
                .nth(1)
                .and_then(|t| t.trim_end_matches('}').parse::<f64>().ok())
                .unwrap_or(0.0);
            [x, y, z]
        }
        other => panic!("FaceNormal returned unexpected value: {other:?}"),
    }
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
        .find(|&f| parse_face_normal_z(&kernel, f).abs() < 0.5)
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
        Err(QueryError::InvalidHandle(id)) => panic!(
            "expected InvalidHandle({unknown:?}), got InvalidHandle({id:?})"
        ),
        other => panic!("expected Err(InvalidHandle({unknown:?})), got {other:?}"),
    }
}

/// Passing a solid handle (not a face) returns `QueryError::QueryFailed` with
/// a message containing "not a face".
#[test]
fn surface_normal_at_non_face_shape_returns_query_failed_with_not_a_face() {
    let (mut kernel, cyl_id) = cylinder_kernel(5.0, 10.0);
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
    let qfn = parse_face_normal(&kernel, face);

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
        .find(|&f| parse_face_normal_z(&kernel, f).abs() < 0.5)
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
        .find(|&f| parse_face_normal_z(&kernel, f).abs() > 0.99)
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
    assert!(
        c.mean.abs() < tol,
        "cap mean: expected 0, got {}",
        c.mean
    );
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

/// Passing an unknown handle returns `QueryError::InvalidHandle`.
#[test]
fn curvature_at_unknown_handle_returns_invalid_handle() {
    let (kernel, _) = sphere_kernel(5.0);
    let unknown = GeometryHandleId(9999);
    match kernel.curvature_at(unknown, 0.0, 0.0) {
        Err(QueryError::InvalidHandle(id)) if id == unknown => {}
        Err(QueryError::InvalidHandle(id)) => panic!(
            "expected InvalidHandle({unknown:?}), got InvalidHandle({id:?})"
        ),
        other => panic!("expected Err(InvalidHandle({unknown:?})), got {other:?}"),
    }
}

/// Passing a solid handle (not a face) returns `QueryError::QueryFailed` with
/// a message containing "not a face".
#[test]
fn curvature_at_non_face_shape_returns_query_failed_with_not_a_face() {
    let (mut kernel, cyl_id) = cylinder_kernel(5.0, 10.0);
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
