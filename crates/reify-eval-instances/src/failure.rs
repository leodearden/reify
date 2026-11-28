//! This crate's failures, in its own terms (contract 9): a code, and a
//! description the host reads back by its type.
//!
//! The first code is `no-kind-supplied` (PRD 5.4): a member that no kind
//! evaluates, because its row of the table of member kinds is empty, or
//! because it is declared under a guard, which no kind of this crate reads
//! yet (PRD 5.2).

use reify_eval_core::Hash128;
use reify_eval_source::MemberKind;

use crate::content::content_of;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum InstanceFailure {
    NoKindSupplied {
        member: MemberKind,
        under_guard: bool,
    },
}

impl InstanceFailure {
    pub fn code(&self) -> &'static str {
        match self {
            InstanceFailure::NoKindSupplied { .. } => "no-kind-supplied",
        }
    }

    pub(crate) fn content(&self) -> Hash128 {
        content_of(self)
    }
}
