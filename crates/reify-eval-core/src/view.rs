//! A view a body opened (contract 6): the state of its snapshot with the
//! substitution's outcomes supposed. Each read through it is a request to
//! the core, answered in that state, and tells whether its answer depended
//! on the substitution. Dropping the handle closes the view.

use std::sync::Arc;

pub use crate::mailbox::Viewed;
use crate::mailbox::{Ask, Mailbox, Reply};
use crate::outcome::Value;
use crate::reference::Ref;
use crate::substitution::Substitution;
use crate::wanted::Wanted;

pub struct View<'a> {
    mailbox: &'a Mailbox,
    /// Which of its run's views it is, in the order they were opened.
    ordinal: u32,
    substitution: Substitution,
}

impl Drop for View<'_> {
    /// Notes that nothing reads through the view any more, so that the core
    /// closes it when the body is next suspended or ends.
    fn drop(&mut self) {
        self.mailbox.drop_view(self.ordinal);
    }
}

impl<'a> View<'a> {
    pub(crate) fn new(mailbox: &'a Mailbox, ordinal: u32, substitution: Substitution) -> Self {
        Self {
            mailbox,
            ordinal,
            substitution,
        }
    }

    /// The outcome at `r` under the view, typed as `T` as `Cx::read` types
    /// one; `r` is relative to the body that opened the view.
    pub async fn read<T: Value>(&self, r: &Ref) -> Viewed<T> {
        let ask = Ask::ViewRead {
            ordinal: self.ordinal,
            substitution: self.substitution.clone(),
            wanted: Arc::new([Wanted::Ref(r.clone())]),
        };
        match self.mailbox.exchange(ask).await {
            Reply::Viewed(mut viewed) => {
                let Viewed { outcome, depended } =
                    viewed.pop().expect("a read gives an outcome per item");
                Viewed {
                    outcome: outcome.downcast(),
                    depended,
                }
            }
            Reply::Outcomes(_) | Reply::Product(_) => {
                unreachable!("the core replies to a read through a view with what it viewed")
            }
        }
    }
}
