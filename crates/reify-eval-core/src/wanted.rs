//! What a body asks for in a read (contract 2.3): the outcome at the place
//! a ref names, or a child it declares at a step below its own place and
//! reads (7.1). The request is recorded as asked, so a child's step and
//! declaration are part of it.

use std::sync::Arc;

use crate::declaration::Declaration;
use crate::hash::Encoder;
use crate::reference::Ref;
use crate::step::Step;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Wanted {
    Ref(Ref),
    /// The child the reader declares at `step` below itself, declared as
    /// the declaration says.
    Child(Step, Arc<Declaration>),
}

impl From<Ref> for Wanted {
    fn from(r: Ref) -> Self {
        Wanted::Ref(r)
    }
}

impl Wanted {
    pub fn child(step: Step, declaration: Declaration) -> Self {
        Wanted::Child(step, Arc::new(declaration))
    }

    pub(crate) fn encode(&self, encoder: &mut Encoder) {
        match self {
            Wanted::Ref(r) => r.encode(encoder),
            Wanted::Child(step, declaration) => {
                encoder.tag("wanted.child");
                step.encode(encoder);
                declaration.encode(encoder);
            }
        }
    }
}
