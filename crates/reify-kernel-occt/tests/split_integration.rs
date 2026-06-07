//! Integration tests for `GeometryKernel::execute_split` on `OcctKernel` (task 4190).
//!
//! These tests verify the BRepAlgoAPI_Splitter-backed planar split:
//! - A centred 10 mm box bisected by the z=0 plane yields two 500 mm³ halves.
//! - A plane that does not intersect the box yields a length-1 result.
//! - Every result piece STEP-exports to a non-empty buffer.
//!
//! `make_box` in the kernel produces a **centred** box (spanning z∈[−5,+5] mm
//! for a 10 mm cube) per `occt_wrapper.h:103`, so the z=0 plane bisects it
//! into two exact 5×10×10 mm = 500 mm³ halves.
//! The non-intersecting case uses plane_origin [0,0,0.05] (outside the box).
//!
//! All dimensions are in SI metres (kernel boundary convention).

#![cfg(has_occt)]

use reify_ir::{ExportFormat, GeometryKernel, GeometryOp, GeometryQuery, Value};
use reify_kernel_occt::OcctKernel;

/// 10 mm in metres.
const BOX_10MM: f64 = 10e-3;
/// Expected volume of one 5×10×10 mm piece, in m³.
const HALF_VOLUME_M3: f64 = 500e-9; // 500 mm³
/// Full box volume: 10³ mm³ = 1000 mm³.
const FULL_VOLUME_M3: f64 = 1000e-9;
/// 2% relative tolerance (generous for OCCT exact bisection numerics).
const REL_TOL: f64 = 0.02;

fn within_rel(actual: f64, expected: f64, tol: f64) -> bool {
    (actual - expected).abs() <= tol * expected
}

/// Build a centred 10 mm × 10 mm × 10 mm box kernel and return the kernel + handle.
fn make_box_kernel() -> (OcctKernel, reify_ir::GeometryHandleId) {
    let mut kernel = OcctKernel::new();
    let h = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(BOX_10MM),
            height: Value::Real(BOX_10MM),
            depth: Value::Real(BOX_10MM),
        })
        .expect("10mm cube creation should succeed");
    (kernel, h.id)
}

/// Bisect the centred 10 mm box with the z=0 plane: expect 2 halves, each ~500 mm³.
///
/// RED until step-4 adds the C++ `split_shape`, FFI, lib.rs, and handle.rs wiring.
#[test]
fn split_bisect_10mm_box_yields_two_500mm3_halves() {
    let (mut kernel, box_id) = make_box_kernel();

    let op = GeometryOp::Split {
        target: box_id,
        plane_origin: [0.0, 0.0, 0.0],
        plane_normal: [0.0, 0.0, 1.0],
    };
    let pieces = kernel
        .execute_split(&op)
        .expect("split of a 10mm box by z=0 plane should succeed");

    assert_eq!(
        pieces.len(),
        2,
        "bisecting a 10mm cube with z=0 must yield exactly 2 pieces, got {}",
        pieces.len()
    );

    for (i, piece_id) in pieces.iter().enumerate() {
        let vol_val = kernel
            .query(&GeometryQuery::Volume(*piece_id))
            .unwrap_or_else(|e| panic!("volume query on piece {i} failed: {e:?}"));
        let vol = vol_val.as_f64().expect("volume should be a numeric Value");
        assert!(
            within_rel(vol, HALF_VOLUME_M3, REL_TOL),
            "piece {i} volume should be ~{HALF_VOLUME_M3:.3e} m³ (±{:.0}%), got {vol:.6e} m³",
            REL_TOL * 100.0,
        );
    }
}

/// A plane that does not intersect the box (origin outside the solid) yields 1 solid.
///
/// The box spans z∈[−5,+5] mm; plane at z=0.05 m = 50 mm is outside → no cut.
///
/// RED until step-4 implements execute_split.
#[test]
fn split_non_intersecting_plane_yields_one_solid() {
    let (mut kernel, box_id) = make_box_kernel();

    // z=0.05 m is well outside the 10mm box (box spans z∈[-0.005, +0.005] m)
    let op = GeometryOp::Split {
        target: box_id,
        plane_origin: [0.0, 0.0, 0.05],
        plane_normal: [0.0, 0.0, 1.0],
    };
    let pieces = kernel
        .execute_split(&op)
        .expect("split with non-intersecting plane should succeed (yields 1 piece)");

    assert_eq!(
        pieces.len(),
        1,
        "non-intersecting plane must yield exactly 1 piece (original solid), got {}",
        pieces.len()
    );

    let vol_val = kernel
        .query(&GeometryQuery::Volume(pieces[0]))
        .expect("volume query on unsplit piece should succeed");
    let vol = vol_val.as_f64().expect("volume should be numeric");
    assert!(
        within_rel(vol, FULL_VOLUME_M3, REL_TOL),
        "unsplit piece volume should be ~{FULL_VOLUME_M3:.3e} m³ (±{:.0}%), got {vol:.6e} m³",
        REL_TOL * 100.0,
    );
}

/// Every piece returned by execute_split must STEP-export to a non-empty buffer.
///
/// RED until step-4 implements execute_split.
#[test]
fn split_pieces_step_export_non_empty() {
    let (mut kernel, box_id) = make_box_kernel();

    let op = GeometryOp::Split {
        target: box_id,
        plane_origin: [0.0, 0.0, 0.0],
        plane_normal: [0.0, 0.0, 1.0],
    };
    let pieces = kernel
        .execute_split(&op)
        .expect("split should succeed for the STEP-export test");

    assert_eq!(pieces.len(), 2, "bisect must yield 2 pieces for STEP test");

    for (i, piece_id) in pieces.iter().enumerate() {
        let mut buf = Vec::new();
        kernel
            .export(*piece_id, ExportFormat::Step, &mut buf)
            .unwrap_or_else(|e| panic!("STEP export of piece {i} failed: {e:?}"));
        assert!(
            !buf.is_empty(),
            "STEP export of piece {i} must produce a non-empty buffer"
        );
        let content = String::from_utf8_lossy(&buf);
        assert!(
            content.contains("ISO-10303-21"),
            "STEP export of piece {i} must contain ISO-10303-21 header"
        );
    }
}
