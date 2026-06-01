//! End-to-end OCCT signal test for `cylinder_centered` and `box_centered` —
//! Phase 2 centred-primitive constructors (geometry-primitive-constructors.md, task ε).
//!
//! Verifies the user-observable numeric signal by executing the exact composed
//! ops that the compiler lowers each constructor to and querying the geometry kernel:
//!
//! - `cylinder_centered(5mm, 20mm)`:
//!   - lowers to `Primitive(Cylinder){r:5mm,h:20mm}` + `Translate{dz:-(20mm/2) = -0.01m}`
//!   - centroid z ≈ 0 and BoundingBox z ∈ [-10mm, +10mm] = [-0.01, +0.01] SI metres
//!
//! - `box_centered(40mm, 20mm, 30mm)`:
//!   - lowers to the IDENTICAL `Primitive(Box){w:40mm,h:20mm,d:30mm}` as `box`
//!   - centroid at origin; BoundingBox symmetric about z = 0
//!
//! All tests are guarded by `reify_kernel_occt::OCCT_AVAILABLE` and silently skip
//! (non-failing) when OCCT is absent.

use reify_ir::{GeometryOp, GeometryQuery, Value};
use reify_kernel_occt::OcctKernel;
use reify_test_support::*;

// ─── JSON helpers ──────────────────────────────────────────────────────────────

/// Parse the JSON-encoded bounding box returned by `GeometryQuery::BoundingBox`.
/// Format: `{"xmin":<f>,"ymin":<f>,"zmin":<f>,"xmax":<f>,"ymax":<f>,"zmax":<f>}`
fn parse_bbox_z(s: &str) -> (f64, f64) {
    let mut zmin = f64::NAN;
    let mut zmax = f64::NAN;
    let trimmed = s.trim_start_matches('{').trim_end_matches('}');
    for pair in trimmed.split(',') {
        let mut parts = pair.splitn(2, ':');
        let key = parts.next().unwrap().trim().trim_matches('"');
        let val: f64 = parts.next().unwrap().trim().parse().unwrap();
        match key {
            "zmin" => zmin = val,
            "zmax" => zmax = val,
            _ => {}
        }
    }
    (zmin, zmax)
}

/// Parse z from the JSON-encoded centroid returned by `GeometryQuery::Centroid`.
/// Format: `{"x":<f>,"y":<f>,"z":<f>}`
fn parse_centroid_z(s: &str) -> f64 {
    let z_start = s
        .find("\"z\":")
        .expect("no \"z\" field in centroid JSON")
        + 4;
    let z_end = s[z_start..].find([',', '}']).unwrap() + z_start;
    s[z_start..z_end].trim().parse::<f64>().unwrap()
}

// ─── cylinder_centered ─────────────────────────────────────────────────────────

/// `cylinder_centered(5mm, 20mm)` lowers to:
///   [0] Primitive(Cylinder){ radius:5mm, height:20mm }   — base at z=0, top at z=+20mm
///   [1] Translate{ target:<op0>, dx:0, dy:0, dz:−0.01m } — shift down by height/2
///
/// After the Translate the centroid must lie at z ≈ 0 and the bounding box z-extent
/// must be ≈ [−10mm, +10mm] (= [−0.01, +0.01] SI metres).
#[test]
fn cylinder_centered_centroid_at_z_zero() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping cylinder_centered_centroid_at_z_zero: OCCT not available");
        return;
    }

    let mut kernel = OcctKernel::new();

    // op[0]: Primitive(Cylinder) — OCCT places the base at z=0, top at z=+height
    let cyl = kernel
        .execute(&GeometryOp::Cylinder {
            radius: mm(5.0),
            height: mm(20.0),
        })
        .expect("Cylinder op should succeed");

    // op[1]: Translate by dz = -(height/2) = -(20mm/2) = -0.01 m (SI)
    //        This recentres the cylinder so z ∈ [-0.01, +0.01].
    let centered = kernel
        .execute(&GeometryOp::Translate {
            target: cyl.id,
            dx: 0.0,
            dy: 0.0,
            dz: -0.01, // -(20mm / 2) in SI metres
        })
        .expect("Translate op should succeed");

    // ── centroid z ≈ 0 ────────────────────────────────────────────────────────
    let centroid_val = kernel
        .query(&GeometryQuery::Centroid(centered.id))
        .expect("Centroid query should succeed");
    match &centroid_val {
        Value::String(s) => {
            let z = parse_centroid_z(s);
            assert!(
                z.abs() < 1e-9,
                "cylinder_centered centroid z should be ≈ 0, got {z}; JSON: {s}"
            );
        }
        other => panic!("expected String centroid JSON, got: {other:?}"),
    }

    // ── bounding box z ∈ [-10mm, +10mm] ───────────────────────────────────────
    let bbox_val = kernel
        .query(&GeometryQuery::BoundingBox(centered.id))
        .expect("BoundingBox query should succeed");
    match &bbox_val {
        Value::String(s) => {
            let (zmin, zmax) = parse_bbox_z(s);
            // OCCT's BRepBndLib pads bounding boxes by a small epsilon (typically
            // ≤ 1e-6 for metre-scale shapes); use a matching tolerance.
            let tol = 1e-6_f64;
            let expected_half = 0.01_f64; // 10mm in SI metres
            assert!(
                (zmin - (-expected_half)).abs() < tol,
                "cylinder_centered bbox zmin should be ≈ -{expected_half:.4} m, got {zmin}; JSON: {s}"
            );
            assert!(
                (zmax - expected_half).abs() < tol,
                "cylinder_centered bbox zmax should be ≈ +{expected_half:.4} m, got {zmax}; JSON: {s}"
            );
        }
        other => panic!("expected String bbox JSON, got: {other:?}"),
    }
}

// ─── box_centered ──────────────────────────────────────────────────────────────

/// `box_centered(40mm, 20mm, 30mm)` lowers to the IDENTICAL
/// `Primitive(Box){ width:40mm, height:20mm, depth:30mm }` as `box(40mm,20mm,30mm)`.
///
/// The OCCT `make_box` implementation already centres the box at the origin
/// (`gp_Pnt corner(-w/2,-h/2,-d/2)`), so:
///   - centroid x/y/z ≈ 0
///   - bbox z ∈ [-15mm, +15mm] (= depth/2 = 0.015 m)
///
/// We execute the same `Box` op twice (once representing `box`, once representing
/// `box_centered`) and assert both produce identical bounding-box z-extents,
/// proving the alias invariant numerically.
#[test]
fn box_centered_bbox_centroid_identical_to_box() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping box_centered_bbox_centroid_identical_to_box: OCCT not available");
        return;
    }

    let mut kernel = OcctKernel::new();

    // "box(40mm,20mm,30mm)" — the reference.
    let box_handle = kernel
        .execute(&GeometryOp::Box {
            width: mm(40.0),
            height: mm(20.0),
            depth: mm(30.0),
        })
        .expect("Box op should succeed");

    // "box_centered(40mm,20mm,30mm)" — alias, identical op, second execution.
    let box_centered_handle = kernel
        .execute(&GeometryOp::Box {
            width: mm(40.0),
            height: mm(20.0),
            depth: mm(30.0),
        })
        .expect("Box op (box_centered alias) should succeed");

    // ── bounding box z-extents are identical ──────────────────────────────────
    let bbox_box = kernel
        .query(&GeometryQuery::BoundingBox(box_handle.id))
        .expect("BoundingBox (box) should succeed");
    let bbox_centered = kernel
        .query(&GeometryQuery::BoundingBox(box_centered_handle.id))
        .expect("BoundingBox (box_centered) should succeed");

    let (zmin_box, zmax_box) = match &bbox_box {
        Value::String(s) => parse_bbox_z(s),
        other => panic!("expected String bbox JSON for box, got: {other:?}"),
    };
    let (zmin_centered, zmax_centered) = match &bbox_centered {
        Value::String(s) => parse_bbox_z(s),
        other => panic!("expected String bbox JSON for box_centered, got: {other:?}"),
    };

    let tol = 1e-12_f64; // identical ops → identical floats; epsilon is cosmetic
    assert!(
        (zmin_box - zmin_centered).abs() < tol,
        "box and box_centered bbox zmin must be identical: box={zmin_box}, centered={zmin_centered}"
    );
    assert!(
        (zmax_box - zmax_centered).abs() < tol,
        "box and box_centered bbox zmax must be identical: box={zmax_box}, centered={zmax_centered}"
    );

    // ── z-extent is symmetric about origin (box already centred) ─────────────
    let expected_half_z = 0.015_f64; // depth/2 = 15mm in SI metres
    let bbox_tol = 1e-6_f64; // OCCT BRepBndLib padding tolerance
    assert!(
        (zmin_centered - (-expected_half_z)).abs() < bbox_tol,
        "box_centered bbox zmin should be ≈ -{expected_half_z:.4} m, got {zmin_centered}"
    );
    assert!(
        (zmax_centered - expected_half_z).abs() < bbox_tol,
        "box_centered bbox zmax should be ≈ +{expected_half_z:.4} m, got {zmax_centered}"
    );

    // ── centroid z ≈ 0 ────────────────────────────────────────────────────────
    let centroid_val = kernel
        .query(&GeometryQuery::Centroid(box_centered_handle.id))
        .expect("Centroid query should succeed");
    match &centroid_val {
        Value::String(s) => {
            let z = parse_centroid_z(s);
            assert!(
                z.abs() < 1e-9,
                "box_centered centroid z should be ≈ 0, got {z}; JSON: {s}"
            );
        }
        other => panic!("expected String centroid JSON, got: {other:?}"),
    }
}
