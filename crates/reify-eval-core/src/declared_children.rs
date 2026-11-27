//! The children a run has declared (contract 7.1), by step. A step declared
//! again with an equal declaration names the same child, and with a
//! different one is a conflict. What a node's answer keeps of them is their
//! steps.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::declaration::Declaration;
use crate::step::Step;

/// A run declaring a different declaration at `step` than it already
/// declared there.
pub(crate) struct Conflict {
    pub step: Step,
}

#[derive(Default)]
pub(crate) struct DeclaredChildren {
    by_step: BTreeMap<Step, Arc<Declaration>>,
}

impl DeclaredChildren {
    /// Whether `declaration` at `step` is a child these do not hold yet:
    /// `false` where they hold an equal one there, a conflict where they
    /// hold a different one.
    pub fn is_new(&self, step: &Step, declaration: &Declaration) -> Result<bool, Conflict> {
        match self.by_step.get(step) {
            None => Ok(true),
            Some(earlier) if **earlier == *declaration => Ok(false),
            Some(_) => Err(Conflict { step: step.clone() }),
        }
    }

    /// Holds the child `declaration` at `step` from now on, unless it is a
    /// conflict.
    pub fn declare(&mut self, step: &Step, declaration: &Arc<Declaration>) -> Result<(), Conflict> {
        if self.is_new(step, declaration)? {
            self.by_step.insert(step.clone(), Arc::clone(declaration));
        }
        Ok(())
    }

    /// Holds the children of `more` too, which were each checked against
    /// these and found no conflict.
    pub fn extend(&mut self, more: DeclaredChildren) {
        self.by_step.extend(more.by_step);
    }

    pub fn into_steps(self) -> ChildSteps {
        ChildSteps(self.by_step.into_keys().collect())
    }
}

/// The steps of the children a run declared, in order, each once.
#[derive(Clone)]
pub(crate) struct ChildSteps(Arc<[Step]>);

impl ChildSteps {
    pub fn none() -> Self {
        ChildSteps(Arc::new([]))
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn contains(&self, step: &Step) -> bool {
        self.0.binary_search(step).is_ok()
    }
}
