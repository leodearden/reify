//! Rigid and affine transforms and patterns through `OcctKernel::execute`: linear, 2-D,
//! circular and arbitrary patterns, mirror, uniform and non-uniform scale, rotate-around and
//! `AffineApply`.

#![cfg(has_occt)]

use crate::support::assert_operation_fails_with;
use reify_ir::{BRepKind, GeometryError, GeometryOp, GeometryQuery, Value};
use reify_kernel_occt::OcctKernel;

// `mesh_aabb` (AABB of a tessellated [`Mesh`] over its flat
// `[x0,y0,z0,x1,y1,z1,...]` vertex buffer) is shared via
// `reify_test_support::mesh_aabb` (task 4959 dedup). Distinct from
// `ffi::ffi::query_bbox` (which computes the exact-geometry AABB via
// `BRepBndLib::Add`) — this reads the tessellation directly, so the
// result is bounded by the tessellation deflection passed to
// `OcctKernel::tessellate`, not `Precision::Confusion()`.
use reify_test_support::mesh_aabb;

/// Parse a single named field out of the `{"xmin":_,"ymin":_,"zmin":_,"xmax":_,
/// "ymax":_,"zmax":_}` JSON string returned by `GeometryQuery::BoundingBox`.
fn parse_bbox_field(s: &str, field: &str) -> f64 {
    let needle = format!("\"{field}\":");
    let start = s
        .find(needle.as_str())
        .unwrap_or_else(|| panic!("field {field} not found in bbox JSON: {s:?}"))
        + needle.len();
    let rest = &s[start..];
    let end = rest.find([',', '}']).unwrap_or(rest.len());
    rest[..end]
        .trim()
        .parse::<f64>()
        .unwrap_or_else(|e| panic!("failed to parse {field} in bbox JSON: {s:?}: {e}"))
}

// --- Pattern tests ---

#[test]
fn linear_pattern_4_boxes() {
    let mut kernel = OcctKernel::new();
    // Create a 10x10x10 box (volume = 1000)
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    // Apply LinearPattern: 4 copies along X with spacing 20
    let pattern_h = kernel
        .execute(&GeometryOp::LinearPattern {
            target: box_h.id,
            direction: [1.0, 0.0, 0.0],
            count: 4,
            spacing: Value::Real(20.0),
        })
        .unwrap();
    // Volume should be approximately 4 * 1000 = 4000
    let vol = kernel.query(&GeometryQuery::Volume(pattern_h.id)).unwrap();
    match vol {
        Value::Real(v) => {
            assert!(
                (v - 4000.0).abs() < 50.0,
                "expected linear pattern volume ~4000, got {v}"
            );
        }
        other => panic!("expected Value::Real, got {:?}", other),
    }
}

#[test]
fn circular_pattern_6_instances() {
    let mut kernel = OcctKernel::new();
    // Create a small 5x5x5 box (volume = 125)
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(5.0),
            size_y: Value::Real(5.0),
            size_z: Value::Real(5.0),
        })
        .unwrap();
    // Translate to (20,0,0) to offset from center
    let translated_h = kernel
        .execute(&GeometryOp::Translate {
            target: box_h.id,
            dx: 20.0,
            dy: 0.0,
            dz: 0.0,
        })
        .unwrap();
    // Circular pattern: 6 instances, full circle (2*PI)
    let pattern_h = kernel
        .execute(&GeometryOp::CircularPattern {
            target: translated_h.id,
            axis_origin: [0.0, 0.0, 0.0],
            axis_dir: [0.0, 0.0, 1.0],
            count: 6,
            angle: Value::Real(2.0 * std::f64::consts::PI),
        })
        .unwrap();
    // Volume should be approximately 6 * 125 = 750
    let vol = kernel.query(&GeometryQuery::Volume(pattern_h.id)).unwrap();
    match vol {
        Value::Real(v) => {
            assert!(
                (v - 750.0).abs() < 50.0,
                "expected circular pattern volume ~750, got {v}"
            );
        }
        other => panic!("expected Value::Real, got {:?}", other),
    }
}

#[test]
fn linear_pattern_2d_3x4_grid() {
    let mut kernel = OcctKernel::new();
    // Create a 10x10x10 box (volume = 1000)
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    // Apply LinearPattern2D: 3×4 grid along X and Y with 20mm spacing
    let pattern_h = kernel
        .execute(&GeometryOp::LinearPattern2D {
            target: box_h.id,
            direction1: [1.0, 0.0, 0.0],
            count1: 3,
            spacing1: Value::Real(20.0),
            direction2: [0.0, 1.0, 0.0],
            count2: 4,
            spacing2: Value::Real(20.0),
        })
        .unwrap();
    // Volume should be approximately 3*4 * 1000 = 12000
    let vol = kernel.query(&GeometryQuery::Volume(pattern_h.id)).unwrap();
    match vol {
        Value::Real(v) => {
            assert!(
                (v - 12000.0).abs() < 200.0,
                "expected linear_pattern_2d volume ~12000, got {v}"
            );
        }
        other => panic!("expected Value::Real, got {:?}", other),
    }
}

#[test]
fn arbitrary_pattern_3_transforms() {
    let mut kernel = OcctKernel::new();
    // Create a 10x10x10 box (volume = 1000)
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    // Apply ArbitraryPattern with 3 non-overlapping translations
    let pattern_h = kernel
        .execute(&GeometryOp::ArbitraryPattern {
            target: box_h.id,
            transforms: vec![
                ([1.0, 0.0, 0.0, 0.0], [20.0, 0.0, 0.0]),
                ([1.0, 0.0, 0.0, 0.0], [0.0, 20.0, 0.0]),
                ([1.0, 0.0, 0.0, 0.0], [20.0, 20.0, 0.0]),
            ],
        })
        .unwrap();
    // Volume should be approximately 4 * 1000 = 4000 (original + 3 copies)
    let vol = kernel.query(&GeometryQuery::Volume(pattern_h.id)).unwrap();
    match vol {
        Value::Real(v) => {
            assert!(
                (v - 4000.0).abs() < 200.0,
                "expected arbitrary_pattern volume ~4000, got {v}"
            );
        }
        other => panic!("expected Value::Real, got {:?}", other),
    }
}

/// RED (task 4168, step-3): a single-instance `ArbitraryPattern` whose transform
/// carries a 90-degree-about-Y rotation (zero translation) must honor the rotation
/// in the fused result, not just the translation.
///
/// `Box { size_x: 2, size_y: 2, size_z: 10 }` has X-extent=2, Y-extent=2, Z-extent=10
/// (`make_box(size_x,size_y,size_z)` -> `gp_Pnt(-size_x/2,-size_y/2,-size_z/2)` +
/// `MakeBox(dx,dy,dz)`). A 90-degree rotation about Y swaps the X and Z
/// extents (2<->10) for the rotated copy; fusing it with the untouched original
/// (X-extent 2) grows the union's overall X-extent to the max of the two, ~10.
///
/// Until the kernel honors `.0` (currently dropped — see step-2's handler comment),
/// the instance behaves as identity, so the result is `original ∪ original` =
/// `box(2,2,10)` and the X-extent stays ~2, failing the ~10 assertion below.
#[test]
fn arbitrary_pattern_honors_rotation() {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(2.0),
            size_y: Value::Real(2.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    // Single instance: 90-degree-about-Y quaternion [cos45,0,sin45,0], zero
    // translation.
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let pattern_h = kernel
        .execute(&GeometryOp::ArbitraryPattern {
            target: box_h.id,
            transforms: vec![([s, 0.0, s, 0.0], [0.0, 0.0, 0.0])],
        })
        .unwrap();
    let bbox = kernel
        .query(&GeometryQuery::BoundingBox(pattern_h.id))
        .unwrap();
    let s = match bbox {
        Value::String(s) => s,
        other => panic!("expected Value::String, got {:?}", other),
    };
    let x_extent = parse_bbox_field(&s, "xmax") - parse_bbox_field(&s, "xmin");
    assert!(
        (x_extent - 10.0).abs() < 1.0,
        "expected rotated-copy union X-extent ~10 (Y-90 swaps X<->Z 2<->10), got {x_extent} (bbox={s})"
    );
}

#[test]
fn linear_pattern_2d_count1_zero_errors() {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    let result = kernel.execute(&GeometryOp::LinearPattern2D {
        target: box_h.id,
        direction1: [1.0, 0.0, 0.0],
        count1: 0,
        spacing1: Value::Real(20.0),
        direction2: [0.0, 1.0, 0.0],
        count2: 3,
        spacing2: Value::Real(20.0),
    });
    match result {
        Err(GeometryError::OperationFailed(msg)) => {
            assert!(
                msg.contains("count1"),
                "error should mention count1, got: {msg}"
            );
        }
        other => panic!("expected OperationFailed for count1==0, got {:?}", other),
    }
}

#[test]
fn linear_pattern_2d_count2_zero_errors() {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    let result = kernel.execute(&GeometryOp::LinearPattern2D {
        target: box_h.id,
        direction1: [1.0, 0.0, 0.0],
        count1: 3,
        spacing1: Value::Real(20.0),
        direction2: [0.0, 1.0, 0.0],
        count2: 0,
        spacing2: Value::Real(20.0),
    });
    match result {
        Err(GeometryError::OperationFailed(msg)) => {
            assert!(
                msg.contains("count2"),
                "error should mention count2, got: {msg}"
            );
        }
        other => panic!("expected OperationFailed for count2==0, got {:?}", other),
    }
}

#[test]
fn linear_pattern_2d_nan_direction_errors() {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    // NaN in direction1 should fail
    let result = kernel.execute(&GeometryOp::LinearPattern2D {
        target: box_h.id,
        direction1: [f64::NAN, 0.0, 0.0],
        count1: 3,
        spacing1: Value::Real(20.0),
        direction2: [0.0, 1.0, 0.0],
        count2: 3,
        spacing2: Value::Real(20.0),
    });
    match result {
        Err(GeometryError::OperationFailed(msg)) => {
            assert!(
                msg.contains("direction1"),
                "error should mention direction1, got: {msg}"
            );
        }
        other => panic!(
            "expected OperationFailed for NaN direction1, got {:?}",
            other
        ),
    }
    // Inf in direction2 should fail
    let result = kernel.execute(&GeometryOp::LinearPattern2D {
        target: box_h.id,
        direction1: [1.0, 0.0, 0.0],
        count1: 3,
        spacing1: Value::Real(20.0),
        direction2: [0.0, f64::INFINITY, 0.0],
        count2: 3,
        spacing2: Value::Real(20.0),
    });
    match result {
        Err(GeometryError::OperationFailed(msg)) => {
            assert!(
                msg.contains("direction2"),
                "error should mention direction2, got: {msg}"
            );
        }
        other => panic!(
            "expected OperationFailed for Inf direction2, got {:?}",
            other
        ),
    }
}

#[test]
fn arbitrary_pattern_zero_transforms_errors() {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    let result = kernel.execute(&GeometryOp::ArbitraryPattern {
        target: box_h.id,
        transforms: vec![],
    });
    match result {
        Err(GeometryError::OperationFailed(msg)) => {
            assert!(
                msg.contains("transform"),
                "error should mention transform, got: {msg}"
            );
        }
        other => panic!(
            "expected OperationFailed for empty transforms, got {:?}",
            other
        ),
    }
}

#[test]
fn pattern_plus_boolean() {
    let mut kernel = OcctKernel::new();
    // Create plate: 100x60x10
    let plate = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(100.0),
            size_y: Value::Real(60.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    let plate_vol = match kernel.query(&GeometryQuery::Volume(plate.id)).unwrap() {
        Value::Real(v) => v,
        _ => panic!("expected Real"),
    };

    // Create hole: small cylinder
    let hole = kernel
        .execute(&GeometryOp::Cylinder {
            radius: Value::Real(3.0),
            height: Value::Real(20.0),
        })
        .unwrap();
    // Move hole down so it passes through the plate
    let hole_pos = kernel
        .execute(&GeometryOp::Translate {
            target: hole.id,
            dx: -30.0,
            dy: 0.0,
            dz: -10.0,
        })
        .unwrap();

    // Pattern: 5 holes along X
    let patterned = kernel
        .execute(&GeometryOp::LinearPattern {
            target: hole_pos.id,
            direction: [1.0, 0.0, 0.0],
            count: 5,
            spacing: Value::Real(15.0),
        })
        .unwrap();

    // Boolean difference: plate minus patterned holes
    let result = kernel
        .execute(&GeometryOp::Difference {
            left: plate.id,
            right: patterned.id,
        })
        .unwrap();

    let result_vol = match kernel.query(&GeometryQuery::Volume(result.id)).unwrap() {
        Value::Real(v) => v,
        _ => panic!("expected Real"),
    };
    assert!(
        result_vol < plate_vol,
        "plate with holes ({result_vol}) should have less volume than solid plate ({plate_vol})"
    );
    assert!(result_vol > 0.0, "result should have positive volume");
}

// --- Mirror tests ---

#[test]
fn mirror_across_yz_plane() {
    let mut kernel = OcctKernel::new();
    // Create a 10x10x10 box (volume = 1000)
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    // Translate box to (5,0,0) so it's off-center
    let translated_h = kernel
        .execute(&GeometryOp::Translate {
            target: box_h.id,
            dx: 10.0,
            dy: 0.0,
            dz: 0.0,
        })
        .unwrap();
    // Mirror across YZ plane (origin=[0,0,0], normal=[1,0,0])
    let mirrored_h = kernel
        .execute(&GeometryOp::Mirror {
            target: translated_h.id,
            plane_origin: [0.0, 0.0, 0.0],
            plane_normal: [1.0, 0.0, 0.0],
        })
        .unwrap();
    // Fuse original and mirrored
    let fused_h = kernel
        .execute(&GeometryOp::Union {
            left: translated_h.id,
            right: mirrored_h.id,
        })
        .unwrap();
    // Volume should be approximately 2 * 1000 = 2000
    let vol = kernel.query(&GeometryQuery::Volume(fused_h.id)).unwrap();
    match vol {
        Value::Real(v) => {
            assert!(
                (v - 2000.0).abs() < 10.0,
                "expected fused mirror volume ~2000, got {v}"
            );
        }
        other => panic!("expected Value::Real, got {:?}", other),
    }
}

#[test]
fn mirror_near_zero_normal_rejected() {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    // plane_normal=[1e-20, 0, 0] has mag_sq=1e-40, physically meaningless.
    // Should be rejected like near-degenerate axes in rotate/revolve.
    let result = kernel.execute(&GeometryOp::Mirror {
        target: box_h.id,
        plane_origin: [0.0, 0.0, 0.0],
        plane_normal: [1e-20, 0.0, 0.0],
    });
    assert_operation_fails_with(result, "zero");
}

// --- Scale tests (task-311 step-5) ---

#[test]
fn scale_doubles_volume() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }
    let mut kernel = OcctKernel::new();
    // Create a 10x10x10 box, volume = 1000
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    // Scale by 2: linear dimensions double, volume becomes 8x = 8000
    let scaled_h = kernel
        .execute(&GeometryOp::Scale {
            target: box_h.id,
            factor: 2.0,
        })
        .unwrap();
    let vol = kernel.query(&GeometryQuery::Volume(scaled_h.id)).unwrap();
    match vol {
        Value::Real(v) => {
            assert!(
                (v - 8000.0).abs() < 1.0,
                "scale(2.0) should give volume ≈ 8000, got {v}"
            );
        }
        other => panic!("expected Value::Real for volume, got {:?}", other),
    }
}

#[test]
fn scale_identity_preserves_volume() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    let scaled_h = kernel
        .execute(&GeometryOp::Scale {
            target: box_h.id,
            factor: 1.0,
        })
        .unwrap();
    let vol = kernel.query(&GeometryQuery::Volume(scaled_h.id)).unwrap();
    match vol {
        Value::Real(v) => {
            assert!(
                (v - 1000.0).abs() < 1.0,
                "scale(1.0) should preserve volume ≈ 1000, got {v}"
            );
        }
        other => panic!("expected Value::Real for volume, got {:?}", other),
    }
}

#[test]
fn scale_zero_factor_rejected() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    let result = kernel.execute(&GeometryOp::Scale {
        target: box_h.id,
        factor: 0.0,
    });
    match result {
        Err(GeometryError::OperationFailed(msg)) => {
            assert!(
                msg.contains("non-zero") || msg.contains("finite"),
                "error should mention non-zero/finite constraint, got: {msg}"
            );
        }
        other => panic!(
            "expected GeometryError::OperationFailed for zero scale factor, got {:?}",
            other
        ),
    }
}

#[test]
fn scale_nan_factor_rejected() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    let result = kernel.execute(&GeometryOp::Scale {
        target: box_h.id,
        factor: f64::NAN,
    });
    match result {
        Err(GeometryError::OperationFailed(msg)) => {
            assert!(
                msg.contains("finite") || msg.contains("non-zero"),
                "error should mention finite constraint, got: {msg}"
            );
        }
        other => panic!(
            "expected GeometryError::OperationFailed for NaN scale factor, got {:?}",
            other
        ),
    }
}

// --- RotateAround tests (task-311 step-7) ---

#[test]
fn rotate_around_non_origin_differs_from_rotate_at_origin() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }
    let mut kernel = OcctKernel::new();
    // Create a 10x10x10 box centered at origin.
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();

    // Rotate around Z through origin by PI/2 — centroid stays at (0,0,0).
    let rotated_origin = kernel
        .execute(&GeometryOp::Rotate {
            target: box_h.id,
            axis: [0.0, 0.0, 1.0],
            angle_rad: std::f64::consts::FRAC_PI_2,
        })
        .unwrap();

    // Rotate around Z through (50, 0, 0) by PI/2 — centroid moves.
    let rotated_around = kernel
        .execute(&GeometryOp::RotateAround {
            target: box_h.id,
            point: [50.0, 0.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            angle_rad: std::f64::consts::FRAC_PI_2,
        })
        .unwrap();

    // Both should succeed (volumes must be preserved).
    let vol_origin = kernel
        .query(&GeometryQuery::Volume(rotated_origin.id))
        .unwrap();
    let vol_around = kernel
        .query(&GeometryQuery::Volume(rotated_around.id))
        .unwrap();

    match (vol_origin, vol_around) {
        (Value::Real(v1), Value::Real(v2)) => {
            assert!(
                (v1 - 1000.0).abs() < 1.0,
                "rotate-at-origin should preserve volume ≈ 1000, got {v1}"
            );
            assert!(
                (v2 - 1000.0).abs() < 1.0,
                "rotate-around-point should preserve volume ≈ 1000, got {v2}"
            );
        }
        other => panic!("expected (Value::Real, Value::Real), got {:?}", other),
    }

    // The centroids should be different: rotate_around (point=(50,0,0), axis=Z, angle=PI/2)
    // moves centroid of origin-centered box from (0,0,0) to roughly (-50, 50, 0).
    let centroid_around = kernel
        .query(&GeometryQuery::Centroid(rotated_around.id))
        .unwrap();
    match centroid_around {
        Value::String(s) => {
            let x_start = s.find("\"x\":").unwrap() + 4;
            let x_end = s[x_start..].find([',', '}']).unwrap() + x_start;
            let x: f64 = s[x_start..x_end].parse().unwrap();
            // After rotating (0,0,0) 90° around Z through (50,0,0):
            // new position = (50,0,0) + Rz(PI/2) * (0-50, 0-0, 0-0)
            //              = (50,0,0) + Rz(PI/2) * (-50, 0, 0)
            //              = (50,0,0) + (0, -50, 0)  [Rz(PI/2)*(-1,0,0) = (0,-1,0)]
            //              = (50, -50, 0)
            // So x ≈ 50
            assert!(
                (x - 50.0).abs() < 1.0,
                "rotate_around centroid x should be ≈ 50, got {x}"
            );
        }
        other => panic!("expected String centroid, got {:?}", other),
    }
}

/// `OcctKernel::execute(&GeometryOp::ApplyTransform { ... })` must dispatch to
/// `apply_transform_to_handle`, return a fresh `GeometryHandle`, and preserve
/// the source BRepKind (Solid → Solid) on the result.
///
/// Locks the kernel-side dispatch arm: the IR-level `GeometryOp::ApplyTransform`
/// is the unified seam that T5/T8 will emit, so executing it through the
/// standard `execute(&op)` entry must produce a usable handle with the right
/// classifier.
#[test]
fn execute_apply_transform_dispatches_correctly() {
    let mut kernel = OcctKernel::new();

    let source = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .expect("box creation should succeed");

    let result = kernel.execute(&GeometryOp::ApplyTransform {
        target: source.id,
        rotation: [1.0, 0.0, 0.0, 0.0],
        translation: [10.0, 20.0, 30.0],
    });

    let handle = result.expect("ApplyTransform execute should succeed");
    assert_eq!(
        handle.repr,
        Some(BRepKind::Solid),
        "ApplyTransform on a Solid source must produce a Solid result handle \
         (rigid isometry preserves topology classification)"
    );
    assert_ne!(
        handle.id, source.id,
        "ApplyTransform must produce a fresh handle, not reuse the source's"
    );
}

/// `OcctKernel::execute(&GeometryOp::AffineApply { ... })` must dispatch to
/// `ffi::ffi::gtransform_shape` (gp_GTrsf / BRepBuilderAPI_GTransform).
///
/// A non-uniform scale (`diag(1,1,2)`, zero translation) applied to a
/// centered 10mm box must double the Z extent while leaving X/Y unchanged —
/// verified against the TESSELLATED mesh (not `query_bbox`), matching the
/// vertex-exact mapping `x ↦ linear·x` within tessellation tolerance.
#[test]
fn execute_affine_apply_non_uniform_scale_maps_tessellated_aabb() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(0.01),
            size_y: Value::Real(0.01),
            size_z: Value::Real(0.01),
        })
        .unwrap();
    let transformed = kernel
        .execute(&GeometryOp::AffineApply {
            target: box_h.id,
            linear: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 2.0]],
            translation: [0.0, 0.0, 0.0],
        })
        .expect("AffineApply execute should succeed");
    let mesh = kernel
        .tessellate(transformed.id, 1e-4)
        .expect("tessellate should succeed");
    let (min, max) = mesh_aabb(&mesh);
    let tol = 1e-4_f32;
    assert!(
        (max[0] - min[0] - 0.01).abs() < tol,
        "X extent expected ≈0.01 (unscaled), got {}",
        max[0] - min[0]
    );
    assert!(
        (max[1] - min[1] - 0.01).abs() < tol,
        "Y extent expected ≈0.01 (unscaled), got {}",
        max[1] - min[1]
    );
    assert!(
        (max[2] - min[2] - 0.02).abs() < tol,
        "Z extent expected ≈0.02 (doubled by linear[2][2]=2.0), got {}",
        max[2] - min[2]
    );
}

/// Identity linear part + zero translation must be an AABB no-op.
#[test]
fn execute_affine_apply_identity_is_aabb_noop() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(0.01),
            size_y: Value::Real(0.01),
            size_z: Value::Real(0.01),
        })
        .unwrap();
    let orig_mesh = kernel.tessellate(box_h.id, 1e-4).unwrap();
    let (orig_min, orig_max) = mesh_aabb(&orig_mesh);

    let transformed = kernel
        .execute(&GeometryOp::AffineApply {
            target: box_h.id,
            linear: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            translation: [0.0, 0.0, 0.0],
        })
        .expect("identity AffineApply execute should succeed");
    let mesh = kernel.tessellate(transformed.id, 1e-4).unwrap();
    let (min, max) = mesh_aabb(&mesh);
    let tol = 1e-4_f32;
    for axis in 0..3 {
        assert!(
            (min[axis] - orig_min[axis]).abs() < tol,
            "identity affine_apply must preserve min[{axis}]: before={}, after={}",
            orig_min[axis],
            min[axis]
        );
        assert!(
            (max[axis] - orig_max[axis]).abs() < tol,
            "identity affine_apply must preserve max[{axis}]: before={}, after={}",
            orig_max[axis],
            max[axis]
        );
    }
}

/// A reflection (`det<0`, e.g. `diag(-1,1,1)`) must produce a valid,
/// tessellatable solid — no crash, no error. A box is centered at the
/// origin, so its AABB extents are symmetric and a reflection leaves the
/// extent *magnitudes* unchanged; the non-vacuous signal here is that the
/// op succeeds and yields real geometry rather than that the AABB visibly
/// "flips" (which a centered, axis-aligned box cannot show).
#[test]
fn execute_affine_apply_reflection_produces_valid_mirrored_solid() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(0.01),
            size_y: Value::Real(0.01),
            size_z: Value::Real(0.01),
        })
        .unwrap();
    let transformed = kernel.execute(&GeometryOp::AffineApply {
        target: box_h.id,
        linear: [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        translation: [0.0, 0.0, 0.0],
    });
    let handle = transformed.expect("reflection (det<0) AffineApply must succeed, not error");
    let mesh = kernel
        .tessellate(handle.id, 1e-4)
        .expect("reflected solid must tessellate without crashing");
    assert!(
        !mesh.vertices.is_empty(),
        "reflected solid must have a non-empty tessellation"
    );
    let (min, max) = mesh_aabb(&mesh);
    let tol = 1e-4_f32;
    assert!(
        (max[0] - min[0] - 0.01).abs() < tol,
        "reflection must preserve X extent magnitude, got {}",
        max[0] - min[0]
    );
}

/// A non-finite linear/translation component must return
/// `Err(GeometryError::OperationFailed)`, not panic.
#[test]
fn execute_affine_apply_non_finite_component_returns_error() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(0.01),
            size_y: Value::Real(0.01),
            size_z: Value::Real(0.01),
        })
        .unwrap();
    let result = kernel.execute(&GeometryOp::AffineApply {
        target: box_h.id,
        linear: [[f64::NAN, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        translation: [0.0, 0.0, 0.0],
    });
    match result {
        Err(GeometryError::OperationFailed(msg)) => {
            assert!(
                msg.contains("finite"),
                "error should mention finite constraint, got: {msg}"
            );
        }
        other => panic!(
            "expected GeometryError::OperationFailed for non-finite linear component, got {:?}",
            other
        ),
    }
}

// --- ScaleNonUniform tests (task 4167 step-1) ---

/// `OcctKernel::execute(&GeometryOp::ScaleNonUniform { ... })` must dispatch
/// to `ffi::ffi::gtransform_shape` (gp_GTrsf) with a diagonal linear part
/// `diag(sx,sy,sz)` and zero translation — mirrors
/// `execute_affine_apply_non_uniform_scale_maps_tessellated_aabb` but through
/// the dedicated per-axis `scale` surface (task 4167).
#[test]
fn scale_non_uniform_maps_tessellated_aabb() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(0.01),
            size_y: Value::Real(0.01),
            size_z: Value::Real(0.01),
        })
        .unwrap();
    let scaled = kernel
        .execute(&GeometryOp::ScaleNonUniform {
            target: box_h.id,
            sx: 2.0,
            sy: 1.0,
            sz: 0.5,
        })
        .expect("ScaleNonUniform execute should succeed");
    let mesh = kernel
        .tessellate(scaled.id, 1e-4)
        .expect("tessellate should succeed");
    let (min, max) = mesh_aabb(&mesh);
    let tol = 1e-4_f32;
    assert!(
        (max[0] - min[0] - 0.02).abs() < tol,
        "X extent expected ≈0.02 (sx=2.0), got {}",
        max[0] - min[0]
    );
    assert!(
        (max[1] - min[1] - 0.01).abs() < tol,
        "Y extent expected ≈0.01 (sy=1.0), got {}",
        max[1] - min[1]
    );
    assert!(
        (max[2] - min[2] - 0.005).abs() < tol,
        "Z extent expected ≈0.005 (sz=0.5), got {}",
        max[2] - min[2]
    );
}

/// Volume scales by sx*sy*sz = 2*1*0.5 = 1 → unchanged from the source box
/// (mirrors `scale_doubles_volume`).
#[test]
fn scale_non_uniform_volume_preserved() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    let scaled = kernel
        .execute(&GeometryOp::ScaleNonUniform {
            target: box_h.id,
            sx: 2.0,
            sy: 1.0,
            sz: 0.5,
        })
        .expect("ScaleNonUniform execute should succeed");
    let vol = kernel.query(&GeometryQuery::Volume(scaled.id)).unwrap();
    match vol {
        Value::Real(v) => {
            assert!(
                (v - 1000.0).abs() < 1.0,
                "scale(2,1,0.5) should give volume ≈ 1000 (2*1*0.5=1), got {v}"
            );
        }
        other => panic!("expected Value::Real for volume, got {:?}", other),
    }
}

#[test]
fn scale_non_uniform_zero_component_rejected() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    let result = kernel.execute(&GeometryOp::ScaleNonUniform {
        target: box_h.id,
        sx: 0.0,
        sy: 1.0,
        sz: 1.0,
    });
    match result {
        Err(GeometryError::OperationFailed(msg)) => {
            assert!(
                msg.contains("non-zero") || msg.contains("finite"),
                "error should mention non-zero/finite constraint, got: {msg}"
            );
        }
        other => panic!(
            "expected GeometryError::OperationFailed for zero scale component, got {:?}",
            other
        ),
    }
}

#[test]
fn scale_non_uniform_nan_component_rejected() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    let result = kernel.execute(&GeometryOp::ScaleNonUniform {
        target: box_h.id,
        sx: 1.0,
        sy: f64::NAN,
        sz: 1.0,
    });
    match result {
        Err(GeometryError::OperationFailed(msg)) => {
            assert!(
                msg.contains("finite") || msg.contains("non-zero"),
                "error should mention finite constraint, got: {msg}"
            );
        }
        other => panic!(
            "expected GeometryError::OperationFailed for NaN scale component, got {:?}",
            other
        ),
    }
}
