//! Rings of waits, and how they are broken (contract 7.7). A ring is a set
//! of nodes in progress each of which needs, through what it awaits, its
//! own answer. Rings are looked for only at rest, when every node in
//! progress awaits another, so they do not depend on the order of work.

use std::collections::{BTreeMap, BTreeSet};

use crate::content::Content;
use crate::failure::Failure;
use crate::index::{Entry, NodeKey};
use crate::outcome::Outcome;
use crate::waits::Waits;

/// How the rings at rest are broken: the lookups that miss, and the nodes
/// that answer "cycle".
pub(crate) struct Broken {
    /// Every lookup of each ring that holds one. Each misses and its body
    /// runs; nothing else on its ring is answered yet.
    pub missed: Vec<NodeKey>,
    /// Each ring that holds no lookup, a ring of running bodies and of
    /// paths waiting on their parent, every node of which answers.
    pub answered: Vec<BTreeSet<NodeKey>>,
}

impl Broken {
    pub fn is_empty(&self) -> bool {
        self.missed.is_empty() && self.answered.is_empty()
    }
}

/// How the rings reachable from `from` are broken, `is_lookup` telling
/// which nodes are lookups. A chain of waits through a lookup is not a
/// cycle: the lookup misses. A ring holding one is answered in no part,
/// since the bodies of its lookups may join it, as a fresh evaluation,
/// which has no lookup, finds the whole set as one ring.
pub(crate) fn broken(
    waits: &Waits,
    from: impl IntoIterator<Item = NodeKey>,
    is_lookup: impl Fn(&NodeKey) -> bool,
) -> Broken {
    let mut broken = Broken {
        missed: Vec::new(),
        answered: Vec::new(),
    };
    for ring in rings(waits, from) {
        let lookups: Vec<_> = ring.iter().copied().filter(&is_lookup).collect();
        if lookups.is_empty() {
            broken.answered.push(ring);
        } else {
            broken.missed.extend(lookups);
        }
    }
    broken
}

/// Every ring of the wait relation reachable from `from`: a strongly
/// connected set of nodes with a wait inside it, a node awaiting itself
/// being a ring of one. Found with an explicit stack, never by recursion,
/// and returned in key order.
fn rings(waits: &Waits, from: impl IntoIterator<Item = NodeKey>) -> Vec<BTreeSet<NodeKey>> {
    let mut walk = Walk::default();
    for start in from {
        if !walk.order.contains_key(&start) {
            walk.visit(waits, start);
        }
    }
    walk.rings.sort();
    walk.rings
}

/// The final answer of a node on a ring, which `depended` on a
/// substitution or not. No body produced it, so it has no declaration.
pub(crate) fn entry(depended: bool) -> Entry {
    Entry::settled(
        Outcome::Failure(Failure::Cycle),
        Content::of_failure(&Failure::Cycle),
        depended,
    )
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
    fn visit(&mut self, waits: &Waits, start: NodeKey) {
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
