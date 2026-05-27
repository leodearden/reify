//! Cross-crate dispatcher integration test for the OpenVDB v0.2 adapter.
//!
//! Pins the full inventory-submit → registry-materialise → dispatcher-select
//! pipeline for the openvdb kernel.
//!
//! # Cross-crate isolation rationale
//!
//! This test lives in `crates/reify-kernel-openvdb/tests/` with `reify-eval`
//! as a dev-dep on the openvdb crate — NOT in `crates/reify-eval/tests/` with
//! openvdb as a dev-dep of reify-eval. Inverting the dep direction is critical:
//! adding `reify-kernel-openvdb` as a dev-dep of `reify-eval` would pull
//! openvdb's `inventory::submit!` into the existing `reify-eval` test binaries.
//! Because openvdb claims `(*, Voxel)` pairs exclusively, it would not currently
//! break any selection assertion, but the latent footgun would surface the moment
//! any other kernel adds a `(op, Voxel)` claim. Keeping the dev-dep on openvdb's
//! side isolates openvdb's link closure to openvdb's own test binaries; the
//! existing OCCT, Manifold, and Fidget tests are unaffected.
//!
//! # What this test covers
//!
//! Given a registry that includes the openvdb registration (and possibly OCCT/Manifold/Fidget):
//! - `registry()` contains the key `"openvdb"` (proves the submit fired).
//! - `dispatcher::dispatch(...)` for `(BooleanUnion, Voxel)` with `Voxel` as
//!   the sole available repr selects `"openvdb"` with zero conversion stages
//!   (zero-conversion path: input repr already matches the demanded repr).
//!
//! # Design template
//!
//! `crates/reify-kernel-fidget/tests/dispatcher_integration.rs:1-112`.

use std::collections::{BTreeMap, HashSet};

use reify_eval::{dispatcher, kernel_registry};
use reify_ir::{CapabilityDescriptor, Operation, ReprKind};

/// Proves that `reify_eval::kernel_registry::registry()` contains `"openvdb"`
/// when the openvdb adapter is linked in (i.e. the `inventory::submit!` in
/// `register.rs` fires unconditionally in this task's stub-only build).
///
/// Then asserts that calling `dispatcher::dispatch(...)` for
/// `(BooleanUnion, Voxel)` with `{Voxel}` as the available-repr set produces
/// a `DispatchPlan` that routes to `"openvdb"` with no conversion stages —
/// the zero-conversion (direct) path, since the input repr already matches
/// OpenVDB's declared input repr for `BooleanUnion`.
#[test]
fn openvdb_dispatches_for_voxel_boolean_when_only_kernel() {
    // Linker anchor: call `openvdb_capability_descriptor` and assert the
    // result is non-empty. This serves two purposes:
    //
    // 1. Forces the linker to include `register.rs`'s translation unit from
    //    the `reify-kernel-openvdb` rlib. Without an observable reference,
    //    the linker dead-strips the entire rlib — nothing else in this binary
    //    references it — so the `inventory::submit!` constructor never fires
    //    and `kernel_registry::registry()` returns an empty map.
    //
    // 2. Makes the anchor OBSERVABLE to the optimiser (assigning to a
    //    never-read binding is weaker and MAY be elided under LTO/release).
    //    Asserting on the function's output prevents the call from being
    //    optimised away regardless of the optimisation level.
    //
    // Compare: `crates/reify-kernel-fidget/tests/dispatcher_integration.rs`
    // uses the same linker anchor pattern for the fidget adapter.
    let anchor_descriptor = reify_kernel_openvdb::register::openvdb_capability_descriptor();
    assert!(
        !anchor_descriptor.supports.is_empty(),
        "openvdb_capability_descriptor() must declare at least one capability \
         (linker anchor sanity check — if empty the registration is broken)",
    );

    let reg = kernel_registry::registry();

    // 1. Registry contains "openvdb" — proves the inventory submit fired.
    assert!(
        reg.contains_key("openvdb"),
        "kernel_registry::registry() must contain \"openvdb\"; found keys: {:?}",
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

    // 3. Dispatch BooleanUnion for a Voxel input.
    let available: HashSet<ReprKind> = HashSet::from([ReprKind::Voxel]);
    let plan = dispatcher::dispatch(&view, Operation::BooleanUnion, ReprKind::Voxel, &available);

    // 4. The plan must exist and select "openvdb".
    let plan = plan.expect(
        "dispatcher::dispatch must return Some(...) for (BooleanUnion, Voxel) when openvdb \
         is registered",
    );
    assert_eq!(
        plan.kernel, "openvdb",
        "dispatch must select the openvdb kernel for (BooleanUnion, Voxel); \
         got kernel = {:?}",
        plan.kernel,
    );

    // 5. Zero-conversion path: input repr (Voxel) already satisfies OpenVDB's
    //    declared requirement — no conversion stages needed.
    assert!(
        plan.conversions.is_empty(),
        "dispatch must produce zero conversion stages when the input repr is already Voxel; \
         got conversions = {:?}",
        plan.conversions,
    );
}
