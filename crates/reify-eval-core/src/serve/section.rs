//! Exclusive sections in flight: each started on its resource's thread for
//! one task, and what it gave once it has finished.

use std::any::TypeId;
use std::collections::{BTreeMap, VecDeque};

use crate::cancel::Cancel;
use crate::failure::OutsideFailure;
use crate::job::{Product, SectionFault};
use crate::mailbox::SectionAsk;
use crate::resource::{Resources, Unstarted};
use crate::task::TaskKey;

/// A section started and not yet delivered.
struct Started {
    key: TaskKey,
    resource: TypeId,
    resource_name: &'static str,
}

/// A finished section, for the task that started it: its product, or the
/// failure outside the inputs that task's node ends with.
pub(crate) struct Delivery {
    pub key: TaskKey,
    pub serial: u64,
    pub product: Result<Product, OutsideFailure>,
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

    /// Queues the section task `key` asked for, returning its serial. An
    /// `Err` is the failure the task's node ends with.
    pub fn start(
        &mut self,
        key: &TaskKey,
        ask: SectionAsk,
        cancel: Cancel,
        resources: &mut Resources,
    ) -> Result<u64, OutsideFailure> {
        let SectionAsk {
            resource,
            resource_name,
            job,
        } = ask;
        let serial =
            resources
                .start(resource, cancel, job)
                .map_err(|unstarted| match unstarted {
                    Unstarted::NoResource => OutsideFailure::NoResource {
                        resource: resource_name,
                    },
                    Unstarted::Dead => OutsideFailure::ResourceDead {
                        resource: resource_name,
                    },
                })?;
        let started = Started {
            key: key.clone(),
            resource,
            resource_name,
        };
        self.started.insert(serial, started);
        Ok(serial)
    }

    pub fn arrived(&mut self, serial: u64, product: Result<Product, SectionFault>) {
        self.finished.push_back((serial, product));
    }

    /// The earliest finished section, no longer in flight. A panic marks
    /// its resource dead.
    pub fn deliver(&mut self, resources: &mut Resources) -> Option<Delivery> {
        let (serial, product) = self.finished.pop_front()?;
        let started = self
            .started
            .remove(&serial)
            .expect("a section reports once, after it was started");
        let product = product.map_err(|fault| match fault {
            SectionFault::Panicked { message } => {
                resources.mark_dead(started.resource);
                OutsideFailure::SectionPanicked {
                    resource: started.resource_name,
                    message,
                }
            }
            SectionFault::Dead => OutsideFailure::ResourceDead {
                resource: started.resource_name,
            },
        });
        Some(Delivery {
            key: started.key,
            serial,
            product,
        })
    }

    /// The task that started section `serial`, while it is in flight.
    pub fn starter(&self, serial: u64) -> Option<&TaskKey> {
        self.started.get(&serial).map(|started| &started.key)
    }
}
