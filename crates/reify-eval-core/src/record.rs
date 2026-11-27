//! What a body or a lookup read: the one read request it has outstanding,
//! each item settled at once or awaiting a node, assembled in order into
//! its answers, each outcome with its content; and the children the
//! request declares that are still to be worked out.

use std::collections::{BTreeMap, BTreeSet};
use std::mem;
use std::sync::Arc;

use crate::content::Content;
use crate::declaration::Declaration;
use crate::index::Index;
use crate::outcome::{Erased, Outcome};
use crate::path_table::PathId;
use crate::step::Step;
use crate::wanted::Wanted;

enum Slot {
    Settled(Outcome<Erased>, Content),
    /// The answer at a node of the reader's own snapshot.
    Awaits(PathId),
}

pub(crate) struct Reading {
    wanted: Arc<[Wanted]>,
    slots: Vec<Slot>,
    /// Each child the request declares whose path is not answered yet,
    /// with its declaration, in the order asked.
    to_declare: Vec<(PathId, Arc<Declaration>)>,
}

/// A read request's items, and what each read, in order.
pub(crate) struct Answers {
    wanted: Arc<[Wanted]>,
    outcomes: Vec<Outcome<Erased>>,
    contents: Vec<Content>,
}

impl Answers {
    pub fn wanted(&self) -> &Arc<[Wanted]> {
        &self.wanted
    }

    /// Adds each child the request declared to `children`, the children
    /// of the run that made it, by step.
    pub fn record_children(&self, children: &mut BTreeMap<Step, Arc<Declaration>>) {
        for wanted in self.wanted.iter() {
            if let Wanted::Child(step, declaration) = wanted {
                children
                    .entry(step.clone())
                    .or_insert_with(|| Arc::clone(declaration));
            }
        }
    }

    pub fn contents(&self) -> &[Content] {
        &self.contents
    }

    pub fn into_outcomes(self) -> Vec<Outcome<Erased>> {
        self.outcomes
    }
}

impl Reading {
    pub fn new(wanted: Arc<[Wanted]>) -> Self {
        Self {
            wanted,
            slots: Vec::new(),
            to_declare: Vec::new(),
        }
    }

    pub fn settled(&mut self, outcome: Outcome<Erased>, content: Content) {
        self.slots.push(Slot::Settled(outcome, content));
    }

    pub fn awaits(&mut self, node: PathId) {
        self.slots.push(Slot::Awaits(node));
    }

    /// Awaits the child at `node`, which is to be declared `declaration`.
    pub fn declares(&mut self, node: PathId, declaration: Arc<Declaration>) {
        self.awaits(node);
        self.to_declare.push((node, declaration));
    }

    /// The children still to be declared, which are taken.
    pub fn take_to_declare(&mut self) -> Vec<(PathId, Arc<Declaration>)> {
        mem::take(&mut self.to_declare)
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

    /// The answers, one per item in order, once every node the reading
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
            wanted: self.wanted,
            outcomes,
            contents,
        }
    }
}
