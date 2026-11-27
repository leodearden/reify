//! A kind's own account of a failure (contract 9), of a type the kind
//! chooses, with the content the kind gives it (contract 5.2). It may hold
//! refs and holds no path, as a result holds none (contract 5.4).

use std::any::{Any, TypeId, type_name};
use std::fmt;
use std::sync::Arc;

use crate::hash::{Encoder, Hash128};

#[derive(Clone)]
pub struct Description {
    value: Arc<dyn Any + Send + Sync>,
    type_id: TypeId,
    type_name: &'static str,
    content: Hash128,
}

impl Description {
    /// `value`, with the content its kind gives it: a hash of everything a
    /// reader can see of the value, as a kind's `content` is. The core tags
    /// it with the value's type, so two descriptions of one type with equal
    /// content are the same description.
    pub fn of<T: Any + Send + Sync>(value: T, content: Hash128) -> Self {
        Self {
            value: Arc::new(value),
            type_id: TypeId::of::<T>(),
            type_name: type_name::<T>(),
            content,
        }
    }

    /// The value, where it is a `T`; none where it is of another type.
    pub fn read<T: Any>(&self) -> Option<&T> {
        (*self.value).downcast_ref::<T>()
    }

    pub(crate) fn encode(&self, encoder: &mut Encoder) {
        encoder
            .tag("description")
            .type_of(self.type_id, self.type_name)
            .hash(self.content);
    }
}

/// Two descriptions are equal where their content is: of one type, with
/// equal kind-given content.
impl PartialEq for Description {
    fn eq(&self, other: &Self) -> bool {
        self.type_id == other.type_id && self.content == other.content
    }
}

impl Eq for Description {}

impl fmt::Debug for Description {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Description")
            .field("type", &self.type_name)
            .field("content", &self.content)
            .finish()
    }
}
