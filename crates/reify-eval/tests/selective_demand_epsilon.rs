//! ε (task 4741) selective-demand integration gate — engine-side rows.
//!
//! This is the integration-gate leaf (ε) for selective-demand. It pins the
//! reify-eval primitive that ε adds — a NON-gated per-realization dispatch
//! tally (`Engine::last_dispatch_count_by_realization()`) — and exercises the
//! landed β/γ/δ prune chain through the §8 boundary-table rows that live on the
//! engine side (rows 1/5/6). The GUI-side rows (debug-MCP JSON projections) live
//! in `gui/src-tauri/src/tests/commands_tests.rs`.
//!
//! ## Why a per-realization tally (and why NON-gated)
//!
//! The existing aggregate `Engine::last_dispatch_count()` is
//! `#[cfg(any(test, feature = "test-instrumentation"))]`-gated and is therefore
//! UNREACHABLE from production GUI / debug-server code (gui/src-tauri links
//! reify-eval's `test-instrumentation` feature only as a DEV-dependency). ε
//! needs to surface per-body dispatch attribution through the debug-MCP JSON in
//! a *production* build, so the new `last_dispatch_count_by_realization()`
//! accessor is NON-gated, mirroring the already-non-gated observational
//! accessors `last_eval_set()` / `last_demand_prune_measurement()`.
//!
//! The aggregate and the per-realization map both increment at the single
//! dispatch site (`engine_build.rs` `execute_realization_ops`) and reset at the
//! same 4 entry points, so `sum(map.values()) == last_dispatch_count()` at every
//! read (exact-by-construction). step-1 pins that equality (this integration
//! test build sees BOTH the gated aggregate and the non-gated map).
//!
//! Fixture: [`differential::SELECTIVE_DEMAND_MULTIBODY_SRC`] —
//! `param w : Length = 10mm`; `sa = w*3` → `box a` (body_a = realization[0]);
//! `sb = w*2` → `box b` (body_b = realization[1]).

#[path = "common/differential.rs"]
mod differential;

use reify_constraints::SimpleConstraintChecker;
use reify_core::RealizationNodeId;
use reify_eval::cache::NodeId;
use reify_eval::{BuildScheduler, Engine};
use reify_test_support::{compile_source, MockGeometryKernel};

// ─────────────────────────────────────────────────────────────────────────────
// step-1 (RED until step-2): the NEW non-gated per-realization dispatch tally
// `Engine::last_dispatch_count_by_realization()` exists and is correct on a
// cold tessellate path.
// ─────────────────────────────────────────────────────────────────────────────

/// step-1 (RED until step-2): a cold all-visible selective `tessellate_snapshot`
/// attributes geometry-kernel dispatch to each demanded realization, and the
/// per-realization tally sums to the existing aggregate `last_dispatch_count()`.
///
/// Fresh engine + `MockGeometryKernel`, `eval()` (cold cache, no prior
/// `build()`), `set_demand_selective([R0, R1])` (both bodies demanded),
/// `tessellate_snapshot()`. Asserts:
/// 1. the returned map has an entry for body_a (`RealizationNodeId::new(e, 0)`)
///    >= 1 (its `box` op dispatched);
/// 2. the returned map has an entry for body_b (`RealizationNodeId::new(e, 1)`)
///    >= 1 (its `box` op dispatched — both bodies visible);
/// 3. `sum(map.values()) == last_dispatch_count()` (the gated aggregate, visible
///    in this `cfg(test)` integration build) — exact-by-construction.
///
/// **RED today**: `last_dispatch_count_by_realization()` and its backing field
/// do not exist yet → won't compile.
#[test]
fn cold_tessellate_per_realization_tally_matches_aggregate() {
    let e = "SelectiveMultiBody";
    let compiled = compile_source(differential::SELECTIVE_DEMAND_MULTIBODY_SRC);

    // Realization roots: `let a = box(..)` → realization[0],
    //                    `let b = box(..)` → realization[1].
    let body_a_rid = RealizationNodeId::new(e, 0);
    let body_b_rid = RealizationNodeId::new(e, 1);
    let body_a = NodeId::Realization(body_a_rid.clone());
    let body_b = NodeId::Realization(body_b_rid.clone());

    let mut engine = Engine::new(
        Box::new(SimpleConstraintChecker),
        Some(Box::new(MockGeometryKernel::new())),
    );
    engine.set_build_scheduler(BuildScheduler::UnifiedDag);
    // Cold eval → populates eval_state; does NOT warm the RealizationCache.
    engine.eval(&compiled);
    // Demand both realizations (selective, not full_scope).
    engine.set_demand_selective([body_a.clone(), body_b.clone()]);
    engine
        .tessellate_snapshot(&compiled)
        .expect("tessellate_snapshot must return Some after eval()");

    // ── The NEW non-gated per-realization tally ──────────────────────────────
    let tally = engine.last_dispatch_count_by_realization();

    let a_count = tally.get(&body_a_rid).copied().unwrap_or(0);
    let b_count = tally.get(&body_b_rid).copied().unwrap_or(0);

    assert!(
        a_count >= 1,
        "body_a (SelectiveMultiBody#realization[0]) must have dispatched at least \
         one geometry op on a cold all-visible tessellate; got {a_count}"
    );
    assert!(
        b_count >= 1,
        "body_b (SelectiveMultiBody#realization[1]) must have dispatched at least \
         one geometry op when both bodies are demanded; got {b_count}"
    );

    // ── Exact-by-construction: sum(map) == aggregate ─────────────────────────
    // Both counters increment only at the single dispatch site and reset at the
    // same 4 entry points, so the per-realization map sums to the aggregate at
    // every read. The aggregate accessor is test-instrumentation-gated but
    // visible in this integration (cfg(test)) build.
    let sum: usize = tally.values().copied().sum();
    assert_eq!(
        sum,
        engine.last_dispatch_count(),
        "sum of the per-realization dispatch tally must equal the aggregate \
         last_dispatch_count() (both increment at the same site, reset at the \
         same entry points): sum={sum} vs aggregate={}",
        engine.last_dispatch_count(),
    );
}
