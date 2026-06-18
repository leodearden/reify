//! Cross-crate dispatcher integration test for the Manifold v0.2 adapter.
//!
//! Pins the full inventory-submit → registry-materialise → dispatcher-select
//! pipeline for the manifold kernel.
//!
//! # Cross-crate isolation rationale
//!
//! This test lives in `crates/reify-kernel-manifold/tests/` with `reify-eval`
//! as a dev-dep on the manifold crate — NOT in `crates/reify-eval/tests/` with
//! manifold as a dev-dep of reify-eval. Inverting the dep direction guards
//! against the v0.3 BFS-tie-break breakage path (now the sole load-bearing
//! reason — the present-day path no longer applies; see below).
//!
//! **Present-day (BRep filter).** `Engine::with_registered_kernel` now calls
//! `pick_lexmin_brep_kernel()` at
//! `crates/reify-eval/src/kernel_registry.rs:177-179` (call site:
//! `crates/reify-eval/src/engine_admin.rs:382`) — a BRep-preferring picker
//! that filters for kernels claiming at least one `(_, ReprKind::BRep)` pair
//! before falling back to lex-min. Since manifold's supports table at
//! `crates/reify-kernel-manifold/src/register.rs` declares only Mesh pairs,
//! OCCT is selected for `engine_with_registered_kernel_picks_occt_for_brep_box_build`
//! regardless of `"manifold" < "occt"` lex order — even if manifold's
//! `inventory::submit!` fired in `reify-eval` test binaries. The
//! previously-documented present-day breakage path therefore no longer applies;
//! the v0.3 BFS-tie-break paragraph below is now the load-bearing reason for
//! keeping the dep direction inverted.
//!
//! **v0.3 (BFS chain tie-break).** When OCCT's supports table gains
//! `(Operation::Convert { from: BRep }, Mesh)` (the planned v0.3 entry at
//! `crates/reify-kernel-occt/src/register.rs:27-33`), the dispatcher BFS
//! exposes a chain `BRep → OCCT tessellate → Mesh → Manifold BooleanUnion`.
//! A lex-min tie-break between equal-cost BFS paths could then misroute if
//! manifold's `inventory::submit!` fired in `reify-eval` test binaries via
//! dev-dep transitivity.
//!
//! Keeping the dep on manifold's side isolates its link closure to manifold's
//! own test binaries and prevents the v0.3 BFS-tie-break breakage path. Dep-direction
//! inversion is the structural defensive isolation now that the
//! `inventory::submit!` is unconditional (no longer feature-gated, since
//! `manifold3d` is a regular cargo dep that's always present when the crate
//! is built).
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
use reify_ir::{CapabilityDescriptor, Operation, ReprKind};

/// Proves that `reify_eval::kernel_registry::registry()` contains `"manifold"`
/// when the manifold adapter is linked in (the `inventory::submit!` in
/// `register.rs` fires unconditionally now that real FFI is wired).
///
/// Then asserts that calling `dispatcher::dispatch(...)` for
/// `(BooleanUnion, Mesh)` with `{Mesh}` as the available-repr set produces a
/// `DispatchPlan` that routes to `"manifold"` with no conversion stages — the
/// zero-conversion (direct) path, since the input repr already matches
/// Manifold's declared input repr for `BooleanUnion`.
#[test]
fn manifold_dispatches_for_mesh_boolean_when_only_kernel() {
    // Linker anchor: call `manifold_capability_descriptor` and assert the
    // result is non-empty.  This serves two purposes:
    //
    // 1. Forces the linker to include `register.rs`'s translation unit from
    //    the `reify-kernel-manifold` rlib.  Without an observable reference,
    //    the linker dead-strips the entire rlib — nothing else in this binary
    //    references it — so the `inventory::submit!` constructor never fires
    //    and `kernel_registry::registry()` returns an empty map.
    //
    // 2. Makes the anchor OBSERVABLE to the optimiser (assigning to a
    //    never-read binding is weaker and MAY be elided under LTO/release).
    //    Asserting on the function's output prevents the call from being
    //    optimised away regardless of the optimisation level.
    //
    // Compare: `crates/reify-eval/tests/kernel_registry_inventory.rs` reads
    // `reify_kernel_occt::OCCT_AVAILABLE` as the equivalent observable anchor.
    let anchor_descriptor = reify_kernel_manifold::register::manifold_capability_descriptor();
    assert!(
        !anchor_descriptor.supports.is_empty(),
        "manifold_capability_descriptor() must declare at least one capability \
         (linker anchor sanity check — if empty the registration is broken)",
    );

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
    let plan = dispatcher::dispatch(&view, Operation::BooleanUnion, ReprKind::Mesh, &available, None);

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
