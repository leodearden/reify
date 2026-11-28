//! Re-export edges (`docs/prds/v0_6/resolution-unification.md` D-7): the
//! modules each module re-exports through `pub import`, and the closure a
//! module's import makes visible along them.

use std::collections::HashMap;
use std::sync::OnceLock;

use indexmap::IndexSet;
use reify_core::ModulePath;

use crate::types::CompiledModule;

/// Each module's re-export edges: its `pub import` targets in declaration
/// order, one per module. A module with no entry re-exports nothing.
#[derive(Debug, Default)]
pub(crate) struct ReExports {
    edges: HashMap<ModulePath, Vec<ModulePath>>,
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

    /// The edges of the embedded stdlib, built once.
    pub(crate) fn of_stdlib() -> &'static Self {
        static STDLIB: OnceLock<ReExports> = OnceLock::new();
        STDLIB.get_or_init(|| Self::of_compiled(crate::stdlib_loader::load_stdlib()))
    }

    /// Record `module`'s edges as it was compiled: every `pub import` it
    /// declares, with no `#cfg` filter, because the stdlib build compiles a
    /// module against all its declared imports. A path that is not a module
    /// path names no module, so it is no edge.
    pub(crate) fn record_compiled(&mut self, module: &CompiledModule) {
        let pub_targets = module
            .imports
            .iter()
            .filter(|import| import.is_pub)
            .filter_map(|import| ModulePath::from_dotted(&import.path).ok());
        self.record(module.path.clone(), pub_targets);
    }

    /// Record `module`'s edges from its `pub import` targets in declaration
    /// order: one edge per module, at its first occurrence.
    pub(crate) fn record(
        &mut self,
        module: ModulePath,
        pub_targets: impl IntoIterator<Item = ModulePath>,
    ) {
        let mut edges: Vec<ModulePath> = Vec::new();
        for target in pub_targets {
            if !edges.contains(&target) {
                edges.push(target);
            }
        }
        self.edges.insert(module, edges);
    }

    /// `path` followed, depth-first, by every module it re-exports, each once,
    /// at its first position.
    pub(crate) fn closure(&self, path: &ModulePath) -> Vec<ModulePath> {
        let mut reached = IndexSet::new();
        self.extend_closure(path, &mut reached);
        reached.into_iter().collect()
    }

    /// Append `path` and then, depth-first, its re-export closure to
    /// `reached`, skipping every module `reached` already holds.
    pub(crate) fn extend_closure(&self, path: &ModulePath, reached: &mut IndexSet<ModulePath>) {
        if !reached.insert(path.clone()) {
            return;
        }
        for target in self.edges.get(path).into_iter().flatten() {
            self.extend_closure(target, reached);
        }
    }
}
