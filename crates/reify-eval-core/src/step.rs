//! One step down from a node's place (contract 2.1): a name, or an index.

use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Step {
    Named(Arc<str>),
    Index(u64),
}

impl Step {
    pub fn named(name: impl Into<Arc<str>>) -> Self {
        Step::Named(name.into())
    }

    pub fn index(index: u64) -> Self {
        Step::Index(index)
    }
}
