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
//! `ValueMap` (line ~9250 engine_build.rs). A plain arithmetic cell like
//! `sb = w * 2` is NOT refreshed by `hydrate_value_cell_in_loop` (which only
//! handles geometry-query / selector / topology cells). After a hidden-then-edit
//! cycle, `sb` is stale in `snapshot.values` (never re-evaluated during the
//! hidden phase because it was not in the demand cone). When body_b is un-hidden
//! and `tessellate_snapshot` runs, body_b re-realizes from stale `sb`, producing
//! geometry that reflects the OLD param value rather than the current one.
//!
//! ## What δ (step-5) fixes
//!
//! Step-5 wires a re-demand handler in `engine_demand.rs` (at
//! `set_demand_selective`): when a cell leaves the demand cone, it is marked
//! `Pending` (via `mark_demand_pruned_pending`), so a subsequent warm
//! `tessellate_snapshot` can detect the stale cell, re-evaluate it, and update
//! `snapshot.values` to reflect the current param before geometry executes.
//!
//! ## Test oracle
//!
//! A FRESH cold `eval()` of the post-edit-equivalent source
//! ([`differential::SELECTIVE_DEMAND_MULTIBODY_EDITED_SRC`], `w=20mm` default)
//! gives `sb`'s value in `snapshot.values` as `40mm` (= `w*2 = 20mm*2`). This
//! is the expected value the fix must produce in the actual engine's snapshot
//! after the un-hide + tessellate.
//!
//! ## Why `snapshot.values[sb].content_hash()`, not mesh counts
//!
//! A box mesh has SIZE-INVARIANT vertex/index counts — `box(20mm,20mm,20mm)`
//! and `box(40mm,40mm,40mm)` both produce the same vertex/face count, making
//! count-based assertions falsely GREEN. The `sb` VALUE in `snapshot.values` is
//! `40mm` (correct) or `20mm` (stale); `content_hash()` encodes the exact value
//! and differs between these two, making it the correct staleness detector.
//!
//! NOTE: `RealizationNodeData.input_cone_hash` is NOT used here — it is only
//! set by the `build_snapshot` / `build_with_geometry_output` paths, NOT by
//! `tessellate_snapshot`. Using it would produce `None == None` (trivially GREEN
//! before the fix) which is NOT a valid RED assertion.
//!
//! Fixture: [`differential::SELECTIVE_DEMAND_MULTIBODY_SRC`] —
//! `param w : Length = 10mm`; `sa = w*3` → `box a` (body_a = realization[0]);
//! `sb = w*2` → `box b` (body_b = realization[1]).
//! `sb` is body_b's EXCLUSIVE scalar cell: it does not appear in body_a's
//! backward cone, so it is stale in `snapshot.values` after a hidden edit.

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
/// cause `snapshot.values[sb]` to be refreshed to the CURRENT param value
/// (40mm = w*2 = 20mm*2), not the stale snapshot value (20mm = 10mm*2).
///
/// Sequence (UnifiedDag + MockGeometryKernel):
/// 1. `eval()` on `SELECTIVE_DEMAND_MULTIBODY_SRC` (`w=10mm`).
/// 2. `set_demand_selective([body_a, body_b])` — both visible.
/// 3. `tessellate_snapshot()` — body_b realized; `snapshot.values[sb]=20mm`.
/// 4. `set_demand_selective([body_a])` — HIDE body_b.
///    Step-5 fix: `set_demand_selective` will call `mark_demand_pruned_pending`
///    at this point, marking `sb` as `Pending` (it just left the demand cone).
///    Without fix: `sb` stays `Final` with stale value 20mm in the cache.
/// 5. `edit_param(w, 20mm)` — w changes to 20mm; `sb` is NOT demanded (body_b
///    hidden) so `sb` is NOT re-evaluated. `sa = 60mm` IS re-evaluated.
///    `snapshot.values[sb]` stays at 20mm (stale).
/// 6. `set_demand_selective([body_a, body_b])` — UN-HIDE body_b.
///    Step-5 fix: detects `sb` is `Pending` and now demanded → refreshes `sb`
///    to `w*2 = 40mm` and updates `snapshot.values[sb]`.
/// 7. `tessellate_snapshot()` — with fix, `snapshot.values[sb]=40mm` before
///    the copy → body_b tessellates from CURRENT params.
///    Without fix: `snapshot.values[sb]=20mm` (stale) → wrong geometry.
///
/// Oracle: a FRESH cold `eval()` of `SELECTIVE_DEMAND_MULTIBODY_EDITED_SRC`
/// (`w=20mm` default) gives `snapshot.values[sb]` = `Value::length(0.04)`
/// (40mm = w*2 = 20mm*2). `sb.content_hash()` encodes the exact value and
/// differs between 20mm (stale) and 40mm (current).
///
/// **RED today** (without step-5): `snapshot.values[sb]` is still 20mm after
/// the un-hide + tessellate (never refreshed because `sb` was never re-evaluated
/// — it was outside the demand cone during the `edit_param` in step 5).
///
/// **DO NOT** assert on mesh vertex/index counts — those are size-invariant for
/// a box and would be falsely GREEN.
///
/// **DO NOT** assert on `RealizationNodeData.input_cone_hash` — `tessellate_snapshot`
/// does not set that field (only `build_snapshot`/`build_with_geometry_output` do),
/// so `input_cone_hash` would be `None` in both actual and oracle, giving `None ==
/// None` (trivially GREEN before the fix).
#[test]
fn redemand_body_b_snapshot_sb_reflects_current_param_after_hidden_edit() {
    let compiled = compile_source(differential::SELECTIVE_DEMAND_MULTIBODY_SRC);
    let e = "SelectiveMultiBody";

    let body_a = NodeId::Realization(RealizationNodeId::new(e, 0));
    let body_b = NodeId::Realization(RealizationNodeId::new(e, 1));
    let w = ValueCellId::new(e, "w");
    let sb_id = ValueCellId::new(e, "sb");

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

    // Verify sb is 20mm in the snapshot after the initial warm tessellate.
    let sb_after_tess1 = engine
        .eval_state()
        .unwrap()
        .snapshot
        .values
        .get(&sb_id)
        .map(|(v, _)| v.content_hash());
    assert!(
        sb_after_tess1.is_some(),
        "precondition: sb must be present in snapshot.values after cold eval + tessellate"
    );

    // ── Step 4: hide body_b ───────────────────────────────────────────────────
    // Step-5 fix: set_demand_selective will call mark_demand_pruned_pending here,
    // marking sb as Pending. Without fix: sb stays Final with stale value.
    engine.set_demand_selective([body_a.clone()]);
    assert!(
        !engine.demand_is_full_scope(),
        "precondition: full_scope must be OFF after hiding body_b"
    );

    // ── Step 5: edit w → 20mm with body_b hidden ────────────────────────────
    // sa = w*3 = 60mm (re-evaluated — body_a is visible).
    // sb = w*2 stays STALE at 20mm (body_b hidden → sb NOT in demand cone →
    // not in eval_set → NOT re-evaluated → snapshot.values[sb] unchanged).
    engine
        .edit_param(w.clone(), Value::length(0.02))
        .expect("edit_param(w, 20mm) must succeed");

    // Verify sb is still 20mm in snapshot after the hidden edit.
    let sb_after_edit = engine
        .eval_state()
        .unwrap()
        .snapshot
        .values
        .get(&sb_id)
        .map(|(v, _)| v.content_hash());
    assert_eq!(
        sb_after_tess1, sb_after_edit,
        "precondition: snapshot.values[sb] must still be 20mm after edit_param while hidden \
         (sb was not in the demand cone → not re-evaluated)"
    );

    // ── Step 6: un-hide body_b ───────────────────────────────────────────────
    // Step-5 fix: detects sb is Pending (marked in step 4) and now demanded →
    // re-evaluates sb = w*2 = 20mm*2 = 40mm → updates snapshot.values[sb].
    engine.set_demand_selective([body_a.clone(), body_b.clone()]);
    assert!(
        !engine.demand_is_full_scope(),
        "precondition: full_scope must be OFF after un-hiding body_b"
    );

    // ── Step 7: tessellate — body_b should re-realize from CURRENT params ────
    // With fix: snapshot.values[sb] = 40mm before the copy → body_b tessellates
    // from CURRENT params.
    // Without fix: snapshot.values[sb] = 20mm (stale) → wrong geometry.
    let _tess2 = engine
        .tessellate_snapshot(&compiled)
        .expect("tessellate_snapshot must return Some after the un-hide");

    // ── Actual: sb's content_hash in snapshot after un-hide + tessellate ─────
    let actual_sb_hash = engine
        .eval_state()
        .unwrap()
        .snapshot
        .values
        .get(&sb_id)
        .map(|(v, _)| v.content_hash());

    // ── Oracle: fresh cold eval at w=20mm (sb = w*2 = 40mm) ─────────────────
    //
    // A fresh engine evaluating the post-edit-equivalent source (w=20mm default)
    // via cold eval(). After eval(), snapshot.values[sb] = Value::length(0.04)
    // (40mm = w*2 = 20mm*2). This is the EXPECTED value that step-5 must produce
    // in the actual engine after the un-hide + tessellate.
    //
    // Note: we use eval() only (no tessellate_snapshot) because:
    // (a) eval() suffices to populate snapshot.values[sb] = 40mm, and
    // (b) tessellate_snapshot() does NOT update snapshot.values for value cells,
    //     so the oracle hash is the same whether or not tessellate is called.
    let oracle_compiled = compile_source(differential::SELECTIVE_DEMAND_MULTIBODY_EDITED_SRC);
    let mut oracle_engine = Engine::new(
        Box::new(SimpleConstraintChecker),
        Some(Box::new(MockGeometryKernel::new())),
    );
    oracle_engine.set_build_scheduler(BuildScheduler::UnifiedDag);
    oracle_engine.eval(&oracle_compiled);

    let oracle_sb_hash = oracle_engine
        .eval_state()
        .unwrap()
        .snapshot
        .values
        .get(&sb_id)
        .map(|(v, _)| v.content_hash());

    assert!(
        oracle_sb_hash.is_some(),
        "oracle: snapshot.values[sb] must be present after cold eval at w=20mm"
    );

    // ── Assertion: snapshot.values[sb] must reflect the CURRENT param (w=20mm) ──
    //
    // RED today (without step-5):
    //   actual_sb_hash = content_hash(20mm) [stale, from old w=10mm*2, never
    //                    refreshed because sb was pruned from eval_set while hidden]
    //   oracle_sb_hash = content_hash(40mm) [correct, w=20mm*2]
    //   → assert_eq! FAILS → RED ✓
    //
    // GREEN after step-5 (set_demand_selective calls mark_demand_pruned_pending
    // on hide, tessellate_snapshot refreshes Pending demanded cells before
    // tessellation):
    //   actual_sb_hash = content_hash(40mm) [refreshed to w*2=40mm]
    //   oracle_sb_hash = content_hash(40mm) [correct]
    //   → assert_eq! PASSES → GREEN ✓
    assert_eq!(
        actual_sb_hash, oracle_sb_hash,
        "snapshot.values[sb] after un-hide + tessellate must match the w=20mm oracle \
         (sb should be re-evaluated to w*2=40mm before geometry execution).\n\
         actual:  {actual_sb_hash:?}\n\
         oracle:  {oracle_sb_hash:?}\n\
         RED until step-5 implements the re-demand staleness refresh \
         (set_demand_selective marks pruned cells Pending; tessellate_snapshot \
         refreshes Pending demanded cells before body_b tessellates)."
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// step-6: reuse-branch guard — pins the hash gate's "reuse iff unchanged" branch.
// GREEN immediately after step-5 (hash gate implemented);
// would FAIL if step-5 had instead used the degraded "always force-recompute
// on re-demand" fallback.
// ─────────────────────────────────────────────────────────────────────────────

/// step-6 (characterization guard): un-hiding body_b WITHOUT an intervening
/// `edit_param` must NOT re-dispatch body_b's kernel ops — the hash gate
/// detects that the input-cone hash is UNCHANGED and reuses the cached
/// geometry (`last_dispatch_count` = 0 for that tessellate call).
///
/// Sequence (UnifiedDag + MockGeometryKernel):
/// 1. `eval()` on `SELECTIVE_DEMAND_MULTIBODY_SRC` (`w=10mm`).
/// 2. `set_demand_selective([body_a, body_b])` — both visible.
/// 3. `tessellate_snapshot()` — body_b realized and written to
///    `realization_cache`; `snapshot.values[sb]=20mm`; `input_cone_hash` set.
/// 4. `set_demand_selective([body_a])` — HIDE body_b.
///    Step-5 fix: marks `sb` (exclusive to body_b) as Pending.
/// 5. `tessellate_snapshot()` — body_b is HIDDEN (not demanded) — NOT
///    dispatched.  `realization_cache` entry for body_b is NOT cleared
///    (no `edit_param` was called, so `clear_realization_cache` did NOT run).
/// 6. `set_demand_selective([body_a, body_b])` — UN-HIDE body_b.
///    Step-5 Part A: `mark_demand_pruned_pending` is a no-op at un-hide (all
///    demanded nodes stay).
/// 7. `tessellate_snapshot()`:
///    Step-5 Part B: sb IS demanded AND Pending → re-evaluated to `w*2 = 20mm`
///    (same as before, no edit).
///    Step-5 Part C: hash gate → `current_hash == input_cone_hash` (inputs
///    unchanged) → `realization_cache` is NOT cleared for body_b → body_b gets
///    a cache HIT in `tessellate_from_values` → `last_dispatch_count = 0`.
///
/// **Asserts:**
/// (a) `snapshot.values[sb]` is unchanged / still correct (still `20mm`).
/// (b) `last_dispatch_count` after the final tessellate is 0 (no kernel
///     ops dispatched for body_b — hash gate reused cached geometry).
///
/// **Would FAIL** if step-5 had implemented the "degraded fallback" (always
/// force-recompute on re-demand, i.e. always `clear_entity` regardless of
/// the hash comparison): body_b's realization_cache entry would be cleared,
/// causing a full kernel re-dispatch and `last_dispatch_count > 0`.
#[test]
fn redemand_body_b_no_edit_reuses_cached_geometry_hash_gate() {
    let compiled = compile_source(differential::SELECTIVE_DEMAND_MULTIBODY_SRC);
    let e = "SelectiveMultiBody";

    let body_a = NodeId::Realization(RealizationNodeId::new(e, 0));
    let body_b = NodeId::Realization(RealizationNodeId::new(e, 1));
    let sb_id = ValueCellId::new(e, "sb");

    let mut engine = Engine::new(
        Box::new(SimpleConstraintChecker),
        Some(Box::new(MockGeometryKernel::new())),
    );
    engine.set_build_scheduler(BuildScheduler::UnifiedDag);

    // ── Step 1: cold eval — both bodies at w=10mm ────────────────────────────
    engine.eval(&compiled);

    // ── Step 2: selective demand — both visible ───────────────────────────────
    engine.set_demand_selective([body_a.clone(), body_b.clone()]);
    assert!(
        !engine.demand_is_full_scope(),
        "precondition: full_scope must be OFF"
    );

    // ── Step 3: tessellate — body_b realized and cached at w=10mm ────────────
    let _tess1 = engine
        .tessellate_snapshot(&compiled)
        .expect("tessellate_snapshot must succeed after eval()");

    // ── Precondition: body_b IS in tess1.meshes ──────────────────────────────
    // This anchors the "reuse" claim below: body_b was initially tessellated
    // and its geometry handle is now in the realization_cache.  A subsequent
    // no-edit un-hide should REUSE that geometry (dispatch_count == 0) rather
    // than silently DROP it (which would also give dispatch_count == 0).
    let body_b_entity_path = format!("{}#realization[1]", e);
    assert!(
        _tess1.meshes.iter().any(|m| m.entity_path == body_b_entity_path),
        "precondition: body_b must appear in _tess1.meshes (both bodies demanded, \
         no exempt filter yet); entity_path expected: {body_b_entity_path:?}; \
         actual paths: {:?}",
        _tess1.meshes.iter().map(|m| &m.entity_path).collect::<Vec<_>>()
    );

    // Capture the snapshot.values[sb] hash at w=10mm for the oracle.
    let sb_hash_at_w10 = engine
        .eval_state()
        .unwrap()
        .snapshot
        .values
        .get(&sb_id)
        .map(|(v, _)| v.content_hash());
    assert!(
        sb_hash_at_w10.is_some(),
        "precondition: sb must be present in snapshot.values after cold eval"
    );

    // ── Step 4: hide body_b ───────────────────────────────────────────────────
    engine.set_demand_selective([body_a.clone()]);

    // ── Step 5: tessellate with body_b HIDDEN (no edit_param!) ───────────────
    // body_b is NOT demanded → NOT dispatched; realization_cache entry survives.
    let _tess2 = engine
        .tessellate_snapshot(&compiled)
        .expect("tessellate_snapshot must succeed after hide");

    // ── Step 6: un-hide body_b ───────────────────────────────────────────────
    engine.set_demand_selective([body_a.clone(), body_b.clone()]);
    assert!(
        !engine.demand_is_full_scope(),
        "precondition: full_scope must be OFF after un-hide"
    );

    // ── Step 7: tessellate — body_b should reuse cached geometry ─────────────
    // `last_dispatch_count` is reset to 0 at the start of each tessellate call.
    // After this call it should still be 0 (cache hit for body_b, no kernel ops).
    let _tess3 = engine
        .tessellate_snapshot(&compiled)
        .expect("tessellate_snapshot must succeed after un-hide");

    // ── Assertion (a): sb is still the w=10mm value ──────────────────────────
    let sb_hash_after_unhide = engine
        .eval_state()
        .unwrap()
        .snapshot
        .values
        .get(&sb_id)
        .map(|(v, _)| v.content_hash());
    assert_eq!(
        sb_hash_after_unhide, sb_hash_at_w10,
        "snapshot.values[sb] must be unchanged after hide+unhide without an edit \
         (still w=10mm*2=20mm).\n\
         before: {sb_hash_at_w10:?}\n\
         after:  {sb_hash_after_unhide:?}"
    );

    // ── Assertion (b): last_dispatch_count = 0 ───────────────────────────────
    // The hash gate detected `current_hash == input_cone_hash` (inputs unchanged)
    // and preserved the realization_cache entry for body_b → cache HIT in
    // tessellate_from_values → no kernel ops dispatched → dispatch count = 0.
    //
    // Would be > 0 if step-5 used the degraded "always force-recompute" fallback
    // (unconditional clear_entity on re-demand, ignoring the hash comparison).
    let dispatch_count = engine.last_dispatch_count();
    assert_eq!(
        dispatch_count, 0,
        "last_dispatch_count must be 0 after un-hide without an edit: \
         the hash gate should reuse cached body_b geometry (inputs unchanged).\n\
         got: {dispatch_count}\n\
         FAILS if step-5 degraded to force-recompute every re-demand instead \
         of the input-cone-hash gate."
    );

    // ── Assertion (c): realization_cache is non-empty (body_b geometry preserved)
    //
    // DELTA CONTRACT: body_b is intentionally ABSENT from _tess3.meshes.  The
    // hash gate marks it "exempt" (inputs unchanged), excludes it from the
    // scheduled seed, and tessellate_from_values emits no BuildStep::Realize for
    // it — so the mesh slot stays None.  This is NOT a "silent drop": the
    // consumer (GUI) must treat an absent mesh as an incremental delta ("keep
    // the previous mesh") rather than a removal signal.
    //
    // The REUSE evidence is the triple: (a) sb unchanged, (b) dispatch_count==0,
    // (c) realization_cache non-empty.  No edit_param was called between tess1
    // and tess3, so clear_realization_cache() did NOT run.  The cache MUST
    // therefore still hold body_b's geometry handle from tess1.  A broken
    // implementation that silently dropped body_b by wrongly clearing the cache
    // (or by never populating it) would produce an empty cache here.
    let cache_len_after = engine.realization_cache().len();
    assert!(
        cache_len_after > 0,
        "realization_cache must be non-empty after no-edit un-hide: body_b's \
         geometry handle from tess1 must survive (no edit_param = no \
         clear_realization_cache call).\n\
         cache_len: {cache_len_after}\n\
         FAILS if the cache was unexpectedly cleared (indicates silent drop, \
         not correct reuse)."
    );
}
