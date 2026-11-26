//! Rings of waits (contract 7.7): the nodes in progress each of which
//! needs, through what it awaits, its own answer. Looked for only at rest,
//! when every node in progress awaits another, so the rings do not depend
//! on the order of work.

use std::collections::{BTreeMap, BTreeSet};

use crate::content::Content;
use crate::failure::Failure;
use crate::index::{Entry, NodeKey};
use crate::outcome::Outcome;
use crate::waits::Waits;

/// Every ring of the wait relation reachable from `from`: a strongly
/// connected set of nodes with a wait inside it, a node awaiting itself
/// being a ring of one. Found with an explicit stack, never by recursion,
/// and returned in key order.
pub(crate) fn rings(
    waits: &Waits,
    from: impl IntoIterator<Item = NodeKey>,
) -> Vec<BTreeSet<NodeKey>> {
    let mut walk = Walk::default();
    for start in from {
        if !walk.order.contains_key(&start) {
            walk.from(waits, start);
        }
    }
    walk.rings.sort();
    walk.rings
}

/// The final answer of a node on a ring. No body produced it, so it has
/// no declaration.
pub(crate) fn entry() -> Entry {
    Entry {
        content: Content::of_failure(&Failure::Cycle),
        outcome: Outcome::Failure(Failure::Cycle),
        declaration: None,
    }
}

/// Tarjan's walk for strongly connected sets.
#[derive(Default)]
struct Walk {
    /// The order each node was first reached in.
    order: BTreeMap<NodeKey, usize>,
    /// The earliest order reachable from each node through the nodes not
    /// yet placed in a set.
    low: BTreeMap<NodeKey, usize>,
    /// The nodes reached and not yet placed in a set, latest last.
    unplaced: Vec<NodeKey>,
    placed: BTreeSet<NodeKey>,
    rings: Vec<BTreeSet<NodeKey>>,
}

impl Walk {
    /// Walks every node reachable from `start` that is not reached yet.
    fn from(&mut self, waits: &Waits, start: NodeKey) {
        self.reach(start);
        let mut frames = vec![(start, waits.awaits(start))];
        while let Some(frame) = frames.last_mut() {
            let (node, next) = (frame.0, frame.1.next());
            match next {
                Some(awaited) if !self.order.contains_key(&awaited) => {
                    self.reach(awaited);
                    frames.push((awaited, waits.awaits(awaited)));
                }
                Some(awaited) => {
                    if !self.placed.contains(&awaited) {
                        self.lower(node, self.order[&awaited]);
                    }
                }
                None => {
                    frames.pop();
                    if let Some((parent, _)) = frames.last() {
                        self.lower(*parent, self.low[&node]);
                    }
                    if self.low[&node] == self.order[&node] {
                        self.place(waits, node);
                    }
                }
            }
        }
    }

    fn reach(&mut self, node: NodeKey) {
        let order = self.order.len();
        self.order.insert(node, order);
        self.low.insert(node, order);
        self.unplaced.push(node);
    }

    fn lower(&mut self, node: NodeKey, to: usize) {
        let low = self
            .low
            .get_mut(&node)
            .expect("a node is reached before it is lowered");
        *low = (*low).min(to);
    }

    /// Places the set whose first-reached node is `first`, keeping it if it
    /// is a ring.
    fn place(&mut self, waits: &Waits, first: NodeKey) {
        let at = self
            .unplaced
            .iter()
            .rposition(|node| *node == first)
            .expect("a set's first node is unplaced until the set is placed");
        let set: BTreeSet<_> = self.unplaced.drain(at..).collect();
        self.placed.extend(&set);
        if set.len() > 1 || waits.awaits(first).any(|awaited| awaited == first) {
            self.rings.push(set);
        }
    }
}
