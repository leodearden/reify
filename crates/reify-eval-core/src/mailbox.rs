//! The one slot through which a body and the core talk: a body posts one
//! ask and is suspended; the core puts the reply in the same slot. So a
//! body makes one request at a time (O2) by construction.

use std::any::{Any, TypeId};
use std::future::Future;
use std::mem;
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll};

use crate::job::Product;
use crate::outcome::{Erased, Outcome};
use crate::section::SectionParts;
use crate::substitution::Substitution;
use crate::wanted::Wanted;

pub(crate) type SectionJob = Box<dyn FnOnce(&mut dyn Any, SectionParts) -> Product + Send>;

/// A section a body asks to run on resource `resource`.
pub(crate) struct SectionAsk {
    pub resource: TypeId,
    pub resource_name: &'static str,
    pub job: SectionJob,
}

pub(crate) enum Ask {
    /// The outcome of each item, in order, in one reply.
    Read(Vec<Wanted>),
    /// The outcome of each item under the run's view `ordinal`, which
    /// supposes `substitution`, in order, in one reply.
    ViewRead {
        ordinal: u32,
        substitution: Substitution,
        wanted: Arc<[Wanted]>,
    },
    Exclusive(SectionAsk),
}

pub(crate) enum Reply {
    Outcomes(Vec<Outcome<Erased>>),
    /// What each item read through a view gave.
    Viewed(Vec<Viewed<Erased>>),
    Product(Product),
}

/// What a read through a view gave. Defined with the replies, which carry
/// it, and named by `view`, which reads it.
#[derive(Debug, PartialEq)]
pub struct Viewed<T: ?Sized> {
    pub outcome: Outcome<T>,
    /// Whether the answer depended on the substitution: it is supposed, or
    /// what it was worked out from read a supposed answer.
    pub depended: bool,
}

pub(crate) enum Posted {
    Ask(Ask),
    Nothing,
    TwoRequestsAtOnce,
}

enum Slot {
    Empty,
    Ask(Ask),
    TwoRequestsAtOnce,
    Reply(Reply),
}

pub(crate) struct Mailbox {
    slot: Mutex<Slot>,
    /// How many views the run has opened: a note the body leaves, which
    /// asks nothing of the core.
    views: AtomicU32,
    /// The ordinals of the views whose handles the body has dropped since
    /// the core last took them: notes, like `views`.
    dropped_views: Mutex<Vec<u32>>,
}

impl Mailbox {
    pub fn new() -> Self {
        Self {
            slot: Mutex::new(Slot::Empty),
            views: AtomicU32::new(0),
            dropped_views: Mutex::new(Vec::new()),
        }
    }

    /// Notes that the body dropped the handle of its view `ordinal`, so
    /// that nothing reads through it any more.
    pub fn drop_view(&self, ordinal: u32) {
        lock(&self.dropped_views).push(ordinal);
    }

    /// The ordinals of the views dropped since this was last asked.
    pub fn take_dropped_views(&self) -> Vec<u32> {
        mem::take(&mut *lock(&self.dropped_views))
    }

    /// Notes that the run opens a view, returning its ordinal: the number
    /// of views the run opened before it.
    pub fn open_view(&self) -> u32 {
        self.views.fetch_add(1, Ordering::Relaxed)
    }

    /// Whether the run has opened a view.
    pub fn has_opened_view(&self) -> bool {
        self.views.load(Ordering::Relaxed) > 0
    }

    pub fn take_posted(&self) -> Posted {
        let mut slot = self.slot();
        match mem::replace(&mut *slot, Slot::Empty) {
            Slot::Ask(ask) => Posted::Ask(ask),
            Slot::TwoRequestsAtOnce => Posted::TwoRequestsAtOnce,
            other => {
                *slot = other;
                Posted::Nothing
            }
        }
    }

    pub fn reply(&self, reply: Reply) {
        *self.slot() = Slot::Reply(reply);
    }

    pub fn exchange(&self, ask: Ask) -> Exchange<'_> {
        Exchange {
            mailbox: self,
            ask: Some(ask),
        }
    }

    fn slot(&self) -> MutexGuard<'_, Slot> {
        lock(&self.slot)
    }
}

/// Locks `mutex`, whatever a panicking body left in it.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// An ask in flight: posts on its first poll, then waits for the reply.
pub(crate) struct Exchange<'a> {
    mailbox: &'a Mailbox,
    ask: Option<Ask>,
}

impl Future for Exchange<'_> {
    type Output = Reply;

    fn poll(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Reply> {
        let ask = self.ask.take();
        let mut slot = self.mailbox.slot();
        if let Some(ask) = ask {
            *slot = match *slot {
                Slot::Empty => Slot::Ask(ask),
                _ => Slot::TwoRequestsAtOnce,
            };
            return Poll::Pending;
        }
        match mem::replace(&mut *slot, Slot::Empty) {
            Slot::Reply(reply) => Poll::Ready(reply),
            other => {
                *slot = other;
                Poll::Pending
            }
        }
    }
}
