//! Handle-stability property tests (kernel-seam ζ, INV-GEO-2): repeated
//! `extract_faces`/`extract_edges` calls on the SAME parent handle must
//! return element-for-element IDENTICAL ids, in the same order, within a
//! session, per real kernel. This is the regression guard that "would have
//! caught #4262" — see `docs/prds/kernel-seam-contracts.md` §5/§12.
//!
//! Scope: the three currently-STABLE arms only (§12 ζ) — OCCT
//! `extract_faces`, OCCT `extract_edges`, Manifold `extract_faces`. The
//! Manifold `extract_edges` arm is DELIBERATELY OMITTED here: it is
//! un-memoized on main (`ManifoldKernel::extract_edges`, in
//! `crates/reify-kernel-manifold/src/kernel.rs`, mints a fresh id every
//! call — there is no `extracted_edges` cache), so a stability assertion
//! would be permanently red and un-greenable within this task's scope.
//! That arm — and the memoization fix it depends on — is owned by the
//! downstream leaf η (§5/§8/§12).
//!
//! All three arms here are GREEN on current main: the memoization they pin
//! already landed (OCCT `extracted_faces`/`extracted_edges` caches, #318/
//! #4262; Manifold `extracted_faces` cache). Their value is red-on-revert —
//! delete the relevant cache and a repeated `extract_*` call mints fresh
//! ids, `first != second`, and the test fails: exactly the #4262 defect.

#![cfg(has_occt)]

mod common;

use reify_ir::{GeometryHandleId, GeometryKernel, QueryError};
use reify_kernel_manifold::ManifoldKernel;

/// Assert the handle-stability contract (INV-GEO-2) this file's arms pin:
/// the first run must be non-vacuous (non-empty, pairwise-distinct,
/// non-INVALID, and distinct from `parent` — mirrors the distinctness guard
/// in `topology_extract_integration.rs`'s
/// `extract_edges_box_returns_twelve_distinct_handles`), and every
/// subsequent run must be *exactly* equal to the first (same ids, same
/// order).
fn assert_stable_ids(label: &str, parent: GeometryHandleId, runs: &[Vec<GeometryHandleId>]) {
    let first = runs
        .first()
        .unwrap_or_else(|| panic!("[{label}] assert_stable_ids called with no runs"));

    assert!(
        !first.is_empty(),
        "[{label}] first run must be non-empty (a vacuous stability check would trivially pass)"
    );

    let mut seen = std::collections::HashSet::new();
    for id in first {
        assert_ne!(
            *id, parent,
            "[{label}] extracted handle must differ from the parent handle"
        );
        assert_ne!(
            *id,
            GeometryHandleId::INVALID,
            "[{label}] extracted handle must not be the INVALID sentinel"
        );
        assert!(
            seen.insert(*id),
            "[{label}] duplicate handle id {:?} in first run: {:?}",
            id,
            first
        );
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
/// The Manifold `extract_edges` arm is intentionally NOT added here — see
/// the module doc comment.
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
