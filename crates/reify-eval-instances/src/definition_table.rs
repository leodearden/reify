//! The definition table kind: the node `defs` a root declares, whose
//! children are the member tables of the program's definitions, each at
//! `defs` then the definition's step. A sub reads its table there.

use reify_core::DefinitionName;
use reify_eval_core::{Cx, Declaration, Hash128, Kind, KindId, Produced, Ref, Step};
use reify_eval_source::DefinitionList;

use crate::content::content_of;
use crate::core_seam;
use crate::declarations::{self, NAMES};
use crate::kind_ids;
use crate::step_map;

/// The program's definitions, in definition-list order.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct DefinitionNames(Vec<DefinitionName>);

impl DefinitionNames {
    pub(crate) fn of(definitions: &DefinitionList) -> Self {
        Self(
            definitions
                .iter()
                .map(|definition| definition.name.clone())
                .collect(),
        )
    }
}

pub(crate) struct DefinitionTableKind;

impl Kind for DefinitionTableKind {
    type Out = DefinitionNames;
    const ID: KindId = kind_ids::DEFINITION_TABLE;
    const VERSION: u32 = 1;

    async fn run(cx: &Cx) -> Produced<DefinitionNames> {
        match core_seam::need::<DefinitionNames, _>(cx, &Ref::binding(NAMES)).await {
            Ok(names) => core_seam::passed_on(names),
            Err(answered) => answered,
        }
    }

    fn content(out: &DefinitionNames) -> Option<Hash128> {
        Some(content_of(out))
    }

    fn declares(out: &DefinitionNames, step: &Step) -> Option<Declaration> {
        let definition = step_map::definition_name(step)?;
        out.0
            .contains(&definition)
            .then(|| declarations::member_table(&definition))
    }

    fn steps(out: &DefinitionNames) -> impl Iterator<Item = Step> {
        out.0.iter().map(step_map::definition)
    }
}
