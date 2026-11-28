//! A typed path as the core path it names.

use std::fmt;

use reify_core::{InstancePath, PathStep};
use reify_eval_core::Path;

use crate::step_map;

/// Why a typed path names no core path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DemandError {
    /// No kind of this crate declares a child at `step` of `path`.
    NoNodeAt {
        path: Box<InstancePath>,
        step: Box<PathStep>,
    },
}

impl fmt::Display for DemandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DemandError::NoNodeAt { path, step } => write!(
                f,
                "`{path}`: no kind of this crate declares a child at that step, {step:?}"
            ),
        }
    }
}

impl std::error::Error for DemandError {}

pub(crate) fn core_path(path: &InstancePath) -> Result<Path, DemandError> {
    let root = Path::root(step_map::root(path.root()));
    path.steps().iter().try_fold(root, |at, step| {
        let steps = step_map::core_steps(step).ok_or_else(|| DemandError::NoNodeAt {
            path: Box::new(path.clone()),
            step: Box::new(step.clone()),
        })?;
        Ok(steps.into_iter().fold(at, |at, step| at.child(step)))
    })
}
