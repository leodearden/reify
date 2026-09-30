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

// ---------------------------------------------------------------------------
// Repr coherence for the *_with_history variants.
//
// `extract_boolean_history` is the single shared body behind
// `boolean_fuse_with_history`, `boolean_cut_with_history` and
// `boolean_common_with_history`. It stores `op.Shape()` RAW and then builds
// `face_map()` / `edge_map()` from it, so the with-history path hands back a
// fragmented COMPOUND while the plain path returns a clean unified SOLID — an
// incoherent kernel contract, and the reason the eval realization path (which
// routes every boolean through `execute_with_history`) still sees the
// un-normalized topology.
//
// Normalizing there is not enough on its own: unification RE-IDENTIFIES faces.
// When it merges two coplanar faces the survivor is a new TShape that
// `BRepAlgoAPI::Modified()` never reported, so a naive "normalize, then look up
// the old children" would miss every `result_map.FindIndex` and blow
// `silent_drop_count` past the `== 0` that `boolean_op_history_integration.rs`
// and `topology_diagnostic_denoise_e2e.rs` both depend on. The boolean history
// must be COMPOSED with `ShapeUpgrade_UnifySameDomain::History()`.
// ---------------------------------------------------------------------------

/// Two abutting 10×10×10 cubes — the same fixture
/// `topology_selectors_integration.rs` uses, so the face count is
/// independently corroborated there.
fn two_abutting_cubes(kernel: &mut OcctKernel) -> (GeometryHandleId, GeometryHandleId) {
    let a = cube(kernel, 10.0);
    let b_raw = cube(kernel, 10.0);
    let b = translated(kernel, b_raw, 10.0, 0.0, 0.0);
    (a, b)
}

/// Collect the result-side indices each parent maps onto, as
/// `(parent_index, result_subshape_index)` pairs drawn from BOTH the Modified
/// and Generated records.
fn result_indices(records: &[reify_ir::HistoryRecord]) -> Vec<(u8, u32)> {
    records
        .iter()
        .map(|r| (r.parent_index, r.result_subshape_index))
        .collect()
}

/// Shared well-formedness checks for a normalized boolean history: no silent
/// drops, and every result index inside the LIVE face/edge maps of the shape
/// actually stored on the handle. Bounds are computed from the result rather
/// than hard-coded — this is what catches a history captured against the
/// PRE-unification numbering.
fn assert_history_indices_are_in_the_stored_result(
    kernel: &mut OcctKernel,
    result: GeometryHandleId,
    records: &reify_ir::BooleanOpHistoryRecords,
    what: &str,
) -> (usize, usize) {
    let n_faces = kernel.extract_faces(result).expect("extract_faces").len();
    let n_edges = kernel.extract_edges(result).expect("extract_edges").len();

    for (label, recs, bound) in [
        ("face_modified", &records.face_modified, n_faces),
        ("face_generated", &records.face_generated, n_faces),
        ("edge_modified", &records.edge_modified, n_edges),
        ("edge_generated", &records.edge_generated, n_edges),
    ] {
        for (parent, idx) in result_indices(recs) {
            assert!(
                (idx as usize) < bound,
                "{what}: {label} record (parent {parent}) points at result index \
                 {idx}, outside the stored result's {bound} sub-shapes — the \
                 history was captured against the pre-unification numbering"
            );
        }
    }
    (n_faces, n_edges)
}

#[test]
fn fuse_with_history_on_abutting_boxes_records_indices_into_the_unified_result() {
    let mut kernel = OcctKernel::new();
    let (a, b) = two_abutting_cubes(&mut kernel);
    let (handle, records) = kernel
        .boolean_fuse_with_history(a, b)
        .expect("fuse with history should succeed");
    let result = handle.id;

    // The stored shape must be the unified SOLID, not the raw COMPOUND.
    // `IsWatertight` is the discriminator here, NOT `repr_of`: the Rust
    // with-history arms stamp a hardcoded `BRepKind::Solid` today, so the repr
    // would agree even while the shape underneath is a COMPOUND.
    assert!(
        bool_query(&kernel, GeometryQuery::IsWatertight(result)),
        "the with-history result must be the normalized SOLID; a raw COMPOUND \
         fails the SOLID|COMPSOLID|SHELL guard"
    );
    assert_eq!(
        kernel.repr_of(result),
        Some(BRepKind::Solid),
        "the with-history arms must stamp the repr from the real shape, like \
         the plain arms already do"
    );

    assert_eq!(
        records.silent_drop_count, 0,
        "silent_drop_count must stay 0 — composing the unify history is what \
         keeps it there. A naive normalize-then-lookup would miss every merged \
         face, whose survivor is a NEW TShape that BRepAlgoAPI::Modified() \
         never reported, and blow the count that \
         boolean_op_history_integration.rs and topology_diagnostic_denoise_e2e.rs \
         both depend on"
    );

    let (n_faces, n_edges) =
        assert_history_indices_are_in_the_stored_result(&mut kernel, result, &records, "fuse");

    assert_eq!(
        n_faces, 6,
        "two abutting 10mm cubes fuse into a 20x10x10 prism: 6 faces \
         (RED today: a COMPOUND with 10, the count topology_selectors_integration.rs \
         independently pinned before this fix)"
    );
    assert_eq!(n_edges, 12, "...and 12 edges");

    // Each of the four coplanar face PAIRS that unification merges (top,
    // bottom, front, back — each split either side of the X=10 seam) must
    // still resolve from BOTH parents, onto the SAME surviving result index.
    // Many-to-one is legitimate; a MISSING record is not.
    let from_left: std::collections::HashSet<u32> = records
        .face_modified
        .iter()
        .filter(|r| r.parent_index == 0)
        .map(|r| r.result_subshape_index)
        .collect();
    let from_right: std::collections::HashSet<u32> = records
        .face_modified
        .iter()
        .filter(|r| r.parent_index == 1)
        .map(|r| r.result_subshape_index)
        .collect();
    let shared: Vec<u32> = {
        let mut v: Vec<u32> = from_left.intersection(&from_right).copied().collect();
        v.sort_unstable();
        v
    };
    assert_eq!(
        shared.len(),
        4,
        "exactly 4 result faces must be reported as Modified by BOTH parents — \
         the top/bottom/front/back pairs unification merged across the X=10 \
         seam. Got {shared:?} (left→{from_left:?}, right→{from_right:?}). \
         A pair collapsing to one record means the merged parent lost its \
         provenance"
    );

    // Every face of the result must be reachable from some record — EXCEPT the
    // two end caps at x = -5 and x = +15, which the fuse never touches and for
    // which BRepAlgoAPI reports neither Modified nor Generated (identity
    // passthrough: the result face IS the parent face). That is pre-existing
    // BRepAlgoAPI behaviour, unrelated to normalization, so rather than
    // exempting a hard-coded count this checks the GEOMETRY of whatever is
    // uncovered: anything without a record must be a planar cap at an X
    // extreme of the body. A merged survivor losing its record — the
    // correspondence loss this composition exists to prevent — would surface
    // here as an uncovered face in the middle of the prism.
    let covered: std::collections::HashSet<u32> = records
        .face_modified
        .iter()
        .chain(records.face_generated.iter())
        .map(|r| r.result_subshape_index)
        .collect();
    let body = common::bbox_of(kernel.query(&GeometryQuery::BoundingBox(result)));
    let faces = kernel.extract_faces(result).expect("extract_faces");
    let mut uncovered_caps = 0;
    for (i, face) in faces.iter().enumerate() {
        if covered.contains(&(i as u32)) {
            continue;
        }
        let bb = common::bbox_of(kernel.query(&GeometryQuery::BoundingBox(*face)));
        // 1e-6 absolute: `BRepBndLib::Add` inflates a bbox by OCCT's gap
        // (measured ~1e-7 here, e.g. xmin -5.0000001 / xmax -4.9999999 for the
        // x = -5 cap). Still four orders of magnitude tighter than the 20-unit
        // x-extent of any mid-prism face, so the discriminator is intact.
        const BBOX_TOL: f64 = 1e-6;
        let is_x_extreme_cap = (bb.xmax - bb.xmin).abs() < BBOX_TOL
            && ((bb.xmin - body.xmin).abs() < BBOX_TOL
                || (bb.xmax - body.xmax).abs() < BBOX_TOL);
        assert!(
            is_x_extreme_cap,
            "result face {i} has no Modified/Generated record and is NOT one of \
             the two untouched end caps (face bbox {bb:?}, body bbox {body:?}) — \
             a merged survivor lost its provenance"
        );
        uncovered_caps += 1;
    }
    assert_eq!(
        uncovered_caps, 2,
        "exactly the two end caps (x = body xmin and x = body xmax) are \
         identity passthroughs with no record; got {uncovered_caps}"
    );
}

#[test]
fn cut_with_history_records_indices_into_the_unified_result() {
    let mut kernel = OcctKernel::new();
    // Two 10mm cubes with a +5mm X offset — the SAME fixture
    // `boolean_op_history_integration.rs` uses for its cut test, and the
    // fixture on which its `silent_drop_count == 0` guarantee is established.
    let block = cube(&mut kernel, 10.0);
    let tool_raw = cube(&mut kernel, 10.0);
    let tool = translated(&mut kernel, tool_raw, 5.0, 0.0, 0.0);
    let (handle, records) = kernel
        .boolean_cut_with_history(block, tool)
        .expect("cut with history should succeed");
    let result = handle.id;

    assert!(
        bool_query(&kernel, GeometryQuery::IsWatertight(result)),
        "the with-history cut result must be the normalized SOLID"
    );
    assert_eq!(
        kernel.repr_of(result),
        Some(BRepKind::Solid),
        "the with-history cut arm must stamp the repr from the real shape"
    );
    assert_eq!(
        records.silent_drop_count, 0,
        "silent_drop_count must stay 0 — composing the unify history is what \
         keeps it there. A naive normalize-then-lookup would miss every merged \
         face, whose survivor is a NEW TShape that BRepAlgoAPI::Modified() \
         never reported, and blow the count that \
         boolean_op_history_integration.rs and topology_diagnostic_denoise_e2e.rs \
         both depend on"
    );

    // Counts are deliberately NOT hard-coded here — the assertion is that
    // every recorded index lands inside the LIVE maps of the shape actually
    // stored, which is what a stale pre-unification numbering violates.
    assert_history_indices_are_in_the_stored_result(&mut kernel, result, &records, "cut");
}

/// A corner-notch cut, kept as a SEPARATE case because this geometry carries a
/// pre-existing `BRepAlgoAPI_Cut` correspondence loss that has nothing to do
/// with normalization.
///
/// MEASURED, both before and after task 7054's change to
/// `extract_boolean_history` (the latter by temporarily forcing the raw
/// `op.Shape()` back in): `silent_drop_count == 24` on this fixture either way,
/// split 12 `TopAbs_EDGE` + 12 `TopAbs_VERTEX` children that
/// `BRepAlgoAPI_Cut::Modified()` reports for parent EDGES but that never appear
/// in the result's edge map. Composing the unification history neither adds nor
/// removes a single one — the counts are identical.
///
/// So this test asserts what IS attributable to normalization (the stored shape
/// is the unified solid, and every recorded index lands inside its live maps)
/// and deliberately does NOT assert `silent_drop_count == 0`, which was never
/// true for this geometry. Filed separately as a follow-up.
#[test]
fn cut_with_history_on_notched_box_indexes_into_the_unified_result() {
    let mut kernel = OcctKernel::new();
    let block = cube(&mut kernel, 10.0); // [-5, +5]^3
    let notch_raw = cube(&mut kernel, 4.0);
    let notch = translated(&mut kernel, notch_raw, 5.0, 5.0, 5.0); // corner bite
    let (handle, records) = kernel
        .boolean_cut_with_history(block, notch)
        .expect("cut with history should succeed");
    let result = handle.id;

    assert!(
        bool_query(&kernel, GeometryQuery::IsWatertight(result)),
        "the with-history cut result must be the normalized SOLID"
    );
    assert_history_indices_are_in_the_stored_result(&mut kernel, result, &records, "corner notch");
}

#[test]
fn common_with_history_records_indices_into_the_unified_result() {
    let mut kernel = OcctKernel::new();
    // Two 10mm cubes with a +5mm X offset: the intersection is the single
    // 5x10x10 slab [0,5]x[-5,5]x[-5,5]. Mirrors `cut_with_history_...` above so
    // the third `*_with_history` arm — the one `extract_boolean_history` shares
    // with the other two — is exercised rather than assumed.
    let a = cube(&mut kernel, 10.0);
    let b_raw = cube(&mut kernel, 10.0);
    let b = translated(&mut kernel, b_raw, 5.0, 0.0, 0.0);
    let (handle, records) = kernel
        .boolean_common_with_history(a, b)
        .expect("common with history should succeed");
    let result = handle.id;

    assert!(
        bool_query(&kernel, GeometryQuery::IsWatertight(result)),
        "the with-history common result must be the normalized SOLID"
    );
    assert_eq!(
        kernel.repr_of(result),
        Some(BRepKind::Solid),
        "the with-history common arm must stamp the repr from the real shape, \
         not the hardcoded BRepKind::Solid it used before task 7054 (which \
         happened to agree here, and would NOT for a disjoint result)"
    );
    assert_eq!(
        records.silent_drop_count, 0,
        "silent_drop_count must stay 0 on the common arm too — the unify \
         history is composed in `extract_boolean_history`, which all three \
         *_with_history variants share"
    );

    assert_history_indices_are_in_the_stored_result(&mut kernel, result, &records, "common");
}

/// The `Compound` branch of the CHANGED with-history arms. `binary_fuse_of_
/// disjoint_boxes_is_a_watertight_compsolid` pins it on the plain path only;
/// the with-history arms took their repr from a separate hardcoded
/// `BRepKind::Solid` until task 7054, so the branch needs its own coverage here.
#[test]
fn disjoint_fuse_with_history_is_a_watertight_compsolid() {
    let mut kernel = OcctKernel::new();
    let a = cube(&mut kernel, 10.0); // [-5, +5]
    let b_raw = cube(&mut kernel, 10.0);
    let b = translated(&mut kernel, b_raw, 40.0, 0.0, 0.0); // [35, 45] — disjoint
    let (handle, records) = kernel
        .boolean_fuse_with_history(a, b)
        .expect("disjoint fuse with history should succeed");
    let result = handle.id;

    assert_eq!(
        kernel.repr_of(result),
        Some(BRepKind::Compound),
        "a disjoint with-history fuse must classify from the real shape like \
         the plain arm does; the pre-7054 hardcoded BRepKind::Solid was a lie \
         here, and `run_local_feature_with_history` REJECTS on this value"
    );
    assert!(
        bool_query(&kernel, GeometryQuery::IsWatertight(result)),
        "a disjoint with-history fuse must be rewrapped as a COMPSOLID, which \
         passes the SOLID|COMPSOLID|SHELL guard"
    );
    assert_history_indices_are_in_the_stored_result(&mut kernel, result, &records, "disjoint fuse");
}

/// The reach of the `BRepKind::Solid` guard in `run_local_feature_with_history`,
/// pinned deliberately rather than left as an incidental consequence.
///
/// Task 7054 made the binary boolean arms stamp the TRUE repr, so a disjoint
/// `union` is now `BRepKind::Compound` and the curated-fillet path — the exact
/// `fillet(body, edges, r)` designer idiom — rejects it up front instead of
/// handing a multi-body aggregate to `BRepFilletAPI_MakeFillet`. The n-ary
/// `fuse_all` path has behaved this way since task 5213; this is the binary
/// path catching up, not a new class of failure.
#[test]
fn curated_fillet_over_a_disjoint_fuse_is_rejected_as_non_solid() {
    let mut kernel = OcctKernel::new();
    let a = cube(&mut kernel, 10.0);
    let b_raw = cube(&mut kernel, 10.0);
    let b = translated(&mut kernel, b_raw, 40.0, 0.0, 0.0); // disjoint
    let fused = kernel
        .execute(&GeometryOp::Union { left: a, right: b })
        .expect("disjoint union should succeed")
        .id;
    let edges = kernel.extract_edges(fused).expect("extract_edges");
    assert!(
        !edges.is_empty(),
        "the disjoint fuse must still expose its edges; the rejection below has \
         to be about the SHAPE KIND, not an empty selection"
    );

    let err = kernel
        .fillet_edges_with_history(fused, 0.5, &edges[..1])
        .expect_err("a curated fillet over a multi-body aggregate must be rejected");
    let msg = err.to_string();
    assert!(
        msg.contains("requires a BRepKind::Solid input shape") && msg.contains("Compound"),
        "the rejection must name the Solid-input requirement and the actual \
         kind, so a designer can act on it; got: {msg}"
    );

    // ...and the single-body case must still be accepted, so the guard is
    // discriminating on the repr rather than rejecting every boolean result.
    let c_raw = cube(&mut kernel, 10.0);
    let c = translated(&mut kernel, c_raw, 5.0, 0.0, 0.0); // overlapping
    let merged = kernel
        .execute(&GeometryOp::Union { left: a, right: c })
        .expect("overlapping union should succeed")
        .id;
    let merged_edges = kernel.extract_edges(merged).expect("extract_edges");
    kernel
        .fillet_edges_with_history(merged, 0.5, &merged_edges[..1])
        .expect("a curated fillet over a single-body fuse must still succeed");
}

// ---------------------------------------------------------------------------
// Losslessness and cost of the normalization itself.
// ---------------------------------------------------------------------------

/// The `no solids → leave the compound untouched` branch of
/// `unwrap_boolean_compound`, reachable through every boolean whose result is
/// EMPTY: a `Common` of solids that do not overlap in a volume, and a `Cut`
/// whose tool fully contains its argument.
///
/// This is the one branch where `brep_kind_of_shape` returns `Compound` where
/// the pre-7054 code stamped a hardcoded `Solid` — and therefore exactly what
/// `run_local_feature_with_history` now rejects (see
/// `curated_fillet_over_a_disjoint_fuse_is_rejected_as_non_solid`). The empty
/// compound must survive verbatim: there is nothing to tighten, and there is
/// certainly nothing to unify.
///
/// MEASURED here, and the reason the LOSSLESSNESS PRECONDITION added to
/// `unwrap_boolean_compound` is defense-in-depth rather than a live path: OCCT's
/// solid-solid BOPs never hand back a MIXED compound. A `Common` of face- or
/// edge-touching cubes yields an EMPTY compound (0 faces, 0 edges — not the free
/// contact face), and the kernel refuses a boolean whose operand is a free face
/// outright (`BRepAlgoAPI_Fuse failed`). So no solid-solid boolean in this
/// kernel can currently reach the mixed case the gate exists to protect; the
/// gate is what keeps that true if a non-solid operand ever becomes reachable.
#[test]
fn empty_boolean_results_stay_untouched_compounds() {
    let mut kernel = OcctKernel::new();
    let a = cube(&mut kernel, 10.0); // [-5, +5]^3
    let touching_raw = cube(&mut kernel, 10.0);
    let touching = translated(&mut kernel, touching_raw, 10.0, 0.0, 0.0); // [5, 15] — face contact
    let far_raw = cube(&mut kernel, 10.0);
    let far = translated(&mut kernel, far_raw, 40.0, 0.0, 0.0); // [35, 45] — no contact
    let engulfing = cube(&mut kernel, 40.0); // [-20, +20]^3 — strictly contains `a`

    for (what, op) in [
        (
            "common of face-touching cubes",
            GeometryOp::Intersection {
                left: a,
                right: touching,
            },
        ),
        (
            "common of disjoint cubes",
            GeometryOp::Intersection {
                left: a,
                right: far,
            },
        ),
        (
            "cut by a fully engulfing tool",
            GeometryOp::Difference {
                left: a,
                right: engulfing,
            },
        ),
    ] {
        let empty = kernel
            .execute(&op)
            .unwrap_or_else(|e| panic!("{what} must succeed with an empty result, got {e}"))
            .id;
        assert_eq!(
            kernel.repr_of(empty),
            Some(BRepKind::Compound),
            "{what}: an empty boolean result has no solid to tighten to, so it \
             must stay a COMPOUND — and be CLASSIFIED as one. The pre-7054 arms \
             stamped a hardcoded BRepKind::Solid here, which is the lie that \
             let a void body reach `BRepFilletAPI_MakeFillet`"
        );
        assert!(
            !bool_query(&kernel, GeometryQuery::IsWatertight(empty)),
            "{what}: an empty compound is not a closed body and must not claim to be"
        );
        assert_eq!(
            real_query(&kernel, GeometryQuery::Volume(empty)),
            0.0,
            "{what}: an empty boolean result encloses no volume"
        );
        assert!(
            kernel
                .extract_faces(empty)
                .expect("extract_faces")
                .is_empty(),
            "{what}: an empty boolean result has no faces"
        );
    }
}

/// Standing guard for the pairwise-disjoint short-circuit in
/// `normalize_boolean_result` (see the measured numbers at that call site: the
/// unification pass roughly DOUBLED disjoint pattern realization for zero
/// topological benefit).
///
/// Asserts the property that makes the short-circuit correctness-neutral —
/// disjoint bodies have nothing to merge, so skipping unification must leave the
/// face count exactly as the sum of the parts — alongside the ABUTTING control,
/// which does take the unification path and must still merge. A wall-clock
/// assertion would be flaky on a shared host; this pins the discriminator the
/// existing `boolean_pass_count()` guard is structurally blind to.
#[test]
fn disjoint_fuse_merges_nothing_and_the_abutting_control_still_merges() {
    let mut kernel = OcctKernel::new();
    let a = cube(&mut kernel, 10.0); // [-5, +5]
    let single_faces = kernel.extract_faces(a).expect("extract_faces").len();
    assert_eq!(single_faces, 6, "a box has 6 faces");

    let b_raw = cube(&mut kernel, 10.0);
    let disjoint = translated(&mut kernel, b_raw, 40.0, 0.0, 0.0); // [35, 45]
    let disjoint_fused = kernel
        .execute(&GeometryOp::Union {
            left: a,
            right: disjoint,
        })
        .expect("disjoint union should succeed")
        .id;
    assert_eq!(
        kernel
            .extract_faces(disjoint_fused)
            .expect("extract_faces")
            .len(),
        2 * single_faces,
        "two bbox-disjoint bodies cannot share a surface, so the fuse must keep \
         every face of both — this is what makes skipping the unification pass \
         correctness-neutral (measured identical face counts, 600 at N=100 and \
         6000 at N=1000, with and without the pass)"
    );

    let c_raw = cube(&mut kernel, 10.0);
    let abutting = translated(&mut kernel, c_raw, 10.0, 0.0, 0.0); // shares the x=5 face
    let abutting_fused = kernel
        .execute(&GeometryOp::Union {
            left: a,
            right: abutting,
        })
        .expect("abutting union should succeed")
        .id;
    assert_eq!(
        kernel
            .extract_faces(abutting_fused)
            .expect("extract_faces")
            .len(),
        6,
        "the abutting control must NOT take the short-circuit: two touching \
         cubes unify into a genuine 20x10x10 prism with 6 faces"
    );
}

// ---------------------------------------------------------------------------
// The short-circuit must be predicated on the OPERANDS, not the result.
//
// The guard above is structurally blind to the case that matters: with exactly
// two operands, "the operands could not merge" and "the result's solids are
// pairwise disjoint" happen to coincide. They come apart as soon as a boolean
// merges SOME of its operands into a cluster while other bodies stay far away.
// Then the result's top-level solids ARE pairwise disjoint — one merged prism,
// one lone cube — and yet the prism carries brand-new coplanar seams, created
// by the very boolean being normalized. A RESULT-side disjointness test skips
// unification on exactly the seams this module exists to remove.
//
// The sound condition is a property of the INPUTS: if no two operand solids can
// touch, nothing merged, and the result is a re-wrap of shapes this boolean did
// not alter.
//
// All three cases are anchored on FACE COUNT, never volume or mass: a surviving
// coplanar seam re-describes the boundary without moving it, so the volume is
// bit-identical in both arms and a mass assertion would be a guaranteed false
// green (the same measured constraint recorded for symptom 3 at the top of this
// module).
// ---------------------------------------------------------------------------

/// The n-ary realizer path: one `fuse_all` over a cluster plus a far body.
///
/// MEASURED: 16 faces with the result-side predicate (the merged prism keeps
/// its 4 phantom seam faces), 12 with the operand-side one.
#[test]
fn n_ary_fuse_of_a_cluster_plus_a_far_body_unifies_the_cluster() {
    let mut kernel = OcctKernel::new();
    let a = cube(&mut kernel, 10.0); // [-5, +5]
    let b_raw = cube(&mut kernel, 10.0);
    let abutting = translated(&mut kernel, b_raw, 10.0, 0.0, 0.0); // [5, 15] — merges with `a`
    let c_raw = cube(&mut kernel, 10.0);
    let far = translated(&mut kernel, c_raw, 40.0, 0.0, 0.0); // [35, 45] — merges with nothing

    let fused = kernel
        .fuse_all(&[a, abutting, far])
        .expect("n-ary fuse of a cluster plus a far body should succeed")
        .id;

    assert_eq!(
        kernel.extract_faces(fused).expect("extract_faces").len(),
        12,
        "the two abutting cubes must unify into a genuine 20x10x10 prism (6 \
         faces) beside the untouched far cube (6) — the far body's presence \
         must not buy the cluster an exemption from unification. A result-side \
         disjointness test sees two non-touching solids here and skips the \
         pass, leaving the cluster's 4 phantom seam faces (16 total)"
    );
}

/// The binary-op path to the same shape: `GeometryOp::Union` gates nothing on
/// operand repr, so the inner union stores a two-solid COMPSOLID and the outer
/// one merges `abutting` into `a` while `far` stays separate.
///
/// MEASURED: 16 faces with the result-side predicate, 12 with the operand-side
/// one.
#[test]
fn nested_binary_fuse_of_a_cluster_plus_a_far_body_unifies_the_cluster() {
    let mut kernel = OcctKernel::new();
    let a = cube(&mut kernel, 10.0); // [-5, +5]
    let b_raw = cube(&mut kernel, 10.0);
    let far = translated(&mut kernel, b_raw, 40.0, 0.0, 0.0); // [35, 45]
    let c_raw = cube(&mut kernel, 10.0);
    let abutting = translated(&mut kernel, c_raw, 10.0, 0.0, 0.0); // [5, 15]

    let inner = kernel
        .execute(&GeometryOp::Union {
            left: a,
            right: far,
        })
        .expect("inner union of two disjoint cubes should succeed")
        .id;
    assert_eq!(
        kernel.extract_faces(inner).expect("extract_faces").len(),
        12,
        "the genuinely-disjoint inner fuse must continue to merge nothing: two \
         bbox-disjoint cubes keep all 12 faces, which is what makes skipping \
         unification on disjoint OPERANDS correctness-neutral"
    );

    let outer = kernel
        .execute(&GeometryOp::Union {
            left: inner,
            right: abutting,
        })
        .expect("outer union against the abutting cube should succeed")
        .id;
    assert_eq!(
        kernel.extract_faces(outer).expect("extract_faces").len(),
        12,
        "the outer fuse merges `abutting` into `a` and leaves `far` alone, so \
         its result is a 20x10x10 prism (6 faces) plus the far cube (6). Its \
         two result solids are pairwise disjoint, so a result-side predicate \
         skips the pass and the freshly-created seam survives (16 faces)"
    );
}

/// The grid-pattern realizer reduced to its minimum: rows that abut internally
/// but are mutually disjoint. The result is a COMPSOLID of two row-prisms that
/// ARE pairwise bbox-disjoint, so the result-side predicate fires on a shape
/// whose every seam was introduced by this very fuse.
///
/// MEASURED: 20 faces with the result-side predicate, 12 with the operand-side
/// one.
#[test]
fn grid_pattern_fuse_unifies_within_rows_when_rows_are_disjoint() {
    let mut kernel = OcctKernel::new();
    let mut instances = Vec::new();
    for row in 0..2 {
        for col in 0..2 {
            let raw = cube(&mut kernel, 10.0);
            // x-pitch 10 abuts within a row; y-pitch 40 keeps the rows apart.
            instances.push(translated(
                &mut kernel,
                raw,
                f64::from(col) * 10.0,
                f64::from(row) * 40.0,
                0.0,
            ));
        }
    }

    let fused = kernel
        .fuse_all(&instances)
        .expect("2x2 grid fuse should succeed")
        .id;

    assert_eq!(
        kernel.extract_faces(fused).expect("extract_faces").len(),
        12,
        "each row must unify into one 20x10x10 prism (6 faces), giving 12 for \
         the two rows. The two row-prisms are pairwise bbox-disjoint, so a \
         result-side predicate skips unification and every row keeps its 4 \
         phantom seam faces (20 total)"
    );
}
