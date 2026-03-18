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
