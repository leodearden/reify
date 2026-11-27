//! What a node's kind contributes to every key of its results (contract
//! 2.1, 5.3): the kind and the version of its body. No path, binding or
//! name enters it.

use crate::hash::{Encoder, Hash128};
use crate::kind_id::KindId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Recipe {
    pub kind: KindId,
    pub version: u32,
}

impl Recipe {
    pub fn hash(self) -> Hash128 {
        Encoder::new("recipe")
            .text(self.kind.0)
            .number(u64::from(self.version))
            .finish()
    }
}
