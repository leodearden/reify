//! Handle-stability property tests (kernel-seam ζ, INV-GEO-2): repeated
//! `extract_faces`/`extract_edges` calls on the SAME parent handle must
//! return element-for-element IDENTICAL ids, in the same order, within a
//! session, per real kernel. This is the regression guard that "would have
//! caught #4262" — see `docs/prds/kernel-seam-contracts.md` §5/§12.
//!
//! Scope: the three currently-STABLE arms only (§12 ζ) — OCCT
//! `extract_faces`, OCCT `extract_edges`, Manifold `extract_faces`. The
//! Manifold `extract_edges` arm is DELIBERATELY OMITTED here: it is
//! un-memoized on main (`crates/reify-kernel-manifold/src/kernel.rs:880-901`
//! mints a fresh id every call), so a stability assertion would be
//! permanently red and un-greenable within this task's scope. That arm —
//! and the memoization fix it depends on — is owned by the downstream leaf
//! η (§5/§8/§12).
//!
//! All three arms here are GREEN on current main: the memoization they pin
//! already landed (OCCT `extracted_faces`/`extracted_edges` caches, #318/
//! #4262; Manifold `extracted_faces` cache). Their value is red-on-revert —
//! delete the relevant cache and a repeated `extract_*` call mints fresh
//! ids, `first != second`, and the test fails: exactly the #4262 defect.

#![cfg(has_occt)]

mod common;

use reify_ir::{GeometryHandleId, GeometryKernel};
use reify_kernel_manifold::ManifoldKernel;

/// Assert the handle-stability contract (INV-GEO-2) this file's arms pin:
/// the first run must be non-vacuous (non-empty, pairwise-distinct,
/// non-INVALID ids — mirrors the distinctness guard in
/// `topology_extract_integration.rs:44-60`), and every subsequent run must
/// be *exactly* equal to the first (same ids, same order).
fn assert_stable_ids(label: &str, runs: &[Vec<GeometryHandleId>]) {
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

/// `extract_faces` on a real OCCT BRep box SOLID handle returns identical
/// ids, in identical order, across repeated calls within a session.
///
/// GREEN on main (`extracted_faces` cache,
/// `crates/reify-kernel-occt/src/lib.rs:686-719`); red-on-revert if that
/// cache is removed (a fresh id set would be minted per call, so the second
/// call would diverge from the first — the #4262 defect).
#[test]
fn occt_extract_faces_returns_stable_ids() {
    let (mut kernel, box_id) = common::occt_box_solid();

    let first = kernel
        .extract_faces(box_id)
        .expect("first extract_faces call should succeed");
    let second = kernel
        .extract_faces(box_id)
        .expect("second extract_faces call should succeed");
    let third = kernel
        .extract_faces(box_id)
        .expect("third extract_faces call should succeed");

    assert_eq!(
        first.len(),
        6,
        "a 10x20x30 mm box has exactly 6 unique faces, got {}",
        first.len()
    );

    assert_stable_ids("occt extract_faces", &[first, second, third]);
}

/// `extract_edges` on a real OCCT BRep box SOLID handle returns identical
/// ids, in identical order, across repeated calls within a session.
///
/// GREEN on main (`extracted_edges` cache,
/// `crates/reify-kernel-occt/src/lib.rs:640-676`); red-on-revert if that
/// cache is removed (a fresh id set would be minted per call, so the second
/// call would diverge from the first — the #4262 defect).
#[test]
fn occt_extract_edges_returns_stable_ids() {
    let (mut kernel, box_id) = common::occt_box_solid();

    let first = kernel
        .extract_edges(box_id)
        .expect("first extract_edges call should succeed");
    let second = kernel
        .extract_edges(box_id)
        .expect("second extract_edges call should succeed");
    let third = kernel
        .extract_edges(box_id)
        .expect("third extract_edges call should succeed");

    assert_eq!(
        first.len(),
        12,
        "a 10x20x30 mm box has exactly 12 unique edges, got {}",
        first.len()
    );

    assert_stable_ids("occt extract_edges", &[first, second, third]);
}

/// `extract_faces` on a real Manifold handle — a genuine OCCT box mesh
/// ingested via `ManifoldKernel::ingest_mesh` — returns identical ids, in
/// identical order, across repeated calls within a session.
///
/// The coalesced-face COUNT is deliberately not pinned here (only
/// non-empty + pairwise-distinct + 3-way exact identity): stability, not
/// face count, is this arm's contract. GREEN on main (Manifold
/// `extracted_faces` cache,
/// `crates/reify-kernel-manifold/src/kernel.rs:835-867`); red-on-revert if
/// that cache is removed.
///
/// The Manifold `extract_edges` arm is intentionally NOT added here — see
/// the module doc comment.
#[test]
fn manifold_extract_faces_returns_stable_ids() {
    let mesh = common::occt_box();
    let mut manifold = ManifoldKernel::new();
    let handle = manifold
        .ingest_mesh(&mesh)
        .expect("ingest_mesh should accept a validated real OCCT box mesh");

    let first = manifold
        .extract_faces(handle.id)
        .expect("first extract_faces call should succeed");
    let second = manifold
        .extract_faces(handle.id)
        .expect("second extract_faces call should succeed");
    let third = manifold
        .extract_faces(handle.id)
        .expect("third extract_faces call should succeed");

    assert_stable_ids("manifold extract_faces", &[first, second, third]);
}
