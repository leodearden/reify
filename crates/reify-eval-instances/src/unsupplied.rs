//! The node of a member that no kind evaluates (PRD 5.4): it answers its
//! bound failure, no-kind-supplied, and whatever reads it is blocked. It
//! never has a value.

use reify_eval_core::{Cx, Kind, KindId, Produced, Ref};

use crate::core_seam;
use crate::declarations::FAILURE;
use crate::failure::InstanceFailure;
use crate::kind_ids;

pub(crate) struct UnsuppliedKind;

/// What an unsupplied member's value would be: there is none.
pub(crate) enum Unanswerable {}

impl Kind for UnsuppliedKind {
    type Out = Unanswerable;
    const ID: KindId = kind_ids::UNSUPPLIED;
    const VERSION: u32 = 1;

    async fn run(cx: &Cx) -> Produced<Unanswerable> {
        match core_seam::need::<InstanceFailure, _>(cx, &Ref::binding(FAILURE)).await {
            Ok(failure) => core_seam::failed(kind_ids::UNSUPPLIED, &failure),
            Err(answered) => answered,
        }
    }
}
