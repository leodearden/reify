//! v0.2 selector vocabulary v2 — combinators, direction/extremal/type
//! filters, history/attribute selectors, and topological walks
//! (task 2658, PRD `docs/prds/v0_2/persistent-naming-v2.md` task 10,
//! lines 74-82).
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
//!
//! # PRD vocabulary → Rust API mapping
//!
//! Cross-reference for the PRD vocabulary slots (PRD lines 74-82). The
//! `+X` / `+vec` direction filter for *parallel-to-axis with sign* lives
//! in `topology_selectors::faces_by_normal` / `edges_parallel_to`
//! (v0.1, retained); everything else is in this module.
//!
//! - `+X` / `+Y` / `+Z` / `+vec(<vec3>)` — signed direction filter
//!   → [`crate::topology_selectors::faces_by_normal`] (v0.1, retained).
//! - `|X` / `|Y` / `|Z` — sign-tolerant parallel-to-axis filter for
//!   edges → [`crate::topology_selectors::edges_parallel_to`] (v0.1,
//!   retained); for faces "parallel to axis" means "normal perpendicular
//!   to axis", i.e. → [`faces_perpendicular_to`].
//! - `#X` / `#Y` / `#Z` — perpendicular-to-axis filter
//!   → [`faces_perpendicular_to`] / [`edges_perpendicular_to`].
//! - `>X` / `>Y` / `>Z` (and `<X` / `<Y` / `<Z`) — extremal-by-bounds
//!   → [`extremal_by_bbox`] (sense via [`ExtremalSense::Max`] / `Min`).
//! - `>>X` / `>>Y` / `>>Z` (and `<<X` / `<<Y` / `<<Z`) — extremal-by-center
//!   → [`extremal_by_centroid`].
//! - `%Plane` / `%Cylinder` / `%Cone` / `%Sphere` / `%Torus` — face
//!   surface-kind filter → [`faces_by_surface_kind`].
//! - `%Line` / `%Circle` / `%Ellipse` — edge curve-kind filter
//!   → [`edges_by_curve_kind`].
//! - `%Geom` — universal pass-through identity → [`geom_universal`].
//! - `and` / `or` / `not` / `except` — Boolean combinators over handle
//!   slices → [`intersect`] / [`union`] / [`complement`] / [`except`].
//! - `adjacent_to(face)` — topological walk → [`adjacent_to_face`].
//! - `owner_body(sub)` — provenance walk → [`owner_body_of`].
//! - `ancestors(edge)` — topological walk → [`ancestor_faces_of_edge`].
//! - `siblings(face)` — topological walk → [`siblings_of_face`].
//! - `created_by(feature_id)` (`qCreatedBy`) — history-based selector
//!   → [`created_by_feature`].
//! - `split_by(feature_id)` (`qSplitBy`) — history-based selector,
//!   any-position match in `mod_history` → [`split_by_feature`].
//! - `has_attribute("user_label")` / `attribute_eq("user_label", v)` —
//!   v0.2 attribute primitive → [`has_user_label`] / [`user_label_eq`].
//!   (See the latter's rustdoc for the v0.3 generalisation path.)

use std::collections::HashSet;

use reify_ir::{EdgeCurveKind, FaceSurfaceKind, FeatureId, GeometryHandleId, GeometryKernel, GeometryQuery, QueryError, TopologyAttributeTable, Value};

use crate::topology_selectors::{
    dot3, filter_by_value, normalize3, parse_bbox_axis_extents, parse_xyz_value,
    query_per_subshape, validate_angular_tol,
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
/// - Returns `QueryError::QueryFailed` if `angular_tol_rad` is not finite or
///   outside the valid range `[0, π/2]`. Values beyond π/2 are non-monotonic
///   (sin decreases past π/2), so only `[0, π/2]` has well-defined semantics.
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
    validate_angular_tol(
        "faces_perpendicular_to",
        angular_tol_rad,
        std::f64::consts::FRAC_PI_2,
        "π/2",
    )?;
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
/// - Returns `QueryError::QueryFailed` if `angular_tol_rad` is not finite or
///   outside the valid range `[0, π/2]`. Values beyond π/2 are non-monotonic
///   (sin decreases past π/2), so only `[0, π/2]` has well-defined semantics.
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
    validate_angular_tol(
        "edges_perpendicular_to",
        angular_tol_rad,
        std::f64::consts::FRAC_PI_2,
        "π/2",
    )?;
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
/// - Returns `QueryError::QueryFailed` if any extracted extent is
///   non-finite (NaN or ±∞) — defence-in-depth against a misbehaving
///   kernel that would otherwise propagate non-deterministically through
///   `f64::max`/`f64::min`.
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
    // Defence-in-depth: a NaN metric would propagate inconsistently through
    // `f64::max`/`f64::min` (depending on operand order), letting a NaN
    // candidate silently slip into or out of the extreme cluster. Refuse
    // the query rather than emit a non-deterministic result. Mirrors the
    // `is_finite` discipline in `topology_selectors::normalize3`.
    if metrics.iter().any(|m| !m.is_finite()) {
        return Err(QueryError::QueryFailed(
            "extremal_by_bbox: BoundingBox payload contained a non-finite extent".into(),
        ));
    }
    // Find the global extreme; an empty `candidates` was short-circuited
    // above, so `metrics` is non-empty.
    let extreme = metrics.iter().copied().fold(
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

/// Return the cluster of `candidates` whose `Centroid` projection onto
/// `axis` is within `tol_m` of the global extreme (max or min).
///
/// By-center counterpart of [`extremal_by_bbox`] (PRD line 77's `>>axis`
/// slot). The two diverge on non-flat faces: a curved face's centroid
/// can lie inside the bbox interior even though the bbox extent reaches
/// further along the axis. Both selectors are first-class so callers
/// can choose semantically (e.g. parting-line selection prefers
/// centroid-based; clearance checks prefer bbox-based).
///
/// Issues a single batched `kernel.query_many` for all candidate
/// `Centroid` reads, projects each onto `axis` (using the axis-aligned
/// unit vector — equivalent to reading `centroid[axis]` for the three
/// Cartesian axes), and returns the cluster in input order with
/// dedup-on-first-seen.
///
/// # Edge cases
///
/// - Empty `candidates` → `Ok(Vec::new())`.
/// - On a tie cluster of size > 1, no diagnostic is emitted here — the
///   caller is expected to chain a uniqueness resolver
///   (`resolve_unique_by_attribute`, etc.) for that signal.
///
/// # Errors
///
/// - Propagates any error from `query_many`.
/// - Returns `QueryError::QueryFailed` on a malformed `Centroid`
///   payload (non-string, non-JSON, missing fields).
/// - Returns `QueryError::QueryFailed` if any centroid component is
///   non-finite (NaN or ±∞) — defence-in-depth against a misbehaving
///   kernel that would otherwise propagate non-deterministically through
///   `f64::max`/`f64::min`.
pub fn extremal_by_centroid<K: GeometryKernel + ?Sized>(
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
        "extremal_by_centroid",
        GeometryQuery::Centroid,
    )?;
    // Project each centroid onto the chosen axis. The axis-aligned unit
    // vector reduces a generic dot-product to a single component pick,
    // but using `dot3` keeps the code symmetric with the direction
    // filters and ready for an oblique-axis extension if that ever
    // arrives.
    let axis_vec = axis.unit();
    let mut metrics: Vec<f64> = Vec::with_capacity(candidates.len());
    for value in &values {
        let xyz = parse_xyz_value(value, "Centroid")?;
        metrics.push(dot3(xyz, axis_vec));
    }
    // Defence-in-depth: a NaN metric would propagate inconsistently through
    // `f64::max`/`f64::min` (depending on operand order), letting a NaN
    // candidate silently slip into or out of the extreme cluster. Refuse
    // the query rather than emit a non-deterministic result. Mirrors the
    // `is_finite` discipline in `topology_selectors::normalize3`.
    if metrics.iter().any(|m| !m.is_finite()) {
        return Err(QueryError::QueryFailed(
            "extremal_by_centroid: Centroid payload contained a non-finite component".into(),
        ));
    }
    // Find the global extreme; non-empty since `candidates` is non-empty.
    let extreme = metrics.iter().copied().fold(
        match sense {
            ExtremalSense::Max => f64::NEG_INFINITY,
            ExtremalSense::Min => f64::INFINITY,
        },
        |acc, v| match sense {
            ExtremalSense::Max => acc.max(v),
            ExtremalSense::Min => acc.min(v),
        },
    );
    let mut seen: HashSet<GeometryHandleId> = HashSet::with_capacity(candidates.len());
    let mut out: Vec<GeometryHandleId> = Vec::new();
    for (id, metric) in candidates.iter().zip(metrics.iter()) {
        if (metric - extreme).abs() <= tol_m && seen.insert(*id) {
            out.push(*id);
        }
    }
    Ok(out)
}

// ─────────────────────────────────────────────────────────────────────────────
// Geometry-type filters (PRD line 78)
//
// `%Plane` / `%Cylinder` / `%Cone` / `%Sphere` / `%Torus` for faces and
// `%Line` / `%Circle` / `%Ellipse` / `%Hyperbola` / `%Parabola` for edges
// dispatch the kernel's surface/curve classification (`GeomAbs_*` via
// OCCT's `BRepAdaptor_Surface::GetType()` / `BRepAdaptor_Curve::GetType()`)
// and retain the sub-shapes whose kind matches.
//
// `%Geom` is the universal no-op: every Geometry trivially satisfies it,
// so the filter returns the input slice unchanged. It exists purely so
// downstream chains can compose without a special-case "no kind filter"
// branch.
// ─────────────────────────────────────────────────────────────────────────────

/// Decode a `Value::String(name)` payload into the typed kind `T`,
/// returning a `QueryError::QueryFailed` (with `query_label` and `id`
/// embedded) on a non-string payload or an unknown canonical name.
///
/// `decode` is the kind-specific decoder
/// ([`FaceSurfaceKind::try_from_str`] or
/// [`EdgeCurveKind::try_from_str`]); both share identical control flow,
/// so this helper centralises the diagnostic format and prevents the two
/// selectors drifting in their error messages.
fn parse_kind_string<T, F>(
    value: &Value,
    id: GeometryHandleId,
    query_label: &'static str,
    decode: F,
) -> Result<T, QueryError>
where
    F: Fn(&str) -> Result<T, &str>,
{
    let s = match value {
        Value::String(s) => s,
        other => {
            return Err(QueryError::QueryFailed(format!(
                "{query_label}({id:?}) expected Value::String, got {other:?}"
            )));
        }
    };
    decode(s).map_err(|name| {
        QueryError::QueryFailed(format!(
            "{query_label}({id:?}) returned unknown kind name {name:?}"
        ))
    })
}

/// Return the subset of `extract_faces(handle)` whose underlying surface
/// classifies as `kind` per [`GeometryQuery::FaceSurfaceKind`] (OCCT's
/// `BRepAdaptor_Surface::GetType()`).
///
/// Implements PRD line 78's `%Plane`/`%Cylinder`/`%Cone`/`%Sphere`/`%Torus`
/// filters. Issues a single batched `kernel.query_many` for the candidate
/// slice (matching the v0.1 selector batching discipline), parses each
/// `Value::String` reply via [`FaceSurfaceKind::try_from_str`], and
/// retains faces whose decoded kind is exactly equal to `kind`.
///
/// # Errors
///
/// - Propagates any error from `extract_faces`.
/// - Propagates any error from a per-face `FaceSurfaceKind` query.
/// - Returns `QueryError::QueryFailed` on a non-string payload or an
///   unknown canonical kind-name (defence-in-depth against a misbehaving
///   kernel).
pub fn faces_by_surface_kind<K: GeometryKernel + ?Sized>(
    kernel: &mut K,
    handle: GeometryHandleId,
    kind: FaceSurfaceKind,
) -> Result<Vec<GeometryHandleId>, QueryError> {
    let faces = kernel.extract_faces(handle)?;
    filter_by_value(
        kernel,
        &faces,
        "faces_by_surface_kind",
        GeometryQuery::FaceSurfaceKind,
        |id, value| {
            let parsed =
                parse_kind_string(value, id, "FaceSurfaceKind", FaceSurfaceKind::try_from_str)?;
            Ok(parsed == kind)
        },
    )
}

/// Return the subset of `extract_edges(handle)` whose underlying curve
/// classifies as `kind` per [`GeometryQuery::EdgeCurveKind`] (OCCT's
/// `BRepAdaptor_Curve::GetType()`).
///
/// Implements PRD line 78's `%Line`/`%Circle`/`%Ellipse`/`%Hyperbola`/`%Parabola`
/// filters. Symmetric to [`faces_by_surface_kind`] for edges — same
/// batching, same error shape.
///
/// # Errors
///
/// - Propagates any error from `extract_edges`.
/// - Propagates any error from a per-edge `EdgeCurveKind` query.
/// - Returns `QueryError::QueryFailed` on a non-string payload or an
///   unknown canonical kind-name.
pub fn edges_by_curve_kind<K: GeometryKernel + ?Sized>(
    kernel: &mut K,
    handle: GeometryHandleId,
    kind: EdgeCurveKind,
) -> Result<Vec<GeometryHandleId>, QueryError> {
    let edges = kernel.extract_edges(handle)?;
    filter_by_value(
        kernel,
        &edges,
        "edges_by_curve_kind",
        GeometryQuery::EdgeCurveKind,
        |id, value| {
            let parsed =
                parse_kind_string(value, id, "EdgeCurveKind", EdgeCurveKind::try_from_str)?;
            Ok(parsed == kind)
        },
    )
}

/// `%Geom` — the universal geometry-type filter (PRD line 78).
///
/// Every `GeometryHandleId` trivially satisfies the "is geometry"
/// predicate, so this filter returns the input slice unchanged: same
/// order, same length, same multiplicities (no dedup, in contrast to the
/// combinators above). It exists so callers can compose chains uniformly
/// — substituting `geom_universal` for a kind-specific filter is a
/// syntactic identity at the chain level.
///
/// Pure-Rust, no kernel dependency — `O(n)` clone of the slice.
pub fn geom_universal(handles: &[GeometryHandleId]) -> Vec<GeometryHandleId> {
    handles.to_vec()
}

// ─────────────────────────────────────────────────────────────────────────────
// History-based selectors (PRD line 80)
//
// `created_by_feature(feature_id)` returns candidates whose `feature_id` is
// the topology entity's origin feature (`qCreatedBy` in OnShape). It is the
// inverse mapping of `FeatureTagTable::record` — given a feature, surface
// the entities it produced.
//
// `split_by_feature(feature_id)` returns candidates whose `mod_history`
// contains the feature anywhere (any-position match, not just the most
// recent entry). Aligns with OnShape's `qSplitBy` and the FreeCAD-RealThunder
// `;:M2`/`;:G3` postfix model — a child of multiple sequential splits should
// match a query for any of its splitting ancestors.
//
// Both selectors are pure-Rust readers over `TopologyAttributeTable`; they
// take a `&TopologyAttributeTable` and `&[GeometryHandleId]` (mirroring
// `resolve_unique_by_attribute`'s discipline) and never touch the kernel.
// Order discipline: candidate-input order with dedup-on-first-seen.
// ─────────────────────────────────────────────────────────────────────────────

/// Return the subset of `candidates` whose origin feature (per
/// `TopologyAttribute::feature_id`) equals `feature_id`.
///
/// Implements PRD line 80's `created_by(feature_id)` slot, mirroring
/// OnShape's `qCreatedBy(feature_id)`. Walks `candidates` once, looking
/// each handle up in `table`; a handle whose entry is missing or whose
/// feature does not match is silently skipped (no panic, no error).
///
/// Order discipline: candidate-input order, dedup-on-first-seen.
/// O(|candidates|) — single pass with a `HashSet` for dedup.
pub fn created_by_feature(
    table: &TopologyAttributeTable,
    candidates: &[GeometryHandleId],
    feature_id: &FeatureId,
) -> Vec<GeometryHandleId> {
    let mut seen: HashSet<GeometryHandleId> = HashSet::with_capacity(candidates.len());
    let mut out: Vec<GeometryHandleId> = Vec::new();
    for id in candidates {
        if let Some(attr) = table.lookup(*id)
            && &attr.feature_id == feature_id
            && seen.insert(*id)
        {
            out.push(*id);
        }
    }
    out
}

/// Return the subset of `candidates` whose `mod_history` contains
/// `feature_id` at **any position** (not just the most recent entry).
///
/// Implements PRD line 80's `split_by(feature_id)` slot, mirroring
/// OnShape's `qSplitBy(feature_id)`. A handle whose attribute has no
/// `mod_history` (e.g. an entity unaffected by any split operation) is
/// trivially excluded; an entity that was split by F3 then later split
/// by F4 matches **both** `split_by_feature(F3)` and `split_by_feature(F4)`.
///
/// Any-position match (rather than leaf-only) is the OnShape baseline
/// (PRD line 81): a child of multiple sequential splits should remain
/// queryable by every splitting ancestor. The check is `O(history depth)`
/// per candidate; designs stay shallow in practice (PRD line 141).
///
/// Order discipline: candidate-input order, dedup-on-first-seen.
pub fn split_by_feature(
    table: &TopologyAttributeTable,
    candidates: &[GeometryHandleId],
    feature_id: &FeatureId,
) -> Vec<GeometryHandleId> {
    let mut seen: HashSet<GeometryHandleId> = HashSet::with_capacity(candidates.len());
    let mut out: Vec<GeometryHandleId> = Vec::new();
    for id in candidates {
        if let Some(attr) = table.lookup(*id) {
            let matches = attr
                .mod_history
                .iter()
                .any(|entry| &entry.splitting_feature_id == feature_id);
            if matches && seen.insert(*id) {
                out.push(*id);
            }
        }
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────────
// Attribute primitives (PRD line 82)
//
// These selectors implement the PRD's `has_attribute(key)` /
// `attribute_eq(key, value)` slots, surfacing the v0.2 attribute scheme
// directly to user queries. In v0.2 the only "key" they address is
// `user_label`: the other attribute fields (`feature_id`, `role`,
// `local_index`, `mod_history`) are positional and have first-class
// selectors above (`created_by_feature`, `split_by_feature`). If a future
// version of the PRD adds a free-form `attributes: HashMap<String,
// String>` field, these symbols generalise to a `(key, value)` shape.
// ─────────────────────────────────────────────────────────────────────────────

/// Return the subset of `candidates` whose attribute has a `user_label`
/// set (i.e. `user_label = Some(_)`).
///
/// PRD line 82's `has_attribute(key)` for the `user_label` key. A handle
/// missing from the table cannot have a user label by construction and
/// is silently skipped (no panic). Order discipline: candidate-input
/// order, dedup-on-first-seen.
pub fn has_user_label(
    table: &TopologyAttributeTable,
    candidates: &[GeometryHandleId],
) -> Vec<GeometryHandleId> {
    let mut seen: HashSet<GeometryHandleId> = HashSet::with_capacity(candidates.len());
    let mut out: Vec<GeometryHandleId> = Vec::new();
    for id in candidates {
        if let Some(attr) = table.lookup(*id)
            && attr.user_label.is_some()
            && seen.insert(*id)
        {
            out.push(*id);
        }
    }
    out
}

/// Return the subset of `candidates` whose attribute has
/// `user_label = Some(label)` (exact, case-sensitive match).
///
/// PRD line 82's `attribute_eq(key, value)` for the `user_label` key. A
/// handle whose `user_label = None` does not match any query (not even
/// the empty string). Equality is byte-for-byte exact — no case-folding
/// or whitespace trimming. Order discipline: candidate-input order,
/// dedup-on-first-seen.
pub fn user_label_eq(
    table: &TopologyAttributeTable,
    candidates: &[GeometryHandleId],
    label: &str,
) -> Vec<GeometryHandleId> {
    let mut seen: HashSet<GeometryHandleId> = HashSet::with_capacity(candidates.len());
    let mut out: Vec<GeometryHandleId> = Vec::new();
    for id in candidates {
        if let Some(attr) = table.lookup(*id)
            && attr.user_label.as_deref() == Some(label)
            && seen.insert(*id)
        {
            out.push(*id);
        }
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────────
// Topological walks (PRD line 81)
//
// `adjacent_to_face(parent, face)` returns the faces of `parent` that share
// at least one edge with `face` (PRD line 81's `adjacent_to` slot for face
// adjacency). Composes:
//   1. `extract_faces(parent)` to get the canonical face list, and to map
//      `face_handle` → 0-based `face_index` for the kernel call.
//   2. `GeometryQuery::AdjacentFaces { shape: parent, face_index }` which
//      returns a `Value::List(Vec<Value::Int>)` of global face indices.
//   3. Map each returned index back to a `GeometryHandleId` via the
//      canonical face list.
//
// A `face_handle` not present in `extract_faces(parent)` cannot be mapped
// to a 0-based index without a re-extraction; the selector errors with
// `"not a child of parent"` rather than silently returning an empty result.
// ─────────────────────────────────────────────────────────────────────────────

/// Return the subset of `extract_faces(parent)` that share at least one
/// edge with `face_handle` (i.e. are face-adjacent to it).
///
/// Implements PRD line 81's `adjacent_to(face)` slot. Issues exactly one
/// `extract_faces` call (to recover the canonical face list / face_index
/// mapping) and one `AdjacentFaces` query (which under OCCT walks the
/// cached `edge_face_map`).
///
/// # Errors
///
/// - Propagates any error from `extract_faces`.
/// - Returns `QueryError::QueryFailed("… not a child of parent (was extract_faces(parent) called?)")`
///   if `face_handle` does not appear in `extract_faces(parent)`.
/// - Propagates any error from the `AdjacentFaces` query.
/// - Returns `QueryError::QueryFailed` on a malformed `AdjacentFaces`
///   payload (non-`Value::List`, non-`Value::Int` element, or an index
///   outside the `extract_faces` range).
pub fn adjacent_to_face<K: GeometryKernel + ?Sized>(
    kernel: &mut K,
    parent: GeometryHandleId,
    face_handle: GeometryHandleId,
) -> Result<Vec<GeometryHandleId>, QueryError> {
    let faces = kernel.extract_faces(parent)?;
    let face_index = faces.iter().position(|id| *id == face_handle).ok_or_else(|| {
        QueryError::QueryFailed(format!(
            "adjacent_to_face: face {face_handle:?} is not a child of parent {parent:?} (was extract_faces(parent) called?)"
        ))
    })?;
    let value = kernel.query(&GeometryQuery::AdjacentFaces {
        shape: parent,
        face_index,
    })?;
    let indices = match &value {
        Value::List(items) => items,
        other => {
            return Err(QueryError::QueryFailed(format!(
                "adjacent_to_face: expected Value::List from AdjacentFaces, got {other:?}"
            )));
        }
    };
    // Dedup-on-first-seen mirrors the combinator discipline. OCCT's
    // `edge_face_map` produces unique indices in practice, but a
    // misbehaving kernel could otherwise leak duplicates through this
    // selector while every other selector in this module absorbs them.
    let mut seen: HashSet<GeometryHandleId> = HashSet::with_capacity(indices.len());
    let mut out: Vec<GeometryHandleId> = Vec::with_capacity(indices.len());
    for item in indices {
        let idx = match item {
            Value::Int(i) => *i,
            other => {
                return Err(QueryError::QueryFailed(format!(
                    "adjacent_to_face: expected Value::Int element in AdjacentFaces list, got {other:?}"
                )));
            }
        };
        let usize_idx: usize = idx.try_into().map_err(|_| {
            QueryError::QueryFailed(format!(
                "adjacent_to_face: AdjacentFaces returned negative index {idx}"
            ))
        })?;
        let neighbour = *faces.get(usize_idx).ok_or_else(|| {
            QueryError::QueryFailed(format!(
                "adjacent_to_face: AdjacentFaces index {usize_idx} is out of range for extract_faces(parent) (len = {})",
                faces.len()
            ))
        })?;
        if seen.insert(neighbour) {
            out.push(neighbour);
        }
    }
    Ok(out)
}

/// Return the subset of `extract_faces(parent)` that own `edge_handle`
/// (i.e. are face-ancestors of the edge in topology terms).
///
/// Implements PRD line 81's `ancestors(edge)` slot. Issues exactly one
/// `extract_edges` call (to recover the canonical edge list / edge_index
/// mapping), one `extract_faces` call (to map index → handle for the
/// reply), and one `AncestorFacesOfEdge` query (which under OCCT walks
/// the cached `edge_face_map`).
///
/// For a manifold solid every edge has exactly two ancestor faces, but
/// the kernel does not enforce this — degenerate / seam / non-manifold
/// edges may surface 1 or > 2.
///
/// # Errors
///
/// - Propagates any error from `extract_edges` / `extract_faces`.
/// - Returns `QueryError::QueryFailed("… not a child of parent (was extract_edges(parent) called?)")`
///   if `edge_handle` does not appear in `extract_edges(parent)`.
/// - Propagates any error from the `AncestorFacesOfEdge` query.
/// - Returns `QueryError::QueryFailed` on a malformed `AncestorFacesOfEdge`
///   payload (non-`Value::List`, non-`Value::Int` element, or an index
///   outside the `extract_faces` range).
pub fn ancestor_faces_of_edge<K: GeometryKernel + ?Sized>(
    kernel: &mut K,
    parent: GeometryHandleId,
    edge_handle: GeometryHandleId,
) -> Result<Vec<GeometryHandleId>, QueryError> {
    let edges = kernel.extract_edges(parent)?;
    let edge_index = edges.iter().position(|id| *id == edge_handle).ok_or_else(|| {
        QueryError::QueryFailed(format!(
            "ancestor_faces_of_edge: edge {edge_handle:?} is not a child of parent {parent:?} (was extract_edges(parent) called?)"
        ))
    })?;
    // Faces are needed to map the kernel's integer indices back to
    // `GeometryHandleId`s; do this before issuing the kernel query so any
    // extraction error surfaces ahead of the FFI roundtrip.
    let faces = kernel.extract_faces(parent)?;
    let value = kernel.query(&GeometryQuery::AncestorFacesOfEdge {
        shape: parent,
        edge_index,
    })?;
    let indices = match &value {
        Value::List(items) => items,
        other => {
            return Err(QueryError::QueryFailed(format!(
                "ancestor_faces_of_edge: expected Value::List from AncestorFacesOfEdge, got {other:?}"
            )));
        }
    };
    // Dedup-on-first-seen mirrors the combinator discipline. OCCT's
    // `edge_face_map` produces unique indices in practice, but a
    // misbehaving kernel could otherwise leak duplicates through this
    // selector while every other selector in this module absorbs them.
    let mut seen: HashSet<GeometryHandleId> = HashSet::with_capacity(indices.len());
    let mut out: Vec<GeometryHandleId> = Vec::with_capacity(indices.len());
    for item in indices {
        let idx = match item {
            Value::Int(i) => *i,
            other => {
                return Err(QueryError::QueryFailed(format!(
                    "ancestor_faces_of_edge: expected Value::Int element in AncestorFacesOfEdge list, got {other:?}"
                )));
            }
        };
        let usize_idx: usize = idx.try_into().map_err(|_| {
            QueryError::QueryFailed(format!(
                "ancestor_faces_of_edge: AncestorFacesOfEdge returned negative index {idx}"
            ))
        })?;
        let parent_face = *faces.get(usize_idx).ok_or_else(|| {
            QueryError::QueryFailed(format!(
                "ancestor_faces_of_edge: AncestorFacesOfEdge index {usize_idx} is out of range for extract_faces(parent) (len = {})",
                faces.len()
            ))
        })?;
        if seen.insert(parent_face) {
            out.push(parent_face);
        }
    }
    Ok(out)
}

/// Return every face of `parent` other than `face_handle`, preserving
/// canonical `extract_faces` order.
///
/// Implements PRD line 81's `siblings(face)` slot. Composes
/// `extract_faces(parent)` with a single filter step that drops the
/// matching handle. Order discipline: canonical `extract_faces` order
/// (kernel face lists are duplicate-free, so no extra dedup is needed).
///
/// Symmetric with [`adjacent_to_face`] in error shape: a `face_handle`
/// not present in `extract_faces(parent)` errors with
/// `"not a child of parent"` rather than silently returning the full
/// face list.
///
/// # Errors
///
/// - Propagates any error from `extract_faces`.
/// - Returns `QueryError::QueryFailed("… not a child of parent (was extract_faces(parent) called?)")`
///   if `face_handle` does not appear in `extract_faces(parent)`.
pub fn siblings_of_face<K: GeometryKernel + ?Sized>(
    kernel: &mut K,
    parent: GeometryHandleId,
    face_handle: GeometryHandleId,
) -> Result<Vec<GeometryHandleId>, QueryError> {
    let faces = kernel.extract_faces(parent)?;
    if !faces.contains(&face_handle) {
        return Err(QueryError::QueryFailed(format!(
            "siblings_of_face: face {face_handle:?} is not a child of parent {parent:?} (was extract_faces(parent) called?)"
        )));
    }
    Ok(faces.into_iter().filter(|f| *f != face_handle).collect())
}

/// Recover the parent body handle of a sub-shape produced by
/// [`reify_types::GeometryKernel::extract_edges`] /
/// [`reify_types::GeometryKernel::extract_faces`].
///
/// Implements PRD line 81's `owner_body(sub)` topological walk. The
/// kernel records the parent on every `extract_*` call (the OCCT kernel
/// keeps a `parent_handle` map; the mock kernel routes through
/// `with_owner_body_result`), so any sub-handle can answer "what solid
/// did I come from?" without re-extraction.
///
/// Pure read — takes `&K` rather than `&mut K`. No allocation, no
/// extra-call: a single [`GeometryQuery::OwnerBody`] dispatch.
///
/// # Errors
///
/// - Propagates any error from the `OwnerBody` query (in particular,
///   the OCCT kernel returns `QueryError::QueryFailed("owner_body: …
///   has no recorded parent …")` when the handle was not produced by
///   `extract_edges` / `extract_faces`).
/// - Returns `QueryError::QueryFailed` on a malformed `OwnerBody`
///   payload (non-`Value::Int`, or a negative integer).
pub fn owner_body_of<K: GeometryKernel + ?Sized>(
    kernel: &K,
    sub_handle: GeometryHandleId,
) -> Result<GeometryHandleId, QueryError> {
    let value = kernel.query(&GeometryQuery::OwnerBody(sub_handle))?;
    match value {
        Value::Int(i) => {
            let parent_id_u64: u64 = i.try_into().map_err(|_| {
                QueryError::QueryFailed(format!(
                    "owner_body_of: kernel returned negative parent id {i}"
                ))
            })?;
            Ok(GeometryHandleId(parent_id_u64))
        }
        other => Err(QueryError::QueryFailed(format!(
            "owner_body_of: expected Value::Int from OwnerBody, got {other:?}"
        ))),
    }
}
