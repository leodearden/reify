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
}
