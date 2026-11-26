//! The kinds a core is built with, by id.

use std::collections::BTreeMap;

use crate::declaration::KindId;
use crate::erased_kind::{ErasedKind, Of};
use crate::kind::Kind;

/// Two kinds were registered under one id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DuplicateKind(pub KindId);

#[derive(Default)]
pub(crate) struct Kinds {
    by_id: BTreeMap<KindId, Box<dyn ErasedKind>>,
}

impl Kinds {
    pub fn add<K: Kind>(&mut self) -> Result<(), DuplicateKind> {
        match self.by_id.insert(K::ID, Box::new(Of::<K>::new())) {
            None => Ok(()),
            Some(_) => Err(DuplicateKind(K::ID)),
        }
    }

    pub fn get(&self, id: KindId) -> Option<&dyn ErasedKind> {
        self.by_id.get(&id).map(|kind| &**kind)
    }
}
