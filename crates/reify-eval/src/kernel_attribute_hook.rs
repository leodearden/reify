//! Engine-side dispatcher for [`reify_types::KernelAttributeHook`].
//!
//! Wraps the per-kernel `attribute_hook()` accessor so call sites can
//! delegate to whichever hook (if any) the active `GeometryKernel`
//! advertises, without each call site re-implementing the
//! `Some(hook).propagate_attributes(...)` / `None → fall-through` pattern.
//!
//! See also: `crates/reify-eval/src/topology_attribute_propagation.rs` for
//! the BRep-side `BRepAlgoAPI_*` Modified/Generated/Deleted propagation.
//! This module is the analogue for non-BRep (currently: Manifold mesh)
//! kernels per PRD `docs/prds/v0_2/persistent-naming-v2.md` line 70.

use reify_types::{
    FeatureId, GeometryHandleId, GeometryKernel, GeometryOp, KernelAttributeOutcome, QueryError,
    TopologyAttributeTable,
};

/// Engine-side dispatcher for `KernelAttributeHook`.
///
/// **Note**: this is currently a `unimplemented!()` stub — step 8 of plan
/// #2657 replaces the body with the real dispatch logic. The signature is
/// stable so the step-7 test can compile.
pub fn propagate_via_kernel_attribute_hook(
    _kernel: &dyn GeometryKernel,
    _table: &mut TopologyAttributeTable,
    _op: &GeometryOp,
    _parent_handles: &[GeometryHandleId],
    _result_handle: GeometryHandleId,
    _splitting_feature_id: &FeatureId,
) -> Result<KernelAttributeOutcome, QueryError> {
    unimplemented!("step 8 of plan #2657 implements the dispatch body")
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_types::{
        ExportError, ExportFormat, GeometryError, GeometryHandle, GeometryQuery,
        KernelAttributeHook, Mesh, TessError, Value,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// In-test hook impl that records every `propagate_attributes` call into
    /// a shared call counter and returns `Ok(Propagated)`. Used by the
    /// `_routes_to_kernel_when_some` test to pin that the engine-side
    /// dispatcher routes through the kernel-advertised hook (rather than
    /// short-circuiting to `FellThrough`).
    struct FixedHookStub {
        calls: AtomicUsize,
    }

    impl KernelAttributeHook for FixedHookStub {
        fn propagate_attributes(
            &self,
            _table: &mut TopologyAttributeTable,
            _op: &GeometryOp,
            _parent_handles: &[GeometryHandleId],
            _result_handle: GeometryHandleId,
            _splitting_feature_id: &FeatureId,
        ) -> Result<KernelAttributeOutcome, QueryError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Ok(KernelAttributeOutcome::Propagated)
        }
    }

    /// Mock kernel whose `attribute_hook()` returns a borrowed reference to
    /// an embedded `FixedHookStub`. All other `GeometryKernel` methods are
    /// inert (return errors that the test does not exercise).
    ///
    /// Bound by lifetime to the `FixedHookStub` it borrows: the stub lives
    /// inside the kernel struct so the borrow is `'self`-scoped via the
    /// `attribute_hook(&self)` accessor.
    struct HookAdvertisingKernel {
        hook: FixedHookStub,
    }

    impl GeometryKernel for HookAdvertisingKernel {
        fn execute(&mut self, _op: &GeometryOp) -> Result<GeometryHandle, GeometryError> {
            Err(GeometryError::OperationFailed("not used by this test".into()))
        }
        fn query(&self, _q: &GeometryQuery) -> Result<Value, QueryError> {
            Err(QueryError::QueryFailed("not used by this test".into()))
        }
        fn export(
            &self,
            _h: GeometryHandleId,
            _f: ExportFormat,
            _w: &mut dyn std::io::Write,
        ) -> Result<(), ExportError> {
            Err(ExportError::FormatError("not used by this test".into()))
        }
        fn tessellate(&self, _h: GeometryHandleId, _t: f64) -> Result<Mesh, TessError> {
            Err(TessError::TessellationFailed("not used by this test".into()))
        }
        fn attribute_hook(&self) -> Option<&dyn KernelAttributeHook> {
            Some(&self.hook)
        }
    }

    /// PRD line 70 contract: when a kernel advertises a `KernelAttributeHook`
    /// via `attribute_hook() = Some(hook)`, the engine-side dispatcher must
    /// delegate to `hook.propagate_attributes(...)` and surface its outcome
    /// unchanged. A regression that short-circuits the Some-branch to
    /// `FellThrough` (or that swallows the outcome via `.unwrap_or(...)`)
    /// would silently drop the kernel-side propagation work.
    #[test]
    fn propagate_via_kernel_attribute_hook_routes_to_kernel_when_some() {
        let kernel = HookAdvertisingKernel {
            hook: FixedHookStub {
                calls: AtomicUsize::new(0),
            },
        };
        let mut table = TopologyAttributeTable::default();
        let op = GeometryOp::Union {
            left: GeometryHandleId(1),
            right: GeometryHandleId(2),
        };
        let parents = [GeometryHandleId(1), GeometryHandleId(2)];
        let result = GeometryHandleId(3);
        let feature_id = FeatureId::new("test#realization[0]");

        let outcome = propagate_via_kernel_attribute_hook(
            &kernel,
            &mut table,
            &op,
            &parents,
            result,
            &feature_id,
        );

        match outcome {
            Ok(KernelAttributeOutcome::Propagated) => {}
            other => panic!(
                "dispatcher must surface hook's outcome unchanged when kernel \
                 advertises Some(hook); FixedHookStub returns Ok(Propagated); got {other:?}"
            ),
        }

        // Single call: the dispatcher must not invoke the hook more than once.
        assert_eq!(
            kernel.hook.calls.load(Ordering::Relaxed),
            1,
            "dispatcher must invoke hook.propagate_attributes exactly once",
        );
    }
}
