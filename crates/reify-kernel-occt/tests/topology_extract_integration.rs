//! Integration tests for raw topology extractors `extract_edges` /
//! `extract_faces` on the public OcctKernel API (task 318).
//!
//! These selectors materialize each unique sub-shape (deduplicated by
//! `IsSame`) into a fresh kernel handle whose ReprKind reflects the
//! sub-shape kind (`Edge` or `Face`).

#![cfg(has_occt)]

use reify_kernel_occt::OcctKernel;
use reify_types::{GeometryHandleId, GeometryOp, GeometryQuery, QueryError, ReprKind, Value};

/// Helper: build a kernel containing one box of the given mm dimensions
/// (converted to SI metres at the kernel boundary so geometric queries
/// like `SurfaceArea`/`EdgeLength` return values in m² / m) and return
/// the kernel + its handle id.
fn box_kernel(width_mm: f64, height_mm: f64, depth_mm: f64) -> (OcctKernel, GeometryHandleId) {
    let mut kernel = OcctKernel::new();
    let h = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(width_mm * 1e-3),
            height: Value::Real(height_mm * 1e-3),
            depth: Value::Real(depth_mm * 1e-3),
        })
        .expect("Box creation should succeed");
    (kernel, h.id)
}

#[test]
fn extract_edges_box_returns_twelve_distinct_handles() {
    let (mut kernel, box_id) = box_kernel(10.0, 20.0, 30.0);

    let edges = kernel
        .extract_edges(box_id)
        .expect("extract_edges on a valid box should succeed");

    assert_eq!(
        edges.len(),
        12,
        "a 10x20x30 box has exactly 12 unique edges, got {}",
        edges.len()
    );

    let mut seen = std::collections::HashSet::new();
    for id in &edges {
        assert_ne!(
            *id, box_id,
            "extracted edge handle must differ from the source box handle"
        );
        assert_ne!(
            *id,
            GeometryHandleId::INVALID,
            "extracted edge handle must not be the INVALID sentinel"
        );
        assert!(
            seen.insert(*id),
            "duplicate edge handle id {:?} in extract_edges result",
            id
        );
    }
}

#[test]
fn extract_edges_handles_have_edge_repr_kind() {
    let (mut kernel, box_id) = box_kernel(10.0, 20.0, 30.0);

    let edges = kernel
        .extract_edges(box_id)
        .expect("extract_edges on a valid box should succeed");

    for id in &edges {
        let repr = kernel
            .repr_of(*id)
            .unwrap_or_else(|| panic!("repr_of({:?}) returned None for an extracted edge", id));
        assert_eq!(
            repr,
            ReprKind::Edge,
            "extracted edge handle {:?} should have ReprKind::Edge, got {:?}",
            id,
            repr
        );
    }
}

#[test]
fn extract_faces_box_returns_six_distinct_face_handles() {
    let (mut kernel, box_id) = box_kernel(10.0, 20.0, 30.0);

    let faces = kernel
        .extract_faces(box_id)
        .expect("extract_faces on a valid box should succeed");

    assert_eq!(
        faces.len(),
        6,
        "a 10x20x30 box has exactly 6 unique faces, got {}",
        faces.len()
    );

    let mut seen = std::collections::HashSet::new();
    for id in &faces {
        assert_ne!(
            *id, box_id,
            "extracted face handle must differ from the source box handle"
        );
        assert_ne!(
            *id,
            GeometryHandleId::INVALID,
            "extracted face handle must not be the INVALID sentinel"
        );
        assert!(
            seen.insert(*id),
            "duplicate face handle id {:?} in extract_faces result",
            id
        );
        let repr = kernel
            .repr_of(*id)
            .unwrap_or_else(|| panic!("repr_of({:?}) returned None for an extracted face", id));
        assert_eq!(
            repr,
            ReprKind::Face,
            "extracted face handle {:?} should have ReprKind::Face, got {:?}",
            id,
            repr
        );
    }
}

#[test]
fn extract_faces_face_handles_have_correct_surface_area() {
    // 10x20x30 mm box → 6 faces in 3 axis-aligned pairs:
    //   - 2 × (10mm × 20mm) = 2 × 200e-6 m²
    //   - 2 × (10mm × 30mm) = 2 × 300e-6 m²
    //   - 2 × (20mm × 30mm) = 2 × 600e-6 m²
    let (mut kernel, box_id) = box_kernel(10.0, 20.0, 30.0);
    let faces = kernel
        .extract_faces(box_id)
        .expect("extract_faces on a valid box should succeed");

    let mut areas: Vec<f64> = faces
        .iter()
        .map(|id| match kernel.query(&GeometryQuery::SurfaceArea(*id)) {
            Ok(Value::Real(a)) => a,
            other => panic!("SurfaceArea({:?}) returned unexpected value: {:?}", id, other),
        })
        .collect();
    areas.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let expected = [
        200e-6, 200e-6, // 10 × 20
        300e-6, 300e-6, // 10 × 30
        600e-6, 600e-6, // 20 × 30
    ];
    let tol = 1e-9;
    assert_eq!(areas.len(), expected.len());
    for (got, want) in areas.iter().zip(expected.iter()) {
        assert!(
            (got - want).abs() < tol,
            "extracted-face area mismatch: got {got}, want {want} (tol={tol}). \
             full sorted areas: {:?}",
            areas
        );
    }
}

#[test]
fn query_edge_length_returns_correct_value_for_extracted_box_edge() {
    // 10x20x30 mm box → 12 edges in 3 axis-aligned groups of 4:
    //   - 4 × 10 mm = 4 × 0.010 m
    //   - 4 × 20 mm = 4 × 0.020 m
    //   - 4 × 30 mm = 4 × 0.030 m
    let (mut kernel, box_id) = box_kernel(10.0, 20.0, 30.0);
    let edges = kernel
        .extract_edges(box_id)
        .expect("extract_edges on a valid box should succeed");

    let mut lengths: Vec<f64> = edges
        .iter()
        .map(|id| match kernel.query(&GeometryQuery::EdgeLength(*id)) {
            Ok(Value::Real(l)) => l,
            other => panic!("EdgeLength({:?}) returned unexpected value: {:?}", id, other),
        })
        .collect();
    lengths.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let expected = [
        0.010, 0.010, 0.010, 0.010, // x-axis edges
        0.020, 0.020, 0.020, 0.020, // y-axis edges
        0.030, 0.030, 0.030, 0.030, // z-axis edges
    ];
    let tol = 1e-9;
    assert_eq!(lengths.len(), expected.len());
    for (got, want) in lengths.iter().zip(expected.iter()) {
        assert!(
            (got - want).abs() < tol,
            "edge length mismatch: got {got}, want {want} (tol={tol}). \
             full sorted lengths: {:?}",
            lengths
        );
    }
}

/// Parse a `Value::String` formatted by the kernel as
/// `{"x":...,"y":...,"z":...}` (the JSON encoding used by Centroid,
/// EdgeTangent, FaceNormal) into a 3-tuple of f64.
fn parse_xyz(v: &Value) -> (f64, f64, f64) {
    let s = match v {
        Value::String(s) => s,
        other => panic!("expected Value::String, got {:?}", other),
    };
    let parsed: serde_json::Value = serde_json::from_str(s)
        .unwrap_or_else(|e| panic!("failed to parse {:?} as JSON: {e}", s));
    let x = parsed["x"].as_f64().expect("missing x");
    let y = parsed["y"].as_f64().expect("missing y");
    let z = parsed["z"].as_f64().expect("missing z");
    (x, y, z)
}

#[test]
fn query_face_normal_top_face_of_box_is_plus_z() {
    // 10x10x10 mm box centered at origin → z ∈ [-5e-3, +5e-3]. The "top"
    // face has centroid (0, 0, +5e-3); its outward normal should be ±(0,0,1).
    let (mut kernel, box_id) = box_kernel(10.0, 10.0, 10.0);
    let faces = kernel
        .extract_faces(box_id)
        .expect("extract_faces on a valid box should succeed");

    // Find the face whose centroid lies on z = +5e-3.
    let target_z = 5e-3;
    let pos_tol = 1e-9;
    let top = faces
        .iter()
        .find(|id| {
            let c = kernel
                .query(&GeometryQuery::Centroid(**id))
                .expect("Centroid query");
            let (_x, _y, z) = parse_xyz(&c);
            (z - target_z).abs() < pos_tol
        })
        .copied()
        .expect("a 10x10x10 box centered at origin must have a top face at z=+5e-3");

    let normal = kernel
        .query(&GeometryQuery::FaceNormal(top))
        .expect("FaceNormal query should succeed");
    let (nx, ny, nz) = parse_xyz(&normal);

    let dir_tol = 1e-9;
    assert!(
        nx.abs() < dir_tol,
        "top-face normal x should be ≈0, got {nx}"
    );
    assert!(
        ny.abs() < dir_tol,
        "top-face normal y should be ≈0, got {ny}"
    );
    assert!(
        (nz.abs() - 1.0).abs() < dir_tol,
        "top-face normal |z| should be ≈1, got {nz}"
    );
}

/// Parse the JSON Value::String produced by `BoundingBox` queries.
fn parse_bbox(v: &Value) -> (f64, f64, f64, f64, f64, f64) {
    let s = match v {
        Value::String(s) => s,
        other => panic!("expected Value::String, got {:?}", other),
    };
    let parsed: serde_json::Value = serde_json::from_str(s)
        .unwrap_or_else(|e| panic!("failed to parse {:?} as JSON: {e}", s));
    let xmin = parsed["xmin"].as_f64().expect("missing xmin");
    let ymin = parsed["ymin"].as_f64().expect("missing ymin");
    let zmin = parsed["zmin"].as_f64().expect("missing zmin");
    let xmax = parsed["xmax"].as_f64().expect("missing xmax");
    let ymax = parsed["ymax"].as_f64().expect("missing ymax");
    let zmax = parsed["zmax"].as_f64().expect("missing zmax");
    (xmin, ymin, zmin, xmax, ymax, zmax)
}

#[test]
fn query_edge_tangent_returns_unit_vector_along_axis() {
    // 10×20×30 mm box → 12 axis-aligned edges. For each axis we pick one
    // representative edge (identified by its bounding-box extents) and
    // assert its `EdgeTangent` is ±(unit axis vector).
    let (mut kernel, box_id) = box_kernel(10.0, 20.0, 30.0);
    let edges = kernel
        .extract_edges(box_id)
        .expect("extract_edges on a valid box should succeed");

    // Tolerances: bbox extent has OCCT's geometric tolerance margin baked in
    // (BRepBndLib enlarges the box by the shape's stored tolerance, typically
    // ~1e-7), so the extent comparison must accommodate that. The tangent
    // direction is computed analytically and is tight to ~1e-9.
    let extent_tol = 1e-6;
    let dir_tol = 1e-9;

    // Helper: returns Some(edge_id) for an edge whose bbox extents along
    // (x, y, z) approximately match the given target extents.
    let find_edge_with_extents = |kernel: &mut OcctKernel,
                                  edges: &[GeometryHandleId],
                                  ex: f64,
                                  ey: f64,
                                  ez: f64|
     -> GeometryHandleId {
        for id in edges {
            let bb = kernel
                .query(&GeometryQuery::BoundingBox(*id))
                .expect("BoundingBox query");
            let (xmin, ymin, zmin, xmax, ymax, zmax) = parse_bbox(&bb);
            let dx = xmax - xmin;
            let dy = ymax - ymin;
            let dz = zmax - zmin;
            if (dx - ex).abs() < extent_tol
                && (dy - ey).abs() < extent_tol
                && (dz - ez).abs() < extent_tol
            {
                return *id;
            }
        }
        panic!(
            "no edge found with bbox extents ({ex}, {ey}, {ez}) within tol={extent_tol}"
        );
    };

    // Each axis-aligned edge of a 10×20×30 mm box has zero extent on the
    // two off-axis components and ~length on its axis. Convert mm→m: the
    // axis-aligned lengths are 0.010, 0.020, 0.030 m respectively.
    let x_edge = find_edge_with_extents(&mut kernel, &edges, 0.010, 0.0, 0.0);
    let y_edge = find_edge_with_extents(&mut kernel, &edges, 0.0, 0.020, 0.0);
    let z_edge = find_edge_with_extents(&mut kernel, &edges, 0.0, 0.0, 0.030);

    // x-aligned edge: tangent should be ±(1, 0, 0).
    let t_x = kernel
        .query(&GeometryQuery::EdgeTangent(x_edge))
        .expect("EdgeTangent on x-aligned edge");
    let (tx, ty, tz) = parse_xyz(&t_x);
    assert!(
        (tx.abs() - 1.0).abs() < dir_tol,
        "x-edge tangent |x| should be ≈1, got tx={tx}"
    );
    assert!(ty.abs() < dir_tol, "x-edge tangent y should be ≈0, got {ty}");
    assert!(tz.abs() < dir_tol, "x-edge tangent z should be ≈0, got {tz}");

    // y-aligned edge: tangent should be ±(0, 1, 0).
    let t_y = kernel
        .query(&GeometryQuery::EdgeTangent(y_edge))
        .expect("EdgeTangent on y-aligned edge");
    let (tx, ty, tz) = parse_xyz(&t_y);
    assert!(tx.abs() < dir_tol, "y-edge tangent x should be ≈0, got {tx}");
    assert!(
        (ty.abs() - 1.0).abs() < dir_tol,
        "y-edge tangent |y| should be ≈1, got ty={ty}"
    );
    assert!(tz.abs() < dir_tol, "y-edge tangent z should be ≈0, got {tz}");

    // z-aligned edge: tangent should be ±(0, 0, 1).
    let t_z = kernel
        .query(&GeometryQuery::EdgeTangent(z_edge))
        .expect("EdgeTangent on z-aligned edge");
    let (tx, ty, tz) = parse_xyz(&t_z);
    assert!(tx.abs() < dir_tol, "z-edge tangent x should be ≈0, got {tx}");
    assert!(ty.abs() < dir_tol, "z-edge tangent y should be ≈0, got {ty}");
    assert!(
        (tz.abs() - 1.0).abs() < dir_tol,
        "z-edge tangent |z| should be ≈1, got tz={tz}"
    );
}

#[test]
fn extract_edges_invalid_handle_returns_invalid_reference() {
    // Fresh kernel — no shapes registered, so handle id 999 is unknown.
    let mut kernel = OcctKernel::new();
    let bad = GeometryHandleId(999);

    let result = kernel.extract_edges(bad);

    match result {
        Err(QueryError::InvalidHandle(id)) => {
            assert_eq!(
                id, bad,
                "InvalidHandle should carry the bad handle id verbatim"
            );
        }
        Ok(v) => panic!("expected Err(InvalidHandle), got Ok({:?})", v),
        Err(other) => panic!("expected Err(InvalidHandle), got Err({:?})", other),
    }
}

#[test]
fn extract_edges_after_fillet_count_differs_from_box() {
    // Filleting a box rounds each of its 12 sharp edges. Each rounded
    // edge becomes a curved fillet surface bounded by new edges, and
    // every original face is also re-trimmed so its boundary edges
    // are split. The resulting topology has many more edges than the
    // 12 of a sharp box.
    //
    // We don't lock in an exact count (it varies with OCCT version
    // and fillet algorithm) — only that the count is *not* 12, which
    // is enough to confirm `extract_edges` traverses the post-fillet
    // shape rather than reporting a stale or "passthrough" count.
    let (mut kernel, box_id) = box_kernel(10.0, 10.0, 10.0);
    let filleted = kernel
        .execute(&GeometryOp::Fillet {
            target: box_id,
            radius: Value::Real(0.001),
        })
        .expect("Fillet of a 10mm box with 1mm radius should succeed");

    let edges = kernel
        .extract_edges(filleted.id)
        .expect("extract_edges on a filleted box should succeed");

    assert_ne!(
        edges.len(),
        12,
        "filleting a box must change its edge count from 12 \
         (got {}, which suggests extract_edges did not traverse the post-fillet shape)",
        edges.len()
    );

    // Sanity: the new edges should also be distinct, non-INVALID, and
    // none should equal the source handle (mirrors the invariants on
    // the pre-fillet path).
    let mut seen = std::collections::HashSet::new();
    for id in &edges {
        assert_ne!(*id, box_id, "extracted edge handle must differ from box_id");
        assert_ne!(*id, filleted.id, "extracted edge handle must differ from filleted.id");
        assert_ne!(*id, GeometryHandleId::INVALID, "extracted edge handle must not be INVALID");
        assert!(seen.insert(*id), "duplicate edge handle id {:?}", id);
    }
}
