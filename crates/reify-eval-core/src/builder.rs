//! What a core is built with.

use crate::executor::Executor;
use crate::host::Core;

#[derive(Default)]
pub struct Builder {}

impl Builder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn build(self) -> Core {
        Core::new(Executor::default())
    }
}
