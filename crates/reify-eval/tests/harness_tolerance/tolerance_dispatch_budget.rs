//! Integration-level smoke for task 2729's `per_stage_tolerance_for_plan` API
//! surface.
//!
//! This file exercises the new `per_stage_tolerance_for_plan` function and its
//! re-export through `crates/reify-eval/src/lib.rs`. It is deliberately a
//! separate `tests/` integration file (not a `#[cfg(test)] mod` inside the
//! crate) so it can ONLY see what the crate actually re-exports — a symbol
//! that is `pub` inside `dispatcher.rs` but missing from the lib.rs re-export
//! line will fail to compile here, locking the public surface.
//!
//! Purpose, per the task plan: pin the lib.rs re-export contract so a
//! downstream caller (the eventual engine/kernel-registry timing-loop
//! consumer) can rely on the entire tolerance-allocation primitive set being
//! discoverable through the crate root, not buried inside the private module
//! path.

use std::collections::{BTreeMap, HashSet};

use reify_eval::{
    dispatch, per_stage_tolerance_for_plan,
    tolerance_budget::{SAFETY_FACTOR, per_stage_tolerance},
};
use reify_ir::{CapabilityDescriptor, Operation, ReprKind};

/// Integration smoke: confirms `per_stage_tolerance_for_plan` is re-exported
/// through the crate root and wired correctly to `dispatch()` output.
///
/// Three contracts locked here:
///
/// (a) `dispatch()` on a BRep→Sdf→Mesh→BooleanUnion registry produces a
///     `DispatchPlan` with 2 conversions — pinning that the plan shape fed to
///     `per_stage_tolerance_for_plan` matches what the live dispatcher returns.
///     This is the only test site where a real `dispatch()` call (BFS search
///     over a live capability registry) feeds its result to
///     `per_stage_tolerance_for_plan`; in-crate unit tests construct
///     `DispatchPlan` literals directly and do not exercise that wiring.
///
/// (b) `per_stage_tolerance_for_plan(&plan, req)` equals
///     `per_stage_tolerance(req, 2)` — the integration-usage validation the
///     task description asks for ("validated by … integration usage").
///
/// (c) `SAFETY_FACTOR` is value-pinned to `0.8` at the public surface; the
///     canonical contract lives in `tolerance_budget::tests::*` inside the
///     crate. The assertion also locks the re-export path (compile-time).
#[test]
fn lib_re_exports_per_stage_tolerance_for_plan_and_dispatch_end_to_end() {
    // Value pin at the public surface: the canonical contract lives in
    // `tolerance_budget::tests::*` inside the crate. This assertion also
    // locks the re-export path — a missing `pub use` drops compilation here.
    assert_eq!(SAFETY_FACTOR, 0.8, "SAFETY_FACTOR public value contract");

    // ── (a) end-to-end dispatch produces a 2-conversion plan ─────────────────

    // Fixture mirrors dispatcher.rs:894–920 (`dispatch_two_stage_chain_is_shortest`):
    //   alpha: BRep → Sdf (only; no direct BRep→Mesh anywhere)
    //   beta:  Sdf  → Mesh
    //   manifold: BooleanUnion on Mesh (final-stage kernel, no conversion edges)
    let alpha = CapabilityDescriptor {
        supports: vec![(
            Operation::Convert {
                from: ReprKind::BRep,
            },
            ReprKind::Sdf,
        )],
    };
    let beta = CapabilityDescriptor {
        supports: vec![(
            Operation::Convert {
                from: ReprKind::Sdf,
            },
            ReprKind::Mesh,
        )],
    };
    let manifold = CapabilityDescriptor {
        supports: vec![(Operation::BooleanUnion, ReprKind::Mesh)],
    };

    let mut registry: BTreeMap<String, &CapabilityDescriptor> = BTreeMap::new();
    // Registry keys must be real kernel names so dispatch()'s
    // `KernelId::from_registry_name` bridge resolves each conversion stage
    // (the `alpha`/`beta` locals retain their BRep→Sdf / Sdf→Mesh roles).
    registry.insert("fidget".to_string(), &alpha);
    registry.insert("gmsh".to_string(), &beta);
    registry.insert("manifold".to_string(), &manifold);

    let mut available: HashSet<ReprKind> = HashSet::new();
    available.insert(ReprKind::BRep);

    let plan = dispatch(
        &registry,
        Operation::BooleanUnion,
        ReprKind::Mesh,
        &available,
        // Task #3443: no pragma preference at this call site (tolerance-budget
        // smoke test; pragma steering is exercised in pragma_override_routing.rs).
        None,
    )
    .expect("2-stage chain BRep→Sdf→Mesh + BooleanUnion must be findable");

    assert_eq!(
        plan.conversions.len(),
        2,
        "dispatch must produce exactly 2 conversion stages (BRep→Sdf and Sdf→Mesh); got {plan:?}",
    );

    // ── (b) per_stage_tolerance_for_plan delegates to per_stage_tolerance ─────

    // Asserts the wiring is `per_stage_tolerance(req, plan.conversions.len())`
    // — catches an off-by-one that hard-codes n_stages = 1 or len + 1.
    let req = 1e-3_f64;
    assert_eq!(
        per_stage_tolerance_for_plan(&plan, req),
        per_stage_tolerance(req, 2),
        "per_stage_tolerance_for_plan must equal per_stage_tolerance(req, n_stages=2) \
         for a 2-conversion plan",
    );
}
