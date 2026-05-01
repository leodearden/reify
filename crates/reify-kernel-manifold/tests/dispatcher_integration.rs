//! Cross-crate dispatcher integration test for the Manifold v0.2 adapter.
//!
//! Pins the full inventory-submit → registry-materialise → dispatcher-select
//! pipeline for the manifold kernel.
//!
//! # Cross-crate isolation rationale
//!
//! This test lives in `crates/reify-kernel-manifold/tests/` with `reify-eval`
//! as a dev-dep on the manifold crate — NOT in `crates/reify-eval/tests/` with
//! manifold as a dev-dep of reify-eval. Inverting the dep direction is critical:
//! adding `reify-kernel-manifold` as a dev-dep of `reify-eval` would pull
//! manifold's `inventory::submit!` into the existing `reify-eval` test binaries.
//! Because `"manifold" < "occt"` lexicographically, `pick_lexmin_kernel()` would
//! return the manifold registration, silently breaking the existing
//! `engine_with_registered_kernel_picks_occt_for_brep_box_build` test in
//! `crates/reify-eval/tests/kernel_registry_inventory.rs`. Keeping the
//! dev-dep on manifold's side isolates manifold's link closure to manifold's
//! own test binaries; the existing OCCT test is unaffected.
//!
//! # What this test covers
//!
//! Given a registry that includes the manifold registration (and possibly OCCT):
//! - `registry()` contains the key `"manifold"` (proves the submit fired).
//! - `dispatcher::dispatch(...)` for `(BooleanUnion, Mesh)` with `Mesh` as the
//!   sole available repr selects `"manifold"` with zero conversion stages
//!   (zero-conversion path: input repr already matches the demanded repr).

use std::collections::{BTreeMap, HashSet};

use reify_eval::{dispatcher, kernel_registry};
use reify_types::{CapabilityDescriptor, Operation, ReprKind};

/// Proves that `reify_eval::kernel_registry::registry()` contains `"manifold"`
/// when the manifold adapter is linked in (i.e. the `inventory::submit!` in
/// `register.rs` fires unconditionally in this task's stub-only build).
///
/// Then asserts that calling `dispatcher::dispatch(...)` for
/// `(BooleanUnion, Mesh)` with `{Mesh}` as the available-repr set produces a
/// `DispatchPlan` that routes to `"manifold"` with no conversion stages — the
/// zero-conversion (direct) path, since the input repr already matches
/// Manifold's declared input repr for `BooleanUnion`.
#[test]
fn manifold_dispatches_for_mesh_boolean_when_only_kernel() {
    // Linker anchor: an explicit function-pointer reference to a symbol in
    // `register.rs` forces the linker to include that translation unit from
    // the `reify-kernel-manifold` rlib.  Without this, the linker dead-strips
    // the entire manifold rlib — nothing else in this binary references it —
    // so the `inventory::submit!` `__CTOR` `.init_array` entry never fires
    // and `kernel_registry::registry()` returns an empty map.
    //
    // Compare: `crates/reify-eval/tests/kernel_registry_inventory.rs` uses
    // `reify_kernel_occt::OCCT_AVAILABLE` as the equivalent anchor for the
    // OCCT registration.  This is the same pattern, applied to manifold.
    let _anchor: fn() -> reify_types::CapabilityDescriptor =
        reify_kernel_manifold::register::manifold_capability_descriptor;

    let reg = kernel_registry::registry();

    // 1. Registry contains "manifold" — proves the inventory submit fired.
    assert!(
        reg.contains_key("manifold"),
        "kernel_registry::registry() must contain \"manifold\"; found keys: {:?}",
        reg.keys().collect::<Vec<_>>(),
    );

    // 2. Build a descriptor view for the dispatcher.
    //    `registry()` values are `&'static KernelRegistration`; we call the
    //    `descriptor` function pointer on each to get an owned `CapabilityDescriptor`,
    //    collect them into a local owned map, then build a borrowed view that
    //    matches `dispatcher::dispatch`'s `&BTreeMap<String, &CapabilityDescriptor>`.
    let owned: BTreeMap<String, CapabilityDescriptor> = reg
        .iter()
        .map(|(k, entry)| (k.clone(), (entry.descriptor)()))
        .collect();
    let view: BTreeMap<String, &CapabilityDescriptor> =
        owned.iter().map(|(k, v)| (k.clone(), v)).collect();

    // 3. Dispatch BooleanUnion for a Mesh input.
    let available: HashSet<ReprKind> = HashSet::from([ReprKind::Mesh]);
    let plan = dispatcher::dispatch(&view, Operation::BooleanUnion, ReprKind::Mesh, &available);

    // 4. The plan must exist and select "manifold".
    let plan = plan.expect(
        "dispatcher::dispatch must return Some(...) for (BooleanUnion, Mesh) when manifold \
         is registered",
    );
    assert_eq!(
        plan.kernel, "manifold",
        "dispatch must select the manifold kernel for (BooleanUnion, Mesh); \
         got kernel = {:?}",
        plan.kernel,
    );

    // 5. Zero-conversion path: input repr (Mesh) already satisfies Manifold's
    //    declared requirement — no conversion stages needed.
    assert!(
        plan.conversions.is_empty(),
        "dispatch must produce zero conversion stages when the input repr is already Mesh; \
         got conversions = {:?}",
        plan.conversions,
    );
}
