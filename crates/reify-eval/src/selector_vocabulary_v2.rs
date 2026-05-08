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

use crate::topology_selectors::{
    dot3, filter_by_value, normalize3, parse_bbox_axis_extents, parse_xyz_value,
    query_per_subshape,
};

// ─────────────────────────────────────────────────────────────────────────────
// Axis / ExtremalSense — direction enums for extremal selectors (PRD line 77)
// ─────────────────────────────────────────────────────────────────────────────

/// Cartesian axis for direction-aware selectors and extremal queries.
///
/// Used by [`extremal_by_bbox`] (and the upcoming `extremal_by_centroid`)
/// to pick which component of a `BoundingBox` / `Centroid` payload to
/// compare. The PRD vocabulary slots `>X` / `>Y` / `>Z` map to
/// `Axis::{X, Y, Z}`; sign is carried by [`ExtremalSense`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    /// Return the axis byte tag (`b'x' | b'y' | b'z'`) used by
    /// [`crate::topology_selectors::parse_bbox_axis_extents`].
    pub(crate) fn as_byte(self) -> u8 {
        match self {
            Axis::X => b'x',
            Axis::Y => b'y',
            Axis::Z => b'z',
        }
    }

    /// Return the axis-aligned unit vector (used by direction filters and
    /// to project a Centroid payload onto a single component).
    pub(crate) fn unit(self) -> [f64; 3] {
        match self {
            Axis::X => [1.0, 0.0, 0.0],
            Axis::Y => [0.0, 1.0, 0.0],
            Axis::Z => [0.0, 0.0, 1.0],
        }
    }
}

/// Sense of an extremal selector — whether to pick the maximum or minimum
/// candidate along the chosen axis. Maps to the PRD vocabulary's `>axis`
/// (Max — "highest") and the symmetric `<axis` (Min — "lowest").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExtremalSense {
    Max,
    Min,
}

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

/// Return the subset of `extract_edges(handle)` whose midpoint tangent is
/// **perpendicular** to `axis` within `angular_tol_rad`.
///
/// An edge is retained iff its (unit) tangent `t` satisfies
/// `|t · axis| <= sin(angular_tol_rad)`. Symmetric counterpart of
/// [`faces_perpendicular_to`] for edges.
///
/// **Sign-tolerant**: the kernel may return either direction along an
/// edge, so a tangent's sign is irrelevant — the absolute-cosine check
/// `|t · axis|` already provides this.
///
/// # Errors
///
/// - Returns `QueryError::QueryFailed` if `axis` is the zero vector or
///   contains a non-finite component.
/// - Propagates any error from `extract_edges`.
/// - Propagates any error from a per-edge `EdgeTangent` query.
/// - Returns `QueryError::QueryFailed` on a malformed `EdgeTangent`
///   payload or a degenerate (near-zero magnitude) tangent.
pub fn edges_perpendicular_to<K: GeometryKernel + ?Sized>(
    kernel: &mut K,
    handle: GeometryHandleId,
    axis: [f64; 3],
    angular_tol_rad: f64,
) -> Result<Vec<GeometryHandleId>, QueryError> {
    let axis = normalize3(axis).ok_or_else(|| {
        QueryError::QueryFailed(
            "edges_perpendicular_to: axis direction must be non-zero and finite".into(),
        )
    })?;
    let sin_tol = angular_tol_rad.sin();
    let edges = kernel.extract_edges(handle)?;
    filter_by_value(
        kernel,
        &edges,
        "edges_perpendicular_to",
        GeometryQuery::EdgeTangent,
        |id, value| {
            let raw = parse_xyz_value(value, "EdgeTangent")?;
            let tan = normalize3(raw).ok_or_else(|| {
                QueryError::QueryFailed(format!(
                    "EdgeTangent({:?}) returned a degenerate (near-zero) tangent",
                    id
                ))
            })?;
            Ok(dot3(tan, axis).abs() <= sin_tol)
        },
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// Extremal selectors (PRD line 77)
//
// `>axis` (extremal-by-bounds) walks each candidate's BoundingBox along
// the chosen axis and picks the cluster of candidates whose extreme is
// within `tol_m` of the global extreme. `>>axis` (extremal-by-centroid)
// is the centroid-based counterpart and follows in `extremal_by_centroid`.
//
// Tie semantics: the cluster of candidates within `tol_m` of the global
// extreme is returned in input order. PRD line 66 explicitly accepts that
// symmetric splits are unsolved across the literature; the selector
// returns the full cluster (not an arbitrary single pick) so callers can
// chain another disambiguator (`owner_body_of`, `user_label_eq`, …) or
// surface a `TopologyAttributeStale`-style diagnostic from the resolver.
// ─────────────────────────────────────────────────────────────────────────────

/// Return the cluster of `candidates` whose `BoundingBox` extent along
/// `axis` (using `bbox.max[axis]` for `Max` and `bbox.min[axis]` for `Min`)
/// is within `tol_m` of the global extreme.
///
/// Issues a single batched `kernel.query_many` for the entire candidate
/// slice (matching the v0.1 batching discipline of `topology_selectors`),
/// reads each `BoundingBox` payload via [`parse_bbox_axis_extents`], and
/// returns the cluster in input order with dedup-on-first-seen.
///
/// # Edge cases
///
/// - Empty `candidates` → `Ok(Vec::new())`.
/// - All candidates extreme within `tol_m` of one another → returns all
///   of them (the cluster spans the whole input).
/// - On a tie cluster of size > 1, no diagnostic is emitted here — the
///   caller is expected to chain a uniqueness resolver
///   (`resolve_unique_by_attribute`, etc.) for that signal.
///
/// # Errors
///
/// - Propagates any error from `query_many`.
/// - Returns `QueryError::QueryFailed` on a malformed `BoundingBox`
///   payload (non-string, non-JSON, missing axis fields).
pub fn extremal_by_bbox<K: GeometryKernel + ?Sized>(
    kernel: &mut K,
    candidates: &[GeometryHandleId],
    axis: Axis,
    sense: ExtremalSense,
    tol_m: f64,
) -> Result<Vec<GeometryHandleId>, QueryError> {
    if candidates.is_empty() {
        return Ok(Vec::new());
    }
    // Batched read: one `query_many` for the entire candidate slice.
    let values = query_per_subshape(
        kernel,
        candidates,
        "extremal_by_bbox",
        GeometryQuery::BoundingBox,
    )?;
    // Extract the per-candidate scalar to compare against (bbox.min[axis]
    // for Min, bbox.max[axis] for Max).
    let axis_byte = axis.as_byte();
    let mut metrics: Vec<f64> = Vec::with_capacity(candidates.len());
    for value in &values {
        let (min_v, max_v) = parse_bbox_axis_extents(value, axis_byte)?;
        metrics.push(match sense {
            ExtremalSense::Max => max_v,
            ExtremalSense::Min => min_v,
        });
    }
    // Find the global extreme; an empty `candidates` was short-circuited
    // above, so `metrics` is non-empty.
    let extreme = metrics
        .iter()
        .copied()
        .fold(
            match sense {
                ExtremalSense::Max => f64::NEG_INFINITY,
                ExtremalSense::Min => f64::INFINITY,
            },
            |acc, v| match sense {
                ExtremalSense::Max => acc.max(v),
                ExtremalSense::Min => acc.min(v),
            },
        );
    // Walk candidates in input order, emitting any whose metric is
    // within `tol_m` of `extreme`. Dedup-on-first-seen mirrors the
    // combinator discipline.
    let mut seen: HashSet<GeometryHandleId> = HashSet::with_capacity(candidates.len());
    let mut out: Vec<GeometryHandleId> = Vec::new();
    for (id, metric) in candidates.iter().zip(metrics.iter()) {
        if (metric - extreme).abs() <= tol_m && seen.insert(*id) {
            out.push(*id);
        }
    }
    Ok(out)
}
