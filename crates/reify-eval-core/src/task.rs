//! A running body and what it was started with.

use std::sync::Arc;
use std::task::{Context, Poll, Waker};

use crate::cancel::Cancel;
use crate::cx::Cx;
use crate::declaration::Declaration;
use crate::erased_kind::{Body, ErasedKind};
use crate::mailbox::Mailbox;
use crate::outcome::Erased;
use crate::path::Path;
use crate::produced::Produced;
use crate::snapshot::{Snapshot, SnapshotId};

/// A node of one snapshot: within a snapshot, one body runs for a path.
pub(crate) type TaskKey = (SnapshotId, Path);

pub(crate) struct Task {
    pub declaration: Arc<Declaration>,
    pub snapshot: Snapshot,
    pub mailbox: Arc<Mailbox>,
    pub cancel: Cancel,
    body: Body,
}

impl Task {
    pub fn start(kind: &dyn ErasedKind, declaration: Arc<Declaration>, snapshot: Snapshot) -> Self {
        let mailbox = Arc::new(Mailbox::new());
        let cancel = Cancel::default();
        let body = kind.start(Cx::new(Arc::clone(&mailbox), cancel.clone()));
        Self { declaration, snapshot, mailbox, cancel, body }
    }

    /// Runs the body until it finishes or is suspended. Nothing wakes a
    /// body: the core queues it again when its reply is in the mailbox.
    pub fn poll(&mut self) -> Poll<Produced<Erased>> {
        self.body.as_mut().poll(&mut Context::from_waker(Waker::noop()))
    }
}
