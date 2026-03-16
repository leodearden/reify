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
    assert!(mesh.vertices.len() > 0, "mesh should have vertices");
    assert!(mesh.indices.len() % 3 == 0, "indices should be triangle triples");
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
    #[test]
    #[ignore = "requires OCCT kernel implementation"]
    fn create_box_export_step() {
        // Create box → export STEP → non-empty, contains "ISO-10303-21"
    }

    #[test]
    #[ignore = "requires OCCT kernel implementation"]
    fn cylinder_volume_query() {
        // Create cylinder → query volume → ≈ π·r²·h
    }

    #[test]
    #[ignore = "requires OCCT kernel implementation"]
    fn boolean_difference() {
        // Boolean difference → export STEP
    }

    #[test]
    #[ignore = "requires OCCT kernel implementation"]
    fn fillet_edges() {
        // Fillet edges of box
    }

    #[test]
    #[ignore = "requires OCCT kernel implementation"]
    fn translate_centroid() {
        // Translate → query centroid → verify displacement
    }

    #[test]
    #[ignore = "requires OCCT kernel implementation"]
    fn invalid_reference_error() {
        // Invalid op reference → GeometryError::InvalidReference
    }

    #[test]
    #[ignore = "requires OCCT kernel implementation"]
    fn zero_dimension_error() {
        // Zero-dimension primitive → GeometryError::OperationFailed
    }

    #[test]
    #[ignore = "requires OCCT kernel implementation"]
    fn tessellation_valid_mesh() {
        // Tessellation → valid mesh (vertices > 0, indices % 3 == 0)
    }

    #[test]
    #[ignore = "requires OCCT kernel implementation"]
    fn box_volume_10mm() {
        // Volume query: 10×10×10mm box → ≈ 1e-6 m³
    }
}
