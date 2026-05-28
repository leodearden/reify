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

use reify_eval::{DispatchPlan, dispatcher, kernel_registry};
use reify_ir::{CapabilityDescriptor, GeometryError, GeometryHandleId, GeometryKernel, GeometryOp, Operation, ReprKind};
use reify_kernel_openvdb::register::openvdb_capability_descriptor;
use reify_kernel_openvdb::OpenVdbKernel;

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

/// OpenVDB's capability descriptor must declare `(Convert{from:Mesh}, Voxel)`.
///
/// This is the entry added in task η (3438) that enables the dispatcher BFS
/// to route a two-stage BRep→Mesh→Voxel chain. RED until register.rs is
/// updated in step-4.
#[test]
fn openvdb_descriptor_declares_convert_mesh_to_voxel() {
    let descriptor = openvdb_capability_descriptor();
    assert!(
        descriptor.supports(Operation::Convert { from: ReprKind::Mesh }, ReprKind::Voxel),
        "OpenVDB descriptor must declare (Convert{{from:Mesh}}, Voxel) — \
         required for the BRep→Mesh→Voxel two-stage dispatch chain (task η)",
    );
}

// ---------------------------------------------------------------------------
// Shared helper: two-stage BRep→Mesh→Voxel planning fixture
// ---------------------------------------------------------------------------

/// Build a synthetic registry, dispatch `(BooleanUnion, Voxel)` from `{BRep}`,
/// assert the two-stage chain plan, and return the plan.
///
/// Called by both the chain-dispatch test and the graceful-degradation test to
/// avoid duplicating the registry setup and planning assertions verbatim.
fn assert_two_stage_brep_to_voxel_plan() -> DispatchPlan {
    let occt_descriptor = CapabilityDescriptor {
        supports: vec![(Operation::Convert { from: ReprKind::BRep }, ReprKind::Mesh)],
    };
    let openvdb_descriptor = openvdb_capability_descriptor();

    let owned: BTreeMap<String, CapabilityDescriptor> = BTreeMap::from([
        ("occt".to_string(), occt_descriptor),
        ("openvdb".to_string(), openvdb_descriptor),
    ]);
    let view: BTreeMap<String, &CapabilityDescriptor> =
        owned.iter().map(|(k, v)| (k.clone(), v)).collect();

    let available: HashSet<ReprKind> = HashSet::from([ReprKind::BRep]);
    let plan = dispatcher::dispatch(&view, Operation::BooleanUnion, ReprKind::Voxel, &available)
        .expect(
            "dispatcher::dispatch must return Some(...) for (BooleanUnion, Voxel) with BRep \
             input when the two-stage BRep→Mesh→Voxel chain is available",
        );

    assert_eq!(
        plan.kernel, "openvdb",
        "two-stage chain must resolve to openvdb as the final-stage kernel; got {:?}",
        plan.kernel,
    );
    assert_eq!(
        plan.conversions,
        vec![
            ("occt".to_string(), ReprKind::BRep, ReprKind::Mesh),
            ("openvdb".to_string(), ReprKind::Mesh, ReprKind::Voxel),
        ],
        "two-stage chain must produce conversions [(occt,BRep,Mesh),(openvdb,Mesh,Voxel)]",
    );
    plan
}

// ---------------------------------------------------------------------------

/// With a synthetic registry containing OCCT's `(Convert{from:BRep}, Mesh)`
/// and OpenVDB's full descriptor (including the new `(Convert{from:Mesh}, Voxel)`),
/// dispatching `(BooleanUnion, Voxel)` from `{BRep}` must produce a two-stage
/// plan: kernel="openvdb", conversions=[("occt",BRep,Mesh),("openvdb",Mesh,Voxel)].
///
/// Uses a synthetic in-test registry so this test does not depend on OCCT
/// being linked into the openvdb test binary (dep-direction isolation — see
/// module-level doc). RED until register.rs is updated in step-4.
#[test]
fn openvdb_dispatches_two_stage_chain_brep_to_voxel() {
    assert_two_stage_brep_to_voxel_plan();
}

/// Pins the planning-vs-execution contract for the BRep→Mesh→Voxel two-stage chain.
///
/// # What this test pins
///
/// - **PLANNING**: `dispatcher::dispatch` with a synthetic `{ "occt": (Convert{BRep},Mesh),
///   "openvdb": descriptor }` registry resolves `(BooleanUnion, Voxel)` from `{BRep}` to
///   `kernel="openvdb", conversions=[("occt",BRep,Mesh),("openvdb",Mesh,Voxel)]` — pinning
///   the exact two-stage chain the reviewer flagged.
/// - **EXECUTION (graceful degradation)**: calling `GeometryKernel::execute` on a freshly
///   constructed `OpenVdbKernel` with a `GeometryOp::Union` (dummy handle IDs — `execute`
///   short-circuits before reading them) returns `Err(GeometryError::OperationFailed(_))`,
///   NOT a panic and NOT `Ok(_)` (which would be silent-wrong-geometry).
///
/// # Contract documented
///
/// The `(Convert{from:Mesh}, Voxel)` descriptor edge is a **PLANNING** declaration that
/// lets the dispatcher BFS reach Voxel. The executable Mesh→Voxel primitive is
/// `OpenVdbKernel::realize_voxel_from_mesh_with_options` (task η steps 5-6). Trait-`execute()`
/// of the planned terminal Voxel op intentionally degrades to a typed error until task ε wires
/// engine dispatch (no `GeometryOp` Mesh-input variant exists, so trait-execute routing is
/// structurally deferred). This test makes the graceful-degradation contract visible from green CI.
///
/// cfg-agnostic: both the real `kernel_real.rs` and stub `kernel.rs` return
/// `GeometryError::OperationFailed` from `execute()` — no `cfg(has_openvdb)` split needed.
#[test]
fn openvdb_two_stage_chain_terminal_op_execute_degrades_gracefully() {
    // -----------------------------------------------------------------------
    // PLANNING side — pin the two-stage BRep→Mesh→Voxel dispatch chain via
    // the shared helper (avoids duplicating registry setup + plan assertions).
    // -----------------------------------------------------------------------
    assert_two_stage_brep_to_voxel_plan();

    // -----------------------------------------------------------------------
    // EXECUTION side — terminal Voxel op through trait degrades gracefully.
    // -----------------------------------------------------------------------
    // Drive the planned final-stage op through the GeometryKernel trait.
    // execute() short-circuits before reading the handle IDs, so dummy IDs
    // need no real Voxel handles / FFI.
    let mut k = OpenVdbKernel::new();
    let r = GeometryKernel::execute(
        &mut k,
        &GeometryOp::Union {
            left: GeometryHandleId(0),
            right: GeometryHandleId(1),
        },
    );

    // Must be a typed, intentional error — NOT a panic, NOT Ok(_).
    assert!(
        matches!(r, Err(GeometryError::OperationFailed(_))),
        "execute() for a terminal Voxel op must degrade to GeometryError::OperationFailed, \
         got {:?}",
        r,
    );
}
