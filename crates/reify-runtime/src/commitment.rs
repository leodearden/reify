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
}
