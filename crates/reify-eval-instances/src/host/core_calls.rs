//! The seam between the host and the core's host interface (PRD 5.1 rule
//! 8): the only file that calls it, and the only one that matches the
//! core's `Answer` and `Failure`, so a core task adding a variant edits this
//! file and `core_seam` only.
//!
//! What each helper promises:
//! - `new`: a core built with every kind of this crate;
//! - `snapshot`: the snapshot of the inputs given;
//! - `answer`: the answer at one path, in a request of its own, run to idle
//!   and ended.
//!
//! The claim of determinism: the same snapshot answers the same path alike,
//! whatever was asked before.

use reify_eval_core::{
    Answer, Budget, Builder, Core, Demand, Erased, Event, Failure, Input, InputKey, Outcome, Path,
    Snapshot,
};

use crate::definition_table::DefinitionTableKind;
use crate::failure::InstanceFailure;
use crate::host::answer::{self, AnswerFailure, HostAnswer};
use crate::instance::InstanceKind;
use crate::member_table::MemberTableKind;
use crate::unsupplied::UnsuppliedKind;

pub(crate) struct OnCore {
    core: Core,
}

impl OnCore {
    pub(crate) fn new() -> Self {
        let builder = Builder::new()
            .kind::<DefinitionTableKind>()
            .and_then(Builder::kind::<InstanceKind>)
            .and_then(Builder::kind::<MemberTableKind>)
            .and_then(Builder::kind::<UnsuppliedKind>);
        let builder = builder.expect("this crate's kind ids are distinct");
        Self {
            core: builder.build(),
        }
    }

    pub(crate) fn snapshot(&mut self, inputs: Vec<(InputKey, Input)>) -> Snapshot {
        self.core.snapshot(inputs)
    }

    /// # Panics
    ///
    /// If the idle core gave no answer at `path`, which contract 2.5 rules
    /// out.
    pub(crate) fn answer(&mut self, snapshot: &Snapshot, path: Path) -> HostAnswer {
        let request = self
            .core
            .evaluate(snapshot, Demand::of([path.clone()]), Budget::unlimited());
        self.core.run();
        let answer = self
            .core
            .take_events()
            .into_iter()
            .find_map(|event| match event {
                Event::Answered {
                    request: to,
                    path: at,
                    answer,
                    ..
                } if to == request.id() && at == path => Some(answer),
                _ => None,
            });
        self.core.end(request);
        let answer = answer.unwrap_or_else(|| panic!("the idle core gave no answer at {path:?}"));
        host_answer(answer)
    }
}

fn host_answer(answer: Answer<Erased>) -> HostAnswer {
    match answer {
        Answer::Final(Outcome::Value(value)) => answer::of_value(&*value),
        Answer::Final(Outcome::Absent) => HostAnswer::Absent,
        Answer::Final(Outcome::Failure(failure)) => HostAnswer::Failed(answer_failure(failure)),
        Answer::Pending(failure) => HostAnswer::Pending(format!("{failure:?}")),
    }
}

fn answer_failure(failure: Failure) -> AnswerFailure {
    match &failure {
        Failure::Cycle => AnswerFailure::Cycle,
        Failure::TooDeep { .. } | Failure::TooManyChildren { .. } => AnswerFailure::Limit,
        Failure::BlockedBy { .. } => AnswerFailure::Blocked,
        Failure::Described { description, .. } => match description.read::<InstanceFailure>() {
            Some(instance) => AnswerFailure::Instance(instance.clone()),
            None => AnswerFailure::Fault(format!("{failure:?}")),
        },
        Failure::WrongType { .. }
        | Failure::TwoRequestsAtOnce
        | Failure::OfKind { .. }
        | Failure::ConflictingChild { .. }
        | Failure::NotSupposable { .. }
        | Failure::ReadOutsideView => AnswerFailure::Fault(format!("{failure:?}")),
    }
}
