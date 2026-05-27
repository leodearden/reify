//! Cross-crate `KernelAttributeHook` integration test for the Manifold v0.2
//! adapter.
//!
//! Pins the **chained** plumbing that PRD `docs/prds/v0_2/persistent-naming-v2.md`
//! line 70 requires for "first concrete impl of `KernelAttributeHook`":
//! `ManifoldKernel::new()` → `&dyn GeometryKernel::attribute_hook()` → `Some` →
//! `KernelAttributeHook::propagate_attributes(...)` → `Ok(Discarded)` with
//! exactly one WARN-level diagnostic at the `reify_kernel_manifold::kernel` target and
//! no writes to `TopologyAttributeTable`.
//!
//! # Why this lives in `crates/reify-kernel-manifold/tests/` (not in `kernel.rs`)
//!
//! The `mod tests` block inside `crates/reify-kernel-manifold/src/kernel.rs`
//! already pins each property in isolation (Some-hook accessor, Discarded
//! outcome, WARN diagnostic). This integration test pins the **chained**
//! contract: a regression that only breaks the binding between the steps
//! (e.g. `attribute_hook()` returns `Some(&BogusHook)` whose
//! `propagate_attributes` doesn't emit a WARN, mutates the table, or returns
//! the wrong outcome) escapes the per-step unit tests but is caught here.
//!
//! Test layout follows the sibling `tests/dispatcher_integration.rs` convention
//! of "manifold dev-deps on reify-eval, not the reverse" — see that file's
//! cross-crate isolation rationale at lines 6-38. This test does NOT depend on
//! `reify-eval`'s engine-side dispatcher (`propagate_via_kernel_attribute_hook`),
//! only on the trait surface in `reify-types` and the Manifold impl. Future
//! Manifold FFI work that breaks any of (Some-hook accessor, trait method
//! present, Discarded outcome path, WARN diagnostic, empty-table invariant)
//! is caught here.

use std::sync::atomic::Ordering;

use reify_kernel_manifold::ManifoldKernel;
use reify_test_support::CountingSubscriberBuilder;
use reify_ir::{FeatureId, GeometryHandleId, GeometryKernel, GeometryOp, KernelAttributeOutcome, TopologyAttributeTable};

/// PRD line 70 cross-crate contract: `ManifoldKernel` round-trips its
/// `KernelAttributeHook` through the `&dyn GeometryKernel::attribute_hook()`
/// accessor and the resulting hook's `propagate_attributes(...)` returns
/// `Ok(KernelAttributeOutcome::Discarded)`, leaves `TopologyAttributeTable`
/// empty, and emits exactly one WARN-level event at the `reify_kernel_manifold::kernel`
/// target.
///
/// This is the cross-crate plumbing pin: future Manifold FFI work that breaks
/// any of (Some-hook accessor, Discarded outcome, empty-table invariant, WARN
/// diagnostic) on the **trait-object** path lights up here even if the per-step
/// unit tests in `kernel.rs` continue to pass in isolation against the inherent
/// impl.
#[test]
fn manifold_kernel_attribute_hook_round_trip_via_geometry_kernel_trait_object() {
    // Inoculate against tracing's per-callsite Interest cache — see
    // `prime_tracing_callsite_cache` in reify-test-support for why.
    reify_test_support::prime_tracing_callsite_cache();
    let kernel = ManifoldKernel::new();
    let kernel_ref: &dyn GeometryKernel = &kernel;

    let hook = kernel_ref.attribute_hook().expect(
        "ManifoldKernel must advertise a KernelAttributeHook via the trait-object accessor",
    );

    let mut table = TopologyAttributeTable::default();
    let op = GeometryOp::Union {
        left: GeometryHandleId(1),
        right: GeometryHandleId(2),
    };
    let parents = [GeometryHandleId(1), GeometryHandleId(2)];
    let result = GeometryHandleId(3);
    let feature_id = FeatureId::new("integration#realization[0]");

    let (subscriber, counters) = CountingSubscriberBuilder::new()
        .count_level(tracing::Level::WARN)
        // Qualified prefix intentionally pins the `crate::module` tracing target
        // (mirrors `target: "reify_kernel_manifold::kernel"` in the impl).
        // If the `KernelAttributeHook` impl moves to a different submodule, update
        // both the `target:` literal in `kernel.rs` and this prefix.
        .target_prefix("reify_kernel_manifold::kernel")
        .build();
    let warn_count = counters[&tracing::Level::WARN].clone();

    let outcome = tracing::subscriber::with_default(subscriber, || {
        hook.propagate_attributes(&mut table, &op, &parents, result, &feature_id)
    });

    // (a) Chained plumbing must reach Ok(Discarded) end-to-end. `QueryError`
    // does not derive `PartialEq`, so we match on the outcome rather than use
    // `assert_eq!`.
    match outcome {
        Ok(KernelAttributeOutcome::Discarded) => {}
        other => panic!(
            "chained &dyn GeometryKernel → attribute_hook → propagate_attributes must reach \
             Ok(Discarded) for the v0.2 stub; got {other:?}"
        ),
    }

    // (b) Table is unchanged: stub does not write spurious entries on the
    // trait-object path either.
    assert!(
        table.is_empty(),
        "Manifold Discarded path must not write to TopologyAttributeTable on the \
         trait-object path — attributes were lost, not propagated",
    );

    // (c) Exactly one WARN event at the reify_kernel_manifold::kernel target on
    // the trait-object path — catches a BogusHook that returns Ok(Discarded)
    // but omits the operator-visibility diagnostic.
    assert_eq!(
        warn_count.load(Ordering::Acquire),
        1,
        "ManifoldKernel's hook must emit exactly one WARN event at \
         reify_kernel_manifold::kernel target on the trait-object path — operator \
         visibility for the intentional attribute-loss diagnostic per PRD \
         docs/prds/v0_2/persistent-naming-v2.md line 70",
    );
}
