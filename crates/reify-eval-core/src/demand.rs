//! What a request asks for: a set of paths (contract 2.5).

use std::collections::BTreeSet;

use crate::path::Path;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Demand {
    paths: BTreeSet<Path>,
}

impl Demand {
    pub fn of(paths: impl IntoIterator<Item = Path>) -> Self {
        Self {
            paths: paths.into_iter().collect(),
        }
    }

    pub(crate) fn into_paths(self) -> impl Iterator<Item = Path> {
        self.paths.into_iter()
    }
}
