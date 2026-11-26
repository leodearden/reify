//! Where a result is filed (contract 5.1): the recipe's key, then one step
//! per read request, over its refs and the contents they read, in order.
//! A request of several refs is one step.

use std::sync::Arc;

use crate::content::Content;
use crate::hash::{Encoder, Hash128};
use crate::recipe::Recipe;
use crate::reference::Ref;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Key(Hash128);

impl Key {
    pub fn of_recipe(recipe: Recipe) -> Self {
        Self(recipe.hash())
    }

    /// The key one read request further on: `refs` read `contents`.
    pub fn then(self, refs: &[Ref], contents: &[Content]) -> Self {
        let mut encoder = Encoder::new("key.read");
        encoder
            .hash(self.0)
            .sequence(refs.iter().zip(contents), |encoder, (r, content)| {
                r.encode(encoder);
                encoder.hash(content.hash());
            });
        Self(encoder.finish())
    }

    pub fn hash(self) -> Hash128 {
        self.0
    }
}

/// A key a running body passed through, and the read request it made
/// there.
pub(crate) struct Passed {
    pub key: Key,
    pub refs: Arc<[Ref]>,
}

/// The keys a running body has passed through, and the key it stands at
/// now.
pub(crate) struct Chain {
    passed: Vec<Passed>,
    key: Key,
}

impl Chain {
    pub fn new(recipe: Recipe) -> Self {
        Self {
            passed: Vec::new(),
            key: Key::of_recipe(recipe),
        }
    }

    /// Records that the body read `contents` at `refs` where it stands.
    pub fn read(&mut self, refs: Arc<[Ref]>, contents: &[Content]) {
        let next = self.key.then(&refs, contents);
        self.passed.push(Passed {
            key: self.key,
            refs,
        });
        self.key = next;
    }

    pub fn key(&self) -> Key {
        self.key
    }

    /// Each key passed through with its request, in order, and the key
    /// the chain ends at.
    pub fn into_parts(self) -> (Vec<Passed>, Key) {
        (self.passed, self.key)
    }
}
