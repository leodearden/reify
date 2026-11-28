//! The one map between a typed path's steps and the core's steps
//! (docs/prds/instances-on-the-core.md section 15 item 2), both ways.
//!
//! The spaces: a member by its name in space `member`; an instance's fixed
//! children by their word in space `fixed`; a definition's member table
//! below `defs` in space `definition`, keyed by the definition's path text.
//! So a typed `/defs[Name]` is two core steps, `fixed:defs` then
//! `definition:Name`. A kind reads a core step back to a typed name here,
//! never by matching its text.
//!
//! Only the steps a kind of this crate declares are mapped. A leaf that
//! declares another kind of step adds its space and its arm of
//! `core_steps` here.

use std::str::FromStr;

use reify_core::{DefinitionName, InstancePath, PathName, PathStep};
use reify_eval_core::{Root, Space, Step, StepKey};

const MEMBER: Space = Space::Named("member");
const FIXED: Space = Space::Named("fixed");
const DEFINITION: Space = Space::Named("definition");

/// The fixed child holding the definition table, by the word a typed path
/// prints for it.
const DEFS_WORD: &str = "defs";

/// The member called `name`.
pub(crate) fn member(name: &PathName) -> Step {
    Step::new(MEMBER, StepKey::Name(name.as_str().into()))
}

/// The member name `step` is the step of; none for a step of another space.
pub(crate) fn member_name(step: &Step) -> Option<PathName> {
    match (step.space(), step.key()) {
        (MEMBER, StepKey::Name(text)) => PathName::new(&**text).ok(),
        _ => None,
    }
}

/// The fixed child holding the definition table.
pub(crate) fn defs() -> Step {
    Step::new(FIXED, StepKey::Name(DEFS_WORD.into()))
}

/// The member table of `definition`, below `defs`.
pub(crate) fn definition(definition: &DefinitionName) -> Step {
    Step::new(
        DEFINITION,
        StepKey::Name(definition_text(definition).into()),
    )
}

/// The definition `step` is the step of; none for a step of another space.
pub(crate) fn definition_name(step: &Step) -> Option<DefinitionName> {
    match (step.space(), step.key()) {
        (DEFINITION, StepKey::Name(text)) => InstancePath::from_str(text)
            .ok()
            .filter(|path| path.steps().is_empty())
            .map(|path| path.root().clone()),
        _ => None,
    }
}

/// A definition's text: the path text of the definition alone, which
/// `definition_name` parses back.
pub(crate) fn definition_text(definition: &DefinitionName) -> String {
    InstancePath::new(definition.clone()).to_string()
}

/// The core root of the instance of `definition`.
pub(crate) fn root(definition: &DefinitionName) -> Root {
    Root::named(definition_text(definition))
}

/// The core steps of a typed step; none where no kind of this crate
/// declares a child at such a step.
pub(crate) fn core_steps(step: &PathStep) -> Option<Vec<Step>> {
    match step {
        PathStep::Member(name) => Some(vec![member(name)]),
        PathStep::Defs(d) => Some(vec![defs(), definition(d)]),
        PathStep::Element(_)
        | PathStep::Summary
        | PathStep::AsValue
        | PathStep::Constraint(_)
        | PathStep::Braces
        | PathStep::Purpose(_)
        | PathStep::Arguments
        | PathStep::Guard(_)
        | PathStep::Count(_)
        | PathStep::At(_)
        | PathStep::Auto(_)
        | PathStep::MadeList
        | PathStep::Made(_)
        | PathStep::Open(_)
        | PathStep::Problem
        | PathStep::Solves => None,
    }
}

#[cfg(test)]
mod tests {
    use reify_core::{DefinitionName, InstancePath, ModuleName, PathName, PathStep};

    use super::{core_steps, definition, definition_name, defs, member, member_name, root};

    fn name(text: &str) -> PathName {
        PathName::new(text).expect("a test name is an identifier")
    }

    fn in_entry(text: &str) -> DefinitionName {
        DefinitionName::new(None, name(text))
    }

    fn qualified() -> DefinitionName {
        let module =
            ModuleName::new(vec![name("std"), name("units")]).expect("a module of two segments");
        DefinitionName::new(Some(module), name("Plate"))
    }

    /// Member names, including those that read like a fixed child's word.
    fn member_names() -> Vec<PathName> {
        ["t", "body", "summary", "defs", "as_value", "_x9"]
            .into_iter()
            .map(name)
            .collect()
    }

    fn definitions() -> Vec<DefinitionName> {
        vec![in_entry("Plate"), qualified()]
    }

    #[test]
    fn a_member_step_reads_back_as_its_name() {
        for n in member_names() {
            assert_eq!(member_name(&member(&n)), Some(n.clone()), "{n:?}");
        }
    }

    #[test]
    fn a_definition_step_reads_back_as_its_definition() {
        for d in definitions() {
            assert_eq!(definition_name(&definition(&d)), Some(d.clone()), "{d:?}");
        }
    }

    #[test]
    fn the_spaces_are_disjoint() {
        for n in member_names() {
            assert_eq!(definition_name(&member(&n)), None, "{n:?}");
        }
        for d in definitions() {
            assert_eq!(member_name(&definition(&d)), None, "{d:?}");
        }
        assert_eq!(member_name(&defs()), None);
        assert_eq!(definition_name(&defs()), None);
    }

    #[test]
    fn a_typed_step_maps_to_the_core_steps_its_kinds_declare() {
        for n in member_names() {
            assert_eq!(
                core_steps(&PathStep::Member(n.clone())),
                Some(vec![member(&n)])
            );
        }
        for d in definitions() {
            assert_eq!(
                core_steps(&PathStep::Defs(d.clone())),
                Some(vec![defs(), definition(&d)])
            );
        }
    }

    #[test]
    fn a_root_is_named_by_its_definitions_path_text() {
        for d in definitions() {
            assert_eq!(
                root(&d).name(),
                InstancePath::new(d.clone()).to_string(),
                "{d:?}"
            );
        }
    }
}
