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

impl Case {
    /// The inputs of each snapshot, by index: the initial inputs, then
    /// each Edit applied to the inputs of the snapshot it starts from.
    pub fn snapshots(&self) -> Vec<Inputs> {
        let mut snapshots = vec![self.inputs.clone()];
        for event in &self.events {
            if let HostEvent::Edit { from, changes } = event {
                let mut next = snapshots[*from].clone();
                for (key, value) in changes {
                    match value {
                        Some(value) => next.insert(key.clone(), value.clone()),
                        None => next.remove(key),
                    };
                }
                snapshots.push(next);
            }
        }
        snapshots
    }

    /// Whether every event names a snapshot made before it.
    pub fn names_made_snapshots(&self) -> bool {
        let mut made = 1;
        for event in &self.events {
            match event {
                HostEvent::Edit { from, .. } if *from < made => made += 1,
                HostEvent::Evaluate { on, .. } | HostEvent::Drive { on, .. } if *on < made => {}
                _ => return false,
            }
        }
        true
    }

    /// Every path an event demands.
    pub fn demanded(&self) -> impl Iterator<Item = &ModelPath> {
        self.events.iter().flat_map(|event| match event {
            HostEvent::Edit { .. } => [].iter(),
            HostEvent::Evaluate { paths, .. } | HostEvent::Drive { paths, .. } => paths.iter(),
        })
    }
}

/// What becomes of a request once its event is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Then {
    Leave,
    End,
}
