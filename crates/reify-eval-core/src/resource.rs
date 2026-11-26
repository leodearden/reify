//! Every exclusive resource, each alone on a thread of its own that runs
//! one job at a time, and the one channel their reports come back on.

use std::any::{Any, TypeId};
use std::collections::BTreeMap;
use std::mem;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::{self, JoinHandle};

use crate::held::ReleaseHome;
use crate::job::{Job, Report, SectionRun};
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

pub(crate) struct Resources {
    threads: BTreeMap<TypeId, ResourceThread>,
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
        let thread = thread::spawn(move || serve(make(), queued, reports));
        if let Some(replaced) = self.threads.insert(TypeId::of::<R>(), ResourceThread { jobs, thread }) {
            replaced.stop();
        }
    }

    /// Queues a section on its resource's thread; `None` if nobody added
    /// that resource.
    pub fn start(&mut self, resource: TypeId, job: SectionJob) -> Option<u64> {
        let thread = self.threads.get(&resource)?;
        self.serial += 1;
        let serial = self.serial;
        let parts = SectionParts {
            home: ReleaseHome::new(thread.jobs.clone(), Arc::clone(&self.pending_releases)),
        };
        let run: SectionRun = Box::new(move |resource| job(resource, parts));
        thread.jobs.send(Job::Section { serial, run }).ok()?;
        Some(serial)
    }

    /// Blocks until a resource thread reports.
    pub fn receive(&self) -> Report {
        self.reports.recv().expect("the core keeps a sender of its own")
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

fn serve<R: 'static>(mut resource: R, jobs: Receiver<Job>, reports: Sender<Report>) {
    let resource: &mut dyn Any = &mut resource;
    for job in jobs {
        match job {
            Job::Section { serial, run } => {
                let product = Ok(run(resource));
                let _ = reports.send(Report::Finished { serial, product });
            }
            Job::Release(release) => {
                release(resource);
                let _ = reports.send(Report::Released);
            }
            Job::Stop => break,
        }
    }
}
