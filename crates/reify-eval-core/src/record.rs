//! What a body or a lookup read: the one read request it has outstanding,
//! each ref settled at once or awaiting a node, assembled in order into
//! its answers, each outcome with its content.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::content::Content;
use crate::index::Index;
use crate::outcome::{Erased, Outcome};
use crate::path_table::PathId;
use crate::reference::Ref;

enum Slot {
    Settled(Outcome<Erased>, Content),
    /// The answer at a node of the reader's own snapshot.
    Awaits(PathId),
}

pub(crate) struct Reading {
    refs: Arc<[Ref]>,
    slots: Vec<Slot>,
}

/// A read request's refs, and what each read, in order.
pub(crate) struct Answers {
    refs: Arc<[Ref]>,
    outcomes: Vec<Outcome<Erased>>,
    contents: Vec<Content>,
}

impl Answers {
    pub fn refs(&self) -> &Arc<[Ref]> {
        &self.refs
    }

    pub fn contents(&self) -> &[Content] {
        &self.contents
    }

    pub fn into_outcomes(self) -> Vec<Outcome<Erased>> {
        self.outcomes
    }
}

impl Reading {
    pub fn new(refs: Arc<[Ref]>) -> Self {
        Self {
            refs,
            slots: Vec::new(),
        }
    }

    pub fn settled(&mut self, outcome: Outcome<Erased>, content: Content) {
        self.slots.push(Slot::Settled(outcome, content));
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
                Slot::Settled(..) => None,
            })
            .collect()
    }

    /// The answers, one per ref in order, once every node the reading
    /// awaits is answered in `index`.
    pub fn reply(self, index: &Index) -> Answers {
        let (outcomes, contents) = self
            .slots
            .into_iter()
            .map(|slot| match slot {
                Slot::Settled(outcome, content) => (outcome, content),
                Slot::Awaits(node) => {
                    let entry = index
                        .entry(node)
                        .expect("a reading is answered once every node it awaits is");
                    (entry.outcome.clone(), entry.content)
                }
            })
            .unzip();
        Answers {
            refs: self.refs,
            outcomes,
            contents,
        }
    }
}
