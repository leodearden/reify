//! What a body or a lookup read: the one read request it has outstanding,
//! each item settled at once or awaiting a node, assembled in order into
//! its answers, each outcome with its content; the children the request
//! declares, and those of them that are still to be worked out.

use std::collections::BTreeSet;
use std::mem;
use std::sync::Arc;

use crate::content::Content;
use crate::declaration::Declaration;
use crate::declared_children::{Conflict, DeclaredChildren};
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
    /// The children the request declares.
    declared: DeclaredChildren,
    /// Each child the request declares whose path is not answered yet,
    /// with its declaration, in the order asked.
    to_declare: Vec<(PathId, Arc<Declaration>)>,
}

/// A read request's items, and what each read, in order; and the children
/// it declared.
pub(crate) struct Answers {
    wanted: Arc<[Wanted]>,
    outcomes: Vec<Outcome<Erased>>,
    contents: Vec<Content>,
    declared: DeclaredChildren,
}

impl Answers {
    pub fn wanted(&self) -> &Arc<[Wanted]> {
        &self.wanted
    }

    /// Moves the children the request declared to `children`, those of the
    /// run that made it, which the request was served in agreement with.
    pub fn record_children(&mut self, children: &mut DeclaredChildren) {
        children.extend(mem::take(&mut self.declared));
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
            declared: DeclaredChildren::default(),
            to_declare: Vec::new(),
        }
    }

    pub fn settled(&mut self, outcome: Outcome<Erased>, content: Content) {
        self.slots.push(Slot::Settled(outcome, content));
    }

    pub fn awaits(&mut self, node: PathId) {
        self.slots.push(Slot::Awaits(node));
    }

    /// Notes that the request declares the child `declaration` at `step`,
    /// unless it declared a different one there.
    pub fn declare(&mut self, step: &Step, declaration: &Arc<Declaration>) -> Result<(), Conflict> {
        self.declared.declare(step, declaration)
    }

    /// Awaits the child at `node`, which is to be declared `declaration`.
    pub fn awaits_child(&mut self, node: PathId, declaration: Arc<Declaration>) {
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
            declared: self.declared,
        }
    }
}
