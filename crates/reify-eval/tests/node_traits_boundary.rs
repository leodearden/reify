//! Boundary tests for [`NodeTraitsMap<NodeId>`] — PRD §5 B1 / §9 T4 (precedence chain).
//!
//! This file is the PRD §9 reserved location for the full T1–T7 boundary test grid.
//! Task β seeds it with the NodeTraitsMap<NodeId> cases that can be tested without
//! any additional scheduler wiring. Later tasks (γ/δ/ζ/η/θ) will append T1–T7 here.
//!
//! All tests use real `reify_eval::cache::NodeId` values so that the
//! `impl HasNodeKind for NodeId` bridge in `reify-eval/src/cache.rs` is exercised
//! against the production type rather than a test stub.

use reify_core::{
    ComputeNodeId, ConstraintNodeId, RealizationNodeId, ResolutionNodeId, ValueCellId,
};
use reify_eval::cache::NodeId;
use reify_ir::{NodeKind, NodeTraits, NodeTraitsMap};
use reify_runtime::commitment::{CommitmentPolicy, CommitmentTracker, NodeCommitmentOverride, default_overrides};
use reify_runtime::Priority;

// ── helpers ─────────────────────────────────────────────────────────────────

fn value_node() -> NodeId {
    NodeId::Value(ValueCellId::new("E", "x"))
}

fn constraint_node() -> NodeId {
    NodeId::Constraint(ConstraintNodeId::new("E", 0))
}

fn realization_node() -> NodeId {
    NodeId::Realization(RealizationNodeId::new("E", 0))
}

fn resolution_node() -> NodeId {
    NodeId::Resolution(ResolutionNodeId::new("E", 0))
}

fn compute_node(idx: u32) -> NodeId {
    NodeId::Compute(ComputeNodeId::new("E", idx))
}

// ── default-fallback tests ───────────────────────────────────────────────────

/// Sweep all five NodeId variants against `default_traits()` in one loop.
///
/// This is the unique coverage for the production `impl HasNodeKind for NodeId`
/// bridge in `cache.rs` — unlike the reify-types unit tests which use a `TestKey`
/// stub. The loop form avoids hard-coding the per-kind expected values as literals
/// that would need to be updated in lockstep with the §7.6 table if it ever changes.
#[test]
fn node_traits_map_with_node_id_resolves_all_kind_defaults() {
    let m = NodeTraitsMap::<NodeId>::default();
    let cases: Vec<(NodeId, NodeKind)> = vec![
        (value_node(), NodeKind::Value),
        (constraint_node(), NodeKind::Constraint),
        (realization_node(), NodeKind::Realization),
        (resolution_node(), NodeKind::Resolution),
        (compute_node(0), NodeKind::Compute),
    ];
    for (node, kind) in cases {
        assert_eq!(
            m.resolve(&node),
            kind.default_traits(),
            "unexpected default for {kind:?}"
        );
    }
}

// ── T4 (lite): instance > kind precedence with real NodeId ───────────────────

#[test]
fn node_traits_map_with_node_id_instance_wins_over_kind() {
    let mut m = NodeTraitsMap::<NodeId>::default();
    // Set a kind-level override for all Compute nodes
    m.set_type(NodeKind::Compute, NodeTraits::PROGRESSIVE);
    // Set an instance-level override for one specific compute node
    let specific = compute_node(42);
    m.set_instance(specific.clone(), NodeTraits::IMMEDIATE);

    // Instance wins for the specific node
    assert_eq!(m.resolve(&specific), NodeTraits::IMMEDIATE);
    // Kind-level applies to other compute nodes
    assert_eq!(m.resolve(&compute_node(99)), NodeTraits::PROGRESSIVE);
    // Value default is unaffected
    assert_eq!(m.resolve(&value_node()), NodeTraits::IMMEDIATE);
}

// ── T2 (PRD §9 / §5 B3): default_overrides(kind, kind.default_traits()) ─────────
//
// Pins the architecture-specified commitment-override default for every NodeKind:
//   - Compute / Realization / Resolution → CommitIfSlow  (WARM_STARTABLE|COMMITTABLE has COMMITTABLE)
//   - Constraint → AlwaysCancelWhenStale                 (empty traits, no COMMITTABLE)
//   - Value → AlwaysCancelWhenStale                      (IMMEDIATE, no COMMITTABLE; Q-3 resolution)
//
// PRD §5 B3: "absent COMMITTABLE → always cancellable; present → CommitIfSlow".
// The AlwaysCancelWhenStale for Value is safe because task η/3581 (B4) will
// short-circuit Value cancellation at the scheduler before resolve_with_traits
// is wired into scheduler dispatch.

#[test]
fn t2_default_overrides_matches_arch_kind_defaults() {
    // Loop form mirrors the sibling `node_traits_map_with_node_id_resolves_all_kind_defaults`
    // to avoid per-kind literal blocks that must be updated in lockstep with the §7.6 table.
    let cases = [
        (NodeKind::Compute, NodeCommitmentOverride::CommitIfSlow), // WARM_STARTABLE|COMMITTABLE
        (NodeKind::Realization, NodeCommitmentOverride::CommitIfSlow), // WARM_STARTABLE|COMMITTABLE
        (NodeKind::Resolution, NodeCommitmentOverride::CommitIfSlow), // WARM_STARTABLE|COMMITTABLE
        (
            NodeKind::Constraint,
            NodeCommitmentOverride::AlwaysCancelWhenStale,
        ), // empty traits
        (
            NodeKind::Value,
            NodeCommitmentOverride::AlwaysCancelWhenStale,
        ), // IMMEDIATE, no COMMITTABLE (Q-3)
    ];
    for (kind, expected) in cases {
        assert_eq!(
            default_overrides(kind, kind.default_traits()),
            expected,
            "{kind:?}: default_overrides(kind, kind.default_traits()) mismatch (PRD §5 B3)"
        );
    }
}

// ── T6 (PRD §9 / §5 B4): CommitmentTracker::should_continue — never-cancel guard ─
//
// Pins the B4 priority short-circuit: P0Interactive and P1Fast nodes ALWAYS continue
// (return true) regardless of dirty-cone state or commitment status.
// P1Slow bypasses the guard and falls through to commitment logic as usual.

mod t6 {
    use super::*;
    use std::time::Duration;

    #[test]
    fn t6_immediate_priority_never_cancelled_in_dirty_cone() {
        let policy = CommitmentPolicy {
            always_commit_after: Duration::from_secs(10),
            commit_when_proportion_done: 0.5,
        };
        let mut tracker = CommitmentTracker::new(policy);
        let node = value_node();
        // Register as AlwaysCancelWhenStale (the default override for Value nodes per B3)
        tracker.register_task(node.clone(), NodeCommitmentOverride::AlwaysCancelWhenStale);
        // Leave uncommitted (no update_status called → NotYet)

        // P0Interactive: B4 guard short-circuits → always continue (never cancelled)
        assert!(
            tracker.should_continue(&node, true, Priority::P0Interactive),
            "P0Interactive node must never be cancelled (B4 short-circuit, PRD §5 B4)"
        );
        // P1Fast: B4 guard short-circuits → always continue (never cancelled)
        assert!(
            tracker.should_continue(&node, true, Priority::P1Fast),
            "P1Fast node must never be cancelled (B4 short-circuit, PRD §5 B4)"
        );
        // P1Slow: guard skipped → uncommitted in dirty cone → cancelled
        assert!(
            !tracker.should_continue(&node, true, Priority::P1Slow),
            "P1Slow uncommitted node in dirty cone must be cancelled"
        );
    }
}

// ── T7 (PRD §9 / §5 B6): CacheStore::write_intermediate — public-API boundary ─
//
// Confirms that `node_traits_mut()` and `write_intermediate()` are reachable
// through `reify_eval`'s public API from outside the crate.
//
// Authoritative behavioural matrix (positive permit / debug panic / release
// soft-invariant) lives in `crates/reify-eval/src/cache.rs` unit tests
// (task 3584 step-3/step-4 suite).  T7 is intentionally trimmed to the
// public-boundary smoke — any signature or semantic change to the API will
// be caught here without duplicating the full three-case matrix.

mod t7 {
    use super::value_node;
    use reify_core::VersionId;
    use reify_eval::cache::{CacheStore, CachedResult, NodeCache};
    use reify_eval::deps::DependencyTrace;
    use reify_ir::{DeterminacyState, Freshness, NodeTraits, Value};

    /// T7i — Public-API smoke: `node_traits_mut()` and `write_intermediate()` are
    /// accessible from outside the crate, and the positive permit (PROGRESSIVE-tagged
    /// node) works correctly through the public API.
    ///
    /// Un-gated: must hold in both debug and release profiles.
    #[test]
    fn t7i_progressive_node_permitted_both_profiles() {
        let mut store = CacheStore::new();
        let node = value_node();
        store.put(
            node.clone(),
            NodeCache::new(
                CachedResult::Value(Value::Real(0.0), DeterminacyState::Determined),
                Freshness::Final,
                DependencyTrace::default(),
                VersionId(0),
            ),
        );

        // Confirm both public methods are reachable and the positive permit holds.
        store
            .node_traits_mut()
            .set_instance(node.clone(), NodeTraits::PROGRESSIVE);

        let result = store.write_intermediate(&node, 42);
        assert!(
            result.is_none(),
            "PROGRESSIVE node must not produce a diagnostic (positive permit)"
        );
        assert_eq!(
            store.freshness(&node),
            Freshness::Intermediate { generation: 42 },
            "write_intermediate must update freshness to Intermediate{{generation:42}}"
        );
    }
}
