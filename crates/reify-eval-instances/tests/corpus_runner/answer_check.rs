//! Whether one host answer meets one row's form, in the runner's own types
//! (docs/prds/instances-on-the-core.md section 5.8, step 3). A form whose
//! answer no kind gives yet is a mismatch, never a skip.

use reify_eval_instances::{AnswerFailure, HostAnswer, InstanceFailure};

use crate::expectation::{FailureCode, Form, Presence};

/// How `answer` misses `form`, as text; none where it meets it. An `open`
/// row is never missed: its answer is not pinned.
pub fn mismatch(form: &Form, answer: &HostAnswer) -> Option<String> {
    let met = match form {
        Form::Presence(Presence::Present) => matches!(answer, HostAnswer::Present),
        Form::Presence(Presence::Absent) => matches!(answer, HostAnswer::Absent),
        Form::Failure(code) => failure_met(*code, answer),
        Form::Open { .. } => true,
        Form::Value(_)
        | Form::Verdict(_)
        | Form::Summary(_)
        | Form::UndeterminedBecause(_)
        | Form::Presence(Presence::Undetermined) => false,
        Form::SameAs(_) | Form::Roots(_) | Form::Compile | Form::CompileError { .. } => {
            return Some(format!(
                "a {form:?} row is not one answer's to meet: the runner compares it elsewhere"
            ));
        }
    };
    (!met).then(|| format!("expected {form:?}, answered {answer:?}"))
}

fn failure_met(code: FailureCode, answer: &HostAnswer) -> bool {
    let HostAnswer::Failed(failure) = answer else {
        return false;
    };
    match code {
        FailureCode::NoKindSupplied => matches!(
            failure,
            AnswerFailure::Instance(InstanceFailure::NoKindSupplied { .. })
        ),
        FailureCode::Cycle => matches!(failure, AnswerFailure::Cycle),
        FailureCode::Blocked => matches!(failure, AnswerFailure::Blocked),
        FailureCode::Limit => matches!(failure, AnswerFailure::Limit),
        FailureCode::AmbiguousDeclaration | FailureCode::TypeArgumentUnresolved => false,
    }
}
