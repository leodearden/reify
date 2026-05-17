//! Cross-crate dispatcher integration test for the OCCT v0.3 adapter.
//!
//! Pins the full inventory-submit → registry-materialise → dispatcher-select
//! pipeline for the OCCT kernel along the BRep→Mesh tessellation route.
//!
//! # Cross-crate isolation rationale
//!
//! This test lives in `crates/reify-kernel-occt/tests/` with `reify-eval`
//! as a dev-dep on the OCCT crate — NOT in `crates/reify-eval/tests/` with
//! OCCT as a dev-dep of reify-eval. Inverting the dep direction is critical:
//! adding `reify-kernel-occt` as a dev-dep of `reify-eval` would pull
//! OCCT's `inventory::submit!` into the existing `reify-eval` test binaries.
//! The OCCT `(Convert{from: BRep}, Mesh)` claim is unique among v0.2/v0.3
//! kernels in having BRep as the source repr, but the latent footgun would
//! surface the moment a future kernel adds an overlapping claim. Keeping the
//! dev-dep on OCCT's side isolates OCCT's link closure to OCCT's own test
//! binaries; the existing Manifold, Fidget, Gmsh, and OpenVDB tests are
//! unaffected.
//!
//! # What this test covers
//!
//! Given a registry that includes the OCCT registration (stub-mode skips):
//! - `registry()` contains the key `"occt"` (proves the `inventory::submit!`
//!   fired under `cfg(has_occt)`).
//! - `dispatcher::dispatch(...)` for `(Convert{from: BRep}, Mesh)` with
//!   `BRep` as the sole available repr selects `"occt"` with one conversion
//!   stage `("occt", BRep, Mesh)`. The BFS seeds at `BRep`, expands via
//!   OCCT's `(Convert{from: BRep}, Mesh)` entry to `Mesh`, and the final-
//!   stage probe at `Mesh` matches OCCT.
//!
//! # Design template
//!
//! `crates/reify-kernel-gmsh/tests/dispatcher_integration.rs:1-131`
//! (single-Convert-edge kernel, closest design analog).

use std::collections::{BTreeMap, HashSet};

use reify_eval::{dispatcher, kernel_registry};
use reify_types::{CapabilityDescriptor, Operation, ReprKind};

/// Proves that `reify_eval::kernel_registry::registry()` contains `"occt"`
/// when the OCCT adapter is linked in (i.e. the `inventory::submit!` in
/// `register.rs` fires under `cfg(has_occt)`).
///
/// Then asserts that calling `dispatcher::dispatch(...)` for
/// `(Convert{from: BRep}, Mesh)` with `{BRep}` as the available-repr set
/// produces a `DispatchPlan` that routes to `"occt"` with a single
/// `("occt", BRep, Mesh)` conversion stage — one Convert hop from BRep to
/// Mesh through OCCT tessellation.
///
/// # Stub-mode skip
///
/// The test early-returns when `!reify_kernel_occt::OCCT_AVAILABLE` (i.e.
/// `cfg(has_occt)` not set — stub-mode build). The `inventory::submit!` in
/// `register.rs` is gated on `cfg(has_occt)`, so in stub mode the registry
/// is empty and the dispatch assertion would fail with a misleading message.
/// The `eprintln!` makes the skip visible in CI output rather than silently
/// passing.
#[test]
fn occt_dispatches_for_brep_to_mesh_tessellation_conversion() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping occt_dispatches_for_brep_to_mesh_tessellation_conversion: \
             OCCT unavailable (cfg(has_occt) not set — stub-mode build)"
        );
        return;
    }

    // Linker anchor: call `occt_capability_descriptor` and assert the
    // result is non-empty. This serves two purposes:
    //
    // 1. Forces the linker to include `register.rs`'s translation unit
    //    from the `reify-kernel-occt` rlib. Without an observable
    //    reference, the linker dead-strips the entire rlib — nothing else
    //    in this binary references it — so the `inventory::submit!`
    //    constructor never fires and `kernel_registry::registry()`
    //    returns an empty map.
    //
    // 2. Makes the anchor OBSERVABLE to the optimiser (assigning to a
    //    never-read binding is weaker and MAY be elided under LTO/release).
    //    Asserting on the function's output prevents the call from being
    //    optimised away regardless of the optimisation level.
    //
    // Compare: `crates/reify-kernel-gmsh/tests/dispatcher_integration.rs`
    // uses the same linker anchor pattern for the gmsh adapter.
    let anchor_descriptor = reify_kernel_occt::register::occt_capability_descriptor();
    assert!(
        !anchor_descriptor.supports.is_empty(),
        "occt_capability_descriptor() must declare at least one capability \
         (linker anchor sanity check — if empty the registration is broken)",
    );

    let reg = kernel_registry::registry();

    // 1. Registry contains "occt" — proves the inventory submit fired.
    assert!(
        reg.contains_key("occt"),
        "kernel_registry::registry() must contain \"occt\"; found keys: {:?}",
        reg.keys().collect::<Vec<_>>(),
    );

    // 2. Build a descriptor view for the dispatcher.
    //    `registry()` values are `&'static KernelRegistration`; we call
    //    the `descriptor` function pointer on each to get an owned
    //    `CapabilityDescriptor`, collect them into a local owned map,
    //    then build a borrowed view that matches `dispatcher::dispatch`'s
    //    `&BTreeMap<String, &CapabilityDescriptor>`.
    let owned: BTreeMap<String, CapabilityDescriptor> = reg
        .iter()
        .map(|(k, entry)| (k.clone(), (entry.descriptor)()))
        .collect();
    let view: BTreeMap<String, &CapabilityDescriptor> =
        owned.iter().map(|(k, v)| (k.clone(), v)).collect();

    // 3. Dispatch the BRep→Mesh Convert with BRep as the only available
    //    input repr.
    let available: HashSet<ReprKind> = HashSet::from([ReprKind::BRep]);
    let plan = dispatcher::dispatch(
        &view,
        Operation::Convert {
            from: ReprKind::BRep,
        },
        ReprKind::Mesh,
        &available,
    );

    // 4. The plan must exist and select "occt".
    let plan = plan.expect(
        "dispatcher::dispatch must return Some(...) for (Convert{from: BRep}, Mesh) \
         when OCCT is registered — PRD §8 task δ",
    );
    assert_eq!(
        plan.kernel, "occt",
        "dispatch must select the OCCT kernel for (Convert{{from: BRep}}, Mesh); \
         got kernel = {:?}",
        plan.kernel,
    );

    // 5. One-stage Convert path: input repr (BRep) is tessellated to
    //    Mesh via OCCT.
    assert_eq!(
        plan.conversions,
        vec![("occt".to_string(), ReprKind::BRep, ReprKind::Mesh,)],
        "dispatch must produce a single (\"occt\", BRep, Mesh) conversion stage; \
         got conversions = {:?}",
        plan.conversions,
    );
}
