//! What a core is built with.

use crate::executor::Executor;
use crate::host::Core;
use crate::kind::Kind;
use crate::kinds::{DuplicateKind, Kinds};

#[derive(Default)]
pub struct Builder {
    kinds: Kinds,
}

impl Builder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn kind<K: Kind>(mut self) -> Result<Self, DuplicateKind> {
        self.kinds.add::<K>()?;
        Ok(self)
    }

    pub fn build(self) -> Core {
        Core::new(Executor::new(self.kinds))
    }
}
