//! Module DAG: dependency graph construction, cycle detection, and topological ordering.
//!
//! Provides `ModuleResolver` for mapping import paths to filesystem paths,
//! and `ModuleDag` for building and traversing the module dependency graph.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use reify_types::Diagnostic;

use crate::CompiledModule;

/// Resolves import dot-paths to filesystem paths.
///
/// Conventions:
/// - `std.*` imports resolve to `<stdlib_root>/...`
/// - Other imports resolve relative to `<project_root>/...`
/// - Dots become path separators
/// - Tries `<path>.ri` first, then `<path>/mod.ri`
pub struct ModuleResolver {
    /// Root of the project (for non-std imports).
    pub project_root: PathBuf,
    /// Root of the standard library (for `std.*` imports).
    pub stdlib_root: PathBuf,
}

impl ModuleResolver {
    pub fn new(project_root: impl Into<PathBuf>, stdlib_root: impl Into<PathBuf>) -> Self {
        Self {
            project_root: project_root.into(),
            stdlib_root: stdlib_root.into(),
        }
    }

    /// Resolve a dot-separated import path to a filesystem path.
    ///
    /// Returns the resolved `PathBuf` or an error diagnostic.
    pub fn resolve_import_path(&self, import_path: &str) -> Result<PathBuf, Diagnostic> {
        let segments: Vec<&str> = import_path.split('.').collect();
        if segments.is_empty() {
            return Err(Diagnostic::error("empty import path".to_string()));
        }

        // Determine base directory: std.* → stdlib_root, else → project_root
        let (base, path_segments) = if segments[0] == "std" {
            (&self.stdlib_root, &segments[1..])
        } else {
            (&self.project_root, &segments[..])
        };

        // Build filesystem path from remaining segments
        let mut fs_path = base.to_path_buf();
        for seg in path_segments {
            fs_path.push(seg);
        }

        // Try <path>.ri first
        let ri_path = fs_path.with_extension("ri");
        if ri_path.exists() {
            return Ok(ri_path);
        }

        // Try <path>/mod.ri (directory module)
        let mod_path = fs_path.join("mod.ri");
        if mod_path.exists() {
            return Ok(mod_path);
        }

        Err(Diagnostic::error(format!(
            "module '{}' not found: tried '{}' and '{}'",
            import_path,
            ri_path.display(),
            mod_path.display()
        )))
    }
}

/// The module dependency DAG.
///
/// Tracks compiled modules, detects cycles, and provides topological ordering.
pub struct ModuleDag {
    /// Compiled modules keyed by canonical path.
    pub modules: HashMap<String, CompiledModule>,
    /// Post-order traversal (leaves first) — topological sort.
    pub topo_order: Vec<String>,
    /// Modules currently being compiled (for cycle detection).
    in_progress: HashSet<String>,
}

impl Default for ModuleDag {
    fn default() -> Self {
        Self::new()
    }
}

impl ModuleDag {
    pub fn new() -> Self {
        Self {
            modules: HashMap::new(),
            topo_order: Vec::new(),
            in_progress: HashSet::new(),
        }
    }

    /// Compile a module and all its transitive dependencies.
    ///
    /// Performs DFS with cycle detection. Returns diagnostics on error.
    pub fn compile_module(
        &mut self,
        module_path: &str,
        resolver: &ModuleResolver,
    ) -> Result<(), Vec<Diagnostic>> {
        // Already compiled — skip
        if self.modules.contains_key(module_path) {
            return Ok(());
        }

        // Cycle detection
        if self.in_progress.contains(module_path) {
            let cycle: Vec<&str> = self.in_progress.iter().map(|s| s.as_str()).collect();
            return Err(vec![Diagnostic::error(format!(
                "circular dependency detected: {} -> {}",
                cycle.join(" -> "),
                module_path,
            ))]);
        }

        // Resolve to filesystem path
        let fs_path = resolver
            .resolve_import_path(module_path)
            .map_err(|d| vec![d])?;

        // Read and parse
        let source = std::fs::read_to_string(&fs_path).map_err(|e| {
            vec![Diagnostic::error(format!(
                "failed to read module '{}' at '{}': {}",
                module_path,
                fs_path.display(),
                e,
            ))]
        })?;

        let module_path_type =
            reify_types::ModulePath::new(module_path.split('.').map(|s| s.to_string()).collect());
        let parsed = reify_syntax::parse(&source, module_path_type);

        if !parsed.errors.is_empty() {
            return Err(parsed
                .errors
                .iter()
                .map(|e| Diagnostic::error(e.message.clone()))
                .collect());
        }

        // Mark in-progress for cycle detection
        self.in_progress.insert(module_path.to_string());

        // Use inner closure to guarantee in_progress cleanup on all exit paths
        let result = (|| -> Result<CompiledModule, Vec<Diagnostic>> {
            // Recursively compile dependencies
            for decl in &parsed.declarations {
                if let reify_syntax::Declaration::Import(import) = decl {
                    self.compile_module(&import.path, resolver)?;
                }
            }

            // Collect prelude modules (already-compiled imports) for constraint def propagation.
            let preludes: Vec<CompiledModule> = parsed
                .declarations
                .iter()
                .filter_map(|d| {
                    if let reify_syntax::Declaration::Import(import) = d {
                        self.modules.get(&import.path).cloned()
                    } else {
                        None
                    }
                })
                .collect();

            // Compile this module with prelude context so imported constraint defs are visible.
            Ok(crate::compile_with_prelude(&parsed, &preludes))
        })();

        // Always remove from in-progress, whether the inner block succeeded or failed
        self.in_progress.remove(module_path);

        // Propagate error after cleanup
        let compiled = result?;

        // Record in post-order (only on success)
        self.topo_order.push(module_path.to_string());
        self.modules.insert(module_path.to_string(), compiled);

        Ok(())
    }
}

/// Compile a project starting from an entry file.
///
/// Builds the full module DAG, detects cycles, and returns modules in
/// topological order (dependencies before dependents).
pub fn compile_project(
    entry_path: &Path,
    resolver: &ModuleResolver,
) -> Result<Vec<CompiledModule>, Vec<Diagnostic>> {
    // Read and parse entry file
    let source = std::fs::read_to_string(entry_path).map_err(|e| {
        vec![Diagnostic::error(format!(
            "failed to read entry file '{}': {}",
            entry_path.display(),
            e,
        ))]
    })?;

    let entry_name = entry_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("main");
    let module_path = reify_types::ModulePath::single(entry_name);
    let parsed = reify_syntax::parse(&source, module_path);

    if !parsed.errors.is_empty() {
        return Err(parsed
            .errors
            .iter()
            .map(|e| Diagnostic::error(e.message.clone()))
            .collect());
    }

    let mut dag = ModuleDag::new();

    // Recursively compile all imports
    for decl in &parsed.declarations {
        if let reify_syntax::Declaration::Import(import) = decl {
            dag.compile_module(&import.path, resolver)?;
        }
    }

    // Compile the entry module itself
    let compiled_entry = crate::compile(&parsed);
    let entry_key = entry_name.to_string();
    dag.topo_order.push(entry_key.clone());
    dag.modules.insert(entry_key, compiled_entry);

    // Return modules in topological order
    let modules: Vec<CompiledModule> = dag
        .topo_order
        .iter()
        .filter_map(|key| dag.modules.remove(key))
        .collect();

    Ok(modules)
}
