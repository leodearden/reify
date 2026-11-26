//! A running body and what it was started with.

use std::mem;
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
use crate::produced::Produced;

pub(crate) struct Task {
    declaration: Arc<Declaration>,
    mailbox: Arc<Mailbox>,
    cancel: Cancel,
    /// The section the body waits on, if it waits on one.
    section: Option<u64>,
    /// Whether the body is queued to be polled. A ready-queue entry for a
    /// task that is not owed a poll is stale: left by a dropped task whose
    /// successor has the same key.
    owed_poll: bool,
    body: Body,
}

impl Task {
    pub fn start(kind: &dyn ErasedKind, declaration: Arc<Declaration>) -> Self {
        let mailbox = Arc::new(Mailbox::new());
        let cancel = Cancel::default();
        let body = kind.start(Cx::new(Arc::clone(&mailbox), cancel.clone()));
        Self {
            declaration,
            mailbox,
            cancel,
            section: None,
            owed_poll: false,
            body,
        }
    }

    pub fn declaration(&self) -> &Arc<Declaration> {
        &self.declaration
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

    pub fn answer_read(&self, outcomes: Vec<Outcome<Erased>>) {
        self.mailbox.reply(Reply::Outcomes(outcomes));
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

    /// Marks the body as owed a poll: `false` if it already was, so that it
    /// is queued once.
    pub fn owe_poll(&mut self) -> bool {
        !mem::replace(&mut self.owed_poll, true)
    }

    /// Whether the body is owed a poll, which from then on it is not.
    pub fn take_poll(&mut self) -> bool {
        mem::take(&mut self.owed_poll)
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
