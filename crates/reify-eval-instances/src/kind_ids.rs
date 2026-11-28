//! The id each kind of this crate is registered under and its declarations
//! name. It imports no kind, so a declaration names a kind without
//! importing it.

use reify_eval_core::KindId;

pub(crate) const DEFINITION_TABLE: KindId = KindId("instances.definition-table");
pub(crate) const MEMBER_TABLE: KindId = KindId("instances.member-table");
pub(crate) const INSTANCE: KindId = KindId("instances.instance");
pub(crate) const UNSUPPLIED: KindId = KindId("instances.unsupplied");
