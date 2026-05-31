//! End-to-end smoke test for `length(Curve)` and `perimeter(Surface)`
//! (task 3622, PRD `docs/prds/v0_3/kernel-geometry-queries.md` §9 KGQ-ν).
//!
//! Two assertions:
//!
//! 1. **COMPILE-LEVEL** (always) — `length_perimeter.ri` parses and compiles
//!    with no error-severity diagnostics. Both names are registered in
//!    `units.rs` under `GEOMETRY_QUERY_NAMES` (KGQ-α chain), so the cells
//!    resolve to `Scalar<Length>` at compile time. At DSL eval time both
//!    cells evaluate to `Value::Undef` (pre-Phase-3 selector-chaining
//!    limitation, engine_build.rs:3942-3949) — same as normal_smoke.ri and
//!    adjacent_faces.ri. Only Warning diagnostics are emitted, not Errors.
//!
//! 2. **OCCT-BACKED RUNTIME** (gated on `reify_kernel_occt::OCCT_AVAILABLE`) —
//!    Spawn a real `OcctKernelHandle`, build geometry directly at the kernel
//!    level, and confirm the underlying query composition is live:
//!
//!    - `(length)` Build `box(10mm, 20mm, 30mm)`, extract all 12 edges, query
//!      `GeometryQuery::EdgeLength` per edge. Assert each result is within
//!      1e-9 relative of one of {0.010, 0.020, 0.030} m, all three values
//!      appear across the 12 edges, and edges[0]'s length is in that set.
//!
//!    - `(perimeter)` Build `box(10mm, 10mm, 10mm)`, extract faces[0] (one
//!      face of the cube), `extract_edges(faces[0])` → assert exactly 4
//!      boundary edges, sum their `EdgeLength` → assert within 1e-9 of
//!      0.040 m (40 mm = 4 × 10 mm).
//!
//! Modelled on `kernel_queries_curvature_smoke.rs` and
//! `kernel_queries_normal_smoke.rs`.

use reify_ir::{GeometryOp, GeometryQuery, Value};
use reify_test_support::{errors_only, parse_and_compile_with_stdlib};

const LENGTH_PERIMETER_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/kernel_queries/length_perimeter.ri"
);

/// Pins the user-observable signal for KGQ-ν:
///
/// - `length_perimeter.ri` compiles with no error diagnostics, confirming
///   `length`/`perimeter` are registered in `units.rs`.
/// - `GeometryQuery::EdgeLength` on box edges through the real OCCT kernel
///   returns exact {10,20,30}mm values.
/// - `extract_edges(face)` + `EdgeLength` sum yields 40mm for a 10mm cube face.
///
/// Skips the OCCT-backed assertions cleanly when OCCT is not available.
#[test]
fn length_perimeter_compiles_and_occt_queries_match_expected() {
    // ── assertion 1: fixture exists and compiles with no ERROR diagnostics ────

    let source = std::fs::read_to_string(LENGTH_PERIMETER_PATH).expect(
        "examples/kernel_queries/length_perimeter.ri should exist (task 3622 step-6)",
    );

    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "examples/kernel_queries/length_perimeter.ri should compile with no \
         error-severity diagnostics (Warnings from Undef eval are acceptable \
         pre-Phase-3), got:\n{:#?}",
        errors_only(&compiled)
    );

    // ── assertion 2: real-OCCT kernel query composition is live ──────────────

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping real-OCCT assertions: OCCT not available");
        return;
    }

    let kernel = reify_kernel_occt::OcctKernelHandle::spawn();

    // ── 2a: EdgeLength on box(10mm, 20mm, 30mm) edges ────────────────────────

    let box_handle = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(0.010),
            height: Value::Real(0.020),
            depth: Value::Real(0.030),
        })
        .expect("box(10mm, 20mm, 30mm) should build successfully");

    let edges = kernel
        .extract_edges(box_handle.id)
        .expect("extract_edges should succeed on box");
    assert_eq!(edges.len(), 12, "box should have exactly 12 edges");

    let expected_lengths = [0.010_f64, 0.020_f64, 0.030_f64];
    let mut seen = [false; 3];
    for &edge_id in &edges {
        let reply = kernel
            .query(&GeometryQuery::EdgeLength(edge_id))
            .expect("EdgeLength should succeed on box edge");
        let length_m = match reply {
            Value::Real(v) => v,
            other => panic!("EdgeLength should return Value::Real, got: {other:?}"),
        };
        let matched_idx = expected_lengths
            .iter()
            .enumerate()
            .find(|&(_, &exp)| (length_m - exp).abs() / exp < 1e-9)
            .map(|(i, _)| i);
        assert!(
            matched_idx.is_some(),
            "box edge length {length_m} m is not within 1e-9 relative of any of \
             {{10,20,30}}mm"
        );
        if let Some(i) = matched_idx {
            seen[i] = true;
        }
    }
    assert!(
        seen.iter().all(|&s| s),
        "all three lengths {{10,20,30}}mm must appear across the 12 box edges; \
         seen={seen:?}"
    );

    // edges[0]'s length must be one of the three expected values.
    let first_reply = kernel
        .query(&GeometryQuery::EdgeLength(edges[0]))
        .expect("EdgeLength on edges[0] should succeed");
    let first_len = match first_reply {
        Value::Real(v) => v,
        other => panic!("EdgeLength should return Value::Real, got: {other:?}"),
    };
    assert!(
        expected_lengths
            .iter()
            .any(|exp| (first_len - exp).abs() / exp < 1e-9),
        "edges[0] length {first_len} m must be in {{10,20,30}}mm"
    );

    // ── 2b: extract_edges(face) + EdgeLength sum on box(10mm, 10mm, 10mm) ───

    let cube_handle = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(0.010),
            height: Value::Real(0.010),
            depth: Value::Real(0.010),
        })
        .expect("box(10mm, 10mm, 10mm) should build successfully");

    let faces = kernel
        .extract_faces(cube_handle.id)
        .expect("extract_faces should succeed on cube");
    assert!(!faces.is_empty(), "cube should have at least one face");

    let face_edges = kernel
        .extract_edges(faces[0])
        .expect("extract_edges on cube face[0] should succeed");
    assert_eq!(
        face_edges.len(),
        4,
        "a square face of a cube should have exactly 4 boundary edges"
    );

    let mut perim_m = 0.0_f64;
    for &edge_id in &face_edges {
        let reply = kernel
            .query(&GeometryQuery::EdgeLength(edge_id))
            .expect("EdgeLength should succeed on cube face edge");
        perim_m += match reply {
            Value::Real(v) => v,
            other => panic!("EdgeLength should return Value::Real, got: {other:?}"),
        };
    }
    let expected_perim = 0.040_f64; // 4 × 10mm
    let rel_err = (perim_m - expected_perim).abs() / expected_perim;
    assert!(
        rel_err < 1e-9,
        "perimeter of cube face (4×10mm) should be {expected_perim} m, \
         got {perim_m} m (rel_err={rel_err})"
    );
}
