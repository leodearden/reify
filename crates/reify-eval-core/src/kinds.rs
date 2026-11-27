//! The kinds a core is built with, by id.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;

use crate::erased_kind::{ErasedKind, Of};
use crate::kind::Kind;
use crate::kind_id::KindId;

/// Two kinds were registered under one id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DuplicateKind(pub KindId);

#[derive(Default)]
pub(crate) struct Kinds {
    by_id: BTreeMap<KindId, Box<dyn ErasedKind>>,
}

impl Kinds {
    pub fn add<K: Kind>(&mut self) -> Result<(), DuplicateKind> {
        let Entry::Vacant(slot) = self.by_id.entry(K::ID) else {
            return Err(DuplicateKind(K::ID));
        };
        slot.insert(Box::new(Of::<K>::new()));
        Ok(())
    }

    pub fn get(&self, id: KindId) -> Option<&dyn ErasedKind> {
        self.by_id.get(&id).map(|kind| &**kind)
    }
}
