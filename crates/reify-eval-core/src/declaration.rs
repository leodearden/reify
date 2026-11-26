//! What a node is: the kind that computes it and what its bindings name.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::input::InputKey;
use crate::reference::Ref;
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
    /// The snapshot input of that name attached to the bound node's own
    /// path.
    Attached(Arc<str>),
    /// The bound node's declarer's own place, then `steps` down.
    Declarer(Vec<Step>),
    /// What the ref names when read by the bound node's declarer: one of
    /// the declarer's own bindings, then steps down (contract 2.1).
    Passed(Ref),
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
