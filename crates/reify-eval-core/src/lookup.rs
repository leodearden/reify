//! A lookup (contract 4.2): a node in progress following the store from
//! its recipe's key through the contents of what each stored request
//! reads, until it reaches a result or a key where nothing is filed. A
//! stored request that declares children declares them again, at the
//! lookup's own path (contract 7.1).

use std::mem;
use std::sync::Arc;

use crate::declaration::Declaration;
use crate::declared_children::{ChildSteps, DeclaredChildren};
use crate::key::Key;
use crate::record::Answers;

pub(crate) struct Lookup {
    declaration: Arc<Declaration>,
    /// Whether its declaration depended on a substitution.
    declared_dependent: bool,
    /// Whether the answer it finds depends on a substitution so far: its
    /// declaration did, or a request it moved past read an answer that did.
    depended: bool,
    key: Key,
    /// The children the requests it moved past declared.
    children: DeclaredChildren,
    /// Whether it is queued to be stepped; as a task's `owed_poll`.
    queued: bool,
}

impl Lookup {
    /// A lookup for a node declared `declaration`, whose declaration
    /// `depended` on a substitution or not, starting at `key`.
    pub fn new(declaration: Arc<Declaration>, depended: bool, key: Key) -> Self {
        Self {
            declaration,
            declared_dependent: depended,
            depended,
            key,
            children: DeclaredChildren::default(),
            queued: false,
        }
    }

    pub fn declaration(&self) -> &Arc<Declaration> {
        &self.declaration
    }

    /// The children the requests it moved past declared.
    pub fn children(&self) -> &DeclaredChildren {
        &self.children
    }

    /// Whether the answer it finds depends on a substitution so far.
    pub fn depended(&self) -> bool {
        self.depended
    }

    /// Gives up the lookup, which found nothing to apply, leaving the
    /// declaration for the body that runs in its place, with whether it
    /// depended on a substitution.
    pub fn miss(self) -> (Arc<Declaration>, bool) {
        (self.declaration, self.declared_dependent)
    }

    /// Gives up the lookup, which found a result: the node's declaration,
    /// the steps of the children it declared and whether the result
    /// depended on a substitution.
    pub fn found(self) -> (Arc<Declaration>, ChildSteps, bool) {
        (self.declaration, self.children.into_steps(), self.depended)
    }

    pub fn key(&self) -> Key {
        self.key
    }

    /// Moves on past the request stored at its key, given what it read.
    pub fn advance(&mut self, mut answers: Answers) {
        self.depended |= answers.depended();
        answers.record_children(&mut self.children);
        self.key = self.key.then(answers.wanted(), answers.contents());
    }

    /// Marks it queued: `false` if it already was.
    pub fn queue(&mut self) -> bool {
        !mem::replace(&mut self.queued, true)
    }

    /// Whether it was queued, which from then on it is not.
    pub fn take_queued(&mut self) -> bool {
        mem::take(&mut self.queued)
    }
}
