//! Contract tests of the cxx bridge `crate::ffi::ffi`, driven directly beneath `OcctKernel`
//! (which kernel tests live here, in `tests/`, or in `lib.rs`: the doc on `lib.rs`'s `mod tests`).

use crate::ffi;

// --- Query FFI null-topology guards ---

/// Defense-in-depth: each OCCT geometry-query FFI entry point must reject a
/// null-topology shape with a catchable `Err` — never a hardware SIGSEGV —
/// even when called DIRECTLY, bypassing the `get_shape` boundary guard.
/// This pins the C++ IsNull guards independently of the Rust chokepoint, so
/// any future or direct-FFI path that skips `get_shape` still fails safely.
/// Covers the mass-property queries (volume/volume_measurement/centroid/
/// bbox/inertia) plus the surface/linear-property queries
/// (face_centroid/area/edge_length);
/// `query_face_centroid` is the one reached from the production `Centroid`
/// dispatch for Face-repr handles, so its direct-FFI guard closes the last
/// gap the get_shape chokepoint already covers.
///
/// Without its guard, `query_volume` dereferences `ShapeType()` on null
/// topology → SIGSEGV, while `query_centroid`, `query_face_centroid`,
/// `query_area` and `query_edge_length` return `Ok` (origin / mass 0.0).
/// With the guards, each throws `std::runtime_error`, which cxx maps to a
/// catchable `Err`.
///
/// MUST run under `cargo nextest run` (process-per-test isolation) so a
/// missing guard's crash is contained as one failing test, not a suite abort.
#[test]
fn occt_query_ffi_rejects_null_topology_shape() {
    let null_shape = ffi::ffi::make_null_shape_for_test();

    // query_volume is the crash vector an unguarded path hits (ShapeType()
    // fallback over null topology).
    assert!(
        ffi::ffi::query_volume(&null_shape).is_err(),
        "query_volume on null-topology shape must return Err, not crash"
    );
    // Same crash vector, second entry point: both delegate to
    // compute_volume_arm, which is where the IsNull guard lives.
    assert!(
        ffi::ffi::query_volume_measurement(&null_shape).is_err(),
        "query_volume_measurement on null-topology shape must return Err, not crash"
    );
    assert!(
        ffi::ffi::query_centroid(&null_shape).is_err(),
        "query_centroid on null-topology shape must return Err"
    );
    assert!(
        ffi::ffi::query_bbox(&null_shape).is_err(),
        "query_bbox on null-topology shape must return Err"
    );
    assert!(
        ffi::ffi::query_inertia_tensor(&null_shape, 1000.0).is_err(),
        "query_inertia_tensor on null-topology shape must return Err"
    );
    // Surface/linear-property queries: unguarded, these return Ok (origin /
    // mass 0.0) on null topology rather than crashing, so a missing guard
    // here is silent. query_face_centroid is reached from the production Centroid
    // dispatch for Face-repr handles (see GeometryQuery::Centroid dispatch).
    assert!(
        ffi::ffi::query_face_centroid(&null_shape).is_err(),
        "query_face_centroid on null-topology shape must return Err"
    );
    assert!(
        ffi::ffi::query_area(&null_shape).is_err(),
        "query_area on null-topology shape must return Err"
    );
    assert!(
        ffi::ffi::query_edge_length(&null_shape).is_err(),
        "query_edge_length on null-topology shape must return Err"
    );
}

// --- Offset solid / offset surface tests ---

#[test]
fn offset_solid_shape_ffi_contract() {
    let b = ffi::ffi::make_box(10.0, 10.0, 10.0).expect("make_box");
    assert!(
        ffi::ffi::offset_solid_shape(&b, 2.0).is_ok(),
        "outward offset should succeed"
    );
    assert!(
        ffi::ffi::offset_solid_shape(&b, -2.0).is_ok(),
        "inward offset < inradius should succeed"
    );
    assert!(
        ffi::ffi::offset_solid_shape(&b, -100.0).is_err(),
        "inward offset >> inradius should fail (degenerate)"
    );
}

#[test]
fn make_offset_surface_ffi_contract() {
    // 20mm x 10mm rectangle face at z=0 (metres).
    let face = ffi::ffi::make_rectangle_face(0.020, 0.010, 0.0)
        .expect("make_rectangle_face(0.020, 0.010, 0.0) should succeed");
    assert!(
        ffi::ffi::make_offset_surface(&face, 0.002).is_ok(),
        "2mm surface offset should succeed"
    );
    assert!(
        ffi::ffi::make_offset_surface(&face, 0.0).is_err(),
        "zero-distance surface offset should fail (degenerate)"
    );

    // Negative distance: offsets along the face's -normal, opposite the
    // +0.002 case above. Pin the *sign*, not just success, via
    // query_bbox: the result must land at z ≈ -0.002m, catching a
    // regression that mis-signs the normal or silently no-ops on
    // inward/negative offsets.
    let neg = ffi::ffi::make_offset_surface(&face, -0.002)
        .expect("-2mm surface offset should succeed (-normal direction)");
    let bb = ffi::ffi::query_bbox(&neg).expect("query_bbox should succeed");
    let expected_z = -0.002_f64;
    let tol = 0.0005_f64; // 0.5mm, matching offset_surface_e2e's bbox tolerance
    assert!(
        (bb.zmin - expected_z).abs() < tol,
        "-2mm surface offset: bbox.zmin should be ≈-0.002m, got {}",
        bb.zmin
    );
    assert!(
        (bb.zmax - expected_z).abs() < tol,
        "-2mm surface offset: bbox.zmax should be ≈-0.002m, got {}",
        bb.zmax
    );
}

// --- Offset curve tests ---

/// Overload-1 precise signal (FFI level): offsetting a planar circular arc
/// of radius 10mm outward by 2mm yields a concentric arc of radius 12mm.
///
/// Ground truth: the planar offset of a radius-R circular arc is a
/// concentric arc of radius R+d (outward offset); curvature κ = 1/R is
/// exact for circles, so the offset arc's curvature at its angle-0 point
/// (R+d, 0, 0) is exactly 1/(R+d). We assert radius = 1/κ ≈ 12mm within 2%
/// (covers OCCT numerical error). Units are SI metres (10mm = 0.01m),
/// matching `tests/curve_curvature_integration.rs`.
#[test]
fn make_offset_curve_grows_arc_radius_by_distance() {
    use std::f64::consts::FRAC_PI_4;
    // Radius-10mm arc in the XY plane, swept symmetric about angle 0.
    let arc = ffi::ffi::make_arc_wire(
        0.0, 0.0, 0.0, // center at origin
        0.01,          // radius = 10mm
        -FRAC_PI_4, FRAC_PI_4, // symmetric about angle 0
        0.0, 0.0, 1.0, // axis = +Z (XY plane)
    )
    .expect("make_arc_wire should build a radius-10mm arc");

    // Offset outward by 2mm. Positive distance grows the radius (convention).
    let offset = ffi::ffi::make_offset_curve(&arc, 0.002)
        .expect("make_offset_curve should succeed on a planar arc");
    assert!(!offset.is_null(), "offset result should be a non-null shape");

    // Extract the offset arc edge (IsOpenResult → single concentric edge).
    let edges = ffi::ffi::get_edges(&offset).expect("get_edges on offset wire");
    let n_edges = ffi::ffi::shape_vec_len(&edges);
    assert!(n_edges >= 1, "offset wire should contain at least one edge");
    let edge = ffi::ffi::shape_vec_at(&edges, 0).expect("first offset edge");

    // Measure radius at the offset arc's angle-0 point (12mm, 0, 0).
    let kappa = ffi::ffi::curve_curvature_at(&edge, 0.012, 0.0, 0.0)
        .expect("curve_curvature_at on offset arc");
    let radius = 1.0 / kappa.abs();
    let expected = 0.012; // 12mm
    let rel_err = (radius - expected).abs() / expected;
    assert!(
        rel_err <= 0.02,
        "offset arc radius should be 12mm (ratio 1.2) within 2%, \
         got radius={radius} (κ={kappa}, rel_err={rel_err})"
    );
}

/// Overloads 2 & 3 cpp/ffi smoke (FFI level): the directional and
/// on-surface offset variants each build a non-null wire from a planar arc.
///
/// The PRD bar for overloads 2 & 3 is only "non-Undef Curve" — the precise
/// radius signal is overload-1's job. Overload 3 supplies a direction
/// vector; overload 2 supplies a reference surface (a box face).
#[test]
fn make_offset_curve_directional_and_on_surface_smoke() {
    use std::f64::consts::FRAC_PI_4;
    let arc = ffi::ffi::make_arc_wire(
        0.0, 0.0, 0.0, 0.01, -FRAC_PI_4, FRAC_PI_4, 0.0, 0.0, 1.0,
    )
    .expect("make_arc_wire should build a radius-10mm arc");

    // Overload 3: directional offset (direction = +Z).
    let directional = ffi::ffi::make_offset_curve_directional(&arc, 0.002, 0.0, 0.0, 1.0)
        .expect("make_offset_curve_directional should succeed");
    assert!(!directional.is_null(), "directional offset should be non-null");

    // Overload 2: offset on a reference surface (a face of a box).
    let boxx = ffi::ffi::make_box(0.02, 0.02, 0.02).expect("make_box");
    let faces = ffi::ffi::get_faces(&boxx).expect("get_faces on box");
    assert!(ffi::ffi::shape_vec_len(&faces) >= 1, "box should have faces");
    let face = ffi::ffi::shape_vec_at(&faces, 0).expect("first box face");
    let on_surface = ffi::ffi::make_offset_curve_on_surface(&arc, 0.002, &face)
        .expect("make_offset_curve_on_surface should succeed");
    assert!(!on_surface.is_null(), "on-surface offset should be non-null");
}

// --- make_circle_wire tests ---

#[test]
fn make_circle_wire_rejects_degenerate_radius() {
    // Radius of 0 should cause MakeEdge to fail; verify we get an error
    // rather than a silently invalid shape.
    let result = ffi::ffi::make_circle_wire(0.0, 0.0);
    assert!(
        result.is_err(),
        "make_circle_wire(0.0, 0.0) should return Err, got Ok"
    );
}

#[test]
fn make_circle_wire_valid_produces_wire() {
    // A valid radius should produce a usable wire shape without error.
    let wire = ffi::ffi::make_circle_wire(10.0, 0.0);
    assert!(
        wire.is_ok(),
        "make_circle_wire(10.0, 0.0) should succeed, got {:?}",
        wire.err()
    );
}

// --- Extrude tests ---

#[test]
fn extrude_circle_face_volume() {
    // Tests via direct FFI: make_circle_face → make_prism → query_volume.
    // Circle radius=5, extrude distance=10 → volume ≈ π * 5² * 10 = 785.4
    let face = ffi::ffi::make_circle_face(5.0, 0.0)
        .expect("make_circle_face should succeed for radius=5");
    let prism = ffi::ffi::make_prism(&face, 0.0, 0.0, 10.0)
        .expect("make_prism should succeed for circle face");
    let vol =
        ffi::ffi::query_volume(&prism).expect("query_volume should work for extruded circle");
    let expected = std::f64::consts::PI * 25.0 * 10.0; // π * r² * h
    let rel_err = (vol - expected).abs() / expected;
    assert!(
        rel_err < 0.01,
        "expected extrude circle volume ≈ {:.2}, got {:.2} (rel_err={:.4})",
        expected,
        vol,
        rel_err
    );
}

#[test]
fn extrude_negative_distance_same_volume() {
    // Extruding in -Z should produce the same volume as +Z.
    let face = ffi::ffi::make_circle_face(5.0, 0.0).expect("make_circle_face should succeed");
    let prism_pos =
        ffi::ffi::make_prism(&face, 0.0, 0.0, 10.0).expect("make_prism +Z should succeed");
    let vol_pos = ffi::ffi::query_volume(&prism_pos).expect("query_volume +Z should work");

    let face2 = ffi::ffi::make_circle_face(5.0, 0.0)
        .expect("make_circle_face should succeed for negative test");
    let prism_neg =
        ffi::ffi::make_prism(&face2, 0.0, 0.0, -10.0).expect("make_prism -Z should succeed");
    let vol_neg = ffi::ffi::query_volume(&prism_neg).expect("query_volume -Z should work");

    let rel_diff = (vol_pos - vol_neg).abs() / vol_pos;
    assert!(
        rel_diff < 0.01,
        "positive and negative extrude should have same volume, got pos={:.2} neg={:.2}",
        vol_pos,
        vol_neg
    );
}

// --- Revolve FFI tests ---

#[test]
fn make_rect_face_creates_valid_face() {
    // make_rect_face(width=10, height=5, cx=20, cy=0, cz=0) → area ≈ 50
    let face = ffi::ffi::make_rect_face(10.0, 5.0, 20.0, 0.0, 0.0)
        .expect("make_rect_face should succeed");
    let area = ffi::ffi::query_area(&face).expect("query_area should work on rect face");
    let expected = 50.0;
    let rel_err = (area - expected).abs() / expected;
    assert!(
        rel_err < 0.01,
        "expected rect face area ≈ {:.2}, got {:.2} (rel_err={:.4})",
        expected,
        area,
        rel_err
    );
}

#[test]
fn revolve_ffi_circle_face_full_rotation() {
    // Create a circle face in XY plane, rotate 90° around X to put it in XZ plane,
    // translate to offset 20 on X, then revolve around Z axis by 2π → torus.
    // Profile must be in a plane CONTAINING the revolution axis for a solid torus.
    let face = ffi::ffi::make_circle_face(5.0, 0.0).expect("make_circle_face should succeed");
    // Rotate 90° around X axis: XY plane → XZ plane
    let rotated = ffi::ffi::rotate_shape(
        &face,
        1.0,
        0.0,
        0.0, // X axis
        std::f64::consts::FRAC_PI_2,
    )
    .expect("rotate_shape should succeed");
    // Translate 20 along X so centroid is offset from Z axis
    let translated = ffi::ffi::translate_shape(&rotated, 20.0, 0.0, 0.0)
        .expect("translate_shape should succeed");
    let revolved = ffi::ffi::make_revolve(
        &translated,
        0.0,
        0.0,
        0.0, // axis origin
        0.0,
        0.0,
        1.0, // axis direction (Z)
        std::f64::consts::TAU,
    )
    .expect("make_revolve should succeed for full rotation");
    let vol =
        ffi::ffi::query_volume(&revolved).expect("query_volume should work for revolved shape");
    assert!(
        vol > 0.0,
        "revolved circle face should have positive volume, got {}",
        vol
    );
}

#[test]
fn revolve_result_is_solid_not_shell() {
    // Regression test: make_revolve must always return a volumetric Solid,
    // never a Shell. A Shell shape returns 0 or near-0 for volume queries,
    // which would silently produce wrong results.
    let face = ffi::ffi::make_rect_face(4.0, 2.0, 0.0, 0.0, 0.0)
        .expect("make_rect_face should succeed");
    // Rotate to XZ plane and translate to offset 10 from Z axis
    let rotated = ffi::ffi::rotate_shape(&face, 1.0, 0.0, 0.0, std::f64::consts::FRAC_PI_2)
        .expect("rotate_shape should succeed");
    let translated = ffi::ffi::translate_shape(&rotated, 10.0, 0.0, 0.0)
        .expect("translate_shape should succeed");
    // Revolve around Z axis by full rotation
    let revolved = ffi::ffi::make_revolve(
        &translated,
        0.0,
        0.0,
        0.0, // axis origin
        0.0,
        0.0,
        1.0, // axis direction (Z)
        std::f64::consts::TAU,
    )
    .expect("make_revolve should succeed");

    // Volume must be positive (a Shell would give 0 or near-0)
    let vol = ffi::ffi::query_volume(&revolved).expect("query_volume should succeed");
    assert!(
        vol > 1.0,
        "revolve result should be a Solid with positive volume, got {}",
        vol
    );

    // Verify geometric correctness via Pappus' theorem:
    // V = 2π × R × A = 2π × 10 × (4×2) = 160π ≈ 502.65
    let expected = 2.0 * std::f64::consts::PI * 10.0 * (4.0 * 2.0);
    let rel_err = (vol - expected).abs() / expected;
    assert!(
        rel_err < 0.02,
        "expected volume ≈ {:.2} (160π), got {:.2} (rel_err={:.4})",
        expected,
        vol,
        rel_err
    );
}

// --- Torus tests ---

#[test]
fn make_torus_ffi_produces_correct_volume() {
    // Pappus' theorem: a torus with major radius R and minor radius r has
    // analytic volume V = 2π²·R·r². With R=20, r=5 → V = 2π²·20·25.
    // BRepPrimAPI_MakeTorus builds a genuine Geom_ToroidalSurface, so
    // query_volume takes OCCT's analytic BRepGProp::VolumeProperties path
    // (not the mesh fallback, which only triggers when VolumeProperties
    // returns 0 for surfaces of revolution). The result is exact to f64
    // rounding — measured rel_err ≈ 1.8e-16 — so a 1e-9 tolerance pins the
    // major/minor radius wiring tightly while leaving ~7 orders of margin
    // above the machine-epsilon floor for cross-platform variance.
    let shape = ffi::ffi::make_torus(20.0, 5.0).expect("make_torus should succeed");
    let vol = ffi::ffi::query_volume(&shape).expect("query_volume should work on torus");
    let expected = 2.0 * std::f64::consts::PI.powi(2) * 20.0 * 25.0; // 2π²Rr²
    let rel_err = (vol - expected).abs() / expected;
    assert!(
        rel_err < 1e-9,
        "expected torus volume ≈ {:.2}, got {:.2} (rel_err={:e})",
        expected,
        vol,
        rel_err
    );
}

// --- wire_start_tangent FFI tests ---

#[test]
fn ffi_wire_start_tangent_returns_unit_z_for_z_line() {
    let wire = ffi::ffi::make_line_wire(0.0, 0.0, 0.0, 0.0, 0.0, 0.020)
        .expect("make_line_wire should succeed");
    let t = ffi::ffi::wire_start_tangent(&wire)
        .expect("wire_start_tangent should succeed for Z line");
    assert!(
        t.x.abs() < 1e-6,
        "start-tangent x should be ≈ 0 for Z line, got {}",
        t.x
    );
    assert!(
        t.y.abs() < 1e-6,
        "start-tangent y should be ≈ 0 for Z line, got {}",
        t.y
    );
    assert!(
        (t.z - 1.0).abs() < 1e-6,
        "start-tangent z should be ≈ 1 for Z line, got {}",
        t.z
    );
}

#[test]
fn ffi_wire_start_tangent_returns_unit_x_for_x_line() {
    let wire = ffi::ffi::make_line_wire(0.0, 0.0, 0.0, 0.020, 0.0, 0.0)
        .expect("make_line_wire should succeed");
    let t = ffi::ffi::wire_start_tangent(&wire)
        .expect("wire_start_tangent should succeed for X line");
    assert!(
        (t.x - 1.0).abs() < 1e-6,
        "start-tangent x should be ≈ 1 for X line, got {}",
        t.x
    );
    assert!(
        t.y.abs() < 1e-6,
        "start-tangent y should be ≈ 0 for X line, got {}",
        t.y
    );
    assert!(
        t.z.abs() < 1e-6,
        "start-tangent z should be ≈ 0 for X line, got {}",
        t.z
    );
}

/// These two tests specifically exercise `BRepAdaptor_CompCurve` on
/// **multi-edge** wires (two line segments joined end-to-end via
/// `make_polyline_wire`). If a future refactor replaced the composite-curve
/// adaptor with a single-edge `BRepAdaptor_Curve`, it would break these
/// tests while the single-edge tests above would still pass — making the
/// regression immediately detectable.
#[test]
fn ffi_wire_start_tangent_composite_wire_non_z_direction() {
    // Two +X segments: (0,0,0)→(0.010,0,0)→(0.020,0,0).
    // start-tangent should be ≈ +X = (1, 0, 0).
    #[rustfmt::skip]
    let coords: &[f64] = &[
        0.0, 0.0, 0.0,
        0.010, 0.0, 0.0,
        0.020, 0.0, 0.0,
    ];
    let wire = ffi::ffi::make_polyline_wire(coords, 3)
        .expect("make_polyline_wire should succeed for 3-point +X polyline");
    let t = ffi::ffi::wire_start_tangent(&wire)
        .expect("wire_start_tangent should succeed for composite +X wire");
    assert!(
        (t.x - 1.0).abs() < 1e-6,
        "composite +X wire: start-tangent x should be ≈ 1.0, got {}",
        t.x
    );
    assert!(
        t.y.abs() < 1e-6,
        "composite +X wire: start-tangent y should be ≈ 0, got {}",
        t.y
    );
    assert!(
        t.z.abs() < 1e-6,
        "composite +X wire: start-tangent z should be ≈ 0, got {}",
        t.z
    );
}

#[test]
fn ffi_wire_start_tangent_composite_wire_z_direction() {
    // Two +Z segments: (0,0,0)→(0,0,0.010)→(0,0,0.020).
    // start-tangent should be ≈ +Z = (0, 0, 1).
    #[rustfmt::skip]
    let coords: &[f64] = &[
        0.0, 0.0, 0.0,
        0.0, 0.0, 0.010,
        0.0, 0.0, 0.020,
    ];
    let wire = ffi::ffi::make_polyline_wire(coords, 3)
        .expect("make_polyline_wire should succeed for 3-point +Z polyline");
    let t = ffi::ffi::wire_start_tangent(&wire)
        .expect("wire_start_tangent should succeed for composite +Z wire");
    assert!(
        t.x.abs() < 1e-6,
        "composite +Z wire: start-tangent x should be ≈ 0, got {}",
        t.x
    );
    assert!(
        t.y.abs() < 1e-6,
        "composite +Z wire: start-tangent y should be ≈ 0, got {}",
        t.y
    );
    assert!(
        (t.z - 1.0).abs() < 1e-6,
        "composite +Z wire: start-tangent z should be ≈ 1.0, got {}",
        t.z
    );
}

// --- wire_start_point / make_oriented_circle_face FFI tests ---
//
// Direct FFI coverage for the two primitives added to pose the pipe profile
// on the path's start frame. Otherwise both are reached only transitively
// through the `GeometryOp::Pipe` arm, which is blind to two failures: a
// start/end swap produces a plausible solid on a symmetric spine, and the
// radius guard inside `make_oriented_circle_face` is unreachable from that
// arm entirely (it runs `validate_positive_finite` Rust-side first).

/// The line runs (0.005,0,0) → (0.025,0,0): asymmetric about the origin, so
/// the start and end are 0.020 apart and a swap cannot hide inside the
/// tolerance. Pinning the start specifically matters because the profile is
/// *centred* at this point — an end-point result would sweep a solid of the
/// right shape from the wrong place.
#[test]
fn ffi_wire_start_point_returns_start_not_end_of_line() {
    let wire = ffi::ffi::make_line_wire(0.005, 0.0, 0.0, 0.025, 0.0, 0.0)
        .expect("make_line_wire should succeed");
    let p = ffi::ffi::wire_start_point(&wire)
        .expect("wire_start_point should succeed for +X line");
    assert!(
        (p.x - 0.005).abs() < 1e-6,
        "start-point x should be ≈ 0.005 (the START, not the END 0.025), got {}",
        p.x
    );
    assert!(p.y.abs() < 1e-6, "start-point y should be ≈ 0, got {}", p.y);
    assert!(p.z.abs() < 1e-6, "start-point z should be ≈ 0, got {}", p.z);
}

/// Composite-wire counterpart to `ffi_wire_start_tangent_composite_wire_*`,
/// exercising `BRepAdaptor_CompCurve` on a **multi-edge** wire: a future
/// refactor to a single-edge `BRepAdaptor_Curve` would break this while the
/// single-edge test above still passed.
///
/// The polyline is deliberately **non-collinear** — (0.005,0,0) →
/// (0.015,0,0) → (0.015,0.010,0) — which is what lets this test pin the
/// agreement between the two queries rather than just one of them. The
/// profile frame combines *both*: centred at `wire_start_point`, normal
/// along `wire_start_tangent`. If they disambiguated the wire's ends
/// differently the frame would be silently wrong, and here that is visible:
/// at the start the point is (0.005,0,0) with tangent +X, at the far end
/// (0.015,0.010,0) with tangent +Y. Both are asserted together.
#[test]
fn ffi_wire_start_point_composite_wire_agrees_with_start_tangent() {
    #[rustfmt::skip]
    let coords: &[f64] = &[
        0.005, 0.0,   0.0,
        0.015, 0.0,   0.0,
        0.015, 0.010, 0.0,
    ];
    let wire = ffi::ffi::make_polyline_wire(coords, 3)
        .expect("make_polyline_wire should succeed for 3-point L-shaped polyline");

    let p = ffi::ffi::wire_start_point(&wire)
        .expect("wire_start_point should succeed for composite wire");
    assert!(
        (p.x - 0.005).abs() < 1e-6,
        "composite wire: start-point x should be ≈ 0.005, got {}",
        p.x
    );
    assert!(
        p.y.abs() < 1e-6,
        "composite wire: start-point y should be ≈ 0 (the far end has y = 0.010), got {}",
        p.y
    );
    assert!(
        p.z.abs() < 1e-6,
        "composite wire: start-point z should be ≈ 0, got {}",
        p.z
    );

    let t = ffi::ffi::wire_start_tangent(&wire)
        .expect("wire_start_tangent should succeed for composite wire");
    assert!(
        (t.x - 1.0).abs() < 1e-6,
        "composite wire: start-tangent x should be ≈ 1.0 — the FIRST edge runs +X, so a \
         value of ≈ 0 here means the tangent came from a different end than the point did; \
         got {}",
        t.x
    );
    assert!(
        t.y.abs() < 1e-6,
        "composite wire: start-tangent y should be ≈ 0 (the far end's tangent is +Y), got {}",
        t.y
    );
    assert!(
        t.z.abs() < 1e-6,
        "composite wire: start-tangent z should be ≈ 0, got {}",
        t.z
    );
}

/// `gp_Ax2(P, N)` builds a valid orthonormal frame for ANY unit normal, so
/// no direction is privileged and there is no antiparallel singularity —
/// which is the whole reason the profile is posed at construction rather
/// than by a post-hoc rotation carrying +Z onto the tangent. `-Z` is in the
/// table because it is exactly where such a rotation degenerates.
///
/// The centre is off-origin in all three coordinates, so a dropped
/// translation cannot pass. Area is exact — a planar face bounded by an
/// exact `Geom_Circle`, integrated by `BRepGProp::SurfaceProperties` — so it
/// is checked at the same 1e-6 the neighbouring FFI tests use.
#[test]
fn ffi_make_oriented_circle_face_poses_centre_and_normal() {
    let radius = 0.004_f64;
    let (cx, cy, cz) = (0.005, -0.003, 0.011);
    let cases: &[(&str, f64, f64, f64)] = &[
        ("+Z", 0.0, 0.0, 1.0),
        ("+X", 1.0, 0.0, 0.0),
        ("-Z", 0.0, 0.0, -1.0),
    ];

    for &(label, nx, ny, nz) in cases {
        let face = ffi::ffi::make_oriented_circle_face(radius, cx, cy, cz, nx, ny, nz)
            .unwrap_or_else(|e| {
                panic!("make_oriented_circle_face should succeed for normal {label}: {e}")
            });

        let c = ffi::ffi::query_face_centroid(&face)
            .unwrap_or_else(|e| panic!("query_face_centroid should succeed for {label}: {e}"));
        assert!(
            (c.x - cx).abs() < 1e-6,
            "normal {label}: face centroid x should be ≈ {cx}, got {}",
            c.x
        );
        assert!(
            (c.y - cy).abs() < 1e-6,
            "normal {label}: face centroid y should be ≈ {cy}, got {}",
            c.y
        );
        assert!(
            (c.z - cz).abs() < 1e-6,
            "normal {label}: face centroid z should be ≈ {cz}, got {}",
            c.z
        );

        let n = ffi::ffi::query_face_normal(&face)
            .unwrap_or_else(|e| panic!("query_face_normal should succeed for {label}: {e}"));
        assert!(
            (n.x - nx).abs() < 1e-6,
            "normal {label}: face normal x should be ≈ {nx}, got {}",
            n.x
        );
        assert!(
            (n.y - ny).abs() < 1e-6,
            "normal {label}: face normal y should be ≈ {ny}, got {}",
            n.y
        );
        assert!(
            (n.z - nz).abs() < 1e-6,
            "normal {label}: face normal z should be ≈ {nz}, got {}",
            n.z
        );

        let area = ffi::ffi::query_area(&face)
            .unwrap_or_else(|e| panic!("query_area should succeed for {label}: {e}"));
        let expected_area = std::f64::consts::PI * radius * radius;
        let rel_err = (area - expected_area).abs() / expected_area;
        assert!(
            rel_err < 1e-6,
            "normal {label}: face area should be ≈ pi*r^2 = {expected_area}, got {area} \
             (rel_err {rel_err:.3e})",
        );
    }
}

/// The `Pipe` arm runs `validate_positive_finite(r, "pipe radius")` before it
/// ever calls this primitive, so the C++ radius guard is unreachable from its
/// only production caller. This test is the guard's sole coverage: it is
/// pinned here or it is dead code.
#[test]
fn ffi_make_oriented_circle_face_rejects_non_positive_or_nan_radius() {
    for (label, radius) in [
        ("zero", 0.0_f64),
        ("negative", -1.0_f64),
        ("NaN", f64::NAN),
    ] {
        let result = ffi::ffi::make_oriented_circle_face(radius, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0);
        assert!(
            result.is_err(),
            "make_oriented_circle_face should reject {label} radius ({radius}), got Ok"
        );
    }
}

// --- make_line_wire degeneracy threshold tests ---

#[test]
fn ffi_make_line_wire_rejects_sub_epsilon_length() {
    // length = 5e-6 m → dist_sq = 2.5e-11, which is above the old
    // CPP_AXIS_MAG_SQ_MIN (1e-30) but below the new CPP_LINE_WIRE_MIN_LENGTH_SQ
    // (1e-10). Confirms that make_line_wire now rejects degenerate lengths
    // < 1e-5 m (= √(1e-10) m ≈ 10 µm).
    let result = ffi::ffi::make_line_wire(0.0, 0.0, 0.0, 5e-6, 0.0, 0.0);
    // Use .err().expect(...) instead of unwrap_err() because UniquePtr<OcctShape>
    // doesn't implement Debug, so unwrap_err()'s panic message can't format the Ok arm.
    let err = result
        .err()
        .expect("make_line_wire with 5µm segment should return Err, got Ok");
    let msg = format!("{:?}", err);
    assert!(
        msg.contains("distinct"),
        "error message should mention 'distinct', got: {msg}"
    );
}

#[test]
fn cpp_line_wire_floor_matches_rust_const() {
    // Behavioral anchor: verifies the C++ layer honours the Rust-defined
    // CPP_LINE_WIRE_MIN_LENGTH_SQ constant by bracketing the floor tightly.
    // Inputs are derived from the Rust const so this test auto-tracks any
    // future change to the canonical value in src/floor_constants.rs.
    // Multipliers 0.99× / 1.01× give squared distances 0.9801× / 1.0201× of
    // the floor — tight enough that drift >~2% in the C++ floor will fail.
    let floor_len = crate::floor_constants::CPP_LINE_WIRE_MIN_LENGTH_SQ.sqrt();
    // Just below the C++ floor — must be rejected.
    let result = ffi::ffi::make_line_wire(0.0, 0.0, 0.0, floor_len * 0.99, 0.0, 0.0);
    result
        .err()
        .expect("make_line_wire just below CPP floor should return Err, got Ok");
    // Just above the C++ floor — must succeed.
    let result = ffi::ffi::make_line_wire(0.0, 0.0, 0.0, floor_len * 1.01, 0.0, 0.0);
    result.expect("make_line_wire just above CPP floor should succeed");
}

// --- OCCT Precision::Confusion() pin ---

/// Pin `DEFAULT_POINT_ON_SHAPE_TOLERANCE_M` against OCCT's authoritative
/// `Precision::Confusion()` value at runtime.
///
/// `Precision::Confusion()` is defined in OCCT as the `constexpr` literal
/// `1.0e-7`, identical to the Rust constant.  This test would fail if a
/// future OCCT release changed `Precision::Confusion()` away from 1e-7 —
/// drift detection that comparing the constant against its own literal
/// could never give.
/// The comparison uses a relative tolerance of ~4 ULPs so a future OCCT
/// switch from a literal to an arithmetically equivalent computed
/// `constexpr` (same nominal value, possible 1-ulp rounding difference)
/// does not cause a spurious failure — the intent is to catch numerical
/// drift (a genuine change in the nominal value), not bit drift (a sub-ULP
/// rounding artefact at the same nominal scale).
#[test]
fn default_point_on_shape_tolerance_m_pins_occt_precision_confusion() {
    let occt = ffi::ffi::precision_confusion();
    let rust = reify_ir::DEFAULT_POINT_ON_SHAPE_TOLERANCE_M;
    // ~4 ULPs relative tolerance: permits arithmetic rounding if OCCT ever
    // computes `Precision::Confusion()` rather than returning a bare literal,
    // while still loudly catching any genuine change to its nominal value.
    assert!(
        (occt - rust).abs() <= 4.0 * f64::EPSILON * occt.abs(),
        "DEFAULT_POINT_ON_SHAPE_TOLERANCE_M ({rust}) must match OCCT \
         Precision::Confusion() ({occt}) within 4 ULPs"
    );
}

// --- gtransform_shape FFI smoke tests ---

/// Non-uniform scale (diag(1,1,2)) + translation (10,0,0) applied to a centered
/// 2×2×2 box.  Analytically: corner (±1,±1,±1) → (10±1, ±1, ±2).
/// Expected AABB: [9,11] × [-1,1] × [-2,2], each bound within 1e-6.
/// (BRepBndLib::Add adds a gap of ~Precision::Confusion() ≈ 1e-7, so 1e-6 is the
/// tightest reliable tolerance for vertex-level checks through query_bbox.)
#[test]
fn gtransform_shape_non_uniform_scale_and_translation_maps_aabb() {
    let shape = ffi::ffi::make_box(2.0, 2.0, 2.0).expect("make_box should succeed");
    let transformed = ffi::ffi::gtransform_shape(
        &shape,
        // row 0: scale x=1
        1.0, 0.0, 0.0,
        // row 1: scale y=1
        0.0, 1.0, 0.0,
        // row 2: scale z=2
        0.0, 0.0, 2.0,
        // translation
        10.0, 0.0, 0.0,
    )
    .expect("gtransform_shape (non-uniform) should succeed");
    let bb = ffi::ffi::query_bbox(&transformed).expect("query_bbox should succeed");
    let tol = 1e-6;
    assert!(
        (bb.xmin - 9.0).abs() < tol,
        "xmin expected 9.0, got {}", bb.xmin
    );
    assert!(
        (bb.xmax - 11.0).abs() < tol,
        "xmax expected 11.0, got {}", bb.xmax
    );
    assert!(
        (bb.ymin - (-1.0)).abs() < tol,
        "ymin expected -1.0, got {}", bb.ymin
    );
    assert!(
        (bb.ymax - 1.0).abs() < tol,
        "ymax expected 1.0, got {}", bb.ymax
    );
    assert!(
        (bb.zmin - (-2.0)).abs() < tol,
        "zmin expected -2.0, got {}", bb.zmin
    );
    assert!(
        (bb.zmax - 2.0).abs() < tol,
        "zmax expected 2.0, got {}", bb.zmax
    );
}

/// Identity affine (I, 0) must leave the source AABB unchanged within 1e-6.
/// (BRepBndLib::Add adds ~Precision::Confusion() ≈ 1e-7 gap, so 1e-6 is the
/// tightest reliable tolerance through query_bbox.)
#[test]
fn gtransform_shape_identity_preserves_aabb() {
    let shape = ffi::ffi::make_box(2.0, 2.0, 2.0).expect("make_box should succeed");
    let transformed = ffi::ffi::gtransform_shape(
        &shape,
        1.0, 0.0, 0.0,
        0.0, 1.0, 0.0,
        0.0, 0.0, 1.0,
        0.0, 0.0, 0.0,
    )
    .expect("gtransform_shape (identity) should succeed");
    let bb = ffi::ffi::query_bbox(&transformed).expect("query_bbox should succeed");
    let tol = 1e-6;
    assert!(
        (bb.xmin - (-1.0)).abs() < tol,
        "identity xmin expected -1.0, got {}", bb.xmin
    );
    assert!(
        (bb.xmax - 1.0).abs() < tol,
        "identity xmax expected 1.0, got {}", bb.xmax
    );
    assert!(
        (bb.ymin - (-1.0)).abs() < tol,
        "identity ymin expected -1.0, got {}", bb.ymin
    );
    assert!(
        (bb.ymax - 1.0).abs() < tol,
        "identity ymax expected 1.0, got {}", bb.ymax
    );
    assert!(
        (bb.zmin - (-1.0)).abs() < tol,
        "identity zmin expected -1.0, got {}", bb.zmin
    );
    assert!(
        (bb.zmax - 1.0).abs() < tol,
        "identity zmax expected 1.0, got {}", bb.zmax
    );
}

/// Singular linear part (diag(1,1,0), det=0) must return Err containing "singular".
/// BRepBuilderAPI_GTransform silently emits a degenerate shape for det=0 without
/// failing IsDone(), so the explicit determinant guard is what this pins.
#[test]
fn gtransform_shape_singular_linear_returns_error() {
    let shape = ffi::ffi::make_box(2.0, 2.0, 2.0).expect("make_box should succeed");
    let result = ffi::ffi::gtransform_shape(
        &shape,
        // diag(1, 1, 0) — det = 0
        1.0, 0.0, 0.0,
        0.0, 1.0, 0.0,
        0.0, 0.0, 0.0,
        0.0, 0.0, 0.0,
    );
    match result {
        Err(e) => {
            let msg = e.to_string().to_lowercase();
            assert!(
                msg.contains("singular"),
                "expected error containing 'singular', got: {e}"
            );
        }
        Ok(_) => panic!("expected Err for singular linear part (det=0), got Ok"),
    }
}

/// `Copy=true` contract: the source shape AABB must be unchanged after the transform.
/// Applies diag(1,1,2)+(10,0,0) and re-queries the original shape.
#[test]
fn gtransform_shape_does_not_mutate_source_shape() {
    let shape = ffi::ffi::make_box(2.0, 2.0, 2.0).expect("make_box should succeed");
    // Record the original AABB.
    let orig_bb = ffi::ffi::query_bbox(&shape).expect("query_bbox (original) should succeed");
    // Apply a non-trivial transform.
    let _transformed = ffi::ffi::gtransform_shape(
        &shape,
        1.0, 0.0, 0.0,
        0.0, 1.0, 0.0,
        0.0, 0.0, 2.0,
        10.0, 0.0, 0.0,
    )
    .expect("gtransform_shape should succeed");
    // Re-query the ORIGINAL shape — must still be [-1,1]³.
    let post_bb = ffi::ffi::query_bbox(&shape).expect("query_bbox (post-transform) should succeed");
    let tol = 1e-12;
    assert!(
        (post_bb.xmin - orig_bb.xmin).abs() < tol,
        "source xmin mutated: before={}, after={}", orig_bb.xmin, post_bb.xmin
    );
    assert!(
        (post_bb.xmax - orig_bb.xmax).abs() < tol,
        "source xmax mutated: before={}, after={}", orig_bb.xmax, post_bb.xmax
    );
    assert!(
        (post_bb.ymin - orig_bb.ymin).abs() < tol,
        "source ymin mutated: before={}, after={}", orig_bb.ymin, post_bb.ymin
    );
    assert!(
        (post_bb.ymax - orig_bb.ymax).abs() < tol,
        "source ymax mutated: before={}, after={}", orig_bb.ymax, post_bb.ymax
    );
    assert!(
        (post_bb.zmin - orig_bb.zmin).abs() < tol,
        "source zmin mutated: before={}, after={}", orig_bb.zmin, post_bb.zmin
    );
    assert!(
        (post_bb.zmax - orig_bb.zmax).abs() < tol,
        "source zmax mutated: before={}, after={}", orig_bb.zmax, post_bb.zmax
    );
}

// --- Wedge FFI tests ---

/// FFI-level test: make_wedge produces a solid with the expected volume and
/// face count.
///
/// V = depth·height·(width+top_width)/2 = 0.010·0.015·(0.020+0.005)/2
///   = 1.875e-6 m³ (exact for flat-faced OCCT BRepPrimAPI_MakeWedge).
/// Face count = 6: 2 trapezoid end-caps + 4 lateral faces.
#[test]
fn make_wedge_ffi_produces_solid_with_expected_volume_and_face_count() {
    let shape = ffi::ffi::make_wedge(0.020, 0.010, 0.015, 0.005)
        .expect("make_wedge(0.020, 0.010, 0.015, 0.005) should succeed");

    // Volume: V = depth·height·(width+top_width)/2
    let expected_vol: f64 = 0.010 * 0.015 * (0.020 + 0.005) / 2.0; // 1.875e-6 m³
    let vol = ffi::ffi::query_volume(&shape).expect("query_volume should succeed");
    let rel_err = (vol - expected_vol).abs() / expected_vol;
    assert!(
        rel_err < 0.02,
        "wedge volume: expected ≈ {expected_vol:.3e}, got {vol:.3e} (rel_err={rel_err:.4})"
    );

    // Face count: 6 (2 trapezoid caps + 4 lateral faces)
    let faces = ffi::ffi::get_faces(&shape).expect("get_faces should succeed");
    let face_count = ffi::ffi::shape_vec_len(&faces);
    assert_eq!(
        face_count, 6,
        "wedge should have exactly 6 faces, got {face_count}"
    );
}

// --- RectangleProfile FFI tests ---

/// FFI-level test: make_rectangle_face produces a face that can be extruded
/// to a solid with the expected volume.
///
/// Rectangle w=0.020 m, h=0.010 m, extruded by d=0.003 m →
/// V = w·h·d = 0.020·0.010·0.003 = 6.0e-7 m³ (exact for rectangular prism).
#[test]
fn make_rectangle_face_ffi_extrude_volume() {
    let face = ffi::ffi::make_rectangle_face(0.020, 0.010, 0.0)
        .expect("make_rectangle_face(0.020, 0.010, 0.0) should succeed");
    let prism = ffi::ffi::make_prism(&face, 0.0, 0.0, 0.003)
        .expect("make_prism should succeed for rectangle face");
    let vol =
        ffi::ffi::query_volume(&prism).expect("query_volume should work for extruded rect");
    let expected = 0.020_f64 * 0.010 * 0.003; // w·h·d = 6.0e-7 m³
    let rel_err = (vol - expected).abs() / expected;
    assert!(
        rel_err < 0.02,
        "rectangle extrude volume: expected ≈ {expected:.3e}, got {vol:.3e} (rel_err={rel_err:.4})"
    );
}

/// Degenerate input: width=0 must return Err (not panic).
#[test]
fn make_rectangle_face_ffi_zero_width_returns_err() {
    let result = ffi::ffi::make_rectangle_face(0.0, 0.010, 0.0);
    assert!(
        result.is_err(),
        "make_rectangle_face with width=0 should return Err, got Ok"
    );
}

/// Degenerate input: negative height must return Err (not panic).
#[test]
fn make_rectangle_face_ffi_negative_height_returns_err() {
    let result = ffi::ffi::make_rectangle_face(0.020, -0.010, 0.0);
    assert!(
        result.is_err(),
        "make_rectangle_face with height=-0.01 should return Err, got Ok"
    );
}

// --- FFI face builders: polygon + ellipse ---

/// (a) Square polygon face → extrude → volume ≈ 1e-4 m² × 0.005 m = 5e-7 m³.
///
/// Shoelace area of [0,0],[0.01,0],[0.01,0.01],[0,0.01] = 1e-4 m²;
/// prism volume = base_area × height = 1e-4 × 0.005 = 5e-7 m³ (exact for rectilinear mesh).
#[test]
fn make_polygon_face_ffi_square_extrude_volume() {
    #[rustfmt::skip]
    let coords = [0.0_f64, 0.0, 0.01, 0.0, 0.01, 0.01, 0.0, 0.01];
    let face = ffi::ffi::make_polygon_face(&coords, 4, 0.0)
        .expect("make_polygon_face(4-vertex square) should succeed");
    let prism = ffi::ffi::make_prism(&face, 0.0, 0.0, 0.005)
        .expect("make_prism should succeed for polygon square face");
    let vol = ffi::ffi::query_volume(&prism)
        .expect("query_volume should work for extruded polygon square");
    let expected = 1e-4_f64 * 0.005; // area × height = 5e-7 m³
    let rel_err = (vol - expected).abs() / expected;
    assert!(
        rel_err < 0.02,
        "polygon square extrude volume: expected ≈ {expected:.3e}, got {vol:.3e} (rel_err={rel_err:.4})"
    );
}

/// (b) Triangle polygon face → surface area ≈ shoelace = 0.5 × 0.02 × 0.01 = 1e-4 m².
#[test]
fn make_polygon_face_ffi_triangle_surface_area() {
    #[rustfmt::skip]
    let coords = [0.0_f64, 0.0, 0.02, 0.0, 0.0, 0.01];
    let face = ffi::ffi::make_polygon_face(&coords, 3, 0.0)
        .expect("make_polygon_face(triangle) should succeed");
    let area = ffi::ffi::query_area(&face)
        .expect("query_area should work for polygon triangle face");
    let expected = 0.5_f64 * 0.02 * 0.01; // shoelace = 1e-4 m²
    let rel_err = (area - expected).abs() / expected;
    assert!(
        rel_err < 0.02,
        "polygon triangle area: expected ≈ {expected:.3e}, got {area:.3e} (rel_err={rel_err:.4})"
    );
}

/// (c-i) Degenerate: n_points < 3 must return Err.
#[test]
fn make_polygon_face_ffi_too_few_points_returns_err() {
    let coords = [0.0_f64, 0.0, 0.01, 0.0];
    let result = ffi::ffi::make_polygon_face(&coords, 2, 0.0);
    assert!(
        result.is_err(),
        "make_polygon_face with n_points=2 should return Err, got Ok"
    );
}

/// (c-ii) Degenerate: coords.len() != 2 × n_points must return Err.
#[test]
fn make_polygon_face_ffi_wrong_coords_len_returns_err() {
    // 3 points claimed but only 4 coords (should be 6)
    let coords = [0.0_f64, 0.0, 0.01, 0.0];
    let result = ffi::ffi::make_polygon_face(&coords, 3, 0.0);
    assert!(
        result.is_err(),
        "make_polygon_face with coords.len()=4 but n_points=3 should return Err, got Ok"
    );
}

/// (c-iii) Degenerate: 3 collinear points produce a degenerate polygon that
/// BRepBuilderAPI_MakeFace rejects (IsDone() = false → Err).
#[test]
fn make_polygon_face_ffi_collinear_points_returns_err() {
    // All three points on the X-axis → zero area → degenerate face
    let coords = [0.0_f64, 0.0, 0.01, 0.0, 0.02, 0.0];
    let result = ffi::ffi::make_polygon_face(&coords, 3, 0.0);
    assert!(
        result.is_err(),
        "make_polygon_face with 3 collinear points should return Err (degenerate face), got Ok"
    );
}

/// (d) Ellipse face → surface area ≈ π × 0.010 × 0.005 ≈ 1.5708e-4 m².
#[test]
fn make_ellipse_face_ffi_surface_area() {
    let face = ffi::ffi::make_ellipse_face(0.010, 0.005, 0.0)
        .expect("make_ellipse_face(0.010, 0.005) should succeed");
    let area = ffi::ffi::query_area(&face)
        .expect("query_area should work for ellipse face");
    let expected = std::f64::consts::PI * 0.010 * 0.005; // π·a·b ≈ 1.5708e-4 m²
    let rel_err = (area - expected).abs() / expected;
    assert!(
        rel_err < 1e-3,
        "ellipse face area: expected ≈ {expected:.4e}, got {area:.4e} (rel_err={rel_err:.6})"
    );
}

/// (e) Swapped semi-axes → identical area (π·a·b is orientation-invariant).
///
/// make_ellipse_face(0.005, 0.010) must normalize major/minor internally and
/// produce the same area as make_ellipse_face(0.010, 0.005).
#[test]
fn make_ellipse_face_ffi_swapped_axes_same_area() {
    let face_ab = ffi::ffi::make_ellipse_face(0.010, 0.005, 0.0)
        .expect("make_ellipse_face(0.010, 0.005) should succeed");
    let area_ab = ffi::ffi::query_area(&face_ab)
        .expect("query_area for ellipse(a,b) should work");

    let face_ba = ffi::ffi::make_ellipse_face(0.005, 0.010, 0.0)
        .expect("make_ellipse_face(0.005, 0.010) should succeed");
    let area_ba = ffi::ffi::query_area(&face_ba)
        .expect("query_area for ellipse(b,a) should work");

    let rel_diff = (area_ab - area_ba).abs() / area_ab;
    assert!(
        rel_diff < 1e-9,
        "swapped ellipse axes should give same area: a,b={area_ab:.6e} vs b,a={area_ba:.6e} (rel_diff={rel_diff:.2e})"
    );
}

/// (e-ii) Bounding-box orientation test for make_ellipse_face(semi_major < semi_minor).
///
/// When semi_major=0.005 < semi_minor=0.010, the implementation normalises
/// major=max=0.010, minor=min=0.005 and orients the OCCT Ax2 X-direction along
/// Y so that the longer semi-axis lies along the Y axis.  The bounding box must
/// therefore have X half-extent ≈ semi_major = 0.005 and Y half-extent ≈
/// semi_minor = 0.010.  A regression breaking the conditional orientation would
/// swap these extents.
///
/// Requires OCCT (BBox relies on the actual built shape).
#[test]
fn make_ellipse_face_ffi_swapped_axes_bbox_orientation() {
    let semi_major = 0.005_f64;
    let semi_minor = 0.010_f64;
    // semi_major < semi_minor → major axis should be along Y, minor along X
    let face = ffi::ffi::make_ellipse_face(semi_major, semi_minor, 0.0)
        .expect("make_ellipse_face(0.005, 0.010) should succeed");
    let bbox = ffi::ffi::query_bbox(&face).expect("query_bbox should work for ellipse face");
    // The face is centred at origin; half-extents should equal the semi-axis lengths.
    // BRepBndLib adds Precision::Confusion() ≈ 1e-7 padding per side, so 1e-5 is a
    // conservative tolerance that accepts the correct orientation while rejecting the
    // wrong one (axes differ by 2×: 0.005 vs 0.010).
    let x_half = (bbox.xmax - bbox.xmin) / 2.0;
    let y_half = (bbox.ymax - bbox.ymin) / 2.0;
    assert!(
        (x_half - semi_major).abs() < 1e-5,
        "X half-extent should be ≈ semi_major={semi_major:.4e}, got {x_half:.4e} \
         (xmin={:.4e}, xmax={:.4e})",
        bbox.xmin, bbox.xmax
    );
    assert!(
        (y_half - semi_minor).abs() < 1e-5,
        "Y half-extent should be ≈ semi_minor={semi_minor:.4e}, got {y_half:.4e} \
         (ymin={:.4e}, ymax={:.4e})",
        bbox.ymin, bbox.ymax
    );
}

/// (f-i) Non-positive semi-axis → Err.
#[test]
fn make_ellipse_face_ffi_zero_semi_axis_returns_err() {
    let result = ffi::ffi::make_ellipse_face(0.0, 0.005, 0.0);
    assert!(
        result.is_err(),
        "make_ellipse_face with semi_major=0 should return Err, got Ok"
    );

    let result2 = ffi::ffi::make_ellipse_face(0.010, 0.0, 0.0);
    assert!(
        result2.is_err(),
        "make_ellipse_face with semi_minor=0 should return Err, got Ok"
    );
}

/// (f-ii) Non-finite semi-axis → Err.
#[test]
fn make_ellipse_face_ffi_nonfinite_semi_axis_returns_err() {
    let result = ffi::ffi::make_ellipse_face(f64::INFINITY, 0.005, 0.0);
    assert!(
        result.is_err(),
        "make_ellipse_face with semi_major=Inf should return Err, got Ok"
    );

    let result2 = ffi::ffi::make_ellipse_face(0.010, f64::NAN, 0.0);
    assert!(
        result2.is_err(),
        "make_ellipse_face with semi_minor=NaN should return Err, got Ok"
    );
}
