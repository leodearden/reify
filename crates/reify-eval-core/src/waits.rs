//! The wait relation: each node in progress and the nodes it awaits — a
//! body waiting on its reads, or a path waiting on its parent — seen from
//! both ends, with one owner.

use std::collections::{BTreeMap, BTreeSet};

use crate::task::TaskKey;

#[derive(Default)]
pub(crate) struct Waits {
    awaits: BTreeMap<TaskKey, BTreeSet<TaskKey>>,
    awaited_by: BTreeMap<TaskKey, BTreeSet<TaskKey>>,
}

impl Waits {
    pub fn add(&mut self, waiter: TaskKey, awaited: TaskKey) {
        self.awaits.entry(waiter).or_default().insert(awaited);
        self.awaited_by.entry(awaited).or_default().insert(waiter);
    }

    /// Ends every wait on `awaited`, which has been answered, returning the
    /// waiters that now await nothing.
    pub fn release(&mut self, awaited: TaskKey) -> Vec<TaskKey> {
        let mut freed = Vec::new();
        for waiter in self.awaited_by.remove(&awaited).unwrap_or_default() {
            let Some(rest) = self.awaits.get_mut(&waiter) else {
                continue;
            };
            rest.remove(&awaited);
            if rest.is_empty() {
                self.awaits.remove(&waiter);
                freed.push(waiter);
            }
        }
        freed
    }

    /// The nodes waiting on `awaited`.
    pub fn waiters(&self, awaited: TaskKey) -> Vec<TaskKey> {
        self.awaited_by
            .get(&awaited)
            .into_iter()
            .flatten()
            .copied()
            .collect()
    }

    /// Ends every wait of `waiter`, which has been dropped, returning what
    /// it awaited.
    pub fn forget(&mut self, waiter: TaskKey) -> BTreeSet<TaskKey> {
        let awaited = self.awaits.remove(&waiter).unwrap_or_default();
        for key in &awaited {
            if let Some(waiters) = self.awaited_by.get_mut(key) {
                waiters.remove(&waiter);
                if waiters.is_empty() {
                    self.awaited_by.remove(key);
                }
            }
        }
        awaited
    }
}
