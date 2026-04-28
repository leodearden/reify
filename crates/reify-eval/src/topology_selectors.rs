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

use reify_types::{
    Diagnostic, DiagnosticCode, DiagnosticLabel, FeatureTag, FeatureTagTable, GeometryHandleId,
    GeometryKernel, GeometryQuery, QueryError, SourceSpan, Value,
};

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
    value: Value,
) -> Result<f64, QueryError> {
    match value {
        Value::Real(x) => Ok(x),
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
fn query_per_subshape<K: GeometryKernel + ?Sized, F>(
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
    let values = query_per_subshape(kernel, &edges, "edges_by_length", GeometryQuery::EdgeLength)?;
    let mut out = Vec::with_capacity(edges.len());
    for (id, value) in edges.iter().zip(values) {
        let len = expect_real("EdgeLength", *id, value)?;
        if len >= min_m && len <= max_m {
            out.push(*id);
        }
    }
    Ok(out)
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
    let values = query_per_subshape(kernel, &faces, "faces_by_area", GeometryQuery::SurfaceArea)?;
    let mut out = Vec::with_capacity(faces.len());
    for (id, value) in faces.iter().zip(values) {
        let area = expect_real("SurfaceArea", *id, value)?;
        if area >= min_m2 && area <= max_m2 {
            out.push(*id);
        }
    }
    Ok(out)
}

/// Parse a `Value::String` that the kernel formatted as JSON
/// `{"x":...,"y":...,"z":...}` (the Centroid / EdgeTangent / FaceNormal
/// encoding) into an `[f64; 3]`.
///
/// Returns `QueryError::QueryFailed` on any deviation from the expected
/// shape (non-string Value, malformed JSON, missing numeric fields).
fn parse_xyz_value(value: &Value, query_label: &str) -> Result<[f64; 3], QueryError> {
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
fn parse_xyz_json(s: &str) -> Option<[f64; 3]> {
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
fn parse_flat_number_object<F>(s: &str, mut on_pair: F) -> Option<()>
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
fn normalize3(v: [f64; 3]) -> Option<[f64; 3]> {
    let mag = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if !mag.is_finite() || mag < f64::EPSILON {
        return None;
    }
    Some([v[0] / mag, v[1] / mag, v[2] / mag])
}

/// Dot product of two 3-vectors.
fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
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
    let target = normalize3(target).ok_or_else(|| {
        QueryError::QueryFailed(
            "faces_by_normal: target direction must be non-zero and finite".into(),
        )
    })?;
    let faces = kernel.extract_faces(handle)?;
    let values = query_per_subshape(kernel, &faces, "faces_by_normal", GeometryQuery::FaceNormal)?;
    let mut out = Vec::with_capacity(faces.len());
    for (id, normal_value) in faces.iter().zip(values) {
        let raw = parse_xyz_value(&normal_value, "FaceNormal")?;
        let normal = normalize3(raw).ok_or_else(|| {
            QueryError::QueryFailed(format!(
                "FaceNormal({:?}) returned a degenerate (near-zero) normal",
                id
            ))
        })?;
        let cos = dot3(normal, target).clamp(-1.0, 1.0);
        let angle = cos.acos();
        if angle <= angular_tol_rad {
            out.push(*id);
        }
    }
    Ok(out)
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
    let axis = normalize3(axis).ok_or_else(|| {
        QueryError::QueryFailed(
            "edges_parallel_to: axis direction must be non-zero and finite".into(),
        )
    })?;
    // Threshold on |dot|: edges accepted iff |t · axis| >= cos(tol).
    let cos_tol = angular_tol_rad.cos();
    let edges = kernel.extract_edges(handle)?;
    let values = query_per_subshape(kernel, &edges, "edges_parallel_to", GeometryQuery::EdgeTangent)?;
    let mut out = Vec::with_capacity(edges.len());
    for (id, tan_value) in edges.iter().zip(values) {
        let raw = parse_xyz_value(&tan_value, "EdgeTangent")?;
        let tan = normalize3(raw).ok_or_else(|| {
            QueryError::QueryFailed(format!(
                "EdgeTangent({:?}) returned a degenerate (near-zero) tangent",
                id
            ))
        })?;
        let abs_dot = dot3(tan, axis).abs();
        // Note: cos is monotone-decreasing on [0, π], so the condition
        // angle <= tol is equivalent to cos(angle) >= cos(tol). For the
        // sign-tolerant variant we use |cos|.
        if abs_dot >= cos_tol {
            out.push(*id);
        }
    }
    Ok(out)
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
    let values = query_per_subshape(kernel, &edges, "edges_at_height", GeometryQuery::BoundingBox)?;
    let mut out = Vec::with_capacity(edges.len());
    for (id, bbox_value) in edges.iter().zip(values) {
        let (zmin, zmax) = parse_bbox_z_extents(&bbox_value)?;
        if (zmin - z_m).abs() <= tol_m && (zmax - z_m).abs() <= tol_m {
            out.push(*id);
        }
    }
    Ok(out)
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
    // Record a per-edge tag for every extracted edge (before filtering).
    for (i, edge_id) in edges.iter().enumerate() {
        table.record(
            *edge_id,
            FeatureTag {
                source_span: parent_tag.source_span,
                step_kind: parent_tag.step_kind,
                sub_index: i as u32,
            },
        );
    }
    let values = query_per_subshape(
        kernel,
        &edges,
        "edges_at_height_with_tags",
        GeometryQuery::BoundingBox,
    )?;
    let mut out = Vec::with_capacity(edges.len());
    for (id, bbox_value) in edges.iter().zip(values) {
        let (zmin, zmax) = parse_bbox_z_extents(&bbox_value)?;
        if (zmin - z_m).abs() <= tol_m && (zmax - z_m).abs() <= tol_m {
            out.push(*id);
        }
    }
    Ok(out)
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
/// `candidates` must be deduplicated; duplicate handles inflate the match count
/// and produce spurious split-topology warnings.
pub fn resolve_unique_by_tag(
    table: &FeatureTagTable,
    candidates: &[GeometryHandleId],
    target: FeatureTag,
    selector_span: SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<GeometryHandleId> {
    let mut found: Option<GeometryHandleId> = None;
    let mut n: usize = 0;
    for &id in candidates {
        if table.lookup(id) == Some(&target) {
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
fn parse_bbox_z_extents(value: &Value) -> Result<(f64, f64), QueryError> {
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
fn parse_bbox_z_extents_json(s: &str) -> Option<(f64, f64)> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use reify_types::{
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
    }

    impl CountingKernel {
        fn new() -> Self {
            CountingKernel {
                query_calls: AtomicUsize::new(0),
                query_many_calls: AtomicUsize::new(0),
                edges: Vec::new(),
                faces: Vec::new(),
                responses: HashMap::new(),
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
        fn execute(
            &mut self,
            _op: &GeometryOp,
        ) -> Result<GeometryHandle, GeometryError> {
            unimplemented!("CountingKernel does not implement execute")
        }

        fn query(&self, query: &GeometryQuery) -> Result<Value, QueryError> {
            self.query_calls.fetch_add(1, Ordering::SeqCst);
            self.lookup(query)
        }

        fn query_many(
            &self,
            queries: &[GeometryQuery],
        ) -> Result<Vec<Value>, QueryError> {
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
            unimplemented!("CountingKernel does not implement tessellate")
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
            .with_response(face_ids[0], Value::String("{\"x\":0,\"y\":0,\"z\":1}".into()))
            .with_response(face_ids[1], Value::String("{\"x\":1,\"y\":0,\"z\":0}".into()))
            .with_response(
                face_ids[2],
                Value::String("{\"x\":0,\"y\":0,\"z\":-1}".into()),
            );

        let source = GeometryHandleId(1);
        let result = faces_by_normal(
            &mut kernel,
            source,
            [0.0, 0.0, 1.0],
            1f64.to_radians(),
        )
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
            .with_response(edge_ids[0], Value::String("{\"x\":1,\"y\":0,\"z\":0}".into()))
            .with_response(
                edge_ids[1],
                Value::String("{\"x\":-1,\"y\":0,\"z\":0}".into()),
            )
            .with_response(edge_ids[2], Value::String("{\"x\":0,\"y\":1,\"z\":0}".into()));

        let source = GeometryHandleId(1);
        let result = edges_parallel_to(
            &mut kernel,
            source,
            [1.0, 0.0, 0.0],
            1f64.to_radians(),
        )
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
        let result = faces_by_area(&mut kernel, source, 199e-6, 201e-6)
            .expect("selector should succeed");

        assert_eq!(result, vec![face_ids[0]], "expected only the 200e-6 m^2 face");
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
        fn execute(
            &mut self,
            _op: &GeometryOp,
        ) -> Result<GeometryHandle, GeometryError> {
            unimplemented!("FixedReplyQueryManyKernel does not implement execute")
        }

        fn query(&self, _query: &GeometryQuery) -> Result<Value, QueryError> {
            unimplemented!("FixedReplyQueryManyKernel only supports query_many")
        }

        fn query_many(
            &self,
            _queries: &[GeometryQuery],
        ) -> Result<Vec<Value>, QueryError> {
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
        let err = expect_real("EdgeLength", id, Value::String("not a number".into()))
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
        let mut kernel = CountingKernel::new()
            .with_edges(edge_ids.clone())
            .with_response(edge_ids[0], Value::Real(0.001))
            .with_response(edge_ids[1], Value::Real(0.002))
            .with_response(edge_ids[2], Value::Real(0.003));

        let values = query_per_subshape(
            &mut kernel,
            &edge_ids,
            "test_label",
            GeometryQuery::EdgeLength,
        )
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
        let mut kernel = FixedReplyQueryManyKernel {
            edges: edge_ids.clone(),
            canned_reply: vec![Value::Real(0.001), Value::Real(0.002)],
        };

        let err = query_per_subshape(
            &mut kernel,
            &edge_ids,
            "my_selector",
            GeometryQuery::EdgeLength,
        )
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
        use reify_types::{Diagnostic, FeatureTag, FeatureTagTable, SourceSpan, StepKind};

        let id1 = GeometryHandleId(1);
        let id2 = GeometryHandleId(2);
        let id3 = GeometryHandleId(3);

        let shared_span = SourceSpan::new(0, 10);
        let tag1 = FeatureTag { source_span: shared_span, step_kind: StepKind::Primitive, sub_index: 0 };
        let tag2 = FeatureTag { source_span: shared_span, step_kind: StepKind::Primitive, sub_index: 1 };
        let tag3 = FeatureTag { source_span: shared_span, step_kind: StepKind::Primitive, sub_index: 2 };

        let mut table = FeatureTagTable::default();
        table.record(id1, tag1);
        table.record(id2, tag2);
        table.record(id3, tag3);

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let selector_span = SourceSpan::new(10, 20);
        let result = resolve_unique_by_tag(&table, &[id1, id2, id3], tag2, selector_span, &mut diagnostics);

        assert_eq!(result, Some(id2), "should return the uniquely-matching handle");
        assert!(diagnostics.is_empty(), "no diagnostics on a clean unique match");
    }

    /// Zero-match path: no candidates carry the target tag.
    /// Resolver must return `None` and push exactly one `TopologyTagStale` warning
    /// with labels pointing at both the selector call site and the tag origin.
    #[test]
    fn resolve_unique_by_tag_zero_matches_emits_warning_and_returns_none() {
        use reify_types::{
            Diagnostic, DiagnosticCode, FeatureTag, FeatureTagTable, Severity, SourceSpan, StepKind,
        };

        let id1 = GeometryHandleId(10);
        let id2 = GeometryHandleId(11);

        // Both handles carry a non-target tag (sub_index differs from target).
        let tag_source_span = SourceSpan::new(100, 110);
        let tag1 = FeatureTag { source_span: tag_source_span, step_kind: StepKind::Boolean, sub_index: 5 };
        let tag2 = FeatureTag { source_span: tag_source_span, step_kind: StepKind::Boolean, sub_index: 6 };

        let mut table = FeatureTagTable::default();
        table.record(id1, tag1);
        table.record(id2, tag2);

        // Target tag is distinct from both (sub_index 99 not present).
        let target_tag = FeatureTag { source_span: tag_source_span, step_kind: StepKind::Boolean, sub_index: 99 };
        let selector_span = SourceSpan::new(200, 210);

        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = resolve_unique_by_tag(&table, &[id1, id2], target_tag, selector_span, &mut diagnostics);

        assert!(result.is_none(), "zero matches should return None");
        assert_eq!(diagnostics.len(), 1, "exactly one diagnostic on zero matches");

        let diag = &diagnostics[0];
        assert_eq!(diag.severity, Severity::Warning, "should be a warning");
        assert_eq!(diag.code, Some(DiagnosticCode::TopologyTagStale), "must carry TopologyTagStale code");
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
        use reify_types::{
            Diagnostic, DiagnosticCode, FeatureTag, FeatureTagTable, Severity, SourceSpan, StepKind,
        };

        let id1 = GeometryHandleId(20);
        let id2 = GeometryHandleId(21);
        let id3 = GeometryHandleId(22);

        // All three handles carry the SAME target tag — ambiguous split scenario.
        let tag_source_span = SourceSpan::new(50, 60);
        let target_tag = FeatureTag { source_span: tag_source_span, step_kind: StepKind::Sweep, sub_index: 7 };

        let mut table = FeatureTagTable::default();
        table.record(id1, target_tag);
        table.record(id2, target_tag);
        table.record(id3, target_tag);

        let selector_span = SourceSpan::new(300, 310);
        let mut diagnostics: Vec<Diagnostic> = Vec::new();
        let result = resolve_unique_by_tag(&table, &[id1, id2, id3], target_tag, selector_span, &mut diagnostics);

        assert!(result.is_none(), "multiple matches should return None");
        assert_eq!(diagnostics.len(), 1, "must fire exactly one diagnostic regardless of match count");

        let diag = &diagnostics[0];
        assert_eq!(diag.severity, Severity::Warning, "should be a warning");
        assert_eq!(diag.code, Some(DiagnosticCode::TopologyTagStale), "must carry TopologyTagStale code");
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
        use reify_types::{Diagnostic, FeatureTag, FeatureTagTable, SourceSpan, StepKind};

        let id1 = GeometryHandleId(50);

        let tag_source_span = SourceSpan::new(400, 410);
        let target_tag = FeatureTag { source_span: tag_source_span, step_kind: StepKind::Primitive, sub_index: 0 };

        let mut table = FeatureTagTable::default();
        table.record(id1, target_tag);

        let selector_span = SourceSpan::new(500, 510);
        let mut diagnostics: Vec<Diagnostic> = Vec::new();

        // Pass the SAME id three times — an unguarded resolver would count n=3 and
        // emit a spurious W_TOPOLOGY_TAG_STALE warning instead of returning Some(id1).
        let result = resolve_unique_by_tag(&table, &[id1, id1, id1], target_tag, selector_span, &mut diagnostics);

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
}
