//! Boundary 4 (eval → geometry) — Mock geometry kernel tests.
//!
//! These tests verify the eval side can correctly call the GeometryKernel trait.
//! Actual OCCT tests go in reify-kernel-occt; here we test with MockGeometryKernel.

use reify_test_support::*;
use reify_types::{
    ExportFormat, GeometryHandleId, GeometryKernel, GeometryOp, GeometryQuery, Value,
};

#[test]
fn mock_kernel_create_box() {
    let mut kernel = MockGeometryKernel::new();
    let op = GeometryOp::Box {
        width: mm(80.0),
        height: mm(100.0),
        depth: mm(5.0),
    };
    let handle = kernel.execute(&op).unwrap();
    assert_eq!(handle.id, GeometryHandleId(1));
}

#[test]
fn mock_kernel_boolean_union() {
    let mut kernel = MockGeometryKernel::new();

    let box1 = kernel
        .execute(&GeometryOp::Box {
            width: mm(10.0),
            height: mm(10.0),
            depth: mm(10.0),
        })
        .unwrap();

    let box2 = kernel
        .execute(&GeometryOp::Box {
            width: mm(5.0),
            height: mm(5.0),
            depth: mm(5.0),
        })
        .unwrap();

    let union = kernel
        .execute(&GeometryOp::Union {
            left: box1.id,
            right: box2.id,
        })
        .unwrap();

    assert_eq!(union.id, GeometryHandleId(3));
    assert_eq!(kernel.operations().len(), 3);
}

#[test]
fn mock_kernel_export() {
    let mut kernel = MockGeometryKernel::new();
    let handle = kernel
        .execute(&GeometryOp::Box {
            width: mm(10.0),
            height: mm(10.0),
            depth: mm(10.0),
        })
        .unwrap();

    let mut output = Vec::new();
    kernel.export(handle.id, ExportFormat::Step, &mut output).unwrap();
    assert!(!output.is_empty(), "export should produce output");
}

#[test]
fn mock_kernel_tessellate() {
    let mut kernel = MockGeometryKernel::new();
    let handle = kernel
        .execute(&GeometryOp::Sphere {
            radius: mm(10.0),
        })
        .unwrap();

    let mesh = kernel.tessellate(handle.id, 0.1).unwrap();
    assert!(!mesh.vertices.is_empty(), "mesh should have vertices");
    assert!(mesh.indices.len().is_multiple_of(3), "indices should be triangle triples");
}

#[test]
fn mock_kernel_query_with_configured_result() {
    let handle_id = GeometryHandleId(1);
    let expected_volume = Value::Real(1e-6); // 1 cm³

    let mut kernel = MockGeometryKernel::new().with_query_result(handle_id, expected_volume);

    // First create a shape so handle 1 exists
    kernel
        .execute(&GeometryOp::Box {
            width: mm(10.0),
            height: mm(10.0),
            depth: mm(10.0),
        })
        .unwrap();

    let result = kernel.query(&GeometryQuery::Volume(handle_id)).unwrap();
    match result {
        Value::Real(v) => assert!((v - 1e-6).abs() < 1e-15),
        other => panic!("expected Real, got {:?}", other),
    }
}

#[test]
fn mock_kernel_translate() {
    let mut kernel = MockGeometryKernel::new();
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
            dx: 0.05,
            dy: 0.0,
            dz: 0.0,
        })
        .unwrap();

    assert_ne!(handle.id, translated.id, "translation should create new handle");
}

/// Tests that will run against the real OCCT kernel — ignored until implemented.
mod occt_tests {
    use reify_kernel_occt::OcctKernel;
    use reify_test_support::*;
    use reify_types::{
        ExportFormat, GeometryError, GeometryHandleId, GeometryOp, GeometryQuery,
        Value,
    };
    #[test]
    fn create_box_export_step() {
        if !reify_kernel_occt::OCCT_AVAILABLE {
            eprintln!("skipping: OCCT not available");
            return;
        }
        let mut kernel = OcctKernel::new();
        let handle = kernel
            .execute(&GeometryOp::Box {
                width: mm(10.0),
                height: mm(10.0),
                depth: mm(10.0),
            })
            .unwrap();

        let mut output = Vec::new();
        kernel
            .export(handle.id, ExportFormat::Step, &mut output)
            .unwrap();

        let step_str = String::from_utf8(output).expect("STEP output should be valid UTF-8");
        assert!(!step_str.is_empty(), "STEP export should produce output");
        assert!(
            step_str.contains("ISO-10303-21"),
            "STEP output should contain ISO-10303-21 header"
        );
    }

    #[test]
    fn cylinder_volume_query() {
        if !reify_kernel_occt::OCCT_AVAILABLE {
            eprintln!("skipping: OCCT not available");
            return;
        }
        let mut kernel = OcctKernel::new();
        let handle = kernel
            .execute(&GeometryOp::Cylinder {
                radius: mm(5.0),
                height: mm(20.0),
            })
            .unwrap();

        let result = kernel
            .query(&GeometryQuery::Volume(handle.id))
            .unwrap();
        match result {
            Value::Real(v) => {
                // r = 0.005m, h = 0.02m, V = π·r²·h ≈ 1.5708e-6 m³
                let expected = std::f64::consts::PI * 0.005_f64.powi(2) * 0.02;
                assert!(
                    (v - expected).abs() < 1e-9,
                    "expected volume ≈ {}, got {}",
                    expected,
                    v
                );
            }
            other => panic!("expected Real, got {:?}", other),
        }
    }

    #[test]
    fn boolean_difference() {
        if !reify_kernel_occt::OCCT_AVAILABLE {
            eprintln!("skipping: OCCT not available");
            return;
        }
        let mut kernel = OcctKernel::new();

        let box1 = kernel
            .execute(&GeometryOp::Box {
                width: mm(20.0),
                height: mm(20.0),
                depth: mm(20.0),
            })
            .unwrap();

        let box2 = kernel
            .execute(&GeometryOp::Box {
                width: mm(10.0),
                height: mm(10.0),
                depth: mm(10.0),
            })
            .unwrap();

        let diff = kernel
            .execute(&GeometryOp::Difference {
                left: box1.id,
                right: box2.id,
            })
            .unwrap();

        // Export to STEP to verify valid shape
        let mut output = Vec::new();
        kernel
            .export(diff.id, ExportFormat::Step, &mut output)
            .unwrap();
        let step_str = String::from_utf8(output).unwrap();
        assert!(!step_str.is_empty(), "difference STEP should be non-empty");
        assert!(step_str.contains("ISO-10303-21"));
    }

    #[test]
    fn fillet_edges() {
        if !reify_kernel_occt::OCCT_AVAILABLE {
            eprintln!("skipping: OCCT not available");
            return;
        }
        let mut kernel = OcctKernel::new();
        let handle = kernel
            .execute(&GeometryOp::Box {
                width: mm(20.0),
                height: mm(20.0),
                depth: mm(20.0),
            })
            .unwrap();

        let filleted = kernel
            .execute(&GeometryOp::Fillet {
                target: handle.id,
                radius: mm(2.0),
            })
            .unwrap();

        // Export to STEP to verify valid shape
        let mut output = Vec::new();
        kernel
            .export(filleted.id, ExportFormat::Step, &mut output)
            .unwrap();
        let step_str = String::from_utf8(output).unwrap();
        assert!(!step_str.is_empty(), "filleted STEP should be non-empty");
        assert!(step_str.contains("ISO-10303-21"));
    }

    #[test]
    fn translate_centroid() {
        if !reify_kernel_occt::OCCT_AVAILABLE {
            eprintln!("skipping: OCCT not available");
            return;
        }
        let mut kernel = OcctKernel::new();
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
                dx: 0.05,
                dy: 0.0,
                dz: 0.0,
            })
            .unwrap();

        let result = kernel
            .query(&GeometryQuery::Centroid(translated.id))
            .unwrap();
        match result {
            Value::String(s) => {
                // Parse centroid JSON and check x ≈ 0.05
                assert!(s.contains("\"x\":"), "centroid should contain x coordinate");
                // Extract x value from JSON string
                let x_start = s.find("\"x\":").unwrap() + 4;
                let x_end = s[x_start..].find([',', '}']).unwrap() + x_start;
                let x: f64 = s[x_start..x_end].parse().unwrap();
                assert!(
                    (x - 0.05).abs() < 1e-9,
                    "centroid x should be ≈ 0.05, got {}",
                    x
                );
            }
            other => panic!("expected String (centroid JSON), got {:?}", other),
        }
    }

    #[test]
    fn invalid_reference_error() {
        if !reify_kernel_occt::OCCT_AVAILABLE {
            eprintln!("skipping: OCCT not available");
            return;
        }
        let mut kernel = OcctKernel::new();
        let result = kernel.execute(&GeometryOp::Union {
            left: GeometryHandleId(999),
            right: GeometryHandleId(1000),
        });

        match result {
            Err(GeometryError::InvalidReference(_)) => {} // expected
            other => panic!(
                "expected GeometryError::InvalidReference, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn zero_dimension_error() {
        if !reify_kernel_occt::OCCT_AVAILABLE {
            eprintln!("skipping: OCCT not available");
            return;
        }
        let mut kernel = OcctKernel::new();
        let result = kernel.execute(&GeometryOp::Box {
            width: mm(0.0),
            height: mm(10.0),
            depth: mm(10.0),
        });

        match result {
            Err(GeometryError::OperationFailed(_)) => {} // expected
            other => panic!(
                "expected GeometryError::OperationFailed for zero width, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn tessellation_valid_mesh() {
        if !reify_kernel_occt::OCCT_AVAILABLE {
            eprintln!("skipping: OCCT not available");
            return;
        }
        let mut kernel = OcctKernel::new();
        let handle = kernel
            .execute(&GeometryOp::Sphere {
                radius: mm(10.0),
            })
            .unwrap();

        let mesh = kernel.tessellate(handle.id, 0.1).unwrap();
        assert!(
            !mesh.vertices.is_empty(),
            "mesh should have vertices"
        );
        assert!(
            mesh.indices.len().is_multiple_of(3),
            "indices should be triangle triples, got len={}",
            mesh.indices.len()
        );
        assert!(
            mesh.normals.is_some(),
            "mesh normals should be present"
        );
    }

    #[test]
    fn box_volume_10mm() {
        if !reify_kernel_occt::OCCT_AVAILABLE {
            eprintln!("skipping: OCCT not available");
            return;
        }
        let mut kernel = OcctKernel::new();
        let handle = kernel
            .execute(&GeometryOp::Box {
                width: mm(10.0),
                height: mm(10.0),
                depth: mm(10.0),
            })
            .unwrap();

        let result = kernel
            .query(&GeometryQuery::Volume(handle.id))
            .unwrap();
        match result {
            Value::Real(v) => {
                // 10mm = 0.01m, volume = 0.01³ = 1e-6 m³
                assert!(
                    (v - 1e-6).abs() < 1e-9,
                    "expected volume ≈ 1e-6 m³, got {}",
                    v
                );
            }
            other => panic!("expected Real, got {:?}", other),
        }
    }

    // --- task-311 integration tests (step-11) ---

    #[test]
    fn translate_10mm_box_50mm_x_centroid() {
        if !reify_kernel_occt::OCCT_AVAILABLE {
            eprintln!("skipping: OCCT not available");
            return;
        }
        let mut kernel = OcctKernel::new();
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
                dx: 0.05,
                dy: 0.0,
                dz: 0.0,
            })
            .unwrap();
        let centroid = kernel
            .query(&GeometryQuery::Centroid(translated.id))
            .unwrap();
        match centroid {
            Value::String(s) => {
                let x_start = s.find("\"x\":").unwrap() + 4;
                let x_end = s[x_start..].find([',', '}']).unwrap() + x_start;
                let x: f64 = s[x_start..x_end].parse().unwrap();
                assert!(
                    (x - 0.05).abs() < 1e-9,
                    "translate 50mm: centroid x should be ≈ 0.05, got {x}"
                );
            }
            other => panic!("expected String centroid, got {:?}", other),
        }
    }

    #[test]
    fn rotate_box_90deg_z_preserves_volume() {
        if !reify_kernel_occt::OCCT_AVAILABLE {
            eprintln!("skipping: OCCT not available");
            return;
        }
        let mut kernel = OcctKernel::new();
        let handle = kernel
            .execute(&GeometryOp::Box {
                width: mm(10.0),
                height: mm(10.0),
                depth: mm(10.0),
            })
            .unwrap();
        let rotated = kernel
            .execute(&GeometryOp::Rotate {
                target: handle.id,
                axis: [0.0, 0.0, 1.0],
                angle_rad: std::f64::consts::FRAC_PI_2,
            })
            .unwrap();
        let vol = kernel
            .query(&GeometryQuery::Volume(rotated.id))
            .unwrap();
        match vol {
            Value::Real(v) => {
                assert!(
                    (v - 1e-6).abs() < 1e-9,
                    "rotate 90°Z: volume should be preserved ≈ 1e-6 m³, got {v}"
                );
            }
            other => panic!("expected Value::Real, got {:?}", other),
        }
    }

    #[test]
    fn scale_box_2x_volume_becomes_8x() {
        if !reify_kernel_occt::OCCT_AVAILABLE {
            eprintln!("skipping: OCCT not available");
            return;
        }
        let mut kernel = OcctKernel::new();
        let handle = kernel
            .execute(&GeometryOp::Box {
                width: mm(10.0),
                height: mm(10.0),
                depth: mm(10.0),
            })
            .unwrap();
        let scaled = kernel
            .execute(&GeometryOp::Scale {
                target: handle.id,
                factor: 2.0,
            })
            .unwrap();
        let vol = kernel
            .query(&GeometryQuery::Volume(scaled.id))
            .unwrap();
        match vol {
            Value::Real(v) => {
                // Original volume = 1e-6 m³, scaled by 2x → 8e-6 m³
                assert!(
                    (v - 8e-6).abs() < 1e-9,
                    "scale(2.0): volume should be ≈ 8e-6 m³, got {v}"
                );
            }
            other => panic!("expected Value::Real, got {:?}", other),
        }
    }

    #[test]
    fn scale_identity_volume_unchanged() {
        if !reify_kernel_occt::OCCT_AVAILABLE {
            eprintln!("skipping: OCCT not available");
            return;
        }
        let mut kernel = OcctKernel::new();
        let handle = kernel
            .execute(&GeometryOp::Box {
                width: mm(10.0),
                height: mm(10.0),
                depth: mm(10.0),
            })
            .unwrap();
        let scaled = kernel
            .execute(&GeometryOp::Scale {
                target: handle.id,
                factor: 1.0,
            })
            .unwrap();
        let vol = kernel
            .query(&GeometryQuery::Volume(scaled.id))
            .unwrap();
        match vol {
            Value::Real(v) => {
                assert!(
                    (v - 1e-6).abs() < 1e-9,
                    "scale(1.0): volume should be unchanged ≈ 1e-6 m³, got {v}"
                );
            }
            other => panic!("expected Value::Real, got {:?}", other),
        }
    }

    #[test]
    fn rotate_around_point_preserves_volume() {
        if !reify_kernel_occt::OCCT_AVAILABLE {
            eprintln!("skipping: OCCT not available");
            return;
        }
        let mut kernel = OcctKernel::new();
        let handle = kernel
            .execute(&GeometryOp::Box {
                width: mm(10.0),
                height: mm(10.0),
                depth: mm(10.0),
            })
            .unwrap();
        let rotated = kernel
            .execute(&GeometryOp::RotateAround {
                target: handle.id,
                point: [0.05, 0.0, 0.0],
                axis: [0.0, 0.0, 1.0],
                angle_rad: std::f64::consts::FRAC_PI_2,
            })
            .unwrap();
        let vol = kernel
            .query(&GeometryQuery::Volume(rotated.id))
            .unwrap();
        match vol {
            Value::Real(v) => {
                assert!(
                    (v - 1e-6).abs() < 1e-9,
                    "rotate_around: volume should be preserved ≈ 1e-6 m³, got {v}"
                );
            }
            other => panic!("expected Value::Real, got {:?}", other),
        }

        // Verify centroid of rotated shape matches analytical expectation.
        // OCCT boxes are centered at origin, so centroid = (0, 0, 0).
        // Rotating 90° around Z through pivot (0.05, 0, 0):
        //   translate(-pivot): (0 - 0.05, 0, 0) = (-0.05, 0, 0)
        //   Rz(PI/2): (x,y) → (-y, x) = (0, -0.05, 0)
        //   translate(+pivot): (0 + 0.05, -0.05, 0) = (0.05, -0.05, 0)
        let centroid_val = kernel
            .query(&GeometryQuery::Centroid(rotated.id))
            .unwrap();
        let (cx, cy, cz) = match &centroid_val {
            Value::String(s) => {
                let parse_coord = |key: &str| -> f64 {
                    let prefix = format!("\"{}\":", key);
                    let start = s.find(&prefix).unwrap() + prefix.len();
                    let end = s[start..].find([',', '}']).unwrap() + start;
                    s[start..end].parse().unwrap()
                };
                (parse_coord("x"), parse_coord("y"), parse_coord("z"))
            }
            other => panic!("expected String centroid, got {:?}", other),
        };
        assert!(
            (cx - 0.05).abs() < 1e-4,
            "rotate_around centroid x should be ≈ 0.05, got {cx}"
        );
        assert!(
            (cy - (-0.05)).abs() < 1e-4,
            "rotate_around centroid y should be ≈ -0.05, got {cy}"
        );
        assert!(
            cz.abs() < 1e-4,
            "rotate_around centroid z should be ≈ 0.0, got {cz}"
        );

        // Execute a plain Rotate (no pivot) with same axis/angle and confirm
        // centroid differs — proving the pivot point parameter is actually used.
        let plain_rotated = kernel
            .execute(&GeometryOp::Rotate {
                target: handle.id,
                axis: [0.0, 0.0, 1.0],
                angle_rad: std::f64::consts::FRAC_PI_2,
            })
            .unwrap();
        let plain_centroid = kernel
            .query(&GeometryQuery::Centroid(plain_rotated.id))
            .unwrap();
        match plain_centroid {
            Value::String(s) => {
                let parse_coord = |key: &str| -> f64 {
                    let prefix = format!("\"{}\":", key);
                    let start = s.find(&prefix).unwrap() + prefix.len();
                    let end = s[start..].find([',', '}']).unwrap() + start;
                    s[start..end].parse().unwrap()
                };
                let px = parse_coord("x");
                let py = parse_coord("y");
                let diff_x = (px - cx).abs();
                let diff_y = (py - cy).abs();
                assert!(
                    diff_x > 0.01 || diff_y > 0.01,
                    "rotate_around centroid should differ from plain rotate by > 0.01 in at least one coord; \
                     rotate_around=({cx}, {cy}), plain=({px}, {py})"
                );
            }
            other => panic!("expected String centroid, got {:?}", other),
        }
    }
}
