//! A running body and what it was started with.

use std::collections::BTreeMap;
use std::mem;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

use crate::cancel::Cancel;
use crate::cx::Cx;
use crate::declaration::Declaration;
use crate::declared_children::{ChildSteps, DeclaredChildren};
use crate::erased_kind::{Body, ErasedKind};
use crate::index::StateId;
use crate::job::Product;
use crate::key::Chain;
use crate::mailbox::{Mailbox, Posted, Reply};
use crate::outcome::Erased;
use crate::produced::Produced;
use crate::record::Answers;

/// What a finished body leaves for its node's answer.
pub(crate) struct Finished {
    pub declaration: Arc<Declaration>,
    /// The keys its reads took it through.
    pub chain: Chain,
    /// The steps of the children its run declared.
    pub children: ChildSteps,
    /// Whether its answer depended on a substitution.
    pub depended: bool,
}

pub(crate) struct Task {
    declaration: Arc<Declaration>,
    /// Whether its answer depends on a substitution so far: its declaration
    /// did, or a read of its own returned an answer that did.
    depended: bool,
    /// The state of each view its run opened, by ordinal, once its first
    /// read opened it.
    views: BTreeMap<u32, StateId>,
    /// The keys its reads have taken it through, from its recipe's key.
    chain: Chain,
    /// The children its answered reads declared.
    children: DeclaredChildren,
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
    /// Starts the body of `kind` for a node declared `declaration`, whose
    /// declaration `depended` on a substitution or not.
    pub fn start(kind: &dyn ErasedKind, declaration: Arc<Declaration>, depended: bool) -> Self {
        let mailbox = Arc::new(Mailbox::new());
        let cancel = Cancel::default();
        let body = kind.start(Cx::new(Arc::clone(&mailbox), cancel.clone()));
        Self {
            declaration,
            depended,
            views: BTreeMap::new(),
            chain: Chain::new(kind.recipe()),
            children: DeclaredChildren::default(),
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

    /// The children its answered reads declared.
    pub fn children(&self) -> &DeclaredChildren {
        &self.children
    }

    /// Whether its answer depends on a substitution so far.
    pub fn depended(&self) -> bool {
        self.depended
    }

    /// The state of the view its run opened as `ordinal`, if a read has
    /// opened it.
    pub fn view(&self, ordinal: u32) -> Option<StateId> {
        self.views.get(&ordinal).copied()
    }

    /// Records that `state` is the view its run opened as `ordinal`.
    pub fn opened(&mut self, ordinal: u32, state: StateId) {
        self.views.insert(ordinal, state);
    }

    /// The states of the views its run opened.
    pub fn views(&self) -> impl Iterator<Item = StateId> + '_ {
        self.views.values().copied()
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

    /// Replies to its read, which its chain records, with the children it
    /// declared; its answer depends on a substitution from then on if any
    /// answer did. A read through a view it opened is replied to with what
    /// depended on that view's substitution, which its own answer does not
    /// depend on: its run is in the state that opened the view.
    pub fn answer_read(&mut self, mut answers: Answers) {
        if answers.through_view() {
            self.mailbox.reply(Reply::Viewed(answers.into_viewed()));
            return;
        }
        self.depended |= answers.depended();
        answers.record_children(&mut self.children);
        self.chain
            .read(Arc::clone(answers.wanted()), answers.contents());
        self.mailbox.reply(Reply::Outcomes(answers.into_outcomes()));
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

    /// Discards the finished body, keeping what it was started with, the
    /// chain its reads took it through, the steps of the children they
    /// declared and whether its answer depended on a substitution.
    pub fn finish(self) -> Finished {
        let Self {
            declaration,
            depended,
            chain,
            children,
            mailbox,
            body,
            ..
        } = self;
        let _ = catch_unwind(AssertUnwindSafe(move || drop((body, mailbox))));
        Finished {
            declaration,
            chain,
            children: children.into_steps(),
            depended,
        }
    }

    /// Tells the section the body waits on, if any, that nobody wants the
    /// node any more, then discards the task.
    pub fn abandon(self) {
        self.cancel.set();
        self.discard();
    }
}
