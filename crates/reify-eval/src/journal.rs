//! Event journal for recording evaluation events.
//!
//! Provides an append-only journal dual-indexed by time (BTreeMap<Instant>)
//! and NodeId (HashMap<NodeId>), recording Started, Completed, Cancelled,
//! Failed, CacheHit, and WarmStartUsed events during evaluation.

use std::collections::{BTreeMap, HashMap};
use std::time::Instant;

use reify_types::VersionId;

use crate::cache::{EvalOutcome, NodeId};

/// A single evaluation event recorded in the journal.
#[derive(Clone, Debug)]
pub struct EvalEvent {
    /// Monotonic timestamp when the event occurred.
    pub timestamp: Instant,
    /// The node this event pertains to.
    pub node_id: NodeId,
    /// What kind of event occurred.
    pub kind: EventKind,
    /// The evaluation version when this event was recorded.
    pub version: VersionId,
    /// Optional payload with additional event data.
    pub payload: Option<EventPayload>,
}

/// The kind of evaluation event.
#[derive(Clone, Debug)]
pub enum EventKind {
    /// Evaluation of a node has started.
    Started,
    /// Evaluation of a node completed with the given outcome.
    Completed { outcome: EvalOutcome },
    /// Evaluation of a node was cancelled.
    Cancelled,
    /// Evaluation of a node failed with an error message.
    Failed { error: String },
    /// A cache hit was used instead of re-evaluating.
    CacheHit,
    /// A warm-start state was used for evaluation.
    WarmStartUsed,
    /// A warm-state pool entry was evicted — either because LRU eviction kicked in to
    /// free budget, OR because the same key was overwritten by a subsequent donate call
    /// (the prior entry being the victim whose state was discarded).
    ///
    /// Translation contract: when the engine translator converts a `WarmPoolEvent::Evicted`
    /// into an `EvalEvent`, `EvalEvent.node_id` **must** be the evicted node (the victim
    /// whose state was discarded), not the donating node that triggered the eviction.
    /// This applies equally to LRU-eviction victims and same-key-overwrite victims.
    Evicted { size_bytes: usize },
    /// A warm state was donated to the pool (insertion or topology-removal donation).
    ///
    /// Translation contract: when the engine translator converts a `WarmPoolEvent::Donated`
    /// into an `EvalEvent`, `EvalEvent.node_id` **must** be the donating node (the node
    /// whose state was inserted into the pool).
    Donated { size_bytes: usize },
}

/// Optional payload attached to an evaluation event.
#[derive(Clone, Debug)]
pub enum EventPayload {
    /// Duration of the evaluation.
    Duration(std::time::Duration),
    /// Error details.
    Error(String),
    /// Custom string payload.
    Custom(String),
}

/// Append-only event journal dual-indexed by time and node.
pub struct EventJournal {
    /// All events in insertion order.
    events: Vec<EvalEvent>,
    /// Index from timestamp to event indices (events at the same Instant).
    by_time: BTreeMap<Instant, Vec<usize>>,
    /// Index from NodeId to event indices.
    by_node: HashMap<NodeId, Vec<usize>>,
}

impl EventJournal {
    /// Create a new, empty event journal.
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            by_time: BTreeMap::new(),
            by_node: HashMap::new(),
        }
    }

    /// Record an event in the journal.
    pub fn record(&mut self, event: EvalEvent) {
        let idx = self.events.len();
        self.by_time.entry(event.timestamp).or_default().push(idx);
        self.by_node
            .entry(event.node_id.clone())
            .or_default()
            .push(idx);
        self.events.push(event);
    }

    /// Number of events recorded.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether the journal is empty.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// All events in insertion order.
    pub fn all_events(&self) -> &[EvalEvent] {
        &self.events
    }

    /// Events within a time range, in insertion order.
    pub fn events_in_range(&self, range: impl std::ops::RangeBounds<Instant>) -> Vec<&EvalEvent> {
        let mut indices: Vec<usize> = self
            .by_time
            .range(range)
            .flat_map(|(_, idxs)| idxs.iter().copied())
            .collect();
        indices.sort_unstable();
        indices.dedup();
        indices.iter().map(|&idx| &self.events[idx]).collect()
    }

    /// Events since (inclusive) the given version, in insertion order.
    pub fn events_since(&self, version: VersionId) -> Vec<&EvalEvent> {
        self.events
            .iter()
            .filter(|e| e.version >= version)
            .collect()
    }

    /// Events for a specific node, in insertion order.
    pub fn events_for_node(&self, node_id: &NodeId) -> Vec<&EvalEvent> {
        self.by_node
            .get(node_id)
            .map(|indices| indices.iter().map(|&idx| &self.events[idx]).collect())
            .unwrap_or_default()
    }

    /// Count events whose `EventKind` satisfies `predicate`.
    ///
    /// Generic base for diagnostic counters — prefer the typed convenience wrappers
    /// (`count_donated`, `count_evicted`) for common cases.  As new `EventKind`
    /// variants land (e.g. `CacheHit` stats, `Cancelled` counts) they can be
    /// expressed as one-liners via this helper rather than duplicating the
    /// filter-and-count pattern.
    pub fn count_matching(&self, predicate: impl Fn(&EventKind) -> bool) -> usize {
        self.events.iter().filter(|e| predicate(&e.kind)).count()
    }

    /// Number of `EventKind::Donated` events recorded in this journal.
    pub fn count_donated(&self) -> usize {
        self.count_matching(|k| matches!(k, EventKind::Donated { .. }))
    }

    /// Number of `EventKind::Evicted` events recorded in this journal.
    pub fn count_evicted(&self) -> usize {
        self.count_matching(|k| matches!(k, EventKind::Evicted { .. }))
    }

    /// The most recent event for a specific node, or None if no events exist.
    pub fn latest_for_node(&self, node_id: &NodeId) -> Option<&EvalEvent> {
        self.by_node
            .get(node_id)
            .and_then(|indices| indices.last())
            .map(|&idx| &self.events[idx])
    }
}

impl Default for EventJournal {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use reify_types::{ErrorRef, VersionId};

    use crate::cache::{EvalOutcome, NodeId};
    use crate::journal::*;

    #[test]
    fn event_kind_started() {
        let kind = EventKind::Started;
        let _ = format!("{:?}", kind); // Debug works
        let _ = kind.clone(); // Clone works
    }

    #[test]
    fn event_kind_completed_changed() {
        let kind = EventKind::Completed {
            outcome: EvalOutcome::Changed,
        };
        let _ = format!("{:?}", kind);
        let _ = kind.clone();
    }

    #[test]
    fn event_kind_completed_unchanged() {
        let kind = EventKind::Completed {
            outcome: EvalOutcome::Unchanged,
        };
        let _ = format!("{:?}", kind);
        let _ = kind.clone();
    }

    #[test]
    fn event_kind_cancelled() {
        let kind = EventKind::Cancelled;
        let _ = format!("{:?}", kind);
        let _ = kind.clone();
    }

    #[test]
    fn event_kind_failed() {
        let kind = EventKind::Failed {
            error: ErrorRef::new("test error"),
        };
        let _ = format!("{:?}", kind);
        let _ = kind.clone();
    }

    #[test]
    fn event_kind_failed_carries_error_ref() {
        // The payload is an `ErrorRef` — carrying typed error metadata
        // (including optional `DiagnosticCode`) — not a bare String. Pin
        // that the typed payload survives Clone/Debug and that the message
        // round-trips via `error.message()`.
        let kind = EventKind::Failed {
            error: ErrorRef::new("test error"),
        };
        let cloned = kind.clone();
        let debug = format!("{:?}", kind);
        assert!(
            debug.contains("Failed"),
            "Debug output should mention Failed: {debug}"
        );
        match cloned {
            EventKind::Failed { error } => {
                assert_eq!(error.message(), "test error");
            }
            other => panic!("expected EventKind::Failed, got {:?}", other),
        }
    }

    #[test]
    fn event_kind_cache_hit() {
        let kind = EventKind::CacheHit;
        let _ = format!("{:?}", kind);
        let _ = kind.clone();
    }

    #[test]
    fn event_kind_warm_start_used() {
        let kind = EventKind::WarmStartUsed;
        let _ = format!("{:?}", kind);
        let _ = kind.clone();
    }

    #[test]
    fn event_payload_variants() {
        let d = EventPayload::Duration(std::time::Duration::from_millis(42));
        let _ = format!("{:?}", d);
        let _ = d.clone();

        let e = EventPayload::Error("something went wrong".to_string());
        let _ = format!("{:?}", e);
        let _ = e.clone();

        let c = EventPayload::Custom("custom data".to_string());
        let _ = format!("{:?}", c);
        let _ = c.clone();
    }

    fn make_event(node_name: &str, kind: EventKind, version: u64) -> EvalEvent {
        EvalEvent {
            timestamp: Instant::now(),
            node_id: NodeId::Value(reify_types::ValueCellId::new("Test", node_name)),
            kind,
            version: VersionId(version),
            payload: None,
        }
    }

    #[test]
    fn journal_new_is_empty() {
        let journal = EventJournal::new();
        assert_eq!(journal.len(), 0);
        assert!(journal.is_empty());
    }

    #[test]
    fn journal_record_increments_len() {
        let mut journal = EventJournal::new();
        journal.record(make_event("x", EventKind::Started, 0));
        assert_eq!(journal.len(), 1);
        assert!(!journal.is_empty());

        journal.record(make_event("y", EventKind::Started, 0));
        assert_eq!(journal.len(), 2);
    }

    #[test]
    fn journal_record_maintains_insertion_order() {
        let mut journal = EventJournal::new();
        journal.record(make_event("a", EventKind::Started, 0));
        journal.record(make_event("b", EventKind::CacheHit, 0));
        journal.record(make_event("c", EventKind::Cancelled, 0));

        let events = journal.all_events();
        assert_eq!(events.len(), 3);
        assert_eq!(
            events[0].node_id,
            NodeId::Value(reify_types::ValueCellId::new("Test", "a"))
        );
        assert_eq!(
            events[1].node_id,
            NodeId::Value(reify_types::ValueCellId::new("Test", "b"))
        );
        assert_eq!(
            events[2].node_id,
            NodeId::Value(reify_types::ValueCellId::new("Test", "c"))
        );
    }

    #[test]
    fn events_for_node_returns_correct_events() {
        let mut journal = EventJournal::new();
        let node_a = NodeId::Value(reify_types::ValueCellId::new("Test", "a"));
        let node_b = NodeId::Value(reify_types::ValueCellId::new("Test", "b"));
        let node_c = NodeId::Value(reify_types::ValueCellId::new("Test", "c"));

        journal.record(make_event("a", EventKind::Started, 0));
        journal.record(make_event("b", EventKind::Started, 0));
        journal.record(make_event(
            "a",
            EventKind::Completed {
                outcome: EvalOutcome::Changed,
            },
            0,
        ));
        journal.record(make_event("c", EventKind::CacheHit, 0));
        journal.record(make_event(
            "b",
            EventKind::Completed {
                outcome: EvalOutcome::Unchanged,
            },
            0,
        ));

        let a_events = journal.events_for_node(&node_a);
        assert_eq!(a_events.len(), 2);
        assert!(matches!(a_events[0].kind, EventKind::Started));
        assert!(matches!(
            a_events[1].kind,
            EventKind::Completed {
                outcome: EvalOutcome::Changed
            }
        ));

        let b_events = journal.events_for_node(&node_b);
        assert_eq!(b_events.len(), 2);

        let c_events = journal.events_for_node(&node_c);
        assert_eq!(c_events.len(), 1);
        assert!(matches!(c_events[0].kind, EventKind::CacheHit));
    }

    #[test]
    fn events_for_node_unknown_returns_empty() {
        let journal = EventJournal::new();
        let unknown = NodeId::Value(reify_types::ValueCellId::new("Test", "unknown"));
        assert!(journal.events_for_node(&unknown).is_empty());
    }

    #[test]
    fn events_for_node_preserves_insertion_order() {
        let mut journal = EventJournal::new();
        // Interleave events for same node
        journal.record(make_event("x", EventKind::Started, 0));
        journal.record(make_event("y", EventKind::Started, 0));
        journal.record(make_event("x", EventKind::CacheHit, 1));
        journal.record(make_event("y", EventKind::CacheHit, 1));
        journal.record(make_event(
            "x",
            EventKind::Completed {
                outcome: EvalOutcome::Changed,
            },
            2,
        ));

        let node_x = NodeId::Value(reify_types::ValueCellId::new("Test", "x"));
        let x_events = journal.events_for_node(&node_x);
        assert_eq!(x_events.len(), 3);
        assert!(matches!(x_events[0].kind, EventKind::Started));
        assert!(matches!(x_events[1].kind, EventKind::CacheHit));
        assert!(matches!(x_events[2].kind, EventKind::Completed { .. }));
    }

    #[test]
    fn events_in_range_returns_subset() {
        let mut journal = EventJournal::new();
        let t1 = Instant::now();
        journal.record(EvalEvent {
            timestamp: t1,
            node_id: NodeId::Value(reify_types::ValueCellId::new("Test", "a")),
            kind: EventKind::Started,
            version: VersionId(0),
            payload: None,
        });
        // Force distinct timestamps
        std::thread::sleep(std::time::Duration::from_millis(1));
        let t2 = Instant::now();
        journal.record(EvalEvent {
            timestamp: t2,
            node_id: NodeId::Value(reify_types::ValueCellId::new("Test", "b")),
            kind: EventKind::Started,
            version: VersionId(0),
            payload: None,
        });
        std::thread::sleep(std::time::Duration::from_millis(1));
        let t3 = Instant::now();
        journal.record(EvalEvent {
            timestamp: t3,
            node_id: NodeId::Value(reify_types::ValueCellId::new("Test", "c")),
            kind: EventKind::Started,
            version: VersionId(0),
            payload: None,
        });

        // Range t1..t3 should include t1 and t2 (exclusive end)
        let events = journal.events_in_range(t1..t3);
        assert_eq!(events.len(), 2);
        assert_eq!(
            events[0].node_id,
            NodeId::Value(reify_types::ValueCellId::new("Test", "a"))
        );
        assert_eq!(
            events[1].node_id,
            NodeId::Value(reify_types::ValueCellId::new("Test", "b"))
        );
    }

    #[test]
    fn events_in_range_from() {
        let mut journal = EventJournal::new();
        let t1 = Instant::now();
        journal.record(EvalEvent {
            timestamp: t1,
            node_id: NodeId::Value(reify_types::ValueCellId::new("Test", "a")),
            kind: EventKind::Started,
            version: VersionId(0),
            payload: None,
        });
        std::thread::sleep(std::time::Duration::from_millis(1));
        let t2 = Instant::now();
        journal.record(EvalEvent {
            timestamp: t2,
            node_id: NodeId::Value(reify_types::ValueCellId::new("Test", "b")),
            kind: EventKind::Started,
            version: VersionId(0),
            payload: None,
        });

        // t2.. should include only event at t2
        let events = journal.events_in_range(t2..);
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].node_id,
            NodeId::Value(reify_types::ValueCellId::new("Test", "b"))
        );
    }

    #[test]
    fn events_in_range_full() {
        let mut journal = EventJournal::new();
        journal.record(make_event("a", EventKind::Started, 0));
        journal.record(make_event("b", EventKind::Started, 0));

        let events = journal.events_in_range(..);
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn events_in_range_empty() {
        let mut journal = EventJournal::new();
        journal.record(make_event("a", EventKind::Started, 0));
        std::thread::sleep(std::time::Duration::from_millis(1));
        let future = Instant::now();
        std::thread::sleep(std::time::Duration::from_millis(1));
        let more_future = Instant::now();

        let events = journal.events_in_range(future..more_future);
        assert!(events.is_empty());
    }

    #[test]
    fn events_since_version_filters_correctly() {
        let mut journal = EventJournal::new();
        journal.record(make_event("a", EventKind::Started, 0));
        journal.record(make_event("b", EventKind::Started, 0));
        journal.record(make_event("c", EventKind::Started, 1));
        journal.record(make_event("d", EventKind::Started, 2));

        let events = journal.events_since(VersionId(1));
        assert_eq!(events.len(), 2);
        assert_eq!(
            events[0].node_id,
            NodeId::Value(reify_types::ValueCellId::new("Test", "c"))
        );
        assert_eq!(
            events[1].node_id,
            NodeId::Value(reify_types::ValueCellId::new("Test", "d"))
        );
    }

    #[test]
    fn events_since_exact_boundary() {
        let mut journal = EventJournal::new();
        journal.record(make_event("a", EventKind::Started, 5));
        journal.record(make_event("b", EventKind::Started, 5));

        let events = journal.events_since(VersionId(5));
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn events_since_higher_than_all_returns_empty() {
        let mut journal = EventJournal::new();
        journal.record(make_event("a", EventKind::Started, 0));
        journal.record(make_event("b", EventKind::Started, 1));

        let events = journal.events_since(VersionId(99));
        assert!(events.is_empty());
    }

    #[test]
    fn latest_for_node_returns_last_event() {
        let mut journal = EventJournal::new();
        journal.record(make_event("x", EventKind::Started, 0));
        journal.record(make_event(
            "x",
            EventKind::Completed {
                outcome: EvalOutcome::Changed,
            },
            0,
        ));
        journal.record(make_event("x", EventKind::CacheHit, 1));

        let node_x = NodeId::Value(reify_types::ValueCellId::new("Test", "x"));
        let latest = journal.latest_for_node(&node_x);
        assert!(latest.is_some());
        assert!(matches!(latest.unwrap().kind, EventKind::CacheHit));
    }

    #[test]
    fn latest_for_node_unknown_returns_none() {
        let journal = EventJournal::new();
        let unknown = NodeId::Value(reify_types::ValueCellId::new("Test", "unknown"));
        assert!(journal.latest_for_node(&unknown).is_none());
    }

    #[test]
    fn latest_for_node_interleaved() {
        let mut journal = EventJournal::new();
        journal.record(make_event("a", EventKind::Started, 0));
        journal.record(make_event("b", EventKind::Started, 0));
        journal.record(make_event(
            "a",
            EventKind::Completed {
                outcome: EvalOutcome::Unchanged,
            },
            0,
        ));
        journal.record(make_event(
            "b",
            EventKind::Completed {
                outcome: EvalOutcome::Changed,
            },
            0,
        ));

        let node_a = NodeId::Value(reify_types::ValueCellId::new("Test", "a"));
        let latest_a = journal.latest_for_node(&node_a).unwrap();
        assert!(matches!(
            latest_a.kind,
            EventKind::Completed {
                outcome: EvalOutcome::Unchanged
            }
        ));

        let node_b = NodeId::Value(reify_types::ValueCellId::new("Test", "b"));
        let latest_b = journal.latest_for_node(&node_b).unwrap();
        assert!(matches!(
            latest_b.kind,
            EventKind::Completed {
                outcome: EvalOutcome::Changed
            }
        ));
    }

    #[test]
    fn count_donated_returns_zero_for_empty_journal() {
        let journal = EventJournal::new();
        assert_eq!(journal.count_donated(), 0);
    }

    #[test]
    fn count_evicted_returns_zero_for_empty_journal() {
        let journal = EventJournal::new();
        assert_eq!(journal.count_evicted(), 0);
    }

    #[test]
    fn count_donated_counts_only_donated_events() {
        let mut journal = EventJournal::new();
        // Mixed sequence: Started, Donated, CacheHit, Donated, Evicted, Completed
        journal.record(make_event("a", EventKind::Started, 0));
        journal.record(make_event("b", EventKind::Donated { size_bytes: 100 }, 0));
        journal.record(make_event("c", EventKind::CacheHit, 0));
        journal.record(make_event("d", EventKind::Donated { size_bytes: 200 }, 0));
        journal.record(make_event("e", EventKind::Evicted { size_bytes: 100 }, 0));
        journal.record(make_event(
            "f",
            EventKind::Completed {
                outcome: EvalOutcome::Changed,
            },
            0,
        ));
        assert_eq!(journal.count_donated(), 2);
        assert_eq!(journal.count_evicted(), 1);
    }

    #[test]
    fn count_helpers_ignore_other_kinds() {
        let mut journal = EventJournal::new();
        journal.record(make_event("a", EventKind::Started, 0));
        journal.record(make_event("b", EventKind::CacheHit, 0));
        journal.record(make_event("c", EventKind::WarmStartUsed, 0));
        journal.record(make_event("d", EventKind::Cancelled, 0));
        journal.record(make_event(
            "e",
            EventKind::Failed {
                error: ErrorRef::new("oops"),
            },
            0,
        ));
        assert_eq!(journal.count_donated(), 0);
        assert_eq!(journal.count_evicted(), 0);
    }

    #[test]
    fn journal_records_evicted_and_donated_events() {
        let mut journal = EventJournal::new();
        let node_a = NodeId::Value(reify_types::ValueCellId::new("Test", "a"));
        let node_b = NodeId::Value(reify_types::ValueCellId::new("Test", "b"));

        journal.record(make_event("a", EventKind::Evicted { size_bytes: 512 }, 0));
        journal.record(make_event("b", EventKind::Donated { size_bytes: 4096 }, 1));

        let a_events = journal.events_for_node(&node_a);
        assert_eq!(a_events.len(), 1);
        assert!(matches!(
            a_events[0].kind,
            EventKind::Evicted { size_bytes: 512 }
        ));

        let b_events = journal.events_for_node(&node_b);
        assert_eq!(b_events.len(), 1);
        assert!(matches!(
            b_events[0].kind,
            EventKind::Donated { size_bytes: 4096 }
        ));
    }

    #[test]
    fn eval_event_construction() {
        let event = EvalEvent {
            timestamp: Instant::now(),
            node_id: NodeId::Value(reify_types::ValueCellId::new("Test", "x")),
            kind: EventKind::Started,
            version: VersionId(0),
            payload: None,
        };
        let _ = format!("{:?}", event);
        let _ = event.clone();

        // With payload
        let event2 = EvalEvent {
            timestamp: Instant::now(),
            node_id: NodeId::Value(reify_types::ValueCellId::new("Test", "y")),
            kind: EventKind::Completed {
                outcome: EvalOutcome::Changed,
            },
            version: VersionId(1),
            payload: Some(EventPayload::Duration(std::time::Duration::from_nanos(100))),
        };
        let _ = format!("{:?}", event2);
        let _ = event2.clone();
    }
}
