//! The content of an outcome (contract 5.2): what a reader sees of it,
//! hashed. Every content the core uses is computed here.

use crate::erased_kind::ErasedKind;
use crate::failure::Failure;
use crate::hash::{Encoder, Hash128};
use crate::input::Input;
use crate::outcome::{Erased, Outcome, tagged_content};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Content(Hash128);

impl Content {
    /// A snapshot input or a constant: its host-given content, by the rule
    /// for a value hashed by value.
    pub fn of_input(input: &Input) -> Self {
        Self(input.tagged_content())
    }

    /// What a body of `kind` produced, whose chain of reads ended at the
    /// key hashed as `key`. A value the kind does not hash by value has
    /// the content of that key: every result is treated as deterministic
    /// until leaf ο.
    pub fn of_produced(outcome: &Outcome<Erased>, kind: &dyn ErasedKind, key: Hash128) -> Self {
        match outcome {
            Outcome::Value(value) => match kind.content(&**value) {
                Some(leaf) => Self(tagged_content(&**value, leaf)),
                None => Self(Encoder::new("content.keyed").hash(key).finish()),
            },
            Outcome::Failure(failure) => Self::of_failure(failure),
            Outcome::Absent => Self::absent(),
        }
    }

    /// An outcome a view supposes, by the rule for the outcome it is: a
    /// value has its given content, as a snapshot input's.
    pub fn of_supposed(supposed: &Outcome<Input>) -> Self {
        match supposed {
            Outcome::Value(input) => Self::of_input(input),
            Outcome::Failure(failure) => Self::of_failure(failure),
            Outcome::Absent => Self::absent(),
        }
    }

    pub fn of_failure(failure: &Failure) -> Self {
        let mut encoder = Encoder::new("content.failure");
        failure.encode(&mut encoder);
        Self(encoder.finish())
    }

    pub fn absent() -> Self {
        Self(Encoder::new("content.absent").finish())
    }

    pub fn hash(self) -> Hash128 {
        self.0
    }
}
