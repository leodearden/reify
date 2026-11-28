//! What each kind of this crate is declared with: the names of its
//! bindings, the snapshot inputs and places they name, and the marker a
//! root's declaration alone binds. One function builds each input key, and
//! nothing parses one back.

use reify_core::DefinitionName;
use reify_eval_core::{Binding, Declaration, Input, InputKey, KindId, Ref};

use crate::failure::InstanceFailure;
use crate::kind_ids;
use crate::step_map;

/// A definition's member table.
pub(crate) const TABLE: &str = "table";
/// The definition table, `defs`.
pub(crate) const DEFS: &str = "defs";
/// The table of member kinds.
pub(crate) const MEMBER_KINDS: &str = "member_kinds";
/// The root marker: bound on a root's instance alone.
pub(crate) const AT_ROOT: &str = "at_root";
/// The program's definition names.
pub(crate) const NAMES: &str = "names";
/// The failure an unsupplied member answers.
pub(crate) const FAILURE: &str = "failure";

pub(crate) fn member_kinds_key() -> InputKey {
    InputKey::named("instances.member-kinds")
}

pub(crate) fn at_root_key() -> InputKey {
    InputKey::named("instances.at-root")
}

pub(crate) fn definition_names_key() -> InputKey {
    InputKey::named("instances.definition-names")
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

/// The definition table a root declares at `defs`.
pub(crate) fn definition_table() -> Declaration {
    Declaration::new(kind_ids::DEFINITION_TABLE).bind(NAMES, Binding::Input(definition_names_key()))
}

/// The member table of `definition`, over its snapshot input.
pub(crate) fn member_table(definition: &DefinitionName) -> Declaration {
    Declaration::new(kind_ids::MEMBER_TABLE)
        .bind(TABLE, Binding::Input(member_table_key(definition)))
}

/// A member no kind evaluates, which answers `failure`.
pub(crate) fn unsupplied(failure: &InstanceFailure) -> Declaration {
    let failure = Input::of(failure.clone(), failure.content());
    Declaration::new(kind_ids::UNSUPPLIED).bind(FAILURE, Binding::Constant(failure))
}

/// A member evaluated by `kind`, instantiating `definition` where it is a
/// sub, declared by an instance that is a root or not. Under a root, `defs`
/// and the table are places below the root; below one, they are passed on
/// from the declarer, as is the table of member kinds.
pub(crate) fn member(
    kind: KindId,
    definition: Option<&DefinitionName>,
    at_root: bool,
) -> Declaration {
    let defs = if at_root {
        Binding::Declarer(vec![step_map::defs()])
    } else {
        Binding::Passed(Ref::binding(DEFS))
    };
    let declaration = Declaration::new(kind)
        .bind(DEFS, defs)
        .bind(MEMBER_KINDS, Binding::Passed(Ref::binding(MEMBER_KINDS)));
    let Some(definition) = definition else {
        return declaration;
    };
    let table = if at_root {
        Binding::Declarer(vec![step_map::defs(), step_map::definition(definition)])
    } else {
        Binding::Passed(Ref::binding(DEFS).step(step_map::definition(definition)))
    };
    declaration.bind(TABLE, table)
}
