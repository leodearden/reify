//! A definition's member table: every member it declares or inherits, in
//! order, each with its kind and declared type.

use reify_core::PathName;

/// One member of a definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// The member's own name; `None` for a form that names nothing.
    pub name: Option<PathName>,
}

/// A definition's members, in order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MemberTable {
    members: Vec<Member>,
}

impl MemberTable {
    pub fn iter(&self) -> std::slice::Iter<'_, Member> {
        self.members.iter()
    }

    pub fn len(&self) -> usize {
        self.members.len()
    }

    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }
}

impl<'t> IntoIterator for &'t MemberTable {
    type Item = &'t Member;
    type IntoIter = std::slice::Iter<'t, Member>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
