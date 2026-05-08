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

use reify_types::{GeometryHandleId, GeometryKernel, GeometryQuery, QueryError};

use crate::topology_selectors::{dot3, filter_by_value, normalize3, parse_xyz_value};

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

/// Set difference: returns the elements of `universe` that do **not** appear
/// in `exclude`, preserving `universe`'s encounter order with
/// dedup-on-first-seen.
///
/// Equivalent to PRD's `not(universe, exclude)`. Walks `universe` once with
/// a `HashSet` of `exclude` for membership testing — O(|universe| + |exclude|).
pub fn complement(
    universe: &[GeometryHandleId],
    exclude: &[GeometryHandleId],
) -> Vec<GeometryHandleId> {
    let rhs: HashSet<GeometryHandleId> = exclude.iter().copied().collect();
    let mut seen: HashSet<GeometryHandleId> = HashSet::with_capacity(universe.len());
    let mut out: Vec<GeometryHandleId> = Vec::with_capacity(universe.len());
    for id in universe {
        if !rhs.contains(id) && seen.insert(*id) {
            out.push(*id);
        }
    }
    out
}

/// `except(a, b)` — alias for `complement(a, b)` from the LHS perspective.
///
/// Currently identical to `complement` but kept as a separately-named
/// public symbol because the PRD vocabulary (line 79) names `not` and
/// `except` distinctly: a future change might let `except` carry
/// LHS-anchored semantics that differ from set difference (e.g. retaining
/// LHS multiplicity if the API ever moves off dedup-on-first-seen).
pub fn except(a: &[GeometryHandleId], b: &[GeometryHandleId]) -> Vec<GeometryHandleId> {
    complement(a, b)
}

// ─────────────────────────────────────────────────────────────────────────────
// Direction filters (PRD line 76)
//
// `+X` / `-X` (signed) are covered by v0.1 `faces_by_normal` already.
// `|axis` (parallel-to-axis, sign-tolerant) is covered by v0.1
// `edges_parallel_to`. The new variants below cover `#axis`
// (perpendicular-to-axis) for both faces and edges.
// ─────────────────────────────────────────────────────────────────────────────

/// Return the subset of `extract_faces(handle)` whose surface normal is
/// **perpendicular** to `axis` within `angular_tol_rad`.
///
/// A face is retained iff its (unit) normal `n` satisfies
/// `|n · axis| <= sin(angular_tol_rad)`. This is the small-angle linearisation
/// of "the angle between n and the axis is within `(π/2 ± tol)`":
/// when the angle is exactly π/2 the dot is 0; the projection grows as
/// `sin(deviation)` for small deviations.
///
/// **Sign-tolerant**: a face whose normal is anti-parallel to the axis is
/// considered as parallel (not perpendicular) — both `+axis` and `-axis`
/// are equally "the axis direction" for the purposes of this filter.
/// This matches PRD line 76's `#X` operator (direction-agnostic).
///
/// # Errors
///
/// - Returns `QueryError::QueryFailed` if `axis` is the zero vector or
///   contains a non-finite component.
/// - Propagates any error from `extract_faces`.
/// - Propagates any error from a per-face `FaceNormal` query.
/// - Returns `QueryError::QueryFailed` on a malformed `FaceNormal` payload
///   or a degenerate (near-zero magnitude) normal.
pub fn faces_perpendicular_to<K: GeometryKernel + ?Sized>(
    kernel: &mut K,
    handle: GeometryHandleId,
    axis: [f64; 3],
    angular_tol_rad: f64,
) -> Result<Vec<GeometryHandleId>, QueryError> {
    let axis = normalize3(axis).ok_or_else(|| {
        QueryError::QueryFailed(
            "faces_perpendicular_to: axis direction must be non-zero and finite".into(),
        )
    })?;
    // |n · axis| <= sin(tol) means n is perpendicular to axis within `tol` of π/2.
    let sin_tol = angular_tol_rad.sin();
    let faces = kernel.extract_faces(handle)?;
    filter_by_value(
        kernel,
        &faces,
        "faces_perpendicular_to",
        GeometryQuery::FaceNormal,
        |id, value| {
            let raw = parse_xyz_value(value, "FaceNormal")?;
            let normal = normalize3(raw).ok_or_else(|| {
                QueryError::QueryFailed(format!(
                    "FaceNormal({:?}) returned a degenerate (near-zero) normal",
                    id
                ))
            })?;
            Ok(dot3(normal, axis).abs() <= sin_tol)
        },
    )
}
