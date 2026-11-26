//! What a node is: the kind that computes it and what its bindings name.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::hash::{Encoder, Hash128};
use crate::input::InputKey;
use crate::kind_id::KindId;
use crate::reference::Ref;
use crate::step::Step;

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

    /// What the declaration says, hashed: its kind and what each binding
    /// names, whatever order they were bound in.
    pub fn content(&self) -> Hash128 {
        let mut encoder = Encoder::new("declaration");
        self.encode(&mut encoder);
        encoder.finish()
    }

    pub(crate) fn encode(&self, encoder: &mut Encoder) {
        encoder
            .text(self.kind.0)
            .sequence(self.bindings.iter(), |encoder, (name, binding)| {
                encoder.text(name);
                binding.encode(encoder);
            });
    }
}

impl Binding {
    pub(crate) fn encode(&self, encoder: &mut Encoder) {
        match self {
            Binding::Input(key) => {
                encoder.tag("binding.input");
                key.encode(encoder);
            }
            Binding::Declarer(steps) => {
                encoder
                    .tag("binding.declarer")
                    .sequence(steps.iter(), |encoder, step| step.encode(encoder));
            }
            Binding::Passed(passed) => {
                encoder.tag("binding.passed");
                passed.encode(encoder);
            }
        }
    }
}
