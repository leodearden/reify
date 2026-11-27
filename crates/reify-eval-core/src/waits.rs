//! The wait relation: each node in progress and the nodes it awaits — a
//! body waiting on its reads, or a path waiting on its parent — seen from
//! both ends, with one owner.

use std::collections::{BTreeMap, BTreeSet};

use crate::index::NodeKey;

#[derive(Default)]
pub(crate) struct Waits {
    awaits: BTreeMap<NodeKey, BTreeSet<NodeKey>>,
    awaited_by: BTreeMap<NodeKey, BTreeSet<NodeKey>>,
}

impl Waits {
    pub fn add(&mut self, waiter: NodeKey, awaited: NodeKey) {
        self.awaits.entry(waiter).or_default().insert(awaited);
        self.awaited_by.entry(awaited).or_default().insert(waiter);
    }

    /// Ends every wait on `awaited`, which has been answered, returning the
    /// waiters that now await nothing.
    pub fn release(&mut self, awaited: NodeKey) -> Vec<NodeKey> {
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
    pub fn awaits(&self, waiter: NodeKey) -> impl Iterator<Item = NodeKey> + '_ {
        self.awaits.get(&waiter).into_iter().flatten().copied()
    }

    /// `from`, and every node reachable from it through what each node
    /// awaits, worked with a stack rather than by recursion.
    pub fn reach(&self, from: impl IntoIterator<Item = NodeKey>) -> BTreeSet<NodeKey> {
        let mut reached = BTreeSet::new();
        let mut unvisited: Vec<_> = from.into_iter().collect();
        while let Some(key) = unvisited.pop() {
            if reached.insert(key) {
                unvisited.extend(self.awaits(key));
            }
        }
        reached
    }

    /// `from`, and every node that waits on it, directly or through other
    /// waits, worked with a stack rather than by recursion.
    pub fn reach_waiters(&self, from: NodeKey) -> BTreeSet<NodeKey> {
        let mut reached = BTreeSet::new();
        let mut unvisited = vec![from];
        while let Some(key) = unvisited.pop() {
            if reached.insert(key) {
                unvisited.extend(self.awaited_by.get(&key).into_iter().flatten());
            }
        }
        reached
    }

    /// Whether any node waits on `awaited`.
    pub fn is_awaited(&self, awaited: NodeKey) -> bool {
        self.awaited_by.contains_key(&awaited)
    }

    /// The nodes waiting on `awaited`.
    pub fn waiters(&self, awaited: NodeKey) -> Vec<NodeKey> {
        self.awaited_by
            .get(&awaited)
            .into_iter()
            .flatten()
            .copied()
            .collect()
    }

    /// Ends every wait of `waiter`, which has been dropped.
    pub fn forget(&mut self, waiter: NodeKey) {
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
