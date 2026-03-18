//! Commitment policy for controlling when speculative evaluation results
//! become committed (run to completion regardless of subsequent edits).
//!
//! Implements a dual-threshold system per §7.3 of the architecture docs:
//! - `always_commit_after`: commits unconditionally after elapsed time
//! - `commit_when_proportion_done`: commits based on estimated progress
//!
//! Per-node overrides allow: 'commit if slow' (default), 'always cancel
//! when stale', and 'only run on final inputs'.

use std::time::Duration;

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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeCommitmentOverride {
    /// Apply the dual-threshold commitment policy (default behavior).
    CommitIfSlow,
    /// Never commit — always cancel when inputs become stale.
    AlwaysCancelWhenStale,
    /// Only run when all inputs are final (skip intermediate evaluations).
    OnlyRunOnFinalInputs,
}

impl Default for NodeCommitmentOverride {
    fn default() -> Self {
        Self::CommitIfSlow
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
    if let Some(estimate) = progress.progress_estimate() {
        if estimate >= policy.commit_when_proportion_done {
            return CommitmentDecision::Committed;
        }
    }

    // 5. Below both thresholds
    CommitmentDecision::NotYet
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
        let cloned = override_.clone();
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
        NodeId::Value(reify_types::ValueCellId::new("T", name))
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
}
