//! Filtered topology selectors composed over `GeometryKernel::extract_edges`
//! / `extract_faces` and the batched `GeometryKernel::query_many` path.
//!
//! These are pure-Rust functions over `&mut dyn GeometryKernel` (rather than
//! new `GeometryQuery` variants or new kernel methods) because:
//!   - Filtering needs `&mut` to allocate sub-shape handles via
//!     `extract_edges` / `extract_faces`.
//!   - The kernel layer should stay focused on raw shape access; filter
//!     predicates compose existing primitives (`EdgeLength`,
//!     `SurfaceArea`, …) rather than introducing one FFI call per filter.
//!   - .ri-language wiring (compiler-side) is intentionally out of scope for
//!     this task; the pub Rust API is the boundary and a future task adds
//!     the language surface.
//!
//! Each selector first allocates sub-shape handles, then issues a single
//! `kernel.query_many(&[...])` call for all per-sub-shape reads, then
//! applies its predicate (length window, area window, normal cone, edge-
//! tangent absolute-cosine, bbox-z window) on the returned `Vec<Value>`.
//! This collapses the actor-channel + FFI hop to O(1) per selector
//! regardless of sub-shape count, resolving the N+1 round-trip overhead
//! flagged in the post-merge review of task 318 (see task 2509).
//!
//! All returned `Vec<GeometryHandleId>`s preserve the kernel's canonical
//! sub-shape order (from `TopExp::MapShapes`), filtered to those satisfying
//! the predicate.
//!
//! All length / area / coordinate filter parameters are in SI base units
//! (metres, square metres). Angular tolerances are in radians (matching
//! the rest of reify's geometry kernel — see `revolve` / `rotate_shape`
//! which also take `angle_rad`).

use std::collections::HashSet;

use reify_core::{Diagnostic, DiagnosticCode, DiagnosticLabel, SourceSpan, hash::ContentHash};
use reify_ir::{
    FeatureTag, FeatureTagTable, GeometryHandleId, GeometryKernel, GeometryQuery, QueryError, Value,
};

// ── Sub-handle lowering primitives (task 3616, KGQ-η) ──────────────────────

/// The kind of a topology sub-shape, used as a domain-separation byte in the
/// sub-handle hash (PRD §4).  Discriminant values are intentionally fixed and
/// stable: downstream tasks (KGQ-θ/ι/κ) rely on the hashes being
/// bit-identical across sessions, so the values must never change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SubKind {
    /// An edge sub-shape.  Discriminant byte: `0x01`.
    Edge = 0x01,
    /// A face sub-shape.  Discriminant byte: `0x02`.
    Face = 0x02,
    /// A split-piece solid sub-shape (task 4190, ζ).  Discriminant byte: `0x03`.
    ///
    /// Domain-separates split-piece hashes from edge (`0x01`) and face
    /// (`0x02`) hashes so a split piece and an edge at the same index never
    /// collide.  Existing discriminants are unchanged (the comment at the
    /// enum level forbids changing them; adding a new value is safe).
    Solid = 0x03,
}

impl SubKind {
    /// Return the stable 1-byte domain-separator for this sub-shape kind.
    pub(crate) fn as_byte(self) -> u8 {
        self as u8
    }
}

/// Build the `upstream_values_hash` for a sub-handle (PRD §4).
///
/// The hash is a deterministic 32-byte digest derived from:
///   - `parent_hash`: the parent solid's `upstream_values_hash`
///   - `sub_kind`: Edge (`0x01`), Face (`0x02`), or Solid (`0x03`) — domain separator
///   - `topexp_index`: 0-based canonical index from `TopExp::MapShapes`
///
/// Uses the same `ContentHash` (XXH3-128) + lo/hi 32-byte packing as the
/// parent-hash construction in `engine_build.rs:3311-3336`.  This keeps all
/// hashes in one deterministic domain and adds no new dependencies.
///
/// PRD §4 invariants guaranteed:
///   (ii)  determinism — pure function of `(parent_hash, sub_kind, index)`
///   (iii) index-inequality — index 0 ≠ 1 for any fixed (parent, kind)
///   (iv)  cache-hit equality — same (parent, kind, index) always matches
pub(crate) fn compose_sub_handle_hash(
    parent_hash: &[u8; 32],
    sub_kind: SubKind,
    topexp_index: u32,
) -> [u8; 32] {
    let h = ContentHash::of(b"subh1")
        .combine(ContentHash::of(parent_hash))
        .combine(ContentHash::of(&[sub_kind.as_byte()]))
        .combine(ContentHash::of(&topexp_index.to_le_bytes()));
    let lo = h.0.to_le_bytes();
    let hi = h.combine(ContentHash::of(b"subh2")).0.to_le_bytes();
    let mut out = [0u8; 32];
    out[..16].copy_from_slice(&lo);
    out[16..].copy_from_slice(&hi);
    out
}

/// Construct a `Value::GeometryHandle` sub-handle for a single topology
/// sub-shape (PRD §4, KGQ-η).
///
/// - `parent_realization_ref`: inherited unchanged from the parent solid
///   (PRD §4 invariant i).
/// - `parent_hash`: the parent's `upstream_values_hash` (used as input to
///   `compose_sub_handle_hash`).
/// - `sub_kind`: `SubKind::Edge`, `SubKind::Face`, or `SubKind::Solid` — the
///   domain-separation byte that distinguishes sub-shape hashes at the same
///   index across different shape families.
/// - `topexp_index`: 0-based index of this sub-shape in the canonical
///   `TopExp::MapShapes` order returned by `extract_edges` / `extract_faces`.
/// - `sub_kernel_id`: the session-scoped kernel handle for this sub-shape.
///
/// The resulting `upstream_values_hash` satisfies all PRD §4 invariants:
///   (ii)  deterministic — same `(parent_hash, sub_kind, topexp_index)` always
///         yields the same hash;
///   (iii) per-element distinct — index 0 ≠ index 1 for fixed (parent, kind);
///   (iv)  cache-hit equality — `kernel_handle` is excluded from `PartialEq`,
///         so a re-realized sub-shape with a new session id still matches.
pub(crate) fn make_sub_handle(
    parent_realization_ref: &reify_core::identity::RealizationNodeId,
    parent_hash: &[u8; 32],
    sub_kind: SubKind,
    topexp_index: u32,
    sub_kernel_id: GeometryHandleId,
) -> Value {
    Value::GeometryHandle {
        realization_ref: parent_realization_ref.clone(),
        upstream_values_hash: compose_sub_handle_hash(parent_hash, sub_kind, topexp_index),
        kernel_handle: sub_kernel_id,
    }
}

/// Extract a `Value::Real` payload from a `GeometryQuery` reply, returning a
/// uniformly-formatted `QueryError::QueryFailed` on a non-`Real` reply.
///
/// `query_label` should be the name of the originating query variant (e.g.
/// `"EdgeLength"`, `"SurfaceArea"`) so the error message names what the
/// kernel returned an unexpected payload for. Used by the scalar-window
/// selectors (`edges_by_length`, `faces_by_area`); `edges_at_height` reads
/// a JSON BoundingBox payload via a dedicated parser instead.
fn expect_real(
    query_label: &'static str,
    id: GeometryHandleId,
    value: &Value,
) -> Result<f64, QueryError> {
    match value {
        Value::Real(x) => Ok(*x),
        other => Err(QueryError::QueryFailed(format!(
            "{query_label}({:?}) returned non-real value: {:?}",
            id, other
        ))),
    }
}

/// Defensive length check shared by every selector. Asserts the kernel
/// honored the `query_many` length invariant — `values.len() == ids.len()`
/// — and surfaces `QueryError::QueryFailed` on a mismatch instead of
/// silently truncating selector results via `zip`'s shorter-of-two
/// behaviour. The trait default impl and `OcctKernelHandle`'s override
/// both preserve the invariant; this guards against a misbehaving
/// third-party impl.
fn check_query_many_len(
    selector: &'static str,
    expected: usize,
    got: usize,
) -> Result<(), QueryError> {
    if expected == got {
        Ok(())
    } else {
        Err(QueryError::QueryFailed(format!(
            "{selector}: kernel.query_many returned {got} values for {expected} \
             queries (length invariant violation)"
        )))
    }
}

/// Shared collect / `query_many` / length-check trio used by every filtered
/// selector. Builds a `Vec<GeometryQuery>` from `ids` via `mk_query`, issues
/// a single `kernel.query_many` call, checks the returned length matches the
/// input count (surfacing `QueryError::QueryFailed` with the `selector` label
/// on a mismatch), and returns the `Vec<Value>` on success.
///
/// The selector-specific predicate loop (extract scalar, parse JSON, apply
/// window / cone / dot test) stays in each selector body; only this boilerplate
/// trio moves here.
///
/// Takes `kernel` by shared reference (`&K`) — the helper does not mutate the
/// kernel and is callable from `&self`/`&K` contexts. Callers that hold
/// `&mut K` (needed for the preceding `extract_edges`/`extract_faces` call)
/// compile unchanged because `&mut K` coerces to `&K` automatically.
pub(crate) fn query_per_subshape<K: GeometryKernel + ?Sized, F>(
    kernel: &K,
    ids: &[GeometryHandleId],
    selector: &'static str,
    mk_query: F,
) -> Result<Vec<Value>, QueryError>
where
    F: Fn(GeometryHandleId) -> GeometryQuery,
{
    let queries: Vec<GeometryQuery> = ids.iter().map(|id| mk_query(*id)).collect();
    let values = kernel.query_many(&queries)?;
    check_query_many_len(selector, queries.len(), values.len())?;
    Ok(values)
}

/// Filter `ids` to those for which `predicate(id, &value)` returns `true`,
/// where the value is the kernel's response to `query_ctor(id)`.
///
/// Issues a single batched `kernel.query_many` call (via [`query_per_subshape`]),
/// then applies `predicate` to each `(id, value)` pair in input order.
/// Errors from `predicate` are propagated immediately via `?`.
///
/// `selector_label` is forwarded to `query_per_subshape` and embedded in
/// any `check_query_many_len` error message, so each caller should pass its
/// own distinct label (e.g. `"edges_by_length"` vs `"edges_by_length_with_tags"`).
///
/// `id` is supplied so predicate-side error messages can name the offending
/// sub-shape; predicates that don't need it may use `_id`.
pub(crate) fn filter_by_value<K, Q, F>(
    kernel: &K,
    ids: &[GeometryHandleId],
    selector_label: &'static str,
    query_ctor: Q,
    predicate: F,
) -> Result<Vec<GeometryHandleId>, QueryError>
where
    K: GeometryKernel + ?Sized,
    Q: Fn(GeometryHandleId) -> GeometryQuery,
    F: Fn(GeometryHandleId, &Value) -> Result<bool, QueryError>,
{
    let values = query_per_subshape(kernel, ids, selector_label, query_ctor)?;
    let mut out = Vec::with_capacity(ids.len());
    for (id, value) in ids.iter().zip(values.iter()) {
        if predicate(*id, value)? {
            out.push(*id);
        }
    }
    Ok(out)
}

/// Record a [`FeatureTag`] in `table` for every id in `ids`.
///
/// Each tag is derived from `parent_tag` with `sub_index` set to the
/// enumerate position (overriding `parent_tag.sub_index`). `source_span`
/// and `step_kind` are copied verbatim from `parent_tag`.
///
/// Called by every `*_with_tags` selector **before** applying its filter
/// predicate, ensuring the table is fully populated regardless of which
/// sub-shapes pass the predicate. This centralises the per-child tag-
/// derivation rule so a single change here propagates to all four tagged
/// variants.
fn record_subshape_tags(
    table: &mut FeatureTagTable,
    ids: &[GeometryHandleId],
    parent_tag: FeatureTag,
) {
    for (i, id) in ids.iter().enumerate() {
        table.record(
            *id,
            FeatureTag {
                source_span: parent_tag.source_span,
                step_kind: parent_tag.step_kind,
                sub_index: i as u32,
            },
        );
    }
}

/// Return the subset of `extract_edges(handle)` whose length lies in
/// `[min_m, max_m]` (inclusive on both ends).
///
/// Lengths are queried via `GeometryQuery::EdgeLength` and compared in
/// metres. Edges whose length falls outside the window are dropped.
///
/// # Errors
///
/// - Propagates any error from `extract_edges` (e.g. `InvalidHandle` if
///   `handle` is not registered with the kernel).
/// - Propagates any error from a per-edge `EdgeLength` query.
/// - Returns `QueryError::QueryFailed` if `EdgeLength` ever returns a
///   non-`Value::Real` (a kernel-side contract violation).
pub fn edges_by_length<K: GeometryKernel + ?Sized>(
    kernel: &mut K,
    handle: GeometryHandleId,
    min_m: f64,
    max_m: f64,
) -> Result<Vec<GeometryHandleId>, QueryError> {
    let edges = kernel.extract_edges(handle)?;
    filter_by_value(
        kernel,
        &edges,
        "edges_by_length",
        GeometryQuery::EdgeLength,
        |id, value| {
            let len = expect_real("EdgeLength", id, value)?;
            Ok(len >= min_m && len <= max_m)
        },
    )
}

/// Return the subset of `extract_edges(parent_handle)` whose length lies in
/// `[min_m, max_m]` (inclusive on both ends), while also recording a
/// [`FeatureTag`] for every extracted edge in `table`.
///
/// Mirrors [`edges_by_length`]'s logic exactly — same filter predicate, same
/// canonical sub-shape order — while additionally populating `table` with
/// per-edge tags derived from `parent_tag`: each edge at position `i` in the
/// extracted list gets a tag whose `source_span` and `step_kind` are copied
/// from `parent_tag` and whose `sub_index` is `i as u32`.
///
/// Tags are recorded for **all** extracted edges (before the length-filter
/// runs), so callers can query the table even for edges that do not pass the
/// filter. This matches the recording contract established by
/// [`edges_at_height_with_tags`] (task 2323 / task 2329).
///
/// Downstream consumers can pass the populated table to
/// [`resolve_unique_by_tag`] to pin a specific sub-shape across topology
/// changes, receiving [`DiagnosticCode::TopologyTagStale`] if the
/// unique-tag invariant is later violated.
///
/// # Errors
///
/// Same as [`edges_by_length`].
pub fn edges_by_length_with_tags<K: GeometryKernel + ?Sized>(
    kernel: &mut K,
    table: &mut FeatureTagTable,
    parent_handle: GeometryHandleId,
    parent_tag: FeatureTag,
    min_m: f64,
    max_m: f64,
) -> Result<Vec<GeometryHandleId>, QueryError> {
    let edges = kernel.extract_edges(parent_handle)?;
    record_subshape_tags(table, &edges, parent_tag);
    filter_by_value(
        kernel,
        &edges,
        "edges_by_length_with_tags",
        GeometryQuery::EdgeLength,
        |id, value| {
            let len = expect_real("EdgeLength", id, value)?;
            Ok(len >= min_m && len <= max_m)
        },
    )
}

/// Return the subset of `extract_faces(handle)` whose surface area lies in
/// `[min_m2, max_m2]` (inclusive on both ends).
///
/// Areas are queried via `GeometryQuery::SurfaceArea` and compared in
/// square metres. Faces whose area falls outside the window are dropped.
///
/// # Errors
///
/// - Propagates any error from `extract_faces` (e.g. `InvalidHandle` if
///   `handle` is not registered with the kernel).
/// - Propagates any error from a per-face `SurfaceArea` query.
/// - Returns `QueryError::QueryFailed` if `SurfaceArea` ever returns a
///   non-`Value::Real` (a kernel-side contract violation).
pub fn faces_by_area<K: GeometryKernel + ?Sized>(
    kernel: &mut K,
    handle: GeometryHandleId,
    min_m2: f64,
    max_m2: f64,
) -> Result<Vec<GeometryHandleId>, QueryError> {
    let faces = kernel.extract_faces(handle)?;
    filter_by_value(
        kernel,
        &faces,
        "faces_by_area",
        GeometryQuery::SurfaceArea,
        |id, value| {
            let area = expect_real("SurfaceArea", id, value)?;
            Ok(area >= min_m2 && area <= max_m2)
        },
    )
}

/// Return the subset of `extract_faces(parent_handle)` whose surface area lies
/// in `[min_m2, max_m2]` (inclusive on both ends), while also recording a
/// [`FeatureTag`] for every extracted face in `table`.
///
/// Mirrors [`faces_by_area`]'s logic exactly — same filter predicate, same
/// canonical sub-shape order — while additionally populating `table` with
/// per-face tags derived from `parent_tag`: each face at position `i` in the
/// extracted list gets a tag whose `source_span` and `step_kind` are copied
/// from `parent_tag` and whose `sub_index` is `i as u32` (the parent's
/// `sub_index` is **not** inherited — each child position determines its own
/// `sub_index`).
///
/// Tags are recorded for **all** extracted faces (before the area-filter
/// runs), so callers can query the table even for faces that do not pass the
/// filter. This matches the recording contract established by
/// [`edges_at_height_with_tags`] (task 2323 / task 2329).
///
/// Downstream consumers can pass the populated table to
/// [`resolve_unique_by_tag`] to pin a specific sub-shape across topology
/// changes, receiving [`DiagnosticCode::TopologyTagStale`] if the
/// unique-tag invariant is later violated.
///
/// # Errors
///
/// Same as [`faces_by_area`].
pub fn faces_by_area_with_tags<K: GeometryKernel + ?Sized>(
    kernel: &mut K,
    table: &mut FeatureTagTable,
    parent_handle: GeometryHandleId,
    parent_tag: FeatureTag,
    min_m2: f64,
    max_m2: f64,
) -> Result<Vec<GeometryHandleId>, QueryError> {
    let faces = kernel.extract_faces(parent_handle)?;
    record_subshape_tags(table, &faces, parent_tag);
    filter_by_value(
        kernel,
        &faces,
        "faces_by_area_with_tags",
        GeometryQuery::SurfaceArea,
        |id, value| {
            let area = expect_real("SurfaceArea", id, value)?;
            Ok(area >= min_m2 && area <= max_m2)
        },
    )
}

/// Parse a `Value::String` that the kernel formatted as JSON
/// `{"x":...,"y":...,"z":...}` (the Centroid / EdgeTangent / FaceNormal
/// encoding) into an `[f64; 3]`.
///
/// Returns `QueryError::QueryFailed` on any deviation from the expected
/// shape (non-string Value, malformed JSON, missing numeric fields).
pub(crate) fn parse_xyz_value(value: &Value, query_label: &str) -> Result<[f64; 3], QueryError> {
    let s = match value {
        Value::String(s) => s,
        other => {
            return Err(QueryError::QueryFailed(format!(
                "{query_label} returned non-string value: {other:?}"
            )));
        }
    };
    // Minimal JSON parse — the kernel always emits exactly the
    // `{"x":..,"y":..,"z":..}` shape, so a strict regex-free scan is
    // sufficient and avoids pulling in serde_json as a non-dev dependency.
    let parsed = parse_xyz_json(s).ok_or_else(|| {
        QueryError::QueryFailed(format!(
            "{query_label} returned malformed JSON Point3: {s:?}"
        ))
    })?;
    Ok(parsed)
}

/// Parse `{"x":NUMBER,"y":NUMBER,"z":NUMBER}` (with arbitrary whitespace)
/// into `[x, y, z]`. Returns `None` on any structural deviation. Used
/// internally by the filter selectors to read the kernel's Point3 JSON
/// without taking on a serde_json dependency.
pub(crate) fn parse_xyz_json(s: &str) -> Option<[f64; 3]> {
    let mut x: Option<f64> = None;
    let mut y: Option<f64> = None;
    let mut z: Option<f64> = None;
    parse_flat_number_object(s, |key, num| match key {
        "x" => {
            x = Some(num);
            true
        }
        "y" => {
            y = Some(num);
            true
        }
        "z" => {
            z = Some(num);
            true
        }
        _ => false,
    })?;
    Some([x?, y?, z?])
}

/// Walk a flat JSON object of the form
/// `{"key1":NUMBER,"key2":NUMBER,...}` (arbitrary whitespace, no nested
/// objects, no string values), invoking `on_pair(key, num)` for every
/// entry. The closure returns `false` to reject an unknown / unexpected
/// key, in which case the helper short-circuits and returns `None`.
///
/// Returns `None` on any structural deviation: missing outer braces,
/// missing colon between key and value, or a value that fails to parse
/// as `f64`. The kernel never emits nested objects or string values for
/// the payloads consumed here, so a naive comma-split is safe.
pub(crate) fn parse_flat_number_object<F>(s: &str, mut on_pair: F) -> Option<()>
where
    F: FnMut(&str, f64) -> bool,
{
    // Strip leading/trailing whitespace and outer braces, then split on
    // commas. The kernel-emitted format never contains nested objects or
    // strings, so this naive split is safe.
    let inner = s.trim().strip_prefix('{')?.strip_suffix('}')?;
    for part in inner.split(',') {
        let mut kv = part.splitn(2, ':');
        let key = kv.next()?.trim().trim_matches('"');
        let val = kv.next()?.trim();
        let num: f64 = val.parse().ok()?;
        if !on_pair(key, num) {
            return None;
        }
    }
    Some(())
}

/// Normalize a 3-vector. Returns `None` (caller should treat as a
/// degenerate / unfilterable face/edge) if the magnitude is below
/// `f64::EPSILON` or non-finite.
///
/// The `!mag.is_finite()` guard rejects NaN and ±∞ inputs before they
/// poison downstream `acos` / `clamp` arithmetic — `mag < f64::EPSILON`
/// alone does not catch NaN (any comparison with NaN is false).
pub(crate) fn normalize3(v: [f64; 3]) -> Option<[f64; 3]> {
    let mag = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if !mag.is_finite() || mag < f64::EPSILON {
        return None;
    }
    Some([v[0] / mag, v[1] / mag, v[2] / mag])
}

/// Dot product of two 3-vectors.
pub(crate) fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Validate that `angular_tol_rad` is finite and in the inclusive range
/// `[0, max]`, returning `Err(QueryError::QueryFailed)` on violation.
///
/// `fn_name` names the calling selector in the diagnostic.  `max_label` is
/// the human-readable form of `max` (e.g. `"π/2"` or `"π"`) so the error
/// message uses Unicode rather than a raw float literal.
///
/// Used by all five angular-tolerance selectors to guard NaN, ±∞, and
/// out-of-range values **before** any kernel touch or tag-table mutation.
pub(crate) fn validate_angular_tol(
    fn_name: &'static str,
    tol: f64,
    max: f64,
    max_label: &'static str,
) -> Result<(), QueryError> {
    // `!tol.is_finite()` is redundant (the range check already rejects NaN and ±∞)
    // but kept for readability — it makes the NaN/infinity guard explicit at a glance.
    if !tol.is_finite() || !(0.0..=max).contains(&tol) {
        return Err(QueryError::QueryFailed(format!(
            "{fn_name}: angular_tol_rad must be finite and in [0, {max_label}] (got {tol})"
        )));
    }
    Ok(())
}

/// Return the subset of `extract_faces(handle)` whose surface normal at
/// the face's centroid is within `angular_tol_rad` of the `target`
/// direction.
///
/// The face normal is queried via `GeometryQuery::FaceNormal`, parsed
/// from the kernel's `{"x":..,"y":..,"z":..}` JSON encoding, normalized,
/// and compared to the (also normalized) `target` via
/// `acos(clamp(dot, -1, 1))`. Faces whose normal differs from `target`
/// by more than `angular_tol_rad` are dropped.
///
/// Direction matters: a face whose normal is anti-parallel to `target`
/// (180° off) is **not** accepted. This is intentional — `faces_by_normal`
/// distinguishes "front" vs "back" of a sheet, e.g. selecting only the
/// outward `+z` face of a closed solid (kernels return the topologically-
/// oriented outward normal for solid-shell faces). For orientation-
/// agnostic edge filtering, see `edges_parallel_to`.
///
/// # Errors
///
/// - Returns `QueryError::QueryFailed` if `target` is the zero vector or
///   contains a non-finite component (an undefined direction).
/// - Returns `QueryError::QueryFailed` if `angular_tol_rad` is not finite or
///   outside the valid range `[0, π]`. The predicate uses `acos`, whose output
///   is naturally bounded in `[0, π]`, making any value outside that range
///   meaningless. Negative tol silently rejects everything; tol > π silently
///   accepts everything — both are incorrect semantics.
/// - Propagates any error from `extract_faces`.
/// - Propagates any error from a per-face `FaceNormal` query.
/// - Returns `QueryError::QueryFailed` on a malformed `FaceNormal`
///   payload (non-string, non-JSON, missing fields) or on a degenerate
///   face whose normal magnitude is below `f64::EPSILON`.
pub fn faces_by_normal<K: GeometryKernel + ?Sized>(
    kernel: &mut K,
    handle: GeometryHandleId,
    target: [f64; 3],
    angular_tol_rad: f64,
) -> Result<Vec<GeometryHandleId>, QueryError> {
    validate_angular_tol(
        "faces_by_normal",
        angular_tol_rad,
        std::f64::consts::PI,
        "π",
    )?;
    let target = normalize3(target).ok_or_else(|| {
        QueryError::QueryFailed(
            "faces_by_normal: target direction must be non-zero and finite".into(),
        )
    })?;
    let faces = kernel.extract_faces(handle)?;
    filter_by_value(
        kernel,
        &faces,
        "faces_by_normal",
        GeometryQuery::FaceNormal,
        |id, value| {
            let raw = parse_xyz_value(value, "FaceNormal")?;
            let normal = normalize3(raw).ok_or_else(|| {
                QueryError::QueryFailed(format!(
                    "FaceNormal({:?}) returned a degenerate (near-zero) normal",
                    id
                ))
            })?;
            let cos = dot3(normal, target).clamp(-1.0, 1.0);
            let angle = cos.acos();
            Ok(angle <= angular_tol_rad)
        },
    )
}

// ── DFM overhang / draft selectors (task 4406 α) ───────────────────────────

/// Maximum face-normal angle from the horizontal plane for a face to be
/// classified as a "wall" (as opposed to a floor/ceiling) during draft-angle
/// analysis.  Faces with `|n·pull_dir| < WALL_WINDOW_RAD.sin()` are
/// wall-window candidates; faces outside that range are horizontal and
/// excluded from the draft calculation.
///
/// **§9-Q1 contract constant** — pinned to 45° by the fixture
/// `draft_wall_window_is_45_degrees`.  Changing this value here changes
/// the selector's semantics; update the fixture accordingly.
pub(crate) const WALL_WINDOW_RAD: f64 = std::f64::consts::FRAC_PI_4;

/// Tessellation tolerance (metres) shared by both the overhang and draft DFM
/// selectors when sampling curved faces for their conservative scalar bounds.
/// This is a *sampling density* parameter, **not** a numeric-accuracy floor —
/// the bound is conservative at any positive tolerance value (finer sampling
/// can only worsen, never improve, the reported worst-dip / min-draft).
///
/// **Per-call cost note:** `tessellate` is called once per selector invocation
/// regardless of whether the BRep contains curved faces.  For purely planar
/// solids this is a no-op at the mesh level (the fold finds no normals to
/// process), but the kernel call itself is not skipped.  If the call overhead
/// becomes measurable, a future optimisation can query face-type metadata first
/// and skip tessellation for all-planar shapes.
///
/// **Kernel contract:** `Mesh.normals`, when `Some`, must contain
/// *per-facet outward normals* — one outward normal per triangle vertex,
/// consistent in orientation with `FaceNormal` (outward = away from the solid
/// interior).  Smoothed or averaged vertex normals violate the conservative-
/// bound invariant: averaged normals at wall/floor seams can fall into the
/// wall window when neither adjacent face is a true wall, producing a
/// misleading `signed_min_draft` or a spurious `has_undercut`.  If a kernel
/// emits smoothed normals, derive facet normals from `vertices`+`indices`
/// via cross-product instead.
const DFM_TESS_TOLERANCE: f64 = 1e-3;

/// Iterate the per-facet normals in `mesh`, normalising each, and call `f`
/// with `dot3(n_facet, dir)` for every non-degenerate facet normal.
///
/// This is the shared tessellate-fold kernel for both
/// [`unsupported_overhang_faces`] and [`min_draft_angle`]: both functions
/// read `Mesh.normals` in the same `chunks(3) → f32→f64 cast → normalize3`
/// pattern but apply different reduction closures (max-dip vs min-draft/
/// undercut).  Extracting the loop here removes the duplication so the
/// orientation convention and conservative-bound logic live in one place.
///
/// The `Mesh.normals` kernel contract (per-facet outward, unsmoothed) is
/// stated on [`DFM_TESS_TOLERANCE`].  When `mesh.normals` is `None` or
/// empty, `f` is never called (no-op).  Degenerate (near-zero) facet
/// normals are silently skipped.
fn fold_mesh_facet_dots(mesh: &reify_ir::Mesh, dir: [f64; 3], mut f: impl FnMut(f64)) {
    let Some(ns) = &mesh.normals else { return };
    for chunk in ns.chunks(3) {
        if chunk.len() == 3 {
            let nf = [chunk[0] as f64, chunk[1] as f64, chunk[2] as f64];
            if let Some(nf_unit) = normalize3(nf) {
                f(dot3(nf_unit, dir));
            }
        }
    }
}

/// Return the subset of faces whose outward normal is "unsupported" in the
/// additive-manufacturing sense, together with the worst (largest) overhang
/// dip angle over **all** faces.
///
/// A face is *unsupported* iff its outward normal satisfies
/// `n · build_dir < −sin(max_overhang_angle)`, i.e. the face points more
/// than `max_overhang_angle` below the horizontal build plane.
///
/// The per-face *dip* is defined as
/// `asin(clamp(−n · build_dir, −1, 1)) ∈ [−π/2, π/2]`.  Positive dip means
/// the face points downward (overhang); negative dip means it points upward
/// (self-supporting).  `worst_dip` is the maximum over **all** BRep faces
/// and tessellated facet normals, seeded at `f64::NEG_INFINITY`.  The seeded
/// value is returned only when both the BRep face list and the tessellation
/// yield no normals; closed solids always have faces, so this is a
/// theoretical edge case in practice.
///
/// For curved faces the scalar `worst_dip` is additionally refined by
/// per-vertex normals from `kernel.tessellate` (conservative bound — finer
/// sampling only worsens the reported value).  The unsupported **face set**
/// comes solely from per-BRep-face `FaceNormal` queries (Mesh carries no
/// per-face attribution — a documented v1 limitation).
///
/// All angles are SI radians, consistent with the rest of this file.
///
/// # Errors
///
/// - Returns `QueryError::QueryFailed` if `build_dir` is the zero vector or
///   contains a non-finite component.
/// - Returns `QueryError::QueryFailed` if `max_overhang_angle` is not finite
///   or outside `[0, π/2]`.
/// - Propagates any error from `extract_faces` or per-face `FaceNormal`.
/// - Returns `QueryError::QueryFailed` on a malformed `FaceNormal` payload
///   or a degenerate (near-zero) face normal.
pub fn unsupported_overhang_faces<K: GeometryKernel + ?Sized>(
    kernel: &mut K,
    handle: GeometryHandleId,
    build_dir: [f64; 3],
    max_overhang_angle: f64,
) -> Result<(Vec<GeometryHandleId>, f64), QueryError> {
    // Validate angle range [0, π/2] before any kernel touch.
    validate_angular_tol(
        "unsupported_overhang_faces",
        max_overhang_angle,
        std::f64::consts::FRAC_PI_2,
        "π/2",
    )?;
    // Normalize build_dir; reject zero / non-finite vectors.
    let b = normalize3(build_dir).ok_or_else(|| {
        QueryError::QueryFailed(
            "unsupported_overhang_faces: build_dir must be non-zero and finite".into(),
        )
    })?;

    let faces = kernel.extract_faces(handle)?;
    let values = query_per_subshape(
        kernel,
        &faces,
        "unsupported_overhang_faces",
        GeometryQuery::FaceNormal,
    )?;

    let threshold = -max_overhang_angle.sin();
    let mut unsupported = Vec::new();
    let mut worst_dip = f64::NEG_INFINITY;

    for (id, value) in faces.iter().zip(values.iter()) {
        let raw = parse_xyz_value(value, "FaceNormal")?;
        let n = normalize3(raw).ok_or_else(|| {
            QueryError::QueryFailed(format!(
                "FaceNormal({:?}) returned a degenerate (near-zero) normal",
                id
            ))
        })?;
        let d = dot3(n, b);
        if d < threshold {
            unsupported.push(*id);
        }
        let dip = (-d).clamp(-1.0, 1.0).asin();
        if dip > worst_dip {
            worst_dip = dip;
        }
    }

    // Conservative tessellate fold: refine worst_dip from per-facet normals.
    // A tessellate error or absent normals is a no-op (per-face result stands).
    // The unsupported FACE SET is not updated here — Mesh has no per-face
    // attribution (documented v1 limitation; per-region overhang maps are
    // out of scope per PRD §5).
    // Kernel contract: Mesh.normals must be per-facet outward unsmoothed normals
    // (see DFM_TESS_TOLERANCE doc).
    if let Ok(mesh) = kernel.tessellate(handle, DFM_TESS_TOLERANCE) {
        fold_mesh_facet_dots(&mesh, b, |d| {
            let dip = (-d).clamp(-1.0, 1.0).asin();
            if dip > worst_dip {
                worst_dip = dip;
            }
        });
    }

    Ok((unsupported, worst_dip))
}

/// Return the minimum signed draft angle over the wall-window faces of
/// `handle`, together with a flag indicating whether any wall face is
/// re-entrant (undercut).
///
/// *Wall-window* faces satisfy `|n · pull_dir| < sin(WALL_WINDOW_RAD)` where
/// [`WALL_WINDOW_RAD`] = π/4 (45°).  For each such face the signed draft
/// angle is
/// `δ = π/2 − acos(clamp(n · pull_dir, −1, 1)) ∈ (−π/2, π/2)`.
/// Positive δ means the face has positive draft (tapers away from the die);
/// negative δ means the face is re-entrant (undercut).
///
/// `signed_min_draft` is the minimum δ over all wall-window faces.  When no
/// wall-window faces exist (the part has only horizontal faces) the function
/// returns the sentinel `+π/2` — a wall-less part trivially satisfies any
/// draft requirement.
///
/// `has_undercut` is `true` iff any wall-window face (or facet, once the
/// tessellate fold is applied) has `n · pull_dir < 0`.
///
/// For curved faces the scalar `signed_min_draft` and `has_undercut` are
/// additionally refined by per-vertex normals from `kernel.tessellate`
/// (conservative bound — only lowers the reported min draft / sets undercut,
/// never improves it).
///
/// All angles are SI radians.
///
/// # Errors
///
/// - Returns `QueryError::QueryFailed` if `pull_dir` is the zero vector or
///   contains a non-finite component.
/// - Propagates any error from `extract_faces` or per-face `FaceNormal`.
/// - Returns `QueryError::QueryFailed` on a malformed `FaceNormal` payload
///   or a degenerate face normal.
pub fn min_draft_angle<K: GeometryKernel + ?Sized>(
    kernel: &mut K,
    handle: GeometryHandleId,
    pull_dir: [f64; 3],
) -> Result<(f64, bool), QueryError> {
    let p = normalize3(pull_dir).ok_or_else(|| {
        QueryError::QueryFailed(
            "min_draft_angle: pull_dir must be non-zero and finite".into(),
        )
    })?;

    let faces = kernel.extract_faces(handle)?;
    let values = query_per_subshape(
        kernel,
        &faces,
        "min_draft_angle",
        GeometryQuery::FaceNormal,
    )?;

    let wall_sin = WALL_WINDOW_RAD.sin(); // sin(π/4) ≈ 0.7071
    let mut min_draft = f64::INFINITY;
    let mut has_undercut = false;

    for (id, value) in faces.iter().zip(values.iter()) {
        let raw = parse_xyz_value(value, "FaceNormal")?;
        let n = normalize3(raw).ok_or_else(|| {
            QueryError::QueryFailed(format!(
                "FaceNormal({:?}) returned a degenerate (near-zero) normal",
                id
            ))
        })?;
        let d = dot3(n, p);
        if d.abs() < wall_sin {
            // Wall-window face: compute signed draft angle.
            let delta = std::f64::consts::FRAC_PI_2 - d.clamp(-1.0, 1.0).acos();
            if delta < min_draft {
                min_draft = delta;
            }
            if d < 0.0 {
                has_undercut = true;
            }
        }
    }

    // Conservative tessellate fold: lower min_draft / set undercut flag from
    // per-facet normals. A tessellate error or absent normals is a no-op.
    // Kernel contract: Mesh.normals must be per-facet outward unsmoothed normals
    // (see DFM_TESS_TOLERANCE doc).
    if let Ok(mesh) = kernel.tessellate(handle, DFM_TESS_TOLERANCE) {
        fold_mesh_facet_dots(&mesh, p, |d| {
            if d.abs() < wall_sin {
                let delta = std::f64::consts::FRAC_PI_2 - d.clamp(-1.0, 1.0).acos();
                if delta < min_draft {
                    min_draft = delta;
                }
                if d < 0.0 {
                    has_undercut = true;
                }
            }
        });
    }

    // No wall-window face seen → return +π/2 sentinel (trivially conforms).
    let signed_min_draft = if min_draft.is_finite() {
        min_draft
    } else {
        std::f64::consts::FRAC_PI_2
    };

    Ok((signed_min_draft, has_undercut))
}

/// Return the subset of `extract_edges(handle)` whose midpoint tangent is
/// (anti-)parallel to `axis` within `angular_tol_rad`.
///
/// The tangent is queried via `GeometryQuery::EdgeTangent`, parsed from
/// the kernel's `{"x":..,"y":..,"z":..}` JSON encoding, and normalized.
/// Unlike `faces_by_normal`, **sign of the tangent does not matter** —
/// the kernel may return either direction along an edge, so an edge is
/// retained if its tangent satisfies *either* `angle(t, axis) <= tol`
/// *or* `angle(-t, axis) <= tol`.
///
/// Equivalently: an edge is retained if the absolute cosine
/// `|t · axis| >= cos(angular_tol_rad)`. This formulation avoids two
/// `acos` calls per edge and is well-conditioned at small tolerances.
///
/// # Errors
///
/// - Returns `QueryError::QueryFailed` if `axis` is the zero vector or
///   contains a non-finite component (an undefined direction).
/// - Returns `QueryError::QueryFailed` if `angular_tol_rad` is not finite or
///   outside the valid range `[0, π/2]`. Values beyond π/2 cause `cos` to go
///   negative, making the `|dot| >= cos(tol)` predicate trivially true for all
///   edges (silent over-acceptance). Only `[0, π/2]` has well-defined semantics.
/// - Propagates any error from `extract_edges`.
/// - Propagates any error from a per-edge `EdgeTangent` query.
/// - Returns `QueryError::QueryFailed` on a malformed tangent payload
///   or a degenerate (near-zero magnitude) tangent.
pub fn edges_parallel_to<K: GeometryKernel + ?Sized>(
    kernel: &mut K,
    handle: GeometryHandleId,
    axis: [f64; 3],
    angular_tol_rad: f64,
) -> Result<Vec<GeometryHandleId>, QueryError> {
    validate_angular_tol(
        "edges_parallel_to",
        angular_tol_rad,
        std::f64::consts::FRAC_PI_2,
        "π/2",
    )?;
    let axis = normalize3(axis).ok_or_else(|| {
        QueryError::QueryFailed(
            "edges_parallel_to: axis direction must be non-zero and finite".into(),
        )
    })?;
    // Threshold on |dot|: edges accepted iff |t · axis| >= cos(tol).
    // Note: cos is monotone-decreasing on [0, π], so angle <= tol is
    // equivalent to cos(angle) >= cos(tol); for the sign-tolerant variant
    // we use |cos|.
    let cos_tol = angular_tol_rad.cos();
    let edges = kernel.extract_edges(handle)?;
    filter_by_value(
        kernel,
        &edges,
        "edges_parallel_to",
        GeometryQuery::EdgeTangent,
        |id, value| {
            let raw = parse_xyz_value(value, "EdgeTangent")?;
            let tan = normalize3(raw).ok_or_else(|| {
                QueryError::QueryFailed(format!(
                    "EdgeTangent({:?}) returned a degenerate (near-zero) tangent",
                    id
                ))
            })?;
            Ok(dot3(tan, axis).abs() >= cos_tol)
        },
    )
}

/// Return the subset of `extract_edges(parent_handle)` whose midpoint tangent
/// is (anti-)parallel to `axis` within `angular_tol_rad`, while also recording
/// a [`FeatureTag`] for every extracted edge in `table`.
///
/// Mirrors [`edges_parallel_to`]'s logic exactly — same sign-tolerant predicate
/// (`|t · axis| >= cos(angular_tol_rad)`), same canonical sub-shape order —
/// while additionally populating `table` with per-edge tags derived from
/// `parent_tag`: each edge at position `i` in the extracted list gets a tag
/// whose `source_span` and `step_kind` are copied from `parent_tag` and whose
/// `sub_index` is `i as u32`.
///
/// **Both tolerance and axis are validated before extraction:** if
/// `angular_tol_rad` is out of range or non-finite, or if `axis` is the zero
/// vector or contains a non-finite component, the function returns a
/// `QueryError::QueryFailed` immediately, before calling `extract_edges` or
/// touching `table`. This matches the baseline's "fail before kernel touch"
/// contract.
///
/// Tags are recorded for **all** extracted edges (before the axis-filter
/// runs), so callers can query the table even for edges that do not pass the
/// filter. This matches the recording contract established by
/// [`edges_at_height_with_tags`] (task 2323 / task 2329).
///
/// Downstream consumers can pass the populated table to
/// [`resolve_unique_by_tag`] to pin a specific sub-shape across topology
/// changes, receiving [`DiagnosticCode::TopologyTagStale`] if the
/// unique-tag invariant is later violated.
///
/// # Errors
///
/// - Returns `QueryError::QueryFailed` if `angular_tol_rad` is not finite or
///   outside the valid range `[0, π/2]`. Fires before any kernel touch or
///   table mutation.
/// - Returns `QueryError::QueryFailed` if `axis` is the zero vector or
///   contains a non-finite component. Fires before any kernel touch or table
///   mutation.
/// - Otherwise same as [`edges_parallel_to`].
pub fn edges_parallel_to_with_tags<K: GeometryKernel + ?Sized>(
    kernel: &mut K,
    table: &mut FeatureTagTable,
    parent_handle: GeometryHandleId,
    parent_tag: FeatureTag,
    axis: [f64; 3],
    angular_tol_rad: f64,
) -> Result<Vec<GeometryHandleId>, QueryError> {
    // Tolerance validation is FIRST — before axis normalization, extract_edges,
    // and table mutation. "Fail before kernel touch" contract pinned by
    // edges_parallel_to_with_tags_*_errors_before_table_mutation tests.
    validate_angular_tol(
        "edges_parallel_to_with_tags",
        angular_tol_rad,
        std::f64::consts::FRAC_PI_2,
        "π/2",
    )?;
    let axis = normalize3(axis).ok_or_else(|| {
        QueryError::QueryFailed(
            "edges_parallel_to_with_tags: axis direction must be non-zero and finite".into(),
        )
    })?;
    let cos_tol = angular_tol_rad.cos();
    let edges = kernel.extract_edges(parent_handle)?;
    record_subshape_tags(table, &edges, parent_tag);
    filter_by_value(
        kernel,
        &edges,
        "edges_parallel_to_with_tags",
        GeometryQuery::EdgeTangent,
        |id, value| {
            let raw = parse_xyz_value(value, "EdgeTangent")?;
            let tan = normalize3(raw).ok_or_else(|| {
                QueryError::QueryFailed(format!(
                    "EdgeTangent({:?}) returned a degenerate (near-zero) tangent",
                    id
                ))
            })?;
            Ok(dot3(tan, axis).abs() >= cos_tol)
        },
    )
}

/// Return the subset of `extract_edges(handle)` that lie entirely within
/// `tol_m` (metres) of the horizontal plane `z = z_m`.
///
/// For each edge the bounding box is queried via
/// `GeometryQuery::BoundingBox`, parsed for `zmin` / `zmax`, and the
/// edge is retained only if **both** extents are within tolerance:
/// `(zmin - z_m).abs() <= tol_m && (zmax - z_m).abs() <= tol_m`. This
/// accepts horizontal edges lying flat on the plane and rejects vertical
/// edges that merely pass through it.
///
/// All length parameters are in metres.
///
/// # Errors
///
/// - Propagates any error from `extract_edges` (e.g. `InvalidHandle` if
///   `handle` is not registered with the kernel).
/// - Propagates any error from a per-edge `BoundingBox` query.
/// - Returns `QueryError::QueryFailed` on a malformed BoundingBox
///   payload (non-string, non-JSON, missing `zmin` / `zmax`).
pub fn edges_at_height<K: GeometryKernel + ?Sized>(
    kernel: &mut K,
    handle: GeometryHandleId,
    z_m: f64,
    tol_m: f64,
) -> Result<Vec<GeometryHandleId>, QueryError> {
    let edges = kernel.extract_edges(handle)?;
    filter_by_value(
        kernel,
        &edges,
        "edges_at_height",
        GeometryQuery::BoundingBox,
        |_id, value| {
            let (zmin, zmax) = parse_bbox_z_extents(value)?;
            Ok((zmin - z_m).abs() <= tol_m && (zmax - z_m).abs() <= tol_m)
        },
    )
}

/// Return the subset of `extract_edges(parent_handle)` that lie entirely within
/// `tol_m` (metres) of the horizontal plane `z = z_m`, while also recording a
/// [`FeatureTag`] for every extracted edge in `table`.
///
/// This is a proof-of-concept variant of [`edges_at_height`] that demonstrates
/// the feature-tag runtime table (task 2323). It mirrors `edges_at_height`'s
/// logic exactly — same filter predicate, same canonical sub-shape order — while
/// additionally populating `table` with per-edge tags derived from `parent_tag`:
/// each edge at position `i` in the extracted list gets a tag whose
/// `source_span` and `step_kind` are copied from `parent_tag` and whose
/// `sub_index` is `i as u32`.
///
/// Tags are recorded for **all** extracted edges (before the z-filter runs),
/// so callers can query the table even for edges that do not pass the filter.
///
/// # Errors
///
/// Same as [`edges_at_height`].
pub fn edges_at_height_with_tags<K: GeometryKernel + ?Sized>(
    kernel: &mut K,
    table: &mut FeatureTagTable,
    parent_handle: GeometryHandleId,
    parent_tag: FeatureTag,
    z_m: f64,
    tol_m: f64,
) -> Result<Vec<GeometryHandleId>, QueryError> {
    let edges = kernel.extract_edges(parent_handle)?;
    record_subshape_tags(table, &edges, parent_tag);
    filter_by_value(
        kernel,
        &edges,
        "edges_at_height_with_tags",
        GeometryQuery::BoundingBox,
        |_id, value| {
            let (zmin, zmax) = parse_bbox_z_extents(value)?;
            Ok((zmin - z_m).abs() <= tol_m && (zmax - z_m).abs() <= tol_m)
        },
    )
}

/// Resolve a `FeatureTag` to a unique candidate geometry handle.
///
/// Filters `candidates` to those whose recorded tag in `table` equals `target`
/// (full `FeatureTag` equality via the `PartialEq` derive). Returns `Some(handle)`
/// iff exactly one match is found.
///
/// If zero or more than one candidates match, returns `None` and pushes a
/// [`DiagnosticCode::TopologyTagStale`] warning onto `diagnostics` with:
/// - a primary label at `selector_span` (`"selector call"`), and
/// - a secondary label at `target.source_span` (`"feature originally produced here"`).
///
/// The match count is embedded in the message so callers can distinguish the
/// zero-match (sub-shape lost) from the multi-match (topology split) case.
///
/// # Scope
/// This is a pure building-block helper: it does not call into the geometry kernel
/// and does not require any `&mut dyn GeometryKernel` reference. Callers are
/// expected to have already extracted the candidate handles (via
/// `kernel.extract_edges` / `kernel.extract_faces`) and populated the table
/// (via `edges_at_height_with_tags` or equivalent) before calling this resolver.
///
/// # Preconditions
/// Callers SHOULD pass a deduplicated `candidates` slice (the OCCT-backed
/// kernel extractors guarantee this via `TopoDS_Shape::IsSame`). As a
/// defense-in-depth measure, the resolver internally deduplicates via a
/// `HashSet<GeometryHandleId>` so that accidental duplicates from a
/// misbehaving extractor cannot inflate the match count or produce a spurious
/// `W_TOPOLOGY_TAG_STALE` diagnostic.
pub fn resolve_unique_by_tag(
    table: &FeatureTagTable,
    candidates: &[GeometryHandleId],
    target: FeatureTag,
    selector_span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<GeometryHandleId> {
    let mut seen: HashSet<GeometryHandleId> = HashSet::with_capacity(candidates.len());
    let mut found: Option<GeometryHandleId> = None;
    let mut n: usize = 0;
    for &id in candidates {
        if seen.insert(id) && table.lookup(id) == Some(&target) {
            n += 1;
            if n == 1 {
                found = Some(id);
            }
        }
    }
    match n {
        1 => found,
        n => {
            diagnostics.push(
                Diagnostic::warning(format!(
                    "feature-tag selector matched {n} sub-shapes (expected exactly 1; topology may have changed)"
                ))
                .with_code(DiagnosticCode::TopologyTagStale)
                .with_label(DiagnosticLabel::new(selector_span, "selector call"))
                .with_label(DiagnosticLabel::new(
                    target.source_span,
                    "feature originally produced here",
                )),
            );
            None
        }
    }
}

/// Parse a `Value::String` that the kernel formatted as JSON
/// `{"xmin":..,"ymin":..,"zmin":..,"xmax":..,"ymax":..,"zmax":..}` (the
/// BoundingBox encoding) and return `(zmin, zmax)`. The other extents
/// are ignored — `edges_at_height` only filters along z.
///
/// Returns `QueryError::QueryFailed` on any deviation from the expected
/// shape (non-string Value, malformed JSON, missing zmin/zmax fields).
pub(crate) fn parse_bbox_z_extents(value: &Value) -> Result<(f64, f64), QueryError> {
    let s = match value {
        Value::String(s) => s,
        other => {
            return Err(QueryError::QueryFailed(format!(
                "BoundingBox returned non-string value: {other:?}"
            )));
        }
    };
    parse_bbox_z_extents_json(s).ok_or_else(|| {
        QueryError::QueryFailed(format!(
            "BoundingBox returned malformed JSON payload: {s:?}"
        ))
    })
}

/// Parse `{"xmin":NUMBER,...,"zmax":NUMBER}` (with arbitrary whitespace)
/// for the `zmin` and `zmax` keys, ignoring the other axis extents.
/// Returns `None` on any structural deviation.
pub(crate) fn parse_bbox_z_extents_json(s: &str) -> Option<(f64, f64)> {
    let mut zmin: Option<f64> = None;
    let mut zmax: Option<f64> = None;
    parse_flat_number_object(s, |key, num| match key {
        "zmin" => {
            zmin = Some(num);
            true
        }
        "zmax" => {
            zmax = Some(num);
            true
        }
        // xmin/xmax/ymin/ymax are part of the well-formed payload but
        // not needed for this selector; tolerate them silently.
        "xmin" | "xmax" | "ymin" | "ymax" => true,
        _ => false,
    })?;
    Some((zmin?, zmax?))
}

/// Parse a `Value::String` BoundingBox payload (the kernel's
/// `{"xmin":..,"ymin":..,"zmin":..,"xmax":..,"ymax":..,"zmax":..}` JSON
/// encoding) and return `(min, max)` for the requested axis.
///
/// Generalises [`parse_bbox_z_extents`] to all three axes — the
/// `extremal_by_bbox` selector dispatches on `Axis::{X, Y, Z}` and reads
/// either the `*min` or `*max` extent depending on `ExtremalSense`.
///
/// Returns `QueryError::QueryFailed` on any deviation from the expected
/// shape (non-string `Value`, malformed JSON, missing fields for the
/// requested axis).
pub(crate) fn parse_bbox_axis_extents(value: &Value, axis: u8) -> Result<(f64, f64), QueryError> {
    let s = match value {
        Value::String(s) => s,
        other => {
            return Err(QueryError::QueryFailed(format!(
                "BoundingBox returned non-string value: {other:?}"
            )));
        }
    };
    parse_bbox_axis_extents_json(s, axis).ok_or_else(|| {
        QueryError::QueryFailed(format!(
            "BoundingBox returned malformed JSON payload: {s:?}"
        ))
    })
}

/// Parse `{"xmin":..,"ymin":..,"zmin":..,"xmax":..,"ymax":..,"zmax":..}`
/// for the requested axis (`b'x' | b'y' | b'z'`), returning `(min, max)`.
/// Returns `None` on structural deviation or unexpected `axis` byte.
pub(crate) fn parse_bbox_axis_extents_json(s: &str, axis: u8) -> Option<(f64, f64)> {
    let (min_key, max_key): (&str, &str) = match axis {
        b'x' => ("xmin", "xmax"),
        b'y' => ("ymin", "ymax"),
        b'z' => ("zmin", "zmax"),
        _ => return None,
    };
    let mut min_v: Option<f64> = None;
    let mut max_v: Option<f64> = None;
    parse_flat_number_object(s, |key, num| {
        if key == min_key {
            min_v = Some(num);
            true
        } else if key == max_key {
            max_v = Some(num);
            true
        } else if matches!(key, "xmin" | "xmax" | "ymin" | "ymax" | "zmin" | "zmax") {
            // Other-axis extents are part of the well-formed payload
            // but not needed for this caller; tolerate them silently.
            true
        } else {
            false
        }
    })?;
    Some((min_v?, max_v?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_ir::{
        ExportError, ExportFormat, GeometryError, GeometryHandle, GeometryOp, Mesh, TessError,
    };
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// In-test `GeometryKernel` that records `query` and `query_many`
    /// invocation counts so we can prove the migrated selectors batch
    /// their kernel reads via `query_many` (one call) instead of looping
    /// over per-element `query` (N calls).
    ///
    /// Configure with:
    ///   * `edges` / `faces`: handle ids returned by `extract_edges` /
    ///     `extract_faces`.
    ///   * `responses`: map from sub-shape handle id to the `Value` the
    ///     kernel should reply with for any query against that id
    ///     (regardless of query variant — every selector uses exactly
    ///     one `GeometryQuery` kind, so a single `Value` per id is
    ///     unambiguous).
    ///
    /// `query_many`'s override looks up `Value`s directly from
    /// `responses` rather than calling `self.query()` per element —
    /// this lets each test assert `query_calls == 0` after a successful
    /// batched selector run, which would be impossible if the override
    /// fell back to per-element `query`.
    struct CountingKernel {
        query_calls: AtomicUsize,
        query_many_calls: AtomicUsize,
        edges: Vec<GeometryHandleId>,
        faces: Vec<GeometryHandleId>,
        responses: HashMap<GeometryHandleId, Value>,
        /// Mesh returned by `tessellate`. Defaults to an empty mesh
        /// (no vertices, no indices, no normals) so existing per-face tests
        /// are unaffected — the curved conservative-bound fold is a no-op
        /// when `normals` is `None` or the mesh is empty.
        mesh: Mesh,
        /// When `true`, `tessellate` returns `Err(TessellationFailed)` instead
        /// of `Ok(mesh)`. Use in tests that verify the tessellate-error-is-no-op
        /// path.
        fail_tessellate: bool,
    }

    impl CountingKernel {
        fn new() -> Self {
            CountingKernel {
                query_calls: AtomicUsize::new(0),
                query_many_calls: AtomicUsize::new(0),
                edges: Vec::new(),
                faces: Vec::new(),
                responses: HashMap::new(),
                mesh: Mesh { vertices: vec![], indices: vec![], normals: None },
                fail_tessellate: false,
            }
        }

        fn with_edges(mut self, edges: Vec<GeometryHandleId>) -> Self {
            self.edges = edges;
            self
        }

        fn with_faces(mut self, faces: Vec<GeometryHandleId>) -> Self {
            self.faces = faces;
            self
        }

        fn with_response(mut self, id: GeometryHandleId, value: Value) -> Self {
            self.responses.insert(id, value);
            self
        }

        /// Stage a `Mesh` to be returned by `tessellate`. Use in
        /// curved-conservative-bound tests (step-5, step-7) to inject vertex
        /// normals without touching the BRep-face response map.
        fn with_mesh(mut self, mesh: Mesh) -> Self {
            self.mesh = mesh;
            self
        }

        /// Make `tessellate` return `Err(TessellationFailed)`. Use to verify
        /// that a tessellate failure is a no-op for both DFM selectors.
        fn with_fail_tessellate(mut self) -> Self {
            self.fail_tessellate = true;
            self
        }

        fn query_calls(&self) -> usize {
            self.query_calls.load(Ordering::SeqCst)
        }

        fn query_many_calls(&self) -> usize {
            self.query_many_calls.load(Ordering::SeqCst)
        }

        /// Look up the staged response for `query`, returning a clone or an
        /// `InvalidHandle` error if no response was staged for the queried
        /// handle id. Centralizes the dispatch shared by `query` and
        /// `query_many`.
        fn lookup(&self, query: &GeometryQuery) -> Result<Value, QueryError> {
            let id = match query {
                GeometryQuery::EdgeLength(id)
                | GeometryQuery::EdgeTangent(id)
                | GeometryQuery::FaceNormal(id)
                | GeometryQuery::SurfaceArea(id)
                | GeometryQuery::BoundingBox(id) => *id,
                other => {
                    return Err(QueryError::QueryFailed(format!(
                        "CountingKernel: unsupported query variant {:?}",
                        other
                    )));
                }
            };
            self.responses
                .get(&id)
                .cloned()
                .ok_or(QueryError::InvalidHandle(id))
        }
    }

    impl GeometryKernel for CountingKernel {
        fn execute(&mut self, _op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
            unimplemented!("CountingKernel does not implement execute")
        }

        fn query(&self, query: &GeometryQuery) -> Result<Value, QueryError> {
            self.query_calls.fetch_add(1, Ordering::SeqCst);
            self.lookup(query)
        }

        fn query_many(&self, queries: &[GeometryQuery]) -> Result<Vec<Value>, QueryError> {
            self.query_many_calls.fetch_add(1, Ordering::SeqCst);
            // Look up directly from the staged responses so per-element
            // `query` is *not* called — the assertion `query_calls == 0`
            // proves the migrated selector relies on the batched path.
            queries.iter().map(|q| self.lookup(q)).collect()
        }

        fn export(
            &self,
            _handle: GeometryHandleId,
            _format: ExportFormat,
            _writer: &mut dyn std::io::Write,
        ) -> Result<(), ExportError> {
            unimplemented!("CountingKernel does not implement export")
        }

        fn tessellate(
            &self,
            _handle: GeometryHandleId,
            _tolerance: f64,
        ) -> Result<Mesh, TessError> {
            if self.fail_tessellate {
                return Err(TessError::TessellationFailed(
                    "CountingKernel: tessellate stubbed to fail".into(),
                ));
            }
            Ok(self.mesh.clone())
        }

        fn extract_edges(
            &mut self,
            _handle: GeometryHandleId,
        ) -> Result<Vec<GeometryHandleId>, QueryError> {
            Ok(self.edges.clone())
        }

        fn extract_faces(
            &mut self,
            _handle: GeometryHandleId,
        ) -> Result<Vec<GeometryHandleId>, QueryError> {
            Ok(self.faces.clone())
        }
    }

    /// Compile-time witness: `CountingKernel` must satisfy the `Send + Sync`
    /// supertrait bound that `GeometryKernel` requires of its impls.
    const _: fn() = || {
        fn must_be_send_sync<T: Send + Sync>() {}
        must_be_send_sync::<CountingKernel>();
    };

    #[test]
    fn edges_by_length_uses_query_many_once() {
        // Three edges with lengths 5mm, 10mm, 15mm. The window
        // [8mm, 12mm] selects only the middle edge.
        let edge_ids = vec![
            GeometryHandleId(101),
            GeometryHandleId(102),
            GeometryHandleId(103),
        ];
        let mut kernel = CountingKernel::new()
            .with_edges(edge_ids.clone())
            .with_response(edge_ids[0], Value::Real(0.005))
            .with_response(edge_ids[1], Value::Real(0.010))
            .with_response(edge_ids[2], Value::Real(0.015));

        let source = GeometryHandleId(1);
        let result =
            edges_by_length(&mut kernel, source, 0.008, 0.012).expect("selector should succeed");

        assert_eq!(result, vec![edge_ids[1]], "expected only the 10mm edge");
        assert_eq!(
            kernel.query_many_calls(),
            1,
            "edges_by_length must call query_many exactly once"
        );
        assert_eq!(
            kernel.query_calls(),
            0,
            "edges_by_length must not loop over per-element query"
        );
    }

    #[test]
    fn faces_by_normal_uses_query_many_once() {
        // Three faces with normals (+Z, +X, -Z). Filter on +Z direction
        // with 1 deg tolerance: only the +Z face is accepted (anti-
        // parallel -Z is rejected per the documented contract).
        let face_ids = vec![
            GeometryHandleId(301),
            GeometryHandleId(302),
            GeometryHandleId(303),
        ];
        let mut kernel = CountingKernel::new()
            .with_faces(face_ids.clone())
            .with_response(
                face_ids[0],
                Value::String("{\"x\":0,\"y\":0,\"z\":1}".into()),
            )
            .with_response(
                face_ids[1],
                Value::String("{\"x\":1,\"y\":0,\"z\":0}".into()),
            )
            .with_response(
                face_ids[2],
                Value::String("{\"x\":0,\"y\":0,\"z\":-1}".into()),
            );

        let source = GeometryHandleId(1);
        let result = faces_by_normal(&mut kernel, source, [0.0, 0.0, 1.0], 1f64.to_radians())
            .expect("selector should succeed");

        assert_eq!(result, vec![face_ids[0]], "expected only the +Z face");
        assert_eq!(
            kernel.query_many_calls(),
            1,
            "faces_by_normal must call query_many exactly once"
        );
        assert_eq!(
            kernel.query_calls(),
            0,
            "faces_by_normal must not loop over per-element query"
        );
    }

    #[test]
    fn edges_parallel_to_uses_query_many_once() {
        // Three edges with tangents +X, -X, +Y. Filter on +X axis with
        // 1 deg tolerance: the +X and -X edges are both retained
        // (sign-tolerant predicate); the +Y edge is rejected.
        let edge_ids = vec![
            GeometryHandleId(401),
            GeometryHandleId(402),
            GeometryHandleId(403),
        ];
        let mut kernel = CountingKernel::new()
            .with_edges(edge_ids.clone())
            .with_response(
                edge_ids[0],
                Value::String("{\"x\":1,\"y\":0,\"z\":0}".into()),
            )
            .with_response(
                edge_ids[1],
                Value::String("{\"x\":-1,\"y\":0,\"z\":0}".into()),
            )
            .with_response(
                edge_ids[2],
                Value::String("{\"x\":0,\"y\":1,\"z\":0}".into()),
            );

        let source = GeometryHandleId(1);
        let result = edges_parallel_to(&mut kernel, source, [1.0, 0.0, 0.0], 1f64.to_radians())
            .expect("selector should succeed");

        assert_eq!(
            result,
            vec![edge_ids[0], edge_ids[1]],
            "expected both x-aligned edges (sign-tolerant)"
        );
        assert_eq!(
            kernel.query_many_calls(),
            1,
            "edges_parallel_to must call query_many exactly once"
        );
        assert_eq!(
            kernel.query_calls(),
            0,
            "edges_parallel_to must not loop over per-element query"
        );
    }

    #[test]
    fn faces_by_area_uses_query_many_once() {
        // Three faces with surface areas 200, 300, 600 in mm^2 (i.e.
        // 200e-6, 300e-6, 600e-6 m^2). The window [199e-6, 201e-6] m^2
        // selects only the first face.
        let face_ids = vec![
            GeometryHandleId(201),
            GeometryHandleId(202),
            GeometryHandleId(203),
        ];
        let mut kernel = CountingKernel::new()
            .with_faces(face_ids.clone())
            .with_response(face_ids[0], Value::Real(200e-6))
            .with_response(face_ids[1], Value::Real(300e-6))
            .with_response(face_ids[2], Value::Real(600e-6));

        let source = GeometryHandleId(1);
        let result =
            faces_by_area(&mut kernel, source, 199e-6, 201e-6).expect("selector should succeed");

        assert_eq!(
            result,
            vec![face_ids[0]],
            "expected only the 200e-6 m^2 face"
        );
        assert_eq!(
            kernel.query_many_calls(),
            1,
            "faces_by_area must call query_many exactly once"
        );
        assert_eq!(
            kernel.query_calls(),
            0,
            "faces_by_area must not loop over per-element query"
        );
    }

    #[test]
    fn edges_at_height_uses_query_many_once() {
        // Three edges:
        //   * edge_ids[0]: top edge — flat at z = +5mm (zmin == zmax == 5e-3).
        //   * edge_ids[1]: vertical edge spanning -5mm to +5mm.
        //   * edge_ids[2]: bottom edge — flat at z = -5mm.
        // Filter on z = +5mm with 1e-6 m tolerance: only the top edge is
        // retained (the vertical edge fails because zmin is 10mm away,
        // and the bottom edge fails on both extents).
        let edge_ids = vec![
            GeometryHandleId(501),
            GeometryHandleId(502),
            GeometryHandleId(503),
        ];
        let mut kernel = CountingKernel::new()
            .with_edges(edge_ids.clone())
            .with_response(
                edge_ids[0],
                Value::String(
                    "{\"xmin\":0,\"ymin\":0,\"zmin\":0.005,\"xmax\":0.01,\"ymax\":0,\"zmax\":0.005}"
                        .into(),
                ),
            )
            .with_response(
                edge_ids[1],
                Value::String(
                    "{\"xmin\":0,\"ymin\":0,\"zmin\":-0.005,\"xmax\":0,\"ymax\":0,\"zmax\":0.005}"
                        .into(),
                ),
            )
            .with_response(
                edge_ids[2],
                Value::String(
                    "{\"xmin\":0,\"ymin\":0,\"zmin\":-0.005,\"xmax\":0.01,\"ymax\":0,\"zmax\":-0.005}"
                        .into(),
                ),
            );

        let source = GeometryHandleId(1);
        let result =
            edges_at_height(&mut kernel, source, 5e-3, 1e-6).expect("selector should succeed");

        assert_eq!(result, vec![edge_ids[0]], "expected only the top edge");
        assert_eq!(
            kernel.query_many_calls(),
            1,
            "edges_at_height must call query_many exactly once"
        );
        assert_eq!(
            kernel.query_calls(),
            0,
            "edges_at_height must not loop over per-element query"
        );
    }

    /// In-test `GeometryKernel` whose `query_many` returns a fixed,
    /// canned reply regardless of input length. Used to prove selectors
    /// detect length mismatches (too-few or overlong) and surface
    /// `QueryError::QueryFailed` instead of silently truncating or
    /// ignoring extra results via `zip`.
    struct FixedReplyQueryManyKernel {
        edges: Vec<GeometryHandleId>,
        // The Vec returned from query_many regardless of input length.
        canned_reply: Vec<Value>,
    }

    impl GeometryKernel for FixedReplyQueryManyKernel {
        fn execute(&mut self, _op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
            unimplemented!("FixedReplyQueryManyKernel does not implement execute")
        }

        fn query(&self, _query: &GeometryQuery) -> Result<Value, QueryError> {
            unimplemented!("FixedReplyQueryManyKernel only supports query_many")
        }

        fn query_many(&self, _queries: &[GeometryQuery]) -> Result<Vec<Value>, QueryError> {
            Ok(self.canned_reply.clone())
        }

        fn export(
            &self,
            _handle: GeometryHandleId,
            _format: ExportFormat,
            _writer: &mut dyn std::io::Write,
        ) -> Result<(), ExportError> {
            unimplemented!()
        }

        fn tessellate(
            &self,
            _handle: GeometryHandleId,
            _tolerance: f64,
        ) -> Result<Mesh, TessError> {
            unimplemented!()
        }

        fn extract_edges(
            &mut self,
            _handle: GeometryHandleId,
        ) -> Result<Vec<GeometryHandleId>, QueryError> {
            Ok(self.edges.clone())
        }
    }

    #[test]
    fn edges_by_length_detects_query_many_overlong_reply() {
        // Three edges, kernel returns FOUR values (len(queries)+1): selector
        // must surface `QueryError::QueryFailed` instead of silently ignoring
        // the extra value. `FixedReplyQueryManyKernel` is staged with
        // len(queries)+1 values to exercise the overlong direction.
        let edge_ids = vec![
            GeometryHandleId(601),
            GeometryHandleId(602),
            GeometryHandleId(603),
        ];
        let mut kernel = FixedReplyQueryManyKernel {
            edges: edge_ids,
            canned_reply: vec![
                Value::Real(0.005),
                Value::Real(0.010),
                Value::Real(0.015),
                Value::Real(0.020), // one extra — overlong reply
            ],
        };
        let err = edges_by_length(&mut kernel, GeometryHandleId(1), 0.0, 1.0)
            .expect_err("selector must reject overlong query_many output");
        match err {
            QueryError::QueryFailed(msg) => {
                assert!(
                    msg.contains("edges_by_length") && msg.contains("length invariant"),
                    "expected length-invariant message, got {:?}",
                    msg
                );
            }
            other => panic!("expected QueryFailed, got {:?}", other),
        }
    }

    #[test]
    fn expect_real_error_message_names_query_label_and_id() {
        // Direct sanity test of the helper: a non-Real value yields a
        // QueryFailed whose message names the query label and id.
        let id = GeometryHandleId(701);
        let err = expect_real("EdgeLength", id, &Value::String("not a number".into()))
            .expect_err("expect_real must reject non-Real values");
        match err {
            QueryError::QueryFailed(msg) => {
                assert!(
                    msg.contains("EdgeLength") && msg.contains("701"),
                    "expected label + id in error, got {:?}",
                    msg
                );
            }
            other => panic!("expected QueryFailed, got {:?}", other),
        }
    }

    #[test]
    fn query_per_subshape_returns_values_aligned_with_ids_via_single_query_many() {
        // Three edge ids staged with distinct Real values. The helper must
        // return those values in input-id order, using a single query_many
        // call and zero per-element query calls.
        let edge_ids = vec![
            GeometryHandleId(801),
            GeometryHandleId(802),
            GeometryHandleId(803),
        ];
        let kernel = CountingKernel::new()
            .with_edges(edge_ids.clone())
            .with_response(edge_ids[0], Value::Real(0.001))
            .with_response(edge_ids[1], Value::Real(0.002))
            .with_response(edge_ids[2], Value::Real(0.003));

        let values =
            query_per_subshape(&kernel, &edge_ids, "test_label", GeometryQuery::EdgeLength)
                .expect("query_per_subshape should succeed");

        assert_eq!(
            values,
            vec![Value::Real(0.001), Value::Real(0.002), Value::Real(0.003)],
            "returned values must be aligned with input ids in order"
        );
        assert_eq!(
            kernel.query_many_calls(),
            1,
            "query_per_subshape must call query_many exactly once"
        );
        assert_eq!(
            kernel.query_calls(),
            0,
            "query_per_subshape must not call per-element query"
        );
    }

    #[test]
    fn query_per_subshape_surfaces_query_many_length_invariant_violation() {
        // Three edge ids, but the kernel returns only two values. The helper
        // must surface QueryError::QueryFailed naming "my_selector" and
        // "length invariant" rather than silently truncating via zip.
        let edge_ids = vec![
            GeometryHandleId(901),
            GeometryHandleId(902),
            GeometryHandleId(903),
        ];
        let kernel = FixedReplyQueryManyKernel {
            edges: edge_ids.clone(),
            canned_reply: vec![Value::Real(0.001), Value::Real(0.002)],
        };

        let err = query_per_subshape(&kernel, &edge_ids, "my_selector", GeometryQuery::EdgeLength)
            .expect_err("query_per_subshape must reject length-mismatched query_many output");

        match err {
            QueryError::QueryFailed(msg) => {
                assert!(
                    msg.contains("my_selector") && msg.contains("length invariant"),
                    "expected selector name + length invariant in error, got {:?}",
                    msg
                );
            }
            other => panic!("expected QueryFailed, got {:?}", other),
        }
    }

    #[test]
    fn query_per_subshape_accepts_shared_kernel_reference() {
        // Compile witness: query_per_subshape must accept &K, not &mut K.
        let edge_ids = vec![GeometryHandleId(1101), GeometryHandleId(1102)];
        let kernel = CountingKernel::new()
            .with_response(GeometryHandleId(1101), Value::Real(0.001))
            .with_response(GeometryHandleId(1102), Value::Real(0.002));

        let values = query_per_subshape(
            &kernel,
            &edge_ids,
            "shared_ref_test",
            GeometryQuery::EdgeLength,
        )
        .expect("query_per_subshape should succeed with a shared kernel reference");

        assert_eq!(
            values,
            vec![Value::Real(0.001), Value::Real(0.002)],
            "helper must return values aligned with input ids through a shared reference"
        );
    }

    // ─── resolve_unique_by_tag tests (task 2332 — W_TOPOLOGY_TAG_STALE) ────────

    /// Happy-path: exactly one candidate matches the target tag.
    /// Resolver must return `Some(matched_handle)` and push no diagnostics.
    #[test]
    fn resolve_unique_by_tag_one_match_returns_some_with_no_diagnostics() {
        use reify_core::{Diagnostic, SourceSpan};
        use reify_ir::{FeatureTag, FeatureTagTable, StepKind};

        let id1 = GeometryHandleId(1);
        let id2 = GeometryHandleId(2);
        let id3 = GeometryHandleId(3);

        let shared_span = SourceSpan::new(0, 10);
        let tag1 = FeatureTag {
            source_span: shared_span,
            step_kind: StepKind::Primitive,
            sub_index: 0,
        };
        let tag2 = FeatureTag {
            source_span: shared_span,
            step_kind: StepKind::Primitive,
            sub_index: 1,
        };
        let tag3 = FeatureTag {
            source_span: shared_span,
            step_kind: StepKind::Primitive,
            sub_index: 2,
        };

        let mut table = FeatureTagTable::default();
        table.record(id1, tag1);
        table.record(id2, tag2);
        table.record(id3, tag3);

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let selector_span = SourceSpan::new(10, 20);
        let result = resolve_unique_by_tag(
            &table,
            &[id1, id2, id3],
            tag2,
            selector_span,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(id2),
            "should return the uniquely-matching handle"
        );
        assert!(
            diagnostics.is_empty(),
            "no diagnostics on a clean unique match"
        );
    }

    /// Zero-match path: no candidates carry the target tag.
    /// Resolver must return `None` and push exactly one `TopologyTagStale` warning
    /// with labels pointing at both the selector call site and the tag origin.
    #[test]
    fn resolve_unique_by_tag_zero_matches_emits_warning_and_returns_none() {
        use reify_core::{Diagnostic, DiagnosticCode, Severity, SourceSpan};
        use reify_ir::{FeatureTag, FeatureTagTable, StepKind};

        let id1 = GeometryHandleId(10);
        let id2 = GeometryHandleId(11);

        // Both handles carry a non-target tag (sub_index differs from target).
        let tag_source_span = SourceSpan::new(100, 110);
        let tag1 = FeatureTag {
            source_span: tag_source_span,
            step_kind: StepKind::Boolean,
            sub_index: 5,
        };
        let tag2 = FeatureTag {
            source_span: tag_source_span,
            step_kind: StepKind::Boolean,
            sub_index: 6,
        };

        let mut table = FeatureTagTable::default();
        table.record(id1, tag1);
        table.record(id2, tag2);

        // Target tag is distinct from both (sub_index 99 not present).
        let target_tag = FeatureTag {
            source_span: tag_source_span,
            step_kind: StepKind::Boolean,
            sub_index: 99,
        };
        let selector_span = SourceSpan::new(200, 210);

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = resolve_unique_by_tag(
            &table,
            &[id1, id2],
            target_tag,
            selector_span,
            &mut diagnostics,
        );

        assert!(result.is_none(), "zero matches should return None");
        assert_eq!(
            diagnostics.len(),
            1,
            "exactly one diagnostic on zero matches"
        );

        let diag = &diagnostics[0];
        assert_eq!(diag.severity, Severity::Warning, "should be a warning");
        assert_eq!(
            diag.code,
            Some(DiagnosticCode::TopologyTagStale),
            "must carry TopologyTagStale code"
        );
        assert!(
            diag.message.contains("matched 0 "),
            "message should contain 'matched 0 ' to pin the exact count, got: {:?}",
            diag.message,
        );
        assert!(
            diag.labels.iter().any(|l| l.span == selector_span),
            "labels must include selector_span"
        );
        assert!(
            diag.labels.iter().any(|l| l.span == target_tag.source_span),
            "labels must include target tag source_span"
        );
    }

    /// Multi-match path: THREE candidates all carry the same target tag (ambiguous/split topology).
    /// Resolver must return `None`, push exactly ONE diagnostic (not one per duplicate),
    /// the message must name the count "3", and labels include both spans.
    #[test]
    fn resolve_unique_by_tag_multiple_matches_emits_warning_and_returns_none() {
        use reify_core::{Diagnostic, DiagnosticCode, Severity, SourceSpan};
        use reify_ir::{FeatureTag, FeatureTagTable, StepKind};

        let id1 = GeometryHandleId(20);
        let id2 = GeometryHandleId(21);
        let id3 = GeometryHandleId(22);

        // All three handles carry the SAME target tag — ambiguous split scenario.
        let tag_source_span = SourceSpan::new(50, 60);
        let target_tag = FeatureTag {
            source_span: tag_source_span,
            step_kind: StepKind::Sweep,
            sub_index: 7,
        };

        let mut table = FeatureTagTable::default();
        table.record(id1, target_tag);
        table.record(id2, target_tag);
        table.record(id3, target_tag);

        let selector_span = SourceSpan::new(300, 310);
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = resolve_unique_by_tag(
            &table,
            &[id1, id2, id3],
            target_tag,
            selector_span,
            &mut diagnostics,
        );

        assert!(result.is_none(), "multiple matches should return None");
        assert_eq!(
            diagnostics.len(),
            1,
            "must fire exactly one diagnostic regardless of match count"
        );

        let diag = &diagnostics[0];
        assert_eq!(diag.severity, Severity::Warning, "should be a warning");
        assert_eq!(
            diag.code,
            Some(DiagnosticCode::TopologyTagStale),
            "must carry TopologyTagStale code"
        );
        assert!(
            diag.message.contains("matched 3 "),
            "message must contain 'matched 3 ' to pin the exact count, got: {:?}",
            diag.message,
        );
        assert!(
            diag.labels.iter().any(|l| l.span == selector_span),
            "labels must include selector_span"
        );
        assert!(
            diag.labels.iter().any(|l| l.span == target_tag.source_span),
            "labels must include target tag source_span"
        );
    }

    /// Regression: duplicate candidate ids must not inflate the match count to a
    /// spurious split-topology warning.
    ///
    /// If the resolver doesn't deduplicate, passing `&[id1, id1, id1]` for a
    /// handle that carries the target tag would count `n = 3` and emit a
    /// `TopologyTagStale` warning — a false positive.  The resolver must treat
    /// all three slots as one logical match and return `Some(id1)` with zero
    /// diagnostics.
    #[test]
    fn resolve_unique_by_tag_duplicate_candidate_does_not_inflate_match_count() {
        use reify_core::{Diagnostic, SourceSpan};
        use reify_ir::{FeatureTag, FeatureTagTable, StepKind};

        let id1 = GeometryHandleId(50);

        let tag_source_span = SourceSpan::new(400, 410);
        let target_tag = FeatureTag {
            source_span: tag_source_span,
            step_kind: StepKind::Primitive,
            sub_index: 0,
        };

        let mut table = FeatureTagTable::default();
        table.record(id1, target_tag);

        let selector_span = SourceSpan::new(500, 510);
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        // Pass the SAME id three times — an unguarded resolver would count n=3 and
        // emit a spurious W_TOPOLOGY_TAG_STALE warning instead of returning Some(id1).
        let result = resolve_unique_by_tag(
            &table,
            &[id1, id1, id1],
            target_tag,
            selector_span,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(id1),
            "duplicate candidate ids must not inflate the match count to a spurious split-topology warning",
        );
        assert!(
            diagnostics.is_empty(),
            "duplicate candidate ids must not inflate the match count to a spurious split-topology warning; \
             got diagnostics: {:?}",
            diagnostics,
        );
    }

    // ─── filter_by_value tests ───────────────────────────────────────────────

    /// Happy-path: `filter_by_value` returns only the ids whose predicate
    /// returns `true`, issues exactly one `query_many` call, and never calls
    /// the per-element `query` path.
    ///
    /// Three edges staged with Real values 0.001 / 0.002 / 0.003.
    /// The predicate selects the middle id (0.0015 ≤ v ≤ 0.0025).
    ///
    /// This test will fail to compile until `filter_by_value` is added
    /// (step-2 impl), satisfying the TDD "red" requirement.  It also pins
    /// that the predicate closure receives `&Value` — matching the `expect_real`
    /// signature change in step-2.
    #[test]
    fn filter_by_value_returns_predicate_passing_subset_via_single_query_many() {
        let ids = vec![
            GeometryHandleId(1001),
            GeometryHandleId(1002),
            GeometryHandleId(1003),
        ];
        let kernel = CountingKernel::new()
            .with_edges(ids.clone())
            .with_response(ids[0], Value::Real(0.001))
            .with_response(ids[1], Value::Real(0.002))
            .with_response(ids[2], Value::Real(0.003));

        let result = filter_by_value(
            &kernel,
            &ids,
            "test_label",
            GeometryQuery::EdgeLength,
            |id, value| {
                let x = expect_real("EdgeLength", id, value)?;
                Ok((0.0015..=0.0025).contains(&x))
            },
        )
        .expect("filter_by_value should succeed");

        assert_eq!(
            result,
            vec![ids[1]],
            "only the middle id (value 0.002) should pass the predicate"
        );
        assert_eq!(
            kernel.query_many_calls(),
            1,
            "filter_by_value must issue exactly one query_many call"
        );
        assert_eq!(
            kernel.query_calls(),
            0,
            "filter_by_value must not fall back to per-element query"
        );
    }

    /// Error-propagation contract: an `Err` returned from the predicate closure
    /// surfaces verbatim from `filter_by_value` — the helper does not swallow or
    /// transform closure errors.
    ///
    /// Stages two ids: the first has a non-`Value::Real` response (triggers
    /// `expect_real` → `Err`); the second has a valid `Value::Real` (would pass
    /// if ever reached).  A `Cell<usize>` counter inside the predicate proves
    /// short-circuit behaviour: after the `Err` the counter is exactly 1,
    /// demonstrating the second id was never visited.
    ///
    /// Also asserts:
    ///   (i)  the returned error message contains `"non-real value"` and the id,
    ///   (ii) `kernel.query_many_calls() == 1` (the batched call fired before the
    ///        predicate loop, so the error is a predicate error, not a kernel error),
    ///   (iii) `kernel.query_calls() == 0` (no per-element fallback).
    #[test]
    fn filter_by_value_propagates_predicate_error() {
        use std::cell::Cell;

        let id1 = GeometryHandleId(1010);
        let id2 = GeometryHandleId(1011);
        // id1 triggers the error; id2 has a valid Real that would pass if visited.
        let kernel = CountingKernel::new()
            .with_response(id1, Value::String("not real".into()))
            .with_response(id2, Value::Real(0.5));

        let call_count = Cell::new(0usize);
        let err = filter_by_value(
            &kernel,
            &[id1, id2],
            "test_label",
            GeometryQuery::EdgeLength,
            |id, value| {
                call_count.set(call_count.get() + 1);
                let _ = expect_real("EdgeLength", id, value)?;
                Ok(true)
            },
        )
        .expect_err("filter_by_value must propagate predicate Err");

        match err {
            QueryError::QueryFailed(ref msg) => {
                assert!(
                    msg.contains("non-real value"),
                    "error must mention 'non-real value', got: {:?}",
                    msg
                );
                assert!(
                    msg.contains("1010"),
                    "error must name the id (1010), got: {:?}",
                    msg
                );
            }
            other => panic!("expected QueryFailed, got {:?}", other),
        }
        assert_eq!(
            call_count.get(),
            1,
            "predicate must be invoked exactly once (short-circuit on first Err, second id never visited)"
        );
        assert_eq!(
            kernel.query_many_calls(),
            1,
            "query_many must have fired (predicate error happens after batched query)"
        );
        assert_eq!(
            kernel.query_calls(),
            0,
            "per-element query must not be called"
        );
    }

    /// Regression: dedup must apply to the full candidate set, not only matching
    /// ids.  Interleaves a matching id with a non-matching id (both duplicated)
    /// to verify that the dedup gate (`seen.insert`) is evaluated unconditionally
    /// for every candidate — regardless of tag-match result.
    ///
    /// This protects against a future refactor that moves `seen.insert` inside the
    /// tag-match branch (e.g. swapping the `&&` operands to
    /// `table.lookup(id) == Some(&target) && seen.insert(id)`), which would
    /// correctly dedup matching ids but silently skip adding non-matching ids to
    /// `seen`, leaving them visible to subsequent loop iterations.
    ///
    /// Slice under test: `[id_match, id_nomatch, id_nomatch, id_match]`.
    /// Expected: `Some(id_match)`, zero diagnostics.
    #[test]
    fn resolve_unique_by_tag_interleaved_matching_and_nonmatching_duplicates() {
        use reify_core::{Diagnostic, SourceSpan};
        use reify_ir::{FeatureTag, FeatureTagTable, StepKind};

        let id_match = GeometryHandleId(100);
        let id_nomatch = GeometryHandleId(200);

        let tag_source_span = SourceSpan::new(600, 620);
        let target_tag = FeatureTag {
            source_span: tag_source_span,
            step_kind: StepKind::Primitive,
            sub_index: 0,
        };
        let other_tag = FeatureTag {
            source_span: tag_source_span,
            step_kind: StepKind::Primitive,
            sub_index: 1,
        };

        let mut table = FeatureTagTable::default();
        table.record(id_match, target_tag);
        table.record(id_nomatch, other_tag);

        let selector_span = SourceSpan::new(700, 720);
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        // Both ids appear twice; they are interleaved so the duplicate of id_match
        // is not adjacent to its first occurrence.  An unguarded resolver (or one
        // that only deduplicates matching ids) would count n=2 and emit a spurious
        // W_TOPOLOGY_TAG_STALE instead of returning Some(id_match).
        let result = resolve_unique_by_tag(
            &table,
            &[id_match, id_nomatch, id_nomatch, id_match],
            target_tag,
            selector_span,
            &mut diagnostics,
        );

        assert_eq!(
            result,
            Some(id_match),
            "duplicate candidate ids must not inflate the match count regardless of tag-match order",
        );
        assert!(
            diagnostics.is_empty(),
            "no spurious W_TOPOLOGY_TAG_STALE when matching and non-matching duplicates are interleaved; \
             got diagnostics: {:?}",
            diagnostics,
        );
    }

    // ── step-1 (task 3616): SubKind + compose_sub_handle_hash RED tests ────────

    /// SubKind::Edge discriminant must be 0x01 (PRD §4 domain-separator).
    #[test]
    fn sub_kind_edge_discriminant_is_0x01() {
        assert_eq!(SubKind::Edge.as_byte(), 0x01u8);
    }

    /// SubKind::Face discriminant must be 0x02.
    #[test]
    fn sub_kind_face_discriminant_is_0x02() {
        assert_eq!(SubKind::Face.as_byte(), 0x02u8);
    }

    /// compose_sub_handle_hash is deterministic: same (parent, kind, index)
    /// produces bit-identical output across two independent calls (PRD §4 ii).
    #[test]
    fn compose_sub_handle_hash_is_deterministic() {
        let parent: [u8; 32] = [0xAB; 32];
        let a = compose_sub_handle_hash(&parent, SubKind::Edge, 0);
        let b = compose_sub_handle_hash(&parent, SubKind::Edge, 0);
        assert_eq!(a, b, "identical inputs must produce identical outputs");
    }

    /// Different topexp indices must produce different hashes (PRD §4 iii).
    #[test]
    fn compose_sub_handle_hash_differs_by_index() {
        let parent: [u8; 32] = [0x11; 32];
        let h0 = compose_sub_handle_hash(&parent, SubKind::Edge, 0);
        let h1 = compose_sub_handle_hash(&parent, SubKind::Edge, 1);
        assert_ne!(h0, h1, "index 0 and index 1 must hash differently");
    }

    /// Edge and Face at the same index must produce different hashes
    /// (PRD §4 iii — sub_kind is part of the domain separation).
    #[test]
    fn compose_sub_handle_hash_differs_by_sub_kind() {
        let parent: [u8; 32] = [0x22; 32];
        let he = compose_sub_handle_hash(&parent, SubKind::Edge, 0);
        let hf = compose_sub_handle_hash(&parent, SubKind::Face, 0);
        assert_ne!(he, hf, "Edge and Face at same index must hash differently");
    }

    /// The output hash must be non-zero (a zero hash is the collision-free
    /// domain sentinel; an honest ContentHash of non-zero input must differ).
    #[test]
    fn compose_sub_handle_hash_is_non_zero() {
        let parent: [u8; 32] = [0x33; 32];
        let h = compose_sub_handle_hash(&parent, SubKind::Edge, 0);
        assert_ne!(h, [0u8; 32], "hash output must be non-zero");
    }

    // ── step-3 (task 3616): make_sub_handle RED tests ──────────────────────────

    /// make_sub_handle carries the parent's realization_ref unchanged (PRD §4).
    #[test]
    fn make_sub_handle_carries_parent_realization_ref() {
        use reify_core::identity::RealizationNodeId;
        let rr = RealizationNodeId::new("BoxEdges", 0);
        let parent_hash: [u8; 32] = [0xAA; 32];
        let sub = make_sub_handle(&rr, &parent_hash, SubKind::Edge, 0, GeometryHandleId(5));
        match sub {
            Value::GeometryHandle {
                realization_ref, ..
            } => {
                assert_eq!(realization_ref.entity, "BoxEdges");
                assert_eq!(realization_ref.index, 0);
            }
            other => panic!("expected Value::GeometryHandle, got {:?}", other),
        }
    }

    /// make_sub_handle sets upstream_values_hash to compose_sub_handle_hash output.
    #[test]
    fn make_sub_handle_upstream_hash_matches_compose() {
        use reify_core::identity::RealizationNodeId;
        let rr = RealizationNodeId::new("BoxEdges", 0);
        let parent_hash: [u8; 32] = [0xBB; 32];
        let expected_hash = compose_sub_handle_hash(&parent_hash, SubKind::Edge, 3);
        let sub = make_sub_handle(&rr, &parent_hash, SubKind::Edge, 3, GeometryHandleId(7));
        match sub {
            Value::GeometryHandle {
                upstream_values_hash,
                ..
            } => {
                assert_eq!(upstream_values_hash, expected_hash);
            }
            other => panic!("expected Value::GeometryHandle, got {:?}", other),
        }
    }

    /// Two sub-handles of one parent at different topexp indices must compare
    /// UNEQUAL under PartialEq (PRD §4 iii — upstream_values_hash differs).
    #[test]
    fn make_sub_handle_different_indices_are_unequal() {
        use reify_core::identity::RealizationNodeId;
        let rr = RealizationNodeId::new("BoxEdges", 0);
        let parent_hash: [u8; 32] = [0xCC; 32];
        let a = make_sub_handle(&rr, &parent_hash, SubKind::Edge, 0, GeometryHandleId(1));
        let b = make_sub_handle(&rr, &parent_hash, SubKind::Edge, 1, GeometryHandleId(2));
        assert_ne!(a, b, "different topexp indices must compare unequal");
    }

    // ── unsupported_overhang_faces tests (task 4406 step-1 RED) ─────────────

    /// Helper: build a FaceNormal JSON string from x,y,z components.
    fn face_normal_json(x: f64, y: f64, z: f64) -> Value {
        Value::String(format!("{{\"x\":{x},\"y\":{y},\"z\":{z}}}"))
    }

    /// (a) Wedge fixture: three planar faces with hand-chosen outward normals.
    ///
    /// n0 = (√3/2, 0, −1/2)  → n·b = −0.5 → dip = asin(0.5) = 30°
    ///                             in set at max=20° (sin20°<0.5);
    ///                             NOT in set at max=45° (0.5 < sin45°)
    /// n1 = (0, 0, 1)         → top face, dip = −π/2 (never overhang)
    /// n2 = (1, 0, 0)         → side face, dip = 0 (never overhang)
    ///
    /// worst_dip = max(30°, −90°, 0°) = 30° regardless of max_overhang_angle.
    #[test]
    fn overhang_wedge_worst_dip_and_face_set() {
        let face_ids = vec![
            GeometryHandleId(501),
            GeometryHandleId(502),
            GeometryHandleId(503),
        ];
        let sqrt3_over2: f64 = (3.0_f64).sqrt() / 2.0;
        let mut kernel = CountingKernel::new()
            .with_faces(face_ids.clone())
            .with_response(face_ids[0], face_normal_json(sqrt3_over2, 0.0, -0.5))
            .with_response(face_ids[1], face_normal_json(0.0, 0.0, 1.0))
            .with_response(face_ids[2], face_normal_json(1.0, 0.0, 0.0));
        let handle = GeometryHandleId(1);
        let build_dir = [0.0, 0.0, 1.0_f64];

        // At max_overhang_angle = 20°: n0 is unsupported (sin20° ≈ 0.342 < 0.5).
        let (faces_20, worst_dip) =
            unsupported_overhang_faces(&mut kernel, handle, build_dir, 20f64.to_radians())
                .expect("20° call should succeed");
        assert_eq!(
            faces_20,
            vec![face_ids[0]],
            "only n0 (30° dip) is unsupported at max=20°"
        );
        let expected_dip = 30f64.to_radians();
        assert!(
            (worst_dip - expected_dip).abs() < 1e-9,
            "worst_dip ≈ 30° = π/6 (got {worst_dip})"
        );
        assert_eq!(kernel.query_many_calls(), 1, "must batch via query_many");
        assert_eq!(kernel.query_calls(), 0, "must not use per-element query");

        // At max_overhang_angle = 45°: sin45° ≈ 0.707 > 0.5, so n0 is NOT in set.
        let (faces_45, worst_dip2) =
            unsupported_overhang_faces(&mut kernel, handle, build_dir, 45f64.to_radians())
                .expect("45° call should succeed");
        assert!(
            faces_45.is_empty(),
            "no face is unsupported at max=45° (set must be empty)"
        );
        assert!(
            (worst_dip2 - expected_dip).abs() < 1e-9,
            "worst_dip is still 30° independent of max_overhang_angle (got {worst_dip2})"
        );
    }

    /// (b) Self-supporting: the dip-30° face is NOT in the unsupported set when
    ///     max_overhang_angle = 45° (matches the wedge_worst_dip test above).
    #[test]
    fn overhang_self_supporting_at_45_degrees() {
        let face_ids = vec![GeometryHandleId(511), GeometryHandleId(512)];
        let sqrt3_over2: f64 = (3.0_f64).sqrt() / 2.0;
        let mut kernel = CountingKernel::new()
            .with_faces(face_ids.clone())
            .with_response(face_ids[0], face_normal_json(sqrt3_over2, 0.0, -0.5))
            .with_response(face_ids[1], face_normal_json(0.0, 0.0, 1.0));
        let handle = GeometryHandleId(1);

        let (faces, _worst) =
            unsupported_overhang_faces(&mut kernel, handle, [0.0, 0.0, 1.0], 45f64.to_radians())
                .expect("should succeed");
        assert!(
            faces.is_empty(),
            "dip-30° face is self-supporting at max=45°"
        );
    }

    /// (c) Validation: zero / non-finite build_dir and out-of-range angle → QueryFailed.
    #[test]
    fn overhang_validation_errors() {
        let mut kernel = CountingKernel::new().with_faces(vec![GeometryHandleId(521)]);
        let handle = GeometryHandleId(1);

        // Zero build_dir
        assert!(
            matches!(
                unsupported_overhang_faces(&mut kernel, handle, [0.0, 0.0, 0.0], 0.1),
                Err(QueryError::QueryFailed(_))
            ),
            "zero build_dir must return QueryFailed"
        );
        // Non-finite build_dir
        assert!(
            matches!(
                unsupported_overhang_faces(&mut kernel, handle, [f64::NAN, 0.0, 1.0], 0.1),
                Err(QueryError::QueryFailed(_))
            ),
            "NaN component in build_dir must return QueryFailed"
        );
        // max_overhang_angle < 0
        assert!(
            matches!(
                unsupported_overhang_faces(&mut kernel, handle, [0.0, 0.0, 1.0], -0.1),
                Err(QueryError::QueryFailed(_))
            ),
            "negative max_overhang_angle must return QueryFailed"
        );
        // max_overhang_angle > π/2
        assert!(
            matches!(
                unsupported_overhang_faces(
                    &mut kernel,
                    handle,
                    [0.0, 0.0, 1.0],
                    std::f64::consts::PI
                ),
                Err(QueryError::QueryFailed(_))
            ),
            "max_overhang_angle > π/2 must return QueryFailed"
        );
    }

    // ── min_draft_angle tests (task 4406 step-3 RED) ────────────────────────

    /// (a) Taper + re-entrant fixture: two wall faces + top/bottom excluded.
    ///
    /// pull_dir = +Z; WALL_WINDOW = 45° (sin45° ≈ 0.7071).
    /// n_taper    = (cos5°, 0, sin5°)   → n·p = sin5°  > 0, in window → δ ≈ +5°
    /// n_reentrant= (cos3°, 0, −sin3°)  → n·p = −sin3° < 0, in window → δ ≈ −3°, undercut
    /// n_top = (0,0,1) / n_bot = (0,0,−1) → |n·p|=1 ≥ sin45° → excluded
    ///
    /// signed_min_draft ≈ −3°.to_radians(), has_undercut = true.
    #[test]
    fn draft_taper_reentrant_fixture() {
        use std::f64::consts::FRAC_PI_2;
        let face_ids = vec![
            GeometryHandleId(601),
            GeometryHandleId(602),
            GeometryHandleId(603),
            GeometryHandleId(604),
        ];
        let cos5 = 5f64.to_radians().cos();
        let sin5 = 5f64.to_radians().sin();
        let cos3 = 3f64.to_radians().cos();
        let sin3 = 3f64.to_radians().sin();
        let mut kernel = CountingKernel::new()
            .with_faces(face_ids.clone())
            .with_response(face_ids[0], face_normal_json(cos5, 0.0, sin5)) // taper
            .with_response(face_ids[1], face_normal_json(cos3, 0.0, -sin3)) // re-entrant
            .with_response(face_ids[2], face_normal_json(0.0, 0.0, 1.0)) // top
            .with_response(face_ids[3], face_normal_json(0.0, 0.0, -1.0)); // bottom
        let handle = GeometryHandleId(1);
        let pull_dir = [0.0_f64, 0.0, 1.0];

        let (signed_min_draft, has_undercut) =
            min_draft_angle(&mut kernel, handle, pull_dir).expect("should succeed");

        let expected = (-3f64).to_radians();
        assert!(
            (signed_min_draft - expected).abs() < 1e-9,
            "signed_min_draft ≈ −3° (got {signed_min_draft})"
        );
        assert!(has_undercut, "re-entrant wall face must set has_undercut=true");
        assert_eq!(kernel.query_many_calls(), 1, "must batch via query_many");
        assert_eq!(kernel.query_calls(), 0, "must not use per-element query");
        let _ = FRAC_PI_2; // silence unused-import lint if any
    }

    /// (b) WALL_WINDOW contract: pins WALL_WINDOW_RAD == π/4 (45°).
    ///     A near-vertical wall face (|n·p| just below sin45°) must contribute;
    ///     top/bottom (|n·p| = 1) must be excluded.
    #[test]
    fn draft_wall_window_is_45_degrees() {
        // Contract constant must equal π/4.
        assert!(
            (WALL_WINDOW_RAD - std::f64::consts::FRAC_PI_4).abs() < f64::EPSILON,
            "WALL_WINDOW_RAD must be π/4 (45°)"
        );

        // A face with |n·p| just below sin(45°) must be in the wall window.
        let face_ids = vec![GeometryHandleId(611), GeometryHandleId(612)];
        // sin(44.9°) ≈ 0.7059 < sin(45°) ≈ 0.7071 → in window
        let sin449 = 44.9f64.to_radians().sin();
        let cos449 = 44.9f64.to_radians().cos();
        let mut kernel = CountingKernel::new()
            .with_faces(face_ids.clone())
            .with_response(face_ids[0], face_normal_json(cos449, 0.0, sin449)) // near-vertical wall
            .with_response(face_ids[1], face_normal_json(0.0, 0.0, 1.0)); // top (excluded)
        let handle = GeometryHandleId(1);

        let (signed_min_draft, has_undercut) =
            min_draft_angle(&mut kernel, handle, [0.0, 0.0, 1.0]).expect("should succeed");

        // The near-vertical face has δ = π/2 - acos(sin449) ≈ 44.9° and contributes.
        let expected = std::f64::consts::FRAC_PI_2 - sin449.acos();
        assert!(
            (signed_min_draft - expected).abs() < 1e-9,
            "near-vertical wall must set min_draft (got {signed_min_draft})"
        );
        assert!(!has_undercut, "positive draft: no undercut");
    }

    /// (c) No-wall fixture: only top/bottom faces → sentinel π/2, no undercut.
    #[test]
    fn draft_no_wall_returns_pi_over_2_sentinel() {
        let face_ids = vec![GeometryHandleId(621), GeometryHandleId(622)];
        let mut kernel = CountingKernel::new()
            .with_faces(face_ids.clone())
            .with_response(face_ids[0], face_normal_json(0.0, 0.0, 1.0))
            .with_response(face_ids[1], face_normal_json(0.0, 0.0, -1.0));
        let handle = GeometryHandleId(1);

        let (signed_min_draft, has_undercut) =
            min_draft_angle(&mut kernel, handle, [0.0, 0.0, 1.0]).expect("should succeed");

        assert!(
            (signed_min_draft - std::f64::consts::FRAC_PI_2).abs() < f64::EPSILON,
            "no wall faces → sentinel π/2 (got {signed_min_draft})"
        );
        assert!(!has_undercut, "no wall faces → no undercut");
    }

    /// (d) Validation: zero / non-finite pull_dir → QueryFailed.
    #[test]
    fn draft_validation_errors() {
        let mut kernel = CountingKernel::new().with_faces(vec![GeometryHandleId(631)]);
        let handle = GeometryHandleId(1);

        assert!(
            matches!(
                min_draft_angle(&mut kernel, handle, [0.0, 0.0, 0.0]),
                Err(QueryError::QueryFailed(_))
            ),
            "zero pull_dir must return QueryFailed"
        );
        assert!(
            matches!(
                min_draft_angle(&mut kernel, handle, [f64::INFINITY, 0.0, 0.0]),
                Err(QueryError::QueryFailed(_))
            ),
            "infinite pull_dir must return QueryFailed"
        );
    }

    // ── Curved conservative-bound tests (task 4406 step-5 / step-7 RED) ─────

    /// step-5 RED: tessellate per-vertex normals must refine worst_dip to a
    /// value ≥ the steepest facet dip (conservative bound; G6).
    ///
    /// Fixture: one BRep face with n=(√3/2,0,−1/2) → per-face dip ≈ 30°.
    /// Mesh carries a steep outward vertex normal n_f=(0.6427,0,−0.766)
    /// → −n_f·b = 0.766 ≈ sin(50°) → facet dip ≈ 50°.
    ///
    /// Assertion: worst_dip ≥ 50°.to_radians() − ε  (inequality — G6).
    /// Fails until step-6 adds the tessellate fold (step-2 impl ignores
    /// the mesh → worst_dip stays at ~30°).
    #[test]
    fn overhang_curved_conservative_bound() {
        let face_ids = vec![GeometryHandleId(701)];
        let sqrt3_over2 = (3.0_f64).sqrt() / 2.0;

        // Steep vertex normal: z ≈ −0.766 → facet dip ≈ asin(0.766) ≈ 50°.
        let steep_mesh = Mesh {
            vertices: vec![0.0, 0.0, 0.0],
            indices: vec![],
            normals: Some(vec![0.6427_f32, 0.0_f32, -0.766_f32]),
        };

        let mut kernel = CountingKernel::new()
            .with_faces(face_ids.clone())
            .with_response(face_ids[0], face_normal_json(sqrt3_over2, 0.0, -0.5))
            .with_mesh(steep_mesh);
        let handle = GeometryHandleId(1);

        let (_faces, worst_dip) =
            unsupported_overhang_faces(&mut kernel, handle, [0.0, 0.0, 1.0], 20f64.to_radians())
                .expect("should succeed");

        // The steep facet dip ≈ asin(0.766) ≥ 50° — exact float would be
        // fragile on f32→f64 cast; an inequality is G6-safe.
        let min_expected = 50f64.to_radians() - 1e-4;
        assert!(
            worst_dip >= min_expected,
            "curved conservative bound: worst_dip must be ≥ 50° (got {} rad ≈ {}°)",
            worst_dip,
            worst_dip.to_degrees()
        );
    }

    /// step-7 RED: tessellate per-vertex normals must lower signed_min_draft and
    /// set has_undercut when a re-entrant facet is present (conservative; G6).
    ///
    /// Fixture: one wall face n=(cos10°,0,sin10°) → δ=+10°, no undercut.
    /// Mesh carries a re-entrant wall-window vertex normal
    /// n_f=(cos4°,0,−sin4°) → n_f·p=−sin4°≈−0.0698, in window → δ_f≈−4°.
    ///
    /// Assertions (inequalities — G6):
    ///   signed_min_draft ≤ (−4°).to_radians() + ε
    ///   has_undercut == true
    ///
    /// Fails until step-8 adds the draft tessellate fold.
    #[test]
    fn draft_curved_conservative_bound() {
        let face_ids = vec![GeometryHandleId(711)];
        let cos10 = 10f64.to_radians().cos();
        let sin10 = 10f64.to_radians().sin();

        // Re-entrant facet normal: n_f·p = −sin4° → δ_f ≈ −4° (undercut).
        let cos4 = 4f32.to_radians().cos();
        let sin4 = 4f32.to_radians().sin();
        let reentrant_mesh = Mesh {
            vertices: vec![0.0, 0.0, 0.0],
            indices: vec![],
            normals: Some(vec![cos4, 0.0_f32, -sin4]),
        };

        let mut kernel = CountingKernel::new()
            .with_faces(face_ids.clone())
            .with_response(face_ids[0], face_normal_json(cos10, 0.0, sin10))
            .with_mesh(reentrant_mesh);
        let handle = GeometryHandleId(1);

        let (signed_min_draft, has_undercut) =
            min_draft_angle(&mut kernel, handle, [0.0, 0.0, 1.0]).expect("should succeed");

        // min_draft must be ≤ −4° (more negative than the per-face +10°).
        let max_expected = (-4f64).to_radians() + 1e-4;
        assert!(
            signed_min_draft <= max_expected,
            "curved conservative bound: signed_min_draft must be ≤ −4° (got {} rad ≈ {}°)",
            signed_min_draft,
            signed_min_draft.to_degrees()
        );
        assert!(has_undercut, "re-entrant facet must set has_undercut=true");
    }

    // ── Tessellate-error-is-no-op tests (suggestion 5 coverage) ─────────────

    /// When `tessellate` returns `Err`, `unsupported_overhang_faces` must
    /// still succeed and return the per-BRep-face result unchanged.
    ///
    /// Fixture: one face with n=(√3/2,0,−1/2) → per-face worst_dip ≈ 30°.
    /// With `fail_tessellate` the mesh fold path is skipped entirely,
    /// so worst_dip stays at ~30° and the selector returns `Ok`.
    #[test]
    fn overhang_tessellate_error_is_noop() {
        let face_ids = vec![GeometryHandleId(801)];
        let sqrt3_over2 = (3.0_f64).sqrt() / 2.0;

        let mut kernel = CountingKernel::new()
            .with_faces(face_ids.clone())
            .with_response(face_ids[0], face_normal_json(sqrt3_over2, 0.0, -0.5))
            .with_fail_tessellate();
        let handle = GeometryHandleId(1);

        let (_faces, worst_dip) =
            unsupported_overhang_faces(&mut kernel, handle, [0.0, 0.0, 1.0], 20f64.to_radians())
                .expect("tessellate failure must not propagate — selector must succeed");

        // Per-face result stands: worst_dip ≈ 30° (not lowered or invalidated).
        let expected = 30f64.to_radians();
        assert!(
            (worst_dip - expected).abs() < 1e-9,
            "tessellate error must be no-op: worst_dip must stay ≈ 30° (got {} rad = {}°)",
            worst_dip,
            worst_dip.to_degrees()
        );
    }

    /// When `tessellate` returns `Err`, `min_draft_angle` must still succeed
    /// and return the per-BRep-face result unchanged.
    ///
    /// Fixture: one wall face n=(cos10°,0,sin10°) → per-face δ=+10°, no undercut.
    /// With `fail_tessellate` the mesh fold is skipped, so signed_min_draft
    /// stays at +10° and has_undercut stays false.
    #[test]
    fn draft_tessellate_error_is_noop() {
        let face_ids = vec![GeometryHandleId(811)];
        let cos10 = 10f64.to_radians().cos();
        let sin10 = 10f64.to_radians().sin();

        let mut kernel = CountingKernel::new()
            .with_faces(face_ids.clone())
            .with_response(face_ids[0], face_normal_json(cos10, 0.0, sin10))
            .with_fail_tessellate();
        let handle = GeometryHandleId(1);

        let (signed_min_draft, has_undercut) =
            min_draft_angle(&mut kernel, handle, [0.0, 0.0, 1.0])
                .expect("tessellate failure must not propagate — selector must succeed");

        // Per-face result stands: signed_min_draft ≈ +10°, no undercut.
        let expected = 10f64.to_radians();
        assert!(
            (signed_min_draft - expected).abs() < 1e-9,
            "tessellate error must be no-op: min_draft must stay ≈ +10° (got {} rad = {}°)",
            signed_min_draft,
            signed_min_draft.to_degrees()
        );
        assert!(
            !has_undercut,
            "tessellate error must be no-op: has_undercut must stay false"
        );
    }

    // ── Two sub-handles at the same (parent, kind, index) but different
    /// kernel_handle ids must compare EQUAL — kernel_handle is excluded from
    /// PartialEq (PRD §4 iv cache-hit equality).
    #[test]
    fn make_sub_handle_same_parent_kind_index_equal_despite_differing_kernel_handle() {
        use reify_core::identity::RealizationNodeId;
        let rr = RealizationNodeId::new("BoxEdges", 0);
        let parent_hash: [u8; 32] = [0xDD; 32];
        let a = make_sub_handle(&rr, &parent_hash, SubKind::Edge, 2, GeometryHandleId(100));
        let b = make_sub_handle(&rr, &parent_hash, SubKind::Edge, 2, GeometryHandleId(999));
        assert_eq!(
            a, b,
            "same (parent, kind, index) must be EQUAL regardless of kernel_handle"
        );
    }
}
