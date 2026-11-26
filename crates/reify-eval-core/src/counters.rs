//! What the core counts, for tests to assert on in place of time.

use std::collections::BTreeMap;

use crate::declaration::KindId;

#[derive(Debug, Default)]
pub struct Counters {
    body_runs: BTreeMap<KindId, u64>,
}

impl Counters {
    /// How many bodies of `kind` have been started.
    pub fn body_runs(&self, kind: KindId) -> u64 {
        self.body_runs.get(&kind).copied().unwrap_or(0)
    }

    pub(crate) fn count_body_run(&mut self, kind: KindId) {
        *self.body_runs.entry(kind).or_default() += 1;
    }
}
