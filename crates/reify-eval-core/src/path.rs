//! Where a node sits (contract 2.1). A path is a root for now; the steps
//! below a root come with structure by declaration.

use std::sync::Arc;

/// A root, named by the session.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Root(Arc<str>);

impl Root {
    pub fn named(name: impl Into<Arc<str>>) -> Self {
        Self(name.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Path {
    root: Root,
}

impl Path {
    pub fn root(root: Root) -> Self {
        Self { root }
    }
}
