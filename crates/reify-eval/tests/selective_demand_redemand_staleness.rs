//! δ (task 4740) re-demand staleness gate — deliverable B.
//!
//! Pins the headline δ invariant (deliverable B): when a previously hidden body
//! is un-hidden (its realization re-enters the demand cone via
//! `set_demand_selective`) and `tessellate_snapshot` is called, the body must
//! re-realize from the CURRENT parameter values, not from stale snapshot values
//! left over from when the body was hidden.
//!
//! ## Root cause (without fix)
//!
//! `tessellate_snapshot` copies `snapshot.values` AS-IS to build its working
//! `ValueMap` (line ~9004 engine_build.rs). A plain arithmetic cell like
//! `sb = w * 2` is NOT refreshed by `hydrate_value_cell_in_loop` (which only
//! handles geometry-query / selector / topology cells). After a hidden-then-edit
//! cycle, `sb` is stale in `snapshot.values` (pruned from the warm eval set
//! during the hidden phase). When body_b is un-hidden and `tessellate_snapshot`
//! runs, body_b re-realizes from stale `sb`, producing geometry that reflects
//! the OLD param value rather than the current one.
//!
//! ## Test oracle
//!
//! A FRESH cold `eval()` + `tessellate_snapshot()` of the post-edit-equivalent
//! source ([`differential::SELECTIVE_DEMAND_MULTIBODY_EDITED_SRC`], `w=20mm`
//! default) gives body_b's geometry `input_cone_hash` at the CORRECT params
//! (`sb = w*2 = 40mm`). This is the expected value the re-demand gate must
//! produce after the fix (step-5).
//!
//! ## Why `input_cone_hash`, not mesh counts
//!
//! A box mesh has SIZE-INVARIANT vertex/index counts — `box(20mm,20mm,20mm)`
//! and `box(40mm,40mm,40mm)` both produce the same vertex/face count, making
//! count-based assertions falsely GREEN. `RealizationNodeData.input_cone_hash`
//! is computed from the arg values fed to each geometry op
//! (`compute_realization_upstream_values_hash`); it DIFFERS between `sb=20mm`
//! and `sb=40mm`, making it the correct staleness detector.
//!
//! Fixture: [`differential::SELECTIVE_DEMAND_MULTIBODY_SRC`] —
//! `param w : Length = 10mm`; `sa = w*3` → `box a` (body_a = realization[0]);
//! `sb = w*2` → `box b` (body_b = realization[1]).
//! `sb` is body_b's EXCLUSIVE scalar cell: it does not appear in body_a's
//! backward cone, so it is pruned from the warm eval set when body_b is hidden.

#[path = "common/differential.rs"]
mod differential;

use reify_constraints::SimpleConstraintChecker;
use reify_core::{RealizationNodeId, ValueCellId};
use reify_eval::cache::NodeId;
use reify_eval::{BuildScheduler, Engine};
use reify_ir::Value;
use reify_test_support::{compile_source, MockGeometryKernel};

// ─────────────────────────────────────────────────────────────────────────────
// step-4 RED: after hide → edit_param(w, 20mm) → un-hide, body_b must
// re-realize from the CURRENT param (w=20mm, sb=40mm), not from stale sb=20mm.
// ─────────────────────────────────────────────────────────────────────────────

/// step-4 (RED until step-5): un-hiding body_b after a hidden edit must
/// re-realize from the CURRENT param value, not from the stale snapshot.
///
/// Sequence (UnifiedDag + MockGeometryKernel):
/// 1. `eval()` on `SELECTIVE_DEMAND_MULTIBODY_SRC` (`w=10mm`, both bodies at
///    their cold defaults).
/// 2. `set_demand_selective([body_a, body_b])` — both visible.
/// 3. `tessellate_snapshot()` — body_b realized; `input_cone_hash` stamped as
///    hash(`sb=20mm`) (since `sb = w*2 = 10mm*2 = 20mm`).
/// 4. `set_demand_selective([body_a])` — HIDE body_b (`sb` pruned from warm
///    eval-set via `mark_demand_pruned_pending`).
/// 5. `edit_param(w, 20mm)` — w changes to 20mm; `sb` is NOT demanded (body_b
///    hidden) so `sb` stays STALE at 20mm (the old `10mm*2` value).
///    `sa = 60mm` IS re-evaluated (body_a is visible and `sa = w*3`).
/// 6. `set_demand_selective([body_a, body_b])` — UN-HIDE body_b.
/// 7. `tessellate_snapshot()` — body_b should re-realize from CURRENT params
///    (`sb = w*2 = 20mm*2 = 40mm`). Without the fix, it re-realizes from stale
///    `sb = 20mm` (the old `10mm*2` value), producing the wrong geometry.
///
/// Oracle: a FRESH cold engine on `SELECTIVE_DEMAND_MULTIBODY_EDITED_SRC`
/// (`w=20mm` default) gives body_b's `input_cone_hash` at the CORRECT
/// `sb = 40mm`. The test asserts these hashes match.
///
/// **RED today** (without step-5): `tessellate_snapshot` copies `snapshot.values`
/// AS-IS; `hydrate_value_cell_in_loop` does NOT refresh plain arithmetic cells
/// like `sb`; so body_b re-realizes from stale `sb=20mm` → `input_cone_hash`
/// = hash(sb=20mm) ≠ oracle hash(sb=40mm). The assertion fails.
///
/// **DO NOT** assert on mesh vertex/index counts — those are size-invariant for
/// a box and would be falsely GREEN.
#[test]
fn redemand_body_b_reflects_current_param_after_hidden_edit() {
    let compiled = compile_source(differential::SELECTIVE_DEMAND_MULTIBODY_SRC);
    let e = "SelectiveMultiBody";

    let body_a = NodeId::Realization(RealizationNodeId::new(e, 0));
    let body_b_id = RealizationNodeId::new(e, 1);
    let body_b = NodeId::Realization(body_b_id.clone());
    let w = ValueCellId::new(e, "w");

    let mut engine = Engine::new(
        Box::new(SimpleConstraintChecker),
        Some(Box::new(MockGeometryKernel::new())),
    );
    engine.set_build_scheduler(BuildScheduler::UnifiedDag);

    // ── Step 1: cold eval — both bodies at w=10mm ────────────────────────────
    engine.eval(&compiled);

    // ── Step 2: selective demand — both bodies visible ────────────────────────
    engine.set_demand_selective([body_a.clone(), body_b.clone()]);
    assert!(
        !engine.demand_is_full_scope(),
        "precondition: full_scope must be OFF after set_demand_selective"
    );

    // ── Step 3: tessellate — body_b realized at w=10mm, sb=20mm ─────────────
    let _tess1 = engine
        .tessellate_snapshot(&compiled)
        .expect("tessellate_snapshot must return Some after eval()");

    // ── Step 4: hide body_b ───────────────────────────────────────────────────
    engine.set_demand_selective([body_a.clone()]);
    assert!(
        !engine.demand_is_full_scope(),
        "precondition: full_scope must be OFF after hiding body_b"
    );

    // ── Step 5: edit w → 20mm with body_b hidden ────────────────────────────
    // sa = w*3 = 60mm (re-evaluated — body_a is visible).
    // sb = w*2 stays STALE at 20mm (body_b hidden → sb NOT in demand cone).
    engine
        .edit_param(w.clone(), Value::length(0.02))
        .expect("edit_param(w, 20mm) must succeed");

    // ── Step 6: un-hide body_b ───────────────────────────────────────────────
    engine.set_demand_selective([body_a.clone(), body_b.clone()]);
    assert!(
        !engine.demand_is_full_scope(),
        "precondition: full_scope must be OFF after un-hiding body_b"
    );

    // ── Step 7: tessellate — body_b should re-realize from CURRENT params ────
    let _tess2 = engine
        .tessellate_snapshot(&compiled)
        .expect("tessellate_snapshot must return Some after the un-hide");

    // ── Actual: body_b's input_cone_hash after the un-hide + tessellate ──────
    let actual_hash = engine
        .eval_state()
        .unwrap()
        .snapshot
        .graph
        .realizations
        .get(&body_b_id)
        .unwrap()
        .input_cone_hash;

    // ── Oracle: fresh cold tessellate at w=20mm (sb = 40mm) ─────────────────
    //
    // Compile the post-edit-equivalent source (w=20mm default) and tessellate
    // from a fresh engine with a fresh MockGeometryKernel. `input_cone_hash`
    // is computed from `compute_realization_upstream_values_hash(body_b, ctx)`
    // using the CURRENT values (sb=40mm at w=20mm). This gives the EXPECTED
    // hash that the re-demand gate must produce after step-5.
    let oracle_compiled = compile_source(differential::SELECTIVE_DEMAND_MULTIBODY_EDITED_SRC);
    let mut oracle_engine = Engine::new(
        Box::new(SimpleConstraintChecker),
        Some(Box::new(MockGeometryKernel::new())),
    );
    oracle_engine.set_build_scheduler(BuildScheduler::UnifiedDag);
    oracle_engine.eval(&oracle_compiled);
    let _oracle_tess = oracle_engine
        .tessellate_snapshot(&oracle_compiled)
        .expect("oracle tessellate_snapshot must return Some");

    let oracle_hash = oracle_engine
        .eval_state()
        .unwrap()
        .snapshot
        .graph
        .realizations
        .get(&body_b_id)
        .unwrap()
        .input_cone_hash;

    // ── Assertion: body_b's geometry must reflect the CURRENT param (w=20mm) ─
    //
    // RED today (without step-5): actual_hash = hash(sb=20mm) [stale, from
    // old w=10mm*2] ≠ oracle_hash = hash(sb=40mm) [correct, w=20mm*2].
    // GREEN after step-5 refreshes stale sb before tessellation.
    assert_eq!(
        actual_hash, oracle_hash,
        "body_b's input_cone_hash after un-hide must match the w=20mm oracle \
         (sb should be re-evaluated to 40mm before geometry execution).\n\
         actual:  {actual_hash:?}\n\
         oracle:  {oracle_hash:?}\n\
         RED until step-5 implements the re-demand staleness refresh."
    );
}
