//! Cross-crate dispatcher integration test for the Gmsh v0.3 adapter.
//!
//! Pins the full inventory-submit → registry-materialise → dispatcher-select
//! pipeline for the gmsh kernel along the surface→volume route.
//!
//! # Cross-crate isolation rationale
//!
//! This test lives in `crates/reify-kernel-gmsh/tests/` with `reify-eval`
//! as a dev-dep on the gmsh crate — NOT in `crates/reify-eval/tests/` with
//! gmsh as a dev-dep of reify-eval. Inverting the dep direction is critical:
//! adding `reify-kernel-gmsh` as a dev-dep of `reify-eval` would pull
//! gmsh's `inventory::submit!` into the existing `reify-eval` test
//! binaries. Because gmsh's `(Convert{from: Mesh}, VolumeMesh)` claim is
//! unique among v0.2/v0.3 kernels, it would not currently break any
//! selection assertion, but the latent footgun would surface the moment a
//! future kernel adds an overlapping `(op, VolumeMesh)` claim. Keeping the
//! dev-dep on gmsh's side isolates gmsh's link closure to gmsh's own test
//! binaries; the existing OCCT, Manifold, Fidget, and OpenVDB tests are
//! unaffected.
//!
//! # What this test covers
//!
//! Given a registry that includes the gmsh registration:
//! - `registry()` contains the key `"gmsh"` (proves the submit fired).
//! - `dispatcher::dispatch(...)` for `(Convert{from: Mesh}, VolumeMesh)`
//!   with `Mesh` as the sole available repr selects `"gmsh"` with one
//!   conversion stage `(\"gmsh\", Mesh, VolumeMesh)`. The BFS seeds at
//!   `Mesh`, expands via gmsh's `(Convert{from: Mesh}, VolumeMesh)` entry
//!   to `VolumeMesh`, and the final-stage probe at `VolumeMesh` matches
//!   gmsh.
//!
//! # Design template
//!
//! `crates/reify-kernel-openvdb/tests/dispatcher_integration.rs:1-115`.

use std::collections::{BTreeMap, HashSet};

use reify_eval::{dispatcher, kernel_registry};
use reify_ir::{CapabilityDescriptor, Operation, ReprKind};

/// Proves that `reify_eval::kernel_registry::registry()` contains `"gmsh"`
/// when the gmsh adapter is linked in (i.e. the `inventory::submit!` in
/// `register.rs` fires unconditionally in this task's stub-only build).
///
/// Then asserts that calling `dispatcher::dispatch(...)` for
/// `(Convert{from: Mesh}, VolumeMesh)` with `{Mesh}` as the available-repr
/// set produces a `DispatchPlan` that routes to `"gmsh"` with a single
/// `(\"gmsh\", Mesh, VolumeMesh)` conversion stage — i.e. one Convert hop
/// from Mesh to VolumeMesh through gmsh.
#[test]
fn gmsh_dispatches_for_mesh_to_volume_mesh_conversion() {
    // Linker anchor: call `gmsh_capability_descriptor` and assert the
    // result is non-empty. This serves two purposes:
    //
    // 1. Forces the linker to include `register.rs`'s translation unit
    //    from the `reify-kernel-gmsh` rlib. Without an observable
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
    // Compare: `crates/reify-kernel-openvdb/tests/dispatcher_integration.rs`
    // uses the same linker anchor pattern for the openvdb adapter.
    let anchor_descriptor = reify_kernel_gmsh::register::gmsh_capability_descriptor();
    assert!(
        !anchor_descriptor.supports.is_empty(),
        "gmsh_capability_descriptor() must declare at least one capability \
         (linker anchor sanity check — if empty the registration is broken)",
    );

    let reg = kernel_registry::registry();

    // 1. Registry contains "gmsh" — proves the inventory submit fired.
    assert!(
        reg.contains_key("gmsh"),
        "kernel_registry::registry() must contain \"gmsh\"; found keys: {:?}",
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

    // 3. Dispatch the surface→volume Convert with Mesh as the only available
    //    input repr.
    let available: HashSet<ReprKind> = HashSet::from([ReprKind::Mesh]);
    let plan = dispatcher::dispatch(
        &view,
        Operation::Convert {
            from: ReprKind::Mesh,
        },
        ReprKind::VolumeMesh,
        &available,
    );

    // 4. The plan must exist and select "gmsh".
    let plan = plan.expect(
        "dispatcher::dispatch must return Some(...) for (Convert{from: Mesh}, VolumeMesh) \
         when gmsh is registered",
    );
    assert_eq!(
        plan.kernel, "gmsh",
        "dispatch must select the gmsh kernel for (Convert{{from: Mesh}}, VolumeMesh); \
         got kernel = {:?}",
        plan.kernel,
    );

    // 5. One-stage Convert path: input repr (Mesh) is converted to
    //    VolumeMesh via gmsh.
    assert_eq!(
        plan.conversions,
        vec![("gmsh".to_string(), ReprKind::Mesh, ReprKind::VolumeMesh,)],
        "dispatch must produce a single (\"gmsh\", Mesh, VolumeMesh) conversion stage; \
         got conversions = {:?}",
        plan.conversions,
    );
}
