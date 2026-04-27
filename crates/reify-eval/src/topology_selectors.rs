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
    let mut out = Vec::with_capacity(edges.len());
    for id in edges {
        let len = match kernel.query(&GeometryQuery::EdgeLength(id))? {
            Value::Real(l) => l,
            other => {
                return Err(QueryError::QueryFailed(format!(
                    "EdgeLength({:?}) returned non-real value: {:?}",
                    id, other
                )));
            }
        };
        if len >= min_m && len <= max_m {
            out.push(id);
        }
    }
    Ok(out)
}
