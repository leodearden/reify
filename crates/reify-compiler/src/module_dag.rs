//! Module DAG: dependency graph construction, cycle detection, and topological ordering.
//!
//! Provides `ModuleResolver` for mapping import paths to filesystem paths,
//! and `ModuleDag` for building and traversing the module dependency graph.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use indexmap::IndexSet;

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

/// Tracks which stdlib source was committed on the first `std.*` resolution.
///
/// All `std.*` modules within a single `ModuleDag` instance must come from the
/// same source. Mixing filesystem-resolved and embedded modules is unsafe because
/// the embedded stdlib was compiled as a unit; using an embedded `std.materials.mechanical`
/// alongside a filesystem-resolved `std.units` can produce downstream type/trait mismatches.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum StdlibMode {
    /// All `std.*` modules resolved from the filesystem stdlib_root.
    FileSystem,
    /// All `std.*` modules resolved from the embedded compiled stdlib.
    Embedded,
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
    /// IndexSet preserves insertion order (= DFS traversal order), enabling
    /// the cycle error to show the actual import chain.
    in_progress: IndexSet<String>,
    /// Source committed on the first `std.*` resolution (all-or-nothing invariant).
    stdlib_mode: Option<StdlibMode>,
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
            in_progress: IndexSet::new(),
            stdlib_mode: None,
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
        if let Some(cycle_start) = self.in_progress.get_index_of(module_path) {
            // in_progress is ordered by DFS insertion, so slicing from cycle_start
            // gives only the cycle members, excluding any non-cycle ancestors.
            // Appending module_path at the end closes the cycle visually.
            let chain: Vec<&str> = self.in_progress[cycle_start..]
                .iter()
                .map(|s| s.as_str())
                .collect();
            let mut arrow_chain = chain.join(" -> ");
            arrow_chain.push_str(" -> ");
            arrow_chain.push_str(module_path);
            return Err(vec![Diagnostic::error(format!(
                "circular dependency detected: {}",
                arrow_chain,
            ))]);
        }

        // Detect std.* / bare std paths for embedded-stdlib fallback.
        // Filesystem lookup is always tried first; the embedded stdlib is only
        // consulted when the filesystem cannot resolve the module (e.g., no
        // stdlib_root directory on disk). This preserves the behaviour of
        // compile_project_stdlib_unit_collision_mentions_stdlib, which places a
        // real units.ri under stdlib_root and expects the filesystem version to win.
        let is_std_path = module_path == "std" || module_path.starts_with("std.");

        // Resolve to filesystem path, with embedded-stdlib fallback for std.* paths.
        let fs_path = match resolver.resolve_import_path(module_path) {
            Ok(path) if is_std_path => {
                // Filesystem resolved a std.* path.
                // All-or-nothing invariant: if a prior std.* was served from the
                // embedded stdlib, mixing in a filesystem-resolved module is unsafe.
                if self.stdlib_mode == Some(StdlibMode::Embedded) {
                    return Err(vec![Diagnostic::error(format!(
                        "partial stdlib overlay: '{}' resolved on the filesystem but earlier \
                         std.* imports were served from the embedded stdlib; either populate \
                         all stdlib modules under '{}' or remove that directory to use the \
                         embedded stdlib exclusively",
                        module_path,
                        resolver.stdlib_root.display(),
                    ))]);
                }
                // Commit to filesystem mode on first std.* filesystem success.
                if self.stdlib_mode.is_none() {
                    self.stdlib_mode = Some(StdlibMode::FileSystem);
                }
                path
            }
            Ok(path) => path,
            Err(fs_err) if is_std_path => {
                // Filesystem lookup failed for a std.* path.
                // All-or-nothing invariant: if a prior std.* was served from the
                // filesystem, falling back to embedded now would mix sources.
                if self.stdlib_mode == Some(StdlibMode::FileSystem) {
                    return Err(vec![Diagnostic::error(format!(
                        "partial stdlib overlay: '{}' not found on the filesystem under '{}' \
                         but earlier std.* imports were resolved from the filesystem; either \
                         add the missing module to '{}' or remove that directory to use the \
                         embedded stdlib exclusively",
                        module_path,
                        resolver.stdlib_root.display(),
                        resolver.stdlib_root.display(),
                    ))]);
                }
                // Commit to embedded mode and consult the embedded stdlib.
                if self.stdlib_mode.is_none() {
                    self.stdlib_mode = Some(StdlibMode::Embedded);
                }
                let target = reify_types::ModulePath::from_dotted(module_path);
                let stdlib = crate::stdlib_loader::load_stdlib();
                if let Some(idx) = stdlib.iter().position(|m| m.path == target) {
                    // Found in embedded stdlib. Insert all modules up to and including
                    // the target in topological order. The stdlib slice is itself
                    // topologically ordered: module at index i was compiled against
                    // all modules at indices 0..i. By inserting the full prefix we
                    // ensure transitive deps (e.g. std.units and std.si_units when
                    // std.materials.mechanical is requested) are present in both
                    // `modules` and `topo_order`, so consumers that iterate
                    // topo_order see a consistent view of all stdlib transitive deps.
                    for embedded in &stdlib[..=idx] {
                        let dotted = embedded.path.0.join(".");
                        if !self.modules.contains_key(&dotted) {
                            self.topo_order.push(dotted.clone());
                            self.modules.insert(dotted, embedded.clone());
                        }
                    }
                    return Ok(());
                }
                // Unknown std.* submodule — surface the original fs error so callers
                // get a clear diagnostic rather than a silent no-op.
                return Err(vec![fs_err]);
            }
            Err(fs_err) => return Err(vec![fs_err]),
        };

        // Read and parse
        let source = std::fs::read_to_string(&fs_path).map_err(|e| {
            vec![Diagnostic::error(format!(
                "failed to read module '{}' at '{}': {}",
                module_path,
                fs_path.display(),
                e,
            ))]
        })?;

        let parsed = reify_syntax::parse(&source, reify_types::ModulePath::from_dotted(module_path));

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
            // Single pass: compile each import dependency and collect its path.
            // Merges the previous two-pass pattern (compile + collect_import_preludes)
            // into one iteration over parsed.declarations.
            let mut import_paths: Vec<String> = Vec::new();
            for decl in &parsed.declarations {
                if let reify_syntax::Declaration::Import(import) = decl {
                    self.compile_module(&import.path, resolver)?;
                    import_paths.push(import.path.clone());
                }
            }

            // Topological ordering guarantees every import was just compiled and inserted.
            debug_assert!(
                import_paths.iter().all(|p| self.modules.contains_key(p.as_str())),
                "all imports must be compiled and in self.modules before prelude collection; \
                 this invariant is guaranteed by the DFS loop above"
            );

            // Build prelude slice from the collected import paths.
            // NB: self.modules is stable for shared borrowing here — no more mutations
            // until after compile_with_prelude_refs returns.
            let preludes: Vec<&CompiledModule> = import_paths
                .iter()
                .filter_map(|p| self.modules.get(p.as_str()))
                .collect();

            // Compile this module with prelude context so imported constraint defs are visible.
            Ok(crate::compile_with_prelude_refs(&parsed, &preludes))
        })();

        // Always remove from in-progress, whether the inner block succeeded or failed.
        // shift_remove preserves insertion order of remaining elements, which matters
        // for the cycle error message in sibling-import scenarios.
        self.in_progress.shift_remove(module_path);

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

    // Collect imported modules as prelude so their pub units (and other
    // exported definitions) are visible in the entry module.
    // Block-scope the preludes so the shared borrows of dag.modules are
    // dropped before the mutable borrow in dag.modules.insert below.
    let compiled_entry = {
        // Inline the same prelude-collection pattern used by compile_module:
        // iterate import declarations once, filter_map to look up compiled modules.
        // All imports were recursively compiled above, so every lookup succeeds.
        let preludes: Vec<&CompiledModule> = parsed
            .declarations
            .iter()
            .filter_map(|d| {
                if let reify_syntax::Declaration::Import(import) = d {
                    dag.modules.get(&import.path)
                } else {
                    None
                }
            })
            .collect();
        crate::compile_with_prelude_refs(&parsed, &preludes)
    };
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
