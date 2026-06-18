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
use reify_runtime::commitment::{default_overrides, NodeCommitmentOverride};

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
        (NodeKind::Compute,      NodeCommitmentOverride::CommitIfSlow),       // WARM_STARTABLE|COMMITTABLE
        (NodeKind::Realization,  NodeCommitmentOverride::CommitIfSlow),       // WARM_STARTABLE|COMMITTABLE
        (NodeKind::Resolution,   NodeCommitmentOverride::CommitIfSlow),       // WARM_STARTABLE|COMMITTABLE
        (NodeKind::Constraint,   NodeCommitmentOverride::AlwaysCancelWhenStale), // empty traits
        (NodeKind::Value,        NodeCommitmentOverride::AlwaysCancelWhenStale), // IMMEDIATE, no COMMITTABLE (Q-3)
    ];
    for (kind, expected) in cases {
        assert_eq!(
            default_overrides(kind, kind.default_traits()),
            expected,
            "{kind:?}: default_overrides(kind, kind.default_traits()) mismatch (PRD §5 B3)"
        );
    }
}

// ── T5 (PRD §9 / §5 B5): bidirectional default_traits ↔ WarmStartableRegistry ──
//
// These cases pin that `ConcurrentScheduler::execute_with_config` consults the
// optional `warm_startable_registry` field on `SchedulerConfig` and forwards it
// to `reify_runtime::assert_warm_startable_coextensive` (debug-builds only,
// fires once per execute call after the empty-eval-set short-circuit).
//
// Scope: this file pins the scheduler **wiring** — that `execute_with_config`
// actually invokes the assertion. The underlying invariant (both directions
// of the coextension) is already pinned by the runtime-internal unit tests
// in `crates/reify-runtime/src/warm_startable_assert.rs`
// (`empty_registry_panics_declared_without_registered` /
// `extra_value_panics_registered_without_declared`); one T5 case here
// demonstrates the wiring observation without re-pinning the invariant a
// second time at a higher cost (full cyclic dev-dep + tokio runtime).
//
// Test vehicle uses a single-node eval set with a no-op evaluator so the
// scheduler reaches the assertion site. The assertion firing — or not firing —
// is the observable signal; the actual node evaluation is incidental.
//
// The whole T5 region is gated on `#[cfg(debug_assertions)]` (one gate on the
// `t5` submodule, not six per-item gates) because the only remaining case
// (T5a) is itself debug-only — the release-mode no-op invariant is pinned
// more cheaply by `release_mode_no_op_on_empty_registry` in
// `crates/reify-runtime/src/warm_startable_assert.rs`. Module-level gating
// avoids the per-item-cfg drift hazard (where one item silently drops its
// cfg and fails to compile in release).

#[cfg(debug_assertions)]
mod t5 {
    use super::*;

    use std::collections::{HashMap, HashSet};
    use std::sync::Arc;

    use reify_eval::cache::EvalOutcome;
    use reify_eval::deps::DependencyTrace;
    use reify_ir::WarmStartableRegistry;
    use reify_runtime::concurrent::{
        AsyncNodeEvaluator, CancellationToken, ConcurrentScheduler, SchedulerConfig,
    };

    /// Minimal no-op evaluator: every node evaluates to `Changed`. The T5 cases
    /// don't care about the evaluation result — they care only whether the
    /// pre-spawn assertion fires.
    struct NoopEvaluator;

    impl AsyncNodeEvaluator for NoopEvaluator {
        async fn evaluate(&self, _node: NodeId) -> EvalOutcome {
            EvalOutcome::Changed
        }
    }

    /// Common scheduler-driver harness for T5 cases.
    ///
    /// Builds a single-node eval set with the given fixture registry attached
    /// to the config, then awaits the scheduler. The registry assertion runs
    /// in `execute_with_config` above the empty-eval-set short-circuit, so a
    /// single-node set is overkill for the assertion's sake — it is kept here
    /// to exercise the surrounding scheduler wiring rather than reach for the
    /// invariant by a different path.
    async fn drive_scheduler_with_registry(registry: WarmStartableRegistry) {
        // Single Compute node, no upstream reads — dirty by safety default.
        let node = compute_node(0);
        let eval_set = vec![node.clone()];
        let mut traces = HashMap::new();
        traces.insert(node, DependencyTrace::default());

        let config = SchedulerConfig {
            warm_startable_registry: Some(registry),
            ..SchedulerConfig::default()
        };

        let cancel = CancellationToken::new();
        let changed_cells = HashSet::new();
        let scheduler = ConcurrentScheduler;
        let evaluator = Arc::new(NoopEvaluator);

        // The assertion fires synchronously in `execute_with_config` before any
        // spawn — `.await` is required because the function is async even though
        // the panic happens pre-spawn in debug builds.
        let _ = scheduler
            .execute_with_config(
                eval_set,
                evaluator,
                &traces,
                &cancel,
                &changed_cells,
                config,
            )
            .await;
    }

    /// T5a — declared-without-registered (debug): an empty `WarmStartableRegistry`
    /// trips the bidirectional assertion because Realization / Resolution / Compute
    /// all declare `WARM_STARTABLE` via `default_traits()` but no producer
    /// registered presence. PRD §5 B5 / I-3 (M-013 fix).
    ///
    /// This is the single T5 wiring-pin case: it demonstrates that
    /// `execute_with_config` reaches the assertion site. The opposite
    /// (registered-without-declared) direction of the coextension is covered by
    /// the runtime-internal unit test `extra_value_panics_registered_without_declared`
    /// in `crates/reify-runtime/src/warm_startable_assert.rs`; pinning it a
    /// second time here would add a redundant tokio-runtime + cyclic dev-dep
    /// without strengthening the observation.
    ///
    /// The release-mode no-op invariant (empty registry compiles to no-op under
    /// `cfg(not(debug_assertions))`) is covered by
    /// `release_mode_no_op_on_empty_registry` in
    /// `crates/reify-runtime/src/warm_startable_assert.rs`; a parallel `T5c` case
    /// here would duplicate that observation at the higher cost of a tokio runtime
    /// and a cyclic dev-dep without strengthening it.
    #[tokio::test]
    #[should_panic(expected = "WarmStartableRegistry presence")]
    async fn t5a_empty_registry_panics_in_debug() {
        let empty = WarmStartableRegistry::new();
        drive_scheduler_with_registry(empty).await;
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
    use reify_eval::cache::{CachedResult, CacheStore, NodeCache};
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
