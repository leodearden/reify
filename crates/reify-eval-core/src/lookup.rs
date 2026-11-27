//! A lookup (contract 4.2): a node in progress following the store from
//! its recipe's key through the contents of what each stored request
//! reads, until it reaches a result or a key where nothing is filed. A
//! stored request that declares children declares them again, at the
//! lookup's own path (contract 7.1).

use std::mem;
use std::sync::Arc;

use crate::declaration::Declaration;
use crate::declared_children::{ChildSteps, DeclaredChildren};
use crate::index::Declared;
use crate::key::Key;
use crate::record::Answers;

pub(crate) struct Lookup {
    declared: Declared,
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
    /// A lookup for a node `declared` so, starting at `key`.
    pub fn new(declared: Declared, key: Key) -> Self {
        Self {
            depended: declared.depended,
            declared,
            key,
            children: DeclaredChildren::default(),
            queued: false,
        }
    }

    pub fn declaration(&self) -> &Arc<Declaration> {
        &self.declared.declaration
    }

    /// The children the requests it moved past declared.
    pub fn children(&self) -> &DeclaredChildren {
        &self.children
    }

    /// Whether the answer it finds depends on a substitution so far.
    pub fn depended(&self) -> bool {
        self.depended
    }

    /// Gives up the lookup, which found nothing to apply, leaving what the
    /// node was declared as for the body that runs in its place.
    pub fn miss(self) -> Declared {
        self.declared
    }

    /// Gives up the lookup, which found a result: the node's declaration,
    /// the steps of the children it declared and whether the result
    /// depended on a substitution.
    pub fn found(self) -> (Arc<Declaration>, ChildSteps, bool) {
        let declaration = self.declared.declaration;
        (declaration, self.children.into_steps(), self.depended)
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
