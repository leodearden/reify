//! Integration tests for the analytic-datum kernel queries
//! (`GeometryQuery::FaceAnalyticDatum` / `EdgeAnalyticDatum` /
//! `ShapeLocalTolerance`) — geometric-relations ε.
//!
//! The analytic-datum queries project a face's / edge's underlying analytic
//! surface / curve (`BRepAdaptor_*` → `GeomAbs_*`) to a datum `Value`:
//!   * `GeomAbs_Cylinder` / `GeomAbs_Cone` face → `Value::Axis` (the surface
//!     axis: a point on the axis + the unit axis direction),
//!   * `GeomAbs_Plane` face → `Value::Plane` (a point on the plane + the unit
//!     normal),
//!   * `GeomAbs_Sphere` face → `Value::Point` (the sphere centre),
//!   * `GeomAbs_Line` edge → `Value::Axis`, `GeomAbs_Circle`/`Ellipse` edge →
//!     `Value::Axis` (centre + axis direction).
//!
//! Step-1 (this file's first two tests) is RED until the C++ FFI body
//! (`face_analytic_datum`) + the `FaceAnalyticDatum` dispatch in `lib.rs` land
//! in step-2: the pre-1 scaffolding stub returns
//! `Err(QueryError::QueryFailed("FaceAnalyticDatum: unimplemented"))`, so the
//! `.expect(...)` on the query panics.

#![cfg(has_occt)]

use reify_ir::{GeometryHandleId, GeometryOp, GeometryQuery, Value};
use reify_kernel_occt::OcctKernel;

/// Linear tolerance for on-axis / on-plane point comparisons (metres). OCCT's
/// analytic accessors are exact to machine precision for primitives; 1e-9 is a
/// generous margin well below the kernel confusion floor (~1e-7).
const LIN_TOL: f64 = 1e-9;
/// Direction tolerance for unit-vector component comparisons.
const DIR_TOL: f64 = 1e-9;

// ── Shape builders ───────────────────────────────────────────────────────────

/// Build a kernel with a cylinder (radius, height) along the +Z axis through
/// the origin — `BRepPrimAPI_MakeCylinder`'s default placement. Returns the
/// kernel and the cylinder body handle.
fn cylinder_kernel(radius: f64, height: f64) -> (OcctKernel, GeometryHandleId) {
    let mut kernel = OcctKernel::new();
    let h = kernel
        .execute(&GeometryOp::Cylinder {
            radius: Value::Real(radius),
            height: Value::Real(height),
        })
        .expect("cylinder creation should succeed");
    (kernel, h.id)
}

/// Build a kernel with a box (cube of `side` metres) centred at the origin.
/// Returns the kernel and the box body handle.
fn box_kernel(side: f64) -> (OcctKernel, GeometryHandleId) {
    let mut kernel = OcctKernel::new();
    let h = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(side),
            height: Value::Real(side),
            depth: Value::Real(side),
        })
        .expect("box creation should succeed");
    (kernel, h.id)
}

// ── Value decomposition helpers ──────────────────────────────────────────────

/// Decompose a `Value::Point` (3 numeric components) into `[x, y, z]`.
fn point_xyz(v: &Value) -> [f64; 3] {
    match v {
        Value::Point(comps) if comps.len() == 3 => [
            comps[0].as_f64().expect("point component 0 numeric"),
            comps[1].as_f64().expect("point component 1 numeric"),
            comps[2].as_f64().expect("point component 2 numeric"),
        ],
        other => panic!("expected a 3-component Value::Point, got {other:?}"),
    }
}

/// Decompose a direction-bearing `Value` (`Direction` or dimensionless 3-vector)
/// into `[x, y, z]`.
fn dir_xyz(v: &Value) -> [f64; 3] {
    match v {
        Value::Direction { x, y, z } => [*x, *y, *z],
        Value::Vector(comps) if comps.len() == 3 => [
            comps[0].as_f64().expect("dir component 0 numeric"),
            comps[1].as_f64().expect("dir component 1 numeric"),
            comps[2].as_f64().expect("dir component 2 numeric"),
        ],
        other => panic!("expected Value::Direction or 3-component Vector, got {other:?}"),
    }
}

/// Decompose a `Value::Axis` into `(origin, direction)`.
fn axis_parts(v: &Value) -> ([f64; 3], [f64; 3]) {
    match v {
        Value::Axis { origin, direction } => (point_xyz(origin), dir_xyz(direction)),
        other => panic!("expected Value::Axis, got {other:?}"),
    }
}

/// Decompose a `Value::Plane` into `(origin, normal)`.
fn plane_parts(v: &Value) -> ([f64; 3], [f64; 3]) {
    match v {
        Value::Plane { origin, normal } => (point_xyz(origin), dir_xyz(normal)),
        other => panic!("expected Value::Plane, got {other:?}"),
    }
}

/// Find the unique face of `body` whose `FaceSurfaceKind` equals `kind`.
fn find_face_of_kind(kernel: &mut OcctKernel, body: GeometryHandleId, kind: &str) -> GeometryHandleId {
    let faces = kernel
        .extract_faces(body)
        .expect("extract_faces should succeed");
    faces
        .iter()
        .copied()
        .find(|id| {
            matches!(
                kernel.query(&GeometryQuery::FaceSurfaceKind(*id)),
                Ok(Value::String(ref s)) if s == kind
            )
        })
        .unwrap_or_else(|| panic!("no face of surface-kind {kind:?} found on body {body:?}"))
}

// ── Tests ────────────────────────────────────────────────────────────────────

/// A cylinder's lateral (`GeomAbs_Cylinder`) face projects to a `Value::Axis`
/// whose origin lies on the cylinder's central axis (the +Z line through the
/// origin) and whose direction is parallel to that axis (±Z).
#[test]
fn cylinder_side_face_projects_to_axis_along_cylinder_axis() {
    let (mut kernel, cyl) = cylinder_kernel(0.003, 0.010);
    let side = find_face_of_kind(&mut kernel, cyl, "Cylinder");

    let datum = kernel
        .query(&GeometryQuery::FaceAnalyticDatum(side))
        .expect("FaceAnalyticDatum on a cylindrical face should succeed");

    let (origin, direction) = axis_parts(&datum);

    // The cylinder axis is the +Z line through the origin: any point ON that
    // axis has x = y = 0 (z is free).
    assert!(
        origin[0].abs() < LIN_TOL && origin[1].abs() < LIN_TOL,
        "cylinder-axis datum origin must lie on the Z axis (x=y=0), got {origin:?}"
    );

    // Direction parallel to the cylinder axis (±Z): |z| ≈ 1, x ≈ y ≈ 0.
    assert!(
        direction[0].abs() < DIR_TOL
            && direction[1].abs() < DIR_TOL
            && (direction[2].abs() - 1.0).abs() < DIR_TOL,
        "cylinder-axis datum direction must be parallel to Z (±(0,0,1)), got {direction:?}"
    );
}

/// A box's planar (`GeomAbs_Plane`) top face projects to a `Value::Plane` whose
/// normal is parallel to Z (±(0,0,1)) and whose origin point lies on the plane
/// `z = +side/2`.
#[test]
fn box_planar_top_face_projects_to_plane() {
    let side = 0.010;
    let half = side / 2.0;
    let (mut kernel, body) = box_kernel(side);

    // All six faces of an axis-aligned box are planar; pick the +Z (top) face
    // by its centroid z ≈ +half.
    let faces = kernel
        .extract_faces(body)
        .expect("extract_faces should succeed");
    let top = faces
        .iter()
        .copied()
        .find(|id| {
            let c = kernel
                .query(&GeometryQuery::Centroid(*id))
                .expect("Centroid query should succeed");
            // Centroid is a JSON-encoded {"x":..,"y":..,"z":..} string.
            if let Value::String(s) = c {
                let parsed: serde_json::Value =
                    serde_json::from_str(&s).expect("centroid JSON parses");
                (parsed["z"].as_f64().expect("centroid z") - half).abs() < 1e-6
            } else {
                false
            }
        })
        .expect("box must have a +Z top face at z = +side/2");

    let datum = kernel
        .query(&GeometryQuery::FaceAnalyticDatum(top))
        .expect("FaceAnalyticDatum on a planar face should succeed");

    let (origin, normal) = plane_parts(&datum);

    // Normal parallel to Z (±(0,0,1)).
    assert!(
        normal[0].abs() < DIR_TOL
            && normal[1].abs() < DIR_TOL
            && (normal[2].abs() - 1.0).abs() < DIR_TOL,
        "top-face plane normal must be parallel to Z (±(0,0,1)), got {normal:?}"
    );

    // The plane datum's origin is a point ON the plane z = +half.
    assert!(
        (origin[2] - half).abs() < LIN_TOL,
        "top-face plane datum origin must lie on z = +{half} (got z = {})",
        origin[2]
    );
}
