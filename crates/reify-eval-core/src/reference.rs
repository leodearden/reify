//! What a body reads: one of its declaration's bindings, by name. Steps
//! down from a binding come with structure by declaration.

use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ref {
    binding: Arc<str>,
}

impl Ref {
    pub fn binding(name: impl Into<Arc<str>>) -> Self {
        Self {
            binding: name.into(),
        }
    }

    pub(crate) fn binding_name(&self) -> &str {
        &self.binding
    }
}
