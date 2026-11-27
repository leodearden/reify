//! Where a result is filed (contract 5.1): the recipe's key, then one step
//! per read request, over its items and the contents they read, in order.
//! A request of several items is one step; a child item enters with its
//! step and declaration.

use std::sync::Arc;

use crate::content::Content;
use crate::hash::{Encoder, Hash128};
use crate::recipe::Recipe;
use crate::substitution::Substitution;
use crate::wanted::Wanted;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Key(Hash128);

impl Key {
    pub fn of_recipe(recipe: Recipe) -> Self {
        Self(recipe.hash())
    }

    /// The key one read request further on: `wanted` read `contents`.
    pub fn then(self, wanted: &[Wanted], contents: &[Content]) -> Self {
        let mut encoder = Encoder::new("key.read");
        encoder.hash(self.0);
        read(&mut encoder, wanted, contents);
        Self(encoder.finish())
    }

    /// The key one read request through a view further on: under
    /// `substitution`, each ref with the content of the outcome supposed
    /// there, `wanted` read `contents`.
    pub fn through_view(
        self,
        substitution: &Substitution,
        wanted: &[Wanted],
        contents: &[Content],
    ) -> Self {
        let mut encoder = Encoder::new("key.read_through_view");
        encoder
            .hash(self.0)
            .sequence(substitution.items(), |encoder, (r, supposed)| {
                r.encode(encoder);
                encoder.hash(Content::of_supposed(supposed).hash());
            });
        read(&mut encoder, wanted, contents);
        Self(encoder.finish())
    }

    pub fn hash(self) -> Hash128 {
        self.0
    }
}

/// Hashes the items of a read request, each with the content it read.
fn read(encoder: &mut Encoder, wanted: &[Wanted], contents: &[Content]) {
    encoder.sequence(wanted.iter().zip(contents), |encoder, (item, content)| {
        item.encode(encoder);
        encoder.hash(content.hash());
    });
}

/// A key a running body passed through, and the read request it made
/// there.
pub(crate) struct Passed {
    pub key: Key,
    pub wanted: Arc<[Wanted]>,
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

    /// Records that the body read `contents` for `wanted` where it stands.
    pub fn read(&mut self, wanted: Arc<[Wanted]>, contents: &[Content]) {
        let next = self.key.then(&wanted, contents);
        self.passed.push(Passed {
            key: self.key,
            wanted,
        });
        self.key = next;
    }

    /// Records that the body read `contents` for `wanted` through a view
    /// supposing `substitution`. No lookup follows such a step, so it is
    /// not among the keys passed through: the result of a run that read
    /// through a view is never filed (`finish::filed`).
    pub fn read_through_view(
        &mut self,
        substitution: &Substitution,
        wanted: &[Wanted],
        contents: &[Content],
    ) {
        self.key = self.key.through_view(substitution, wanted, contents);
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
