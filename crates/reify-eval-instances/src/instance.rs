//! The instance kind (PRD 5.4): one kind makes every instance, a root or a
//! sub, from its definition's member table and the table of member kinds.
//!
//! An instance declares one child per named member, the names inside a
//! group's branches included, at the place of the name's first declaration.
//! A name declared under any guard answers no-kind-supplied, since no kind
//! of this crate reads a guard (PRD 5.2). Otherwise the name's last
//! declaration decides, which is the definition's own where it re-declares
//! an inherited member, and its row of the table of member kinds gives the
//! kind that evaluates it; an empty row answers no-kind-supplied. A member
//! with no name gets no child here: its steps belong to the kind that
//! evaluates it. A root also declares `defs`, the definition table.

use reify_core::{DefinitionName, PathName};
use reify_eval_core::{Cx, Declaration, Hash128, Kind, KindId, Produced, Ref, Step};
use reify_eval_source::{DeclaredType, Member, MemberTable};

use crate::content::content_of;
use crate::core_seam;
use crate::declarations::{self, AT_ROOT, AtRoot, MEMBER_KINDS, TABLE};
use crate::failure::InstanceFailure;
use crate::kind_ids;
use crate::member_kinds::MemberKinds;
use crate::step_map;

pub(crate) struct InstanceKind;

/// An instance: whether it is a root, and the members it declares.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct PresentInstance {
    at_root: bool,
    members: Vec<DeclaredMember>,
}

/// A named member, and how it is evaluated.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DeclaredMember {
    name: PathName,
    evaluated_by: Evaluation,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Evaluation {
    /// By the kind its row gives, instantiating `definition` where the
    /// member is a sub.
    Kind {
        kind: KindId,
        definition: Option<DefinitionName>,
    },
    /// By no kind: the member answers this failure.
    Failure(InstanceFailure),
}

impl Kind for InstanceKind {
    type Out = PresentInstance;
    const ID: KindId = kind_ids::INSTANCE;
    const VERSION: u32 = 1;

    async fn run(cx: &Cx) -> Produced<PresentInstance> {
        match instance(cx).await {
            Ok(instance) => core_seam::value(instance),
            Err(answered) => answered,
        }
    }

    fn content(out: &PresentInstance) -> Option<Hash128> {
        Some(content_of(out))
    }

    fn declares(out: &PresentInstance, step: &Step) -> Option<Declaration> {
        if out.at_root && *step == step_map::defs() {
            return Some(declarations::definition_table());
        }
        let name = step_map::member_name(step)?;
        let member = out.members.iter().find(|member| member.name == name)?;
        Some(member.declaration(out.at_root))
    }

    fn steps(out: &PresentInstance) -> impl Iterator<Item = Step> {
        let defs = out.at_root.then(step_map::defs);
        let members = out
            .members
            .iter()
            .map(|member| step_map::member(&member.name));
        defs.into_iter().chain(members)
    }
}

impl DeclaredMember {
    /// The member's child, declared by an instance that is a root or not.
    fn declaration(&self, at_root: bool) -> Declaration {
        match &self.evaluated_by {
            Evaluation::Kind { kind, definition } => {
                declarations::member(*kind, definition.as_ref(), at_root)
            }
            Evaluation::Failure(failure) => declarations::unsupplied(failure),
        }
    }
}

/// The instance the body reads, or what it answers instead. It is a root
/// where its declaration binds the root marker, which only a root's does.
async fn instance(cx: &Cx) -> Result<PresentInstance, Produced<PresentInstance>> {
    let at_root = core_seam::optional::<AtRoot, _>(cx, &Ref::binding(AT_ROOT))
        .await?
        .is_some();
    let table = core_seam::need::<MemberTable, _>(cx, &Ref::binding(TABLE)).await?;
    let member_kinds = core_seam::need::<MemberKinds, _>(cx, &Ref::binding(MEMBER_KINDS)).await?;
    Ok(PresentInstance {
        at_root,
        members: declared_members(&table, &member_kinds),
    })
}

/// Every declaration of one name, in table order, each with whether it is
/// under a guard.
struct Written<'t> {
    name: PathName,
    declarations: Vec<(&'t Member, bool)>,
}

/// One member per name `table` declares, in the order of each name's first
/// declaration.
fn declared_members(table: &MemberTable, member_kinds: &MemberKinds) -> Vec<DeclaredMember> {
    let mut written = Vec::new();
    collect_written(table, false, &mut written);
    written
        .into_iter()
        .map(|written| DeclaredMember {
            evaluated_by: evaluation(&written.declarations, member_kinds),
            name: written.name,
        })
        .collect()
}

/// Adds each named member of `table` to its name's declarations, walking
/// into branches, whose members are under a guard.
fn collect_written<'t>(table: &'t MemberTable, in_branch: bool, written: &mut Vec<Written<'t>>) {
    for member in table {
        let under_guard = in_branch || member.guarded;
        if let Some(name) = &member.name {
            match written.iter_mut().find(|written| written.name == *name) {
                Some(written) => written.declarations.push((member, under_guard)),
                None => written.push(Written {
                    name: name.clone(),
                    declarations: vec![(member, under_guard)],
                }),
            }
        }
        for branch in &member.branches {
            collect_written(branch, true, written);
        }
    }
}

/// How a name with `declarations` is evaluated. A sub whose definition did
/// not resolve has no table to make an instance of, so no kind evaluates it.
fn evaluation(declarations: &[(&Member, bool)], member_kinds: &MemberKinds) -> Evaluation {
    if let Some((guarded, _)) = declarations.iter().find(|(_, under_guard)| *under_guard) {
        return no_kind_supplied(guarded, true);
    }
    let (deciding, _) = declarations
        .last()
        .expect("a written name has a declaration");
    let Some(kind) = member_kinds.recipe(deciding.kind) else {
        return no_kind_supplied(deciding, false);
    };
    let definition = match &deciding.declared_type {
        DeclaredType::Definition(definition) => Some(definition.clone()),
        DeclaredType::None | DeclaredType::Value(_) | DeclaredType::Written(_) => None,
    };
    if kind == kind_ids::INSTANCE && definition.is_none() {
        return no_kind_supplied(deciding, false);
    }
    Evaluation::Kind { kind, definition }
}

fn no_kind_supplied(member: &Member, under_guard: bool) -> Evaluation {
    Evaluation::Failure(InstanceFailure::NoKindSupplied {
        member: member.kind,
        under_guard,
    })
}

#[cfg(test)]
mod tests {
    use reify_core::PathName;
    use reify_eval_core::{Kind, Step};

    use super::{InstanceKind, PresentInstance, declared_members};
    use crate::member_kinds::MemberKinds;
    use crate::step_map;
    use crate::test_support::{compiled, declared_steps, in_entry};

    /// A cell, a sub, a name declared in both branches of a group, a member
    /// with no name, and a geometry member.
    const SOURCE: &str = "structure def P {}\nstructure def A { param w : Length = 1mm  sub p : P  \
                          constraint w > 0mm  where w > 1mm { param g : Length = 1mm } else \
                          { param g : Length = 2mm }  let body = box(w, w, w) }\n";

    fn instance_of_a(at_root: bool) -> PresentInstance {
        let definitions = compiled(SOURCE);
        let a = definitions.get(&in_entry("A")).expect("SOURCE defines A");
        PresentInstance {
            at_root,
            members: declared_members(&a.members, &MemberKinds::built()),
        }
    }

    fn member(name: &str) -> Step {
        step_map::member(&PathName::new(name).expect("an identifier"))
    }

    #[test]
    fn a_root_steps_through_defs_then_each_named_member_once() {
        let root = instance_of_a(true);
        assert_eq!(
            declared_steps::<InstanceKind>(&root),
            [
                step_map::defs(),
                member("w"),
                member("p"),
                member("g"),
                member("body")
            ]
        );
        assert!(InstanceKind::declares(&root, &member("nothing")).is_none());
    }

    #[test]
    fn an_instance_below_a_root_steps_through_its_named_members_alone() {
        let below = instance_of_a(false);
        assert_eq!(
            declared_steps::<InstanceKind>(&below),
            [member("w"), member("p"), member("g"), member("body")]
        );
        assert!(InstanceKind::declares(&below, &step_map::defs()).is_none());
    }
}
