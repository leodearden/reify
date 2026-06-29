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
use reify_ir::Value;
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
