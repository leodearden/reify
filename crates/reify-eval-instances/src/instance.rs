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

use std::sync::Arc;

use reify_core::{DefinitionName, PathName};
use reify_eval_core::{Cx, Declaration, Hash128, Kind, KindId, Outcome, Produced, Ref, Step};
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

/// The instance the body reads, or what it answers instead.
async fn instance(cx: &Cx) -> Result<PresentInstance, Produced<PresentInstance>> {
    let at_root = is_root(cx).await?;
    let table = core_seam::need::<MemberTable, _>(cx, &Ref::binding(TABLE)).await?;
    let member_kinds = member_kinds(cx).await?;
    Ok(PresentInstance {
        at_root,
        members: declared_members(&table, &member_kinds),
    })
}

/// Whether the instance is a root: its declaration alone binds the root
/// marker.
async fn is_root(cx: &Cx) -> Result<bool, Produced<PresentInstance>> {
    let marker = Ref::binding(AT_ROOT);
    match core_seam::read::<AtRoot>(cx, &marker).await {
        Outcome::Value(_) => Ok(true),
        Outcome::Absent => Ok(false),
        Outcome::Failure(_) => Err(core_seam::blocked(marker)),
    }
}

/// The table of member kinds; with none bound, the empty one.
async fn member_kinds(cx: &Cx) -> Result<Arc<MemberKinds>, Produced<PresentInstance>> {
    let at = Ref::binding(MEMBER_KINDS);
    match core_seam::read::<MemberKinds>(cx, &at).await {
        Outcome::Value(member_kinds) => Ok(member_kinds),
        Outcome::Absent => Ok(Arc::new(MemberKinds::empty())),
        Outcome::Failure(_) => Err(core_seam::blocked(at)),
    }
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

/// How a name with `declarations` is evaluated.
fn evaluation(declarations: &[(&Member, bool)], member_kinds: &MemberKinds) -> Evaluation {
    if let Some((guarded, _)) = declarations.iter().find(|(_, under_guard)| *under_guard) {
        return Evaluation::Failure(InstanceFailure::NoKindSupplied {
            member: guarded.kind,
            under_guard: true,
        });
    }
    let (deciding, _) = declarations
        .last()
        .expect("a written name has a declaration");
    match member_kinds.recipe(deciding.kind) {
        Some(kind) => Evaluation::Kind {
            kind,
            definition: match &deciding.declared_type {
                DeclaredType::Definition(definition) => Some(definition.clone()),
                DeclaredType::None | DeclaredType::Value(_) | DeclaredType::Written(_) => None,
            },
        },
        None => Evaluation::Failure(InstanceFailure::NoKindSupplied {
            member: deciding.kind,
            under_guard: false,
        }),
    }
}
