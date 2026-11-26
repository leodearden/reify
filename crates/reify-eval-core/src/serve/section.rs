//! An exclusive section: started on its resource's thread, and its product
//! delivered to the body waiting on it.

use std::any::TypeId;
use std::collections::{BTreeMap, VecDeque};

use crate::counters::Counters;
use crate::failure::OutsideFailure;
use crate::job::{Product, SectionFault};
use crate::mailbox::{Reply, SectionJob};
use crate::ready::ReadyQueue;
use crate::resource::{Resources, Unstarted};
use crate::task::{Task, TaskKey};

/// A section started and not yet delivered.
struct Started {
    key: TaskKey,
    resource: TypeId,
    resource_name: &'static str,
}

/// The sections started and not yet delivered, each with the task that
/// started it, and those of them that have finished, in order of arrival.
#[derive(Default)]
pub(crate) struct InFlight {
    started: BTreeMap<u64, Started>,
    finished: VecDeque<(u64, Result<Product, SectionFault>)>,
}

impl InFlight {
    pub fn running(&self) -> usize {
        self.started.len()
    }

    pub fn arrived(&mut self, serial: u64, product: Result<Product, SectionFault>) {
        self.finished.push_back((serial, product));
    }

    pub fn take_finished(&mut self) -> Option<(u64, Result<Product, SectionFault>)> {
        self.finished.pop_front()
    }
}

/// Starts the section `task` posted. An `Err` is the failure its node ends
/// with.
pub(crate) fn start(
    task: &mut Task,
    key: &TaskKey,
    (resource, resource_name): (TypeId, &'static str),
    job: SectionJob,
    resources: &mut Resources,
    in_flight: &mut InFlight,
    counters: &mut Counters,
) -> Result<(), OutsideFailure> {
    let serial = resources.start(resource, task.cancel.clone(), job).map_err(|unstarted| match unstarted {
        Unstarted::NoResource => OutsideFailure::NoResource { resource: resource_name },
        Unstarted::Dead => OutsideFailure::ResourceDead { resource: resource_name },
    })?;
    counters.count_section();
    task.section = Some(serial);
    in_flight.started.insert(serial, Started { key: key.clone(), resource, resource_name });
    Ok(())
}

/// Delivers a finished section to the task waiting on it. A product whose
/// task is gone is dropped here, which releases what it held. A section
/// that gave no product returns the node to end and the failure outside
/// the inputs to end it with; a panic marks its resource dead.
pub(crate) fn deliver(
    serial: u64,
    product: Result<Product, SectionFault>,
    in_flight: &mut InFlight,
    tasks: &mut BTreeMap<TaskKey, Task>,
    ready: &mut ReadyQueue<TaskKey>,
    resources: &mut Resources,
) -> Option<(TaskKey, OutsideFailure)> {
    let started = in_flight.started.remove(&serial)?;
    let waiting = tasks.get_mut(&started.key).filter(|task| task.waits_on(serial));
    let failure = match (product, waiting) {
        (Ok(product), Some(task)) => {
            task.mailbox.reply(Reply::Product(product));
            task.section = None;
            ready.push(started.key);
            return None;
        }
        (Ok(product), None) => {
            drop(product);
            return None;
        }
        (Err(SectionFault::Panicked { message }), waiting) => {
            resources.mark_dead(started.resource);
            waiting.map(|_| OutsideFailure::SectionPanicked { resource: started.resource_name, message })
        }
        (Err(SectionFault::Dead), waiting) => {
            waiting.map(|_| OutsideFailure::ResourceDead { resource: started.resource_name })
        }
    };
    failure.map(|failure| (started.key, failure))
}

/// The task that is still waiting on section `serial`, if any: the one
/// that started it may have been dropped, and another started in its place.
pub(crate) fn waiting_task<'a>(
    serial: u64,
    in_flight: &'a InFlight,
    tasks: &BTreeMap<TaskKey, Task>,
) -> Option<&'a TaskKey> {
    let key = &in_flight.started.get(&serial)?.key;
    tasks.get(key).is_some_and(|task| task.waits_on(serial)).then_some(key)
}
