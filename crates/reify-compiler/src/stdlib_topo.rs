//! The embedded stdlib build: a closed set of `(module_name, source)` pairs,
//! each module compiled against exactly the modules it declares as imports.
//!
//! [`compile_modules_topo`] derives a canonical topological order from the
//! `import` declarations alone, so the result does not depend on the order the
//! pairs are given in (NS-M3), and a module sees builtins plus the pub surfaces
//! of its own imports, never a sibling it does not import (NS-V3).

use std::collections::HashMap;
use std::fmt;

use reify_core::ModulePath;

use crate::CompiledModule;
use crate::compile_builder::pre_pass::{ExpectedPathOrigin, attach_module_path_diag};

/// Why a stdlib source set has no build order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StdlibGraphError {
    /// An import cycle: the dotted module names along it, the first repeated
    /// at the end to close it.
    Cycle { chain: Vec<String> },
    /// `importer` imports `target`, which is not a module of the set.
    UnknownImport { importer: String, target: String },
}

impl fmt::Display for StdlibGraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cycle { chain } => {
                write!(f, "circular dependency detected: {}", chain.join(" -> "))
            }
            Self::UnknownImport { importer, target } => write!(
                f,
                "module '{importer}' imports '{target}', which is not a module of the stdlib"
            ),
        }
    }
}

/// The modules of a source set and, per module, the set indices of its
/// imports in declaration order.
struct ImportGraph<'s> {
    names: Vec<&'s str>,
    paths: Vec<ModulePath>,
    imports: Vec<Vec<usize>>,
}

impl<'s> ImportGraph<'s> {
    fn of(sources: &[(&'s str, &str)]) -> Result<Self, StdlibGraphError> {
        let names: Vec<&str> = sources.iter().map(|(name, _)| *name).collect();
        let index_of: HashMap<&str, usize> = names
            .iter()
            .enumerate()
            .map(|(i, &name)| (name, i))
            .collect();
        let paths: Vec<ModulePath> = names
            .iter()
            .map(|name| ModulePath::from_dotted(name).expect("valid dotted module name"))
            .collect();
        let imports = sources
            .iter()
            .zip(&paths)
            .map(|((name, source), path)| import_indices(name, source, path, &index_of))
            .collect::<Result<_, _>>()?;
        Ok(Self {
            names,
            paths,
            imports,
        })
    }

    /// Every module, each after all of its imports: a DFS post-order whose
    /// roots are taken in ascending dotted name and whose edges are followed
    /// in import-declaration order, so the order depends only on the set.
    ///
    /// The DFS is recursive; its depth is the longest import chain in the set.
    /// Default thread stacks handle chains of several thousand modules; past a
    /// few hundred in a constrained-stack context, convert [`TopoSorter::visit`]
    /// to an explicit work stack.
    fn canonical_order(&self) -> Result<Vec<usize>, StdlibGraphError> {
        let mut roots: Vec<usize> = (0..self.names.len()).collect();
        roots.sort_by_key(|&i| self.names[i]);
        let mut sorter = TopoSorter {
            graph: self,
            color: vec![DfsColor::White; self.names.len()],
            path: Vec::new(),
            order: Vec::with_capacity(self.names.len()),
        };
        for root in roots {
            sorter.visit(root)?;
        }
        Ok(sorter.order)
    }
}

/// The set indices of the modules `source` imports, in declaration order.
/// The source is parsed here prelude-agnostically, for its imports only;
/// [`compile_against_imports`] parses it again with its imports' enum names.
fn import_indices(
    name: &str,
    source: &str,
    path: &ModulePath,
    index_of: &HashMap<&str, usize>,
) -> Result<Vec<usize>, StdlibGraphError> {
    let parsed = reify_syntax::parse(source, path.clone());
    let mut imports: Vec<usize> = Vec::new();
    for declaration in &parsed.declarations {
        let reify_ast::Declaration::Import(import) = declaration else {
            continue;
        };
        let target = index_of.get(import.path.as_str()).copied().ok_or_else(|| {
            StdlibGraphError::UnknownImport {
                importer: name.to_string(),
                target: import.path.clone(),
            }
        })?;
        if !imports.contains(&target) {
            imports.push(target);
        }
    }
    Ok(imports)
}

/// Three-color DFS state for cycle detection.
#[derive(Clone, Copy, PartialEq)]
enum DfsColor {
    White, // not yet visited
    Gray,  // on the current DFS path (reaching it again closes a cycle)
    Black, // fully processed
}

struct TopoSorter<'g, 's> {
    graph: &'g ImportGraph<'s>,
    color: Vec<DfsColor>,
    path: Vec<usize>,
    order: Vec<usize>,
}

impl TopoSorter<'_, '_> {
    fn visit(&mut self, node: usize) -> Result<(), StdlibGraphError> {
        match self.color[node] {
            DfsColor::Black => return Ok(()),
            DfsColor::Gray => {
                let start = self
                    .path
                    .iter()
                    .position(|&on_path| on_path == node)
                    .expect("a gray node is on the DFS path");
                let chain = self.path[start..]
                    .iter()
                    .chain(std::iter::once(&node))
                    .map(|&i| self.graph.names[i].to_string())
                    .collect();
                return Err(StdlibGraphError::Cycle { chain });
            }
            DfsColor::White => {}
        }
        self.color[node] = DfsColor::Gray;
        self.path.push(node);
        for &import in &self.graph.imports[node] {
            self.visit(import)?;
        }
        self.path.pop();
        self.color[node] = DfsColor::Black;
        self.order.push(node);
        Ok(())
    }
}

/// Compile `sources`, each module against exactly its declared imports, in
/// declaration order (NS-P1 import precedence).
///
/// Returns the modules in the canonical order of [`ImportGraph::canonical_order`],
/// with their diagnostics attached, including the module-path check against
/// each module's registered name. `Err` when the imports have no order: a
/// cycle, or an import of a module outside the set.
///
/// Never re-enters the stdlib cache (`parse_with_stdlib`, `load_stdlib_context`):
/// this runs inside `load_stdlib`'s `OnceLock` initialiser, so that would deadlock.
pub(crate) fn compile_modules_topo(
    sources: &[(&str, &str)],
) -> Result<Vec<CompiledModule>, StdlibGraphError> {
    let graph = ImportGraph::of(sources)?;
    let order = graph.canonical_order()?;

    let mut compiled: Vec<Option<CompiledModule>> = sources.iter().map(|_| None).collect();
    for &idx in &order {
        let module = {
            let imports: Vec<&CompiledModule> = graph.imports[idx]
                .iter()
                .map(|&import| {
                    compiled[import]
                        .as_ref()
                        .expect("canonical order compiles every import first")
                })
                .collect();
            compile_against_imports(
                graph.names[idx],
                sources[idx].1,
                &graph.paths[idx],
                &imports,
            )
        };
        compiled[idx] = Some(module);
    }

    Ok(order
        .into_iter()
        .map(|idx| compiled[idx].take().expect("each module is compiled once"))
        .collect())
}

fn compile_against_imports(
    name: &str,
    source: &str,
    path: &ModulePath,
    imports: &[&CompiledModule],
) -> CompiledModule {
    let import_enum_names: Vec<&str> = imports
        .iter()
        .flat_map(|module| module.enum_defs.iter().map(|e| e.name.as_str()))
        .collect();
    let parsed = reify_syntax::parse_with_prelude_enums(source, path.clone(), &import_enum_names);
    assert!(
        parsed.errors.is_empty(),
        "module '{}' has parse errors: {:?}",
        name,
        parsed.errors
    );
    let mut module = crate::compile_with_prelude_refs(&parsed, imports);
    attach_module_path_diag(&mut module, &parsed, ExpectedPathOrigin::StdlibRegistration);
    module
}

#[cfg(test)]
mod tests {
    use super::{StdlibGraphError, compile_modules_topo};
    use crate::CompiledModule;
    use reify_core::{ModulePath, Severity};

    /// SIGNAL #3: a cyclic import pair is rejected with a typed cycle naming
    /// both modules.
    #[test]
    fn signal_3_cyclic_import_pair_returns_a_cycle_naming_both_modules() {
        let set: &[(&str, &str)] = &[
            ("a", "import b\n\npub type A = Real"),
            ("b", "import a\n\npub type B = Real"),
        ];

        let Err(StdlibGraphError::Cycle { chain }) = compile_modules_topo(set) else {
            panic!("compile_modules_topo must return a Cycle for a cyclic module set");
        };
        assert!(
            chain.iter().any(|m| m == "a") && chain.iter().any(|m| m == "b"),
            "the cycle chain must name both modules, got: {chain:?}"
        );
        assert_eq!(chain.first(), chain.last(), "the chain closes the cycle");
    }

    /// SIGNAL #1: a shared surface type declared AFTER its consumers in input order
    /// is reachable from both early and late consumers when compiled via
    /// `compile_modules_topo`.
    ///
    /// Companion assertion: naive compilation in input order (early before shared)
    /// produces an Error on `early` — proving the topo mechanism is load-bearing.
    #[test]
    fn signal_1_shared_surface_type_reachable_from_early_and_late() {
        let early_src = "import shared\n\nstructure def E {\n    param v : Vec3\n}";
        let shared_src = "pub type Vec3 = Real";
        let late_src = "import shared\n\nstructure def L {\n    param w : Vec3\n}";

        // Input order: early BEFORE shared (naive compilation fails for early)
        let set: &[(&str, &str)] = &[
            ("early", early_src),
            ("shared", shared_src),
            ("late", late_src),
        ];

        // Topo path must succeed
        let modules = compile_modules_topo(set)
            .expect("compile_modules_topo must return Ok for a DAG with no cycle");

        // No Error-severity diagnostics across any returned module
        for module in &modules {
            let errors: Vec<_> = module
                .diagnostics
                .iter()
                .filter(|d| d.severity == Severity::Error)
                .collect();
            assert!(
                errors.is_empty(),
                "module '{}' has Error-severity diagnostics via topo path: {:?}",
                module.path,
                errors
            );
        }

        // `early` specifically resolves Vec3 (no Error mentioning Vec3)
        let early_mod = modules
            .iter()
            .find(|m| format!("{}", m.path) == "early")
            .expect("expected 'early' in topo result");
        let vec3_errors: Vec<_> = early_mod
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error && d.message.contains("Vec3"))
            .collect();
        assert!(
            vec3_errors.is_empty(),
            "'early' has Vec3-related Error diagnostics via topo path: {:?}",
            vec3_errors
        );

        // COMPANION: naive input-order compilation makes `early` fail Vec3 resolution
        let mut naive_modules: Vec<CompiledModule> = Vec::new();
        for (module_name, source) in set {
            let prelude_enum_names: Vec<&str> = naive_modules
                .iter()
                .flat_map(|m: &CompiledModule| m.enum_defs.iter().map(|e| e.name.as_str()))
                .collect();
            let parsed = reify_syntax::parse_with_prelude_enums(
                source,
                ModulePath::from_dotted(module_name).expect("valid dotted path"),
                &prelude_enum_names,
            );
            let compiled = crate::compile_with_prelude(&parsed, &naive_modules);
            naive_modules.push(compiled);
        }
        let naive_early = naive_modules
            .iter()
            .find(|m| format!("{}", m.path) == "early")
            .expect("expected 'early' in naive result");
        let naive_errors: Vec<_> = naive_early
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            !naive_errors.is_empty(),
            "naive compilation: expected 'early' to have Error-severity diagnostics \
             (Vec3 unresolved before shared), but got none — \
             the topo mechanism would then be non-load-bearing"
        );
    }
}
