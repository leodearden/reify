//! Integration tests for OCCT boolean-result NORMALIZATION (task #7054).
//!
//! `BRepAlgoAPI_Fuse` / `_Cut` / `_Common` always hand back their answer wrapped
//! in a bare `TopoDS_COMPOUND`, even when the operands merged into a single
//! body, and they leave every coplanar seam face/edge introduced by the boolean
//! in place. Storing that raw shape produces three user-visible defects:
//!
//!   1. `is_watertight` / `is_closed` return **false** on a genuinely closed
//!      solid, because both guard on `SOLID | COMPSOLID | SHELL` and a COMPOUND
//!      is none of those (cpp/occt_wrapper.cpp).
//!   2. `query_distance` / `min_clearance` miss containment: OCCT's
//!      `BRepExtrema_DistShapeShape` only runs its inner-solution /
//!      SolidTreatment test when a top-level operand IS a `TopAbs_SOLID`, so a
//!      fully-buried probe reads the boundary-to-boundary distance instead of 0.
//!   3. Coplanar seam fragmentation: a fuse chain leaves each logical planar
//!      face split into many same-domain fragments, so a bbox-based edge
//!      selector picks up dozens of phantom seam edges instead of the handful of
//!      real ones, and a curated fillet over that selection produces a body with
//!      a wildly inflated face count.
//!
//! The fix is `normalize_boolean_result()` in cpp/occt_wrapper.cpp: unwrap the
//! COMPOUND to the tightest topology-preserving type (one solid → bare SOLID;
//! many → COMPSOLID; none → untouched), then run
//! `ShapeUpgrade_UnifySameDomain` to merge same-domain faces and edges. All four
//! boolean entry points (`boolean_fuse`, `boolean_cut`, `boolean_common`,
//! `fuse_shape_list`) and the shared `extract_boolean_history` chokepoint route
//! through it.
//!
//! # Measured OCCT 7.8 baseline (architect probe, this worktree)
//!
//! Numbers below are direct measurements, not estimates — they are what makes
//! RED distinguishable from GREEN without re-deriving anything:
//!
//! | quantity                                                  | RED (today) | GREEN (fixed) |
//! |-----------------------------------------------------------|-------------|---------------|
//! | `rounded_box(100,60,20,10)` fuse chain: faces / edges      | 50 / 88     | 10 / 24       |
//! | edges selected at the top face (`edges_at_height` predicate)| 40          | 8             |
//! | faces after a 1 mm fillet of that rim selection            | 66          | 18            |
//! | cut-derived container → distance to a fully buried probe   | 90 mm       | 0 mm          |
//! | two abutting 10 mm cubes fused: faces                      | 10          | 6             |
//!
//! Two measured facts that constrain how these tests may be written:
//!
//!   * The filleted body's VOLUME is bit-identical before and after the fix
//!     (118218.498221 mm³ / 150.13749274 g in both probe runs) — the 40 phantom
//!     seam edges sweep exactly the same material as the 8 real ones. A
//!     mass-only assertion for symptom 3 is therefore a guaranteed FALSE GREEN;
//!     the RED signal must be anchored on face/edge/selection COUNTS.
//!   * The un-unified rim fillet SUCCEEDED on this fixture (66 faces) rather
//!     than throwing. Symptom 3's hard failure is fixture-dependent ("which
//!     fuse seams land at the selected height decides"), so no test here may
//!     assert that the fillet errors today.
//!
//! # Observability note
//!
//! `ffi::ffi::shape_type_name` is crate-private (`mod ffi;` at lib.rs:46), so an
//! integration test cannot read the raw `TopAbs` name. The reachable
//! equivalents, via `OcctKernel::repr_of` and `GeometryQuery::IsWatertight`:
//!
//!   * `"Solid"`    ⇔ `BRepKind::Solid` (only `"Solid"` maps to it —
//!     `brep_kind_of_shape` at lib.rs:660 collapses `"CompSolid"`/`"Compound"`
//!     onto `BRepKind::Compound`).
//!   * `"CompSolid"` ⇔ `BRepKind::Compound` **and** `IsWatertight == true`
//!     (a bare `"Compound"` fails the `SOLID|COMPSOLID|SHELL` guard, so
//!     watertightness is what separates the two).
//!
//! Gated on `has_occt` like the 44 sibling OCCT integration modules.

#![cfg(has_occt)]

use reify_ir::{BRepKind, GeometryHandleId, GeometryOp, GeometryQuery, Value};
use reify_kernel_occt::OcctKernel;

/// Build an axis-aligned cube of side `side`, centred on the origin
/// (`make_box` centres its output — cpp/occt_wrapper.cpp).
fn cube(kernel: &mut OcctKernel, side: f64) -> GeometryHandleId {
    kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(side),
            height: Value::Real(side),
            depth: Value::Real(side),
        })
        .expect("box creation should succeed")
        .id
}

/// Translate `target`, returning a fresh handle (the source is left intact).
fn translated(
    kernel: &mut OcctKernel,
    target: GeometryHandleId,
    dx: f64,
    dy: f64,
    dz: f64,
) -> GeometryHandleId {
    kernel
        .execute(&GeometryOp::Translate {
            target,
            dx,
            dy,
            dz,
        })
        .expect("translate should succeed")
        .id
}

fn bool_query(kernel: &OcctKernel, q: GeometryQuery) -> bool {
    match kernel.query(&q) {
        Ok(Value::Bool(b)) => b,
        other => panic!("expected Value::Bool from {:?}, got {:?}", q, other),
    }
}

fn real_query(kernel: &OcctKernel, q: GeometryQuery) -> f64 {
    match kernel.query(&q) {
        Ok(Value::Real(v)) => v,
        other => panic!("expected Value::Real from {:?}, got {:?}", q, other),
    }
}

/// Assert that `id` is a genuinely closed single body: `BRepKind::Solid` (which
/// only `shape_type_name == "Solid"` maps to) plus `IsWatertight` and
/// `IsClosed`, both of which reject a bare COMPOUND at their
/// `SOLID|COMPSOLID|SHELL` type guard.
fn assert_watertight_solid(kernel: &OcctKernel, id: GeometryHandleId, what: &str) {
    assert_eq!(
        kernel.repr_of(id),
        Some(BRepKind::Solid),
        "{what}: a boolean result that merged into ONE closed body must be stored as \
         BRepKind::Solid, not the raw COMPOUND wrapper BRepAlgoAPI hands back"
    );
    assert!(
        bool_query(kernel, GeometryQuery::IsWatertight(id)),
        "{what}: IsWatertight must be true — a bare COMPOUND fails the \
         SOLID|COMPSOLID|SHELL guard even when the body it wraps is closed"
    );
    assert!(
        bool_query(kernel, GeometryQuery::IsClosed(id)),
        "{what}: IsClosed must be true for the same reason as IsWatertight"
    );
}

// ---------------------------------------------------------------------------
// Symptom 1 — a closed boolean result must report watertight / closed.
// ---------------------------------------------------------------------------

#[test]
fn binary_fuse_of_overlapping_boxes_is_a_watertight_solid() {
    let mut kernel = OcctKernel::new();
    let a = cube(&mut kernel, 10.0);
    let b_raw = cube(&mut kernel, 10.0);
    // +5 in X: a half-box overlap, so the union is a single 15x10x10 body.
    let b = translated(&mut kernel, b_raw, 5.0, 0.0, 0.0);
    let fused = kernel
        .execute(&GeometryOp::Union { left: a, right: b })
        .expect("union should succeed")
        .id;
    assert_watertight_solid(&kernel, fused, "boolean_fuse of overlapping boxes");
}

#[test]
fn binary_cut_of_notch_from_box_is_a_watertight_solid() {
    let mut kernel = OcctKernel::new();
    let block = cube(&mut kernel, 10.0); // spans [-5, +5] on each axis
    let notch_raw = cube(&mut kernel, 4.0);
    // Centre the notch on the +X+Y+Z corner so it removes a corner bite and
    // leaves a single connected solid.
    let notch = translated(&mut kernel, notch_raw, 5.0, 5.0, 5.0);
    let cut = kernel
        .execute(&GeometryOp::Difference {
            left: block,
            right: notch,
        })
        .expect("difference should succeed")
        .id;
    assert_watertight_solid(&kernel, cut, "boolean_cut of a corner notch");
}

#[test]
fn binary_common_of_overlapping_boxes_is_a_watertight_solid() {
    let mut kernel = OcctKernel::new();
    let a = cube(&mut kernel, 10.0);
    let b_raw = cube(&mut kernel, 10.0);
    let b = translated(&mut kernel, b_raw, 5.0, 0.0, 0.0);
    let common = kernel
        .execute(&GeometryOp::Intersection { left: a, right: b })
        .expect("intersection should succeed")
        .id;
    assert_watertight_solid(&kernel, common, "boolean_common of overlapping boxes");
}

/// A fuse of two NON-touching boxes cannot collapse to one solid, so the
/// normalization must rewrap it as a COMPSOLID — the shape type that both
/// preserves the two-body component count AND passes the watertight guard.
/// This mirrors `fuse_shape_list`'s reviewed multi-solid contract (task 5213).
///
/// `brep_kind_of_shape` collapses `"CompSolid"` and `"Compound"` onto the same
/// `BRepKind::Compound`, so the repr alone cannot tell them apart —
/// `IsWatertight` is what separates a COMPSOLID from a bare COMPOUND.
#[test]
fn binary_fuse_of_disjoint_boxes_is_a_watertight_compsolid() {
    let mut kernel = OcctKernel::new();
    let a = cube(&mut kernel, 10.0); // spans [-5, +5]
    let b_raw = cube(&mut kernel, 10.0);
    let b = translated(&mut kernel, b_raw, 40.0, 0.0, 0.0); // spans [35, 45] — disjoint
    let fused = kernel
        .execute(&GeometryOp::Union { left: a, right: b })
        .expect("disjoint union should succeed")
        .id;

    assert_eq!(
        kernel.repr_of(fused),
        Some(BRepKind::Compound),
        "a disjoint multi-solid fuse must be stored as the multi-body \
         BRepKind::Compound classified from the real shape, NOT the hardcoded \
         BRepKind::Solid the binary boolean arms stamp today"
    );
    assert!(
        bool_query(&kernel, GeometryQuery::IsWatertight(fused)),
        "a disjoint fuse must be rewrapped as a COMPSOLID, which passes the \
         SOLID|COMPSOLID|SHELL guard; a bare COMPOUND would fail it"
    );
}

// ---------------------------------------------------------------------------
// Symptom 2 — containment must be detected through a boolean-derived container.
//
// OCCT's `BRepExtrema_DistShapeShape` only runs its inner-solution /
// SolidTreatment test when a top-level operand IS a `TopAbs_SOLID`. Wrapped in
// a COMPOUND, a fully-buried probe reads the boundary-to-boundary distance
// instead of 0 — a SILENT false negative for containment/interference checks.
// ---------------------------------------------------------------------------

/// Outer wall half-thickness for both containment fixtures: the 200-cube spans
/// [-100, +100] and the 20-cube probe spans [-10, +10], so the un-normalized
/// boundary-to-boundary answer is exactly 100 - 10 = 90.
const BURIED_PROBE_RED_DISTANCE: f64 = 90.0;

#[test]
fn fully_buried_probe_reads_zero_distance_through_cut_derived_container() {
    let mut kernel = OcctKernel::new();
    let block = cube(&mut kernel, 200.0); // [-100, +100]^3
    let notch_raw = cube(&mut kernel, 40.0);
    // Corner notch at (+100, +100, +100) → removes [80, 100]^3. It never comes
    // near the origin-centred probe, so it cannot influence the answer.
    let notch = translated(&mut kernel, notch_raw, 100.0, 100.0, 100.0);
    let outer = kernel
        .execute(&GeometryOp::Difference {
            left: block,
            right: notch,
        })
        .expect("difference should succeed")
        .id;
    let probe = cube(&mut kernel, 20.0); // [-10, +10]^3, fully buried

    let d = real_query(
        &kernel,
        GeometryQuery::Distance {
            from: outer,
            to: probe,
        },
    );
    assert_eq!(
        d, 0.0,
        "a fully-buried probe must read distance 0 through a cut-derived \
         container; {BURIED_PROBE_RED_DISTANCE} is the boundary-to-boundary \
         answer OCCT gives when the container is a bare COMPOUND rather than a \
         SOLID (measured RED: {BURIED_PROBE_RED_DISTANCE})"
    );
}

#[test]
fn fully_buried_probe_reads_zero_distance_through_fuse_derived_container() {
    let mut kernel = OcctKernel::new();
    let a = cube(&mut kernel, 200.0); // [-100, +100]^3
    let b_raw = cube(&mut kernel, 200.0);
    let b = translated(&mut kernel, b_raw, 50.0, 0.0, 0.0); // [-50, +150] in X
    let outer = kernel
        .execute(&GeometryOp::Union { left: a, right: b })
        .expect("union should succeed")
        .id;
    let probe = cube(&mut kernel, 20.0); // [-10, +10]^3, fully buried

    let d = real_query(
        &kernel,
        GeometryQuery::Distance {
            from: outer,
            to: probe,
        },
    );
    assert_eq!(
        d, 0.0,
        "a fully-buried probe must read distance 0 through a fuse-derived \
         container (measured RED: {BURIED_PROBE_RED_DISTANCE})"
    );
}
