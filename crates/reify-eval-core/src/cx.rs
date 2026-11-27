//! What a running body holds: its own mailbox and cancel flag, and nothing
//! of the core's.

use std::any::{TypeId, type_name};
use std::sync::Arc;

use crate::cancel::Cancel;
use crate::declaration::Declaration;
use crate::mailbox::{Ask, Mailbox, Reply, SectionAsk, SectionJob};
use crate::outcome::{Outcome, Value};
use crate::reference::Ref;
use crate::section::Section;
use crate::step::Step;
use crate::wanted::Wanted;

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
        self.read_one(Wanted::Ref(r.clone())).await
    }

    /// Declares the child of this node at `step` as `declaration` says,
    /// and reads it (contract 7.1). Within one run, a step declared again
    /// with an equal declaration names the same child.
    pub async fn child<T: Value>(&self, step: Step, declaration: Declaration) -> Outcome<T> {
        self.read_one(Wanted::child(step, declaration)).await
    }

    /// The outcome of each of `wanted`, in order, typed as `T` as `read`
    /// types one, asked of the core in one request (contract 2.5).
    pub async fn read_all<T: Value>(&self, wanted: &[Wanted]) -> Vec<Outcome<T>> {
        match self.mailbox.exchange(Ask::Read(wanted.to_vec())).await {
            Reply::Outcomes(outcomes) => outcomes.into_iter().map(Outcome::downcast).collect(),
            Reply::Product(_) => unreachable!("the core replies to a read with outcomes"),
        }
    }

    async fn read_one<T: Value>(&self, wanted: Wanted) -> Outcome<T> {
        let mut outcomes = self.read_all(std::slice::from_ref(&wanted)).await;
        outcomes.pop().expect("a read gives an outcome per item")
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
