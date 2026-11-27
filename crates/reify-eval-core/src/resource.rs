//! Every exclusive resource, each alone on a thread of its own that runs
//! one job at a time, and the one channel their reports come back on.

use std::any::{TypeId, type_name};
use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::mem;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::{self, JoinHandle};

use crate::cancel::Cancel;
use crate::failure::{ResourceDeath, panic_message};
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
    Dead(ResourceDeath),
}

pub(crate) struct Resources {
    threads: BTreeMap<TypeId, ResourceThread>,
    dead: BTreeMap<TypeId, ResourceDeath>,
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
            dead: BTreeMap::new(),
            reports_in,
            reports,
            pending_releases: Arc::default(),
            serial: 0,
        }
    }

    /// Starts resource `R`'s thread, which makes it, so `R` need not be
    /// `Send`. One thread serves `R` for the core's whole life, so whatever
    /// a section made in `R` goes back to the thread that made it.
    ///
    /// # Panics
    ///
    /// If the core already has a resource of type `R`.
    pub fn add<R: 'static>(&mut self, make: impl FnOnce() -> R + Send + 'static) {
        let Entry::Vacant(slot) = self.threads.entry(TypeId::of::<R>()) else {
            panic!(
                "the core already has a resource of type {}",
                type_name::<R>()
            );
        };
        let (jobs, queued) = channel();
        let reports = self.reports_in.clone();
        let thread = thread::spawn(move || serve(make, queued, reports));
        slot.insert(ResourceThread { jobs, thread });
    }

    /// Queues a section on its resource's thread.
    pub fn start(
        &mut self,
        resource: TypeId,
        cancel: Cancel,
        job: SectionJob,
    ) -> Result<u64, Unstarted> {
        if let Some(death) = self.dead.get(&resource) {
            return Err(Unstarted::Dead(death.clone()));
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
        thread
            .jobs
            .send(Job::Section { serial, run })
            .expect("a resource thread runs until the core stops it");
        Ok(serial)
    }

    /// Marks `resource` dead once the core has delivered a section's report
    /// that it died, so later sections for it fail without being queued.
    pub fn mark_dead(&mut self, resource: TypeId, death: ResourceDeath) {
        self.dead.entry(resource).or_insert(death);
    }

    /// Blocks until a resource thread reports.
    pub fn receive(&self) -> Report {
        self.reports
            .recv()
            .expect("the core keeps a sender of its own")
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

/// A resource thread's resource. A dead one is never touched again; what
/// is left of it is dropped when the thread stops.
enum State<R> {
    Alive(R),
    Dead {
        death: ResourceDeath,
        _remains: Option<R>,
    },
}

impl<R> State<R> {
    fn made(make: impl FnOnce() -> R) -> Self {
        match guarded(make) {
            Ok(resource) => State::Alive(resource),
            Err(message) => State::Dead {
                death: ResourceDeath::MakePanicked { message },
                _remains: None,
            },
        }
    }

    fn killed(self, death: ResourceDeath) -> Self {
        match self {
            State::Alive(resource) => State::Dead {
                death,
                _remains: Some(resource),
            },
            dead => dead,
        }
    }
}

/// A resource thread. Once making the resource, a section on it or a
/// release in it panics, the resource is dead: the thread answers each
/// later section `Dead` with what killed it and each release `Released`,
/// running neither, and drops what is left on `Stop`. Every job is
/// answered, so the core never waits on a report that will not come.
fn serve<R: 'static>(make: impl FnOnce() -> R, jobs: Receiver<Job>, reports: Sender<Report>) {
    let mut state = State::made(make);
    for job in jobs {
        match job {
            Job::Section { serial, run } => {
                let product = match &mut state {
                    State::Alive(resource) => guarded(|| run(resource))
                        .map_err(|message| SectionFault::Panicked { message }),
                    State::Dead { death, .. } => {
                        let _ = guarded(move || drop(run));
                        Err(SectionFault::Dead(death.clone()))
                    }
                };
                if let Err(SectionFault::Panicked { message }) = &product {
                    let message = message.clone();
                    state = state.killed(ResourceDeath::SectionPanicked { message });
                }
                let _ = reports.send(Report::Finished { serial, product });
            }
            Job::Release(release) => {
                let failed = match &mut state {
                    State::Alive(resource) => guarded(|| release(resource)).err(),
                    State::Dead { .. } => {
                        let _ = guarded(move || drop(release));
                        None
                    }
                };
                if let Some(message) = failed {
                    state = state.killed(ResourceDeath::ReleasePanicked { message });
                }
                let _ = reports.send(Report::Released);
            }
            Job::Stop => break,
        }
    }
    let _ = guarded(move || drop(state));
}

/// Runs `work`, turning a panic into its message.
fn guarded<T>(work: impl FnOnce() -> T) -> Result<T, String> {
    catch_unwind(AssertUnwindSafe(work)).map_err(|payload| panic_message(&*payload))
}
