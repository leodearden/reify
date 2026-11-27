//! What a body or a lookup read: the one read request it has outstanding,
//! each item settled at once or awaiting a node, assembled in order into
//! its answers, each outcome with its content and whether it depended on a
//! substitution; the state whose index answers it; the children the
//! request declares, and those of them that are still to be worked out.

use std::collections::BTreeSet;
use std::mem;
use std::sync::Arc;

use crate::content::Content;
use crate::declaration::Declaration;
use crate::declared_children::{Conflict, DeclaredChildren};
use crate::index::{Index, StateId};
use crate::outcome::{Erased, Outcome};
use crate::path_table::PathId;
use crate::step::Step;
use crate::wanted::Wanted;

enum Slot {
    Settled(Outcome<Erased>, Content, bool),
    /// The answer at a node of the reading's state.
    Awaits(PathId),
}

pub(crate) struct Reading {
    wanted: Arc<[Wanted]>,
    slots: Vec<Slot>,
    /// The state whose index answers it.
    state: StateId,
    /// Whether it is a read through a view the reader opened, so answered
    /// in a state other than the reader's own.
    through_view: bool,
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
    depended: Vec<bool>,
    declared: DeclaredChildren,
    through_view: bool,
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

    /// Whether any answer depended on a substitution.
    pub fn depended(&self) -> bool {
        self.depended.contains(&true)
    }

    /// Whether these answer a read through a view the reader opened.
    pub fn through_view(&self) -> bool {
        self.through_view
    }

    pub fn into_outcomes(self) -> Vec<Outcome<Erased>> {
        self.outcomes
    }

    /// Each outcome, with whether it depended on a substitution.
    pub fn into_viewed(self) -> Vec<(Outcome<Erased>, bool)> {
        self.outcomes.into_iter().zip(self.depended).collect()
    }
}

impl Reading {
    /// A read by a reader of `state`, answered in its index.
    pub fn new(wanted: Arc<[Wanted]>, state: StateId) -> Self {
        Self {
            wanted,
            slots: Vec::new(),
            state,
            through_view: false,
            declared: DeclaredChildren::default(),
            to_declare: Vec::new(),
        }
    }

    /// A read through `view`, which the reader opened, answered in its
    /// index.
    pub fn through(wanted: Arc<[Wanted]>, view: StateId) -> Self {
        Self {
            through_view: true,
            ..Self::new(wanted, view)
        }
    }

    pub fn state(&self) -> StateId {
        self.state
    }

    pub fn settled(&mut self, outcome: Outcome<Erased>, content: Content, depended: bool) {
        self.slots.push(Slot::Settled(outcome, content, depended));
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
    /// awaits is answered in `index`, its state's.
    pub fn reply(self, index: &Index) -> Answers {
        let (mut outcomes, mut contents, mut depended) = (Vec::new(), Vec::new(), Vec::new());
        for slot in self.slots {
            let (outcome, content, dependent) = match slot {
                Slot::Settled(outcome, content, dependent) => (outcome, content, dependent),
                Slot::Awaits(node) => {
                    let entry = index
                        .entry(node)
                        .expect("a reading is answered once every node it awaits is");
                    (entry.outcome.clone(), entry.content, entry.depended)
                }
            };
            outcomes.push(outcome);
            contents.push(content);
            depended.push(dependent);
        }
        Answers {
            wanted: self.wanted,
            outcomes,
            contents,
            depended,
            declared: self.declared,
            through_view: self.through_view,
        }
    }
}
