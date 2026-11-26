//! What a node is: the kind that computes it and what its bindings name.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::input::InputKey;
use crate::step::Step;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KindId(pub &'static str);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declaration {
    kind: KindId,
    bindings: BTreeMap<Arc<str>, Binding>,
}

/// What a binding of a declaration names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Binding {
    Input(InputKey),
    /// The bound node's declarer's own place, then `steps` down.
    Declarer(Vec<Step>),
}

impl Declaration {
    pub fn new(kind: KindId) -> Self {
        Self {
            kind,
            bindings: BTreeMap::new(),
        }
    }

    pub fn bind(mut self, name: impl Into<Arc<str>>, binding: Binding) -> Self {
        self.bindings.insert(name.into(), binding);
        self
    }

    pub fn kind(&self) -> KindId {
        self.kind
    }

    pub(crate) fn binding(&self, name: &str) -> Option<&Binding> {
        self.bindings.get(name)
    }
}
