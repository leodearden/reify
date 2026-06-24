//! β (task 4738) selective-demand integration tests.
//!
//! These pin the warm `tessellate_snapshot` demand-scoped scheduling that
//! provides the headline kernel-time saving: under a selective demand, a
//! viewport-hidden body's geometry-kernel work is skipped in
//! `tessellate_snapshot`.
//!
//! Also covers the characterization guards that protect the `build()` site
//! refactor in step-4: cold `build()` always stays full scope (eager-error
//! invariant, PRD §7.1/D2), diagnostics are preserved, and the all-visible
//! selective tessellate is byte-identical to the full-scope tessellate.
//!
//! Fixture: [`SELECTIVE_DEMAND_MULTIBODY_SRC`] from
//! `common/differential.rs` — a constraint-free, param-driven two-body source
//! (`param w` → `sa = w*3` → box `a`; `sb = w*2` → box `b`).

#[path = "common/differential.rs"]
mod differential;

use reify_constraints::SimpleConstraintChecker;
use reify_core::RealizationNodeId;
use reify_eval::cache::NodeId;
use reify_eval::{BuildScheduler, Engine};
use reify_test_support::{compile_source, MockGeometryKernel};

// ─────────────────────────────────────────────────────────────────────────────
// step-1 (RED until step-2): warm tessellate_snapshot prunes a hidden body's
// geometry-kernel dispatch under selective demand + UnifiedDag.
// ─────────────────────────────────────────────────────────────────────────────

/// step-1 (RED until step-2): warm `tessellate_snapshot` prunes a hidden
/// body's geometry-kernel work.
///
/// Two fresh sessions from a COLD geometry cache (`eval()` then
/// `tessellate_snapshot()`, WITHOUT a prior `build()` so the
/// `RealizationCache` is empty and every dispatch is observable):
///
/// (A) ALL-VISIBLE: `eval()` → `set_demand_selective([R(a), R(b)])` →
///     `tessellate_snapshot()` → record `d_all = last_dispatch_count()`.
/// (B) HIDDEN: `eval()` → `set_demand_selective([R(a)])` (R(b) hidden) →
///     `tessellate_snapshot()` → record `d_hidden = last_dispatch_count()`.
///
/// Asserts `d_hidden < d_all`: the hidden body `b`'s geometry-kernel dispatch
/// is skipped in the hidden session.
///
/// **RED today**: `tessellate_snapshot` is demand-blind — it computes
/// `run_unified_pass` over the whole graph regardless of `self.demand`, then
/// the `build_steps` fallback appends every unrealized realization in
/// declaration order (also unconditional). Both sessions dispatch both box
/// bodies → `d_hidden == d_all`, failing the `d_hidden < d_all` assertion.
#[test]
fn warm_tessellate_snapshot_prunes_hidden_body_geometry_dispatch() {
    let e = "SelectiveMultiBody";
    let compiled = compile_source(differential::SELECTIVE_DEMAND_MULTIBODY_SRC);

    // Realization roots: `let a = box(..)` → realization[0],
    //                    `let b = box(..)` → realization[1].
    let body_a = NodeId::Realization(RealizationNodeId::new(e, 0));
    let body_b = NodeId::Realization(RealizationNodeId::new(e, 1));

    // ── Session A: ALL-VISIBLE (both realizations demanded) ──────────────────
    //
    // After `set_demand_selective([R(a), R(b)])`, `full_scope` is OFF but both
    // bodies' backward closures cover the full trace map (sa→w, sb→w, R(a),
    // R(b) all demanded). `run_unified_pass_seeded` over the full seed
    // produces the same Kahn schedule as `run_unified_pass`, so both bodies
    // are dispatched.
    let (d_all, mesh_all) = {
        let mut engine = Engine::new(
            Box::new(SimpleConstraintChecker),
            Some(Box::new(MockGeometryKernel::new())),
        );
        engine.set_build_scheduler(BuildScheduler::UnifiedDag);
        // Cold eval → populates eval_state; does NOT warm the RealizationCache.
        engine.eval(&compiled);
        // Demand both realizations (selective, not full_scope).
        engine.set_demand_selective([body_a.clone(), body_b.clone()]);
        let result = engine
            .tessellate_snapshot(&compiled)
            .expect("tessellate_snapshot must return Some after eval()");
        (engine.last_dispatch_count(), result.meshes.len())
    };

    // ── Session B: HIDDEN (only body_a demanded, body_b hidden) ─────────────
    //
    // After `set_demand_selective([R(a)])`, `full_scope` is OFF and only
    // `{R(a), sa, w}` are demanded. Under β's selective driver, only R(a)
    // enters the seed and schedule; R(b)'s box op is never dispatched.
    let (d_hidden, mesh_hidden) = {
        let mut engine = Engine::new(
            Box::new(SimpleConstraintChecker),
            Some(Box::new(MockGeometryKernel::new())),
        );
        engine.set_build_scheduler(BuildScheduler::UnifiedDag);
        // Cold eval → fresh engine, fresh RealizationCache, cold dispatch path.
        engine.eval(&compiled);
        // Demand ONLY body_a; body_b is hidden.
        engine.set_demand_selective([body_a.clone()]);
        let result = engine
            .tessellate_snapshot(&compiled)
            .expect("tessellate_snapshot must return Some after eval()");
        (engine.last_dispatch_count(), result.meshes.len())
    };

    // All-visible session must have dispatched at least one geometry op.
    // (Each `box` primitive is one dispatch; the 2-body fixture dispatches 2.)
    assert!(
        d_all > 0,
        "all-visible session must dispatch at least one geometry op on a cold cache; \
         got d_all=0 — check that MockGeometryKernel is wired correctly"
    );

    // The all-visible session must surface at least one mesh.
    assert!(
        mesh_all > 0,
        "all-visible session must surface at least one mesh; got mesh_all=0"
    );

    // ── PRIMARY RED SIGNAL ───────────────────────────────────────────────────
    // Hidden session must dispatch STRICTLY FEWER geometry ops than all-visible.
    //
    // RED before β: `tessellate_snapshot` is demand-blind — the whole-graph
    // `run_unified_pass` + unconditional fallback puts BOTH realizations in
    // `build_steps` regardless of selective demand, so both sessions dispatch
    // the same two box bodies → `d_hidden == d_all`, and this assertion fails.
    //
    // GREEN after β (step-2): the `demand_scoped_unified_pass` helper seeds
    // `run_unified_pass_seeded` with only the demanded cone `{R(a), sa, w}`,
    // and the fallback guard skips R(b) (not in seed) → d_hidden < d_all.
    assert!(
        d_hidden < d_all,
        "hidden session must dispatch strictly fewer geometry ops than the all-visible session: \
         d_hidden={d_hidden} must be < d_all={d_all} \
         (hidden body_b's box op must be pruned from the tessellate schedule)"
    );

    // Hidden session must surface fewer meshes than the all-visible session
    // (the hidden body's realization produces no terminal handle → no mesh).
    assert!(
        mesh_hidden < mesh_all,
        "hidden session must surface fewer meshes than all-visible: \
         mesh_hidden={mesh_hidden} must be < mesh_all={mesh_all}"
    );
}
