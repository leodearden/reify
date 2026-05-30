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
        Err(ExportError::IoError(NOT_AVAILABLE.into()))
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
    use reify_ir::{ExportFormat, GeometryError, GeometryHandleId, GeometryKernel, GeometryOp, Value, WarmStartable};

    #[test]
    fn stub_kernel_new_succeeds() {
        let _kernel = OcctKernel::new();
    }

    #[test]
    fn stub_kernel_execute_returns_error() {
        let mut kernel = OcctKernel::new();
        let result = kernel.execute(&GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(10.0),
            depth: Value::Real(10.0),
        });
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            format!("{err:?}").contains("OCCT"),
            "error should mention OCCT: {err:?}"
        );
    }

    #[test]
    fn stub_kernel_query_returns_error() {
        let kernel = OcctKernel::new();
        let result = kernel.query(&reify_ir::GeometryQuery::Volume(GeometryHandleId(1)));
        assert!(result.is_err());
    }

    #[test]
    fn stub_kernel_export_returns_error() {
        let kernel = OcctKernel::new();
        let mut buf = Vec::new();
        let result = kernel.export(GeometryHandleId(1), ExportFormat::Step, &mut buf);
        assert!(result.is_err());
    }

    #[test]
    fn stub_kernel_tessellate_returns_error() {
        let kernel = OcctKernel::new();
        let result = kernel.tessellate(GeometryHandleId(1), 0.1);
        assert!(result.is_err());
    }

    #[test]
    fn stub_kernel_warm_state_returns_none() {
        let kernel = OcctKernel::new();
        assert!(kernel.warm_state().is_none());
    }

    #[test]
    fn stub_handle_spawn_succeeds() {
        let _handle = OcctKernelHandle::spawn();
    }

    #[test]
    fn stub_handle_execute_returns_error() {
        let handle = OcctKernelHandle::spawn();
        let result = handle.execute(&GeometryOp::Box {
            width: Value::Real(10.0),
            height: Value::Real(10.0),
            depth: Value::Real(10.0),
        });
        assert!(result.is_err());
    }

    #[test]
    fn stub_handle_is_geometry_kernel() {
        let handle = OcctKernelHandle::spawn();
        // Verify it can be used as Box<dyn GeometryKernel>
        let _boxed: Box<dyn GeometryKernel> = Box::new(handle);
    }

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

    /// Helper: assert a string mentions "OCCT" or "not available", matching
    /// the stub crate's `NOT_AVAILABLE` constant verbatim.
    fn assert_stub_message(msg: &str) {
        assert!(
            msg.contains("OCCT") || msg.contains("not available"),
            "stub error message should mention OCCT or 'not available', got: {msg}"
        );
    }

    #[test]
    fn stub_kernel_extract_edges_returns_error() {
        let mut kernel = OcctKernel::new();
        let result = kernel.extract_edges(GeometryHandleId(1));
        let err = result.expect_err("stub extract_edges should error");
        assert_stub_message(&format!("{err:?}"));
    }

    #[test]
    fn stub_kernel_extract_faces_returns_error() {
        let mut kernel = OcctKernel::new();
        let result = kernel.extract_faces(GeometryHandleId(1));
        let err = result.expect_err("stub extract_faces should error");
        assert_stub_message(&format!("{err:?}"));
    }

    #[test]
    fn stub_kernel_extract_vertices_returns_error() {
        let mut kernel = OcctKernel::new();
        let result = kernel.extract_vertices(GeometryHandleId(1));
        let err = result.expect_err("stub extract_vertices should error");
        assert_stub_message(&format!("{err:?}"));
    }

    #[test]
    fn stub_handle_extract_vertices_returns_error() {
        let mut handle = OcctKernelHandle::spawn();
        let result = handle.extract_vertices(GeometryHandleId(1));
        let err = result.expect_err("stub handle extract_vertices should error");
        assert_stub_message(&format!("{err:?}"));
    }

    #[test]
    fn stub_kernel_query_edge_length_returns_error() {
        let kernel = OcctKernel::new();
        let result = kernel.query(&reify_ir::GeometryQuery::EdgeLength(GeometryHandleId(1)));
        let err = result.expect_err("stub query EdgeLength should error");
        assert_stub_message(&format!("{err:?}"));
    }

    #[test]
    fn stub_kernel_query_face_normal_returns_error() {
        let kernel = OcctKernel::new();
        let result = kernel.query(&reify_ir::GeometryQuery::FaceNormal(GeometryHandleId(1)));
        let err = result.expect_err("stub query FaceNormal should error");
        assert_stub_message(&format!("{err:?}"));
    }

    #[test]
    fn stub_kernel_query_edge_tangent_returns_error() {
        let kernel = OcctKernel::new();
        let result = kernel.query(&reify_ir::GeometryQuery::EdgeTangent(GeometryHandleId(
            1,
        )));
        let err = result.expect_err("stub query EdgeTangent should error");
        assert_stub_message(&format!("{err:?}"));
    }

    #[test]
    fn stub_kernel_closest_point_on_shape_returns_error() {
        let kernel = OcctKernel::new();
        let result = kernel.closest_point_on_shape(GeometryHandleId(1), 0.0, 0.0, 0.0);
        let err = result.expect_err("stub closest_point_on_shape should error");
        assert_stub_message(&format!("{err:?}"));
    }

    #[test]
    fn stub_kernel_surface_angle_returns_error() {
        let kernel = OcctKernel::new();
        let result = kernel.surface_angle(GeometryHandleId(1), GeometryHandleId(2));
        let err = result.expect_err("stub surface_angle should error");
        assert_stub_message(&format!("{err:?}"));
    }

    #[test]
    fn stub_handle_fillet_with_history_returns_error() {
        let handle = OcctKernelHandle::spawn();
        let result = handle.fillet_with_history(GeometryHandleId(1), 1.0e-3);
        let err = result.expect_err("stub fillet_with_history should error");
        assert_stub_message(&format!("{err:?}"));
    }

    #[test]
    fn stub_handle_chamfer_with_history_returns_error() {
        let handle = OcctKernelHandle::spawn();
        let result = handle.chamfer_with_history(GeometryHandleId(1), 1.0e-3);
        let err = result.expect_err("stub chamfer_with_history should error");
        assert_stub_message(&format!("{err:?}"));
    }

    #[test]
    fn stub_kernel_surface_normal_at_returns_error() {
        let kernel = OcctKernel::new();
        let result = kernel.surface_normal_at(GeometryHandleId(1), 0.0, 0.0);
        let err = result.expect_err("stub surface_normal_at should error");
        assert_stub_message(&format!("{err:?}"));
    }

    #[test]
    fn stub_kernel_curvature_at_returns_error() {
        let kernel = OcctKernel::new();
        let result = kernel.curvature_at(GeometryHandleId(1), 0.0, 0.0);
        let err = result.expect_err("stub curvature_at should error");
        assert_stub_message(&format!("{err:?}"));
    }

    #[test]
    fn stub_kernel_point_on_shape_returns_error() {
        let kernel = OcctKernel::new();
        let result = kernel.point_on_shape(
            GeometryHandleId(1),
            0.0,
            0.0,
            0.0,
            reify_ir::DEFAULT_POINT_ON_SHAPE_TOLERANCE_M,
        );
        let err = result.expect_err("stub point_on_shape should error");
        assert_stub_message(&format!("{err:?}"));
    }

    #[test]
    fn stub_kernel_contains_returns_error() {
        let kernel = OcctKernel::new();
        let result = kernel.contains(
            GeometryHandleId(1),
            0.0,
            0.0,
            0.0,
            reify_ir::DEFAULT_CONTAINS_TOLERANCE_M,
        );
        let err = result.expect_err("stub contains should error");
        assert_stub_message(&format!("{err:?}"));
    }
}
