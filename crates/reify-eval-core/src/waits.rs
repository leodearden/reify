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

    /// The nodes `waiter` awaits.
    pub fn awaits(&self, waiter: TaskKey) -> impl Iterator<Item = TaskKey> + '_ {
        self.awaits.get(&waiter).into_iter().flatten().copied()
    }

    /// `from`, and every node reachable from it through what each node
    /// awaits, worked with a stack rather than by recursion.
    pub fn reach(&self, from: impl IntoIterator<Item = TaskKey>) -> BTreeSet<TaskKey> {
        let mut reached = BTreeSet::new();
        let mut unvisited: Vec<_> = from.into_iter().collect();
        while let Some(key) = unvisited.pop() {
            if reached.insert(key) {
                unvisited.extend(self.awaits(key));
            }
        }
        reached
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

    /// Ends every wait of `waiter`, which has been dropped.
    pub fn forget(&mut self, waiter: TaskKey) {
        for key in self.awaits.remove(&waiter).unwrap_or_default() {
            if let Some(waiters) = self.awaited_by.get_mut(&key) {
                waiters.remove(&waiter);
                if waiters.is_empty() {
                    self.awaited_by.remove(&key);
                }
            }
        }
    }
}
