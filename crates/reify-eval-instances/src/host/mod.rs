//! The host (PRD 5.1 layer 5): it loads the second output's definition list
//! into a snapshot and answers typed paths in this crate's own terms.

mod answer;
mod core_calls;
mod demand;
mod load;

use reify_core::{DefinitionName, InstancePath};
use reify_eval_core::Snapshot;
use reify_eval_source::DefinitionList;

pub use answer::{AnswerFailure, HostAnswer};
pub use demand::DemandError;

use crate::member_kinds::MemberKinds;
use core_calls::OnCore;

pub struct Host {
    on_core: OnCore,
}

/// A loaded program. It belongs to the host that loaded it, and is answered
/// on that host only.
pub struct Loaded(Snapshot);

impl Host {
    pub fn new() -> Self {
        Self {
            on_core: OnCore::new(),
        }
    }

    /// The program of `definitions`, its members evaluated by
    /// `member_kinds`, with an instance at each of `roots` it defines.
    pub fn load(
        &mut self,
        definitions: &DefinitionList,
        member_kinds: &MemberKinds,
        roots: &[DefinitionName],
    ) -> Loaded {
        Loaded(
            self.on_core
                .snapshot(load::inputs(definitions, member_kinds, roots)),
        )
    }

    /// The answer at `path` of the program `loaded`.
    pub fn answer(
        &mut self,
        loaded: &Loaded,
        path: &InstancePath,
    ) -> Result<HostAnswer, DemandError> {
        let at = demand::core_path(path)?;
        Ok(self.on_core.answer(&loaded.0, at))
    }
}

impl Default for Host {
    fn default() -> Self {
        Self::new()
    }
}
