//! What a running body holds: its own mailbox and cancel flag, and nothing
//! of the core's.

use std::any::{TypeId, type_name};
use std::sync::Arc;

use crate::cancel::Cancel;
use crate::mailbox::{Ask, Mailbox, Reply, SectionAsk, SectionJob};
use crate::outcome::{Outcome, Value};
use crate::reference::Ref;
use crate::section::Section;

pub struct Cx {
    mailbox: Arc<Mailbox>,
    cancel: Cancel,
}

impl Cx {
    pub(crate) fn new(mailbox: Arc<Mailbox>, cancel: Cancel) -> Self {
        Self { mailbox, cancel }
    }

    /// The outcome at `r`, typed as `T`: a value of another type is a
    /// failure naming both types.
    pub async fn read<T: Value>(&self, r: &Ref) -> Outcome<T> {
        let mut outcomes = self.read_all(std::slice::from_ref(r)).await;
        outcomes.pop().expect("a read gives an outcome per ref")
    }

    /// The outcome at each of `rs`, in order, typed as `T` as `read` types
    /// one, asked of the core in one request (contract 2.5).
    pub async fn read_all<T: Value>(&self, rs: &[Ref]) -> Vec<Outcome<T>> {
        match self.mailbox.exchange(Ask::Read(rs.to_vec())).await {
            Reply::Outcomes(outcomes) => outcomes.into_iter().map(Outcome::downcast).collect(),
            Reply::Product(_) => unreachable!("the core replies to a read with outcomes"),
        }
    }

    /// Runs `section` on the thread of resource `R`, with exclusive use of
    /// it, and waits for what it returns. Other bodies run meanwhile.
    pub async fn exclusive<R, T, F>(&self, section: F) -> T
    where
        R: 'static,
        T: Send + 'static,
        F: FnOnce(&mut R, &Section<R>) -> T + Send + 'static,
    {
        let job: SectionJob = Box::new(move |resource, parts| {
            let resource = resource.downcast_mut::<R>();
            let resource = resource.expect("a resource thread holds the type it was added under");
            Box::new(section(resource, &Section::new(parts)))
        });
        let ask = Ask::Exclusive(SectionAsk {
            resource: TypeId::of::<R>(),
            resource_name: type_name::<R>(),
            job,
        });
        match self.mailbox.exchange(ask).await {
            Reply::Product(product) => *product
                .downcast::<T>()
                .expect("a section's product keeps its type"),
            Reply::Outcomes(_) => unreachable!("the core replies to a section with its product"),
        }
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.is_set()
    }
}
