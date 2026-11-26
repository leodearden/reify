//! A lookup (contract 4.2): a node in progress following the store from
//! its recipe's key through the contents of what each stored request
//! reads, until it reaches a result or a key where nothing is filed.

use std::mem;
use std::sync::Arc;

use crate::declaration::Declaration;
use crate::key::Key;
use crate::record::Answers;

pub(crate) struct Lookup {
    declaration: Arc<Declaration>,
    key: Key,
    /// Whether it is queued to be stepped; as a task's `owed_poll`.
    queued: bool,
}

impl Lookup {
    pub fn new(declaration: Arc<Declaration>, key: Key) -> Self {
        Self {
            declaration,
            key,
            queued: false,
        }
    }

    pub fn declaration(&self) -> &Arc<Declaration> {
        &self.declaration
    }

    pub fn key(&self) -> Key {
        self.key
    }

    /// Moves on past the request stored at its key, given what it read.
    pub fn advance(&mut self, answers: &Answers) {
        self.key = self.key.then(answers.refs(), answers.contents());
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
