//! Stub types for when OCCT libraries are not available at build time.
//!
//! These provide the same public API surface as the real OcctKernel and
//! OcctKernelHandle, but all operations return errors. This allows
//! downstream crates to compile and fail gracefully at runtime.

use crate::{
    BooleanOpHistoryRecords, Curvature, LocalFeatureOpHistoryRecords, LoftOpHistoryRecords,
    SweepOpHistoryRecords,
};
use reify_ir::{AttributeHistory, ExportError, ExportFormat, GeometryError, GeometryHandle, GeometryHandleId, GeometryKernel, GeometryOp, GeometryQuery, Mesh, OpaqueState, QueryError, TessError, Value, WarmStartable};

/// Stub topology cache build counts (OCCT not available).
#[derive(Debug, PartialEq, Eq)]
pub struct TopologyCacheBuildCounts {
    pub face_map_builds: u32,
    pub edge_map_builds: u32,
    pub edge_face_map_builds: u32,
}

const NOT_AVAILABLE: &str = "OCCT libraries not available at build time";

/// Stub OpenCASCADE kernel — all operations return errors.
pub struct OcctKernel {
    _private: (),
}

impl OcctKernel {
    pub fn new() -> Self {
        Self { _private: () }
    }

    pub fn execute(&mut self, _op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
        Err(GeometryError::OperationFailed(NOT_AVAILABLE.into()))
    }

    pub fn query(&self, _query: &GeometryQuery) -> Result<Value, QueryError> {
        Err(QueryError::QueryFailed(NOT_AVAILABLE.into()))
    }

    pub fn export(
        &self,
        _handle: GeometryHandleId,
        _format: ExportFormat,
        _writer: &mut dyn std::io::Write,
    ) -> Result<(), ExportError> {
        Err(ExportError::FormatError(NOT_AVAILABLE.into()))
    }

    pub fn tessellate(
        &self,
        _handle: GeometryHandleId,
        _tolerance: f64,
    ) -> Result<Mesh, TessError> {
        Err(TessError::TessellationFailed(NOT_AVAILABLE.into()))
    }

    /// Returns [`GeometryError::InvalidReference`] for every handle.
    ///
    /// The stub registers no shapes, so every handle is unknown by definition.
    /// This matches the real impl's documented contract (see `lib.rs`
    /// `topology_cache_build_counts`), which also returns `InvalidReference`
    /// for unknown handles via `get_shape`. Returning the same error variant
    /// keeps callers that pattern-match on `InvalidReference` compatible
    /// across `has_occt` and `!has_occt` builds without special-casing.
    pub fn topology_cache_build_counts(
        &self,
        handle: GeometryHandleId,
    ) -> Result<TopologyCacheBuildCounts, GeometryError> {
        Err(GeometryError::InvalidReference(handle))
    }

    /// Stub topology-extraction selector — always errors because OCCT is
    /// unavailable. Mirrors the real `OcctKernel::extract_edges` signature
    /// so call sites compile under both `has_occt` and `!has_occt`.
    pub fn extract_edges(
        &mut self,
        _handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        Err(QueryError::QueryFailed(NOT_AVAILABLE.into()))
    }

    /// Stub topology-extraction selector — always errors because OCCT is
    /// unavailable. Mirrors the real `OcctKernel::extract_faces` signature
    /// so call sites compile under both `has_occt` and `!has_occt`.
    pub fn extract_faces(
        &mut self,
        _handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        Err(QueryError::QueryFailed(NOT_AVAILABLE.into()))
    }

    /// Stub topology-extraction selector — always errors because OCCT is
    /// unavailable. Mirrors the real `OcctKernel::extract_vertices` signature
    /// so call sites compile under both `has_occt` and `!has_occt`.
    pub fn extract_vertices(
        &mut self,
        _handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        Err(QueryError::QueryFailed(NOT_AVAILABLE.into()))
    }

    /// Stub interference probe — always errors because OCCT is unavailable.
    /// Mirrors the real `OcctKernel::shapes_intersect` signature so call sites
    /// compile under both `has_occt` and `!has_occt`.
    pub fn shapes_intersect(
        &self,
        _a: GeometryHandleId,
        _b: GeometryHandleId,
    ) -> Result<bool, QueryError> {
        Err(QueryError::QueryFailed(NOT_AVAILABLE.into()))
    }

    /// Stub transform-aware interference probe — always errors because OCCT is unavailable.
    /// Mirrors the real `OcctKernel::interferes_with_transform` signature so call sites
    /// compile under both `has_occt` and `!has_occt`.
    pub fn interferes_with_transform(
        &self,
        _a: GeometryHandleId,
        _b: GeometryHandleId,
        _t_rel: &crate::Transform3,
    ) -> Result<bool, QueryError> {
        Err(QueryError::QueryFailed(NOT_AVAILABLE.into()))
    }

    /// Stub clearance probe — always errors because OCCT is unavailable.
    /// Mirrors the real `OcctKernel::min_clearance` signature so call sites
    /// compile under both `has_occt` and `!has_occt`.
    pub fn min_clearance(
        &self,
        _a: GeometryHandleId,
        _b: GeometryHandleId,
    ) -> Result<f64, QueryError> {
        Err(QueryError::QueryFailed(NOT_AVAILABLE.into()))
    }

    /// Stub transform-aware distance probe — always errors because OCCT is unavailable.
    /// Mirrors the real `OcctKernel::distance_with_transform` signature so call sites
    /// compile under both `has_occt` and `!has_occt`.
    pub fn distance_with_transform(
        &self,
        _a: GeometryHandleId,
        _b: GeometryHandleId,
        _t_rel: &crate::Transform3,
    ) -> Result<f64, QueryError> {
        Err(QueryError::QueryFailed(NOT_AVAILABLE.into()))
    }

    /// Stub rigid-transform-application primitive — always errors because OCCT
    /// is unavailable. Mirrors the real `OcctKernel::apply_transform_to_handle`
    /// signature so call sites compile under both `has_occt` and `!has_occt`
    /// (sub-placement PRD §5, task 3901).
    pub fn apply_transform_to_handle(
        &mut self,
        _handle: GeometryHandleId,
        _t: &crate::Transform3,
    ) -> Result<GeometryHandleId, GeometryError> {
        Err(GeometryError::OperationFailed(NOT_AVAILABLE.into()))
    }

    /// Stub closest-point probe — always errors because OCCT is unavailable.
    /// Mirrors the real `OcctKernel::closest_point_on_shape` signature so call sites
    /// compile under both `has_occt` and `!has_occt`.
    pub fn closest_point_on_shape(
        &self,
        _handle: GeometryHandleId,
        _px: f64,
        _py: f64,
        _pz: f64,
    ) -> Result<[f64; 3], QueryError> {
        Err(QueryError::QueryFailed(NOT_AVAILABLE.into()))
    }

    /// Stub vertex-position probe — always errors because OCCT is unavailable.
    /// Mirrors the real `OcctKernel::vertex_point` signature so call sites
    /// compile under both `has_occt` and `!has_occt`.
    pub fn vertex_point(&self, _handle: GeometryHandleId) -> Result<[f64; 3], QueryError> {
        Err(QueryError::QueryFailed(NOT_AVAILABLE.into()))
    }

    /// Stub surface-angle probe — always errors because OCCT is unavailable.
    /// Mirrors the real `OcctKernel::surface_angle` signature so call sites
    /// compile under both `has_occt` and `!has_occt`.
    pub fn surface_angle(
        &self,
        _face_a: GeometryHandleId,
        _face_b: GeometryHandleId,
    ) -> Result<f64, QueryError> {
        Err(QueryError::QueryFailed(NOT_AVAILABLE.into()))
    }

    /// Stub surface-normal-at probe — always errors because OCCT is unavailable.
    /// Mirrors the real `OcctKernel::surface_normal_at` signature so call sites
    /// compile under both `has_occt` and `!has_occt`.
    pub fn surface_normal_at(
        &self,
        _handle: GeometryHandleId,
        _u: f64,
        _v: f64,
    ) -> Result<[f64; 3], QueryError> {
        Err(QueryError::QueryFailed(NOT_AVAILABLE.into()))
    }

    /// Stub surface-normal-at-point probe — always errors because OCCT is unavailable.
    /// Mirrors the real `OcctKernel::surface_normal_at_point` signature so call sites
    /// compile under both `has_occt` and `!has_occt`.
    pub fn surface_normal_at_point(
        &self,
        _handle: GeometryHandleId,
        _px: f64,
        _py: f64,
        _pz: f64,
    ) -> Result<[f64; 3], QueryError> {
        Err(QueryError::QueryFailed(NOT_AVAILABLE.into()))
    }

    /// Stub curvature-at probe — always errors because OCCT is unavailable.
    /// Mirrors the real `OcctKernel::curvature_at` signature so call sites
    /// compile under both `has_occt` and `!has_occt`.
    pub fn curvature_at(
        &self,
        _handle: GeometryHandleId,
        _u: f64,
        _v: f64,
    ) -> Result<Curvature, QueryError> {
        Err(QueryError::QueryFailed(NOT_AVAILABLE.into()))
    }

    /// Stub curve-curvature-at probe — always errors because OCCT is unavailable.
    /// Mirrors the real `OcctKernel::curve_curvature_at` signature so call sites
    /// compile under both `has_occt` and `!has_occt`.
    pub fn curve_curvature_at(
        &self,
        _handle: GeometryHandleId,
        _px: f64,
        _py: f64,
        _pz: f64,
    ) -> Result<f64, QueryError> {
        Err(QueryError::QueryFailed(NOT_AVAILABLE.into()))
    }

    /// Stub point-on-shape membership probe — always errors because OCCT is unavailable.
    /// Mirrors the real `OcctKernel::point_on_shape` signature so call sites
    /// compile under both `has_occt` and `!has_occt`.
    ///
    /// The real implementation uses `BRepExtrema_DistShapeShape(shape, vertex)`
    /// returning `dist.Value() <= tolerance`. See `lib.rs` for the full contract,
    /// including: the OCCT solid-overlap behavior (interior solid points return `true`
    /// because `dist = 0` under overlap); the recommended `Precision::Confusion()`
    /// (~1e-7) default tolerance; the tolerance precondition (non-negative finite
    /// `f64`); and the naming caveat that this primitive cannot distinguish on-surface
    /// from inside-solid for `TopoDS_Solid` inputs.
    pub fn point_on_shape(
        &self,
        _handle: GeometryHandleId,
        _px: f64,
        _py: f64,
        _pz: f64,
        _tolerance: f64,
    ) -> Result<bool, QueryError> {
        Err(QueryError::QueryFailed(NOT_AVAILABLE.into()))
    }

    /// Stub contains-solid membership probe — always errors because OCCT is unavailable.
    /// Mirrors the real `OcctKernel::contains` signature so call sites compile
    /// under both `has_occt` and `!has_occt`.
    ///
    /// The real implementation uses `BRepClass3d_SolidClassifier(shape).Perform(pnt, tol)`
    /// and returns `true` for `TopAbs_IN || TopAbs_ON`. See `lib.rs` for the full contract,
    /// including the tolerance precondition (non-negative finite `f64`) and the
    /// `DEFAULT_CONTAINS_TOLERANCE_M` (= `DEFAULT_POINT_ON_SHAPE_TOLERANCE_M`, ~1e-7) default.
    pub fn contains(
        &self,
        _handle: GeometryHandleId,
        _px: f64,
        _py: f64,
        _pz: f64,
        _tolerance: f64,
    ) -> Result<bool, QueryError> {
        Err(QueryError::QueryFailed(NOT_AVAILABLE.into()))
    }
}

impl Default for OcctKernel {
    fn default() -> Self {
        Self::new()
    }
}

impl WarmStartable for OcctKernel {
    fn warm_state(&self) -> Option<OpaqueState> {
        None
    }

    fn with_warm_state(&mut self, _state: OpaqueState) {
        // No-op: OCCT not available, silently ignore per trait contract.
    }
}

/// Stub thread-safe handle — implements GeometryKernel with error returns.
pub struct OcctKernelHandle {
    _private: (),
}

// Safety: stub contains no mutable state, is trivially Send + Sync.
unsafe impl Send for OcctKernelHandle {}
unsafe impl Sync for OcctKernelHandle {}

impl OcctKernelHandle {
    /// Create a stub handle (no thread is spawned).
    pub fn spawn() -> Self {
        Self { _private: () }
    }

    pub fn execute(&self, _op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
        Err(GeometryError::OperationFailed(NOT_AVAILABLE.into()))
    }

    pub fn query(&self, _query: &GeometryQuery) -> Result<Value, QueryError> {
        Err(QueryError::QueryFailed(NOT_AVAILABLE.into()))
    }

    pub fn export(
        &self,
        _handle: GeometryHandleId,
        _format: ExportFormat,
        _writer: &mut dyn std::io::Write,
    ) -> Result<(), ExportError> {
        Err(ExportError::FormatError(NOT_AVAILABLE.into()))
    }

    pub fn tessellate(
        &self,
        _handle: GeometryHandleId,
        _tolerance: f64,
    ) -> Result<Mesh, TessError> {
        Err(TessError::TessellationFailed(NOT_AVAILABLE.into()))
    }

    /// Stub `boolean_fuse_with_history` — always errors because OCCT is
    /// unavailable. Mirrors the real `OcctKernelHandle::boolean_fuse_with_history`
    /// signature so call sites compile under both `has_occt` and `!has_occt`.
    /// Part of v0.2 persistent-naming-v2 (task 2590, step-14).
    pub fn boolean_fuse_with_history(
        &self,
        _left: GeometryHandleId,
        _right: GeometryHandleId,
    ) -> Result<(GeometryHandleId, BooleanOpHistoryRecords), GeometryError> {
        Err(GeometryError::OperationFailed(NOT_AVAILABLE.into()))
    }

    /// Stub `boolean_cut_with_history` — always errors because OCCT is
    /// unavailable. Mirrors the real `OcctKernelHandle::boolean_cut_with_history`
    /// signature so call sites compile under both `has_occt` and `!has_occt`.
    /// Part of v0.2 persistent-naming-v2 (task 2656, step-2).
    pub fn boolean_cut_with_history(
        &self,
        _left: GeometryHandleId,
        _right: GeometryHandleId,
    ) -> Result<(GeometryHandleId, BooleanOpHistoryRecords), GeometryError> {
        Err(GeometryError::OperationFailed(NOT_AVAILABLE.into()))
    }

    /// Stub `boolean_common_with_history` — always errors because OCCT is
    /// unavailable. Mirrors the real `OcctKernelHandle::boolean_common_with_history`
    /// signature so call sites compile under both `has_occt` and `!has_occt`.
    /// Part of v0.2 persistent-naming-v2 (task 2656, step-4).
    pub fn boolean_common_with_history(
        &self,
        _left: GeometryHandleId,
        _right: GeometryHandleId,
    ) -> Result<(GeometryHandleId, BooleanOpHistoryRecords), GeometryError> {
        Err(GeometryError::OperationFailed(NOT_AVAILABLE.into()))
    }

    /// Stub `execute_with_history` — always errors because OCCT is
    /// unavailable. Mirrors the real `OcctKernelHandle::execute_with_history`
    /// signature so call sites compile under both `has_occt` and `!has_occt`.
    /// Part of v0.2 persistent-naming-v2 (task 5a / #2573, step-8).
    pub fn execute_with_history(
        &self,
        _op: &GeometryOp,
    ) -> Result<(GeometryHandle, AttributeHistory), GeometryError> {
        Err(GeometryError::OperationFailed(NOT_AVAILABLE.into()))
    }

    /// Stub `extrude_with_history` — always errors because OCCT is
    /// unavailable. Mirrors the real `OcctKernelHandle::extrude_with_history`
    /// signature so call sites compile under both `has_occt` and `!has_occt`.
    /// Part of v0.2 persistent-naming-v2 (task 5a / #2573, step-8).
    pub fn extrude_with_history(
        &self,
        _profile: GeometryHandleId,
        _distance: f64,
    ) -> Result<(GeometryHandleId, SweepOpHistoryRecords), GeometryError> {
        Err(GeometryError::OperationFailed(NOT_AVAILABLE.into()))
    }

    /// Stub `revolve_with_history` — always errors because OCCT is
    /// unavailable. Mirrors the real `OcctKernelHandle::revolve_with_history`
    /// signature so call sites compile under both `has_occt` and `!has_occt`.
    /// Part of v0.2 persistent-naming-v2 (task 5a / #2573, step-10).
    pub fn revolve_with_history(
        &self,
        _profile: GeometryHandleId,
        _axis_origin: [f64; 3],
        _axis_dir: [f64; 3],
        _angle_rad: f64,
    ) -> Result<(GeometryHandleId, SweepOpHistoryRecords), GeometryError> {
        Err(GeometryError::OperationFailed(NOT_AVAILABLE.into()))
    }

    /// Stub `sweep_with_history` — always errors because OCCT is
    /// unavailable. Mirrors the real `OcctKernelHandle::sweep_with_history`
    /// signature so call sites compile under both `has_occt` and `!has_occt`.
    /// Part of v0.2 persistent-naming-v2 (task 5b / #2619, step-4).
    pub fn sweep_with_history(
        &self,
        _profile: GeometryHandleId,
        _path: GeometryHandleId,
    ) -> Result<(GeometryHandleId, SweepOpHistoryRecords), GeometryError> {
        Err(GeometryError::OperationFailed(NOT_AVAILABLE.into()))
    }

    /// Stub `loft_with_history` — always errors because OCCT is
    /// unavailable. Mirrors the real `OcctKernelHandle::loft_with_history`
    /// signature so call sites compile under both `has_occt` and `!has_occt`.
    /// Part of v0.2 persistent-naming-v2 (task 5b / #2619, step-6).
    pub fn loft_with_history(
        &self,
        _profiles: &[GeometryHandleId],
    ) -> Result<(GeometryHandleId, LoftOpHistoryRecords), GeometryError> {
        Err(GeometryError::OperationFailed(NOT_AVAILABLE.into()))
    }

    /// Stub `fillet_with_history` — always errors because OCCT is
    /// unavailable. Mirrors the real `OcctKernelHandle::fillet_with_history`
    /// signature so call sites compile under both `has_occt` and `!has_occt`.
    /// Part of v0.2 persistent-naming-v2 (task 2655, step-2 / task 2821).
    pub fn fillet_with_history(
        &self,
        _shape: GeometryHandleId,
        _radius: f64,
    ) -> Result<(GeometryHandleId, LocalFeatureOpHistoryRecords), GeometryError> {
        Err(GeometryError::OperationFailed(NOT_AVAILABLE.into()))
    }

    /// Stub `chamfer_with_history` — always errors because OCCT is
    /// unavailable. Mirrors the real `OcctKernelHandle::chamfer_with_history`
    /// signature so call sites compile under both `has_occt` and `!has_occt`.
    /// Part of v0.2 persistent-naming-v2 (task 2655, step-6 / task 2821).
    pub fn chamfer_with_history(
        &self,
        _shape: GeometryHandleId,
        _distance: f64,
    ) -> Result<(GeometryHandleId, LocalFeatureOpHistoryRecords), GeometryError> {
        Err(GeometryError::OperationFailed(NOT_AVAILABLE.into()))
    }

    /// Stub `extract_vertices` — always errors because OCCT is unavailable.
    /// Mirrors the real `OcctKernelHandle::extract_vertices` inherent method
    /// so call sites compile under both `has_occt` and `!has_occt`.
    pub fn extract_vertices(
        &mut self,
        _handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        Err(QueryError::QueryFailed(NOT_AVAILABLE.into()))
    }

    /// No-op shutdown (no thread to join).
    pub async fn shutdown(self) {}
}

impl WarmStartable for OcctKernelHandle {
    fn warm_state(&self) -> Option<OpaqueState> {
        None
    }

    fn with_warm_state(&mut self, _state: OpaqueState) {
        // No-op: OCCT not available.
    }
}

impl GeometryKernel for OcctKernelHandle {
    fn execute(&mut self, op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
        OcctKernelHandle::execute(self, op)
    }

    fn query(&self, query: &GeometryQuery) -> Result<Value, QueryError> {
        OcctKernelHandle::query(self, query)
    }

    fn export(
        &self,
        handle: GeometryHandleId,
        format: ExportFormat,
        writer: &mut dyn std::io::Write,
    ) -> Result<(), ExportError> {
        OcctKernelHandle::export(self, handle, format, writer)
    }

    fn tessellate(&self, handle: GeometryHandleId, tolerance: f64) -> Result<Mesh, TessError> {
        OcctKernelHandle::tessellate(self, handle, tolerance)
    }

    /// Override the trait default to surface the OCCT-unavailable message
    /// (matches the inherent stub `OcctKernel::extract_edges`).
    fn extract_edges(
        &mut self,
        _handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        Err(QueryError::QueryFailed(NOT_AVAILABLE.into()))
    }

    /// Override the trait default to surface the OCCT-unavailable message
    /// (matches the inherent stub `OcctKernel::extract_faces`).
    fn extract_faces(
        &mut self,
        _handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        Err(QueryError::QueryFailed(NOT_AVAILABLE.into()))
    }

    /// Override the trait default to surface the OCCT-unavailable message
    /// (matches the inherent stub `OcctKernel::extract_vertices`).
    fn extract_vertices(
        &mut self,
        _handle: GeometryHandleId,
    ) -> Result<Vec<GeometryHandleId>, QueryError> {
        Err(QueryError::QueryFailed(NOT_AVAILABLE.into()))
    }

    /// Override the trait default to surface the OCCT-unavailable message
    /// (matches the inherent stub `OcctKernelHandle::execute_with_history`).
    /// Part of v0.2 persistent-naming-v2 (task 5a / #2573, step-8).
    fn execute_with_history(
        &mut self,
        _op: &GeometryOp,
    ) -> Result<(GeometryHandle, AttributeHistory), GeometryError> {
        Err(GeometryError::OperationFailed(NOT_AVAILABLE.into()))
    }
}

impl Drop for OcctKernelHandle {
    fn drop(&mut self) {
        // No-op: no thread to join.
    }
}

#[cfg(all(test, not(has_occt)))]
mod tests {
    use super::*;

    // `OcctKernel` (bare) has no `GeometryKernel` impl — only
    // `OcctKernelHandle` does — and is never boxed as `Box<dyn
    // GeometryKernel>` in production; the sole factory
    // (`register::occt_factory`) boxes `OcctKernelHandle::spawn()`. A second
    // `assert_kernel_contract!` instantiation for `OcctKernel` isn't possible
    // (no trait impl to exercise) and isn't needed (task 5110 review): its
    // inherent-method coverage lives in the bundled tests below.
    reify_test_support::assert_kernel_contract!(stub; OcctKernelHandle::spawn, "OCCT");

    /// Inherent `OcctKernel::topology_cache_build_counts` cross-cfg contract
    /// (task 2405): not expressible via the shared `GeometryKernel` suite
    /// above because this method isn't on the trait.
    #[test]
    fn stub_kernel_topology_cache_build_counts_returns_invalid_reference() {
        let kernel = OcctKernel::new();
        let bad_id = GeometryHandleId(42);
        match kernel.topology_cache_build_counts(bad_id) {
            Err(GeometryError::InvalidReference(id)) => {
                assert_eq!(
                    id, bad_id,
                    "InvalidReference should carry the bad handle id"
                );
            }
            Ok(c) => panic!(
                "expected Err(InvalidReference) for unknown handle, got Ok({:?})",
                c
            ),
            Err(other) => panic!(
                "expected Err(InvalidReference) for unknown handle, got Err({:?})",
                other
            ),
        }
    }

    /// Generates a `check_*_failed` helper for one error taxonomy: `Ok(())`
    /// when `result` is `Err($variant(msg))` and `msg` contains "OCCT",
    /// else a descriptive `Err(String)` (never a panic, so bundled
    /// `#[test]`s can collect every failed probe via
    /// [`assert_no_check_failures`] instead of stopping at the first).
    /// Collapses what were four hand-copied match blocks —
    /// `QueryError`/`GeometryError`/`ExportError`/`TessError` — differing
    /// only in the enum/variant matched (task 5110 review).
    macro_rules! check_failed {
        ($name:ident, $err_ty:ty, $variant:path, $variant_str:literal) => {
            fn $name<T: std::fmt::Debug>(
                result: Result<T, $err_ty>,
                method: &str,
            ) -> Result<(), String> {
                match result {
                    Err($variant(msg)) if msg.contains("OCCT") => Ok(()),
                    Err($variant(msg)) => {
                        Err(format!("{method} error should mention OCCT, got: {msg}"))
                    }
                    other => Err(format!(
                        "expected Err({variant}(_)) from {method}, got {other:?}",
                        variant = $variant_str,
                    )),
                }
            }
        };
    }

    // `QueryError` — mirrors `assert_all_error_taxonomy`'s matching for the
    // inherent probe surface the shared macro can't see.
    check_failed!(
        check_query_failed,
        QueryError,
        QueryError::QueryFailed,
        "QueryError::QueryFailed"
    );
    check_failed!(
        check_operation_failed,
        GeometryError,
        GeometryError::OperationFailed,
        "GeometryError::OperationFailed"
    );
    check_failed!(
        check_format_failed,
        ExportError,
        ExportError::FormatError,
        "ExportError::FormatError"
    );
    check_failed!(
        check_tess_failed,
        TessError,
        TessError::TessellationFailed,
        "TessError::TessellationFailed"
    );

    /// `Ok(())` when `state` is `None`, else a descriptive `Err(String)` —
    /// mirrors the `check_*_failed` collect pattern above for the one probe
    /// (`WarmStartable::warm_state`) that signals "not available" via
    /// `Option::None` rather than a `Result::Err` taxonomy (task 5110 review).
    fn check_warm_state_none(state: Option<OpaqueState>) -> Result<(), String> {
        match state {
            None => Ok(()),
            Some(_) => Err("stub warm_state should always be None".to_string()),
        }
    }

    /// Panics naming every failed check, so bundled probes report all
    /// violations from one run instead of stopping at the first (task 5110).
    fn assert_no_check_failures(failures: Vec<String>) {
        assert!(
            failures.is_empty(),
            "{} check(s) failed:\n{}",
            failures.len(),
            failures.join("\n")
        );
    }

    // Coverage below closes gaps the shared `assert_kernel_contract!` suite
    // above structurally cannot reach: `OcctKernel`'s inherent methods (a
    // separate type, never wired to the trait), either type's non-trait
    // methods (probes, with-history variants), and `OcctKernelHandle`'s
    // inherent `extract_vertices` (shadows its trait override). Deliberately
    // not a restoration of the deleted trait-surface suite, which stays
    // deleted per this task's `design_decisions` (task 5110). Each bundled
    // `#[test]` collects failures into a `Vec<String>` via
    // `assert_no_check_failures` so one run surfaces every regression
    // instead of short-circuiting on the first.

    /// `OcctKernel`'s core trait-shaped inherent methods: a separate type
    /// from `OcctKernelHandle`, so the shared suite provides no coverage.
    /// Named `_all_error_or_none` (not `_returns_error`) because it bundles
    /// one non-`Result` probe — `warm_state()`, which signals unavailability
    /// via `Option::None` — alongside the `Result`-returning methods (task
    /// 5110 review).
    #[test]
    fn stub_kernel_core_methods_all_error_or_none() {
        let mut kernel = OcctKernel::new();
        let mut failures = Vec::new();

        if let Err(e) = check_operation_failed(
            kernel.execute(&GeometryOp::Union {
                left: GeometryHandleId(1),
                right: GeometryHandleId(2),
            }),
            "execute",
        ) {
            failures.push(e);
        }
        if let Err(e) = check_query_failed(
            kernel.query(&GeometryQuery::Volume(GeometryHandleId(1))),
            "query",
        ) {
            failures.push(e);
        }

        let mut buf = Vec::new();
        if let Err(e) = check_format_failed(
            kernel.export(GeometryHandleId(1), ExportFormat::Step, &mut buf),
            "export",
        ) {
            failures.push(e);
        }

        if let Err(e) =
            check_tess_failed(kernel.tessellate(GeometryHandleId(1), 0.1), "tessellate")
        {
            failures.push(e);
        }

        if let Err(e) =
            check_query_failed(kernel.extract_edges(GeometryHandleId(1)), "extract_edges")
        {
            failures.push(e);
        }
        if let Err(e) =
            check_query_failed(kernel.extract_faces(GeometryHandleId(1)), "extract_faces")
        {
            failures.push(e);
        }
        if let Err(e) = check_query_failed(
            kernel.extract_vertices(GeometryHandleId(1)),
            "extract_vertices",
        ) {
            failures.push(e);
        }
        if let Err(e) = check_warm_state_none(kernel.warm_state()) {
            failures.push(e);
        }

        assert_no_check_failures(failures);
    }

    /// Full set of `OcctKernel`'s geometric-probe inherent methods (none are
    /// on `GeometryKernel`, so `assert_kernel_contract!` cannot see any of
    /// them), plus `apply_transform_to_handle` for the `GeometryError`-family
    /// case. Each probe's return value is checked at runtime via
    /// `check_query_failed`/`check_operation_failed` rather than trusted to
    /// compilation-only signature parity, so a probe that regresses to
    /// `Ok(_)` or a different error family fails here (task 5110 review).
    #[test]
    fn stub_kernel_probe_methods_return_not_available_error() {
        let mut kernel = OcctKernel::new();
        let mut failures = Vec::new();
        let identity = crate::Transform3 {
            qw: 1.0,
            qx: 0.0,
            qy: 0.0,
            qz: 0.0,
            tx: 0.0,
            ty: 0.0,
            tz: 0.0,
        };

        if let Err(e) = check_query_failed(
            kernel.closest_point_on_shape(GeometryHandleId(1), 0.0, 0.0, 0.0),
            "closest_point_on_shape",
        ) {
            failures.push(e);
        }
        if let Err(e) = check_query_failed(
            kernel.surface_angle(GeometryHandleId(1), GeometryHandleId(2)),
            "surface_angle",
        ) {
            failures.push(e);
        }
        if let Err(e) = check_query_failed(
            kernel.surface_normal_at(GeometryHandleId(1), 0.0, 0.0),
            "surface_normal_at",
        ) {
            failures.push(e);
        }
        if let Err(e) = check_query_failed(
            kernel.curvature_at(GeometryHandleId(1), 0.0, 0.0),
            "curvature_at",
        ) {
            failures.push(e);
        }
        if let Err(e) = check_query_failed(
            kernel.point_on_shape(
                GeometryHandleId(1),
                0.0,
                0.0,
                0.0,
                reify_ir::DEFAULT_POINT_ON_SHAPE_TOLERANCE_M,
            ),
            "point_on_shape",
        ) {
            failures.push(e);
        }
        if let Err(e) = check_query_failed(
            kernel.contains(
                GeometryHandleId(1),
                0.0,
                0.0,
                0.0,
                reify_ir::DEFAULT_CONTAINS_TOLERANCE_M,
            ),
            "contains",
        ) {
            failures.push(e);
        }
        if let Err(e) = check_query_failed(
            kernel.shapes_intersect(GeometryHandleId(1), GeometryHandleId(2)),
            "shapes_intersect",
        ) {
            failures.push(e);
        }
        if let Err(e) = check_query_failed(
            kernel.interferes_with_transform(GeometryHandleId(1), GeometryHandleId(2), &identity),
            "interferes_with_transform",
        ) {
            failures.push(e);
        }
        if let Err(e) = check_query_failed(
            kernel.min_clearance(GeometryHandleId(1), GeometryHandleId(2)),
            "min_clearance",
        ) {
            failures.push(e);
        }
        if let Err(e) = check_query_failed(
            kernel.distance_with_transform(GeometryHandleId(1), GeometryHandleId(2), &identity),
            "distance_with_transform",
        ) {
            failures.push(e);
        }
        if let Err(e) =
            check_query_failed(kernel.vertex_point(GeometryHandleId(1)), "vertex_point")
        {
            failures.push(e);
        }
        if let Err(e) = check_query_failed(
            kernel.surface_normal_at_point(GeometryHandleId(1), 0.0, 0.0, 0.0),
            "surface_normal_at_point",
        ) {
            failures.push(e);
        }
        if let Err(e) = check_query_failed(
            kernel.curve_curvature_at(GeometryHandleId(1), 0.0, 0.0, 0.0),
            "curve_curvature_at",
        ) {
            failures.push(e);
        }
        if let Err(e) = check_operation_failed(
            kernel.apply_transform_to_handle(GeometryHandleId(1), &identity),
            "apply_transform_to_handle",
        ) {
            failures.push(e);
        }

        assert_no_check_failures(failures);
    }

    /// `OcctKernel::query` for non-`Volume` variants (shared suite only
    /// probes `Volume`); guards a future per-variant `match` that forgets one.
    #[test]
    fn stub_kernel_query_variants_return_not_available_error() {
        let kernel = OcctKernel::new();
        let mut failures = Vec::new();

        if let Err(e) = check_query_failed(
            kernel.query(&GeometryQuery::EdgeLength(GeometryHandleId(1))),
            "query(EdgeLength)",
        ) {
            failures.push(e);
        }
        if let Err(e) = check_query_failed(
            kernel.query(&GeometryQuery::FaceNormal(GeometryHandleId(1))),
            "query(FaceNormal)",
        ) {
            failures.push(e);
        }
        if let Err(e) = check_query_failed(
            kernel.query(&GeometryQuery::EdgeTangent(GeometryHandleId(1))),
            "query(EdgeTangent)",
        ) {
            failures.push(e);
        }

        assert_no_check_failures(failures);
    }

    /// `OcctKernelHandle`'s full `*_with_history` inherent surface: the
    /// booleans (task 2590/2656), `execute_with_history` (task 5a / #2573,
    /// step-8), the sweep-family ops (task 5a/5b, #2573/#2619), and
    /// fillet/chamfer (task 2655/2821). None are on `GeometryKernel`, so
    /// the shared suite provides no coverage for any of them — previously
    /// only fillet/chamfer were exercised here, leaving the other eight
    /// variants untested by both the macro and the bespoke tests (task
    /// 5110 review).
    #[test]
    fn stub_handle_with_history_methods_return_not_available_error() {
        let handle = OcctKernelHandle::spawn();
        let mut failures = Vec::new();

        if let Err(e) = check_operation_failed(
            handle.boolean_fuse_with_history(GeometryHandleId(1), GeometryHandleId(2)),
            "boolean_fuse_with_history",
        ) {
            failures.push(e);
        }
        if let Err(e) = check_operation_failed(
            handle.boolean_cut_with_history(GeometryHandleId(1), GeometryHandleId(2)),
            "boolean_cut_with_history",
        ) {
            failures.push(e);
        }
        if let Err(e) = check_operation_failed(
            handle.boolean_common_with_history(GeometryHandleId(1), GeometryHandleId(2)),
            "boolean_common_with_history",
        ) {
            failures.push(e);
        }
        if let Err(e) = check_operation_failed(
            handle.execute_with_history(&GeometryOp::Union {
                left: GeometryHandleId(1),
                right: GeometryHandleId(2),
            }),
            "execute_with_history",
        ) {
            failures.push(e);
        }
        if let Err(e) = check_operation_failed(
            handle.extrude_with_history(GeometryHandleId(1), 1.0),
            "extrude_with_history",
        ) {
            failures.push(e);
        }
        if let Err(e) = check_operation_failed(
            handle.revolve_with_history(GeometryHandleId(1), [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], 1.0),
            "revolve_with_history",
        ) {
            failures.push(e);
        }
        if let Err(e) = check_operation_failed(
            handle.sweep_with_history(GeometryHandleId(1), GeometryHandleId(2)),
            "sweep_with_history",
        ) {
            failures.push(e);
        }
        if let Err(e) = check_operation_failed(
            handle.loft_with_history(&[GeometryHandleId(1), GeometryHandleId(2)]),
            "loft_with_history",
        ) {
            failures.push(e);
        }
        if let Err(e) = check_operation_failed(
            handle.fillet_with_history(GeometryHandleId(1), 1.0e-3),
            "fillet_with_history",
        ) {
            failures.push(e);
        }
        if let Err(e) = check_operation_failed(
            handle.chamfer_with_history(GeometryHandleId(1), 1.0e-3),
            "chamfer_with_history",
        ) {
            failures.push(e);
        }

        assert_no_check_failures(failures);
    }

    /// `OcctKernelHandle`'s *inherent* `extract_vertices`: method
    /// resolution prefers it over the `GeometryKernel` trait override, so
    /// the shared suite's trait-dispatch coverage can't reach it. Handle has
    /// no inherent `extract_edges`/`extract_faces` (only trait overrides,
    /// already covered), so there's no analogous gap for those (task 5110).
    #[test]
    fn stub_handle_extract_vertices_returns_error() {
        let mut handle = OcctKernelHandle::spawn();
        let mut failures = Vec::new();

        if let Err(e) = check_query_failed(
            handle.extract_vertices(GeometryHandleId(1)),
            "extract_vertices",
        ) {
            failures.push(e);
        }

        assert_no_check_failures(failures);
    }
}
