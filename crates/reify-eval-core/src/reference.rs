//! What a body reads: one of its declaration's bindings, by name, then
//! steps down from the place it names (contract 2.1).

use std::sync::Arc;

use crate::hash::Encoder;
use crate::step::Step;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ref {
    binding: Arc<str>,
    steps: Vec<Step>,
}

impl Ref {
    pub fn binding(name: impl Into<Arc<str>>) -> Self {
        Self {
            binding: name.into(),
            steps: Vec::new(),
        }
    }

    /// The same ref, one `step` further down.
    pub fn step(mut self, step: Step) -> Self {
        self.steps.push(step);
        self
    }

    pub(crate) fn binding_name(&self) -> &str {
        &self.binding
    }

    pub(crate) fn steps(&self) -> &[Step] {
        &self.steps
    }

    pub(crate) fn encode(&self, encoder: &mut Encoder) {
        encoder
            .tag("ref")
            .text(&self.binding)
            .sequence(self.steps.iter(), |encoder, step| step.encode(encoder));
    }
}
