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
//!
//! # Production call site
//!
//! [`propagate_via_kernel_attribute_hook`] is wired into
//! `Engine::execute_realization_ops` in `crates/reify-eval/src/engine_build.rs`
//! (task 2875). The dispatcher is invoked once per parent-having op — i.e.
//! once per op for which `parent_handles_for_op` (in `engine_build.rs`) returns a
//! non-empty slice — immediately after the existing
//! `populate_attribute_history` call (BRep-first ordering: OCCT-native
//! attribute population runs first; the hook is the non-BRep fallback path
//! that returns `FellThrough` for OCCT shapes and routes to
//! `propagate_attributes` for kernels that advertise a hook, currently
//! `ManifoldKernel`).
//!
//! Three test layers pin the contract end-to-end:
//!
//! 1. **In-module unit tests** (this file): pin the `Some(hook)` routing and
//!    `None → FellThrough+DEBUG` fallback in isolation
//!    (`propagate_via_kernel_attribute_hook_routes_to_kernel_when_some`,
//!    `propagate_via_kernel_attribute_hook_returns_fell_through_with_debug_diagnostic_when_kernel_has_no_hook`).
//!
//! 2. **Cross-crate Manifold plumbing** —
//!    `crates/reify-kernel-manifold/tests/kernel_attribute_hook_integration.rs`:
//!    pins the trait-object path for `ManifoldKernel` specifically
//!    (kernel advertises `attribute_hook() = Some(self)`, stub returns
//!    `Discarded`, dispatcher surfaces `Ok(Discarded)`).
//!
//! 3. **Engine-level wiring** —
//!    `crates/reify-eval/tests/kernel_attribute_hook_wiring.rs` (task 2875):
//!    pins that the engine dispatches the hook for the right ops with the
//!    right `(op, parents, result, feature_id)` tuple, that primitives are
//!    never dispatched, and that `QueryError` from the hook surfaces as a
//!    `Diagnostic::warning` without regressing `geometry_output` to `None`.

use reify_ir::{FeatureId, GeometryHandleId, GeometryKernel, GeometryOp, KernelAttributeOutcome, QueryError, TopologyAttributeTable};

/// Engine-side dispatcher for [`reify_types::KernelAttributeHook`].
///
/// Routes attribute-propagation work to whichever
/// [`reify_types::KernelAttributeHook`] the active `GeometryKernel`
/// advertises via [`reify_types::GeometryKernel::attribute_hook`]:
///
/// - **`Some(hook)`**: delegates to `hook.propagate_attributes(...)`,
///   surfacing the hook's outcome (`Propagated` / `Discarded` / runtime
///   `QueryError`) unchanged. The hook itself emits any required
///   diagnostics; this dispatcher does not duplicate them.
/// - **`None`**: returns
///   [`reify_types::KernelAttributeOutcome::FellThrough`] without writing
///   to `table`. Step 10 of plan #2657 adds a `tracing::debug!` diagnostic
///   on this branch to give operators visibility into the no-hook case.
///
/// This signature deliberately mirrors
/// [`reify_types::KernelAttributeHook::propagate_attributes`] so that
/// reading either makes the other intuitable.
///
/// PRD line 70: kernels without a native attribute-tracking channel
/// (Fidget's SDF reps, OpenVDB's voxel reps) inherit the
/// [`GeometryKernel::attribute_hook`] default of `None`, so the dispatcher
/// returns `FellThrough` for them and selectors over those reps fall
/// through to computed selectors.
///
/// **Call site:** wired into `Engine::execute_realization_ops` in
/// `crates/reify-eval/src/engine_build.rs` by task 2875. Invoked once per
/// parent-having op (per `parent_handles_for_op` in `engine_build.rs`) immediately after
/// `populate_attribute_history`. See the module-level docstring for the full
/// three-layer test contract and the BRep-first ordering rationale.
pub fn propagate_via_kernel_attribute_hook(
    kernel: &dyn GeometryKernel,
    table: &mut TopologyAttributeTable,
    op: &GeometryOp,
    parent_handles: &[GeometryHandleId],
    result_handle: GeometryHandleId,
    splitting_feature_id: &FeatureId,
) -> Result<KernelAttributeOutcome, QueryError> {
    match kernel.attribute_hook() {
        Some(hook) => hook.propagate_attributes(
            table,
            op,
            parent_handles,
            result_handle,
            splitting_feature_id,
        ),
        None => {
            tracing::debug!(
                target: "reify_eval::kernel_attribute_hook",
                outcome = "fell_through",
                "kernel does not advertise a KernelAttributeHook — selectors over this kernel's reps will fall through to computed selectors"
            );
            Ok(KernelAttributeOutcome::FellThrough)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reify_test_support::{CountingSubscriberBuilder, FailingMockGeometryKernel};
    use reify_ir::{ExportError, ExportFormat, GeometryError, GeometryHandle, GeometryQuery, KernelAttributeHook, Mesh, TessError, Value};
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
            Err(GeometryError::OperationFailed(
                "not used by this test".into(),
            ))
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
            Err(TessError::TessellationFailed(
                "not used by this test".into(),
            ))
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

    /// PRD line 70 contract: kernels without a `KernelAttributeHook` (Fidget's
    /// SDF reps, OpenVDB's voxel reps, mocks/stubs that inherit the trait
    /// default) must cause the engine-side dispatcher to return
    /// `Ok(KernelAttributeOutcome::FellThrough)` *without* mutating the
    /// attribute table — and to emit exactly one `DEBUG`-level diagnostic
    /// (not WARN — the no-hook case is informational, not anomalous) at the
    /// `reify_eval::kernel_attribute_hook` target so an `RUST_LOG=debug`
    /// operator can see when selectors will fall through to computed selectors.
    ///
    /// `FailingMockGeometryKernel` does not override
    /// `GeometryKernel::attribute_hook`, so it inherits the `None` default
    /// established in step 2. A regression that drops the default to a
    /// `Some(...)` placeholder, accidentally upgrades the diagnostic to WARN,
    /// or writes to `table` on the no-hook branch is caught here.
    #[test]
    fn propagate_via_kernel_attribute_hook_returns_fell_through_with_debug_diagnostic_when_kernel_has_no_hook()
     {
        let (subscriber, counters) = CountingSubscriberBuilder::new()
            .count_level(tracing::Level::DEBUG)
            .count_level(tracing::Level::WARN)
            .target_prefix("reify_eval::kernel_attribute_hook")
            .build();
        let debug_count = counters[&tracing::Level::DEBUG].clone();
        let warn_count = counters[&tracing::Level::WARN].clone();

        let kernel = FailingMockGeometryKernel;
        let mut table = TopologyAttributeTable::default();
        let op = GeometryOp::Union {
            left: GeometryHandleId(1),
            right: GeometryHandleId(2),
        };
        let parents = [GeometryHandleId(1), GeometryHandleId(2)];
        let result = GeometryHandleId(3);
        let feature_id = FeatureId::new("test#realization[0]");

        let outcome = tracing::subscriber::with_default(subscriber, || {
            propagate_via_kernel_attribute_hook(
                &kernel,
                &mut table,
                &op,
                &parents,
                result,
                &feature_id,
            )
        });

        match outcome {
            Ok(KernelAttributeOutcome::FellThrough) => {}
            other => panic!(
                "dispatcher must return Ok(FellThrough) when kernel.attribute_hook() \
                 inherits the None default; got {other:?}"
            ),
        }

        assert!(
            table.is_empty(),
            "no-hook branch must not mutate the attribute table — it is the \
             dispatcher's responsibility to leave attribute propagation to the \
             computed-selector fallback in this case",
        );

        assert_eq!(
            debug_count.load(Ordering::Acquire),
            1,
            "no-hook branch must emit exactly one DEBUG event at \
             reify_eval::kernel_attribute_hook so RUST_LOG=debug operators \
             see when selectors will fall through to computed selectors",
        );
        assert_eq!(
            warn_count.load(Ordering::Acquire),
            0,
            "no-hook branch is informational, not anomalous — the diagnostic \
             must be DEBUG-level, never WARN; this guards against a regression \
             that conflates the FellThrough case with the Discarded case (which \
             does emit WARN, but only from the kernel's own hook impl)",
        );
    }
}
