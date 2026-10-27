//! Geometry queries and export formats through the public kernel: distance, moment of
//! inertia, and STEP / STL / 3MF export (with 3MF options).

#![cfg(has_occt)]

use reify_ir::{ExportFormat, ExportOptions, ExportWarning, GeometryOp, GeometryQuery, Value};
use reify_kernel_occt::OcctKernel;

// --- Query tests ---

#[test]
fn distance_between_shapes() {
    let mut kernel = OcctKernel::new();
    // Create box_a at origin (10x10x10, centered)
    let box_a = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    // Create box_b and translate to (50,0,0)
    let box_b_raw = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    let box_b = kernel
        .execute(&GeometryOp::Translate {
            target: box_b_raw.id,
            dx: 50.0,
            dy: 0.0,
            dz: 0.0,
        })
        .unwrap();
    // Distance between them: box_a goes from -5 to 5 on X,
    // box_b goes from 45 to 55 on X, so gap = 40
    let dist = kernel
        .query(&GeometryQuery::Distance {
            from: box_a.id,
            to: box_b.id,
        })
        .unwrap();
    match dist {
        Value::Real(d) => {
            assert!((d - 40.0).abs() < 1.0, "expected distance ~40, got {d}");
        }
        other => panic!("expected Value::Real, got {:?}", other),
    }
}

#[test]
fn moment_of_inertia_box() {
    let mut kernel = OcctKernel::new();
    // Create a 10x10x10 box
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    // Query MoI around Z axis
    let moi = kernel
        .query(&GeometryQuery::MomentOfInertia {
            handle: box_h.id,
            axis: [0.0, 0.0, 1.0],
        })
        .unwrap();
    match moi {
        Value::Real(v) => {
            // For a box of uniform density (mass=volume=1000),
            // MoI around Z through centroid = M/12 * (w^2 + d^2) = 1000/12 * (100 + 100)
            // = 1000/12 * 200 ≈ 16666.7
            assert!(v > 0.0, "MoI should be positive, got {v}");
            assert!(
                (v - 16666.7).abs() < 100.0,
                "expected MoI ~16666.7, got {v}"
            );
        }
        other => panic!("expected Value::Real, got {:?}", other),
    }
}

// --- Integration tests ---

#[test]
fn new_ops_export_step() {
    let mut kernel = OcctKernel::new();
    // Mirror
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    let translated = kernel
        .execute(&GeometryOp::Translate {
            target: box_h.id,
            dx: 15.0,
            dy: 0.0,
            dz: 0.0,
        })
        .unwrap();
    let mirrored = kernel
        .execute(&GeometryOp::Mirror {
            target: translated.id,
            plane_origin: [0.0, 0.0, 0.0],
            plane_normal: [1.0, 0.0, 0.0],
        })
        .unwrap();
    let mut buf = Vec::new();
    kernel
        .export(mirrored.id, ExportFormat::Step, &mut buf)
        .unwrap();
    let content = String::from_utf8(buf).unwrap();
    assert!(
        content.contains("ISO-10303-21"),
        "STEP should contain ISO header"
    );

    // LinearPattern
    let pat = kernel
        .execute(&GeometryOp::LinearPattern {
            target: box_h.id,
            direction: [1.0, 0.0, 0.0],
            count: 3,
            spacing: Value::Real(15.0),
        })
        .unwrap();
    let mut buf2 = Vec::new();
    kernel
        .export(pat.id, ExportFormat::Step, &mut buf2)
        .unwrap();
    let content2 = String::from_utf8(buf2).unwrap();
    assert!(
        content2.contains("ISO-10303-21"),
        "pattern STEP should contain ISO header"
    );
}

#[test]
fn new_ops_export_stl() {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();

    let mut buf = Vec::new();
    kernel
        .export(box_h.id, ExportFormat::Stl, &mut buf)
        .expect("OCCT Stl export of a 10x10x10 box must succeed");

    // Binary STL structure checks (B1/B2 from the PRD)
    let count = u32::from_le_bytes(buf[80..84].try_into().unwrap());
    assert!(count > 0, "STL triangle count must be > 0 for a solid box");
    assert_eq!(
        buf.len(),
        84 + 50 * count as usize,
        "STL byte length must equal 84 + 50*count"
    );

    // AABB check: parse all vertex positions from the binary body and assert
    // each axis extent ≈ 10000.0 (within 10.0).
    //
    // The box is 10 MODEL units = 10 SI metres, and `write_stl_binary` emits
    // millimetres (task #6187), so the expected extent and its tolerance are
    // BOTH the same physical quantities this test has always verified — 10 m
    // and 0.01 m — merely re-expressed in the writer's millimetre regime.
    // The tolerance is scaled alongside the value on purpose: tightening it
    // would pin a fidelity nothing here has measured.
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for tri in 0..count as usize {
        let base = 84 + tri * 50;
        // Skip 12-byte facet normal; each of the 3 vertices is 12 bytes
        for v in 0..3usize {
            let vbase = base + 12 + v * 12;
            for axis in 0..3usize {
                let bytes: [u8; 4] = buf[vbase + axis * 4..vbase + axis * 4 + 4]
                    .try_into()
                    .unwrap();
                let coord = f32::from_le_bytes(bytes);
                if coord < min[axis] {
                    min[axis] = coord;
                }
                if coord > max[axis] {
                    max[axis] = coord;
                }
            }
        }
    }
    for axis in 0..3usize {
        let extent = max[axis] - min[axis];
        assert!(
            (extent - 10000.0).abs() < 10.0,
            "axis {} extent should be ≈10000.0 mm, got {extent}",
            axis
        );
    }
}

/// Degenerate input: CircleProfile with negative radius returns GeometryError::OperationFailed.
///
/// Export a 10×10×10 box to 3MF via the OCCT kernel and assert valid OPC
/// package structure on raw bytes (Stored=uncompressed, no zip reader needed).
///
/// Mirrors `new_ops_export_stl` (:5711). RED before step-6: `ExportFormat::ThreeMF`
/// does not exist → workspace compile fails.
#[test]
fn new_ops_export_3mf() {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();

    let mut buf = Vec::new();
    kernel
        .export(box_h.id, ExportFormat::ThreeMF, &mut buf)
        .expect("OCCT ThreeMF export of a 10x10x10 box must succeed");

    // Stored/uncompressed: OPC part names and model XML appear literally in raw bytes.
    let raw = &buf;

    // The ZIP local-file-header entry for 3D/3dmodel.model must be present.
    assert!(
        raw.windows(b"3D/3dmodel.model".len())
            .any(|w| w == b"3D/3dmodel.model"),
        "raw bytes must contain '3D/3dmodel.model'"
    );

    // At least one <triangle element must appear in the model XML.
    let tri_needle = b"<triangle ";
    let tri_count = raw.windows(tri_needle.len()).filter(|w| *w == tri_needle).count();
    assert!(tri_count > 0, "OCCT 3MF export must contain at least one <triangle>");
}

/// OCCT export_with_options threads color through to the 3MF <basematerials> element.
///
/// (a) color present — export_with_options with ExportOptions{ color: Some(Rgb8{0x88,0x99,0xAA}),
///     include_colors:true, ..Default::default() } → raw bytes contain `displaycolor="#8899AAFF"`;
///     returned warnings Vec is EMPTY (color suppresses NoMaterials).
/// (b) color absent — ExportOptions{ color:None, include_colors:true, ..Default::default() }
///     → raw bytes do NOT contain `displaycolor`; returned warnings ==
///     vec![ExportWarning::ThreeMfNoMaterials].
///
/// RED until step-6: the current export_with_options catch-all delegates ThreeMF to
/// export() (default ThreeMfOptions), ignoring color entirely.
#[test]
fn export_with_options_3mf_threads_color() {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();

    // (a) color present → basematerials emitted, warning suppressed
    {
        let mut buf = Vec::new();
        let warnings = kernel
            .export_with_options(
                box_h.id,
                ExportFormat::ThreeMF,
                &ExportOptions {
                    color: Some(reify_ir::Rgb8 { r: 0x88, g: 0x99, b: 0xAA }),
                    include_colors: true,
                    ..Default::default()
                },
                &mut buf,
            )
            .expect("export_with_options ThreeMF must succeed");

        let displaycolor_needle = b"displaycolor=\"#8899AAFF\"";
        assert!(
            buf.windows(displaycolor_needle.len())
                .any(|w| w == displaycolor_needle),
            "bytes must contain displaycolor=\"#8899AAFF\" when color is Some"
        );
        assert!(
            warnings.is_empty(),
            "no warnings expected when color is provided; got {:?}",
            warnings
        );
    }

    // (b) color absent → no basematerials, ThreeMfNoMaterials warning
    {
        let mut buf = Vec::new();
        let warnings = kernel
            .export_with_options(
                box_h.id,
                ExportFormat::ThreeMF,
                &ExportOptions {
                    color: None,
                    include_colors: true,
                    ..Default::default()
                },
                &mut buf,
            )
            .expect("export_with_options ThreeMF must succeed");

        let displaycolor_needle = b"displaycolor";
        assert!(
            !buf.windows(displaycolor_needle.len())
                .any(|w| w == displaycolor_needle),
            "bytes must NOT contain displaycolor when color is None"
        );
        assert!(
            !buf.windows(b"<basematerials".len())
                .any(|w| w == b"<basematerials"),
            "bytes must NOT contain <basematerials when color is None"
        );
        assert_eq!(
            warnings,
            vec![ExportWarning::ThreeMfNoMaterials],
            "expected ThreeMfNoMaterials warning when color is None and include_colors is true"
        );
    }
}
