//! Module DAG: dependency graph construction, cycle detection, and topological ordering.
//!
//! Provides `ModuleResolver` for mapping import paths to filesystem paths,
//! and `ModuleDag` for building and traversing the module dependency graph.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use indexmap::IndexSet;

use reify_core::{Diagnostic, DiagnosticLabel, ModulePathParseError, SourceSpan};

use crate::cfg::{CfgSet, cfg_satisfied};
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
    /// Active compile-time configuration for evaluating `#cfg(...)` predicates.
    /// `CfgSet::default()` means no cfg constraints — all imports are followed.
    cfg: CfgSet,
}

/// Build an "invalid module path" [`Diagnostic`] vec.
///
/// Returns a singleton `Vec<Diagnostic>` so call sites can use it directly with
/// `.map_err(|e| diag_invalid_path(path, e))?` where the outer `Result`
/// error type is `Vec<Diagnostic>`.
///
/// The context phrase `"resolving import"` is inlined because both current call
/// sites use that identical literal (YAGNI: if a future call site needs different
/// wording, re-introduce the `context_phrase: &str` parameter then).
fn diag_invalid_path(path: &str, e: ModulePathParseError) -> Vec<Diagnostic> {
    vec![Diagnostic::error(format!(
        "invalid module path while resolving import '{}': {}",
        path, e
    ))]
}

/// Identifies which call site triggered a filesystem-over-embedded partial overlay.
///
/// Carried by [`OverlayDirection::FsOverEmbedded`] so that the exact diagnostic
/// wording for each call site lives inside [`partial_overlay_diag`] rather than
/// being passed in as a free-form `&str` from the call sites.
#[derive(Debug)]
enum CommitSite {
    /// Conflict detected at the entry guard (before any import recursion).
    Entry,
    /// Conflict detected after recursion: a transitive import committed Embedded
    /// mode while the outer module was resolving from the filesystem.
    Transitive,
}

/// Direction of a partial stdlib overlay conflict.
///
/// Used by [`partial_overlay_diag`] to select the appropriate diagnostic wording.
/// The two directions warrant different user guidance:
/// - [`FsOverEmbedded`]: filesystem module arrived after embedded was committed — user
///   must populate *all* stdlib modules (adding just one won't cure the prior embedded
///   commit) or remove the directory entirely.
/// - [`EmbeddedOverFs`]: embedded fallback triggered after filesystem mode was committed
///   because a module is missing from `stdlib_root` — user can fix by adding *that
///   specific module*.
#[derive(Debug)]
enum OverlayDirection {
    /// A `std.*` module resolved on the filesystem, but an earlier `std.*` import
    /// was served from the embedded stdlib.
    FsOverEmbedded { commit_site: CommitSite },
    /// A `std.*` module was not found on the filesystem, but an earlier `std.*` import
    /// was resolved from the filesystem.
    EmbeddedOverFs,
}

/// Shared prefix for all partial-stdlib-overlay diagnostics.
///
/// Referenced by both match arms in [`partial_overlay_diag`] so the prefix
/// string cannot drift between arms.
const OVERLAY_PREFIX: &str = "partial stdlib overlay";

/// Shared suffix for all partial-stdlib-overlay diagnostics.
///
/// Referenced by both match arms in [`partial_overlay_diag`] so the remediation
/// tail cannot drift between arms.
const OVERLAY_SUFFIX: &str = "or remove that directory to use the embedded stdlib exclusively";

/// Build a "partial stdlib overlay" [`Diagnostic`].
///
/// Consolidates three call sites in `compile_module` — the entry guard
/// (EmbeddedOverFs or FsOverEmbedded when Embedded is already committed) and the
/// deferred-commit block (FsOverEmbedded when a transitive import committed Embedded
/// during recursion). Each direction produces distinct remediation guidance; both share
/// [`OVERLAY_PREFIX`] and [`OVERLAY_SUFFIX`], preventing future wording drift between
/// sites.
///
/// The rendered message embeds a stable, machine-parsable kind marker immediately after
/// the prefix: `(fs-over-embedded/entry)`, `(fs-over-embedded/transitive)`, or
/// `(embedded-over-fs)`. The marker is structurally distinct from prose (slash+hyphen
/// form, parenthesised) so tests and downstream tooling can match it without depending
/// on natural-language phrasing.
fn partial_overlay_diag(
    module_path: &str,
    direction: OverlayDirection,
    stdlib_root: &Path,
) -> Diagnostic {
    match direction {
        OverlayDirection::FsOverEmbedded { commit_site } => {
            let (kind_marker, context) = match commit_site {
                CommitSite::Entry => (
                    "fs-over-embedded/entry",
                    "earlier std.* imports were served from the embedded stdlib",
                ),
                CommitSite::Transitive => (
                    "fs-over-embedded/transitive",
                    "a transitive std.* import was served from the embedded stdlib",
                ),
            };
            Diagnostic::error(format!(
                "{} ({}): '{}' resolved on the filesystem but {}; \
                 either populate all stdlib modules under '{}' {}",
                OVERLAY_PREFIX,
                kind_marker,
                module_path,
                context,
                stdlib_root.display(),
                OVERLAY_SUFFIX,
            ))
        }
        OverlayDirection::EmbeddedOverFs => Diagnostic::error(format!(
            "{} (embedded-over-fs): '{}' not found on the filesystem under '{}' \
             but earlier std.* imports were resolved from the filesystem; either \
             add the missing module to '{}' {}",
            OVERLAY_PREFIX,
            module_path,
            stdlib_root.display(),
            stdlib_root.display(),
            OVERLAY_SUFFIX,
        )),
    }
}

/// Attach a module-path declaration diagnostic (§7.1/§7.2) to `compiled` if warranted.
///
/// Encapsulates the `check_module_path_decl` call and diagnostic push shared by the
/// user-module entry points in this file. Call sites that can receive stdlib modules
/// (i.e. `compile_module`) must guard with `!is_std_path` before calling this.
fn attach_module_path_diag(
    compiled: &mut CompiledModule,
    parsed: &reify_ast::ParsedModule,
) {
    if let Some(diag) = crate::compile_builder::pre_pass::check_module_path_decl(
        parsed.declared_module_path.as_ref(),
        &parsed.path,
    ) {
        compiled.diagnostics.push(diag);
    }
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
            cfg: CfgSet::default(),
        }
    }

    /// Construct a `ModuleDag` with an active `CfgSet` for `#cfg(...)` gating.
    ///
    /// Imports whose predicates are unsatisfied against `cfg` are skipped (not
    /// recursed into, not added to the DAG, not included in any prelude).
    pub fn with_cfg(cfg: CfgSet) -> Self {
        Self {
            cfg,
            ..Self::new()
        }
    }

    /// Compile a module and all its transitive dependencies.
    ///
    /// Performs DFS with cycle detection. Returns diagnostics on error.
    ///
    /// Imports with `#cfg(...)` predicates that are not satisfied by `self.cfg`
    /// are skipped entirely (not recursed into, not included in the prelude).
    ///
    /// **Stdlib resolution precedence for `std.*` paths:**
    ///
    /// 1. The filesystem resolver is always tried first. If `stdlib_root` contains
    ///    the module (e.g. a user-supplied override), the filesystem version wins.
    /// 2. If the filesystem lookup fails, the DAG falls back to the pre-compiled
    ///    modules returned by `stdlib_loader::load_stdlib()` (embedded copy). This
    ///    ensures `import std.units` resolves correctly even when no local stdlib
    ///    directory exists.
    ///
    /// **All-or-nothing invariant (enforced by `stdlib_mode`):**
    ///
    /// On the first `std.*` resolution, the DAG commits to one source (filesystem
    /// or embedded). Any subsequent `std.*` import that would resolve via the other
    /// source emits a `"partial stdlib overlay"` diagnostic naming the offending
    /// module and the stdlib_root path. Users must either populate the stdlib dir
    /// fully or remove it to use the embedded stdlib exclusively. This prevents
    /// silent type/trait mismatches that arise when the embedded stdlib modules are
    /// mixed with a partial filesystem overlay.
    ///
    /// **One-shot-on-error semantics:**
    ///
    /// When `compile_module` returns `Err`, the `ModuleDag` is left in a partially-
    /// populated state: any modules compiled *before* the error remain in
    /// `self.modules` and `self.topo_order`, and `stdlib_mode` reflects whatever
    /// was committed during the failed subtree. Callers should treat a `ModuleDag`
    /// that has ever returned `Err` as one-shot — discard it and construct a fresh
    /// `ModuleDag::new()` for subsequent attempts rather than retrying on the same
    /// instance.
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

        // `commit_fs_mode` is set to true in the Ok-and-std branch when we are
        // about to commit filesystem mode for the first time. We defer the actual
        // write to `self.stdlib_mode` until after the module body compiles
        // successfully so that a parse or compile failure does not taint the mode
        // for subsequent (legitimate) calls.
        let mut commit_fs_mode = false;

        // Resolve to filesystem path, with embedded-stdlib fallback for std.* paths.
        let fs_path = match resolver.resolve_import_path(module_path) {
            Ok(path) if is_std_path => {
                // Filesystem resolved a std.* path.
                // All-or-nothing invariant: if a prior std.* was served from the
                // embedded stdlib, mixing in a filesystem-resolved module is unsafe.
                if self.stdlib_mode == Some(StdlibMode::Embedded) {
                    return Err(vec![partial_overlay_diag(
                        module_path,
                        OverlayDirection::FsOverEmbedded {
                            commit_site: CommitSite::Entry,
                        },
                        &resolver.stdlib_root,
                    )]);
                }
                // Defer committing filesystem mode until after successful compile so
                // that a parse/compile failure does not taint the mode (a failed
                // compile produces no DAG entry, so no std.* module was actually
                // loaded from the filesystem yet).
                commit_fs_mode = self.stdlib_mode.is_none();
                path
            }
            Ok(path) => path,
            Err(fs_err) if is_std_path => {
                // Filesystem lookup failed for a std.* path.
                // All-or-nothing invariant: if a prior std.* was served from the
                // filesystem, falling back to embedded now would mix sources.
                if self.stdlib_mode == Some(StdlibMode::FileSystem) {
                    return Err(vec![partial_overlay_diag(
                        module_path,
                        OverlayDirection::EmbeddedOverFs,
                        &resolver.stdlib_root,
                    )]);
                }
                // Consult the embedded stdlib. We commit to Embedded mode only after
                // confirming the module exists there — an unknown std.* path (e.g. a
                // typo like "std.unknonwn") must not taint the mode for subsequent
                // valid imports.
                let target = reify_core::ModulePath::from_dotted(module_path)
                    .map_err(|e| diag_invalid_path(module_path, e))?;
                let stdlib = crate::stdlib_loader::load_stdlib();
                if let Some(idx) = stdlib.iter().position(|m| m.path == target) {
                    // Commit embedded mode now that we know the module is present.
                    if self.stdlib_mode.is_none() {
                        self.stdlib_mode = Some(StdlibMode::Embedded);
                    }
                    // Found in embedded stdlib. Insert all modules up to and including
                    // the target in topological order. The stdlib slice is itself
                    // topologically ordered: module at index i was compiled against
                    // all modules at indices 0..i.
                    //
                    // Insert-prefix invariant: if stdlib[k] is present in self.modules,
                    // then stdlib[0..=k] are all present (because any prior fallback call
                    // that reached index k must have inserted the full prefix 0..=k).
                    // We exploit this invariant via a backward walk: scan from idx
                    // downward to find the largest already-present index j, then insert
                    // stdlib[j+1..=idx] unconditionally (no per-entry contains_key).
                    // This is faster on repeat calls with overlapping prefixes and makes
                    // the invariant explicit in code.
                    let start = (0..=idx)
                        .rev()
                        .find(|&j| self.modules.contains_key(&stdlib[j].path.0.join(".")))
                        .map(|j| j + 1)
                        .unwrap_or(0);
                    for embedded in &stdlib[start..=idx] {
                        let dotted = embedded.path.0.join(".");
                        self.topo_order.push(dotted.clone());
                        self.modules.insert(dotted, embedded.clone());
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

        let parsed = reify_syntax::parse(
            &source,
            reify_core::ModulePath::from_dotted(module_path)
                .map_err(|e| diag_invalid_path(module_path, e))?,
        );

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
            // Imports with unsatisfied `#cfg(...)` predicates are skipped entirely.
            let mut import_paths: Vec<String> = Vec::new();
            for decl in &parsed.declarations {
                if let reify_ast::Declaration::Import(import) = decl {
                    if !import_cfg_satisfied(import, &self.cfg) {
                        continue;
                    }
                    self.compile_module(&import.path, resolver)?;
                    import_paths.push(import.path.clone());
                }
            }

            // Block-scope the shared borrows of self.modules so they are
            // dropped before the later self.modules.insert call.
            let compiled = {
                // Topological ordering guarantees every import was just compiled and inserted.
                // Use .map().expect() so a violation is loud in both debug and release builds.
                let preludes: Vec<&CompiledModule> = import_paths
                    .iter()
                    .map(|p| {
                        self.modules
                            .get(p.as_str())
                            .expect("invariant: import compiled before prelude collection")
                    })
                    .collect();
                crate::compile_with_prelude_refs(&parsed, &preludes)
            };
            Ok(compiled)
        })();

        // Always remove from in-progress, whether the inner block succeeded or failed.
        // shift_remove preserves insertion order of remaining elements, which matters
        // for the cycle error message in sibling-import scenarios.
        self.in_progress.shift_remove(module_path);

        // Propagate error after cleanup
        let mut compiled = result?;

        // Commit filesystem mode now that the module has compiled successfully.
        // Deferred from the Ok-and-std match arm above so that a parse/compile
        // failure does not taint stdlib_mode (no std.* module was actually inserted
        // into self.modules on a failed compile). Also reject the case where a
        // transitive std.* import committed Embedded mode during recursion;
        // overwriting would silently mix sources.
        if commit_fs_mode {
            // During import recursion, a transitive std.* import may have fallen
            // back to the embedded stdlib (because it was missing from stdlib_root)
            // and committed Embedded mode. Overwriting with FileSystem here would
            // silently mix stdlib sources — exactly the partial-overlay scenario
            // the all-or-nothing invariant exists to reject.
            if self.stdlib_mode == Some(StdlibMode::Embedded) {
                return Err(vec![partial_overlay_diag(
                    module_path,
                    OverlayDirection::FsOverEmbedded {
                        commit_site: CommitSite::Transitive,
                    },
                    &resolver.stdlib_root,
                )]);
            }
            self.stdlib_mode = Some(StdlibMode::FileSystem);
        }

        // Enforce module-path declaration (spec §7.1/§7.2, task γ).
        // Skip stdlib modules: none of the stdlib .ri files declare `module`, so
        // emitting W_MODULE_DECL_MISSING for every std.* import would be spurious
        // noise under the FileSystem stdlib path. The embedded path is unaffected
        // (stdlib modules are inserted directly; they never reach this branch).
        if !is_std_path {
            attach_module_path_diag(&mut compiled, &parsed);
        }

        // Record in post-order (only on success)
        self.topo_order.push(module_path.to_string());
        self.modules.insert(module_path.to_string(), compiled);

        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;

    #[track_caller]
    fn assert_fs_over_embedded(diag: &Diagnostic, expected_marker: &str, forbidden_marker: &str) {
        assert!(diag.message.contains("std.foo"));
        assert!(diag.message.contains("/tmp/stdlib"));
        assert!(diag.message.contains("resolved on the filesystem"));
        assert!(diag.message.contains("embedded stdlib"));
        assert!(
            diag.message.contains(expected_marker),
            "diagnostic must contain the structural marker '{}', got: {}",
            expected_marker,
            diag.message
        );
        assert!(
            !diag.message.contains(forbidden_marker),
            "diagnostic must NOT contain the forbidden marker '{}', got: {}",
            forbidden_marker,
            diag.message
        );
    }

    #[test]
    fn partial_overlay_diag_fs_over_embedded_format() {
        let stdlib_root = std::path::PathBuf::from("/tmp/stdlib");

        // Entry variant
        let entry_diag = partial_overlay_diag(
            "std.foo",
            OverlayDirection::FsOverEmbedded {
                commit_site: CommitSite::Entry,
            },
            &stdlib_root,
        );
        assert_fs_over_embedded(
            &entry_diag,
            "(fs-over-embedded/entry)",
            "(fs-over-embedded/transitive)",
        );

        // Transitive variant
        let transitive_diag = partial_overlay_diag(
            "std.foo",
            OverlayDirection::FsOverEmbedded {
                commit_site: CommitSite::Transitive,
            },
            &stdlib_root,
        );
        assert_fs_over_embedded(
            &transitive_diag,
            "(fs-over-embedded/transitive)",
            "(fs-over-embedded/entry)",
        );
    }

    #[test]
    fn partial_overlay_diag_embedded_over_fs_format() {
        let stdlib_root = std::path::PathBuf::from("/tmp/stdlib");
        let diag = partial_overlay_diag("std.bar", OverlayDirection::EmbeddedOverFs, &stdlib_root);
        assert!(diag.message.contains("std.bar"));
        assert!(diag.message.contains("/tmp/stdlib"));
        assert!(diag.message.contains("not found on the filesystem"));
        assert!(diag.message.contains("resolved from the filesystem"));
        // Structural kind marker assertion
        assert!(
            diag.message.contains("(embedded-over-fs)"),
            "embedded-over-fs diagnostic must contain the structural marker '(embedded-over-fs)', got: {}",
            diag.message
        );
    }

    #[test]
    fn diag_invalid_path_formats_message() {
        use reify_core::Severity;
        // Calls the 2-arg form; "resolving import" is now inlined into the format string.
        let diags = diag_invalid_path("some.path", ModulePathParseError::Empty);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Error);
        assert_eq!(
            diags[0].message,
            "invalid module path while resolving import 'some.path': module path must not be empty",
        );
    }
}

/// Returns `true` iff all `#cfg(...)` predicates on `import` are satisfied by `cfg`.
///
/// An import with no `cfg_predicates` is vacuously satisfied (always followed).
/// Stacked `#cfg` pragmas are ANDed: all must be satisfied (PRD D-1).
///
/// This is the canonical import-gating predicate. It is `pub` so the GUI's
/// dirty-buffer bridge (`gui/src-tauri/src/engine.rs`) can gate its direct
/// imports through the same definition instead of re-inlining
/// `cfg_predicates.iter().all(cfg_satisfied)`, which could otherwise drift from
/// this semantics.
pub fn import_cfg_satisfied(import: &reify_ast::ImportDecl, cfg: &CfgSet) -> bool {
    import.cfg_predicates.iter().all(|p| cfg_satisfied(p, cfg))
}

/// Compile a project starting from an entry file.
///
/// Builds the full module DAG, detects cycles, and returns modules in
/// topological order (dependencies before dependents).
pub fn compile_project(
    entry_path: &Path,
    resolver: &ModuleResolver,
) -> Result<Vec<CompiledModule>, Vec<Diagnostic>> {
    // Read entry file from disk, then delegate to the public in-memory overload.
    let source = std::fs::read_to_string(entry_path).map_err(|e| {
        vec![Diagnostic::error(format!(
            "failed to read entry file '{}': {}",
            entry_path.display(),
            e,
        ))]
    })?;
    compile_project_with_entry_source(entry_path, &source, resolver)
}

/// Compile a project using a caller-supplied in-memory string for the entry
/// module's source instead of reading `entry_path` from disk.
///
/// This is designed for editor / IDE dirty-buffer compile flows where the
/// user's unsaved edits should be compiled without writing to disk first.
///
/// - **Imports resolve from disk** via `resolver.project_root` and
///   `resolver.stdlib_root`, exactly as in `compile_project`.
/// - **Only the entry's source is supplied in-memory**; sibling modules are
///   read from the filesystem by the existing `ModuleResolver` machinery.
/// - **`entry_path` need not exist on disk.** Its `file_stem()` is used only
///   to derive the entry module's name (so the module gets the correct path in
///   the compiled output and in topological order).
///
/// Returns modules in topological order (dependencies before dependents), or
/// a non-empty `Vec<Diagnostic>` describing any parse / compile errors in the
/// supplied source or in any transitively imported module.
pub fn compile_project_with_entry_source(
    entry_path: &Path,
    entry_source: &str,
    resolver: &ModuleResolver,
) -> Result<Vec<CompiledModule>, Vec<Diagnostic>> {
    compile_project_with_entry_source_cfg(entry_path, entry_source, resolver, &CfgSet::default())
}

/// Compile a project using a caller-supplied in-memory string for the entry
/// module's source, with an active `CfgSet` for `#cfg(...)` import gating.
///
/// Imports whose `#cfg(...)` predicates are unsatisfied against `cfg` are
/// skipped entirely: not recursed into, not compiled, not included in the
/// entry prelude. An empty `CfgSet` (the default) is vacuously satisfied
/// for all imports, so behaviour is identical to `compile_project_with_entry_source`.
///
/// Returns modules in topological order (dependencies before dependents), or
/// a non-empty `Vec<Diagnostic>` on parse / compile error.
pub fn compile_project_with_entry_source_cfg(
    entry_path: &Path,
    entry_source: &str,
    resolver: &ModuleResolver,
    cfg: &CfgSet,
) -> Result<Vec<CompiledModule>, Vec<Diagnostic>> {
    let entry_name = entry_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("main");
    let module_path = reify_core::ModulePath::single(entry_name);
    let parsed = reify_syntax::parse(entry_source, module_path);

    if !parsed.errors.is_empty() {
        return Err(parsed
            .errors
            .iter()
            .map(|e| Diagnostic::error(e.message.clone()))
            .collect());
    }

    let mut dag = ModuleDag::with_cfg(cfg.clone());

    // Recursively compile imports that satisfy the active cfg.
    for decl in &parsed.declarations {
        if let reify_ast::Declaration::Import(import) = decl {
            if !import_cfg_satisfied(import, cfg) {
                continue;
            }
            dag.compile_module(&import.path, resolver)?;
        }
    }

    // Collect imported modules as prelude so their pub units (and other
    // exported definitions) are visible in the entry module.
    // Block-scope the preludes so the shared borrows of dag.modules are
    // dropped before the mutable borrow in dag.modules.insert below.
    // Only satisfied imports were compiled above, so the same gate is applied
    // here to avoid a `.expect()` panic on an uncompiled (gated-out) import.
    let compiled_entry = {
        let preludes: Vec<&CompiledModule> = parsed
            .declarations
            .iter()
            .filter_map(|d| {
                if let reify_ast::Declaration::Import(import) = d {
                    Some(import)
                } else {
                    None
                }
            })
            .filter(|import| import_cfg_satisfied(import, cfg))
            .map(|import| {
                dag.modules
                    .get(&import.path)
                    .expect("invariant: satisfied import compiled before entry prelude collection")
            })
            .collect();
        crate::compile_with_prelude_refs(&parsed, &preludes)
    };
    // Enforce module-path declaration (spec §7.1/§7.2, task γ).
    // parsed.path == ModulePath::single(entry_name) by construction (D-6).
    // Entry source is always a user module, so no stdlib exclusion needed.
    let mut compiled_entry = compiled_entry;
    attach_module_path_diag(&mut compiled_entry, &parsed);
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

/// `true` iff `path` addresses the standard library (`std` or `std.*`).
///
/// Such imports are skipped by [`compile_entry_with_stdlib_cfg`]: the full
/// stdlib is already seeded into the prelude via `stdlib_loader::load_stdlib()`,
/// so resolving `std.*` through the DAG would be redundant work.
fn is_std_import_path(path: &str) -> bool {
    path == "std" || path.starts_with("std.")
}

/// Merge the `pub` templates of already-compiled direct imports into `entry`'s
/// own template list, **first-wins** with a `Warning` on every name collision.
///
/// This is the single home of the cross-import template-merge policy shared by
/// [`compile_entry_with_stdlib_cfg`] (the CLI `reify check` bridge) and the
/// GUI's `compile_entry_with_imports` (dirty-buffer bridge). Keeping the policy
/// — the visibility filter, the first-wins de-dup, and the collision-warning
/// wording — in one place ensures the two callers cannot drift (e.g. a fix to
/// the collision logic landing in only one of them).
///
/// - `entry_origin` is the entry module's own path/name, used only to label
///   entry-vs-import collisions in the warning message.
/// - `imports` is the ordered list of `(import path, compiled module)` pairs for
///   the **followed** (cfg-satisfied, non-`std`) direct imports. Callers own the
///   gating; this function merges whatever it is given, in order.
///
/// Only `Visibility::Public` imported templates are merged — private imports
/// must not leak, mirroring compile-time import semantics. A template name that
/// already exists (declared by the entry or merged from an earlier import) is
/// skipped and a `Severity::Warning` is pushed onto `entry.diagnostics` naming
/// both declaring sides and the `first-wins` policy. The warning is phrased
/// kind-neutrally ("imported pub template") because `templates` can hold
/// non-structure template kinds.
pub fn merge_imported_pub_templates(
    entry: &mut CompiledModule,
    entry_origin: &str,
    imports: &[(&str, &CompiledModule)],
) {
    // `templates_origin` (template name → declaring module path) is pre-seeded
    // with the entry's own templates so BOTH entry-vs-import and
    // import-vs-import collisions emit a Warning naming both sides.
    let mut templates_origin: HashMap<String, String> = HashMap::new();
    for tmpl in &entry.templates {
        templates_origin.insert(tmpl.name.clone(), entry_origin.to_string());
    }
    for &(import_path, imported_module) in imports {
        for template in &imported_module.templates {
            if template.visibility != crate::Visibility::Public {
                continue;
            }
            if let Some(prior_origin) = templates_origin.get(&template.name) {
                entry.diagnostics.push(
                    Diagnostic::warning(format!(
                        "imported pub template '{}' declared in both '{}' and '{}'; first-wins",
                        template.name, prior_origin, import_path
                    ))
                    .with_label(DiagnosticLabel::new(
                        SourceSpan::prelude(),
                        "cross-import collision",
                    )),
                );
                continue;
            }
            entry.templates.push(template.clone());
            templates_origin.insert(template.name.clone(), import_path.to_string());
        }
    }
}

/// Compile an entry module for `reify check` with the full stdlib prelude seeded
/// **and** a `#cfg(...)`-gated user-import DAG.
///
/// This is the bridge `reify check` needs that neither existing compiler entry
/// point provides alone:
/// - [`crate::compile_with_stdlib`] seeds the stdlib prelude but follows no
///   user-import DAG (single module only).
/// - [`compile_project_with_entry_source_cfg`] gates imports by `#cfg(...)` but
///   does **not** seed the stdlib prelude.
///
/// Mirrors the GUI's `compile_entry_with_imports` bridge — `load_stdlib()`
/// chained with user-import refs → [`crate::PreludeContext`] →
/// [`crate::compile_with_prelude_context`] — and layers Task γ's cfg gating on
/// top: imports whose `#cfg(...)` predicates are unsatisfied against `cfg` are
/// skipped entirely (not compiled, not added to the prelude). `std.*` imports
/// are also skipped because the full stdlib is already present via
/// `load_stdlib()`.
///
/// **Diagnostics-embedded contract.** A *satisfied* import that points at a
/// missing or broken module surfaces its failure as `Error`-severity
/// diagnostics on the returned [`CompiledModule`] rather than as an `Err`,
/// matching [`crate::compile_with_stdlib`]'s contract so the CLI's "any Error
/// diagnostic → exit non-zero" logic handles gated-DAG import failures
/// uniformly. A *gated-out* import is inert (its module is never resolved), so
/// it can never produce such an error (mirrors γ's
/// `cfg_gating_unsatisfied_import_never_resolved`).
pub fn compile_entry_with_stdlib_cfg(
    parsed: &reify_ast::ParsedModule,
    resolver: &ModuleResolver,
    cfg: &CfgSet,
) -> CompiledModule {
    let mut dag = ModuleDag::with_cfg(cfg.clone());

    // Compute the FOLLOWED direct imports once: non-`std` imports whose
    // `#cfg(...)` predicates are satisfied by `cfg`. The compile loop, the
    // prelude-refs collection, and the pub-template merge below all reuse this
    // single list, so the gate cannot drift between them (and each import's cfg
    // predicates are evaluated once rather than three times).
    let followed_imports: Vec<&reify_ast::ImportDecl> = parsed
        .declarations
        .iter()
        .filter_map(|decl| match decl {
            reify_ast::Declaration::Import(import) => Some(import),
            _ => None,
        })
        .filter(|import| !is_std_import_path(&import.path) && import_cfg_satisfied(import, cfg))
        .collect();

    // Compile each followed import. Collect — do NOT early-return on —
    // import-compile errors so a broken import still lets the entry compile
    // (with the failure surfaced as a diagnostic below).
    let mut import_error_diags: Vec<Diagnostic> = Vec::new();
    for import in &followed_imports {
        if let Err(diags) = dag.compile_module(&import.path, resolver) {
            import_error_diags.extend(diags);
        }
    }

    // Build the prelude: the full stdlib (always seeded, target-independent)
    // chained with the followed user imports that actually compiled. A
    // satisfied-but-failed import is absent from `dag.modules`, so `filter_map`
    // over `.get()` skips it (its error is already collected); an `.expect()`
    // would panic on that path, which is why this differs from
    // `compile_project_with_entry_source_cfg` (which early-returns on import
    // error and so can assume presence).
    let stdlib_modules = crate::stdlib_loader::load_stdlib();
    let mut compiled = {
        let user_import_refs: Vec<&CompiledModule> = followed_imports
            .iter()
            .filter_map(|import| dag.modules.get(&import.path))
            .collect();

        let prelude_refs: Vec<&CompiledModule> = stdlib_modules
            .iter()
            .chain(user_import_refs.iter().copied())
            .collect();

        crate::compile_with_prelude_context(parsed, &crate::PreludeContext::new(&prelude_refs))
    };

    // Enforce module-path declaration (spec §7.1/§7.2, task γ). The entry source
    // is always a user module, so no stdlib exclusion is needed.
    attach_module_path_diag(&mut compiled, parsed);

    // Surface any import-compile failures as diagnostics on the entry module.
    compiled.diagnostics.extend(import_error_diags);

    // Merge `pub` templates from the followed direct imports into the entry's
    // `compiled.templates` so cross-module entities (e.g. a `sub` whose
    // structure is declared in a followed platform module) resolve for
    // downstream eval. The first-wins merge policy + collision warning live in
    // the shared `merge_imported_pub_templates` helper so this CLI bridge and
    // the GUI's `compile_entry_with_imports` cannot drift. A
    // satisfied-but-failed import is absent from `dag.modules`, so `filter_map`
    // over `.get()` skips it (mirroring the prelude build above).
    let entry_origin = compiled.path.0.join(".");
    let merge_inputs: Vec<(&str, &CompiledModule)> = followed_imports
        .iter()
        .filter_map(|import| {
            dag.modules
                .get(&import.path)
                .map(|module| (import.path.as_str(), module))
        })
        .collect();
    merge_imported_pub_templates(&mut compiled, &entry_origin, &merge_inputs);

    compiled
}
