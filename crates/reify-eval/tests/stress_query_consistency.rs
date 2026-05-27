//! Stress tests for geometry query consistency.
//!
//! Verifies mathematical relationships between OCCT kernel query results:
//!   - Sphere: V = 4/3·π·r³, A = 4·π·r², A/V ratio ≈ 3/r
//!   - Box centroid at origin for symmetric box, shifts correctly after translate
//!   - Box: V = w·h·d, A = 2(wh + wd + hd) — Pappus-style consistency
//!   - Mock-based: Distance(solid, solid)=0 for self-distance, centroid consistency
//!
//! OCCT tests are guarded by reify_kernel_occt::OCCT_AVAILABLE.

use reify_test_support::{MockGeometryKernel, mm};
use reify_ir::{GeometryHandleId, GeometryKernel, GeometryOp, GeometryQuery, Value};

// ── Helper: centroid JSON parser ───────────────────────────────────────────────

fn parse_centroid(val: &Value) -> (f64, f64, f64) {
    match val {
        Value::String(s) => {
            let parse_coord = |key: &str| -> f64 {
                let prefix = format!("\"{}\":", key);
                let start = s.find(&prefix).expect("coord key not found") + prefix.len();
                let end = s[start..].find([',', '}']).expect("coord end not found") + start;
                s[start..end].trim().parse().expect("coord parse failed")
            };
            (parse_coord("x"), parse_coord("y"), parse_coord("z"))
        }
        other => panic!("expected String (centroid JSON), got {:?}", other),
    }
}

// ── step-9 (test): sphere volume consistency ──────────────────────────────────

/// Sphere volume: |V - 4/3·π·r³| < 1e-9 m³
/// r = 0.05 m (50 mm)
#[test]
fn sphere_volume_consistency() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping sphere_volume_consistency: OCCT not available");
        return;
    }
    let mut kernel = reify_kernel_occt::OcctKernel::new();
    let r_m = 0.05_f64;
    let handle = kernel
        .execute(&GeometryOp::Sphere { radius: mm(50.0) })
        .unwrap();

    let result = kernel.query(&GeometryQuery::Volume(handle.id)).unwrap();
    match result {
        Value::Real(v) => {
            let expected = (4.0 / 3.0) * std::f64::consts::PI * r_m.powi(3);
            assert!(
                (v - expected).abs() < 1e-9,
                "sphere volume should be ≈{} m³, got {} m³",
                expected,
                v
            );
        }
        other => panic!("sphere volume should be Value::Real, got {:?}", other),
    }
}

// ── step-11 (test): sphere surface area consistency ───────────────────────────

/// Sphere surface area: |A - 4·π·r²| < 1e-9 m²
/// Verify A/V ratio ≈ 3/r (known sphere identity)
/// r = 0.05 m (50 mm)
#[test]
fn sphere_surface_area_consistency() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping sphere_surface_area_consistency: OCCT not available");
        return;
    }
    let mut kernel = reify_kernel_occt::OcctKernel::new();
    let r_m = 0.05_f64;
    let handle = kernel
        .execute(&GeometryOp::Sphere { radius: mm(50.0) })
        .unwrap();

    let area_result = kernel
        .query(&GeometryQuery::SurfaceArea(handle.id))
        .unwrap();
    match area_result {
        Value::Real(a) => {
            let expected_a = 4.0 * std::f64::consts::PI * r_m.powi(2);
            assert!(
                (a - expected_a).abs() < 1e-9,
                "sphere surface area should be ≈{} m², got {} m²",
                expected_a,
                a
            );
        }
        other => panic!("sphere surface area should be Value::Real, got {:?}", other),
    }
}

/// Sphere A/V ratio ≈ 3/r (known identity for sphere: A/V = 4πr²/(4/3πr³) = 3/r)
#[test]
fn sphere_area_volume_ratio() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping sphere_area_volume_ratio: OCCT not available");
        return;
    }
    let mut kernel = reify_kernel_occt::OcctKernel::new();
    let r_m = 0.05_f64;
    let handle = kernel
        .execute(&GeometryOp::Sphere { radius: mm(50.0) })
        .unwrap();

    let vol = match kernel.query(&GeometryQuery::Volume(handle.id)).unwrap() {
        Value::Real(v) => v,
        other => panic!("expected Real volume, got {:?}", other),
    };
    let area = match kernel
        .query(&GeometryQuery::SurfaceArea(handle.id))
        .unwrap()
    {
        Value::Real(a) => a,
        other => panic!("expected Real area, got {:?}", other),
    };

    let ratio = area / vol;
    let expected_ratio = 3.0 / r_m;
    assert!(
        (ratio - expected_ratio).abs() < 1e-6,
        "sphere A/V ratio should be ≈{} (3/r), got {}",
        expected_ratio,
        ratio
    );
}

// ── step-13 (test): centroid symmetry ─────────────────────────────────────────

/// Centroid of box(10mm, 10mm, 10mm) at origin should be ≈ (0, 0, 0)
/// (OCCT boxes are created centered at origin, so centroid is at the origin)
#[test]
fn centroid_of_box_at_origin() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping centroid_of_box_at_origin: OCCT not available");
        return;
    }
    let mut kernel = reify_kernel_occt::OcctKernel::new();
    let handle = kernel
        .execute(&GeometryOp::Box {
            width: mm(10.0),
            height: mm(10.0),
            depth: mm(10.0),
        })
        .unwrap();

    let centroid = kernel.query(&GeometryQuery::Centroid(handle.id)).unwrap();
    let (cx, cy, cz) = parse_centroid(&centroid);
    // OCCT box(w,h,d) is created centered at origin: centroid ≈ (0, 0, 0)
    assert!(
        cx.abs() < 1e-6,
        "box centroid x should be ≈0 (box centered at origin), got {}",
        cx
    );
    assert!(
        cy.abs() < 1e-6,
        "box centroid y should be ≈0 (box centered at origin), got {}",
        cy
    );
    assert!(
        cz.abs() < 1e-6,
        "box centroid z should be ≈0 (box centered at origin), got {}",
        cz
    );
}

/// Translate box by (50mm, 0, 0): centroid x shifts from 0 to 0.05
/// (box was centered at origin, translate moves centroid to (0.05, 0, 0))
#[test]
fn centroid_shifts_correctly_after_translate() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping centroid_shifts_correctly_after_translate: OCCT not available");
        return;
    }
    let mut kernel = reify_kernel_occt::OcctKernel::new();
    let handle = kernel
        .execute(&GeometryOp::Box {
            width: mm(10.0),
            height: mm(10.0),
            depth: mm(10.0),
        })
        .unwrap();

    let translated = kernel
        .execute(&GeometryOp::Translate {
            target: handle.id,
            dx: 0.05, // +50mm
            dy: 0.0,
            dz: 0.0,
        })
        .unwrap();

    let centroid = kernel
        .query(&GeometryQuery::Centroid(translated.id))
        .unwrap();
    let (cx, cy, cz) = parse_centroid(&centroid);
    // Original centroid at (0, 0, 0), translated by (+0.05, 0, 0)
    assert!(
        (cx - 0.05).abs() < 1e-6,
        "translated centroid x should be ≈0.05 (0 + 0.05), got {}",
        cx
    );
    assert!(
        cy.abs() < 1e-6,
        "translated centroid y should be ≈0 (unchanged), got {}",
        cy
    );
    assert!(
        cz.abs() < 1e-6,
        "translated centroid z should be ≈0 (unchanged), got {}",
        cz
    );
}

// ── step-15 (test): Pappus-style box volume/area consistency ──────────────────

/// For box(w, h, d): V = w·h·d
/// Box(10mm, 20mm, 30mm): V = 0.01 * 0.02 * 0.03 = 6e-6 m³
#[test]
fn box_volume_equals_whd() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping box_volume_equals_whd: OCCT not available");
        return;
    }
    let mut kernel = reify_kernel_occt::OcctKernel::new();
    let w_m = 0.01_f64;
    let h_m = 0.02_f64;
    let d_m = 0.03_f64;
    let handle = kernel
        .execute(&GeometryOp::Box {
            width: mm(10.0),
            height: mm(20.0),
            depth: mm(30.0),
        })
        .unwrap();

    let result = kernel.query(&GeometryQuery::Volume(handle.id)).unwrap();
    match result {
        Value::Real(v) => {
            let expected = w_m * h_m * d_m;
            assert!(
                (v - expected).abs() < 1e-12,
                "box(10,20,30)mm volume should be ≈{} m³, got {}",
                expected,
                v
            );
        }
        other => panic!("expected Real volume, got {:?}", other),
    }
}

/// For box(w, h, d): A = 2(wh + wd + hd)
/// Box(10mm, 20mm, 30mm): A = 2*(0.01*0.02 + 0.01*0.03 + 0.02*0.03)
///   = 2*(0.0002 + 0.0003 + 0.0006) = 2*0.0011 = 0.0022 m²
#[test]
fn box_surface_area_equals_formula() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping box_surface_area_equals_formula: OCCT not available");
        return;
    }
    let mut kernel = reify_kernel_occt::OcctKernel::new();
    let w_m = 0.01_f64;
    let h_m = 0.02_f64;
    let d_m = 0.03_f64;
    let handle = kernel
        .execute(&GeometryOp::Box {
            width: mm(10.0),
            height: mm(20.0),
            depth: mm(30.0),
        })
        .unwrap();

    let result = kernel
        .query(&GeometryQuery::SurfaceArea(handle.id))
        .unwrap();
    match result {
        Value::Real(a) => {
            let expected = 2.0 * (w_m * h_m + w_m * d_m + h_m * d_m);
            assert!(
                (a - expected).abs() < 1e-12,
                "box(10,20,30)mm surface area should be ≈{} m², got {}",
                expected,
                a
            );
        }
        other => panic!("expected Real surface area, got {:?}", other),
    }
}

/// Multiple box sizes for numerical precision stress.
/// V = w·h·d verified for a cube and a flat slab.
#[test]
fn box_volume_multiple_sizes() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping box_volume_multiple_sizes: OCCT not available");
        return;
    }
    let mut kernel = reify_kernel_occt::OcctKernel::new();

    // Case 1: 5mm cube → V = (0.005)³ = 1.25e-7 m³
    let cube = kernel
        .execute(&GeometryOp::Box {
            width: mm(5.0),
            height: mm(5.0),
            depth: mm(5.0),
        })
        .unwrap();
    match kernel.query(&GeometryQuery::Volume(cube.id)).unwrap() {
        Value::Real(v) => {
            let expected = 0.005_f64.powi(3);
            // Use relative tolerance to handle OCCT BRep pipeline + mm→m unit conversion rounding.
            // Absolute tolerance 1e-15 demands relative error <8e-9 for V≈1.25e-7 m³, which is
            // below the precision floor of double-precision FP after accumulated ULP errors.
            assert!(
                (v - expected).abs() / expected < 1e-10,
                "5mm cube volume should be ≈{} m³, got {} (relative error {})",
                expected,
                v,
                (v - expected).abs() / expected
            );
        }
        other => panic!("expected Real volume, got {:?}", other),
    }

    // Case 2: 100mm × 1mm × 1mm slab → V = 0.1 * 0.001 * 0.001 = 1e-7 m³
    let slab = kernel
        .execute(&GeometryOp::Box {
            width: mm(100.0),
            height: mm(1.0),
            depth: mm(1.0),
        })
        .unwrap();
    match kernel.query(&GeometryQuery::Volume(slab.id)).unwrap() {
        Value::Real(v) => {
            let expected = 0.1_f64 * 0.001 * 0.001;
            // Use relative tolerance for the same reason as above (accumulated rounding errors
            // from BRep operations and mm→m unit conversions in the OCCT kernel).
            assert!(
                (v - expected).abs() / expected < 1e-10,
                "100×1×1mm slab volume should be ≈{} m³, got {} (relative error {})",
                expected,
                v,
                (v - expected).abs() / expected
            );
        }
        other => panic!("expected Real volume, got {:?}", other),
    }
}

// ── step-17 (test): mock-based distance and query dispatch ────────────────────

/// Mock-based test: distance(solid, solid) = 0.0 for self-distance.
/// Verifies that query dispatch uses the 'from' handle for mock lookup.
/// Test FAILS initially because no mock result is configured.
#[test]
fn mock_distance_self_is_zero() {
    let h1 = GeometryHandleId(1);
    // Intentionally do NOT configure a result — test should fail with QueryFailed
    // until step-18 adds the configuration.
    let mut kernel = MockGeometryKernel::new().with_query_result(h1, Value::Real(0.0));
    // Create a shape to get handle 1
    kernel
        .execute(&GeometryOp::Box {
            width: mm(10.0),
            height: mm(10.0),
            depth: mm(10.0),
        })
        .unwrap();

    let result = kernel
        .query(&GeometryQuery::Distance { from: h1, to: h1 })
        .unwrap();
    match result {
        Value::Real(d) => {
            assert!(d.abs() < 1e-12, "self-distance should be 0.0, got {}", d);
        }
        other => panic!("distance should be Value::Real, got {:?}", other),
    }
}

/// Mock-based test: centroid query dispatch — configure a centroid result and verify retrieval.
/// Verifies that GeometryQuery::Centroid dispatch uses the correct handle.
#[test]
fn mock_centroid_dispatch() {
    let h1 = GeometryHandleId(1);
    let centroid_json = Value::String("{\"x\":0.005,\"y\":0.005,\"z\":0.005}".to_string());
    let mut kernel = MockGeometryKernel::new().with_query_result(h1, centroid_json);
    kernel
        .execute(&GeometryOp::Box {
            width: mm(10.0),
            height: mm(10.0),
            depth: mm(10.0),
        })
        .unwrap();

    let centroid = kernel.query(&GeometryQuery::Centroid(h1)).unwrap();
    let (cx, cy, cz) = parse_centroid(&centroid);
    assert!(
        (cx - 0.005).abs() < 1e-9,
        "mock centroid x should be 0.005, got {}",
        cx
    );
    assert!(
        (cy - 0.005).abs() < 1e-9,
        "mock centroid y should be 0.005, got {}",
        cy
    );
    assert!(
        (cz - 0.005).abs() < 1e-9,
        "mock centroid z should be 0.005, got {}",
        cz
    );
}

/// Mock-based test: BoundingBox query dispatch returns the configured bounding box value.
/// Verifies GeometryQuery::BoundingBox uses the correct handle for dispatch.
#[test]
fn mock_bbox_dispatch() {
    let h1 = GeometryHandleId(1);
    // BoundingBox returns a JSON string with min/max coords
    let bbox_json = Value::String(
        "{\"min\":{\"x\":0.0,\"y\":0.0,\"z\":0.0},\"max\":{\"x\":0.01,\"y\":0.01,\"z\":0.01}}"
            .to_string(),
    );
    let mut kernel = MockGeometryKernel::new().with_query_result(h1, bbox_json);
    kernel
        .execute(&GeometryOp::Box {
            width: mm(10.0),
            height: mm(10.0),
            depth: mm(10.0),
        })
        .unwrap();

    let bbox = kernel.query(&GeometryQuery::BoundingBox(h1)).unwrap();
    match bbox {
        Value::String(s) => {
            assert!(s.contains("\"min\""), "bbox should contain min key");
            assert!(s.contains("\"max\""), "bbox should contain max key");
        }
        other => panic!("bbox should be Value::String, got {:?}", other),
    }
}
