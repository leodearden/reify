//! A snapshot input (contract 3.1): a value under a key. A root's input is
//! its declaration (decision D11); every other input is named by the host.
//! A constant a declaration binds (contract 2.1) is a value of the same
//! form, held in the declaration itself.

use std::any::Any;
use std::sync::Arc;

use crate::hash::{Encoder, Hash128};
use crate::outcome::{Erased, Outcome, Value, tagged_content};
use crate::path::Root;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InputKey {
    Root(Root),
    Named(Arc<str>),
    /// A snapshot's setting of `Limit`, whose value is a u64.
    Limit(Limit),
}

/// A limit that changes answers, set in a snapshot (contract 7.6) under
/// `InputKey::Limit`; its value is a u64, and with no setting nothing is
/// bounded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Limit {
    /// How many steps below its root a node may be.
    Depth,
    /// How many children a node's value may declare.
    ChildrenPerNode,
}

impl Limit {
    fn name(self) -> &'static str {
        match self {
            Limit::Depth => "depth",
            Limit::ChildrenPerNode => "children_per_node",
        }
    }
}

impl InputKey {
    pub fn root(name: impl Into<Arc<str>>) -> Self {
        InputKey::Root(Root::named(name))
    }

    pub fn named(name: impl Into<Arc<str>>) -> Self {
        InputKey::Named(name.into())
    }

    pub fn limit(limit: Limit) -> Self {
        InputKey::Limit(limit)
    }

    pub(crate) fn encode(&self, encoder: &mut Encoder) {
        match self {
            InputKey::Root(root) => {
                encoder.tag("input.root");
                root.encode(encoder);
            }
            InputKey::Named(name) => {
                encoder.tag("input.named").text(name);
            }
            InputKey::Limit(limit) => {
                encoder.tag("input.limit").text(limit.name());
            }
        }
    }
}

/// A snapshot input's value, or a constant's, with the content its host
/// gives it (PRD D4): a hash of everything a reader can see of the value,
/// as a kind's `content` is. The core tags it with the value's type, so two
/// inputs of one type with equal content are the same input.
#[derive(Debug, Clone)]
pub struct Input {
    value: Arc<Erased>,
    content: Hash128,
}

impl Input {
    pub fn of<T: Value>(value: T, content: Hash128) -> Self {
        Self {
            value: Arc::new(value),
            content,
        }
    }

    pub(crate) fn outcome(&self) -> Outcome<Erased> {
        Outcome::Value(Arc::clone(&self.value))
    }

    /// The content the host gave, tagged with the value's type.
    pub(crate) fn tagged_content(&self) -> Hash128 {
        tagged_content(&*self.value, self.content)
    }
}

/// Two inputs are equal where their content is: of one type, with equal
/// host-given content.
impl PartialEq for Input {
    fn eq(&self, other: &Self) -> bool {
        let (value, other_value): (&dyn Any, &dyn Any) = (&*self.value, &*other.value);
        value.type_id() == other_value.type_id() && self.content == other.content
    }
}

impl Eq for Input {}
