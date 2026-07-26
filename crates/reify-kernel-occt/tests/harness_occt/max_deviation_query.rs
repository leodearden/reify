//! Integration tests for `GeometryQuery::MaxDeviation` via `OcctKernel::query`.
//!
//! Task 4479 ζ / PRD contract C4 — promotes `measure_mesh_deviation` (built
//! tess-QA-only by done 4198) into a repr-gated `GeometryQuery::MaxDeviation`.
//!
//! All tests require a real OCCT build (`cfg(has_occt)`).
//!
//! # Signal test (S1)
//!
//! Build a unit-box nominal and an `actual` = Translate(box, dx=0.5mm).
//! The true maximum deviation is EXACTLY 0.5 mm = 5e-4 m by construction
//! (box faces are axis-aligned planes; translation in x shifts the x-face
//! normals exactly 0.5 mm away from the nominal x-face).
//!
//! The measured deviation is asserted as an INEQUALITY:
//!   `|dev − 0.5mm| ≤ FLOOR`
//! where `const FLOOR: f64 = 1e-5` (metres).
//!
//! HONEST FLOOR (G6): planar-face triangulation places every interior
//! sample point exactly in the face plane (convex combo of coplanar f32
//! vertices). The f32 quantization error at unit scale is ~1e-6 m
//! (validated by mesh_deviation.rs B1 bound ≤ 1e-5 m). The conservative
//! 1e-5 m test band sits ~2 orders below the 0.5 mm signal — never
//! exactness, never machine-eps. Avoids esc-3453/esc-3770 class.
//!
//! # Error tests
//!
//! - Unknown actual handle → `Err`
//! - Unknown nominal handle → `Err`

#![cfg(has_occt)]

use reify_ir::{GeometryHandleId, GeometryOp, GeometryQuery, Value};
use reify_kernel_occt::OcctKernel;

// ── helpers ──────────────────────────────────────────────────────────────────

fn box_op(w: f64, h: f64, d: f64) -> GeometryOp {
    GeometryOp::Box {
        width: Value::Real(w),
        height: Value::Real(h),
        depth: Value::Real(d),
    }
}

// ── S1: signal — translated box deviates by exactly 0.5 mm from nominal ─────

/// S1 (signal test): a unit-box `actual` translated +0.5 mm along X w.r.t.
/// a unit-box `nominal` produces a MaxDeviation query result ≈ 0.5 mm.
///
/// True deviation = 0.5 mm exactly by construction; f32 quantization on planar
/// faces contributes ≤ 1e-5 m (mesh_deviation.rs B1 bound). The 1e-5 m FLOOR
/// sits ~2 orders below the 0.5 mm signal — `|dev − 5e-4| ≤ 1e-5`.
///
/// This is the user-observable SIGNAL for task 4479 ζ.
/// RED until step-4 replaces the STUB Err arm with the real kernel arm.
#[test]
fn s1_translated_box_max_deviation_is_half_mm() {
    /// Honest floor: f32-quantization on planar faces ≤ 1e-5 m (B1 bound,
    /// validated by mesh_deviation.rs). True ~1 µm floor sits ~2 orders below
    /// the 0.5 mm signal. Conservative band; never exactness or machine-eps.
    const FLOOR: f64 = 1e-5; // metres
    const EXPECTED_DEV: f64 = 0.5e-3; // 0.5 mm = 5e-4 m
    const TOLERANCE: f64 = 1e-4; // tessellation deflection for `actual`

    let mut kernel = OcctKernel::new();

    // Nominal: unit box at the origin.
    let nominal_handle = kernel
        .execute(&box_op(1.0, 1.0, 1.0))
        .expect("nominal box should succeed");
    let nominal = nominal_handle.id;

    // Actual: same unit box translated +0.5 mm in x.
    let actual_handle = kernel
        .execute(&GeometryOp::Translate {
            target: nominal,
            dx: EXPECTED_DEV, // 0.5 mm
            dy: 0.0,
            dz: 0.0,
        })
        .expect("translate should succeed");
    let actual = actual_handle.id;

    // Query MaxDeviation.
    let result = kernel.query(&GeometryQuery::MaxDeviation {
        actual,
        nominal,
        tolerance: TOLERANCE,
    });

    let dev = match result {
        Ok(Value::Real(v)) => v,
        Ok(other) => panic!("expected Value::Real, got {other:?}"),
        Err(e) => panic!("MaxDeviation query returned Err: {e:?}"),
    };

    // B3-numeric
    assert!(dev.is_finite(), "deviation must be finite, got {dev}");
    assert!(dev >= 0.0, "deviation must be ≥ 0, got {dev}");

    // S1: honest-floor inequality — |measured − 0.5 mm| ≤ FLOOR
    let diff = (dev - EXPECTED_DEV).abs();
    assert!(
        diff <= FLOOR,
        "S1: |dev ({dev}) − 0.5mm ({EXPECTED_DEV})| = {diff} must be ≤ FLOOR ({FLOOR} m)"
    );
}

// ── Error path: unknown actual handle ────────────────────────────────────────

/// E1: a MaxDeviation query naming an unknown `actual` handle returns `Err`.
#[test]
fn e1_unknown_actual_handle_returns_err() {
    let mut kernel = OcctKernel::new();
    let nominal_handle = kernel
        .execute(&box_op(1.0, 1.0, 1.0))
        .expect("nominal box should succeed");
    let nominal = nominal_handle.id;
    let bad_actual = GeometryHandleId(9_999_998);

    let result = kernel.query(&GeometryQuery::MaxDeviation {
        actual: bad_actual,
        nominal,
        tolerance: 1e-4,
    });
    assert!(
        result.is_err(),
        "E1: unknown actual handle must return Err, got Ok({:?})",
        result.ok()
    );
}

// ── Error path: unknown nominal handle ───────────────────────────────────────

/// E2: a MaxDeviation query naming an unknown `nominal` handle returns `Err`.
#[test]
fn e2_unknown_nominal_handle_returns_err() {
    let mut kernel = OcctKernel::new();
    let actual_handle = kernel
        .execute(&box_op(1.0, 1.0, 1.0))
        .expect("actual box should succeed");
    let actual = actual_handle.id;
    let bad_nominal = GeometryHandleId(9_999_999);

    let result = kernel.query(&GeometryQuery::MaxDeviation {
        actual,
        nominal: bad_nominal,
        tolerance: 1e-4,
    });
    assert!(
        result.is_err(),
        "E2: unknown nominal handle must return Err, got Ok({:?})",
        result.ok()
    );
}
