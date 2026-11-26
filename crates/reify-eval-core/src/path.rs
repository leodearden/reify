//! Where a node sits (contract 2.1): a root, then steps down.

use std::sync::Arc;

use crate::hash::Encoder;
use crate::step::Step;

/// A root, named by the session.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Root(Arc<str>);

impl Root {
    pub fn named(name: impl Into<Arc<str>>) -> Self {
        Self(name.into())
    }

    pub(crate) fn encode(&self, encoder: &mut Encoder) {
        encoder.tag("root").text(&self.0);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Path {
    root: Root,
    steps: Vec<Step>,
}

impl Path {
    pub fn root(root: Root) -> Self {
        Self {
            root,
            steps: Vec::new(),
        }
    }

    /// The path one `step` below this one.
    pub fn child(&self, step: Step) -> Path {
        let mut steps = self.steps.clone();
        steps.push(step);
        Self {
            root: self.root.clone(),
            steps,
        }
    }

    pub(crate) fn from_parts(root: Root, steps: Vec<Step>) -> Self {
        Self { root, steps }
    }

    pub(crate) fn root_name(&self) -> &Root {
        &self.root
    }

    pub(crate) fn steps(&self) -> &[Step] {
        &self.steps
    }

    pub(crate) fn encode(&self, encoder: &mut Encoder) {
        encoder.tag("path");
        self.root.encode(encoder);
        encoder.sequence(self.steps.iter(), |encoder, step| step.encode(encoder));
    }
}
