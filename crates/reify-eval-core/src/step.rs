//! One step down from a node's place (contract 2.1): a key, a name or an
//! index, in a space.

use std::sync::Arc;

use crate::hash::Encoder;

/// Which of a node's namespaces a step's key is in. A named space is a
/// static name chosen by the kind that declares the child; the core
/// compares it and never interprets it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Space {
    Default,
    Named(&'static str),
}

/// What a step picks out within its space.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StepKey {
    Name(Arc<str>),
    Index(u64),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Step {
    space: Space,
    key: StepKey,
}

impl Step {
    pub fn new(space: Space, key: StepKey) -> Self {
        Self { space, key }
    }

    /// A name in the default space.
    pub fn named(name: impl Into<Arc<str>>) -> Self {
        Self::new(Space::Default, StepKey::Name(name.into()))
    }

    /// An index in the default space.
    pub fn index(index: u64) -> Self {
        Self::new(Space::Default, StepKey::Index(index))
    }

    pub fn space(&self) -> Space {
        self.space
    }

    pub fn key(&self) -> &StepKey {
        &self.key
    }

    pub(crate) fn encode(&self, encoder: &mut Encoder) {
        match self.space {
            Space::Default => encoder.tag("space.default"),
            Space::Named(name) => encoder.tag("space.named").text(name),
        };
        match &self.key {
            StepKey::Name(name) => encoder.tag("step.named").text(name),
            StepKey::Index(index) => encoder.tag("step.index").number(*index),
        };
    }
}
