//! What a node's evaluation comes to: a value, a failure, or absent. The
//! core stores values erased; each read names the type it expects.

use std::any::{Any, type_name};
use std::fmt;
use std::sync::Arc;

use crate::failure::Failure;
use crate::hash::{Encoder, Hash128};

/// Anything a node can produce or a snapshot can hold.
pub trait Value: Any + Send + Sync {
    fn type_name(&self) -> &'static str;
}

impl<T: Any + Send + Sync> Value for T {
    fn type_name(&self) -> &'static str {
        type_name::<T>()
    }
}

pub type Erased = dyn Value;

/// `value`'s leaf content tagged with its type, so values of two types
/// never share a content: the content of a value hashed by value, whether
/// a read records it or a declaration holds it as a constant.
pub(crate) fn tagged_content(value: &Erased, leaf: Hash128) -> Hash128 {
    let any: &dyn Any = value;
    Encoder::new("content.value")
        .type_of(any.type_id(), <Erased as Value>::type_name(value))
        .hash(leaf)
        .finish()
}

impl fmt::Debug for Erased {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<{}>", <Erased as Value>::type_name(self))
    }
}

#[derive(Debug, PartialEq)]
pub enum Outcome<T: ?Sized> {
    Value(Arc<T>),
    Failure(Failure),
    Absent,
}

impl<T: ?Sized> Clone for Outcome<T> {
    fn clone(&self) -> Self {
        match self {
            Outcome::Value(value) => Outcome::Value(Arc::clone(value)),
            Outcome::Failure(failure) => Outcome::Failure(failure.clone()),
            Outcome::Absent => Outcome::Absent,
        }
    }
}

impl<T: Value> Outcome<T> {
    pub fn of(value: T) -> Self {
        Outcome::Value(Arc::new(value))
    }

    pub fn erase(self) -> Outcome<Erased> {
        match self {
            Outcome::Value(value) => Outcome::Value(value),
            Outcome::Failure(failure) => Outcome::Failure(failure),
            Outcome::Absent => Outcome::Absent,
        }
    }
}

impl Outcome<Erased> {
    /// The same outcome typed as `T`; a value of another type becomes a
    /// failure naming both types.
    pub fn downcast<T: Value>(self) -> Outcome<T> {
        match self {
            Outcome::Value(value) => {
                let found = <Erased as Value>::type_name(&*value);
                let any: Arc<dyn Any + Send + Sync> = value;
                match any.downcast::<T>() {
                    Ok(value) => Outcome::Value(value),
                    Err(_) => Outcome::Failure(Failure::WrongType {
                        expected: type_name::<T>(),
                        found,
                    }),
                }
            }
            Outcome::Failure(failure) => Outcome::Failure(failure),
            Outcome::Absent => Outcome::Absent,
        }
    }
}
