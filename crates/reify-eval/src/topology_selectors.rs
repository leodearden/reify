//! Filtered topology selectors composed over `GeometryKernel::extract_edges`
//! / `extract_faces` and per-sub-shape `GeometryQuery` reads (task 318).
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
//! All returned `Vec<GeometryHandleId>`s preserve the kernel's canonical
//! sub-shape order (from `TopExp::MapShapes`), filtered to those satisfying
//! the predicate.
//!
//! All length / area / coordinate filter parameters are in SI base units
//! (metres, square metres). Angular tolerances are in radians (matching
//! the rest of reify's geometry kernel — see `revolve` / `rotate_shape`
//! which also take `angle_rad`).

use reify_types::{GeometryHandleId, GeometryKernel, GeometryQuery, QueryError, Value};

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
    let queries: Vec<GeometryQuery> = edges
        .iter()
        .map(|id| GeometryQuery::EdgeLength(*id))
        .collect();
    let values = kernel.query_many(&queries)?;
    let mut out = Vec::with_capacity(edges.len());
    for (id, value) in edges.iter().zip(values) {
        let len = match value {
            Value::Real(l) => l,
            other => {
                return Err(QueryError::QueryFailed(format!(
                    "EdgeLength({:?}) returned non-real value: {:?}",
                    id, other
                )));
            }
        };
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
    let mut out = Vec::with_capacity(faces.len());
    for id in faces {
        let area = match kernel.query(&GeometryQuery::SurfaceArea(id))? {
            Value::Real(a) => a,
            other => {
                return Err(QueryError::QueryFailed(format!(
                    "SurfaceArea({:?}) returned non-real value: {:?}",
                    id, other
                )));
            }
        };
        if area >= min_m2 && area <= max_m2 {
            out.push(id);
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
    let mut out = Vec::with_capacity(faces.len());
    for id in faces {
        let normal_value = kernel.query(&GeometryQuery::FaceNormal(id))?;
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
            out.push(id);
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
    let mut out = Vec::with_capacity(edges.len());
    for id in edges {
        let tan_value = kernel.query(&GeometryQuery::EdgeTangent(id))?;
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
            out.push(id);
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
    let mut out = Vec::with_capacity(edges.len());
    for id in edges {
        let bbox_value = kernel.query(&GeometryQuery::BoundingBox(id))?;
        let (zmin, zmax) = parse_bbox_z_extents(&bbox_value)?;
        if (zmin - z_m).abs() <= tol_m && (zmax - z_m).abs() <= tol_m {
            out.push(id);
        }
    }
    Ok(out)
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
        ExportError, ExportFormat, GeometryError, GeometryHandle, GeometryOp, Mesh, ReprKind,
        TessError,
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

    /// Sanity check: the CountingKernel must compile against the trait.
    /// (Doubles as a tiny smoke test that `Send + Sync` is satisfied.)
    const _: fn() = || {
        fn must_be_send_sync<T: Send + Sync>() {}
        must_be_send_sync::<CountingKernel>();
        // ReprKind is unused in tests but pulled into scope so the import
        // doesn't drift if a future test uses it.
        let _ = ReprKind::Edge;
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
}
