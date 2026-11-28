//! The table of member kinds (PRD 5.4): which kind evaluates a member of
//! each `MemberKind`. It is a snapshot input the root binds and passes down,
//! so it is part of what every result below a root is stored under.
//!
//! `built()` holds the rows this crate builds: only `sub`, to the instance
//! kind, so one kind makes every instance, a root or a sub. Every other row
//! is empty, the geometry and `solves` rows included, and a member whose row
//! is empty answers `no-kind-supplied`.

use reify_eval_core::{Hash128, KindId};
use reify_eval_source::MemberKind;

use crate::content::content_of;
use crate::kind_ids;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MemberKinds {
    rows: Vec<(MemberKind, KindId)>,
}

impl MemberKinds {
    /// The rows this crate builds.
    pub fn built() -> Self {
        Self {
            rows: vec![(MemberKind::Sub, kind_ids::INSTANCE)],
        }
    }

    /// No row filled: no member is evaluated.
    pub fn empty() -> Self {
        Self { rows: Vec::new() }
    }

    /// The kind that evaluates a member of kind `member`; none where its
    /// row is empty.
    pub(crate) fn recipe(&self, member: MemberKind) -> Option<KindId> {
        self.rows
            .iter()
            .find(|(kind, _)| *kind == member)
            .map(|(_, recipe)| *recipe)
    }

    pub(crate) fn content(&self) -> Hash128 {
        content_of(self)
    }
}
