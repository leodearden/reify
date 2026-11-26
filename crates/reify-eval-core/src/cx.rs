//! What a running body holds: its own mailbox and cancel flag, and nothing
//! of the core's.

use std::sync::Arc;

use crate::cancel::Cancel;
use crate::mailbox::{Ask, Mailbox, Reply};
use crate::outcome::{Outcome, Value};
use crate::reference::Ref;

pub struct Cx {
    mailbox: Arc<Mailbox>,
    cancel: Cancel,
}

impl Cx {
    pub(crate) fn new(mailbox: Arc<Mailbox>, cancel: Cancel) -> Self {
        Self { mailbox, cancel }
    }

    /// The outcome at `r`, typed as `T`: a value of another type is a
    /// failure naming both types.
    pub async fn read<T: Value>(&self, r: &Ref) -> Outcome<T> {
        let Reply::Outcome(outcome) = self.mailbox.exchange(Ask::Read(r.clone())).await;
        outcome.downcast()
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.is_set()
    }
}
