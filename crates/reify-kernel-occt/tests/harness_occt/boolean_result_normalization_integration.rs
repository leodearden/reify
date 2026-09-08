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

use crate::common;
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

// ---------------------------------------------------------------------------
// Symptom 3 — coplanar seam fragmentation in a multi-body fuse chain.
//
// A `rounded_box` is not a kernel primitive: the compiler desugars it
// (`emit_rounded_union_compose`, reify-compiler geometry.rs) into a LEFT-FOLDED
// chain of five binary fuses over two boxes and four corner cylinders. Every
// fuse in that chain leaves the coplanar seam where its operands met, so the
// six logical faces of the prism arrive as dozens of same-domain fragments —
// and a bbox-based edge selector then picks up every phantom seam edge.
// ---------------------------------------------------------------------------

/// `rounded_box(width=100, depth=60, height=20, corner_r=10)`.
const RB_WIDTH: f64 = 100.0;
const RB_DEPTH: f64 = 60.0;
const RB_HEIGHT: f64 = 20.0;
const RB_CORNER_R: f64 = 10.0;

/// Analytic volume of that rounded box: the plan-view area is
/// `w*d - 4*(r² - πr²/4)` = 6000 - 4(100 - 25π) = 5914.159265 mm², times the
/// 20 mm height. INVARIANT across the fix — unification merges faces, it never
/// moves material — so this is a sanity guard, not the RED signal.
const RB_VOLUME: f64 = 118283.18530717959;

/// Volume after a 1 mm fillet of the 8 top-rim edges. Also invariant across the
/// fix: the 40 phantom seam edges of the un-unified rim sweep exactly the same
/// material as the 8 real ones (measured identical to 6 decimal places in both
/// probe runs). Anchoring the RED signal on volume would be a FALSE GREEN.
const RB_FILLETED_VOLUME: f64 = 118218.498221;

/// Build the compiler's `rounded_box` desugar directly on the kernel: box A
/// (`width` × `depth-2r` × `height`), box B (`width-2r` × `depth` × `height`),
/// and four `cylinder(r, height)` corner posts translated to
/// `(±(w/2-r), ±(d/2-r), -height/2)`, left-folded through FIVE successive
/// `Union`s in the emitter's own order: A∪B, then ∪(+,+), (+,-), (-,+), (-,-).
///
/// `make_box` centres its output on the origin and `make_cylinder` grows from
/// z=0, which is why the corner posts carry the `-height/2` dz the emitter
/// gives them.
fn rounded_box_fuse_chain(kernel: &mut OcctKernel) -> GeometryHandleId {
    let body_a = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(RB_WIDTH),
            height: Value::Real(RB_DEPTH - 2.0 * RB_CORNER_R),
            depth: Value::Real(RB_HEIGHT),
        })
        .expect("rounded_box body A should build")
        .id;
    let body_b = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(RB_WIDTH - 2.0 * RB_CORNER_R),
            height: Value::Real(RB_DEPTH),
            depth: Value::Real(RB_HEIGHT),
        })
        .expect("rounded_box body B should build")
        .id;

    let mut acc = kernel
        .execute(&GeometryOp::Union {
            left: body_a,
            right: body_b,
        })
        .expect("A union B should succeed")
        .id;

    // Emitter's corner order: (+,+), (+,-), (-,+), (-,-).
    for (sx, sy) in [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
        let post = kernel
            .execute(&GeometryOp::Cylinder {
                radius: Value::Real(RB_CORNER_R),
                height: Value::Real(RB_HEIGHT),
            })
            .expect("corner cylinder should build")
            .id;
        let placed = translated(
            kernel,
            post,
            sx * (RB_WIDTH / 2.0 - RB_CORNER_R),
            sy * (RB_DEPTH / 2.0 - RB_CORNER_R),
            -RB_HEIGHT / 2.0,
        );
        acc = kernel
            .execute(&GeometryOp::Union {
                left: acc,
                right: placed,
            })
            .expect("corner union should succeed")
            .id;
    }
    acc
}

/// Edges whose bbox z-extents BOTH sit within `tol` of `z` — the exact
/// bbox-only predicate `reify_eval::topology_selectors::edges_at_height` uses,
/// reproduced here so the kernel-level test measures the same thing the
/// designer-facing selector does without depending on reify-eval.
fn edges_at_height(
    kernel: &mut OcctKernel,
    shape: GeometryHandleId,
    z: f64,
    tol: f64,
) -> Vec<GeometryHandleId> {
    let edges = kernel.extract_edges(shape).expect("extract_edges");
    edges
        .into_iter()
        .filter(|e| {
            let bb = common::bbox_of(kernel.query(&GeometryQuery::BoundingBox(*e)));
            (bb.zmin - z).abs() <= tol && (bb.zmax - z).abs() <= tol
        })
        .collect()
}

#[test]
fn rounded_box_fuse_chain_unifies_to_a_ten_face_prism() {
    let mut kernel = OcctKernel::new();
    let body = rounded_box_fuse_chain(&mut kernel);

    let faces = kernel.extract_faces(body).expect("extract_faces").len();
    let edges = kernel.extract_edges(body).expect("extract_edges").len();
    let volume = real_query(&kernel, GeometryQuery::Volume(body));

    // Top face sits at z = +height/2; 0.5 tolerance mirrors the designer idiom
    // `edges_at_height(body, 20mm, 0.5mm)` on the translated-up form.
    let rim = edges_at_height(&mut kernel, body, RB_HEIGHT / 2.0, 0.5);
    let rim_count = rim.len();

    // The fillet may or may not survive the fragmented rim — the architect
    // probe measured it SUCCEEDING with 66 faces on this fixture, and the
    // task records symptom 3's hard failure as fixture-dependent. So record
    // the outcome instead of asserting that today's behaviour is an error.
    let filleted = kernel
        .fillet_edges_with_history(body, 1.0, &rim)
        .map(|(h, _records)| h.id);
    let filleted_report = match filleted {
        Ok(id) => {
            let f = kernel.extract_faces(id).expect("extract_faces on fillet").len();
            let v = real_query(&kernel, GeometryQuery::Volume(id));
            format!("faces={f} volume={v:.6}")
        }
        Err(e) => format!("FAILED: {e:?}"),
    };

    // One combined report so a RED run shows every discriminator at once
    // rather than stopping at the first.
    let summary = format!(
        "measured: fuse-chain faces={faces} edges={edges} volume={volume:.6}; \
         rim edges selected={rim_count}; after 1mm rim fillet: {filleted_report}\n\
         expected GREEN: faces=10 edges=24 volume={RB_VOLUME:.6}; rim=8; \
         fillet faces=18 volume={RB_FILLETED_VOLUME:.6}\n\
         measured RED (OCCT 7.8, un-unified): faces=50 edges=88; rim=40; fillet faces=66"
    );

    assert_eq!(
        faces, 10,
        "the fuse chain must unify to the prism's 10 real faces \
         (6 planes + 4 corner cylinders); every extra face is a same-domain \
         seam fragment left by one of the five fuses. {summary}"
    );
    assert_eq!(
        edges, 24,
        "the fuse chain must unify to the prism's 24 real edges. {summary}"
    );
    assert!(
        (volume - RB_VOLUME).abs() <= 1e-3,
        "unification must not move material — the analytic volume is invariant \
         across the fix. {summary}"
    );
    assert_eq!(
        rim_count, 8,
        "the top rim has exactly 8 real edges (4 straight + 4 corner arcs); a \
         larger count is the bbox selector picking up phantom seam edges — THIS \
         is the designer-visible half of symptom 3. {summary}"
    );

    let filleted = kernel
        .fillet_edges_with_history(body, 1.0, &rim)
        .expect("1mm rim fillet over the unified selection must succeed")
        .0
        .id;
    let filleted_faces = kernel
        .extract_faces(filleted)
        .expect("extract_faces on fillet")
        .len();
    let filleted_volume = real_query(&kernel, GeometryQuery::Volume(filleted));
    assert_eq!(
        filleted_faces, 18,
        "10 unified faces + 8 rim-fillet faces = 18. {summary}"
    );
    assert!(
        (filleted_volume - RB_FILLETED_VOLUME).abs() <= 1e-3,
        "the filleted volume is IDENTICAL with and without unification — this \
         assertion guards against material loss, it is NOT the RED signal. {summary}"
    );
}
