//! Event journal for recording evaluation events.
//!
//! Provides an append-only journal dual-indexed by time (BTreeMap<Instant>)
//! and NodeId (HashMap<NodeId>), recording Started, Completed, Cancelled,
//! Failed, CacheHit, and WarmStartUsed events during evaluation.

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
