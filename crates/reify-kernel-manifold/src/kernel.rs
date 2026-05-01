//! Stub `ManifoldKernel` — all operations return descriptive errors.
//!
//! # Design templates
//!
//! `crates/reify-kernel-occt/src/stubs.rs` — `OcctKernel` stub pattern
//! (`_private: ()` field, `new()` constructor, all-error trait impl).
//! `crates/reify-test-support/src/mocks.rs:889` — `FailingMockGeometryKernel`.
//!
//! # v0.2 scope
//!
//! Real Manifold C++ FFI is deferred to a follow-up task. This stub exists
//! so the `inventory::submit!` in `register.rs` has a factory that compiles.
//! When the follow-up task lands, the factory can switch to the real impl
//! behind `cfg(has_manifold)` without changing the registration shape.

use reify_types::{
    ExportError, ExportFormat, FeatureId, GeometryError, GeometryHandle, GeometryHandleId,
    GeometryKernel, GeometryOp, GeometryQuery, KernelAttributeHook, KernelAttributeOutcome, Mesh,
    QueryError, TessError, TopologyAttributeTable, Value,
};

const STUB_MSG: &str = "Manifold mesh booleans not yet implemented; \
    reify-kernel-manifold is a registration-only scaffold for the v0.2 multi-kernel system \
    (see docs/prds/v0_2/multi-kernel.md). Real Manifold C++ FFI is a follow-up.";

/// Stub Manifold kernel — all operations return descriptive errors.
///
/// The `_private: ()` field prevents external struct-literal construction;
/// callers must go through [`Self::new`] or [`Self::default`].
/// Matches the OCCT stub pattern in
/// `crates/reify-kernel-occt/src/stubs.rs:25-27`.
///
/// Trivially `Send + Sync` (no interior mutability, no raw pointers — no
/// `unsafe impl` needed; the auto-derived impls fire).
pub struct ManifoldKernel {
    _private: (),
}

impl ManifoldKernel {
    /// Construct a new stub `ManifoldKernel`.
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for ManifoldKernel {
    fn default() -> Self {
        Self::new()
    }
}

impl GeometryKernel for ManifoldKernel {
    fn execute(&mut self, _op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
        Err(GeometryError::OperationFailed(STUB_MSG.into()))
    }

    fn query(&self, _query: &GeometryQuery) -> Result<Value, QueryError> {
        Err(QueryError::QueryFailed(STUB_MSG.into()))
    }

    fn export(
        &self,
        _handle: GeometryHandleId,
        _format: ExportFormat,
        _writer: &mut dyn std::io::Write,
    ) -> Result<(), ExportError> {
        Err(ExportError::FormatError(STUB_MSG.into()))
    }

    fn tessellate(&self, _handle: GeometryHandleId, _tolerance: f64) -> Result<Mesh, TessError> {
        Err(TessError::TessellationFailed(STUB_MSG.into()))
    }
    // extract_edges, extract_faces, execute_with_history, query_many all use
    // the trait defaults — they error in the standard "not supported" fashion.

    /// Override the trait default to advertise that ManifoldKernel implements
    /// [`KernelAttributeHook`]. Per PRD line 70, ManifoldKernel is the first
    /// concrete impl: returning `Some(self)` here is what makes the engine-
    /// side dispatcher (`reify-eval::propagate_via_kernel_attribute_hook`)
    /// route attribute propagation to [`Self::propagate_attributes`] rather
    /// than `KernelAttributeOutcome::FellThrough`.
    fn attribute_hook(&self) -> Option<&dyn KernelAttributeHook> {
        Some(self)
    }
}

/// First concrete impl of [`KernelAttributeHook`] — see PRD line 70.
///
/// In the v0.2 stub, the body unconditionally returns
/// `Ok(KernelAttributeOutcome::Discarded)`. The `tracing::warn!` diagnostic
/// (required by the `Discarded` contract) is added in step 6 of the plan;
/// for now the impl is a pure stub so the structural plumbing
/// (`attribute_hook() → Some` → `propagate_attributes() → Ok(Discarded)`)
/// can be tested first.
///
/// When real Manifold C++ FFI lands in a follow-up task, the body switches
/// to walk `MeshGL` merge vectors + per-triangle `faceID` / `originalID`
/// to copy parent attributes onto result face handles, returning
/// `Propagated` on success and `Discarded` (with a `reason="heavy_remeshing"`
/// flavoured WARN) on lossy remeshing — the trait surface is stable across
/// that swap.
impl KernelAttributeHook for ManifoldKernel {
    fn propagate_attributes(
        &self,
        _table: &mut TopologyAttributeTable,
        _op: &GeometryOp,
        _parent_handles: &[GeometryHandleId],
        _result_handle: GeometryHandleId,
        _splitting_feature_id: &FeatureId,
    ) -> Result<KernelAttributeOutcome, QueryError> {
        // v0.2 stub: real Manifold FFI is deferred. Always return Discarded.
        // The WARN diagnostic that the Discarded contract requires lands in
        // step 6 of the plan.
        Ok(KernelAttributeOutcome::Discarded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    reify_test_support::assert_stub_kernel_errors!(ManifoldKernel::new, "Manifold");

    /// PRD docs/prds/v0_2/persistent-naming-v2.md line 70: ManifoldKernel is
    /// the first concrete impl of `KernelAttributeHook`. This test pins the
    /// "ManifoldKernel opts into the hook AND is reachable through the
    /// trait-object accessor" contract — a regression that loses the override
    /// (e.g. removed `attribute_hook()` impl on ManifoldKernel) would silently
    /// fall back to the `None` default and the engine-side dispatcher would
    /// route Manifold ops to `FellThrough`, defeating the multi-kernel
    /// propagation pipeline this task builds.
    ///
    /// Bound as `&dyn GeometryKernel` (not `&ManifoldKernel`) because the
    /// engine-side dispatcher invokes the accessor through a trait object —
    /// asserting via the typed concrete reference would let an accidental
    /// `&self`/`&dyn` divergence slip through.
    #[test]
    fn manifold_kernel_advertises_attribute_hook_via_geometry_kernel_trait() {
        let kernel = ManifoldKernel::new();
        let kernel_ref: &dyn reify_types::GeometryKernel = &kernel;
        assert!(
            kernel_ref.attribute_hook().is_some(),
            "ManifoldKernel must override `attribute_hook()` to return Some(self) — \
             enforces PRD line 70 'first concrete impl of KernelAttributeHook' contract \
             reachable through the trait-object accessor",
        );
    }
}
