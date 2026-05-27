//! Cross-crate dispatcher integration test for the Fidget v0.2 adapter.
//!
//! Pins the full inventory-submit → registry-materialise → dispatcher-select
//! pipeline for the fidget kernel.
//!
//! # Cross-crate isolation rationale
//!
//! This test lives in `crates/reify-kernel-fidget/tests/` with `reify-eval`
//! as a dev-dep on the fidget crate — NOT in `crates/reify-eval/tests/` with
//! fidget as a dev-dep of reify-eval. Inverting the dep direction guards
//! against the v0.3 BFS-tie-break breakage path (now the load-bearing reason
//! — the present-day path no longer applies; see below).
//!
//! **Present-day (BRep filter).** `Engine::with_registered_kernel` now calls
//! `pick_lexmin_brep_kernel()` at
//! `crates/reify-eval/src/kernel_registry.rs:177-179` (call site:
//! `crates/reify-eval/src/engine_admin.rs:382`) — a BRep-preferring picker
//! that filters for kernels claiming at least one `(_, ReprKind::BRep)` pair
//! before falling back to lex-min. Since fidget's supports table at
//! `crates/reify-kernel-fidget/src/register.rs` declares only Sdf pairs,
//! OCCT is selected for `engine_with_registered_kernel_picks_occt_for_brep_box_build`
//! regardless of `"fidget" < "manifold" < "occt"` lex order — even if fidget's
//! `inventory::submit!` fired in `reify-eval` test binaries. The
//! previously-documented present-day breakage path therefore no longer applies.
//!
//! **v0.3 (BFS chain tie-break).** When the v0.3 dispatcher BFS exposes a
//! chain that crosses an Sdf rung (e.g. a future `Sdf → Fidget BooleanUnion`
//! step in an equal-cost multi-hop path), a lex-min tie-break between
//! equal-cost BFS paths could misroute if fidget's `inventory::submit!` fired
//! in `reify-eval` test binaries via dev-dep transitivity.
//!
//! Keeping the dep on fidget's side isolates its link closure to fidget's own
//! test binaries and prevents the v0.3 BFS-tie-break breakage path.
//! Dep-direction inversion is the structural defensive isolation now that the
//! `inventory::submit!` is unconditional (no longer feature-gated).
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
use reify_kernel_fidget::FidgetKernel;
use reify_ir::{CapabilityDescriptor, GeometryKernel, GeometryOp, Operation, ReprKind, Value};

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

/// End-to-end pin proving done-criterion #4: a `field def`-shaped SDF
/// realization flows dispatcher → fidget kernel → JIT eval without ever
/// touching OCCT meshing.
///
/// The factory call proves the registry's inventory submission produces
/// a working `Box<dyn GeometryKernel>`. We additionally drive a fresh
/// `FidgetKernel` directly for the eval steps (the trait object alone is
/// sufficient for execute, but `evaluate_sdf_at` is an inherent method
/// not on the trait).
#[test]
fn fidget_dispatcher_to_kernel_chain_realizes_sdf_without_occt() {
    // 1. Pull the registry and find the fidget entry.
    let reg = kernel_registry::registry();
    let fidget_entry = reg
        .get("fidget")
        .expect("registry must contain \"fidget\" — see linker-anchor pattern in companion test");

    // 2. The factory must produce a working `Box<dyn GeometryKernel>`.
    //    Use it to prove the boxed kernel can execute a Sphere op via the
    //    trait surface — that's the contract the dispatcher relies on.
    let mut boxed: Box<dyn reify_ir::GeometryKernel> = (fidget_entry.factory)();
    let _sphere_via_factory = boxed
        .execute(&GeometryOp::Sphere {
            radius: Value::Real(1.0),
        })
        .expect("execute(Sphere) on the boxed registry-factory kernel must succeed");

    // 3. Re-pin the dispatcher selection: with `{Sdf}` available, fidget
    //    wins (BooleanUnion, Sdf) zero-conversion. This is the precondition
    //    for the chain — the test above proves it; we re-assert it locally
    //    so a regression here does not silently fall through.
    let owned: BTreeMap<String, CapabilityDescriptor> = reg
        .iter()
        .map(|(k, entry)| (k.clone(), (entry.descriptor)()))
        .collect();
    let view: BTreeMap<String, &CapabilityDescriptor> =
        owned.iter().map(|(k, v)| (k.clone(), v)).collect();
    let available: HashSet<ReprKind> = HashSet::from([ReprKind::Sdf]);
    let plan = dispatcher::dispatch(&view, Operation::BooleanUnion, ReprKind::Sdf, &available)
        .expect("dispatch must succeed for (BooleanUnion, Sdf)");
    assert_eq!(plan.kernel, "fidget");
    assert!(plan.conversions.is_empty());

    // 4. Drive a fresh kernel for the eval chain. (The boxed factory kernel
    //    above proved the registry entry works; here we use FidgetKernel
    //    directly because evaluate_sdf_at is an inherent method, not part
    //    of the dyn-safe trait.)
    let mut kernel = FidgetKernel::new();

    // Build two boxes with different proportions, both centred at the origin.
    // Choosing different shapes (cube + long thin x-bar) means the union's
    // sample points fall in different operands — the test actually exercises
    // `min` across non-trivial geometry rather than degenerating into "the
    // larger primitive always wins". Translate is not in the kernel's op
    // surface yet, so we vary the box dimensions instead of the centres.
    //
    //   cube:  Box{2, 2, 2}   half-extents (1,    1,    1)
    //   bar:   Box{4, 0.5, 0.5} half-extents (2,    0.25, 0.25)
    let cube = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(2.0),
            height: Value::Real(2.0),
            depth: Value::Real(2.0),
        })
        .expect("Box cube")
        .id;
    let bar = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(4.0),
            height: Value::Real(0.5),
            depth: Value::Real(0.5),
        })
        .expect("Box bar")
        .id;
    let union = kernel
        .execute(&GeometryOp::Union {
            left: cube,
            right: bar,
        })
        .expect("Union via Tree composition")
        .id;

    // 5. Evaluate at four sample points where the analytical answer for
    //    each operand is known and where each operand wins at least once
    //    so the `min` actually selects between them:
    //
    //      (0,   0,   0)  cube=−1,    bar=−0.25  → min = −1   (cube wins, deep interior)
    //      (1.5, 0,   0)  cube=+0.5,  bar=−0.25  → min = −0.25 (bar wins, x-bar extends past cube face)
    //      (0,   0.8, 0)  cube=−0.2,  bar=+0.55  → min = −0.2  (cube wins, bar is thin in y)
    //      (3.0, 0,   0)  cube=+2.0,  bar=+1.0   → min = +1.0  (bar wins outside both, but closer)
    //
    //    See `kernel.rs::box_tree` for the SDF formula; manual sanity-check
    //    in the plan-iteration history. This pins the composition contract
    //    (Tree::min on two distinct trees) end-to-end through the dispatcher
    //    → factory → JIT-eval pipeline.
    let cases: &[(f32, f32, f32, f32)] = &[
        (0.0, 0.0, 0.0, -1.0),
        (1.5, 0.0, 0.0, -0.25),
        (0.0, 0.8, 0.0, -0.2),
        (3.0, 0.0, 0.0, 1.0),
    ];
    for &(x, y, z, expected) in cases {
        let got = kernel
            .evaluate_sdf_at(union, x, y, z)
            .expect("evaluate_sdf_at must succeed on the union handle");
        assert!(
            (got - expected).abs() < 1e-5,
            "union SDF({x},{y},{z}): expected {expected}, got {got}",
        );
    }
}
