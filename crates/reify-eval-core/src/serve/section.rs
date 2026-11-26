//! An exclusive section: started on its resource's thread, and its product
//! delivered to the body waiting on it.

use std::any::TypeId;
use std::collections::{BTreeMap, VecDeque};

use crate::counters::Counters;
use crate::failure::OutsideFailure;
use crate::job::{Product, SectionFault};
use crate::mailbox::{Reply, SectionJob};
use crate::ready::ReadyQueue;
use crate::resource::Resources;
use crate::task::{Task, TaskKey};

/// The sections started and not yet delivered, each with the task that
/// started it, and those of them that have finished, in order of arrival.
#[derive(Default)]
pub(crate) struct InFlight {
    started: BTreeMap<u64, TaskKey>,
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
    let serial = resources.start(resource, job).ok_or(OutsideFailure::NoResource { resource: resource_name })?;
    counters.count_section();
    task.section = Some(serial);
    in_flight.started.insert(serial, key.clone());
    Ok(())
}

/// Delivers a finished section to the task waiting on it. A product whose
/// task is gone is dropped here, which releases what it held.
pub(crate) fn deliver(
    serial: u64,
    product: Result<Product, SectionFault>,
    in_flight: &mut InFlight,
    tasks: &mut BTreeMap<TaskKey, Task>,
    ready: &mut ReadyQueue<TaskKey>,
) {
    let Some(key) = in_flight.started.remove(&serial) else { return };
    let product = match product {
        Ok(product) => product,
        Err(fault) => match fault {},
    };
    match tasks.get_mut(&key).filter(|task| task.section == Some(serial)) {
        Some(task) => {
            task.mailbox.reply(Reply::Product(product));
            task.section = None;
            ready.push(key);
        }
        None => drop(product),
    }
}
