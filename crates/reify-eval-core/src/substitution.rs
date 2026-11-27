//! What a view supposes (contract 6): an outcome at each place a ref
//! names, relative to the body that opens the view. A supposed value
//! carries its own content, as a snapshot input's does.

use std::sync::Arc;

use crate::input::Input;
use crate::outcome::Outcome;
use crate::reference::Ref;

#[derive(Debug, Clone)]
pub struct Substitution {
    items: Arc<[(Ref, Outcome<Input>)]>,
}

impl Substitution {
    /// Supposes each outcome at the place its ref names, in order.
    pub fn of(items: impl IntoIterator<Item = (Ref, Outcome<Input>)>) -> Self {
        Self {
            items: items.into_iter().collect(),
        }
    }

    pub(crate) fn items(&self) -> impl Iterator<Item = &(Ref, Outcome<Input>)> {
        self.items.iter()
    }
}
