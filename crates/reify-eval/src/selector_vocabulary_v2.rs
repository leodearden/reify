//! v0.2 selector vocabulary v2 — combinators, direction/extremal/type
//! filters, history/attribute selectors, and topological walks
//! (task 2658, PRD `docs/prds/v0_2/persistent-naming-v2.md` task 10).
//!
//! This module is **additive** to `topology_selectors`: scalar/cone-window
//! selectors (`edges_by_length`, `faces_by_normal`, …) live there, while
//! the v2 vocabulary lifts patterns from CadQuery / build123d / OnShape
//! and extends with project-specific selectors (history-based, attribute
//! primitives, owner-body provenance).
//!
//! Boundaries:
//!   - **Pure-Rust combinators** (`intersect`, `union`, `complement`,
//!     `except`) operate over `&[GeometryHandleId]` and never touch the
//!     kernel.
//!   - **Filter selectors** (`faces_perpendicular_to`,
//!     `extremal_by_bbox`, `faces_by_surface_kind`, …) take
//!     `&mut K: GeometryKernel`, allocate sub-shape handles, and issue a
//!     single batched `query_many` per filter — same pattern as v0.1.
//!   - **History/attribute selectors** (`created_by_feature`,
//!     `split_by_feature`, `has_user_label`, `user_label_eq`) take a
//!     `&TopologyAttributeTable` and are pure-Rust.
//!   - **Topological walks** (`adjacent_to_face`, `ancestor_faces_of_edge`,
//!     `siblings_of_face`, `owner_body_of`) use new `GeometryQuery`
//!     variants backed by OCCT FFI.
//!
//! Order discipline: every combinator and filter preserves the input
//! encounter order with dedup-on-first-seen, mirroring
//! `topology_selectors::resolve_unique_by_tag`. This keeps selector
//! pipelines deterministic regardless of how downstream consumers
//! traverse the result.

use std::collections::HashSet;

use reify_types::GeometryHandleId;

// ─────────────────────────────────────────────────────────────────────────────
// Boolean combinators (PRD line 79)
//
// `and` / `or` / `not` / `except` over `Vec<GeometryHandleId>`. All four
// preserve left-operand encounter order with dedup-on-first-seen.
// ─────────────────────────────────────────────────────────────────────────────

/// Set intersection: returns the elements of `a` that also appear in `b`,
/// in `a`'s encounter order, with each element appearing at most once
/// (dedup on first occurrence).
///
/// Equivalent to PRD's `and(a, b)`. Asymmetric in **order** (left-operand
/// determines emission order) but symmetric in **membership**.
///
/// O(|a| + |b|): builds a `HashSet` of `b` once, then walks `a` with a
/// secondary `seen` set for LHS dedup.
pub fn intersect(a: &[GeometryHandleId], b: &[GeometryHandleId]) -> Vec<GeometryHandleId> {
    let rhs: HashSet<GeometryHandleId> = b.iter().copied().collect();
    let mut seen: HashSet<GeometryHandleId> = HashSet::with_capacity(a.len());
    let mut out: Vec<GeometryHandleId> = Vec::with_capacity(a.len().min(b.len()));
    for id in a {
        if rhs.contains(id) && seen.insert(*id) {
            out.push(*id);
        }
    }
    out
}

/// Set union: returns the elements of `a` (in `a`'s encounter order)
/// followed by the elements of `b` not already in `a` (in `b`'s encounter
/// order), with each element appearing at most once.
///
/// Equivalent to PRD's `or(a, b)`. Stable left-then-right discipline:
/// callers can rely on `a`'s prefix being preserved verbatim modulo
/// LHS-internal dedup.
///
/// O(|a| + |b|): single `HashSet` populated as we walk `a`, then `b`.
pub fn union(a: &[GeometryHandleId], b: &[GeometryHandleId]) -> Vec<GeometryHandleId> {
    let mut seen: HashSet<GeometryHandleId> = HashSet::with_capacity(a.len() + b.len());
    let mut out: Vec<GeometryHandleId> = Vec::with_capacity(a.len() + b.len());
    for id in a.iter().chain(b.iter()) {
        if seen.insert(*id) {
            out.push(*id);
        }
    }
    out
}
