//! Things that live inside a resource, and how they get back to it: when
//! the last holder drops one, anywhere, it is released on the resource's
//! own thread.

use std::any::Any;
use std::fmt;
use std::marker::PhantomData;
use std::ops::Deref;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Sender;

use crate::job::{Job, ReleaseRun};

/// How a thing that lives inside resource `R` is given back to it.
pub trait Release<R>: Send + 'static {
    fn release(self, resource: &mut R);
}

/// The thread a held thing goes back to. A release is counted pending when
/// it is sent, and the core counts it done when the thread reports it, so
/// a count above zero always has a report on its way.
#[derive(Clone)]
pub(crate) struct ReleaseHome {
    jobs: Sender<Job>,
    pending: Arc<AtomicUsize>,
}

impl ReleaseHome {
    pub fn new(jobs: Sender<Job>, pending: Arc<AtomicUsize>) -> Self {
        Self { jobs, pending }
    }

    fn send(&self, release: ReleaseRun) {
        self.pending.fetch_add(1, Ordering::SeqCst);
        if self.jobs.send(Job::Release(release)).is_err() {
            self.pending.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

/// A thing that lives inside resource `R`.
pub struct Held<R: 'static, P: Release<R>> {
    thing: Option<P>,
    home: ReleaseHome,
    resource: PhantomData<fn(&mut R)>,
}

impl<R: 'static, P: Release<R>> Held<R, P> {
    pub(crate) fn new(thing: P, home: ReleaseHome) -> Self {
        Self { thing: Some(thing), home, resource: PhantomData }
    }
}

impl<R: 'static, P: Release<R>> Deref for Held<R, P> {
    type Target = P;

    fn deref(&self) -> &P {
        self.thing.as_ref().expect("a held thing is taken only when it is dropped")
    }
}

impl<R: 'static, P: Release<R>> Drop for Held<R, P> {
    fn drop(&mut self) {
        let Some(thing) = self.thing.take() else { return };
        self.home.send(Box::new(move |resource: &mut dyn Any| {
            let resource = resource.downcast_mut::<R>();
            thing.release(resource.expect("a resource thread holds the type it was added under"));
        }));
    }
}

impl<R: 'static, P: Release<R> + fmt::Debug> fmt::Debug for Held<R, P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Held").field(&**self).finish()
    }
}

impl<R: 'static, P: Release<R> + PartialEq> PartialEq for Held<R, P> {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}
