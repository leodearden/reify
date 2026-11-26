//! What the core counts, for tests to assert on in place of time.

use std::collections::BTreeMap;

use crate::declaration::KindId;

#[derive(Debug, Default)]
pub struct Counters {
    body_runs: BTreeMap<KindId, u64>,
    sections: u64,
    paths_resolved: u64,
}

impl Counters {
    /// How many bodies of `kind` have been started.
    pub fn body_runs(&self, kind: KindId) -> u64 {
        self.body_runs.get(&kind).copied().unwrap_or(0)
    }

    /// How many sections have been started on a resource's thread.
    pub fn sections(&self) -> u64 {
        self.sections
    }

    /// How many times the core has worked out what a path is in a
    /// snapshot: a root from its snapshot input, any other path from its
    /// parent's final answer.
    pub fn paths_resolved(&self) -> u64 {
        self.paths_resolved
    }

    pub(crate) fn count_body_run(&mut self, kind: KindId) {
        *self.body_runs.entry(kind).or_default() += 1;
    }

    pub(crate) fn count_section(&mut self) {
        self.sections += 1;
    }

    pub(crate) fn count_path_resolved(&mut self) {
        self.paths_resolved += 1;
    }
}
