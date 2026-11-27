//! A snapshot's limit settings (contract 7.6), read from its inputs once,
//! when its index opens. With no setting nothing is bounded: the default a
//! host applies is the host's, never the core's (rule 7).

use imbl::OrdMap;

use crate::input::{Input, InputKey, Limit};
use crate::outcome::Outcome;

pub(crate) struct Limits {
    depth: Option<u64>,
    children_per_node: Option<u64>,
}

impl Limits {
    /// # Panics
    ///
    /// If a limit setting among `inputs` is not a u64: the host parses its
    /// settings into inputs, so another type is the host's fault.
    pub fn of(inputs: &OrdMap<InputKey, Input>) -> Self {
        Self {
            depth: setting(inputs, Limit::Depth),
            children_per_node: setting(inputs, Limit::ChildrenPerNode),
        }
    }

    /// How many steps below its root a node may be.
    pub fn depth(&self) -> Option<u64> {
        self.depth
    }

    /// How many children a node's value may declare.
    pub fn children_per_node(&self) -> Option<u64> {
        self.children_per_node
    }
}

fn setting(inputs: &OrdMap<InputKey, Input>, limit: Limit) -> Option<u64> {
    let setting = inputs.get(&InputKey::Limit(limit))?;
    match setting.outcome().downcast::<u64>() {
        Outcome::Value(bound) => Some(*bound),
        refused => panic!("the limit setting {limit:?} must be a u64: {refused:?}"),
    }
}
