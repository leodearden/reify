//! Commitment policy for controlling when speculative evaluation results
//! become committed (run to completion regardless of subsequent edits).
//!
//! Implements a dual-threshold system per §7.3 of the architecture docs:
//! - `always_commit_after`: commits unconditionally after elapsed time
//! - `commit_when_proportion_done`: commits based on estimated progress
//!
//! Per-node overrides allow: 'commit if slow' (default), 'always cancel
//! when stale', and 'only run on final inputs'.

use std::collections::HashMap;
use std::time::Duration;

use reify_eval::cache::NodeId;
// Re-export the canonical NodeKind from reify-types so all existing call sites
// (including reify_runtime::commitment::NodeKind in tests and concurrent_eval.rs)
// continue to resolve transparently. The From<&NodeId> bridge impl lives in
// reify-eval/src/cache.rs (the only orphan-rule-clean host; see PRD §4).
pub use reify_ir::NodeKind;

/// Project-level configuration for the dual-threshold commitment policy.
///
/// Controls when a running speculative evaluation becomes "committed" —
/// meaning it will run to completion even if subsequent edits arrive.
///
/// - `always_commit_after`: unconditionally commit after this elapsed time
/// - `commit_when_proportion_done`: commit when estimated progress exceeds this fraction
#[derive(Clone, Debug, PartialEq)]
pub struct CommitmentPolicy {
    /// Unconditionally commit after this elapsed time (default: 120s).
    pub always_commit_after: Duration,
    /// Commit when estimated progress exceeds this fraction (default: 0.5).
    pub commit_when_proportion_done: f64,
}

/// Per-node override for commitment behavior.
///
/// Each node can override the project-level commitment policy:
/// - `CommitIfSlow` (default): apply the dual-threshold policy
/// - `AlwaysCancelWhenStale`: never commit, always cancel when stale
/// - `OnlyRunOnFinalInputs`: skip intermediate inputs entirely
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NodeCommitmentOverride {
    /// Apply the dual-threshold commitment policy (default behavior).
    #[default]
    CommitIfSlow,
    /// Never commit — always cancel when inputs become stale.
    AlwaysCancelWhenStale,
    /// Only run when all inputs are final (skip intermediate evaluations).
    OnlyRunOnFinalInputs,
}

/// Per-node commitment policy overrides, settable per instance and per type.
///
/// Implements the precedence chain from architecture §7.3 (lines 751–767):
///   1. **Instance override** — highest priority; set via [`set_instance`](Self::set_instance)
///   2. **Type override** — applied by [`NodeKind`]; set via [`set_type`](Self::set_type)
///   3. **Default** — [`NodeCommitmentOverride::CommitIfSlow`] (lowest priority)
#[derive(Clone, Debug, Default)]
pub struct NodePolicyOverrides {
    instance_overrides: HashMap<NodeId, NodeCommitmentOverride>,
    type_overrides: HashMap<NodeKind, NodeCommitmentOverride>,
}

impl NodePolicyOverrides {
    /// Create a new, empty set of overrides (alias for [`Default::default`]).
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a per-instance override for `node_id`.
    ///
    /// Overwrites any previous value for this node.
    pub fn set_instance(&mut self, node_id: NodeId, override_: NodeCommitmentOverride) {
        self.instance_overrides.insert(node_id, override_);
    }

    /// Set a per-type override for all nodes of the given [`NodeKind`].
    ///
    /// Overwrites any previous value for this kind.
    pub fn set_type(&mut self, kind: NodeKind, override_: NodeCommitmentOverride) {
        self.type_overrides.insert(kind, override_);
    }

    /// Resolve the effective [`NodeCommitmentOverride`] for `node_id`.
    ///
    /// Precedence (highest → lowest):
    /// 1. Instance override (if set for this exact node)
    /// 2. Type override (if set for the node's [`NodeKind`])
    /// 3. [`NodeCommitmentOverride::default()`] (`CommitIfSlow`)
    pub fn resolve(&self, node_id: &NodeId) -> NodeCommitmentOverride {
        if let Some(o) = self.instance_overrides.get(node_id) {
            return *o;
        }
        if let Some(o) = self.type_overrides.get(&NodeKind::from(node_id)) {
            return *o;
        }
        NodeCommitmentOverride::default()
    }
}

/// Progress information for a running task, used by the commitment decision function.
///
/// Captures elapsed time, optional self-reported progress, and optional
/// previous runtime for fallback estimation.
#[derive(Clone, Debug)]
pub struct TaskProgress {
    /// How long the task has been running.
    pub elapsed: Duration,
    /// Self-reported progress fraction (0.0–1.0), if available.
    pub reported_progress: Option<f64>,
    /// Previous runtime for this node (used for fallback estimation).
    pub previous_runtime: Option<Duration>,
}

impl TaskProgress {
    /// Estimate the task's progress as a fraction.
    ///
    /// Returns:
    /// - `reported_progress` if available
    /// - `elapsed / previous_runtime` if previous runtime is available and nonzero
    /// - `None` otherwise
    pub fn progress_estimate(&self) -> Option<f64> {
        if let Some(reported) = self.reported_progress {
            return Some(reported);
        }
        if let Some(prev) = self.previous_runtime {
            let prev_secs = prev.as_secs_f64();
            if prev_secs > 0.0 {
                return Some(self.elapsed.as_secs_f64() / prev_secs);
            }
        }
        None
    }
}

/// Decision returned by [`check_commitment`] indicating the current
/// commitment status of a running task.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitmentDecision {
    /// Task has not yet met commitment thresholds — may be cancelled.
    NotYet,
    /// Task is committed — should run to completion.
    Committed,
    /// Task should never be committed (e.g., AlwaysCancelWhenStale override
    /// or OnlyRunOnFinalInputs with intermediate inputs).
    NeverCommit,
}

/// Check whether a running task should be committed based on policy, override,
/// progress, and input finality.
///
/// This is a pure function — takes all inputs explicitly and returns a decision.
/// The [`CommitmentTracker`] calls this internally.
///
/// Logic:
/// 1. `AlwaysCancelWhenStale` override → `NeverCommit`
/// 2. `OnlyRunOnFinalInputs` with intermediate inputs → `NeverCommit`
/// 3. Elapsed > `always_commit_after` → `Committed`
/// 4. Estimated progress > `commit_when_proportion_done` → `Committed`
/// 5. Otherwise → `NotYet`
pub fn check_commitment(
    policy: &CommitmentPolicy,
    override_: NodeCommitmentOverride,
    progress: &TaskProgress,
    has_intermediate_inputs: bool,
) -> CommitmentDecision {
    // 1. AlwaysCancelWhenStale always returns NeverCommit
    if override_ == NodeCommitmentOverride::AlwaysCancelWhenStale {
        return CommitmentDecision::NeverCommit;
    }

    // 2. OnlyRunOnFinalInputs with intermediate inputs → NeverCommit
    if override_ == NodeCommitmentOverride::OnlyRunOnFinalInputs && has_intermediate_inputs {
        return CommitmentDecision::NeverCommit;
    }

    // 3. Time threshold: unconditionally commit after elapsed time
    if progress.elapsed >= policy.always_commit_after {
        return CommitmentDecision::Committed;
    }

    // 4. Progress threshold: commit when estimated progress exceeds threshold
    if let Some(estimate) = progress.progress_estimate()
        && estimate >= policy.commit_when_proportion_done
    {
        return CommitmentDecision::Committed;
    }

    // 5. Below both thresholds
    CommitmentDecision::NotYet
}

/// Transition signal returned by [`CommitmentTracker::update_status`].
///
/// Enables callers to emit journal events (e.g., `commitment_acquired`)
/// when a task transitions to committed status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitmentTransition {
    /// No change in commitment status.
    Unchanged,
    /// Task just transitioned from NotYet to Committed.
    BecameCommitted,
}

/// Per-node commitment state tracked by [`CommitmentTracker`].
struct CommitmentState {
    override_: NodeCommitmentOverride,
    decision: CommitmentDecision,
}

/// Tracks per-node commitment status for in-flight tasks.
///
/// Wraps the pure [`check_commitment`] function with stateful tracking:
/// register tasks when they start, update status periodically, and query
/// whether a task should continue or be cancelled.
pub struct CommitmentTracker {
    policy: CommitmentPolicy,
    states: HashMap<NodeId, CommitmentState>,
}

impl CommitmentTracker {
    /// Create a new tracker with the given project-level commitment policy.
    pub fn new(policy: CommitmentPolicy) -> Self {
        Self {
            policy,
            states: HashMap::new(),
        }
    }

    /// Register a new in-flight task with its per-node override.
    pub fn register_task(&mut self, node_id: NodeId, override_: NodeCommitmentOverride) {
        self.states.insert(
            node_id,
            CommitmentState {
                override_,
                decision: CommitmentDecision::NotYet,
            },
        );
    }

    /// Update the commitment status for a node based on current progress.
    ///
    /// Calls [`check_commitment`] and stores the result. Only transitions
    /// from `NotYet` to `Committed` or `NeverCommit` — once committed,
    /// the decision is sticky.
    ///
    /// Returns `Some(CommitmentTransition)` indicating whether a transition
    /// occurred, or `None` if the node is not registered.
    pub fn update_status(
        &mut self,
        node_id: &NodeId,
        progress: &TaskProgress,
        has_intermediate_inputs: bool,
    ) -> Option<CommitmentTransition> {
        let state = self.states.get_mut(node_id)?;
        if state.decision == CommitmentDecision::NotYet {
            let new_decision = check_commitment(
                &self.policy,
                state.override_,
                progress,
                has_intermediate_inputs,
            );
            if new_decision == CommitmentDecision::Committed {
                state.decision = new_decision;
                return Some(CommitmentTransition::BecameCommitted);
            }
            state.decision = new_decision;
        }
        Some(CommitmentTransition::Unchanged)
    }

    /// Check if a node is currently committed.
    pub fn is_committed(&self, node_id: &NodeId) -> bool {
        self.states
            .get(node_id)
            .is_some_and(|s| s.decision == CommitmentDecision::Committed)
    }

    /// Determine whether a running task should continue.
    ///
    /// - If not in dirty cone: always continue (no reason to cancel)
    /// - If in dirty cone and committed: continue (run to completion)
    /// - If in dirty cone and not committed: cancel (stale)
    pub fn should_continue(&self, node_id: &NodeId, in_dirty_cone: bool) -> bool {
        if !in_dirty_cone {
            return true;
        }
        self.is_committed(node_id)
    }

    /// Remove a task from the tracker (on completion or cancellation).
    pub fn remove_task(&mut self, node_id: &NodeId) {
        self.states.remove(node_id);
    }

    /// Return the number of tracked tasks (for test verification of cleanup).
    pub fn task_count(&self) -> usize {
        self.states.len()
    }
}

impl Default for CommitmentPolicy {
    fn default() -> Self {
        Self {
            always_commit_after: Duration::from_secs(120),
            commit_when_proportion_done: 0.5,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commitment_policy_default_thresholds() {
        let policy = CommitmentPolicy::default();
        assert_eq!(policy.always_commit_after, Duration::from_secs(120));
        assert_eq!(policy.commit_when_proportion_done, 0.5);
    }

    #[test]
    fn commitment_policy_custom_construction() {
        let policy = CommitmentPolicy {
            always_commit_after: Duration::from_secs(60),
            commit_when_proportion_done: 0.8,
        };
        assert_eq!(policy.always_commit_after, Duration::from_secs(60));
        assert_eq!(policy.commit_when_proportion_done, 0.8);
    }

    #[test]
    fn commitment_policy_field_access() {
        let policy = CommitmentPolicy {
            always_commit_after: Duration::from_secs(300),
            commit_when_proportion_done: 0.1,
        };
        assert_eq!(policy.always_commit_after.as_secs(), 300);
        assert!(policy.commit_when_proportion_done < 0.5);
    }

    #[test]
    fn commitment_policy_clone_and_debug() {
        let policy = CommitmentPolicy::default();
        let cloned = policy.clone();
        assert_eq!(policy, cloned);
        let _ = format!("{:?}", policy);
    }

    // --- NodeCommitmentOverride tests ---

    #[test]
    fn node_commitment_override_default_is_commit_if_slow() {
        let override_ = NodeCommitmentOverride::default();
        assert_eq!(override_, NodeCommitmentOverride::CommitIfSlow);
    }

    #[test]
    fn node_commitment_override_variants() {
        let a = NodeCommitmentOverride::CommitIfSlow;
        let b = NodeCommitmentOverride::AlwaysCancelWhenStale;
        let c = NodeCommitmentOverride::OnlyRunOnFinalInputs;
        assert_ne!(a, b);
        assert_ne!(b, c);
        assert_ne!(a, c);
    }

    #[test]
    fn node_commitment_override_clone_and_debug() {
        let override_ = NodeCommitmentOverride::AlwaysCancelWhenStale;
        let cloned = override_;
        assert_eq!(override_, cloned);
        let _ = format!("{:?}", override_);
    }

    // --- TaskProgress tests ---

    #[test]
    fn task_progress_with_reported_progress() {
        let progress = TaskProgress {
            elapsed: Duration::from_secs(30),
            reported_progress: Some(0.7),
            previous_runtime: Some(Duration::from_secs(100)),
        };
        // reported_progress takes precedence over elapsed/previous_runtime
        assert_eq!(progress.progress_estimate(), Some(0.7));
    }

    #[test]
    fn task_progress_fallback_to_elapsed_over_previous() {
        let progress = TaskProgress {
            elapsed: Duration::from_secs(60),
            reported_progress: None,
            previous_runtime: Some(Duration::from_secs(120)),
        };
        // Fallback: elapsed/previous_runtime = 60/120 = 0.5
        assert_eq!(progress.progress_estimate(), Some(0.5));
    }

    #[test]
    fn task_progress_no_estimate_available() {
        let progress = TaskProgress {
            elapsed: Duration::from_secs(60),
            reported_progress: None,
            previous_runtime: None,
        };
        // No reported progress and no previous runtime → None
        assert_eq!(progress.progress_estimate(), None);
    }

    #[test]
    fn task_progress_elapsed_exceeds_previous_runtime() {
        let progress = TaskProgress {
            elapsed: Duration::from_secs(200),
            reported_progress: None,
            previous_runtime: Some(Duration::from_secs(100)),
        };
        // elapsed/previous_runtime = 2.0 (can exceed 1.0)
        assert_eq!(progress.progress_estimate(), Some(2.0));
    }

    #[test]
    fn task_progress_zero_previous_runtime() {
        let progress = TaskProgress {
            elapsed: Duration::from_secs(10),
            reported_progress: None,
            previous_runtime: Some(Duration::ZERO),
        };
        // Division by zero case — should return None or infinity; we return None
        assert_eq!(progress.progress_estimate(), None);
    }

    // --- CommitmentDecision + check_commitment tests ---

    #[test]
    fn always_cancel_override_returns_never_commit() {
        let policy = CommitmentPolicy::default();
        let progress = TaskProgress {
            elapsed: Duration::from_secs(999),
            reported_progress: Some(0.99),
            previous_runtime: None,
        };
        // AlwaysCancelWhenStale should always return NeverCommit
        let decision = check_commitment(
            &policy,
            NodeCommitmentOverride::AlwaysCancelWhenStale,
            &progress,
            false,
        );
        assert_eq!(decision, CommitmentDecision::NeverCommit);
    }

    #[test]
    fn elapsed_exceeds_always_commit_after_returns_committed() {
        let policy = CommitmentPolicy {
            always_commit_after: Duration::from_secs(120),
            commit_when_proportion_done: 0.5,
        };
        let progress = TaskProgress {
            elapsed: Duration::from_secs(121),
            reported_progress: None,
            previous_runtime: None,
        };
        let decision = check_commitment(
            &policy,
            NodeCommitmentOverride::CommitIfSlow,
            &progress,
            false,
        );
        assert_eq!(decision, CommitmentDecision::Committed);
    }

    #[test]
    fn estimated_progress_exceeds_threshold_returns_committed() {
        let policy = CommitmentPolicy {
            always_commit_after: Duration::from_secs(120),
            commit_when_proportion_done: 0.5,
        };
        let progress = TaskProgress {
            elapsed: Duration::from_secs(10),
            reported_progress: Some(0.6),
            previous_runtime: None,
        };
        let decision = check_commitment(
            &policy,
            NodeCommitmentOverride::CommitIfSlow,
            &progress,
            false,
        );
        assert_eq!(decision, CommitmentDecision::Committed);
    }

    #[test]
    fn below_both_thresholds_returns_not_yet() {
        let policy = CommitmentPolicy {
            always_commit_after: Duration::from_secs(120),
            commit_when_proportion_done: 0.5,
        };
        let progress = TaskProgress {
            elapsed: Duration::from_secs(10),
            reported_progress: Some(0.3),
            previous_runtime: None,
        };
        let decision = check_commitment(
            &policy,
            NodeCommitmentOverride::CommitIfSlow,
            &progress,
            false,
        );
        assert_eq!(decision, CommitmentDecision::NotYet);
    }

    #[test]
    fn only_run_on_final_with_intermediate_inputs_returns_never_commit() {
        let policy = CommitmentPolicy::default();
        let progress = TaskProgress {
            elapsed: Duration::from_secs(999),
            reported_progress: Some(0.99),
            previous_runtime: None,
        };
        // OnlyRunOnFinalInputs with intermediate inputs → NeverCommit
        let decision = check_commitment(
            &policy,
            NodeCommitmentOverride::OnlyRunOnFinalInputs,
            &progress,
            true, // has_intermediate_inputs = true
        );
        assert_eq!(decision, CommitmentDecision::NeverCommit);
    }

    #[test]
    fn only_run_on_final_with_final_inputs_uses_dual_threshold() {
        let policy = CommitmentPolicy {
            always_commit_after: Duration::from_secs(120),
            commit_when_proportion_done: 0.5,
        };
        let progress = TaskProgress {
            elapsed: Duration::from_secs(121),
            reported_progress: None,
            previous_runtime: None,
        };
        // OnlyRunOnFinalInputs with final inputs → falls through to dual-threshold
        let decision = check_commitment(
            &policy,
            NodeCommitmentOverride::OnlyRunOnFinalInputs,
            &progress,
            false, // has_intermediate_inputs = false
        );
        assert_eq!(decision, CommitmentDecision::Committed);
    }

    #[test]
    fn no_progress_estimate_and_below_time_threshold_returns_not_yet() {
        let policy = CommitmentPolicy::default();
        let progress = TaskProgress {
            elapsed: Duration::from_secs(10),
            reported_progress: None,
            previous_runtime: None,
        };
        let decision = check_commitment(
            &policy,
            NodeCommitmentOverride::CommitIfSlow,
            &progress,
            false,
        );
        assert_eq!(decision, CommitmentDecision::NotYet);
    }

    // --- CommitmentTracker tests ---

    fn make_node(name: &str) -> NodeId {
        NodeId::Value(reify_core::ValueCellId::new("T", name))
    }

    fn make_constraint_node(entity: &str, idx: u32) -> NodeId {
        NodeId::Constraint(reify_core::ConstraintNodeId::new(entity, idx))
    }

    fn make_realization_node(entity: &str, idx: u32) -> NodeId {
        NodeId::Realization(reify_core::RealizationNodeId::new(entity, idx))
    }

    fn make_resolution_node(entity: &str, idx: u32) -> NodeId {
        NodeId::Resolution(reify_core::ResolutionNodeId::new(entity, idx))
    }

    fn make_compute_node(entity: &str, idx: u32) -> NodeId {
        NodeId::Compute(reify_core::ComputeNodeId::new(entity, idx))
    }

    // --- NodePolicyOverrides tests ---

    #[test]
    fn node_policy_overrides_default_resolves_to_commit_if_slow() {
        let overrides_new = NodePolicyOverrides::new();
        let overrides_default = NodePolicyOverrides::default();
        let node = make_node("x");

        // Both constructors resolve to the default CommitIfSlow
        assert_eq!(
            overrides_new.resolve(&node),
            NodeCommitmentOverride::CommitIfSlow
        );
        assert_eq!(
            overrides_default.resolve(&node),
            NodeCommitmentOverride::CommitIfSlow
        );
        // It should also equal NodeCommitmentOverride::default()
        assert_eq!(
            overrides_new.resolve(&node),
            NodeCommitmentOverride::default()
        );
    }

    #[test]
    fn precedence_instance_wins_over_type_wins_over_default() {
        let mut overrides = NodePolicyOverrides::new();
        let n = make_node("n");
        let m = make_node("m"); // same kind (Value), different node

        // (a) Set a type override for Value kind and an instance override for n
        overrides.set_type(
            NodeKind::Value,
            NodeCommitmentOverride::OnlyRunOnFinalInputs,
        );
        overrides.set_instance(n.clone(), NodeCommitmentOverride::AlwaysCancelWhenStale);

        // instance override wins over type override for n
        assert_eq!(
            overrides.resolve(&n),
            NodeCommitmentOverride::AlwaysCancelWhenStale,
            "instance override must win over type override"
        );
        // m has no instance override → type override wins over default
        assert_eq!(
            overrides.resolve(&m),
            NodeCommitmentOverride::OnlyRunOnFinalInputs,
            "type override must win over default when no instance is set"
        );

        // (b) Constraint node with no type or instance override → default wins
        let c = make_constraint_node("E", 0);
        assert_eq!(
            overrides.resolve(&c),
            NodeCommitmentOverride::CommitIfSlow,
            "default must win when no instance and no matching type override"
        );

        // (c) set_type last-write-wins: overwriting a type override takes effect
        overrides.set_type(NodeKind::Value, NodeCommitmentOverride::CommitIfSlow);
        assert_eq!(
            overrides.resolve(&m),
            NodeCommitmentOverride::CommitIfSlow,
            "last-write wins: second set_type call must overwrite the first"
        );

        // (d) instance override is unaffected when set_type is updated for the same kind
        // n still has instance=AlwaysCancelWhenStale; type for Value was just changed to CommitIfSlow
        assert_eq!(
            overrides.resolve(&n),
            NodeCommitmentOverride::AlwaysCancelWhenStale,
            "instance override must persist after a subsequent set_type call for the same kind"
        );
    }

    #[test]
    fn set_type_override_resolves_to_type_value_and_isolates_other_kinds() {
        let mut overrides = NodePolicyOverrides::new();
        let value_node = make_node("v");
        let constraint_node = make_constraint_node("E", 0);

        overrides.set_type(
            NodeKind::Value,
            NodeCommitmentOverride::OnlyRunOnFinalInputs,
        );

        // Value node should pick up the type override
        assert_eq!(
            overrides.resolve(&value_node),
            NodeCommitmentOverride::OnlyRunOnFinalInputs
        );
        // Constraint node should still be the default (kind isolation)
        assert_eq!(
            overrides.resolve(&constraint_node),
            NodeCommitmentOverride::CommitIfSlow
        );

        // Set a different override for Constraint kind
        overrides.set_type(
            NodeKind::Constraint,
            NodeCommitmentOverride::AlwaysCancelWhenStale,
        );
        assert_eq!(
            overrides.resolve(&constraint_node),
            NodeCommitmentOverride::AlwaysCancelWhenStale
        );
        // Value node should still have its own type override unchanged
        assert_eq!(
            overrides.resolve(&value_node),
            NodeCommitmentOverride::OnlyRunOnFinalInputs
        );
    }

    #[test]
    fn set_instance_override_resolves_to_instance_value_and_isolates_other_nodes() {
        let mut overrides = NodePolicyOverrides::new();
        let node_a = make_node("a");
        let node_b = make_node("b");

        overrides.set_instance(
            node_a.clone(),
            NodeCommitmentOverride::AlwaysCancelWhenStale,
        );

        // node_a should return the set value
        assert_eq!(
            overrides.resolve(&node_a),
            NodeCommitmentOverride::AlwaysCancelWhenStale
        );
        // node_b (unset) should still return the default
        assert_eq!(
            overrides.resolve(&node_b),
            NodeCommitmentOverride::CommitIfSlow
        );

        // Re-setting node_a: last-write semantics
        overrides.set_instance(node_a.clone(), NodeCommitmentOverride::OnlyRunOnFinalInputs);
        assert_eq!(
            overrides.resolve(&node_a),
            NodeCommitmentOverride::OnlyRunOnFinalInputs
        );
    }

    // --- NodeKind tests ---

    #[test]
    fn nodekind_of_each_variant_maps_correctly() {
        let value_node = make_node("v");
        let constraint_node = make_constraint_node("E", 0);
        let realization_node = make_realization_node("E", 0);
        let resolution_node = make_resolution_node("E", 0);
        let compute_node = make_compute_node("E", 0);

        assert_eq!(NodeKind::from(&value_node), NodeKind::Value);
        assert_eq!(NodeKind::from(&constraint_node), NodeKind::Constraint);
        assert_eq!(NodeKind::from(&realization_node), NodeKind::Realization);
        assert_eq!(NodeKind::from(&resolution_node), NodeKind::Resolution);
        assert_eq!(NodeKind::from(&compute_node), NodeKind::Compute);
    }

    #[test]
    fn node_kind_reexport_identity() {
        // Asserts that crate::commitment::NodeKind IS reify_types::NodeKind
        // (the same type, not a wrapper). After step-6, this compiles because
        // commitment re-exports via `pub use reify_types::NodeKind`.
        let _: reify_ir::NodeKind = crate::commitment::NodeKind::Value;
    }

    #[test]
    fn tracker_new_has_no_committed_nodes() {
        let tracker = CommitmentTracker::new(CommitmentPolicy::default());
        let node = make_node("x");
        assert!(!tracker.is_committed(&node));
    }

    #[test]
    fn tracker_register_and_check_not_yet() {
        let mut tracker = CommitmentTracker::new(CommitmentPolicy::default());
        let node = make_node("x");
        tracker.register_task(node.clone(), NodeCommitmentOverride::CommitIfSlow);
        assert!(!tracker.is_committed(&node));
    }

    #[test]
    fn tracker_update_transitions_to_committed() {
        let policy = CommitmentPolicy {
            always_commit_after: Duration::from_secs(10),
            commit_when_proportion_done: 0.5,
        };
        let mut tracker = CommitmentTracker::new(policy);
        let node = make_node("x");
        tracker.register_task(node.clone(), NodeCommitmentOverride::CommitIfSlow);

        let progress = TaskProgress {
            elapsed: Duration::from_secs(11),
            reported_progress: None,
            previous_runtime: None,
        };
        tracker.update_status(&node, &progress, false);
        assert!(tracker.is_committed(&node));
    }

    #[test]
    fn tracker_committed_in_dirty_cone_should_continue() {
        let policy = CommitmentPolicy {
            always_commit_after: Duration::from_secs(10),
            commit_when_proportion_done: 0.5,
        };
        let mut tracker = CommitmentTracker::new(policy);
        let node = make_node("x");
        tracker.register_task(node.clone(), NodeCommitmentOverride::CommitIfSlow);

        let progress = TaskProgress {
            elapsed: Duration::from_secs(11),
            reported_progress: None,
            previous_runtime: None,
        };
        tracker.update_status(&node, &progress, false);
        assert!(tracker.should_continue(&node, true)); // in dirty cone, committed
    }

    #[test]
    fn tracker_uncommitted_in_dirty_cone_should_not_continue() {
        let policy = CommitmentPolicy::default();
        let mut tracker = CommitmentTracker::new(policy);
        let node = make_node("x");
        tracker.register_task(node.clone(), NodeCommitmentOverride::CommitIfSlow);
        // No update_status called, so still NotYet
        assert!(!tracker.should_continue(&node, true)); // in dirty cone, not committed
    }

    #[test]
    fn tracker_not_in_dirty_cone_should_always_continue() {
        let policy = CommitmentPolicy::default();
        let mut tracker = CommitmentTracker::new(policy);
        let node = make_node("x");
        tracker.register_task(node.clone(), NodeCommitmentOverride::CommitIfSlow);
        // Not in dirty cone → should always continue
        assert!(tracker.should_continue(&node, false));
    }

    #[test]
    fn tracker_remove_task() {
        let mut tracker = CommitmentTracker::new(CommitmentPolicy::default());
        let node = make_node("x");
        tracker.register_task(node.clone(), NodeCommitmentOverride::CommitIfSlow);
        tracker.remove_task(&node);
        assert!(!tracker.is_committed(&node));
    }

    // --- CommitmentTransition tests ---

    #[test]
    fn update_status_returns_unchanged_when_still_not_yet() {
        let mut tracker = CommitmentTracker::new(CommitmentPolicy::default());
        let node = make_node("x");
        tracker.register_task(node.clone(), NodeCommitmentOverride::CommitIfSlow);

        let progress = TaskProgress {
            elapsed: Duration::from_secs(1),
            reported_progress: None,
            previous_runtime: None,
        };
        let transition = tracker.update_status(&node, &progress, false);
        assert_eq!(transition, Some(CommitmentTransition::Unchanged));
    }

    #[test]
    fn update_status_returns_became_committed_on_transition() {
        let policy = CommitmentPolicy {
            always_commit_after: Duration::from_secs(10),
            commit_when_proportion_done: 0.5,
        };
        let mut tracker = CommitmentTracker::new(policy);
        let node = make_node("x");
        tracker.register_task(node.clone(), NodeCommitmentOverride::CommitIfSlow);

        let progress = TaskProgress {
            elapsed: Duration::from_secs(11),
            reported_progress: None,
            previous_runtime: None,
        };
        let transition = tracker.update_status(&node, &progress, false);
        assert_eq!(transition, Some(CommitmentTransition::BecameCommitted));
    }

    #[test]
    fn update_status_returns_unchanged_when_already_committed() {
        let policy = CommitmentPolicy {
            always_commit_after: Duration::from_secs(10),
            commit_when_proportion_done: 0.5,
        };
        let mut tracker = CommitmentTracker::new(policy);
        let node = make_node("x");
        tracker.register_task(node.clone(), NodeCommitmentOverride::CommitIfSlow);

        let progress = TaskProgress {
            elapsed: Duration::from_secs(11),
            reported_progress: None,
            previous_runtime: None,
        };
        // First update transitions
        tracker.update_status(&node, &progress, false);
        // Second update should be Unchanged (already committed)
        let transition = tracker.update_status(&node, &progress, false);
        assert_eq!(transition, Some(CommitmentTransition::Unchanged));
    }

    #[test]
    fn update_status_returns_none_for_unknown_node() {
        let mut tracker = CommitmentTracker::new(CommitmentPolicy::default());
        let node = make_node("unknown");
        let progress = TaskProgress {
            elapsed: Duration::from_secs(1),
            reported_progress: None,
            previous_runtime: None,
        };
        let transition = tracker.update_status(&node, &progress, false);
        assert_eq!(transition, None);
    }
}
