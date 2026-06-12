//! Topological sort for the embedded stdlib prelude.
//!
//! Provides [`compile_modules_topo`] — an in-memory, stable topological sort
//! over a set of `(module_name, source)` pairs, using `Declaration::Import`
//! edges for ordering and a gray/black DFS for cycle detection.

use std::collections::HashMap;

use reify_core::{Diagnostic, ModulePath};

use crate::CompiledModule;

/// Extract intra-set import edges for one module source.
///
/// Returns the indices (into `names`) of modules that this source imports.
/// Import targets not present in `names` are silently ignored — they represent
/// external or stdlib imports that drive no ordering here.
fn import_edges_for(source: &str, module_path: ModulePath, names: &[&str]) -> Vec<usize> {
    let name_to_idx: HashMap<&str, usize> =
        names.iter().enumerate().map(|(i, &n)| (n, i)).collect();
    let parsed = reify_syntax::parse(source, module_path);
    parsed
        .declarations
        .iter()
        .filter_map(|decl| {
            if let reify_ast::Declaration::Import(import) = decl {
                name_to_idx.get(import.path.as_str()).copied()
            } else {
                None
            }
        })
        .collect()
}

/// Three-color DFS state for cycle detection.
#[derive(Clone, Copy, PartialEq)]
enum DfsColor {
    White, // not yet visited
    Gray,  // in the current DFS path (back-edge target → cycle)
    Black, // fully processed
}

/// Helper that carries mutable DFS state across recursive calls.
struct TopoSorter<'a> {
    edges: &'a [Vec<usize>],
    names: &'a [&'a str],
    color: Vec<DfsColor>,
    stack: Vec<usize>, // current DFS path (for cycle chain reconstruction)
    result: Vec<usize>,
}

impl<'a> TopoSorter<'a> {
    fn dfs(&mut self, node: usize) -> Result<(), Diagnostic> {
        match self.color[node] {
            DfsColor::Black => return Ok(()),
            DfsColor::Gray => {
                // Back-edge: node is already in the current DFS path → cycle
                let cycle_start = self.stack.iter().position(|&s| s == node).unwrap_or(0);
                let chain: Vec<&str> = self.stack[cycle_start..]
                    .iter()
                    .map(|&i| self.names[i])
                    .collect();
                let mut arrow_chain = chain.join(" -> ");
                arrow_chain.push_str(" -> ");
                arrow_chain.push_str(self.names[node]);
                return Err(Diagnostic::error(format!(
                    "circular dependency detected: {}",
                    arrow_chain
                )));
            }
            DfsColor::White => {}
        }

        self.color[node] = DfsColor::Gray;
        self.stack.push(node);

        // Clone to avoid simultaneous borrow of self.edges and self (via dfs)
        let deps: Vec<usize> = self.edges[node].clone();
        for dep in deps {
            self.dfs(dep)?;
        }

        self.stack.pop();
        self.color[node] = DfsColor::Black;
        self.result.push(node);
        Ok(())
    }
}

/// Stable DFS post-order topological sort with cycle detection.
///
/// Returns a permutation of `0..sources.len()` such that every dependency
/// appears before its dependents.  Independent modules retain their input order
/// (DFS visits roots in input order; a node with no deps is appended
/// immediately).
///
/// Returns `Err(Diagnostic)` if a cycle is detected, with the chain named in
/// the message ("circular dependency detected: a -> b -> a").
fn stable_topo_sort(sources: &[(&str, &str)]) -> Result<Vec<usize>, Diagnostic> {
    let n = sources.len();
    let names: Vec<&str> = sources.iter().map(|(name, _)| *name).collect();

    // Build dependency edge lists
    let edges: Vec<Vec<usize>> = sources
        .iter()
        .map(|(name, source)| {
            let path = ModulePath::from_dotted(name).expect("valid dotted module name");
            import_edges_for(source, path, &names)
        })
        .collect();

    let mut sorter = TopoSorter {
        edges: &edges,
        names: &names,
        color: vec![DfsColor::White; n],
        stack: Vec::new(),
        result: Vec::with_capacity(n),
    };

    for i in 0..n {
        if sorter.color[i] == DfsColor::White {
            sorter.dfs(i)?;
        }
    }

    Ok(sorter.result)
}

/// Compile `sources` in topological order derived from their `import` declarations.
///
/// Each module is compiled against a growing prelude of all previously compiled
/// modules (dependencies first), using the same `parse_with_prelude_enums` +
/// `compile_with_prelude` loop as `load_stdlib`.
///
/// Returns `Ok(modules)` with per-module diagnostics attached. Modules are
/// returned in the order they were compiled (topo order, not input order).
///
/// The only structural `Err` is a cycle in the import graph — step-4 adds that.
pub(crate) fn compile_modules_topo(
    sources: &[(&str, &str)],
) -> Result<Vec<CompiledModule>, Diagnostic> {
    let topo_indices = stable_topo_sort(sources)?;

    let mut compiled_so_far: Vec<CompiledModule> = Vec::with_capacity(sources.len());

    for idx in topo_indices {
        let (module_name, source) = &sources[idx];

        let prelude_enum_names: Vec<&str> = compiled_so_far
            .iter()
            .flat_map(|m: &CompiledModule| m.enum_defs.iter().map(|e| e.name.as_str()))
            .collect();

        let parsed = reify_syntax::parse_with_prelude_enums(
            source,
            ModulePath::from_dotted(module_name).expect("valid dotted module name"),
            &prelude_enum_names,
        );

        let compiled = crate::compile_with_prelude(&parsed, &compiled_so_far);
        compiled_so_far.push(compiled);
    }

    Ok(compiled_so_far)
}

#[cfg(test)]
mod tests {
    use reify_core::{ModulePath, Severity};
    use crate::CompiledModule;
    use super::compile_modules_topo;

    /// SIGNAL #2 + stability: the real stdlib compiles clean through the topo path
    /// and output order equals input order (no imports → stable sort is identity).
    ///
    /// RED until step-6 extracts `stdlib_sources()` from `load_stdlib`.
    #[test]
    fn signal_2_real_stdlib_compiles_clean_and_order_is_stable() {
        use crate::stdlib_loader::stdlib_sources;
        use reify_core::Severity;

        let owned = stdlib_sources();
        // Build borrowed slice view for compile_modules_topo
        let set: Vec<(&str, &str)> = owned.iter().map(|(n, s)| (*n, s.as_str())).collect();

        let modules = compile_modules_topo(&set)
            .expect("compile_modules_topo must return Ok for the real stdlib (no cycles)");

        // (a) Signal #2: no Error-severity diagnostics across all modules
        for module in &modules {
            let errors: Vec<_> = module
                .diagnostics
                .iter()
                .filter(|d| d.severity == Severity::Error)
                .collect();
            assert!(
                errors.is_empty(),
                "stdlib module '{}' has Error-severity diagnostics via topo path: {:?}",
                module.path,
                errors
            );
        }

        // (b) Stability: output order equals input order (identity permutation because
        //     no production module declares `import`)
        let input_names: Vec<&str> = set.iter().map(|(n, _)| *n).collect();
        let output_names: Vec<String> =
            modules.iter().map(|m| format!("{}", m.path).replace('/', ".")).collect();
        assert_eq!(
            output_names.len(),
            input_names.len(),
            "topo output count must equal input count"
        );
        for (i, (input, output)) in input_names.iter().zip(output_names.iter()).enumerate() {
            assert_eq!(
                output, input,
                "stdlib module at position {} changed order: expected '{}', got '{}'",
                i, input, output
            );
        }
    }

    /// SIGNAL #3: a cyclic import pair is rejected with a "circular dependency" error.
    ///
    /// RED until step-4 adds gray/black cycle detection to the topo sort.
    #[test]
    fn signal_3_cyclic_import_pair_returns_err_with_circular_dependency_message() {
        let a_src = "import b\n\npub type A = Real";
        let b_src = "import a\n\npub type B = Real";

        let set: &[(&str, &str)] = &[("a", a_src), ("b", b_src)];

        let result = compile_modules_topo(set);
        let diag = result.expect_err(
            "compile_modules_topo must return Err for a cyclic module set",
        );

        assert_eq!(
            diag.severity,
            reify_core::Severity::Error,
            "cycle diagnostic must have Error severity"
        );
        assert!(
            diag.message.contains("circular dependency"),
            "cycle diagnostic message must contain 'circular dependency', got: {:?}",
            diag.message
        );
        // Both module names must appear in the chain
        assert!(
            diag.message.contains('a') && diag.message.contains('b'),
            "cycle diagnostic message must name both modules, got: {:?}",
            diag.message
        );
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
