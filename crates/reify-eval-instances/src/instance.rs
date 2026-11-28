//! The instance kind (PRD 5.4): one kind makes every instance, a root or a
//! sub, from its definition's member table and the table of member kinds.

use reify_core::{DefinitionName, PathName};
use reify_eval_core::{Cx, Hash128, Kind, KindId, Outcome, Produced, Ref};
use reify_eval_source::MemberTable;

use crate::content::content_of;
use crate::core_seam;
use crate::declarations::{AT_ROOT, AtRoot, MEMBER_KINDS, TABLE};
use crate::failure::InstanceFailure;
use crate::kind_ids;
use crate::member_kinds::MemberKinds;

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
}

/// The instance the body reads, or what it answers instead.
async fn instance(cx: &Cx) -> Result<PresentInstance, Produced<PresentInstance>> {
    let at_root = is_root(cx).await?;
    let _table = core_seam::need::<MemberTable, _>(cx, &Ref::binding(TABLE)).await?;
    let _member_kinds = core_seam::need::<MemberKinds, _>(cx, &Ref::binding(MEMBER_KINDS)).await?;
    Ok(PresentInstance {
        at_root,
        members: Vec::new(),
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
