//! A snapshot input (contract 3.1): a value under a key. A root's input is
//! its declaration (decision D11); an attached input belongs to a path
//! (contract 2.1); every other input is named by the host.

use std::sync::Arc;

use crate::outcome::{Erased, Outcome, Value};
use crate::path::{Path, Root};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InputKey {
    Root(Root),
    Named(Arc<str>),
    /// The input `name` attached to the node at `path`.
    Attached {
        path: Path,
        name: Arc<str>,
    },
}

impl InputKey {
    pub fn root(name: impl Into<Arc<str>>) -> Self {
        InputKey::Root(Root::named(name))
    }

    pub fn named(name: impl Into<Arc<str>>) -> Self {
        InputKey::Named(name.into())
    }

    pub fn attached(path: Path, name: impl Into<Arc<str>>) -> Self {
        InputKey::Attached {
            path,
            name: name.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Input(Arc<Erased>);

impl Input {
    pub fn of<T: Value>(value: T) -> Self {
        Self(Arc::new(value))
    }

    pub(crate) fn outcome(&self) -> Outcome<Erased> {
        Outcome::Value(Arc::clone(&self.0))
    }
}
