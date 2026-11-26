//! A running body and what it was started with.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

use crate::cancel::Cancel;
use crate::cx::Cx;
use crate::declaration::Declaration;
use crate::erased_kind::{Body, ErasedKind};
use crate::job::Product;
use crate::mailbox::{Mailbox, Posted, Reply};
use crate::outcome::{Erased, Outcome};
use crate::path::Path;
use crate::produced::Produced;
use crate::snapshot::{Snapshot, SnapshotId};

/// A node of one snapshot: within a snapshot, one body runs for a path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct TaskKey {
    pub snapshot: SnapshotId,
    pub path: Path,
}

pub(crate) struct Task {
    declaration: Arc<Declaration>,
    snapshot: Snapshot,
    mailbox: Arc<Mailbox>,
    cancel: Cancel,
    /// The section the body waits on, if it waits on one.
    section: Option<u64>,
    body: Body,
}

impl Task {
    pub fn start(kind: &dyn ErasedKind, declaration: Arc<Declaration>, snapshot: Snapshot) -> Self {
        let mailbox = Arc::new(Mailbox::new());
        let cancel = Cancel::default();
        let body = kind.start(Cx::new(Arc::clone(&mailbox), cancel.clone()));
        Self {
            declaration,
            snapshot,
            mailbox,
            cancel,
            section: None,
            body,
        }
    }

    pub fn declaration(&self) -> &Declaration {
        &self.declaration
    }

    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }

    /// The flag a section this body starts reads as `cancelled`.
    pub fn cancel_flag(&self) -> Cancel {
        self.cancel.clone()
    }

    /// Runs the body until it finishes or is suspended. Nothing wakes a
    /// body: the core queues it again when its reply is in the mailbox.
    pub fn poll(&mut self) -> Poll<Produced<Erased>> {
        self.body
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
    }

    /// What the body asked for when it was last suspended.
    pub fn take_posted(&self) -> Posted {
        self.mailbox.take_posted()
    }

    pub fn answer_read(&self, outcome: Outcome<Erased>) {
        self.mailbox.reply(Reply::Outcome(outcome));
    }

    pub fn begin_section(&mut self, serial: u64) {
        self.section = Some(serial);
    }

    /// Gives the body what its section returned; it no longer waits.
    pub fn resume(&mut self, product: Product) {
        self.section = None;
        self.mailbox.reply(Reply::Product(product));
    }

    pub fn waits_on(&self, serial: u64) -> bool {
        self.section == Some(serial)
    }

    pub fn waits_on_a_section(&self) -> bool {
        self.section.is_some()
    }

    /// Drops the task, so that a panic while dropping what its body holds
    /// cannot escape the core.
    pub fn discard(self) {
        let _ = catch_unwind(AssertUnwindSafe(move || drop(self)));
    }

    /// Tells the section the body waits on, if any, that nobody wants the
    /// node any more, then discards the task.
    pub fn abandon(self) {
        self.cancel.set();
        self.discard();
    }
}
