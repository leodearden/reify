//! A case: a design's initial inputs and the host events run on it.
//! Snapshot 0 holds the initial inputs, each edit adds the next snapshot,
//! and every event names the snapshot it acts on by its index, so an edit
//! or an evaluation can start from any earlier snapshot.

use super::design::{Inputs, Key, ModelPath, Val};

#[derive(Debug, Clone, PartialEq)]
pub struct Case {
    pub inputs: Inputs,
    pub events: Vec<HostEvent>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HostEvent {
    /// A new snapshot: snapshot `from` with each `Some` set and each
    /// `None` removed.
    Edit {
        from: usize,
        changes: Vec<(Key, Option<Val>)>,
    },
    /// A request for `paths` on snapshot `on`, run until the core is idle.
    Evaluate {
        on: usize,
        paths: Vec<ModelPath>,
        then: Then,
    },
    /// A request for `paths` on snapshot `on`, given `turns` turns.
    Drive {
        on: usize,
        paths: Vec<ModelPath>,
        turns: u8,
        then: Then,
    },
}

/// What becomes of a request once its event is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Then {
    Leave,
    End,
}
