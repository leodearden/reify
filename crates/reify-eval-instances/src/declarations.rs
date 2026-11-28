//! What each kind of this crate is declared with: the names of its
//! bindings, the snapshot inputs and places they name, and the marker a
//! root's declaration alone binds. One function builds each input key, and
//! nothing parses one back.

use reify_core::DefinitionName;
use reify_eval_core::{Binding, Declaration, InputKey};

use crate::kind_ids;
use crate::step_map;

/// A definition's member table.
pub(crate) const TABLE: &str = "table";
/// The table of member kinds.
pub(crate) const MEMBER_KINDS: &str = "member_kinds";
/// The root marker: bound on a root's instance alone.
pub(crate) const AT_ROOT: &str = "at_root";

pub(crate) fn member_kinds_key() -> InputKey {
    InputKey::named("instances.member-kinds")
}

pub(crate) fn at_root_key() -> InputKey {
    InputKey::named("instances.at-root")
}

pub(crate) fn member_table_key(definition: &DefinitionName) -> InputKey {
    InputKey::named(format!(
        "instances.member-table[{}]",
        step_map::definition_text(definition)
    ))
}

/// The value of the root marker input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct AtRoot;

/// The instance at the root of `definition`: its table, the table of
/// member kinds and the root marker, each a snapshot input.
pub(crate) fn root_instance(definition: &DefinitionName) -> Declaration {
    Declaration::new(kind_ids::INSTANCE)
        .bind(TABLE, Binding::Input(member_table_key(definition)))
        .bind(MEMBER_KINDS, Binding::Input(member_kinds_key()))
        .bind(AT_ROOT, Binding::Input(at_root_key()))
}
