//! Event journal for recording evaluation events.
//!
//! Provides an append-only journal dual-indexed by time (BTreeMap<Instant>)
//! and NodeId (HashMap<NodeId>), recording Started, Completed, Cancelled,
//! Failed, CacheHit, and WarmStartUsed events during evaluation.

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

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use reify_types::VersionId;

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
        let kind = EventKind::Completed { outcome: EvalOutcome::Changed };
        let _ = format!("{:?}", kind);
        let _ = kind.clone();
    }

    #[test]
    fn event_kind_completed_unchanged() {
        let kind = EventKind::Completed { outcome: EvalOutcome::Unchanged };
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
        let kind = EventKind::Failed { error: "test error".to_string() };
        let _ = format!("{:?}", kind);
        let _ = kind.clone();
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
        assert_eq!(events[0].node_id, NodeId::Value(reify_types::ValueCellId::new("Test", "a")));
        assert_eq!(events[1].node_id, NodeId::Value(reify_types::ValueCellId::new("Test", "b")));
        assert_eq!(events[2].node_id, NodeId::Value(reify_types::ValueCellId::new("Test", "c")));
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
            kind: EventKind::Completed { outcome: EvalOutcome::Changed },
            version: VersionId(1),
            payload: Some(EventPayload::Duration(std::time::Duration::from_nanos(100))),
        };
        let _ = format!("{:?}", event2);
        let _ = event2.clone();
    }
}
