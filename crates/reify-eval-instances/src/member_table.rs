//! The member table kind: one definition's member table, passed on from
//! its snapshot input. It declares no children.

use reify_eval_core::{Cx, Hash128, Kind, KindId, Produced, Ref};
use reify_eval_source::MemberTable;

use crate::content::member_table_content;
use crate::core_seam;
use crate::declarations::TABLE;
use crate::kind_ids;

pub(crate) struct MemberTableKind;

impl Kind for MemberTableKind {
    type Out = MemberTable;
    const ID: KindId = kind_ids::MEMBER_TABLE;
    const VERSION: u32 = 1;

    async fn run(cx: &Cx) -> Produced<MemberTable> {
        match core_seam::need::<MemberTable, _>(cx, &Ref::binding(TABLE)).await {
            Ok(table) => core_seam::passed_on(table),
            Err(answered) => answered,
        }
    }

    fn content(out: &MemberTable) -> Option<Hash128> {
        Some(member_table_content(out))
    }
}
