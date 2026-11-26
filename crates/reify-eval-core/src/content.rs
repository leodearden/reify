//! The content of an outcome (contract 5.2): what a reader sees of it,
//! hashed. Every content the core uses is computed here.

use crate::erased_kind::ErasedKind;
use crate::failure::Failure;
use crate::hash::{Encoder, Hash128};
use crate::input::Input;
use crate::outcome::{Erased, Outcome, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Content(Hash128);

impl Content {
    /// A snapshot input: its host-given content, by the rule for a value
    /// hashed by value.
    pub fn of_input(input: &Input) -> Self {
        Self::of_value(input.type_name(), input.leaf_content())
    }

    /// What a body of `kind` produced, whose chain of reads ended at the
    /// key hashed as `key`. A value the kind does not hash by value has
    /// the content of that key: every result is treated as deterministic
    /// until leaf ο.
    pub fn of_produced(outcome: &Outcome<Erased>, kind: &dyn ErasedKind, key: Hash128) -> Self {
        match outcome {
            Outcome::Value(value) => match kind.content(&**value) {
                Some(leaf) => Self::of_value(<Erased as Value>::type_name(&**value), leaf),
                None => Self(Encoder::new("content.keyed").hash(key).finish()),
            },
            settled => Self::of_settled(settled),
        }
    }

    /// An outcome no body produced: a failure, or absent.
    ///
    /// # Panics
    ///
    /// If `outcome` is a value: only a body or a snapshot input has one.
    pub fn of_settled<T: ?Sized>(outcome: &Outcome<T>) -> Self {
        let encoder = match outcome {
            Outcome::Failure(failure) => Self::failure_encoder(failure),
            Outcome::Absent => Encoder::new("content.absent"),
            Outcome::Value(_) => unreachable!("a value's content comes from its kind or its host"),
        };
        Self(encoder.finish())
    }

    pub fn hash(self) -> Hash128 {
        self.0
    }

    /// A value's leaf content, tagged with the name of its type, so values
    /// of two types never share a content.
    fn of_value(type_name: &str, leaf: Hash128) -> Self {
        Self(
            Encoder::new("content.value")
                .text(type_name)
                .hash(leaf)
                .finish(),
        )
    }

    fn failure_encoder(failure: &Failure) -> Encoder {
        let mut encoder = Encoder::new("content.failure");
        failure.encode(&mut encoder);
        encoder
    }
}
