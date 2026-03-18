//! Event journal for recording evaluation events.
//!
//! Provides an append-only journal dual-indexed by time (BTreeMap<Instant>)
//! and NodeId (HashMap<NodeId>), recording Started, Completed, Cancelled,
//! Failed, CacheHit, and WarmStartUsed events during evaluation.
