//! Primitive solids and planar profiles through `OcctKernel::execute`: tube, torus, cone,
//! wedge and compound, and the rectangle, circle, polygon and ellipse profiles.

#![cfg(has_occt)]

use crate::support::assert_operation_fails_with;
use reify_ir::{
    BRepKind, ExportFormat, GeometryError, GeometryHandleId, GeometryOp, GeometryQuery, Value,
};
use reify_kernel_occt::OcctKernel;

/// Assert that the volume of the shape at `handle_id` is within `tolerance`
/// relative error of `expected`. Panics with a descriptive message including `label`.
///
/// Twin: `src/lib.rs`'s in-file `mod tests` holds a byte-identical copy
/// for the private-reach tests that stayed. Edit both until #8659 moves
/// those tests and deletes that copy.
fn assert_volume_near(
    kernel: &mut OcctKernel,
    handle_id: GeometryHandleId,
    expected: f64,
    tolerance: f64,
    label: &str,
) {
    let vol = kernel
        .query(&GeometryQuery::Volume(handle_id))
        .expect("Volume query should succeed")
        .as_f64()
        .expect("Volume should be numeric");
    let rel_err = (vol - expected).abs() / expected;
    assert!(
        rel_err < tolerance,
        "{label}: expected volume \u{2248} {expected:.2}, got {vol:.2} (rel_err={rel_err:.4})"
    );
}

// --- Tube tests (task-324) ---

#[test]
fn kernel_tube_volume_matches_pi_r2_minus_r2_h() {
    // Volume formula: π*(R² - r²)*h. With R=0.010, r=0.005, h=0.020 →
    // π*(1e-4 - 2.5e-5)*0.020 ≈ 4.712e-6 m³.
    let mut kernel = OcctKernel::new();
    let handle = kernel
        .execute(&GeometryOp::Tube {
            outer_r: Value::Real(0.010),
            inner_r: Value::Real(0.005),
            height: Value::Real(0.020),
        })
        .expect("Tube execute should succeed");

    let vol = kernel
        .query(&GeometryQuery::Volume(handle.id))
        .expect("Volume query should succeed");
    let v = vol.as_f64().expect("Volume should be numeric");
    let expected = std::f64::consts::PI * (0.010_f64.powi(2) - 0.005_f64.powi(2)) * 0.020;
    let rel_err = (v - expected).abs() / expected;
    assert!(
        rel_err < 0.01,
        "tube volume should be ≈ {expected:.3e} m³, got {v:.3e} (rel_err={rel_err:.4})"
    );
}

#[test]
fn kernel_tube_inner_ge_outer_returns_error() {
    // inner_r >= outer_r must be rejected with a message mentioning "inner".
    let mut kernel = OcctKernel::new();
    let result = kernel.execute(&GeometryOp::Tube {
        outer_r: Value::Real(0.005),
        inner_r: Value::Real(0.010),
        height: Value::Real(0.020),
    });
    match result {
        Err(GeometryError::OperationFailed(msg)) => {
            assert!(
                msg.contains("inner"),
                "expected error mentioning 'inner', got: {msg}"
            );
        }
        Ok(_) => panic!("expected error for inner_r > outer_r"),
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
    }
}

#[test]
fn kernel_tube_non_positive_dimensions_return_error() {
    // Each of outer_r=0, inner_r=0, height=0, outer_r=-1 must be rejected.
    let cases: &[(f64, f64, f64, &str)] = &[
        (0.0, 0.001, 0.010, "outer_r=0"),
        (0.005, 0.0, 0.010, "inner_r=0"),
        (0.005, 0.001, 0.0, "height=0"),
        (-1.0, 0.001, 0.010, "outer_r=-1"),
    ];
    for (outer, inner, height, label) in cases {
        let mut kernel = OcctKernel::new();
        let result = kernel.execute(&GeometryOp::Tube {
            outer_r: Value::Real(*outer),
            inner_r: Value::Real(*inner),
            height: Value::Real(*height),
        });
        match result {
            Err(GeometryError::OperationFailed(_)) => {}
            Ok(_) => panic!("expected error for {label}, got Ok"),
            Err(other) => panic!("expected OperationFailed for {label}, got {:?}", other),
        }
    }
}

#[test]
fn torus_execute_volume() {
    // OcctKernel::execute(&GeometryOp::Torus{..}) → exact analytic torus.
    // V = 2π²·R·r² for R=20, r=5. The kernel's Volume query routes through
    // the same analytic VolumeProperties path as the FFI test, so the
    // result is machine-exact (measured rel_err ≈ 1.8e-16); a 1e-9 tolerance
    // pins the radius wiring with ample margin above the f64 floor.
    let mut kernel = OcctKernel::new();
    let handle = kernel
        .execute(&GeometryOp::Torus {
            major_radius: Value::Real(20.0),
            minor_radius: Value::Real(5.0),
        })
        .expect("Torus execute should succeed");
    let expected = 2.0 * std::f64::consts::PI.powi(2) * 20.0 * 25.0; // 2π²Rr²
    assert_volume_near(&mut kernel, handle.id, expected, 1e-9, "torus");
}

#[test]
fn torus_minor_ge_major_returns_error() {
    // minor_radius >= major_radius is a self-intersecting torus → rejected
    // with a message naming the strict-inequality contract.
    let mut kernel = OcctKernel::new();
    let result = kernel.execute(&GeometryOp::Torus {
        major_radius: Value::Real(5.0),
        minor_radius: Value::Real(20.0),
    });
    assert_operation_fails_with(result, "minor radius must be strictly less than major");
}

#[test]
fn torus_nonfinite_or_zero_radius_returns_error() {
    // Zero, negative, NaN, and +Inf radii are each rejected by
    // validate_positive_finite (which requires `is_finite() && > 0.0`),
    // before the FFI is reached — for both the major and minor radius.
    let cases: &[(f64, f64, &str)] = &[
        (0.0, 5.0, "major=0"),
        (20.0, 0.0, "minor=0"),
        (-1.0, 5.0, "major<0"),
        (20.0, -1.0, "minor<0"),
        (f64::NAN, 5.0, "major=NaN"),
        (20.0, f64::NAN, "minor=NaN"),
        (f64::INFINITY, 5.0, "major=+Inf"),
        (20.0, f64::INFINITY, "minor=+Inf"),
    ];
    for (major, minor, label) in cases {
        let mut kernel = OcctKernel::new();
        let result = kernel.execute(&GeometryOp::Torus {
            major_radius: Value::Real(*major),
            minor_radius: Value::Real(*minor),
        });
        match result {
            Err(GeometryError::OperationFailed(_)) => {}
            Ok(_) => panic!("expected error for {label}, got Ok"),
            Err(other) => panic!("expected OperationFailed for {label}, got {:?}", other),
        }
    }
}

// --- ScaleNonUniform tests (task 4167 step-1) ---

/// step-1 (RED) — `make_compound` primitive.
///
/// Gated on `has_occt` because compound assembly and STEP export
/// require the real OCCT kernel.
///
/// Asserts:
/// (a) `make_compound(&[id_a, id_b])` returns `Ok` with a handle whose
///     `repr` field is `Some(BRepKind::Compound)`.
/// (b) Exporting that compound handle to `ExportFormat::Step` yields bytes
///     that contain exactly 2 `MANIFOLD_SOLID_BREP` occurrences — the two
///     input box solids appear as independent solid manifolds inside the
///     compound STEP document.
/// (c) `make_compound(&[])` returns `Err` (empty input is nonsensical).
///
/// Fails to compile until step-2 adds `GeometryKernel::make_compound` and
/// the OCCT override.
#[cfg(has_occt)]
#[test]
fn make_compound_two_boxes_and_empty_err() {
    let mut kernel = OcctKernel::new();

    // Build two distinct box handles.
    let ha = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::length(0.02),
            size_y: Value::length(0.02),
            size_z: Value::length(0.02),
        })
        .expect("Box A must succeed");
    let hb = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::length(0.02),
            size_y: Value::length(0.02),
            size_z: Value::length(0.02),
        })
        .expect("Box B must succeed");

    // (a) make_compound returns Ok and the result repr is Compound.
    let compound = kernel
        .make_compound(&[ha.id, hb.id])
        .expect("make_compound of two boxes must succeed");
    assert_eq!(
        compound.repr,
        Some(BRepKind::Compound),
        "make_compound result must have BRepKind::Compound"
    );

    // (b) Exporting the compound to STEP must contain exactly 2 MANIFOLD_SOLID_BREPs.
    let mut step_bytes: Vec<u8> = Vec::new();
    kernel
        .export(compound.id, ExportFormat::Step, &mut step_bytes)
        .expect("STEP export of compound must succeed");
    let step_text = String::from_utf8_lossy(&step_bytes);
    let solid_count = step_text.matches("MANIFOLD_SOLID_BREP").count();
    assert_eq!(
        solid_count,
        2,
        "STEP compound must contain exactly 2 MANIFOLD_SOLID_BREP entities, got {}; \
         STEP snippet: {:.200}",
        solid_count,
        &step_text
    );

    // (c) make_compound(&[]) must return Err.
    let empty_result = kernel.make_compound(&[]);
    assert!(
        empty_result.is_err(),
        "make_compound with empty input must return Err"
    );
}

/// step-7 (RED) — `make_compound` with the SAME handle twice yields 2 solids.
///
/// Pins `make_compound` as the owner of the distinct-TShape requirement,
/// independent of the `apply_transform_to_shape` path.
///
/// When the same box handle is added to a compound twice via bare
/// `builder.Add`, the two members share one underlying `TShape`.  The STEP
/// writer deduplicates by `TShape` and emits only **1** `MANIFOLD_SOLID_BREP`
/// for the entire compound.  After step-8's per-input `BRepBuilderAPI_Copy`
/// the two copies are distinct `TShape`s → **2** `MANIFOLD_SOLID_BREP`.
///
/// RED on base: `builder.Add(compound, shape)` twice with the same shape →
/// STEP deduplication → 1 `MANIFOLD_SOLID_BREP`, not 2.
#[cfg(has_occt)]
#[test]
fn make_compound_same_handle_twice_yields_two_solids() {
    let mut kernel = OcctKernel::new();

    // Build ONE box handle — used as BOTH inputs to make_compound.
    let h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::length(0.02),
            size_y: Value::length(0.02),
            size_z: Value::length(0.02),
        })
        .expect("Box must succeed");

    // Pass the same handle id twice — both compound members share one TShape.
    let compound = kernel
        .make_compound(&[h.id, h.id])
        .expect("make_compound of same handle twice must succeed");

    // Export to STEP and assert exactly 2 distinct solid entities.
    let mut step_bytes: Vec<u8> = Vec::new();
    kernel
        .export(compound.id, ExportFormat::Step, &mut step_bytes)
        .expect("STEP export of same-handle compound must succeed");
    let step_text = String::from_utf8_lossy(&step_bytes);
    let solid_count = step_text.matches("MANIFOLD_SOLID_BREP").count();
    assert_eq!(
        solid_count,
        2,
        "STEP compound of the same handle twice must contain exactly 2 \
         MANIFOLD_SOLID_BREP entities (per-input deep-copy prevents TShape \
         deduplication); got {};\nSTEP snippet: {:.400}",
        solid_count,
        &step_text
    );
}

// --- Cone tests (task-4156) ---

#[test]
#[cfg(has_occt)]
fn kernel_cone_frustum_volume_matches_closed_form() {
    // Frustum: bottom_r=0.010, top_r=0.005, height=0.020.
    // Volume = (π/3)·h·(r1²+r1·r2+r2²)
    //        = (π/3)·0.020·(1e-4 + 5e-5 + 2.5e-5) ≈ 3.665e-6 m³.
    let mut kernel = OcctKernel::new();
    let handle = kernel
        .execute(&GeometryOp::Cone {
            bottom_radius: Value::Real(0.010),
            top_radius: Value::Real(0.005),
            height: Value::Real(0.020),
        })
        .expect("Cone frustum execute should succeed");

    let r1: f64 = 0.010;
    let r2: f64 = 0.005;
    let h: f64 = 0.020;
    let expected = (std::f64::consts::PI / 3.0) * h * (r1 * r1 + r1 * r2 + r2 * r2);
    assert_volume_near(&mut kernel, handle.id, expected, 0.02, "cone frustum");
}

#[test]
#[cfg(has_occt)]
fn kernel_cone_pointed_volume_matches_closed_form() {
    // Pointed cone: bottom_r=0.010, top_r=0.0, height=0.020.
    // Volume = (π/3)·h·r1² ≈ 2.094e-6 m³.
    let mut kernel = OcctKernel::new();
    let handle = kernel
        .execute(&GeometryOp::Cone {
            bottom_radius: Value::Real(0.010),
            top_radius: Value::Real(0.0),
            height: Value::Real(0.020),
        })
        .expect("Pointed cone execute should succeed");

    let r1: f64 = 0.010;
    let h: f64 = 0.020;
    let expected = (std::f64::consts::PI / 3.0) * h * r1 * r1;
    assert_volume_near(&mut kernel, handle.id, expected, 0.02, "cone pointed");
}

#[test]
#[cfg(has_occt)]
fn kernel_cone_negative_radius_returns_error() {
    let mut kernel = OcctKernel::new();
    let result = kernel.execute(&GeometryOp::Cone {
        bottom_radius: Value::Real(-0.001),
        top_radius: Value::Real(0.005),
        height: Value::Real(0.020),
    });
    match result {
        Err(GeometryError::OperationFailed(_)) => {}
        Ok(_) => panic!("expected error for negative bottom_radius"),
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
    }
}

#[test]
#[cfg(has_occt)]
fn kernel_cone_both_zero_radii_returns_error() {
    let mut kernel = OcctKernel::new();
    let result = kernel.execute(&GeometryOp::Cone {
        bottom_radius: Value::Real(0.0),
        top_radius: Value::Real(0.0),
        height: Value::Real(0.020),
    });
    match result {
        Err(GeometryError::OperationFailed(_)) => {}
        Ok(_) => panic!("expected error for both-zero radii (degenerate line)"),
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
    }
}

#[test]
#[cfg(has_occt)]
fn kernel_cone_negative_top_radius_returns_error() {
    let mut kernel = OcctKernel::new();
    let result = kernel.execute(&GeometryOp::Cone {
        bottom_radius: Value::Real(0.010),
        top_radius: Value::Real(-0.001),
        height: Value::Real(0.020),
    });
    match result {
        Err(GeometryError::OperationFailed(_)) => {}
        Ok(_) => panic!("expected error for negative top_radius"),
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
    }
}

#[test]
#[cfg(has_occt)]
fn kernel_cone_non_positive_height_returns_error() {
    let mut kernel = OcctKernel::new();
    // height == 0 is a degenerate flat disk — must be rejected.
    let result = kernel.execute(&GeometryOp::Cone {
        bottom_radius: Value::Real(0.010),
        top_radius: Value::Real(0.005),
        height: Value::Real(0.0),
    });
    match result {
        Err(GeometryError::OperationFailed(_)) => {}
        Ok(_) => panic!("expected error for zero height"),
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
    }
}

// --- Wedge kernel-execute tests (task-4158, step-3) ---

/// Kernel execute: wedge(20mm,10mm,15mm,5mm) → Ok; volume ≈ 1.875e-6 m³
/// within 2%; exactly 6 faces.
///
/// V = depth·height·(width+top_width)/2 = 0.010·0.015·(0.020+0.005)/2 = 1.875e-6 m³.
/// Face count 6 = 2 trapezoid end-caps + 4 lateral faces.
///
/// RED until step-4 adds GeometryOp::Wedge and wires the execute arm.
#[test]
#[cfg(has_occt)]
fn kernel_wedge_volume_and_face_count_match_expected() {
    let mut kernel = OcctKernel::new();
    let handle = kernel
        .execute(&GeometryOp::Wedge {
            width: Value::Real(0.020),
            depth: Value::Real(0.010),
            height: Value::Real(0.015),
            top_width: Value::Real(0.005),
        })
        .expect("Wedge execute should succeed");

    let expected = 0.010_f64 * 0.015 * (0.020 + 0.005) / 2.0; // 1.875e-6 m³
    assert_volume_near(&mut kernel, handle.id, expected, 0.02, "wedge 20×10×15 top5");

    let face_ids = kernel
        .extract_faces(handle.id)
        .expect("extract_faces on wedge should succeed");
    assert_eq!(
        face_ids.len(),
        6,
        "wedge should have exactly 6 faces, got {}",
        face_ids.len()
    );
}

/// top_width=0 is valid (degenerate triangular-prism wedge — analogous to
/// cone with top_radius=0). Should succeed with 5 faces.
#[test]
#[cfg(has_occt)]
fn kernel_wedge_zero_top_width_degenerate_succeeds() {
    let mut kernel = OcctKernel::new();
    let result = kernel.execute(&GeometryOp::Wedge {
        width: Value::Real(0.020),
        depth: Value::Real(0.010),
        height: Value::Real(0.015),
        top_width: Value::Real(0.0),
    });
    assert!(
        result.is_ok(),
        "top_width=0 should produce a valid degenerate wedge, got {:?}",
        result.err()
    );
}

/// Negative width must be rejected with OperationFailed.
#[test]
fn kernel_wedge_negative_width_returns_error() {
    let mut kernel = OcctKernel::new();
    let result = kernel.execute(&GeometryOp::Wedge {
        width: Value::Real(-0.001),
        depth: Value::Real(0.010),
        height: Value::Real(0.015),
        top_width: Value::Real(0.005),
    });
    match result {
        Err(GeometryError::OperationFailed(_)) => {}
        Ok(_) => panic!("expected error for negative width"),
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
    }
}

/// Zero depth must be rejected.
#[test]
fn kernel_wedge_zero_depth_returns_error() {
    let mut kernel = OcctKernel::new();
    let result = kernel.execute(&GeometryOp::Wedge {
        width: Value::Real(0.020),
        depth: Value::Real(0.0),
        height: Value::Real(0.015),
        top_width: Value::Real(0.005),
    });
    match result {
        Err(GeometryError::OperationFailed(_)) => {}
        Ok(_) => panic!("expected error for zero depth"),
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
    }
}

/// Non-finite height must be rejected.
#[test]
fn kernel_wedge_infinite_height_returns_error() {
    let mut kernel = OcctKernel::new();
    let result = kernel.execute(&GeometryOp::Wedge {
        width: Value::Real(0.020),
        depth: Value::Real(0.010),
        height: Value::Real(f64::INFINITY),
        top_width: Value::Real(0.005),
    });
    match result {
        Err(GeometryError::OperationFailed(_)) => {}
        Ok(_) => panic!("expected error for infinite height"),
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
    }
}

/// Negative top_width must be rejected.
#[test]
fn kernel_wedge_negative_top_width_returns_error() {
    let mut kernel = OcctKernel::new();
    let result = kernel.execute(&GeometryOp::Wedge {
        width: Value::Real(0.020),
        depth: Value::Real(0.010),
        height: Value::Real(0.015),
        top_width: Value::Real(-0.001),
    });
    match result {
        Err(GeometryError::OperationFailed(_)) => {}
        Ok(_) => panic!("expected error for negative top_width"),
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
    }
}

// --- Profile IR → kernel execute tests (task-4160, step-3) ---
// These exercise OcctKernel::execute() for the new GeometryOp::RectangleProfile
// and GeometryOp::CircleProfile variants.  RED until step-4 adds those variants.

/// RectangleProfile execute → extrude → volume ≈ w·h·d.
///
/// w=0.020, h=0.010, d=0.003 → V = 6.0e-7 m³ (exact rectangular prism;
/// OCCT error ~1e-12 relative, so the 2 % bound is met with enormous margin).
///
/// RED until step-4 adds GeometryOp::RectangleProfile.
#[test]
fn rectangle_profile_executes_and_extrude_volume() {
    let mut kernel = OcctKernel::new();
    let profile_h = kernel
        .execute(&GeometryOp::RectangleProfile {
            width: Value::Real(0.020),
            height: Value::Real(0.010),
        })
        .expect("RectangleProfile execute should succeed");
    let solid_h = kernel
        .execute(&GeometryOp::Extrude {
            profile: profile_h.id,
            distance: Value::Real(0.003),
        })
        .expect("Extrude of RectangleProfile should succeed");
    let vol = kernel
        .query(&GeometryQuery::Volume(solid_h.id))
        .expect("Volume query should succeed")
        .as_f64()
        .expect("Volume should be numeric");
    let expected = 0.020_f64 * 0.010 * 0.003; // w·h·d = 6.0e-7 m³
    let rel_err = (vol - expected).abs() / expected;
    assert!(
        rel_err < 0.02,
        "RectangleProfile extrude volume: expected ≈ {expected:.3e}, got {vol:.3e} (rel_err={rel_err:.4})"
    );
}

/// CircleProfile execute → SurfaceArea ≈ π·r².
///
/// r=0.008 → A = π·0.064e-3 = 2.0106e-4 m² (OCCT analytic Geom_Circle; exact).
///
/// RED until step-4 adds GeometryOp::CircleProfile.
#[test]
fn circle_profile_executes_surface_area() {
    let mut kernel = OcctKernel::new();
    let profile_h = kernel
        .execute(&GeometryOp::CircleProfile {
            radius: Value::Real(0.008),
        })
        .expect("CircleProfile execute should succeed");
    let area = kernel
        .query(&GeometryQuery::SurfaceArea(profile_h.id))
        .expect("SurfaceArea query should succeed")
        .as_f64()
        .expect("SurfaceArea should be numeric");
    let expected = std::f64::consts::PI * 0.008_f64 * 0.008; // π·r² = 2.0106e-4 m²
    let rel_err = (area - expected).abs() / expected;
    assert!(
        rel_err < 0.02,
        "CircleProfile surface area: expected ≈ {expected:.4e}, got {area:.4e} (rel_err={rel_err:.4})"
    );
}

/// Degenerate input: RectangleProfile with width=0 returns GeometryError::OperationFailed.
///
/// RED until step-4 adds GeometryOp::RectangleProfile.
#[test]
fn rectangle_profile_zero_width_returns_error() {
    let mut kernel = OcctKernel::new();
    let result = kernel.execute(&GeometryOp::RectangleProfile {
        width: Value::Real(0.0),
        height: Value::Real(0.010),
    });
    match result {
        Err(GeometryError::OperationFailed(_)) => {}
        Ok(_) => panic!("expected OperationFailed for width=0, got Ok"),
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
    }
}

/// RED until step-4 adds GeometryOp::CircleProfile.
#[test]
fn circle_profile_negative_radius_returns_error() {
    let mut kernel = OcctKernel::new();
    let result = kernel.execute(&GeometryOp::CircleProfile {
        radius: Value::Real(-0.008),
    });
    match result {
        Err(GeometryError::OperationFailed(_)) => {}
        Ok(_) => panic!("expected OperationFailed for negative radius, got Ok"),
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
    }
}

// --- kernel execute for PolygonProfile + EllipseProfile (task-4161 step-3) ---
// RED until step-4 adds GeometryOp::PolygonProfile / EllipseProfile.

/// (a) PolygonProfile (4-vertex square) execute → extrude → volume ≈ 5e-7 m³.
///
/// Shoelace area of [0,0],[0.01,0],[0.01,0.01],[0,0.01] = 1e-4 m²;
/// extrude height = 0.005 m → volume = 5e-7 m³ (exact rectangular prism).
///
/// RED until step-4 adds GeometryOp::PolygonProfile.
#[test]
fn polygon_profile_executes_and_extrude_volume() {
    let mut kernel = OcctKernel::new();
    let profile_h = kernel
        .execute(&GeometryOp::PolygonProfile {
            points: vec![[0.0, 0.0], [0.01, 0.0], [0.01, 0.01], [0.0, 0.01]],
        })
        .expect("PolygonProfile execute should succeed");
    let solid_h = kernel
        .execute(&GeometryOp::Extrude {
            profile: profile_h.id,
            distance: Value::Real(0.005),
        })
        .expect("Extrude of PolygonProfile should succeed");
    let vol = kernel
        .query(&GeometryQuery::Volume(solid_h.id))
        .expect("Volume query should succeed")
        .as_f64()
        .expect("Volume should be numeric");
    let expected = 1e-4_f64 * 0.005; // shoelace_area × height = 5e-7 m³
    let rel_err = (vol - expected).abs() / expected;
    assert!(
        rel_err < 0.02,
        "PolygonProfile extrude volume: expected ≈ {expected:.3e}, got {vol:.3e} (rel_err={rel_err:.4})"
    );
}

/// (b) EllipseProfile execute → SurfaceArea ≈ π × 0.010 × 0.005 ≈ 1.5708e-4 m².
///
/// RED until step-4 adds GeometryOp::EllipseProfile.
#[test]
fn ellipse_profile_executes_surface_area() {
    let mut kernel = OcctKernel::new();
    let profile_h = kernel
        .execute(&GeometryOp::EllipseProfile {
            semi_major: Value::Real(0.010),
            semi_minor: Value::Real(0.005),
        })
        .expect("EllipseProfile execute should succeed");
    let area = kernel
        .query(&GeometryQuery::SurfaceArea(profile_h.id))
        .expect("SurfaceArea query should succeed")
        .as_f64()
        .expect("SurfaceArea should be numeric");
    let expected = std::f64::consts::PI * 0.010 * 0.005; // π·a·b ≈ 1.5708e-4 m²
    let rel_err = (area - expected).abs() / expected;
    assert!(
        rel_err < 0.02,
        "EllipseProfile surface area: expected ≈ {expected:.4e}, got {area:.4e} (rel_err={rel_err:.4})"
    );
}

/// (c-i) Validation: PolygonProfile with fewer than 3 points returns OperationFailed.
///
/// RED until step-4 adds GeometryOp::PolygonProfile.
#[test]
fn polygon_profile_too_few_points_returns_error() {
    let mut kernel = OcctKernel::new();
    let result = kernel.execute(&GeometryOp::PolygonProfile {
        points: vec![[0.0, 0.0], [0.01, 0.0]],
    });
    match result {
        Err(GeometryError::OperationFailed(_)) => {}
        Ok(_) => panic!("expected OperationFailed for <3 points, got Ok"),
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
    }
}

/// (c-ii) Validation: EllipseProfile with non-positive semi-axis returns OperationFailed.
///
/// RED until step-4 adds GeometryOp::EllipseProfile.
#[test]
fn ellipse_profile_nonpositive_axis_returns_error() {
    let mut kernel = OcctKernel::new();
    let result = kernel.execute(&GeometryOp::EllipseProfile {
        semi_major: Value::Real(0.0),
        semi_minor: Value::Real(0.005),
    });
    match result {
        Err(GeometryError::OperationFailed(_)) => {}
        Ok(_) => panic!("expected OperationFailed for semi_major=0, got Ok"),
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
    }
}

/// (c-iii) Validation: PolygonProfile with collinear points returns OperationFailed.
///
/// Collinear points produce a degenerate (zero-area) polygon that BRepBuilderAPI
/// rejects (MakePolygon.IsDone() or MakeFace.IsDone() fails, or the GProp Mass()<=0
/// guard fires).  Exercising this through OcctKernel::execute() confirms that the
/// kernel-level PolygonProfile arm correctly propagates the C++ rejection as
/// OperationFailed rather than panicking or returning Ok with an invalid handle.
///
/// The FFI-level analogue is make_polygon_face_ffi_collinear_points_returns_err;
/// this test adds coverage at the kernel-execute layer.
#[test]
fn polygon_profile_collinear_points_returns_error() {
    let mut kernel = OcctKernel::new();
    // Three collinear points on the X-axis → zero area → degenerate face
    let result = kernel.execute(&GeometryOp::PolygonProfile {
        points: vec![[0.0, 0.0], [0.01, 0.0], [0.02, 0.0]],
    });
    match result {
        Err(GeometryError::OperationFailed(_)) => {}
        Ok(_) => panic!("expected OperationFailed for collinear polygon points, got Ok"),
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
    }
}
