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
    /// A request for `paths` on snapshot `on`, run until the core is idle,
    /// given a budget of `budget` paths resolved, or none for no bound.
    Evaluate {
        on: usize,
        paths: Vec<ModelPath>,
        budget: Option<u64>,
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
                let next = edited(&snapshots[*from], changes);
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

/// `inputs` with each `Some` in `changes` set and each `None` removed.
pub fn edited(inputs: &Inputs, changes: &[(Key, Option<Val>)]) -> Inputs {
    let mut next = inputs.clone();
    for (key, value) in changes {
        match value {
            Some(value) => next.insert(key.clone(), value.clone()),
            None => next.remove(key),
        };
    }
    next
}

/// What becomes of a request once its event is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Then {
    Leave,
    End,
}

mod tests {
    use super::super::design::{Inputs, int, named, path};
    use super::{Case, HostEvent, Then};

    fn case(events: Vec<HostEvent>) -> Case {
        Case {
            inputs: Inputs::new(),
            events,
        }
    }

    fn edit(from: usize, x: Option<i64>) -> HostEvent {
        HostEvent::Edit {
            from,
            changes: vec![(named("x"), x.map(int))],
        }
    }

    fn evaluate(on: usize) -> HostEvent {
        HostEvent::Evaluate {
            on,
            paths: vec![path("c", [])],
            budget: None,
            then: Then::End,
        }
    }

    fn drive(on: usize) -> HostEvent {
        HostEvent::Drive {
            on,
            paths: vec![path("c", [])],
            turns: 0,
            then: Then::Leave,
        }
    }

    #[test]
    fn events_on_snapshots_made_before_them_name_made_snapshots() {
        let events = vec![
            evaluate(0),
            edit(0, Some(1)),
            drive(1),
            edit(1, Some(2)),
            evaluate(2),
            edit(0, None),
            evaluate(3),
        ];
        assert!(case(events).names_made_snapshots());
    }

    #[test]
    fn a_request_on_a_snapshot_no_edit_has_made_yet_names_none() {
        assert!(!case(vec![evaluate(1), edit(0, Some(1))]).names_made_snapshots());
        assert!(!case(vec![drive(1), edit(0, Some(1))]).names_made_snapshots());
    }

    #[test]
    fn an_edit_from_a_snapshot_not_made_yet_names_none() {
        assert!(!case(vec![edit(1, Some(1))]).names_made_snapshots());
        assert!(!case(vec![edit(0, Some(1)), edit(2, Some(2))]).names_made_snapshots());
    }

    #[test]
    fn each_edit_applies_to_the_snapshot_it_starts_from() {
        let mut started = case(vec![edit(0, Some(2)), edit(0, None), edit(1, Some(3))]);
        started.inputs.insert(named("x"), int(1));
        let x = |inputs: &Inputs| inputs.get(&named("x")).cloned();
        let snapshots = started.snapshots();
        let seen: Vec<_> = snapshots.iter().map(x).collect();
        assert_eq!(seen, [Some(int(1)), Some(int(2)), None, Some(int(3))]);
    }
}
