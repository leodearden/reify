//! γ (task 4739) selective-demand integration test.
//!
//! Pins arch §8 prune-safety scenario 3 end-to-end: after a warm
//! `tessellate_snapshot` under a selective demand that hides body_b, body_b's
//! exclusive value cell `sb` is reported as `Pending` (NOT a silently-stale
//! `Final` number) AND its last-substantive value is preserved and surfaced via
//! `Engine::last_substantive_value`. The visible body_a's cell `sa` stays
//! `Final`.
//!
//! Fixture: [`SELECTIVE_DEMAND_MULTIBODY_SRC`] from `common/differential.rs` —
//! `param w` → `sa = w*3` → box `a` (realization[0]); `sb = w*2` → box `b`
//! (realization[1]).

#[path = "common/differential.rs"]
mod differential;

use reify_constraints::SimpleConstraintChecker;
use reify_core::{RealizationNodeId, ValueCellId};
use reify_eval::cache::NodeId;
use reify_eval::{BuildScheduler, Engine};
use reify_ir::Freshness;
use reify_test_support::{compile_source, MockGeometryKernel};

/// step-9 (RED until step-10): a warm `tessellate_snapshot` under selective
/// demand flips the hidden body's cell to `Pending` while preserving its
/// last-substantive value; the visible body's cell stays `Final`.
///
/// RED today: the warm build surface does not yet invoke
/// `mark_demand_pruned_pending`, so `sb` keeps its stale `Final` freshness from
/// the cold eval and the `Pending` assertion fails. GREEN after step-10 wires
/// the producer into the warm snapshot surfaces.
#[test]
fn warm_tessellate_snapshot_marks_hidden_body_cell_pending_and_preserves_last_value() {
    let e = "SelectiveMultiBody";
    let compiled = compile_source(differential::SELECTIVE_DEMAND_MULTIBODY_SRC);

    // Realization roots: `let a = box(..)` → realization[0] (visible),
    //                    `let b = box(..)` → realization[1] (hidden).
    let body_a = NodeId::Realization(RealizationNodeId::new(e, 0));
    let sa = NodeId::Value(ValueCellId::new(e, "sa")); // body_a exclusive cell
    let sb = NodeId::Value(ValueCellId::new(e, "sb")); // body_b exclusive cell

    let mut engine = Engine::new(
        Box::new(SimpleConstraintChecker),
        Some(Box::new(MockGeometryKernel::new())),
    );
    engine.set_build_scheduler(BuildScheduler::UnifiedDag);

    // Cold full-scope eval: every cell (incl. sb) is cached Final with a value.
    engine.eval(&compiled);
    assert_eq!(
        engine.freshness(&sb),
        Freshness::Final,
        "precondition: sb is Final after a cold full-scope eval"
    );
    let sb_prior = engine.last_substantive_value(&sb);
    assert!(
        sb_prior.is_some(),
        "precondition: sb has a substantive cached value after eval"
    );

    // Hide body_b (demand only body_a). `full_scope` flips OFF.
    engine.set_demand_selective([body_a.clone()]);

    // Warm tessellate_snapshot: the γ producer flips pruned-Final `sb` → Pending.
    engine
        .tessellate_snapshot(&compiled)
        .expect("tessellate_snapshot must return Some after eval()");

    // §8 scenario 3: the hidden body's cell is Pending (never a stale Final).
    assert!(
        matches!(engine.freshness(&sb), Freshness::Pending { .. }),
        "hidden body_b's cell sb must be Pending after the warm selective build, \
         got {:?}",
        engine.freshness(&sb)
    );

    // The last-substantive value is preserved (mark_pending keeps entry.result),
    // so the GUI can display the last good value rather than a stale current one.
    assert_eq!(
        engine.last_substantive_value(&sb),
        sb_prior,
        "sb's last-substantive value must be preserved after the prune→Pending flip"
    );

    // The visible body_a's cell stays Final (it is in the demand cone).
    assert_eq!(
        engine.freshness(&sa),
        Freshness::Final,
        "visible body_a's cell sa must stay Final"
    );
}
