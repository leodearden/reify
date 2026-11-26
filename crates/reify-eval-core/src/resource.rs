//! Every exclusive resource, each alone on a thread of its own that runs
//! one job at a time, and the one channel their reports come back on.

use std::any::TypeId;
use std::collections::{BTreeMap, BTreeSet};
use std::mem;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::{self, JoinHandle};

use crate::cancel::Cancel;
use crate::failure::panic_message;
use crate::held::ReleaseHome;
use crate::job::{Job, Report, SectionFault, SectionRun};
use crate::mailbox::SectionJob;
use crate::section::SectionParts;

struct ResourceThread {
    jobs: Sender<Job>,
    thread: JoinHandle<()>,
}

impl ResourceThread {
    fn stop(self) {
        let _ = self.jobs.send(Job::Stop);
        let _ = self.thread.join();
    }
}

/// Why a section could not be queued.
pub(crate) enum Unstarted {
    NoResource,
    Dead,
}

pub(crate) struct Resources {
    threads: BTreeMap<TypeId, ResourceThread>,
    dead: BTreeSet<TypeId>,
    reports_in: Sender<Report>,
    reports: Receiver<Report>,
    pending_releases: Arc<AtomicUsize>,
    serial: u64,
}

impl Resources {
    pub fn new() -> Self {
        let (reports_in, reports) = channel();
        Self {
            threads: BTreeMap::new(),
            dead: BTreeSet::new(),
            reports_in,
            reports,
            pending_releases: Arc::default(),
            serial: 0,
        }
    }

    /// Starts resource `R`'s thread, which makes it, so `R` need not be
    /// `Send`. Adding a type again replaces the first, once its queued
    /// jobs have run.
    pub fn add<R: 'static>(&mut self, make: impl FnOnce() -> R + Send + 'static) {
        let (jobs, queued) = channel();
        let reports = self.reports_in.clone();
        let thread = thread::spawn(move || serve(make, queued, reports));
        self.dead.remove(&TypeId::of::<R>());
        if let Some(replaced) = self.threads.insert(TypeId::of::<R>(), ResourceThread { jobs, thread }) {
            replaced.stop();
        }
    }

    /// Queues a section on its resource's thread.
    pub fn start(&mut self, resource: TypeId, cancel: Cancel, job: SectionJob) -> Result<u64, Unstarted> {
        if self.dead.contains(&resource) {
            return Err(Unstarted::Dead);
        }
        let thread = self.threads.get(&resource).ok_or(Unstarted::NoResource)?;
        self.serial += 1;
        let serial = self.serial;
        let parts = SectionParts {
            serial,
            cancel,
            reports: self.reports_in.clone(),
            home: ReleaseHome::new(thread.jobs.clone(), Arc::clone(&self.pending_releases)),
        };
        let run: SectionRun = Box::new(move |resource| job(resource, parts));
        thread.jobs.send(Job::Section { serial, run }).map_err(|_| Unstarted::Dead)?;
        Ok(serial)
    }

    /// Marks `resource` dead once the core has delivered its section's panic,
    /// so later sections for it fail without being queued.
    pub fn mark_dead(&mut self, resource: TypeId) {
        self.dead.insert(resource);
    }

    /// Blocks until a resource thread reports.
    pub fn receive(&self) -> Report {
        self.reports.recv().expect("the core keeps a sender of its own")
    }

    /// A report already sent, if there is one.
    pub fn try_receive(&self) -> Option<Report> {
        self.reports.try_recv().ok()
    }

    pub fn acknowledge_release(&self) {
        self.pending_releases.fetch_sub(1, Ordering::SeqCst);
    }

    /// Releases sent to a resource thread and not yet reported done.
    pub fn pending_releases(&self) -> usize {
        self.pending_releases.load(Ordering::SeqCst)
    }
}

impl Drop for Resources {
    /// Products nobody took are dropped first, so what they hold is queued
    /// for release ahead of `Stop`.
    fn drop(&mut self) {
        while self.reports.try_recv().is_ok() {}
        for (_, thread) in mem::take(&mut self.threads) {
            thread.stop();
        }
    }
}

/// A resource thread. After a section panics the resource is dead: the
/// thread never touches it again, answers each later section `Dead` and
/// each release `Released` without running either, and drops it on `Stop`.
/// Every job is answered, so the core never waits on a report that will
/// not come.
fn serve<R: 'static>(make: impl FnOnce() -> R, jobs: Receiver<Job>, reports: Sender<Report>) {
    let mut resource = guarded(make).ok();
    let mut alive = resource.is_some();
    for job in jobs {
        match job {
            Job::Section { serial, run } => {
                let product = match resource.as_mut() {
                    Some(resource) if alive => guarded(|| run(resource)).map_err(|message| SectionFault::Panicked { message }),
                    _ => {
                        let _ = guarded(move || drop(run));
                        Err(SectionFault::Dead)
                    }
                };
                alive = alive && product.is_ok();
                let _ = reports.send(Report::Finished { serial, product });
            }
            Job::Release(release) => {
                match resource.as_mut() {
                    Some(resource) if alive => alive = guarded(|| release(resource)).is_ok(),
                    _ => {
                        let _ = guarded(move || drop(release));
                    }
                }
                let _ = reports.send(Report::Released);
            }
            Job::Stop => break,
        }
    }
    let _ = guarded(move || drop(resource));
}

/// Runs `work`, turning a panic into its message.
fn guarded<T>(work: impl FnOnce() -> T) -> Result<T, String> {
    catch_unwind(AssertUnwindSafe(work)).map_err(|payload| panic_message(&*payload))
}
