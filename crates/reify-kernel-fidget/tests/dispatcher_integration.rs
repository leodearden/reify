//! Cross-crate dispatcher integration test for the Fidget v0.2 adapter.
//!
//! Pins the full inventory-submit → registry-materialise → dispatcher-select
//! pipeline for the fidget kernel.
//!
//! # Cross-crate isolation rationale
//!
//! This test lives in `crates/reify-kernel-fidget/tests/` with `reify-eval`
//! as a dev-dep on the fidget crate — NOT in `crates/reify-eval/tests/` with
//! fidget as a dev-dep of reify-eval. Inverting the dep direction is critical:
//! adding `reify-kernel-fidget` as a dev-dep of `reify-eval` would pull
//! fidget's `inventory::submit!` into the existing `reify-eval` test binaries.
//! Because `"fidget" < "manifold" < "occt"` lexicographically, `pick_lexmin_kernel()`
//! would return the fidget registration, silently breaking the existing
//! `engine_with_registered_kernel_picks_occt_for_brep_box_build` test in
//! `crates/reify-eval/tests/kernel_registry_inventory.rs`. Keeping the
//! dev-dep on fidget's side isolates fidget's link closure to fidget's own
//! test binaries; the existing OCCT and Manifold tests are unaffected.
//!
//! # What this test covers
//!
//! Given a registry that includes the fidget registration (and possibly OCCT/Manifold):
//! - `registry()` contains the key `"fidget"` (proves the submit fired).
//! - `dispatcher::dispatch(...)` for `(BooleanUnion, Sdf)` with `Sdf` as the
//!   sole available repr selects `"fidget"` with zero conversion stages
//!   (zero-conversion path: input repr already matches the demanded repr).

use std::collections::{BTreeMap, HashSet};

use reify_eval::{dispatcher, kernel_registry};
use reify_types::{CapabilityDescriptor, Operation, ReprKind};

/// Proves that `reify_eval::kernel_registry::registry()` contains `"fidget"`
/// when the fidget adapter is linked in (i.e. the `inventory::submit!` in
/// `register.rs` fires unconditionally in this task's stub-only build).
///
/// Then asserts that calling `dispatcher::dispatch(...)` for
/// `(BooleanUnion, Sdf)` with `{Sdf}` as the available-repr set produces a
/// `DispatchPlan` that routes to `"fidget"` with no conversion stages — the
/// zero-conversion (direct) path, since the input repr already matches
/// Fidget's declared input repr for `BooleanUnion`.
#[test]
fn fidget_dispatches_for_sdf_boolean_when_only_kernel() {
    // Linker anchor: call `fidget_capability_descriptor` and assert the
    // result is non-empty.  This serves two purposes:
    //
    // 1. Forces the linker to include `register.rs`'s translation unit from
    //    the `reify-kernel-fidget` rlib.  Without an observable reference,
    //    the linker dead-strips the entire rlib — nothing else in this binary
    //    references it — so the `inventory::submit!` constructor never fires
    //    and `kernel_registry::registry()` returns an empty map.
    //
    // 2. Makes the anchor OBSERVABLE to the optimiser (assigning to a
    //    never-read binding is weaker and MAY be elided under LTO/release).
    //    Asserting on the function's output prevents the call from being
    //    optimised away regardless of the optimisation level.
    //
    // Compare: `crates/reify-kernel-manifold/tests/dispatcher_integration.rs`
    // uses the same linker anchor pattern for the manifold adapter.
    let anchor_descriptor = reify_kernel_fidget::register::fidget_capability_descriptor();
    assert!(
        !anchor_descriptor.supports.is_empty(),
        "fidget_capability_descriptor() must declare at least one capability \
         (linker anchor sanity check — if empty the registration is broken)",
    );

    let reg = kernel_registry::registry();

    // 1. Registry contains "fidget" — proves the inventory submit fired.
    assert!(
        reg.contains_key("fidget"),
        "kernel_registry::registry() must contain \"fidget\"; found keys: {:?}",
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

    // 3. Dispatch BooleanUnion for an Sdf input.
    let available: HashSet<ReprKind> = HashSet::from([ReprKind::Sdf]);
    let plan = dispatcher::dispatch(&view, Operation::BooleanUnion, ReprKind::Sdf, &available);

    // 4. The plan must exist and select "fidget".
    let plan = plan.expect(
        "dispatcher::dispatch must return Some(...) for (BooleanUnion, Sdf) when fidget \
         is registered",
    );
    assert_eq!(
        plan.kernel, "fidget",
        "dispatch must select the fidget kernel for (BooleanUnion, Sdf); \
         got kernel = {:?}",
        plan.kernel,
    );

    // 5. Zero-conversion path: input repr (Sdf) already satisfies Fidget's
    //    declared requirement — no conversion stages needed.
    assert!(
        plan.conversions.is_empty(),
        "dispatch must produce zero conversion stages when the input repr is already Sdf; \
         got conversions = {:?}",
        plan.conversions,
    );
}
