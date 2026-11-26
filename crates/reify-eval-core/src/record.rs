//! What a body read: the one read request it has outstanding, each ref
//! settled at once or awaiting a node, assembled in order into its reply.

use std::collections::BTreeSet;

use crate::index::Index;
use crate::outcome::{Erased, Outcome};
use crate::path_table::PathId;

enum Slot {
    Settled(Outcome<Erased>),
    /// The answer at a node of the reader's own snapshot.
    Awaits(PathId),
}

#[derive(Default)]
pub(crate) struct Reading {
    slots: Vec<Slot>,
}

impl Reading {
    pub fn settled(&mut self, outcome: Outcome<Erased>) {
        self.slots.push(Slot::Settled(outcome));
    }

    pub fn awaits(&mut self, node: PathId) {
        self.slots.push(Slot::Awaits(node));
    }

    /// The nodes whose answers the reading still awaits, each once.
    pub fn awaited(&self) -> BTreeSet<PathId> {
        self.slots
            .iter()
            .filter_map(|slot| match slot {
                Slot::Awaits(node) => Some(*node),
                Slot::Settled(_) => None,
            })
            .collect()
    }

    /// The reply, one outcome per ref in order, once every node the reading
    /// awaits is answered in `index`.
    pub fn reply(self, index: &Index) -> Vec<Outcome<Erased>> {
        self.slots
            .into_iter()
            .map(|slot| match slot {
                Slot::Settled(outcome) => outcome,
                Slot::Awaits(node) => index
                    .entry(node)
                    .expect("a reading is answered once every node it awaits is")
                    .outcome
                    .clone(),
            })
            .collect()
    }
}
