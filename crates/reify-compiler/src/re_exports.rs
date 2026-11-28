//! Re-export edges (`docs/prds/v0_6/resolution-unification.md` D-7): the
//! modules each module re-exports through `pub import`, and the closure a
//! module's import makes visible along them.

use std::collections::HashMap;

use indexmap::IndexSet;

use crate::types::CompiledModule;

/// Each module's re-export edges, keyed by dotted module path: its `pub
/// import` targets in declaration order, one per path. A module with no entry
/// re-exports nothing.
#[derive(Debug, Default)]
pub(crate) struct ReExports {
    edges: HashMap<String, Vec<String>>,
}

impl ReExports {
    /// The edges of every module in `modules`, each as it was compiled.
    pub(crate) fn of_compiled<'m>(modules: impl IntoIterator<Item = &'m CompiledModule>) -> Self {
        let mut re_exports = Self::default();
        for module in modules {
            re_exports.record_compiled(module);
        }
        re_exports
    }

    /// Record `module`'s edges as it was compiled: every `pub import` it
    /// declares, with no `#cfg` filter, because the stdlib build compiles a
    /// module against all its declared imports.
    pub(crate) fn record_compiled(&mut self, module: &CompiledModule) {
        let pub_targets = module
            .imports
            .iter()
            .filter(|import| import.is_pub)
            .map(|import| import.path.as_str());
        self.record(module.path.to_dotted(), pub_targets);
    }

    /// Record `module`'s edges from its `pub import` target paths in
    /// declaration order: one edge per path, at its first occurrence.
    pub(crate) fn record<'a>(
        &mut self,
        module: String,
        pub_targets: impl IntoIterator<Item = &'a str>,
    ) {
        let mut edges: Vec<String> = Vec::new();
        for target in pub_targets {
            if !edges.iter().any(|edge| edge == target) {
                edges.push(target.to_string());
            }
        }
        self.edges.insert(module, edges);
    }

    /// `path` followed, depth-first, by every module it re-exports, each once,
    /// at its first position.
    pub(crate) fn closure(&self, path: &str) -> Vec<String> {
        let mut reached = IndexSet::new();
        self.extend_closure(path, &mut reached);
        reached.into_iter().collect()
    }

    /// Append `path` and then, depth-first, its re-export closure to
    /// `reached`, skipping every module `reached` already holds.
    pub(crate) fn extend_closure(&self, path: &str, reached: &mut IndexSet<String>) {
        if !reached.insert(path.to_string()) {
            return;
        }
        for target in self.edges.get(path).into_iter().flatten() {
            self.extend_closure(target, reached);
        }
    }
}
