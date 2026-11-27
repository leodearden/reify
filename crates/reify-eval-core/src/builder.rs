//! What a core is built with.

use crate::executor::Executor;
use crate::host::Core;
use crate::kind::Kind;
use crate::kinds::{DuplicateKind, Kinds};
use crate::store::Store;

#[derive(Default)]
pub struct Builder {
    kinds: Kinds,
    store: Store,
}

impl Builder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn kind<K: Kind>(mut self) -> Result<Self, DuplicateKind> {
        self.kinds.add::<K>()?;
        Ok(self)
    }

    /// The store the core starts with, holding results another core
    /// filed; empty unless given. Nothing is seeded into a core after it is
    /// built.
    pub fn on_store(mut self, store: Store) -> Self {
        self.store = store;
        self
    }

    pub fn build(self) -> Core {
        Core::new(Executor::new(self.kinds, self.store))
    }
}
