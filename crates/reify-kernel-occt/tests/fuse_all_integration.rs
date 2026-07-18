//! Integration tests for the single-pass n-ary fuse primitive
//! `OcctKernel::fuse_all(&[GeometryHandleId]) -> Result<GeometryHandle>`
//! (task 5213, Lever 1).
//!
//! `fuse_all` collects all the argument shapes into a single
//! `BRepAlgoAPI_Fuse` (SetArguments/SetTools) pass, replacing the O(N²)
//! pairwise-accumulator loop the pattern realizers used.  These tests pin its
//! correctness directly:
//! - (a) IDENTITY: a single-shape fuse returns that shape's volume unchanged.
//! - (b) DISJOINT: three pairwise-disjoint unit boxes fuse to a watertight
//!   multi-solid result whose total volume is the exact 3.0 m³ sum.
//! - (c) OVERLAPPING: three overlapping boxes fuse to a watertight single solid
//!   whose volume equals a reference computed in-test by chained binary fuses.
//! - (d) EMPTY: an empty argument list is rejected with `Err`.
//!
//! All shapes are built in SI metres (kernel-boundary convention).  Boxes from
//! `GeometryOp::Box` are centred at the origin, so a unit (1 m) box spans
//! [-0.5, +0.5] on each axis; `Translate` produces a fresh, independent handle
//! (the source is left unmodified), which the disjoint/overlapping fixtures use
//! to place copies.
//!
//! `#![cfg(has_occt)]` gates the whole file to hosts where the OCCT shared
//! libraries are present — the same gate every other kernel integration-test
//! file in this crate uses; the verify pipeline always sets it.

#![cfg(has_occt)]

use reify_ir::{GeometryHandleId, GeometryOp, GeometryQuery, Value};
use reify_kernel_occt::OcctKernel;

/// Relative tolerance for volume comparisons.  OCCT boolean volume numerics
/// are exact to well within 1e-6 for these axis-aligned box unions.
const REL_TOL: f64 = 1e-6;

fn within_rel(actual: f64, expected: f64, tol: f64) -> bool {
    (actual - expected).abs() <= tol * expected.abs().max(1.0)
}

/// Build a fresh 1 m unit cube centred at the origin; return its handle id.
fn unit_box(kernel: &mut OcctKernel) -> GeometryHandleId {
    kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(1.0),
            height: Value::Real(1.0),
            depth: Value::Real(1.0),
        })
        .expect("unit box creation should succeed")
        .id
}

/// Translate `src` by `dx` metres along +X, returning a fresh handle id.
fn translated_x(kernel: &mut OcctKernel, src: GeometryHandleId, dx: f64) -> GeometryHandleId {
    kernel
        .execute(&GeometryOp::Translate {
            target: src,
            dx,
            dy: 0.0,
            dz: 0.0,
        })
        .expect("translate should succeed")
        .id
}

/// Query the (positive) volume of a handle in m³.
fn volume_of(kernel: &mut OcctKernel, id: GeometryHandleId) -> f64 {
    let v = kernel
        .query(&GeometryQuery::Volume(id))
        .expect("volume query should succeed")
        .as_f64()
        .expect("volume should be numeric");
    assert!(v > 0.0, "volume must be positive, got {v}");
    v
}

/// Query whether a handle is watertight.
fn is_watertight(kernel: &OcctKernel, id: GeometryHandleId) -> bool {
    match kernel.query(&GeometryQuery::IsWatertight(id)) {
        Ok(Value::Bool(b)) => b,
        other => panic!("IsWatertight should return Ok(Bool(_)), got {other:?}"),
    }
}

/// (a) IDENTITY: `fuse_all` of a single box returns a shape with the box's volume.
#[test]
fn fuse_all_single_box_is_identity_volume() {
    let mut kernel = OcctKernel::new();
    let b = unit_box(&mut kernel);
    let expected = volume_of(&mut kernel, b);

    let fused = kernel
        .fuse_all(&[b])
        .expect("fuse_all of a single box must succeed");
    let got = volume_of(&mut kernel, fused.id);

    assert!(
        within_rel(got, expected, REL_TOL),
        "single-box fuse_all volume should equal the box volume {expected:.9}, got {got:.9}"
    );
}

/// (b) DISJOINT: three pairwise-disjoint unit boxes fuse to a watertight
/// multi-solid result whose total volume is the exact 3.0 m³ sum.
#[test]
fn fuse_all_three_disjoint_boxes_sums_volume_and_is_watertight() {
    let mut kernel = OcctKernel::new();
    // b0 spans x∈[-0.5,0.5]; b2 spans [1.5,2.5]; b4 spans [3.5,4.5] — 1 m gaps.
    let b0 = unit_box(&mut kernel);
    let b2 = translated_x(&mut kernel, b0, 2.0);
    let b4 = translated_x(&mut kernel, b0, 4.0);

    let fused = kernel
        .fuse_all(&[b0, b2, b4])
        .expect("fuse_all of three disjoint boxes must succeed");

    let got = volume_of(&mut kernel, fused.id);
    assert!(
        within_rel(got, 3.0, REL_TOL),
        "three disjoint unit boxes must fuse to volume 3.0 m³, got {got:.9}"
    );
    assert!(
        is_watertight(&kernel, fused.id),
        "the disjoint multi-solid fuse result must be watertight"
    );
}

/// (c) OVERLAPPING: three overlapping boxes fuse to a watertight single solid
/// whose volume equals a reference computed by chained binary fuses.
#[test]
fn fuse_all_three_overlapping_boxes_matches_chained_reference() {
    let mut kernel = OcctKernel::new();
    // b0 [-0.5,0.5]; b_half [0,1]; b_one [0.5,1.5] — pairwise overlapping.
    let b0 = unit_box(&mut kernel);
    let b_half = translated_x(&mut kernel, b0, 0.5);
    let b_one = translated_x(&mut kernel, b0, 1.0);

    // Independent reference: chained binary fuse of the SAME three boxes.
    let ref01 = kernel
        .execute(&GeometryOp::Union { left: b0, right: b_half })
        .expect("reference union b0∪b_half must succeed")
        .id;
    let ref012 = kernel
        .execute(&GeometryOp::Union { left: ref01, right: b_one })
        .expect("reference union (b0∪b_half)∪b_one must succeed")
        .id;
    let ref_vol = volume_of(&mut kernel, ref012);

    let fused = kernel
        .fuse_all(&[b0, b_half, b_one])
        .expect("fuse_all of three overlapping boxes must succeed");
    let got = volume_of(&mut kernel, fused.id);

    assert!(
        within_rel(got, ref_vol, REL_TOL),
        "overlapping fuse_all volume {got:.9} must equal the chained-fuse reference {ref_vol:.9}"
    );
    assert!(
        is_watertight(&kernel, fused.id),
        "the overlapping single-solid fuse result must be watertight"
    );
}

/// (d) EMPTY: an empty argument list is rejected with `Err`.
#[test]
fn fuse_all_empty_list_is_error() {
    let mut kernel = OcctKernel::new();
    let result = kernel.fuse_all(&[]);
    assert!(
        result.is_err(),
        "fuse_all of an empty handle list must return Err, got {result:?}"
    );
}
