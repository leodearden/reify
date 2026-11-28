//! The second output's definition list, as the snapshot inputs the kinds
//! read.

use reify_core::DefinitionName;
use reify_eval_core::{Input, InputKey};
use reify_eval_source::DefinitionList;

use crate::content::{content_of, member_table_content};
use crate::declarations::{self, AtRoot};
use crate::definition_table::DefinitionNames;
use crate::member_kinds::MemberKinds;
use crate::step_map;

/// The table of member kinds, the root marker, the definition names, one
/// member table per definition, and one root declaration for each of
/// `roots` in the list. A root not in the list gets no input, so it answers
/// absent.
pub(crate) fn inputs(
    definitions: &DefinitionList,
    member_kinds: &MemberKinds,
    roots: &[DefinitionName],
) -> Vec<(InputKey, Input)> {
    let mut inputs = vec![
        (
            declarations::member_kinds_key(),
            Input::of(member_kinds.clone(), member_kinds.content()),
        ),
        (
            declarations::at_root_key(),
            Input::of(AtRoot, content_of(&AtRoot)),
        ),
        (declarations::definition_names_key(), {
            let names = DefinitionNames::of(definitions);
            let content = content_of(&names);
            Input::of(names, content)
        }),
    ];
    inputs.extend(definitions.iter().map(|definition| {
        let content = member_table_content(&definition.members);
        (
            declarations::member_table_key(&definition.name),
            Input::of(definition.members.clone(), content),
        )
    }));
    inputs.extend(
        roots
            .iter()
            .filter(|root| definitions.get(root).is_some())
            .map(|root| {
                let declaration = declarations::root_instance(root);
                let content = declaration.content();
                (
                    InputKey::Root(step_map::root(root)),
                    Input::of(declaration, content),
                )
            }),
    );
    inputs
}
