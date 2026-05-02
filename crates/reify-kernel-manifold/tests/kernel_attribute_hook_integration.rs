//! Cross-crate `KernelAttributeHook` integration test for the Manifold v0.2
//! adapter.
//!
//! Pins the **chained** plumbing that PRD `docs/prds/v0_2/persistent-naming-v2.md`
//! line 70 requires for "first concrete impl of `KernelAttributeHook`":
//! `ManifoldKernel::new()` → `&dyn GeometryKernel::attribute_hook()` → `Some` →
//! `KernelAttributeHook::propagate_attributes(...)` → `Ok(Discarded)`.
//!
//! Per-property assertions (empty `TopologyAttributeTable`, single WARN event at
//! the `reify_kernel_manifold` target) live with the unit test
//! `manifold_kernel_attribute_hook_returns_discarded_and_emits_warn_diagnostic`
//! in `crates/reify-kernel-manifold/src/kernel.rs`. This test's unique value is
//! catching a regression where the trait-object accessor returns `Some(&BogusHook)`
//! whose `propagate_attributes` diverges from the inherent impl — a failure mode
//! that escapes the per-property unit tests but is caught by this chained pin.
//!
//! # Why this lives in `crates/reify-kernel-manifold/tests/` (not in `kernel.rs`)
//!
//! The `mod tests` block inside `crates/reify-kernel-manifold/src/kernel.rs`
//! already pins each property in isolation (Some-hook accessor, Discarded
//! outcome, WARN diagnostic). This integration test pins the **chained**
//! contract: a regression that only breaks the binding between the steps
//! (e.g. `attribute_hook()` returns `Some(&BogusHook)` whose
//! `propagate_attributes` diverges from the inherent impl) escapes the
//! per-step unit tests but is caught here.
//!
//! Test layout follows the sibling `tests/dispatcher_integration.rs` convention
//! of "manifold dev-deps on reify-eval, not the reverse" — see that file's
//! cross-crate isolation rationale at lines 6-38. This test does NOT depend on
//! `reify-eval`'s engine-side dispatcher (`propagate_via_kernel_attribute_hook`),
//! only on the trait surface in `reify-types` and the Manifold impl. Future
//! Manifold FFI work that breaks the chained accessor → hook → Discarded outcome
//! plumbing is caught here.

use reify_kernel_manifold::ManifoldKernel;
use reify_types::{
    FeatureId, GeometryHandleId, GeometryKernel, GeometryOp, KernelAttributeOutcome,
    TopologyAttributeTable,
};

/// PRD line 70 cross-crate contract: `ManifoldKernel` round-trips its
/// `KernelAttributeHook` through the `&dyn GeometryKernel::attribute_hook()`
/// accessor and the resulting hook's `propagate_attributes(...)` returns
/// `Ok(KernelAttributeOutcome::Discarded)`.
///
/// This test pins the **chained** accessor → hook → outcome plumbing only.
/// Per-property assertions (empty `TopologyAttributeTable`, single WARN event)
/// live with the unit test
/// `manifold_kernel_attribute_hook_returns_discarded_and_emits_warn_diagnostic`
/// in `crates/reify-kernel-manifold/src/kernel.rs`.
#[test]
fn manifold_kernel_attribute_hook_round_trip_via_geometry_kernel_trait_object() {
    let kernel = ManifoldKernel::new();
    let kernel_ref: &dyn GeometryKernel = &kernel;

    let hook = kernel_ref
        .attribute_hook()
        .expect("ManifoldKernel must advertise a KernelAttributeHook via the trait-object accessor");

    let mut table = TopologyAttributeTable::default();
    let op = GeometryOp::Union {
        left: GeometryHandleId(1),
        right: GeometryHandleId(2),
    };
    let parents = [GeometryHandleId(1), GeometryHandleId(2)];
    let result = GeometryHandleId(3);
    let feature_id = FeatureId::new("integration#realization[0]");

    let outcome = hook.propagate_attributes(&mut table, &op, &parents, result, &feature_id);

    // Chained plumbing must reach Ok(Discarded) end-to-end. `QueryError` does
    // not derive `PartialEq`, so we match on the outcome rather than use
    // `assert_eq!`.
    match outcome {
        Ok(KernelAttributeOutcome::Discarded) => {}
        other => panic!(
            "chained &dyn GeometryKernel → attribute_hook → propagate_attributes must reach \
             Ok(Discarded) for the v0.2 stub; got {other:?}"
        ),
    }
}
