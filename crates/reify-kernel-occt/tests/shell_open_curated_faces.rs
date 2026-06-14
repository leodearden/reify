//! Integration test for the curated per-face shell path —
//! `OcctKernel::shell_solid_faces` (task 4187, γ step-7/step-8).
//!
//! Pins the curated face-SELECTION shell: `BRepOffsetAPI_MakeThickSolid`
//! applied to a CURATED SUBSET of a solid's faces (vs the numeric
//! `shell_shape` path which uses explicit 0-based face indices).
//!
//! Gated on `#![cfg(has_occt)]` and `OCCT_AVAILABLE` so non-OCCT builds
//! skip without linker errors (mirrors `per_edge_fillet.rs` and
//! `shell_shape_oob_face_index_integration.rs`).
//!
//! RED premise: `shell_solid_faces` does not exist yet (added in step-8);
//! this file will FAIL TO BUILD until the method ships.  Test (a) would
//! additionally fail at runtime because the execute arm currently ignores
//! `open_face_handles`.

#![cfg(has_occt)]

use reify_ir::{GeometryError, GeometryHandleId, GeometryOp, GeometryQuery, Value};
use reify_kernel_occt::{OCCT_AVAILABLE, OcctKernel};

/// Box dimensions in SI metres (the kernel stores lengths in metres):
/// 30 × 20 × 10 mm — NON-cube so every face-pair has a DISTINCT area,
/// making face-identity errors observable via volume.
const BOX_LX_M: f64 = 30.0e-3;
const BOX_LY_M: f64 = 20.0e-3;
const BOX_LZ_M: f64 = 10.0e-3;
/// Shell thickness: 1 mm.
const THICKNESS_M: f64 = 1.0e-3;

/// Build a 30×20×10 mm box and return a ready-to-use kernel + its handle id.
fn build_noncube_box() -> (OcctKernel, GeometryHandleId) {
    let mut kernel = OcctKernel::new();
    let box_h = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(BOX_LX_M),
            height: Value::Real(BOX_LY_M),
            depth: Value::Real(BOX_LZ_M),
        })
        .expect("30×20×10 mm box should build");
    (kernel, box_h.id)
}

/// Parse a `Value::String` of the form `{"x":0,"y":0,"z":1}` into `[f64; 3]`.
/// This is the wire format returned by `GeometryQuery::FaceNormal`.
fn parse_xyz(val: &Value) -> [f64; 3] {
    let s = match val {
        Value::String(s) => s,
        other => panic!("expected Value::String from FaceNormal, got {:?}", other),
    };
    // Minimal parser: strip `{`, `}`, split on `,`, extract the numeric part
    // after each `:`.
    let inner = s.trim_start_matches('{').trim_end_matches('}');
    let mut parts = inner.split(',');
    let parse_component = |part: Option<&str>| -> f64 {
        let p = part.expect("FaceNormal JSON must have x, y, z components");
        let colon = p.find(':').expect("FaceNormal JSON field must contain ':'");
        p[colon + 1..].trim().parse::<f64>().expect("FaceNormal component must be a float")
    };
    let x = parse_component(parts.next());
    let y = parse_component(parts.next());
    let z = parse_component(parts.next());
    [x, y, z]
}

/// Query the SI volume (m³) of a solid handle.
fn volume_of(kernel: &mut OcctKernel, id: GeometryHandleId) -> f64 {
    kernel
        .query(&GeometryQuery::Volume(id))
        .expect("volume query should succeed")
        .as_f64()
        .expect("volume value should be numeric")
}

/// (a) FACE-IDENTITY VOLUME — the +Z top face is identified by its
/// outward normal ≈ (0, 0, 1); shelling THAT face at t=1mm must produce a
/// volume V where:
///   0 < V < V_solid  (we have a hollow solid, not empty and not full)
///   V ≈ V_wall(+Z)  within 3%
///
/// V_wall(+Z) = Lx·Ly·Lz − (Lx−2t)·(Ly−2t)·(Lz−t)
///            = 0.03×0.02×0.01 − 0.028×0.018×0.009
///            = 6.000e-6 − 4.536e-6 = 1.464e-6 m³
///
/// Opening the +X face instead gives V_wall(+X) ≈ 1.824e-6 m³  (24.6%
/// different from V_wall(+Z)), so a mis-ordered face map that opens the
/// wrong face causes a genuine volume error > 3% → this test PINS face
/// IDENTITY, not just "some face was opened".
#[test]
fn shell_solid_faces_top_face_volume_pins_face_identity() {
    if !OCCT_AVAILABLE {
        return;
    }
    let (mut kernel, box_id) = build_noncube_box();

    // --- Analytic expected volumes (SI m³) ---
    let v_solid = BOX_LX_M * BOX_LY_M * BOX_LZ_M;
    let t = THICKNESS_M;
    let v_wall = BOX_LX_M * BOX_LY_M * BOX_LZ_M
        - (BOX_LX_M - 2.0 * t) * (BOX_LY_M - 2.0 * t) * (BOX_LZ_M - t);
    // v_wall ≈ 1.464e-6 m³,  v_solid = 6.0e-6 m³

    // --- Extract all 6 faces and identify the +Z top face ---
    let faces = kernel
        .extract_faces(box_id)
        .expect("extract_faces should succeed on a solid box");
    assert_eq!(
        faces.len(),
        6,
        "a box has exactly 6 faces, got {}",
        faces.len()
    );

    let mut top_face: Option<GeometryHandleId> = None;
    for &fh in &faces {
        let normal_val = kernel
            .query(&GeometryQuery::FaceNormal(fh))
            .expect("FaceNormal query should succeed on a box face");
        let [nx, ny, nz] = parse_xyz(&normal_val);
        // +Z top face: normal ≈ (0, 0, 1); use a generous tolerance
        // (axis-aligned box face normals are exact from OCCT).
        if (nz - 1.0).abs() < 0.01 && nx.abs() < 0.01 && ny.abs() < 0.01 {
            top_face = Some(fh);
            break;
        }
    }
    let top_id = top_face.expect(
        "could not find +Z top face by FaceNormal ≈ (0,0,1) — \
         the 30×20×10 mm box must have a +Z face",
    );

    // --- Execute the curated shell via open_face_handles ---
    let shell_h = kernel
        .execute(&GeometryOp::Shell {
            target: box_id,
            thickness: Value::Real(THICKNESS_M),
            faces_to_remove: vec![],
            open_face_handles: vec![top_id],
        })
        .expect("shell_open(box, 1mm, [+Z face]) should succeed");

    let v_shell = volume_of(&mut kernel, shell_h.id);

    // Bound checks
    assert!(
        v_shell > 0.0,
        "shelled solid must have positive volume, got {v_shell}"
    );
    assert!(
        v_shell < v_solid,
        "shelled solid must have less volume than the original solid: \
         v_shell={v_shell}, v_solid={v_solid}"
    );

    // Analytic tolerance: MakeThickSolidByJoin on an axis-aligned box
    // yields clean prismatic walls; V_wall is exact to OCCT's offset
    // tolerance (~1e-3 mm).  We allow 3% relative tolerance.
    let rel_err = (v_shell - v_wall).abs() / v_wall;
    assert!(
        rel_err < 0.03,
        "shell volume {v_shell:.6e} m³ must be within 3% of analytic \
         open-top wall volume {v_wall:.6e} m³ (error={:.1}%)",
        rel_err * 100.0
    );
}

/// (b) EMPTY — `shell_solid_faces` with an empty face slice must return
/// `Err(OperationFailed)`.  The curated path must never accept an empty
/// selection.
/// Mirrors `draft_faces_empty_selection_returns_error` (lib.rs:6710).
#[test]
fn shell_solid_faces_empty_selection_returns_error() {
    if !OCCT_AVAILABLE {
        return;
    }
    let (mut kernel, box_id) = build_noncube_box();
    let result = kernel.shell_solid_faces(box_id, THICKNESS_M, &[]);
    match result {
        Err(GeometryError::OperationFailed(_)) => {}
        Err(other) => panic!(
            "expected OperationFailed for empty selection, got {:?}",
            other
        ),
        Ok(_) => panic!("shell_solid_faces with empty selection must be rejected"),
    }
}

/// (c) NON-MEMBER — a face handle extracted from a DIFFERENT solid must
/// cause `shell_solid_faces` to return `Err(OperationFailed)`.
/// The membership check must be tight: a foreign face handle is not in
/// the parent's extract_faces list and must be rejected rather than
/// opening an arbitrary face or panicking.
#[test]
fn shell_solid_faces_foreign_face_returns_error() {
    if !OCCT_AVAILABLE {
        return;
    }
    let (mut kernel, box1_id) = build_noncube_box();

    // Build a second box in the SAME kernel; extract one of its faces.
    let box2_h = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(5.0e-3),
            height: Value::Real(5.0e-3),
            depth: Value::Real(5.0e-3),
        })
        .expect("second 5×5×5 mm box should build");
    let box2_faces = kernel
        .extract_faces(box2_h.id)
        .expect("extract_faces should succeed on the second box");
    let foreign_face = box2_faces[0];

    // Attempting to shell box1 with a face from box2 must fail.
    let result = kernel.shell_solid_faces(box1_id, THICKNESS_M, &[foreign_face]);
    match result {
        Err(GeometryError::OperationFailed(_)) => {}
        Err(other) => panic!(
            "expected OperationFailed for foreign face handle, got {:?}",
            other
        ),
        Ok(_) => panic!("shell_solid_faces with a foreign face handle must be rejected"),
    }
}
