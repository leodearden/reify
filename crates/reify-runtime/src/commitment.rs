//! Commitment policy for controlling when speculative evaluation results
//! become committed (run to completion regardless of subsequent edits).
//!
//! Implements a dual-threshold system per §7.3 of the architecture docs:
//! - `always_commit_after`: commits unconditionally after elapsed time
//! - `commit_when_proportion_done`: commits based on estimated progress
//!
//! Per-node overrides allow: 'commit if slow' (default), 'always cancel
//! when stale', and 'only run on final inputs'.
