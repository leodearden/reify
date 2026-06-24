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
use reify_core::{DiagnosticCode, RealizationNodeId};
use reify_eval::cache::NodeId;
use reify_eval::{BuildScheduler, Engine};
use reify_ir::ExportFormat;
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

    // The entity_path for body_a under `RealizationNodeId { entity: e, index: 0 }`.
    // Format matches `RealizationNodeId::fmt` → `"{entity}#realization[{index}]"`.
    let body_a_path = format!("{e}#realization[0]");

    // ── Session A: ALL-VISIBLE (both realizations demanded) ──────────────────
    //
    // After `set_demand_selective([R(a), R(b)])`, `full_scope` is OFF but both
    // bodies' backward closures cover the full trace map (sa→w, sb→w, R(a),
    // R(b) all demanded). `run_unified_pass_seeded` over the full seed
    // produces the same Kahn schedule as `run_unified_pass`, so both bodies
    // are dispatched.
    let (d_all, mesh_all, body_a_mesh_all) = {
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
        let body_a_mesh = result
            .meshes
            .iter()
            .find(|m| m.entity_path == body_a_path)
            .map(|m| (m.mesh.vertices.len(), m.mesh.indices.len()));
        (engine.last_dispatch_count(), result.meshes.len(), body_a_mesh)
    };

    // ── Session B: HIDDEN (only body_a demanded, body_b hidden) ─────────────
    //
    // After `set_demand_selective([R(a)])`, `full_scope` is OFF and only
    // `{R(a), sa, w}` are demanded. Under β's selective driver, only R(a)
    // enters the seed and schedule; R(b)'s box op is never dispatched.
    let (d_hidden, mesh_hidden, body_a_mesh_hidden) = {
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
        let body_a_mesh = result
            .meshes
            .iter()
            .find(|m| m.entity_path == body_a_path)
            .map(|m| (m.mesh.vertices.len(), m.mesh.indices.len()));
        (engine.last_dispatch_count(), result.meshes.len(), body_a_mesh)
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

    // ── VISIBLE BODY CORRECTNESS ─────────────────────────────────────────────
    // The surviving visible body (body_a) must be present and identical in both
    // sessions.  A seed that is too narrow (e.g. drops a shared ancestor needed
    // by body_a) would still satisfy `d_hidden < d_all` and `mesh_hidden <
    // mesh_all` while silently corrupting body_a's geometry.  This assertion
    // catches over-pruning: if body_a's mesh has different dimensions or is
    // absent in the hidden session, the seed is wrong.
    let body_a_dims_all = body_a_mesh_all.expect(
        "all-visible session must contain a mesh for body_a (SelectiveMultiBody#realization[0])"
    );
    let body_a_dims_hidden = body_a_mesh_hidden.expect(
        "hidden session must still contain a mesh for the visible body_a \
         (SelectiveMultiBody#realization[0]); the demand seed over-pruned it"
    );
    assert_eq!(
        body_a_dims_hidden, body_a_dims_all,
        "visible body_a mesh must be identical in the hidden session: \
         hidden=(vertices={}, indices={}) vs all-visible=(vertices={}, indices={})",
        body_a_dims_hidden.0, body_a_dims_hidden.1,
        body_a_dims_all.0, body_a_dims_all.1,
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// step-3: characterization guards protecting the step-4 build() site refactor.
//
// These tests pin PRD §7.1/D2 invariants. They are GREEN when written (the
// invariants already hold) and must stay GREEN through step-4. They go RED
// only if step-4 is done naively wrong (e.g. always seeding run_unified_pass
// regardless of full_scope, which would drop E_EVAL_CYCLE diagnostics or
// prune the cold eager-errors path).
// ─────────────────────────────────────────────────────────────────────────────

/// step-3(a): `build()` under `UnifiedDag` preserves `E_EVAL_CYCLE` diagnostics
/// on a cyclic module regardless of the selective-demand state.
///
/// After a naive wrong step-4 that routes build through `run_unified_pass_seeded`
/// unconditionally (ignoring `full_scope`), the helper returns an empty
/// `diagnostics` vec on the seeded branch → the `E_EVAL_CYCLE` code disappears
/// from the `BuildResult`. This test catches that regression.
///
/// The fixture `"structure S { let a = b + 1.0; let b = a + 1.0 }"` is the
/// same mutual let-cycle used in `unified_dag_cycle_contract.rs`.
#[test]
fn build_preserves_eval_cycle_diagnostic_under_unified_dag() {
    let source = "structure S {\n    let a = b + 1.0\n    let b = a + 1.0\n}";
    let compiled = compile_source(source);

    let mut engine = Engine::new(
        Box::new(SimpleConstraintChecker),
        Some(Box::new(MockGeometryKernel::new())),
    );
    engine.set_build_scheduler(BuildScheduler::UnifiedDag);
    let result = engine.build(&compiled, ExportFormat::Step);

    assert!(
        result.diagnostics.iter().any(|d| d.code == Some(DiagnosticCode::EvalCycle)),
        "build() under UnifiedDag must emit DiagnosticCode::EvalCycle on a cyclic module; \
         got diagnostics: {:?}",
        result.diagnostics,
    );
}

/// step-3(b): `build()` forces full scope regardless of a prior selective demand.
///
/// After `set_demand_selective([R(a)])` (R(b) hidden), calling `build()` must
/// still realize BOTH bodies — build's internal `check()`→`eval()` flips
/// `full_scope=true` before reaching the unified pass, so the helper takes the
/// full-scope branch and demand_seed=None (all realizations appended).
///
/// Asserts:
/// 1. `demand_is_full_scope()` is true after build (eval set it).
/// 2. The dispatch count matches a plain full-scope build (both bodies realized).
///
/// A naive wrong step-4 that relied on `self.demand.is_full_scope()` BEFORE
/// eval sets it (i.e. reading the stale selective state) would prune R(b) →
/// dispatch count drops → this assertion fails.
#[test]
fn build_ignores_selective_demand_and_realizes_all_bodies() {
    let e = "SelectiveMultiBody";
    let compiled = compile_source(differential::SELECTIVE_DEMAND_MULTIBODY_SRC);

    let body_a = NodeId::Realization(RealizationNodeId::new(e, 0));

    // Session A: plain build with full_scope (no explicit selective demand).
    let d_full = {
        let mut engine = Engine::new(
            Box::new(SimpleConstraintChecker),
            Some(Box::new(MockGeometryKernel::new())),
        );
        engine.set_build_scheduler(BuildScheduler::UnifiedDag);
        engine.build(&compiled, ExportFormat::Step);
        engine.last_dispatch_count()
    };

    // Session B: set selective demand (body_b hidden) then build.
    let (d_selective_build, full_scope_after) = {
        let mut engine = Engine::new(
            Box::new(SimpleConstraintChecker),
            Some(Box::new(MockGeometryKernel::new())),
        );
        engine.set_build_scheduler(BuildScheduler::UnifiedDag);
        // Install selective demand BEFORE build (body_b hidden).
        engine.set_demand_selective([body_a.clone()]);
        // build() calls eval() → full_scope=true; selective demand is overridden.
        engine.build(&compiled, ExportFormat::Step);
        (engine.last_dispatch_count(), engine.demand_is_full_scope())
    };

    assert!(
        full_scope_after,
        "build() must set demand full_scope=true (via eval()); \
         got demand_is_full_scope()=false after build"
    );

    assert!(
        d_full > 0,
        "full-scope build must dispatch at least one geometry op; got d_full=0"
    );

    assert_eq!(
        d_selective_build, d_full,
        "build() ignores prior selective demand: dispatch count must match full-scope build \
         (d_selective_build={d_selective_build} vs d_full={d_full})"
    );
}

/// step-3(c): all-visible selective `tessellate_snapshot` is byte-identical
/// to a full-scope `tessellate_snapshot` in dispatch count.
///
/// When `set_demand_selective` covers every realization (`full_scope` is OFF
/// but the entire cone is demanded), `demand_scoped_unified_pass` seeds
/// `run_unified_pass_seeded` with all trace-map keys → the schedule is
/// identical to `run_unified_pass` → demand_seed covers all realizations →
/// the fallback appends nothing new → same dispatches as full_scope.
///
/// A wrong β implementation that made the seed too narrow (e.g. only direct
/// realization nodes, missing shared ancestors) would produce fewer dispatches
/// than full scope → this assertion catches it.
#[test]
fn all_visible_selective_tessellate_snapshot_matches_full_scope_dispatch() {
    let e = "SelectiveMultiBody";
    let compiled = compile_source(differential::SELECTIVE_DEMAND_MULTIBODY_SRC);

    let body_a = NodeId::Realization(RealizationNodeId::new(e, 0));
    let body_b = NodeId::Realization(RealizationNodeId::new(e, 1));

    // Session C1: eval() leaves full_scope=true → tessellate_snapshot is full scope.
    let d_full_scope = {
        let mut engine = Engine::new(
            Box::new(SimpleConstraintChecker),
            Some(Box::new(MockGeometryKernel::new())),
        );
        engine.set_build_scheduler(BuildScheduler::UnifiedDag);
        engine.eval(&compiled);
        // demand is full_scope here (eval set it); do not override.
        engine
            .tessellate_snapshot(&compiled)
            .expect("tessellate_snapshot must return Some after eval()");
        engine.last_dispatch_count()
    };

    // Session C2: eval() then set_demand_selective([R(a), R(b)]) → all-visible
    // selective (full_scope=OFF, both bodies demanded).
    let d_all_visible = {
        let mut engine = Engine::new(
            Box::new(SimpleConstraintChecker),
            Some(Box::new(MockGeometryKernel::new())),
        );
        engine.set_build_scheduler(BuildScheduler::UnifiedDag);
        engine.eval(&compiled);
        engine.set_demand_selective([body_a.clone(), body_b.clone()]);
        engine
            .tessellate_snapshot(&compiled)
            .expect("tessellate_snapshot must return Some after eval()");
        engine.last_dispatch_count()
    };

    assert!(
        d_full_scope > 0,
        "full-scope tessellate must dispatch at least one geometry op; got d_full_scope=0"
    );

    assert_eq!(
        d_all_visible, d_full_scope,
        "all-visible selective tessellate_snapshot must dispatch identically to full-scope: \
         d_all_visible={d_all_visible} vs d_full_scope={d_full_scope}"
    );
}
