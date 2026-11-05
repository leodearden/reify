//! Handle-stability property tests (kernel-seam ζ, INV-GEO-2): repeated
//! `extract_faces`/`extract_edges` calls on the SAME parent handle must
//! return element-for-element IDENTICAL ids, in the same order, within a
//! session, per real kernel. This is the regression guard that "would have
//! caught #4262" — see `docs/prds/kernel-seam-contracts.md` §5/§12.
//!
//! Scope: the four arms of the §12 ζ conformance matrix — OCCT
//! `extract_faces`, OCCT `extract_edges`, Manifold `extract_faces`, and
//! Manifold `extract_edges` — plus two OCCT arms outside that matrix,
//! described below. The Manifold `extract_edges` arm was
//! DELIBERATELY OMITTED when this file was first landed (task ζ):
//! `ManifoldKernel::extract_edges`, in
//! `crates/reify-kernel-manifold/src/kernel.rs`, minted a fresh id every
//! call — there was no `extracted_edges` cache — a dormant re-instance of
//! the #4262 defect that `extract_faces` already had fixed, so a stability
//! assertion would have been permanently red and un-greenable within that
//! task's scope. The downstream leaf η landed the mirrored
//! `extracted_edges` memoization cache and added this arm to complete the
//! matrix (§5/§8/§12). The two OCCT arms outside the matrix cover the
//! sub-shape-kind axis behind `OcctKernel`'s one extraction body:
//! `extract_vertices` stability, and edges/faces/vertices interleaved on one
//! parent (each stable, the kinds disjoint, every id carrying its own kind's
//! repr and the parent as owner).
//!
//! Every arm here is GREEN on current main: the memoization they pin already
//! landed (OCCT `extracted_faces`/`extracted_edges` caches, #318/#4262, and
//! its `extracted_vertices` cache; Manifold `extracted_faces`/
//! `extracted_edges` caches). Their value is red-on-revert — delete the
//! relevant cache and a repeated `extract_*` call mints fresh ids,
//! `first != second`, and the test fails: exactly the #4262 defect.
//!
//! Also out of scope: this file pins ONLY within-session stability. The
//! caches' clearing half of the memoization contract belongs to
//! `OcctKernel::reset`, which empties every extraction cache with the shape
//! table; it is covered in
//! `crates/reify-kernel-occt/tests/harness_occt_kernel/lifecycle.rs`.

#![cfg(has_occt)]

mod common;

use std::collections::HashSet;

use reify_ir::{BRepKind, GeometryHandleId, GeometryKernel, GeometryQuery, QueryError, Value};
use reify_kernel_manifold::ManifoldKernel;

/// Assert the handle-stability contract (INV-GEO-2) this file's arms pin:
/// EVERY run must be non-vacuous (non-empty, pairwise-distinct, non-INVALID,
/// and distinct from `parent` — mirrors the distinctness guard in
/// `topology_extract_integration.rs`'s
/// `extract_edges_box_returns_twelve_distinct_handles`), and every run after
/// the first must ALSO be *exactly* equal to the first (same ids, same
/// order). The non-vacuity checks are re-derived per run — rather than only
/// on `runs[0]`, trusting equality to carry them — so a later change that
/// weakens the cross-run equality assertion (e.g. to a length-only compare)
/// can't silently drop the distinctness/non-INVALID guarantees for runs 1..n.
fn assert_stable_ids(label: &str, parent: GeometryHandleId, runs: &[Vec<GeometryHandleId>]) {
    let first = runs
        .first()
        .unwrap_or_else(|| panic!("[{label}] assert_stable_ids called with no runs"));

    for (i, run) in runs.iter().enumerate() {
        assert!(
            !run.is_empty(),
            "[{label}] run {i} must be non-empty (a vacuous stability check would trivially pass)"
        );

        let mut seen = std::collections::HashSet::new();
        for id in run {
            assert_ne!(
                *id, parent,
                "[{label}] run {i}: extracted handle must differ from the parent handle"
            );
            assert_ne!(
                *id,
                GeometryHandleId::INVALID,
                "[{label}] run {i}: extracted handle must not be the INVALID sentinel"
            );
            assert!(
                seen.insert(*id),
                "[{label}] run {i}: duplicate handle id {:?} in run: {:?}",
                id,
                run
            );
        }
    }

    for (i, run) in runs.iter().enumerate().skip(1) {
        assert_eq!(
            run, first,
            "[{label}] run {i} must equal run 0 exactly (same ids, same order); \
             run {i}={run:?}, run 0={first:?}"
        );
    }
}

/// Invoke `extractor` three times in a row, `.expect`-ing each call with an
/// ordinal-labeled panic message, and return the three runs in order.
/// Shared by all three arms below to collapse the repeated "call the
/// extractor three times, then hand the runs to `assert_stable_ids`"
/// boilerplate.
fn run_thrice(
    label: &str,
    mut extractor: impl FnMut() -> Result<Vec<GeometryHandleId>, QueryError>,
) -> Vec<Vec<GeometryHandleId>> {
    ["first", "second", "third"]
        .into_iter()
        .map(|ordinal| {
            extractor()
                .unwrap_or_else(|e| panic!("[{label}] {ordinal} call should succeed: {e:?}"))
        })
        .collect()
}

/// `extract_faces` on a real OCCT BRep box SOLID handle returns identical
/// ids, in identical order, across repeated calls within a session.
///
/// GREEN on main (`OcctKernel::extract_faces`'s `extracted_faces` cache, in
/// `crates/reify-kernel-occt/src/lib.rs`); red-on-revert if that cache is
/// removed (a fresh id set would be minted per call, so the second call
/// would diverge from the first — the #4262 defect).
#[test]
fn occt_extract_faces_returns_stable_ids() {
    let (mut kernel, box_id) = common::occt_box_solid();

    let runs = run_thrice("occt extract_faces", || kernel.extract_faces(box_id));

    assert_eq!(
        runs[0].len(),
        6,
        "a 10x20x30 mm box has exactly 6 unique faces, got {}",
        runs[0].len()
    );

    assert_stable_ids("occt extract_faces", box_id, &runs);
}

/// `extract_edges` on a real OCCT BRep box SOLID handle returns identical
/// ids, in identical order, across repeated calls within a session.
///
/// GREEN on main (`OcctKernel::extract_edges`'s `extracted_edges` cache, in
/// `crates/reify-kernel-occt/src/lib.rs`); red-on-revert if that cache is
/// removed (a fresh id set would be minted per call, so the second call
/// would diverge from the first — the #4262 defect).
#[test]
fn occt_extract_edges_returns_stable_ids() {
    let (mut kernel, box_id) = common::occt_box_solid();

    let runs = run_thrice("occt extract_edges", || kernel.extract_edges(box_id));

    assert_eq!(
        runs[0].len(),
        12,
        "a 10x20x30 mm box has exactly 12 unique edges, got {}",
        runs[0].len()
    );

    assert_stable_ids("occt extract_edges", box_id, &runs);
}

/// `extract_vertices` on a real OCCT BRep box SOLID handle returns identical
/// ids, in identical order, across repeated calls within a session.
#[test]
fn occt_extract_vertices_returns_stable_ids() {
    let (mut kernel, box_id) = common::occt_box_solid();

    let runs = run_thrice("occt extract_vertices", || kernel.extract_vertices(box_id));

    assert_eq!(
        runs[0].len(),
        8,
        "a 10x20x30 mm box has exactly 8 unique vertices, got {}",
        runs[0].len()
    );

    assert_stable_ids("occt extract_vertices", box_id, &runs);
}

/// Interleaving the three OCCT sub-shape kinds on ONE parent keeps each
/// kind's ids stable, keeps the kinds' id sets disjoint, and stamps every id
/// with its own kind's repr and the parent's provenance. Each arm above uses
/// a fresh kernel and one kind, so a per-kind cache, repr or provenance
/// mix-up between kinds would hide from them.
#[test]
fn occt_interleaved_extract_kinds_stay_stable_and_disjoint() {
    let (mut kernel, box_id) = common::occt_box_solid();

    let (mut edge_runs, mut face_runs, mut vertex_runs) = (Vec::new(), Vec::new(), Vec::new());
    for round in 0..3 {
        let call = |r: Result<Vec<GeometryHandleId>, QueryError>, kind: &str| {
            r.unwrap_or_else(|e| panic!("round {round}: extract_{kind} should succeed: {e:?}"))
        };
        edge_runs.push(call(kernel.extract_edges(box_id), "edges"));
        face_runs.push(call(kernel.extract_faces(box_id), "faces"));
        vertex_runs.push(call(kernel.extract_vertices(box_id), "vertices"));
    }

    let kinds = [
        ("edges", BRepKind::Edge, 12, &edge_runs),
        ("faces", BRepKind::Face, 6, &face_runs),
        ("vertices", BRepKind::Vertex, 8, &vertex_runs),
    ];
    for (kind, repr, count, runs) in kinds {
        let label = format!("occt interleaved extract_{kind}");
        assert_eq!(runs[0].len(), count, "[{label}] count");
        assert_stable_ids(&label, box_id, runs);
        for &id in &runs[0] {
            assert_eq!(kernel.repr_of(id), Some(repr), "[{label}] repr of {id:?}");
            match kernel.query(&GeometryQuery::OwnerBody(id)) {
                Ok(Value::Int(owner)) => {
                    assert_eq!(owner, box_id.0 as i64, "[{label}] OwnerBody of {id:?}")
                }
                other => panic!("[{label}] OwnerBody of {id:?}: expected Int, got {other:?}"),
            }
        }
    }

    let sets: Vec<(&str, HashSet<GeometryHandleId>)> = kinds
        .iter()
        .map(|(kind, _, _, runs)| (*kind, runs[0].iter().copied().collect()))
        .collect();
    for (i, (a, a_ids)) in sets.iter().enumerate() {
        for (b, b_ids) in &sets[i + 1..] {
            let shared: Vec<_> = a_ids.intersection(b_ids).collect();
            assert!(shared.is_empty(), "{a} and {b} share ids {shared:?}");
        }
    }
}

/// `extract_faces` on a real Manifold handle — a genuine OCCT box mesh
/// ingested via `ManifoldKernel::ingest_mesh` — returns identical ids, in
/// identical order, across repeated calls within a session.
///
/// The exact coalesced-face COUNT is deliberately not pinned here — only a
/// loose lower bound (a box cannot coalesce below its 6 axis-aligned faces),
/// plus pairwise-distinct + 3-way exact identity: stability, not an exact
/// face count, is this arm's contract. GREEN on main
/// (`ManifoldKernel::extract_faces`'s `extracted_faces` cache, in
/// `crates/reify-kernel-manifold/src/kernel.rs`); red-on-revert if that
/// cache is removed.
///
/// The Manifold `extract_edges` arm follows immediately below
/// (`manifold_extract_edges_returns_stable_ids`) — see the module doc
/// comment for its landing history.
#[test]
fn manifold_extract_faces_returns_stable_ids() {
    let mesh = common::occt_box();
    // Precondition sanity check: `ManifoldKernel::extract_faces` silently
    // memoizes `Ok(Vec::new())` for an empty/degenerate ingest (kernel.rs's
    // `verts.is_empty() || tris.is_empty()` branch), which would otherwise
    // make the `>= 6` assertion below fail with the misleading "cannot
    // coalesce below its 6 axis-aligned faces" message instead of pointing
    // at the real cause. `occt_box()` always yields a real tessellated box,
    // so this is defensive-only.
    assert!(
        !mesh.vertices.is_empty() && !mesh.indices.is_empty(),
        "occt_box() must yield a non-empty mesh; got {} vertices / {} indices",
        mesh.vertices.len(),
        mesh.indices.len()
    );
    let mut manifold = ManifoldKernel::new();
    let handle = manifold
        .ingest_mesh(&mesh)
        .expect("ingest_mesh should accept a validated real OCCT box mesh");

    let runs = run_thrice("manifold extract_faces", || {
        manifold.extract_faces(handle.id)
    });

    assert!(
        runs[0].len() >= 6,
        "a box mesh cannot coalesce below its 6 axis-aligned faces \
         (a lower value suggests a degenerate single-face regression), got {}",
        runs[0].len()
    );

    assert_stable_ids("manifold extract_faces", handle.id, &runs);
}

/// `extract_edges` on a real Manifold handle — a genuine OCCT box mesh
/// ingested via `ManifoldKernel::ingest_mesh` — returns identical ids, in
/// identical order, across repeated calls within a session.
///
/// The exact edge COUNT is deliberately not pinned here — only a loose lower
/// bound (a box has 12 BRep edges, so it cannot have fewer than 6), mirroring
/// `manifold_extract_faces_returns_stable_ids`'s style above: stability, not
/// an exact count, is this arm's contract (the real-mesh canonical edge
/// enumeration is finer-grained than the BRep edge count and depends on
/// OCCT's tessellation, so hard-pinning an exact number would be brittle).
///
/// RED (task η step-2): `ManifoldKernel::extract_edges` is un-memoized on
/// current main — a dormant re-instance of the #4262 defect that
/// `extract_faces` already had fixed — so each of the three `run_thrice`
/// calls mints a fresh, disjoint id set and `assert_stable_ids` fails on the
/// run-1-vs-run-0 equality check. GREEN after task η step-3 (mirrored
/// `extracted_edges` memoization cache, in
/// `crates/reify-kernel-manifold/src/kernel.rs`); red-on-revert if that
/// cache is removed.
#[test]
fn manifold_extract_edges_returns_stable_ids() {
    let mesh = common::occt_box();
    // Precondition sanity check — see the analogous comment in
    // `manifold_extract_faces_returns_stable_ids` above.
    assert!(
        !mesh.vertices.is_empty() && !mesh.indices.is_empty(),
        "occt_box() must yield a non-empty mesh; got {} vertices / {} indices",
        mesh.vertices.len(),
        mesh.indices.len()
    );
    let mut manifold = ManifoldKernel::new();
    let handle = manifold
        .ingest_mesh(&mesh)
        .expect("ingest_mesh should accept a validated real OCCT box mesh");

    let runs = run_thrice("manifold extract_edges", || {
        manifold.extract_edges(handle.id)
    });

    assert!(
        runs[0].len() >= 6,
        "a box mesh cannot have fewer than 6 edges (a lower value suggests a \
         degenerate regression), got {}",
        runs[0].len()
    );

    assert_stable_ids("manifold extract_edges", handle.id, &runs);
}
