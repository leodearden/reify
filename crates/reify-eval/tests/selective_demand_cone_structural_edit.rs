//! δ (task 4740) selective-cone incremental maintenance across structural edits.
//!
//! Pins the headline δ invariant (deliverable A): after a `structural_mutation`
//! edit (collection-count grow via `edit_param`), the engine's PRODUCTION demand
//! registry must be re-derived via in-place `rebuild_cone` — NOT replaced with
//! the degenerate-total `build_demand_for_graph` result — so the selective cone
//! is PRESERVED across the grow.
//!
//! Fixture: [`differential::SELECTIVE_DEMAND_GROW_SRC`] from
//! `common/differential.rs` — a two-body structure with a `sub bolts` collection
//! count-controlled by `param n : Int = 2`. Growing n (via `edit_param`) triggers
//! `structural_mutation = true` in `engine_edit.rs`.

#[path = "common/differential.rs"]
mod differential;

use reify_constraints::SimpleConstraintChecker;
use reify_core::{RealizationNodeId, ValueCellId};
use reify_eval::cache::NodeId;
use reify_eval::{BuildScheduler, Engine};
use reify_ir::{ExportFormat, Value};
use reify_test_support::{compile_source, MockGeometryKernel};

// ─────────────────────────────────────────────────────────────────────────────
// step-1 RED: after a structural-mutation grow, the selective cone is preserved.
// ─────────────────────────────────────────────────────────────────────────────

/// step-1 (RED until step-2): after a collection-count grow (`edit_param(n, 3)`),
/// the selective demand cone must be PRESERVED, not reverted to degenerate-total.
///
/// Sequence:
/// (1) `eval()` on the grow fixture (n=2, two bodies, two bolt instances).
/// (2) `set_demand_selective([body_a])` — hide body_b (`sc` must NOT be demanded).
/// (3) `edit_param(n, Int(3))` — grows `bolts` from 2 to 3, triggering
///     `structural_mutation = true` inside `engine_edit.rs`.
///
/// After (3), assert:
/// (a) `demand_is_full_scope()` is still `false`.
/// (b) `sc` (body_b's exclusive scalar cell) is NOT demanded.
/// (c) `sa` (body_a's exclusive scalar cell) is still demanded (no over-prune).
///
/// RED today on (b): the `structural_mutation` block replaces `self.demand` with
/// `build_demand_for_graph(&new_snapshot.graph)`, which marks EVERY node
/// `always_demanded` (degenerate-total, `full_scope` OFF), so `sc` becomes
/// demanded. The fix (step-2) replaces that reset with an in-place
/// `self.demand.rebuild_cone(&new_snapshot.graph)`, preserving the selective
/// roots and the `full_scope` flag.
#[test]
fn selective_cone_preserved_across_collection_grow() {
    let e = "GrowMultiBody";
    let compiled = compile_source(differential::SELECTIVE_DEMAND_GROW_SRC);

    let body_a = NodeId::Realization(RealizationNodeId::new(e, 0));
    // body_b's exclusive cell (sc = w * 4): feeds ONLY the hidden realization.
    let sc = NodeId::Value(ValueCellId::new(e, "sc"));
    // body_a's exclusive cell (sa = w * 3): feeds ONLY the visible realization.
    let sa = NodeId::Value(ValueCellId::new(e, "sa"));
    let n = ValueCellId::new(e, "n"); // collection count param

    let mut engine = Engine::new(
        Box::new(SimpleConstraintChecker),
        Some(Box::new(MockGeometryKernel::new())),
    );
    engine.set_build_scheduler(BuildScheduler::UnifiedDag);

    // (1) Cold eval: both bodies evaluated, all cells Fresh.
    engine.eval(&compiled);

    // (2) Selective demand: hide body_b (only body_a is demanded).
    engine.set_demand_selective([body_a.clone()]);

    // ── Preconditions before the structural grow ──────────────────────────────
    assert!(
        !engine.demand_is_full_scope(),
        "precondition: full_scope must be OFF after set_demand_selective"
    );
    assert!(
        engine.demand_is_demanded(&sa),
        "precondition: sa must be demanded (it is in the backward cone of visible body_a)"
    );
    assert!(
        !engine.demand_is_demanded(&sc),
        "precondition: sc must NOT be demanded \
         (it feeds only the hidden body_b — not in body_a's cone)"
    );

    // (3) Structural grow: n = 2 → 3, triggers structural_mutation = true.
    engine
        .edit_param(n.clone(), Value::Int(3))
        .expect("edit_param(n, 3) must succeed after a cold eval");

    // ── Post-grow assertions ──────────────────────────────────────────────────

    // (a) full_scope must STILL be false.
    // The in-place rebuild_cone must not touch the full_scope flag.
    assert!(
        !engine.demand_is_full_scope(),
        "(a) demand_is_full_scope() must remain false after the structural grow: \
         rebuild_cone must preserve the full_scope flag"
    );

    // (b) RED today — sc must NOT be demanded after the structural grow.
    //
    // Fails before step-2 because `build_demand_for_graph` marks EVERY node
    // `always_demanded` and assigns `self.demand = new_demand` (degenerate-total
    // cone, full_scope still OFF), so `sc` becomes demanded again even though
    // body_b is hidden. GREEN after step-2 installs the in-place rebuild.
    assert!(
        !engine.demand_is_demanded(&sc),
        "(b) hidden body_b's exclusive cell sc must NOT be demanded after the structural grow: \
         the selective cone must be preserved, not reverted to total demand"
    );

    // (c) GREEN guard — sa must still be demanded (no over-prune of visible cells).
    // Holds both before and after the fix: sa is in body_a's cone and body_a is
    // visible, so the rebuilt cone must include sa regardless.
    assert!(
        engine.demand_is_demanded(&sa),
        "(c) visible body_a's exclusive cell sa must still be demanded after the grow: \
         the in-place rebuild_cone must not accidentally exclude demanded cells"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// step-3: Characterization guards for deliverable A — protect step-2 against
// regressions.  Both guards must stay GREEN after step-2 and catch a naive
// over-prune or cold-path regression.  Run under BOTH BuildScheduler variants.
// ─────────────────────────────────────────────────────────────────────────────

/// step-3 guard (1): Cold-parity across a structural edit.
///
/// After a collection-grow (`edit_param(n, Int(3))`), an ALL-VISIBLE selective
/// session (both body_a AND body_b demanded, `full_scope OFF`) produces the same
/// body-feeding scalar values (`sa`, `sc`, `w`) as a FULL-SCOPE session that went
/// through the identical structural grow.
///
/// This mirrors the `assert_all_visible_selective_matches_full_scope` spine
/// (deliverable A cold-parity) but restricts the comparison to the cells in the
/// selective cone, because the grow adds a new bolt[2].diameter that is present
/// in the full-scope result but absent from the all-visible selective cone
/// (bolt cells do not feed body_a or body_b).
///
/// Catches a regression where `rebuild_cone` accidentally perturbs or excludes
/// a body-feeding scalar so that the selective and full-scope warm maps diverge.
/// Verified GREEN after step-2.
#[test]
fn cold_parity_all_visible_selective_matches_full_scope_across_structural_grow() {
    for scheduler in [BuildScheduler::UnifiedDag, BuildScheduler::LegacyMultiPass] {
        let compiled = compile_source(differential::SELECTIVE_DEMAND_GROW_SRC);
        let e = "GrowMultiBody";
        let n = ValueCellId::new(e, "n");
        let sa = ValueCellId::new(e, "sa");
        let sc = ValueCellId::new(e, "sc");
        let w = ValueCellId::new(e, "w");

        // Engine A: all-visible selective demand (body_a AND body_b both visible).
        let mut sel_engine = Engine::new(
            Box::new(SimpleConstraintChecker),
            Some(Box::new(MockGeometryKernel::new())),
        );
        sel_engine.set_build_scheduler(scheduler);
        sel_engine.eval(&compiled);
        sel_engine.set_demand_selective([
            NodeId::Realization(RealizationNodeId::new(e, 0)),
            NodeId::Realization(RealizationNodeId::new(e, 1)),
        ]);
        assert!(
            !sel_engine.demand_is_full_scope(),
            "precondition: full_scope must be OFF after set_demand_selective under {scheduler:?}"
        );
        let sel_result = sel_engine
            .edit_param(n.clone(), Value::Int(3))
            .expect("selective all-visible grow must succeed");

        // Engine B: cold full-scope (all nodes demanded).
        let mut full_engine = Engine::new(
            Box::new(SimpleConstraintChecker),
            Some(Box::new(MockGeometryKernel::new())),
        );
        full_engine.set_build_scheduler(scheduler);
        full_engine.eval(&compiled);
        full_engine.set_demand_full_scope(true);
        let full_result = full_engine
            .edit_param(n.clone(), Value::Int(3))
            .expect("full-scope grow must succeed");

        // The body-feeding scalar cells (sa = w*3, sc = w*4, w) must be
        // byte-identical between selective-all-visible and full-scope after the grow.
        //
        // Bolt instance cells (bolt[0..2].diameter) are intentionally excluded:
        // they are not in the visible bodies' backward cones and are absent from
        // the selective cone's EvalResult (not demanded → not in the grown_seed →
        // not evaluated in the structural_mutation pass).
        for cell in [&sa, &sc, &w] {
            let sel_hash = sel_result.values.get(cell).map(|v| v.content_hash());
            let full_hash = full_result.values.get(cell).map(|v| v.content_hash());
            assert_eq!(
                sel_hash, full_hash,
                "cell `{cell}` content_hash diverged under {scheduler:?}: \
                 selective-all-visible vs full-scope must agree on body-feeding scalars \
                 after a structural grow"
            );
        }
    }
}

/// step-3 guard (2): Cold eager-errors preserved after a selective structural grow.
///
/// After a `set_demand_selective` + `edit_param(n, Int(3))` (structural grow with
/// selective cone, `full_scope OFF`), a subsequent cold `build()` (which internally
/// calls `check()` → `eval()` → `set_full_scope(true)`) must restore
/// `demand_is_full_scope() == true` and demand ALL realizations — meaning the cold
/// path still surfaces every body (eager-error detection is unaffected).
///
/// Catches a regression where step-2's in-place `rebuild_cone` accidentally
/// interferes with the cold path's `set_full_scope(true)` restoration.
/// Verified GREEN after step-2.
#[test]
fn cold_build_restores_full_scope_after_selective_structural_grow() {
    for scheduler in [BuildScheduler::UnifiedDag, BuildScheduler::LegacyMultiPass] {
        let compiled = compile_source(differential::SELECTIVE_DEMAND_GROW_SRC);
        let e = "GrowMultiBody";
        let body_a = NodeId::Realization(RealizationNodeId::new(e, 0));
        let body_b = NodeId::Realization(RealizationNodeId::new(e, 1));
        let n = ValueCellId::new(e, "n");

        let mut engine = Engine::new(
            Box::new(SimpleConstraintChecker),
            Some(Box::new(MockGeometryKernel::new())),
        );
        engine.set_build_scheduler(scheduler);

        // (1) Cold eval — sets full_scope ON (standard cold path).
        engine.eval(&compiled);
        assert!(
            engine.demand_is_full_scope(),
            "precondition: cold eval must set full_scope ON under {scheduler:?}"
        );

        // (2) Selective demand — hides body_b, sets full_scope OFF.
        engine.set_demand_selective([NodeId::Realization(RealizationNodeId::new(e, 0))]);
        assert!(
            !engine.demand_is_full_scope(),
            "precondition: full_scope must be OFF after set_demand_selective under {scheduler:?}"
        );

        // (3) Structural grow with selective cone (the δ deliverable A scenario).
        //     After step-2's fix, full_scope must STILL be OFF (selective cone preserved).
        engine
            .edit_param(n.clone(), Value::Int(3))
            .expect("selective grow must succeed");
        assert!(
            !engine.demand_is_full_scope(),
            "step-2 invariant: full_scope must stay OFF after the structural grow under {scheduler:?}"
        );

        // (4) Cold build() → check() → eval() → set_full_scope(true).
        //     Rebuilds from the original compiled module (n=2 default); the cold
        //     path is exercised regardless of prior warm edits.
        let _ = engine.build(&compiled, ExportFormat::Step);

        // After the cold build, full_scope must be ON again (cold override restored).
        assert!(
            engine.demand_is_full_scope(),
            "cold build() must restore full_scope=true under {scheduler:?}: \
             the cold path must be unaffected by the prior selective structural grow"
        );
        // Under full_scope, ALL realizations are demanded — body_b (previously hidden)
        // must be surfaced on the cold path (eager-errors/validation cover it too).
        assert!(
            engine.demand_is_demanded(&body_a),
            "body_a must be demanded after cold build() restores full_scope under {scheduler:?}"
        );
        assert!(
            engine.demand_is_demanded(&body_b),
            "body_b (previously hidden in selective session) must be demanded after \
             cold build() restores full_scope under {scheduler:?}"
        );
    }
}
