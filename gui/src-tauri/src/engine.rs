// EngineSession — wraps Engine + CompiledModule + source text

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use tracing::warn;

use reify_compiler::{CompiledModule, EntityKind, ValueCellKind, find_template};
use reify_eval::cache::NodeId;
use reify_eval::tolerance_combine::{
    OutputTarget, conforms_to_output, extract_output_export_spec,
};
use reify_eval::{CancellationHandle, CheckResult, Engine};
use reify_core::{
    ContentHash, ConstraintNodeId, DimensionVector, ModulePath, RealizationNodeId, Severity,
    ValueCellId,
};
use reify_ir::{CompiledExprKind, ConstraintChecker, DeterminacyState, ExportFormat, GeometryKernel, Satisfaction, Value, ValueMap};

#[cfg(test)]
use reify_ir::ConstraintSolver;

use reify_core::{Diagnostic, DiagnosticInfo, DiagnosticLabel, SourceLocationInfo};

use crate::types::{
    AppearanceDirective, AutoResolveConstraintProgress, AutoResolveIteration,
    AutoResolveParameterValue, ConstraintData, DefInfo, DemandPruneMeasurementDto,
    DisplayDirective, DisplayStyleData, EntityIdentity, EntityTreeNode, FileData, GuiState,
    JointBinding, JointDescriptor, MechanismDescriptor, MeshData, SourceSpanInfo,
    TensegritySurfaceData, TensegrityWireData, ValueData, format_determinacy, format_freshness,
    format_value,
};

// ── Persistent-cache startup sweep (task 3698) ────────────────────────────────

/// Test-friendly seam: sweep a caller-supplied `cache_root`.
///
/// Thin wrapper over [`reify_eval::sweep_persistent_cache_at_startup`] exposed
/// as a `pub(crate)` function so unit tests can drive a hermetic `TempDir`
/// without manipulating process env (which is not thread-safe in in-process
/// tests).  Not part of `reify_gui`'s public API.
///
/// Returns the [`reify_eval::persistent_cache::SweepReport`] so tests can
/// assert on `tempfiles_removed` / `orphan_dirs_removed`.
pub(crate) fn sweep_persistent_cache(
    cache_root: &std::path::Path,
) -> reify_eval::persistent_cache::SweepReport {
    reify_eval::sweep_persistent_cache_at_startup(cache_root)
}

/// Production startup hook: resolve `cache_root` from process env and run the
/// sweep.
///
/// Called once from `gui/src-tauri/src/main.rs` before `EngineSession`
/// construction so the stale-tempfile and orphan-directory cleanup runs on
/// every GUI launch (task 3698).
///
/// Resolution mirrors `reify-cli`'s `resolve_cache_root` pipeline:
/// `REIFY_CACHE_DIR` → `REIFY_CACHE_MAX_BYTES` / `HOME` / `XDG_CACHE_HOME`.
/// On resolver error (e.g. malformed `REIFY_CACHE_MAX_BYTES`) the sweep is
/// skipped and the error is logged at `tracing::debug!` level — matching the
/// CLI's policy so both entry points behave identically on bad env.
/// The `SweepReport` is discarded.
pub fn bootstrap_persistent_cache_sweep() {
    use reify_config::cache::{CacheResolverInputs, resolve_cache};

    let env_dir = std::env::var("REIFY_CACHE_DIR").ok();
    let env_max_bytes = std::env::var("REIFY_CACHE_MAX_BYTES").ok();
    let xdg_cache_home = std::env::var("XDG_CACHE_HOME").ok();
    let home = std::env::var("HOME").unwrap_or_default();

    let inputs = CacheResolverInputs {
        cli_dir: None,
        env_dir: env_dir.as_deref(),
        env_max_bytes: env_max_bytes.as_deref(),
        user_config: None,
        project_config: None,
        home: std::path::Path::new(&home),
        xdg_cache_home: xdg_cache_home.as_deref(),
    };

    match resolve_cache(&inputs) {
        Ok(r) => {
            let _ = sweep_persistent_cache(&r.dir);
        }
        Err(e) => {
            tracing::debug!("persistent-cache sweep skipped — resolver error: {e}");
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────

mod core_state {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    use reify_compiler::CompiledModule;
    use reify_eval::{CheckResult, Engine};

    #[cfg(test)]
    use reify_core::Diagnostic;
    #[cfg(test)]
    use reify_ir::ConstraintSolver;

    /// Describes how `commit_state` should handle the `file_path` core field.
    ///
    /// Using an explicit enum instead of `Option<PathBuf>` makes the intent
    /// unambiguous at every call site and prevents a future caller from
    /// accidentally passing `None` while meaning "clear the path" (which is
    /// not a supported operation — `commit_state` never clears `file_path`).
    ///
    /// ## Variants
    ///
    /// - `Set(PathBuf)` — overwrite `file_path` with the given path.  Used by
    ///   `load_file`, which passes `FilePathUpdate::Set(path.to_path_buf())`.
    ///   Because Rust evaluates all call arguments before entering the callee body,
    ///   a panic in `to_path_buf()` lands in the pre-commit window: none of the
    ///   five fields are written.
    /// - `Preserve` — leave `file_path` unchanged.  Used by `load_from_source`
    ///   and `update_source`, which do not change which file is loaded; passing
    ///   `Preserve` keeps the project-root anchor set by a prior `load_file` intact.
    pub(crate) enum FilePathUpdate {
        /// Set `file_path` to the given `PathBuf`.
        Set(PathBuf),
        /// Leave `file_path` unchanged.
        Preserve,
    }

    /// The six core fields of `EngineSession` that must stay consistent across panics.
    ///
    /// Fields have **no visibility marker** — they are strictly private to this `impl`
    /// block.  Any direct field assignment from outside (e.g. `session.core.compiled = …`)
    /// fails to compile, enforcing the poison-recovery invariant at the type level.
    /// The only commit points that touch the five invariant-bearing fields (`compiled`,
    /// `source_map`, `module_name`, `last_check`, `file_path`) are:
    /// - `commit_state` — five-field atomic commit after a successful compile cycle
    ///   (`file_path` is updated when `FilePathUpdate::Set` is passed; `FilePathUpdate::Preserve`
    ///   leaves it unchanged)
    /// - `commit_check` — single-field commit for `last_check` (used by `set_parameter`)
    ///
    /// `engine_mut()` exposes `&mut Engine` for method dispatch and does not touch the
    /// invariant-bearing fields.  The `#[cfg(test)]` mutators (`break_module_name`,
    /// `break_source_map`, `inject_compiled`, `recheck`, `inject_diagnostic`, `with_solver`)
    /// are intentional invariant-breakers — they are absent from production builds, so the
    /// poison-recovery property holds in production.
    ///
    /// See `engine_lock.rs` for the invariant rationale.
    pub(crate) struct CoreState {
        engine: Engine,
        compiled: Option<CompiledModule>,
        source_map: HashMap<String, String>,
        file_path: Option<PathBuf>,
        last_check: Option<CheckResult>,
        module_name: Option<String>,
    }

    impl CoreState {
        /// Construct a fresh `CoreState` wrapping the given engine.
        pub(super) fn new(engine: Engine) -> Self {
            Self {
                engine,
                compiled: None,
                source_map: HashMap::new(),
                file_path: None,
                last_check: None,
                module_name: None,
            }
        }

        /// Return a shared reference to the underlying `Engine`.
        pub(crate) fn engine(&self) -> &Engine {
            &self.engine
        }

        /// Return a mutable reference to the underlying `Engine`.
        ///
        /// Used for method dispatch (`check`, `build`, `tessellate_snapshot`,
        /// `set_panic_on_eval`, `cache_store_mut`).
        pub(crate) fn engine_mut(&mut self) -> &mut Engine {
            &mut self.engine
        }

        /// Return a reference to the compiled module, or `None` if no module is loaded.
        pub(crate) fn compiled(&self) -> Option<&CompiledModule> {
            self.compiled.as_ref()
        }

        /// Return a reference to the last check result, or `None` if no check has run.
        pub(crate) fn last_check(&self) -> Option<&CheckResult> {
            self.last_check.as_ref()
        }

        /// Return the current module name, or `None` if no module is loaded.
        pub(crate) fn module_name(&self) -> Option<&str> {
            self.module_name.as_deref()
        }

        /// Return a reference to the source map.
        pub(crate) fn source_map(&self) -> &HashMap<String, String> {
            &self.source_map
        }

        /// Return the file path of the currently loaded file, or `None` if not set.
        pub(crate) fn file_path(&self) -> Option<&Path> {
            self.file_path.as_deref()
        }

        /// Split borrow: return an immutable ref to `compiled` alongside a mutable
        /// ref to `engine`.
        ///
        /// The two return values come from disjoint struct fields (`compiled` and
        /// `engine`), so the compiler proves they do not alias.  This method
        /// surfaces that split through the encapsulation boundary so callers can
        /// hold both simultaneously — something that would otherwise require direct
        /// field access (which the private-field invariant forbids).
        ///
        /// Typical use: callers that need `compiled` immutably AND need to call a
        /// mutating method on `engine` (e.g. `build`, `tessellate_snapshot`) in the
        /// same expression or closely-coupled block.
        pub(super) fn split_compiled_and_engine_mut(
            &mut self,
        ) -> (Option<&CompiledModule>, &mut Engine) {
            (self.compiled.as_ref(), &mut self.engine)
        }

        /// Atomically commit a fresh `CheckResult` into `last_check`.
        ///
        /// This is the **single** write-point for `last_check` used by
        /// `EngineSession::set_parameter` after a successful `engine.edit_check`.
        /// Callers may rely on this method touching **only** `last_check` — no
        /// other core field is modified.  This guarantee is what lets
        /// `engine_lock::with_engine_lock` safely recover from a poisoned mutex:
        /// a panic inside `set_parameter` between `edit_check` and `commit_check`
        /// leaves `last_check` as the previous value, not a partially-updated one.
        pub(crate) fn commit_check(&mut self, check: CheckResult) {
            self.last_check = Some(check);
        }

        /// Commit the five canonical core fields after a successful
        /// parse+compile+check cycle.
        ///
        /// This is the **single** multi-field commit point.  Writes proceed in a
        /// fixed order: `source_map` is rebuilt first (clear then insert), then
        /// `module_name`, `compiled`, `last_check`, and finally `file_path` (when
        /// `FilePathUpdate::Set`).  This is best-effort atomic: a panic on an
        /// intermediate allocation (e.g. inside `source_map.insert` or a
        /// `to_string()` call) may leave the fields in a partially-updated state.
        /// That is tolerated: the surrounding mutex is recovered via
        /// `PoisonError::into_inner`, and the affected fields are either rebuilt on
        /// the next `commit_state` call or consumed only through graceful-degrade
        /// paths (`resolve_source`, `get_diagnostics`).
        /// Callers must only invoke this after compilation and `check()` have
        /// both succeeded.
        ///
        /// ## `file_path` parameter
        ///
        /// Pass a [`FilePathUpdate`] variant to control whether `file_path` is updated:
        ///
        /// - `FilePathUpdate::Set(p)` — sets `self.file_path = Some(p)`.  Pass
        ///   `FilePathUpdate::Set(path.to_path_buf())` from `load_file`.  Because Rust
        ///   evaluates all call arguments before entering the callee body, a panic in
        ///   `to_path_buf()` lands in the pre-commit window: none of the five fields are
        ///   written (stronger than best-effort — the entire commit is skipped).
        /// - `FilePathUpdate::Preserve` — leaves the existing `file_path` unchanged.
        ///   `load_from_source` and `update_source` pass `Preserve`; this keeps the
        ///   project-root anchor set by a prior `load_file` intact.
        ///
        /// The five cache fields on `EngineSession` (`def_preview_cache`,
        /// `parsed_cache`, `line_offsets_cache`, `consumed_idents_cache`,
        /// `compile_failure`) are NOT committed here — those are updated by the
        /// outer `EngineSession::commit_state` wrapper after this call returns.
        pub(crate) fn commit_state(
            &mut self,
            compiled: CompiledModule,
            check_result: CheckResult,
            module_name: &str,
            source: &str,
            file_path: FilePathUpdate,
        ) {
            self.source_map.clear();
            self.source_map.insert(
                super::module_key(module_name),
                source.to_string(),
            );
            self.module_name = Some(module_name.to_string());
            self.compiled = Some(compiled);
            self.last_check = Some(check_result);
            if let FilePathUpdate::Set(p) = file_path {
                self.file_path = Some(p);
            }
        }

        // ---- Test-only mutators (cfg(test)) ---------------------------------
        //
        // Each method mirrors an existing `EngineSession::*_for_test` helper,
        // encapsulating the direct field write inside `CoreState`'s impl so that
        // the outer EngineSession mutators can delegate here rather than accessing
        // fields directly.  This is the preparation step for strict field
        // privatization in step-8.

        /// Replace the underlying `Engine` with one that has the given constraint
        /// solver installed.  Consumes and returns `Self` to mirror `Engine::with_solver`.
        #[cfg(test)]
        pub(crate) fn with_solver(mut self, solver: Box<dyn ConstraintSolver>) -> Self {
            self.engine = self.engine.with_solver(solver);
            self
        }

        /// Clear `module_name` while leaving `compiled` and `source_map` intact,
        /// intentionally breaking the compiled/module_name/source_map invariant.
        #[cfg(test)]
        pub(crate) fn break_module_name(&mut self) {
            self.module_name.take();
        }

        /// Clear `source_map` while leaving `compiled` and `module_name` intact,
        /// intentionally breaking the compiled/module_name/source_map invariant.
        #[cfg(test)]
        pub(crate) fn break_source_map(&mut self) {
            self.source_map.clear();
        }

        /// Directly inject a `CompiledModule` without running parse/compile/check.
        ///
        /// `module_name`, `source_map`, and `last_check` are NOT updated, so the
        /// session's invariant is intentionally broken after this call.
        #[cfg(test)]
        pub(crate) fn inject_compiled(&mut self, compiled: CompiledModule) {
            self.compiled = Some(compiled);
        }

        /// Re-run `engine.check` on the current compiled module and store the result.
        ///
        /// Clones `self.compiled` to avoid the borrow conflict between
        /// `self.engine` (needs `&mut`) and `self.compiled` (immutable reference
        /// for the check call).  No-op when no module is loaded.
        #[cfg(test)]
        pub(crate) fn recheck(&mut self) {
            if let Some(compiled) = self.compiled.as_ref().cloned() {
                let check_result = self.engine.check(&compiled);
                self.last_check = Some(check_result);
            }
        }

        /// Push a diagnostic into the currently compiled module's diagnostics vec.
        ///
        /// Panics if no module is currently loaded.
        #[cfg(test)]
        pub(crate) fn inject_diagnostic(&mut self, diag: Diagnostic) {
            self.compiled
                .as_mut()
                .expect("inject_diagnostic: no compiled module loaded")
                .diagnostics
                .push(diag);
        }
    }
}

pub(crate) use core_state::CoreState;
pub(crate) use core_state::FilePathUpdate;

/// Discriminant for a stored compile failure: records which execution path produced the error.
///
/// `ColdStart` means `compiled` was `None` at failure time (no prior good compile exists).
/// `LiveEdit`  means `compiled` was `Some`  at failure time (a prior good compile is still
///             in the session — the user is editing live).
///
/// The two variants gate which `build_gui_state` branch surfaces the failure diagnostics:
/// `ColdStart` → early-return branch; `LiveEdit` → append branch alongside `get_diagnostics()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompileFailureKind {
    /// Failure on the cold-start path — `compiled` is `None` at failure time.
    ColdStart,
    /// Failure on the live-edit path — `compiled` is `Some` at failure time
    /// (a prior successful compile is still in the session).
    LiveEdit,
}

/// A stored compile failure from the most recent failed parse/compile attempt.
///
/// Produced by `record_compile_failure` and consumed by `build_gui_state`.
/// The `kind` discriminant controls which `build_gui_state` branch surfaces `diags`.
///
/// `Clone` is required because `build_gui_state`'s early-return branch clones `diags`
/// into the returned `GuiState`.
///
/// # One-snapshot invariant
///
/// `source` and `diags` are always from the SAME compile attempt: `diags` carry
/// line/col positions computed against `source`, so indexing `source` by a
/// diagnostic's `line`/`col` always yields the offending text.  `build_gui_state`
/// surfaces `source` as `files[].content` whenever it reports `diags`, ensuring
/// the MCP `engine_state` snapshot is internally consistent.
#[derive(Debug, Clone)]
pub(crate) struct CompileFailure {
    /// Structured diagnostics from the failed attempt.
    pub(crate) diags: Vec<DiagnosticInfo>,
    /// Which execution path produced this failure.
    pub(crate) kind: CompileFailureKind,
    /// The exact source text the failing compile was run against.
    ///
    /// `build_gui_state` surfaces this as `files[].content` (overriding the
    /// last-good `source_map` entry) so `compile_diagnostics` line/col positions
    /// index into the correct buffer.  Set to the full entry-file text passed to
    /// `compile_single_file_with_stdlib` or `compile_entry_with_imports`.
    pub(crate) source: String,
    /// Module key (e.g. `"bracket.ri"`) derived via `module_key(module_name)`.
    ///
    /// Identifies which `source_map` entry `build_gui_state`'s LiveEdit branch
    /// should override with `source`.
    pub(crate) file_key: String,
}

/// Session wrapping an Engine with its compiled module and source text.
///
/// Provides higher-level operations for the GUI: load, update, set parameter, export.
///
/// # Invariant: compiled / module_name / source_map must stay in sync
///
/// Whenever `compiled` is `Some`, **all three** of the following should hold:
///
/// 1. `module_name` is `Some(name)`.
/// 2. `source_map` contains the key `module_key(name)` (i.e. `"{name}.ri"`).
/// 3. The value stored at that key is the source text that produced the current
///    `CompiledModule`.
///
/// When the invariant is broken (e.g. via test helpers), `resolve_source`
/// returns `None`, and `get_diagnostics` / `get_source_location` degrade
/// gracefully rather than panicking.
///
/// **Mutation is type-enforced via `CoreState`:** the six core fields are held
/// in a private sub-struct whose fields have no visibility marker, so any direct
/// field assignment from outside `CoreState`'s impl fails to compile.  The only
/// commit points that touch the five invariant-bearing fields are `commit_state`
/// (five-field atomic commit, `file_path` updated via `FilePathUpdate::Set` /
/// preserved via `FilePathUpdate::Preserve`) and `commit_check`
/// (single-field `last_check`); `engine_mut()` does not touch those fields,
/// and the `#[cfg(test)]` mutators are intentional invariant-breakers absent from
/// production builds — the poison-recovery property holds in production.
/// See `engine_lock.rs` for the rationale.
pub struct EngineSession {
    /// The six core fields protected by the type system via `CoreState`.
    ///
    /// Fields are strictly private — direct assignment from outside `CoreState`'s
    /// impl fails to compile.  Use `commit_state` (five-field atomic commit,
    /// `file_path` updated via `FilePathUpdate::Set` / preserved via `FilePathUpdate::Preserve`)
    /// or `commit_check` (single-field
    /// `last_check`) to commit the invariant-bearing fields atomically.
    core: CoreState,
    /// In-memory cache for `get_def_preview` results.
    ///
    /// Keyed by `(definition_name, template.content_hash)` — the cache is
    /// automatically invalidated when a new module is loaded (via `commit_state`
    /// which clears the map) or when the template's content hash changes.
    def_preview_cache: HashMap<(String, ContentHash), GuiState>,
    /// Cached parse result for the currently-loaded source.
    ///
    /// Populated by `commit_state` immediately after a successful parse+compile+check
    /// cycle.  Set to `None` until the first load; overwritten (not appended) on
    /// every subsequent `commit_state` call.  Used by `get_containing_definition`
    /// to avoid re-parsing the source on every cursor/hover event.
    parsed_cache: Option<reify_ast::ParsedModule>,
    /// Cached line-offset table for the currently-loaded source.
    ///
    /// Each entry is the byte position of a `\n` character in the source text.
    /// Populated by `commit_state` via `build_line_offsets(source)` in the same
    /// atomic block as `parsed_cache`.  Set to `None` until the first load;
    /// overwritten on every `commit_state` call.  Used by `get_containing_definition`
    /// to skip the O(M) newline scan on every cursor/hover call.
    line_offsets_cache: Option<Vec<usize>>,
    /// Consumed-idents cache for the terminal-mechanism filter in `get_mechanism_descriptors`.
    ///
    /// Keyed by structure name (template name); maps to the set of mechanism member names
    /// consumed as `mech_in` by `body()` calls within that structure.  Populated lazily on
    /// the first `get_mechanism_descriptors` call after a successful parse+compile+check cycle.
    /// Invalidated (set to `None`) in `commit_state` alongside `parsed_cache` so both caches
    /// share the same lifecycle.  Left `None` when `parsed_cache` is `None` at population time
    /// — preserves the per-template WARN so fallback regressions remain visible.
    consumed_idents_cache: Option<HashMap<String, HashSet<String>>>,
    /// Tagged compile failure from the most recent failed parse/compile attempt, or
    /// `None` when no failure is stored (after construction or after every successful
    /// `commit_state` cycle).  The `kind` discriminant encodes which path produced
    /// the failure: `ColdStart` (`compiled` was `None` at failure time) routes through
    /// `build_gui_state`'s early-return branch; `LiveEdit` (`compiled` was `Some`)
    /// routes through the append branch alongside `get_diagnostics()` output.
    ///
    /// `Option<CompileFailure>` makes the at-most-one-non-empty invariant inexpressible —
    /// the prior two-field representation enforced it only at runtime via `debug_assert!`s.
    compile_failure: Option<CompileFailure>,
    /// Set of `(structure, member)` pairs for which a `W_KinematicReservedParamName`
    /// WARN has already been emitted in this session (i.e. since the last
    /// `commit_state` / module load).
    ///
    /// `get_mechanism_descriptors` checks this set before emitting each WARN so that
    /// a scrub-path re-invocation of `get_mechanism_descriptors` does not re-flood
    /// the log with the same collision for every parameter change.  Cleared on every
    /// `commit_state` call (same lifecycle as `consumed_idents_cache`).  Never `None`
    /// — always initialized to an empty `HashSet` in `new()` and cleared (not replaced)
    /// on commit.
    reserved_param_warned: HashSet<(String, String)>,
    /// Optional auto-resolve event sink installed by the GUI layer.
    ///
    /// When `Some`, `emit_auto_resolve_if_any` calls `start → iteration → complete`
    /// after every check that produces non-empty `resolved_params`. When `None`
    /// (the default), all emit paths are no-ops — existing tests that construct an
    /// EngineSession without installing an emitter are unaffected.
    auto_resolve_emitter: Option<Arc<dyn AutoResolveEmitter>>,
    /// Optional warm-pool event sink installed by the GUI layer.
    ///
    /// When `Some`, `drain_and_emit_warm_pool_events` forwards each drained
    /// [`reify_eval::warm_pool::WarmPoolEvent`] (translated to the IPC
    /// [`crate::types::WarmPoolEvent`] shape) to the installed emitter. When
    /// `None` (the default), the drain still records events on the journal but
    /// no IPC emission occurs — existing tests that don't install an emitter are
    /// unaffected.
    warm_pool_event_emitter: Option<Arc<dyn WarmPoolEventEmitter>>,
    /// Optional fea-case event sink installed by the GUI layer.
    ///
    /// When `Some`, `emit_fea_case_if_any` scans `CheckResult.values` for a
    /// `MultiCaseResult`-shaped cell and fires `changed(FeaCaseChanged)` on the
    /// first match. When `None` (the default), all emit paths are no-ops.
    /// Fire-every-commit semantics: no engine-side dedup (mirrors `emit_auto_resolve_if_any`).
    fea_case_emitter: Option<Arc<dyn FeaCaseEmitter>>,
    /// Optional fea-diagnostics-changed event sink installed by the GUI layer (task #4884).
    ///
    /// When `Some`, `emit_fea_diagnostics` calls `changed(build_fea_diagnostics())` on
    /// every commit — full-list snapshot including the empty list (to clear a stale overlay).
    /// When `None` (the default), all emit paths are no-ops.
    fea_diagnostics_emitter: Option<Arc<dyn FeaDiagnosticsEmitter>>,
    /// Optional fea-convergence-changed event sink installed by the GUI layer (task #5032).
    ///
    /// When `Some`, `emit_fea_convergence` calls `changed(build_fea_convergence())` on
    /// every commit — full-value snapshot including `None` (to clear a stale indicator).
    /// When `None` (the default), all emit paths are no-ops.
    fea_convergence_emitter: Option<Arc<dyn FeaConvergenceEmitter>>,
    /// Optional mode-shape-frame event sink installed by the GUI layer.
    ///
    /// When `Some`, `emit_mode_shape_frames_if_any` scans `CheckResult.values` for a
    /// `BucklingResult`-shaped cell and fires `frame(ModeShapeFrame)` for each
    /// reference frame (one undeformed base + one peak per mode).
    /// When `None` (the default), all emit paths are no-ops.
    mode_shape_frame_emitter: Option<Arc<dyn ModeShapeFrameEmitter>>,
    /// Optional solve-cancellation sink installed by the GUI layer.
    ///
    /// When `Some`, `check_with_solve_slot` fires `solve_started(handle)` before
    /// `engine.check()` and `solve_finished()` after.  The production sink
    /// (`PendingSolveCancelSink` in `commands.rs`) writes the handle into
    /// `AppState.pending_solve_cancel` so `cancel_solve_impl` can read it.
    /// When `None` (the default), all lifecycle calls are no-ops.
    solve_cancel_sink: Option<Arc<dyn SolveCancellationSink>>,
    /// Optional solver-progress sink installed by the GUI layer (task 4079).
    ///
    /// When `Some`, `set_solver_progress_sink` forwards the sink to the inner
    /// `reify_eval::Engine`, which installs it in the thread-local dispatch
    /// context around every trampoline call.  When `None` (the default) no
    /// per-iteration progress events are emitted.
    solver_progress_sink: Option<Arc<dyn reify_eval::SolverProgressSink>>,
    /// Error message from the most recent failed hot-reload attempt, or `None`
    /// when no failure is recorded (after construction, after a successful
    /// `commit_state` cycle, or before any reload has been attempted).
    ///
    /// Set by `record_reload_error` at the `commands::update_source_impl`
    /// chokepoint — AFTER `with_engine_lock` has caught and converted any
    /// `check()` panic to `Err` — so recording is panic-safe.  Covers both
    /// the compile-error path and the check-panic path uniformly.
    ///
    /// Cleared in `commit_state` (alongside `compile_failure`) so any
    /// successful reeval auto-resets staleness.
    ///
    /// Surfaced via `is_stale()` / `reload_error()` for the debug API and
    /// via `build_gui_state`'s synthetic DiagnosticInfo for the GUI channel.
    last_reload_error: Option<String>,
    /// Explicitly selected FEA case name for multi-case results.
    ///
    /// `None` (the initial value) means "use the lex-first case" — the same
    /// default that `detect_multi_case_result` and `emit_fea_case_if_any`
    /// use for `active_case_id`.  Set by `set_active_fea_case`; read by
    /// `apply_fea_channels` and `build_gui_state` when assembling FEA channels.
    active_fea_case: Option<String>,
    /// Cache of bare tessellation mesh data (vertices/indices/normals, no FEA
    /// or shell channels).
    ///
    /// Populated by `build_gui_state` immediately after `tessellate_snapshot`,
    /// before `apply_fea_channels` and `apply_shell_channels`.  `None` until
    /// the first successful tessellation.  Used by `set_active_fea_case` to
    /// re-source per-case FEA channels without re-tessellating — critical for
    /// keeping case-switch latency sub-frame even with large OCCT meshes.
    tess_mesh_cache: Option<Vec<MeshData>>,
    /// Cache of tessellation diagnostics from the last `tessellate_snapshot`.
    ///
    /// Populated alongside `tess_mesh_cache` in `build_gui_state`.  Used by
    /// `set_active_fea_case` so the returned GuiState accurately reflects the
    /// last tessellation result (no re-tessellation → same diagnostics).
    tess_diag_cache: Vec<DiagnosticInfo>,
    /// Task #5338: last delta-resolved value per geometry-derived panel cell —
    /// the VALUE-side twin of the mesh-side retention the frontend already does.
    ///
    /// A `TessellateResult` is an INCREMENTAL DELTA, not a full snapshot. Its two
    /// halves have different normative homes — the MESH half upstream (the DELTA
    /// CONTRACT block on `Engine::demand_scoped_unified_pass`, reify-eval
    /// `engine_build.rs`), the VALUES half in this crate, at
    /// `surface_geometry_derived_cells`, which carries the measurement. Read them
    /// there rather than here. The consequence this cache exists for: a
    /// realization the pass skipped carries `Undef` for its auto-derived
    /// mass-property cells even though those values are unchanged and still
    /// correct, so an absent/`Undef` entry means "retain the previous value", NOT
    /// "the value is gone". Retaining them here keeps a `: Rigid` body's `mass` /
    /// `centroid` / `moment_of_inertia` / `moi_principal` from reverting to
    /// `Undef` on a no-edit re-render.
    ///
    /// Three invalidation triggers, and only three:
    /// * `commit_state` — unconditional `clear()` on every recompile;
    /// * `sync_demand` — drops entries for entities the frontend no longer
    ///   declares visible. This is where arch §8 ("a pruned realization's cached
    ///   result is never served as Final") is discharged, so every entry reaching
    ///   a rebuild belongs to a demanded ENTITY. See that method for the
    ///   entity-vs-realization granularity limitation qualifying it;
    /// * `invalidate_geometry_derived_cache_for_entity`, from `set_parameter` —
    ///   the warm edit whose realization stays hash-exempt, where no fresh delta
    ///   entry can ever exist to outrank the retained one.
    ///
    /// A `Value::Undef` in the delta is not unconditionally a gap:
    /// `surface_geometry_derived_cells` retains only when the cell's realization
    /// did not run this pass, so a DISPATCHED degeneration clears the entry rather
    /// than replaying a stale value as Final. See that function for the limit of
    /// the dispatch signal — it is inferred from mesh presence, which an op
    /// failure can defeat.
    ///
    /// NOT a usable discriminator: the cell's own `freshness == "pending"` flag.
    /// Measured — a HASH-EXEMPT but VISIBLE body's mass-prop cells also read
    /// `"pending"`, because they are downstream CONSUMERS of the realization while
    /// the demand cone is its BACKWARD closure, so `mark_demand_pruned_pending`
    /// flips them regardless of visibility. Cell freshness cannot tell HIDDEN from
    /// HASH-EXEMPT; the frontend's declared visible set can.
    geometry_derived_cache: HashMap<ValueCellId, Value>,
}

/// Trait for sinking auto-resolve loop events to the GUI transport layer.
///
/// Implemented by [`TauriAutoResolveEmitter`] in `main.rs` for the production
/// path, and by `RecordingEmitter` in tests.  The trait is object-safe:
/// no method takes or returns `Self`.
pub trait AutoResolveEmitter: Send + Sync {
    fn start(&self);
    fn iteration(&self, iter: AutoResolveIteration);
    fn complete(&self);
}

/// Trait for sinking warm-pool telemetry events to the GUI transport layer.
///
/// Implemented by [`crate::TauriWarmPoolEventEmitter`] in `main.rs` for the
/// production path (calls `event_bus::emit_typed` with channel `"warm-pool-event"`),
/// and by `RecordingWarmPoolEventEmitter` in engine tests.
///
/// The trait is object-safe: no method takes or returns `Self`.
pub trait WarmPoolEventEmitter: Send + Sync {
    fn emit(&self, event: crate::types::WarmPoolEvent);
}

/// Trait for sinking fea-case-changed events to the GUI transport layer.
///
/// Implemented by `TauriFeaCaseEmitter` in `main.rs` for the production path
/// (calls `event_bus::emit_typed` with channel `"fea-case-changed"`), and by
/// `RecordingFeaCaseEmitter` in engine tests.
///
/// The trait is object-safe: no method takes or returns `Self`.
pub trait FeaCaseEmitter: Send + Sync {
    fn changed(&self, payload: crate::types::FeaCaseChanged);
}

/// Trait for sinking fea-diagnostics-changed events to the GUI transport layer (task #4884).
///
/// Implemented by `TauriFeaDiagnosticsEmitter` in `main.rs` for the production path
/// (calls `event_bus::emit_typed` with channel `"fea-diagnostics-changed"`), and by
/// `RecordingFeaDiagnosticsEmitter` in engine tests.
///
/// Payload semantics: full-list snapshot of `Vec<FeaDiagnosticInfo>` — fires on EVERY
/// commit including the empty list (so a param edit that fixes the FEA problem clears
/// the stale overlay on the frontend).
///
/// The trait is object-safe: no method takes or returns `Self`.
pub trait FeaDiagnosticsEmitter: Send + Sync {
    fn changed(&self, payload: Vec<crate::types::FeaDiagnosticInfo>);
}

/// Trait for sinking fea-convergence-changed events to the GUI transport layer (task #5032).
///
/// Implemented by `TauriFeaConvergenceEmitter` in `main.rs` for the production path
/// (calls `event_bus::emit_typed` with channel `"fea-convergence-changed"`), and by
/// `RecordingFeaConvergenceEmitter` in engine tests.
///
/// Payload semantics: full-value snapshot of `Option<FeaConvergenceInfo>` — fires on
/// EVERY commit including `None` (so a param edit that clears the FEA problem clears
/// the stale convergence indicator on the frontend).
///
/// The trait is object-safe: no method takes or returns `Self`.
pub trait FeaConvergenceEmitter: Send + Sync {
    fn changed(&self, payload: Option<crate::types::FeaConvergenceInfo>);
}

/// Trait for sinking mode-shape-frame events to the GUI transport layer (task ι/3458).
///
/// Implemented by `TauriModeShapeFrameEmitter` in `main.rs` for the production path
/// (calls `event_bus::emit_typed` with channel `"mode-shape-frame"`), and by
/// `RecordingModeShapeFrameEmitter` in engine tests.
///
/// The trait is object-safe: no method takes or returns `Self`.
pub trait ModeShapeFrameEmitter: Send + Sync {
    /// Deliver a single reference frame for the mode-shape animator.
    fn frame(&self, payload: crate::types::ModeShapeFrame);
}

/// Trait for sinking solve-cancellation slot lifecycle events (task γ/4086).
///
/// Implemented by `PendingSolveCancelSink` in `commands.rs` for the production
/// path (writes the handle into `AppState.pending_solve_cancel`) and by
/// `RecordingSolveCancelSink` in engine tests.
///
/// **Slot lifecycle only — not mid-solve interruption.**
/// `solve_started` is called with a fresh `CancellationHandle` *before*
/// `engine.check()` runs; `solve_finished` is called *after* `check()`
/// returns.  Because the elastic_static trampoline ignores its `_cancellation`
/// handle and `solve_cantilever_fea` is a single blocking call, the handle
/// does *not* interrupt the in-flight solve.  True interruption requires
/// cross-cutting `reify-eval` handle-injection and is future work outside
/// task γ's scope.
///
/// Publishing is serialized under the session mutex (`with_engine_lock`), so
/// the `AppState`-doc invariant "at most one Some at a time" holds.
/// `cancel_solve_impl` locks only the slot, never the session mutex — no
/// lock-order inversion.
///
/// The trait is object-safe: no method takes or returns `Self`.
pub trait SolveCancellationSink: Send + Sync {
    /// Called with a fresh handle immediately before `engine.check()` starts.
    fn solve_started(&self, handle: CancellationHandle);
    /// Called immediately after `engine.check()` returns (or on any early
    /// return / unwind via [`SolveFinishedGuard`]).
    fn solve_finished(&self);
}

/// RAII guard that fires `sink.solve_finished()` on drop.
///
/// Ensures `solve_finished` is called even if the surrounding block exits
/// via a `?` early-return (e.g., the `edit_check` path in `set_parameter`).
/// When the sink is `None`, `drop` is a no-op.
struct SolveFinishedGuard(Option<Arc<dyn SolveCancellationSink>>);

impl Drop for SolveFinishedGuard {
    fn drop(&mut self) {
        if let Some(ref sink) = self.0 {
            sink.solve_finished();
        }
    }
}

/// Build the normalized source-map key for a module name: `"{name}.ri"`.
///
/// This is the single authoritative point for key derivation, replacing three
/// formerly-identical `format!("{}.ri", ...)` call sites in
/// `load_from_source`, `update_source`, and `resolve_source`.
pub(crate) fn module_key(name: &str) -> String {
    debug_assert!(!name.is_empty(), "module_key called with empty name");
    format!("{}.ri", name)
}

/// Returns `true` for any `std` or `std.*` import path.
///
/// Used by `compile_entry_with_imports` at two filter sites (prelude-ref
/// de-duplication and template merge) so both stay in lockstep if the
/// stdlib path convention ever changes.
fn is_stdlib_path(p: &str) -> bool {
    p == "std" || p.starts_with("std.")
}

/// Build the `(error_string, diag_infos)` payload for an error result.
///
/// Centralises the mechanical pattern shared by all parse- and compile-error
/// return sites in `compile_single_file_with_stdlib` and
/// `compile_entry_with_imports`: join diagnostic messages into a human-readable
/// string and simultaneously call [`diagnostics_to_info`] for the structured
/// wire payload.
///
/// `prefix` becomes the leading label (e.g. `"Parse errors"`, `"Compile errors"`).
/// The returned string has the form `"{prefix}: msg1; msg2; …"`, preserving the
/// wire-format invariant the function docstrings promise.
fn build_err_payload(
    prefix: &str,
    diags: &[Diagnostic],
    file_path: &str,
    source: &str,
) -> (String, Vec<DiagnosticInfo>) {
    let msgs: Vec<String> = diags.iter().map(|d| d.message.clone()).collect();
    let error_string = format!("{}: {}", prefix, msgs.join("; "));
    let diag_infos = diagnostics_to_info(diags, file_path, source);
    (error_string, diag_infos)
}

/// Synthesize [`Diagnostic`] values from a slice of [`reify_syntax::ParseError`]s
/// and delegate to [`build_err_payload`].
///
/// Parse errors carry span information but are not `Diagnostic` values; this
/// helper wraps each one in a synthetic `Diagnostic::error` with a label so
/// [`diagnostics_to_info`] can resolve spans to line/column numbers.
fn parse_errs_to_payload(
    errors: &[reify_ast::ParseError],
    file_path: &str,
    source: &str,
) -> (String, Vec<DiagnosticInfo>) {
    let synthetic_diags: Vec<Diagnostic> = errors
        .iter()
        .map(|e| Diagnostic::error(e.message.clone()).with_label(DiagnosticLabel::new(e.span, "")))
        .collect();
    build_err_payload("Parse errors", &synthetic_diags, file_path, source)
}

/// Parse and compile a single-file source string using the stdlib prelude.
///
/// Returns `(ParsedModule, CompiledModule)` on success, or an `Err` containing
/// a human-readable error string (preserving the existing `"Parse errors: …"` /
/// `"Compile errors: …"` format so existing substring assertions remain valid)
/// **and** a `Vec<DiagnosticInfo>` with the same errors in structured form.
/// The structured payload is used by callers to populate
/// `EngineSession::compile_failure` (via `record_compile_failure`) so `build_gui_state`
/// can surface the failure in the diagnostics panel.
///
/// This is the single-file counterpart to `compile_entry_with_imports`.  It is
/// called by both `load_from_source` (which always uses the single-file path) and
/// the `self.file_path == None` branch of `update_source` (no project-root anchor).
fn compile_single_file_with_stdlib(
    content: &str,
    module_name: &str,
) -> Result<(reify_ast::ParsedModule, CompiledModule), (String, Vec<DiagnosticInfo>)> {
    // Prelude-aware parse so stdlib enum references like `CorrosionClass.C5`
    // disambiguate to `EnumAccess` rather than `MemberAccess`.  See task 2525.
    let parsed = reify_compiler::parse_with_stdlib(content, ModulePath::single(module_name));
    if !parsed.errors.is_empty() {
        let file_path = module_key(module_name);
        return Err(parse_errs_to_payload(&parsed.errors, &file_path, content));
    }
    let compiled = reify_compiler::compile_with_stdlib_checked(
        &parsed,
        &reify_constraints::SimpleConstraintChecker,
    );
    let has_errors = compiled
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error);
    if has_errors {
        let error_diags: Vec<Diagnostic> = compiled
            .diagnostics
            .into_iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        let file_path = module_key(module_name);
        return Err(build_err_payload(
            "Compile errors",
            &error_diags,
            &file_path,
            content,
        ));
    }
    Ok((parsed, compiled))
}

/// Parse and compile `source` with multi-file import resolution.
///
/// This is the compile-side of `load_file`'s multi-file flow (task 3228 v1).
///
/// `reify_compiler::module_dag::compile_project_with_entry_source` (at
/// module_dag.rs:607) covers most of the same scaffolding — parse,
/// `ModuleDag::new`, recursive `compile_module` per import, prelude
/// collection, final entry compile.  This GUI helper exists because that
/// compiler function does **not** yet do two things load_file needs:
///
///   1. **Stdlib in prelude** — the compiler function uses
///      `compile_with_prelude_refs` with user imports only.  load_file needs
///      stdlib enum disambiguation (e.g. `CorrosionClass.C5` → `EnumAccess`)
///      and stdlib functions like `box(...)` to resolve, which require the
///      stdlib slice in the prelude.
///   2. **Template merge for eval** — `find_template` is called against the
///      entry module only (engine_eval.rs:1629; unfold.rs:418, :466), so
///      imported pub structures must be merged into `entry.templates` before
///      eval; the compiler function's return value doesn't do that merge.
///
/// Replacing this helper with a call into the compiler API is filed as a
/// follow-up — extend `compile_project_with_entry_source` to seed stdlib
/// and return entry-with-merged-templates, then this becomes a one-liner.
///
/// # Flow
///
/// 1. Parse `source` with `parse_with_stdlib` (preserves stdlib enum
///    disambiguation, e.g. `CorrosionClass.C5` → `EnumAccess`).
/// 2. Build `ModuleResolver::new(project_root, stdlib_root)` where
///    `project_root` is the directory containing `entry_path` and
///    `stdlib_root = project_root.join("crates/reify-compiler/stdlib")`.
///    Matching the LSP heuristic: for user projects the stdlib dir usually
///    doesn't exist on disk, so `ModuleDag` falls back to the embedded stdlib.
/// 3. For each `import` declaration in the parsed module, call
///    `dag.compile_module(&import.path, &resolver)`.  Errors are surfaced as
///    `"Compile errors: ..."` strings.
/// 4. Build prelude refs: stdlib modules (from `load_stdlib()`) + user imports
///    from `dag.modules` (in declaration order, skipping `std.*` keys which are
///    already present via the stdlib slice).
/// 5. Compile the entry with `compile_with_prelude_context(&parsed, &ctx)`.
/// 6. Merge non-stdlib imported templates into `compiled.templates` so that
///    `find_template` during eval finds imported pub structures.
///
/// # v1 transitive-import limitation
///
/// Only **direct** (1-hop) imports of the entry file have their templates merged
/// into `compiled.templates`.  If `helper.ri` itself imports `util.ri`, `Util`'s
/// `TopologyTemplate` will not be present at eval time, and `find_template` will
/// fail with "unknown structure" for any `sub` referencing `Util`.  Iterating
/// all entries in `dag.modules` and merging each would fix this; deferred to a
/// follow-up task.
///
/// # v1 source-map limitation
///
/// Only the entry's source is stored in `source_map` (under the entry module
/// key). Imported file contents are not added to `source_map`; the GUI's
/// "files" panel will show only the entry file.  See task 3228 for the
/// planned follow-up.
fn compile_entry_with_imports(
    entry_path: &Path,
    source: &str,
    module_name: &str,
) -> Result<(CompiledModule, reify_ast::ParsedModule), (String, Vec<DiagnosticInfo>)> {
    // Parse with stdlib enum pre-seeding (same as load_from_source / update_source).
    let parsed = reify_compiler::parse_with_stdlib(source, ModulePath::single(module_name));
    if !parsed.errors.is_empty() {
        let file_path = module_key(module_name);
        return Err(parse_errs_to_payload(&parsed.errors, &file_path, source));
    }

    // project_root = directory of the entry file; stdlib_root matches LSP heuristic.
    let project_root = entry_path.parent().unwrap_or(Path::new("."));
    let stdlib_root = project_root.join("crates/reify-compiler/stdlib");

    let resolver = reify_compiler::module_dag::ModuleResolver::new(project_root, &stdlib_root);

    // The GUI dirty-buffer path uses the host default active cfg (PRD §4 D-2):
    // there is no GUI cfg selector in v1, so `target` is the compiling host's
    // platform string (std::env::consts::OS) and no flags/kv are set. Build the
    // DAG with this cfg so TRANSITIVE imports are gated by it too (mirrors the
    // CLI's compile_entry_with_stdlib_cfg).
    let host_cfg = reify_compiler::cfg::CfgSet::host_default();
    let mut dag = reify_compiler::module_dag::ModuleDag::with_cfg(host_cfg.clone());

    // Collect import paths from the parsed module (top-level Import declarations
    // only), gating each DIRECT import by its `#cfg(...)` predicates against the
    // host cfg. An import whose predicates are unsatisfied (e.g. a non-host
    // `#cfg(target = ...)`) is dropped here, so it is skipped uniformly by the
    // compile loop, the prelude `user_import_refs` collection, and the
    // pub-template-merge loop below — all three iterate this filtered list.
    let import_paths: Vec<String> = parsed
        .declarations
        .iter()
        .filter_map(|decl| {
            if let reify_ast::Declaration::Import(imp) = decl {
                // Gate each DIRECT import through the canonical shared predicate
                // (`module_dag::import_cfg_satisfied`) rather than re-inlining
                // `cfg_predicates.iter().all(cfg_satisfied)`, so the GUI and CLI
                // gating semantics cannot drift.
                if reify_compiler::module_dag::import_cfg_satisfied(imp, &host_cfg) {
                    Some(imp.path.clone())
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    // Compile each non-stdlib import.  Std.* paths are skipped: the full
    // stdlib is seeded into the prelude below via `load_stdlib()`, and the
    // user_import_refs / template-merge loops both filter std.* out, so a
    // `dag.compile_module("std.units", ...)` call would be wasted work
    // (one extra parse+compile per std import in the typical case).
    for import_path in &import_paths {
        if is_stdlib_path(import_path) {
            continue;
        }
        dag.compile_module(import_path, &resolver)
            .map_err(|diags| {
                let file_path = format!("{}.ri", import_path);
                // Resolve the import's source via the resolver for accurate span resolution
                // so line/column numbers in the diagnostics panel point to real locations.
                // Falls back to "" (spans collapse to 1:1) if resolution or I/O fails.
                let import_source = resolver
                    .resolve_import_path(import_path)
                    .ok()
                    .and_then(|p| std::fs::read_to_string(p).ok())
                    .unwrap_or_default();
                build_err_payload(
                    &format!("Compile errors in import '{}'", import_path),
                    &diags,
                    &file_path,
                    &import_source,
                )
            })?;
    }

    // Build prelude refs: stdlib (static) + user imports from dag.modules.
    // Skipping std.* keys from the import list because the full stdlib is already
    // present via the load_stdlib() slice — adding them again would be redundant.
    let stdlib_modules = reify_compiler::stdlib_loader::load_stdlib();
    let user_import_refs: Vec<&CompiledModule> = import_paths
        .iter()
        .filter(|p| !is_stdlib_path(p))
        .filter_map(|p| dag.modules.get(p))
        .collect();

    let prelude_refs: Vec<&CompiledModule> = stdlib_modules
        .iter()
        .chain(user_import_refs.iter().copied())
        .collect();

    let ctx = reify_compiler::PreludeContext::new(&prelude_refs);
    let mut compiled = reify_compiler::compile_with_prelude_context_checked(
        &parsed,
        &ctx,
        &reify_constraints::SimpleConstraintChecker,
    );

    // Surface compile errors.
    let has_errors = compiled
        .diagnostics
        .iter()
        .any(|d| d.severity == Severity::Error);
    if has_errors {
        let error_diags: Vec<Diagnostic> = compiled
            .diagnostics
            .into_iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        let file_path = module_key(module_name);
        return Err(build_err_payload(
            "Compile errors",
            &error_diags,
            &file_path,
            source,
        ));
    }

    // Merge pub templates from direct (1-hop) non-stdlib imports into the entry's
    // compiled.templates so that reify_eval::Engine::eval / unfold can find them
    // via find_template(&module.templates, name).
    //
    // The first-wins merge policy — the `Visibility::Public` filter, the de-dup,
    // and the collision warning — lives in the shared
    // `reify_compiler::module_dag::merge_imported_pub_templates` helper so this
    // GUI dirty-buffer bridge and the CLI `reify check` bridge
    // (`compile_entry_with_stdlib_cfg`) cannot drift. Std.* imports are excluded
    // (stdlib structures are not top-level GUI entities); cfg-gated-out imports
    // are already absent from `import_paths`.
    //
    // v1 limitation: only DIRECT imports are merged.  If helper.ri itself imports
    // util.ri, Util's template will be absent from this list and find_template will
    // fail at eval for any sub referencing Util.  A future fix should iterate all
    // dag.modules entries instead of just import_paths.
    let merge_inputs: Vec<(&str, &CompiledModule)> = import_paths
        .iter()
        .filter(|p| !is_stdlib_path(p))
        .filter_map(|p| dag.modules.get(p).map(|module| (p.as_str(), module)))
        .collect();
    reify_compiler::module_dag::merge_imported_pub_templates(
        &mut compiled,
        module_name,
        &merge_inputs,
    );

    Ok((compiled, parsed))
}

impl EngineSession {
    /// Shared field-initializer from a pre-constructed `Engine`.
    ///
    /// Both `new` and `with_registered_kernel` delegate here so the field list
    /// stays in one place and the two constructors cannot drift.
    ///
    /// CRITICAL: `register_production_compute_fns` is called HERE (once) rather
    /// than in `new` or `with_registered_kernel` individually.  Both public
    /// constructors delegate to this method (`new` → `from_engine(Engine::new(..))`,
    /// `with_registered_kernel` → `from_engine(Engine::with_registered_kernel(..))`),
    /// so registering here covers both paths.  `register_production_compute_fns`
    /// **panics on duplicate registration** — it inherits the single-install
    /// discipline of the trampoline registrars it bundles internally (see its
    /// rustdoc "# Panics" on `Engine::register_production_compute_fns`,
    /// compute_targets/mod.rs); calling it in `new` *and* here would register
    /// twice on the same `Engine` → guaranteed panic.
    /// PRD §4.5 / esc-2962-66 root cause; PRD docs/prds/compute-fea-hardening.md
    /// task A3.
    fn from_engine(mut engine: Engine) -> Self {
        // Canonical production compute-trampoline bundle (INV-FEA-1; PRD
        // docs/prds/compute-fea-hardening.md task A1/A3): one call installs the
        // FEA / buckling / modal / form-find / multi-case / dynamics / trajectory
        // trampolines, the shell-extract trampoline, and — new here, the
        // esc-2962-66-class gap this migration closes — the mesh-morph producer.
        //
        // `reify-mesh-morph` is optional, gated on this crate's `gui` feature,
        // while this module is ungated — so the non-`gui` lib/bin build cannot
        // name `reify_mesh_morph::` directly, forcing the cfg split below. See
        // `MorphRegistration`'s rustdoc (reify-eval's `compute_targets` module)
        // for the `Unavailable` contract this feeds.
        #[cfg(feature = "gui")]
        let morph =
            reify_eval::MorphRegistration::Enabled(reify_mesh_morph::register_morph_producer);
        #[cfg(not(feature = "gui"))]
        let morph = reify_eval::MorphRegistration::Unavailable {
            // Scoped to the lib/bin (shipping) build, not the test build:
            // reify-mesh-morph is also an unconditional *dev*-dependency
            // (`features = ["testing"]`), so it IS on the crate graph for
            // `cargo test`'s gui-off compilation and this arm still runs
            // there (the split is on `feature = "gui"`, not `cfg(test)`) —
            // the reason text must not overclaim un-linkability in that
            // configuration, only in a real shipping build.
            reason: "reify-gui lib/bin built without the `gui` feature: \
                     reify-mesh-morph is an optional (non-dev) dep gated on \
                     that feature, so no producer fn is linkable in a \
                     shipping build",
        };
        engine.register_production_compute_fns(morph);
        // Enable undef-cause capture before any check/eval so the per-cell
        // origin side-map and post-eval snapshot are populated. Capture is
        // purely additive (PRD A1): values, determinacy, constraints, and
        // caching are byte-identical with it on. Mirrors reify-lsp analysis.rs:106.
        engine.set_capture_undef_causes(true);

        Self {
            core: CoreState::new(engine),
            def_preview_cache: HashMap::new(),
            parsed_cache: None,
            line_offsets_cache: None,
            consumed_idents_cache: None,
            compile_failure: None,
            reserved_param_warned: HashSet::new(),
            auto_resolve_emitter: None,
            warm_pool_event_emitter: None,
            fea_case_emitter: None,
            fea_diagnostics_emitter: None,
            fea_convergence_emitter: None,
            mode_shape_frame_emitter: None,
            solve_cancel_sink: None,
            solver_progress_sink: None,
            last_reload_error: None,
            active_fea_case: None,
            tess_mesh_cache: None,
            tess_diag_cache: Vec::new(),
            // Task #5338: empty until the first kernel-bearing tessellate resolves
            // a geometry-derived cell. See the field's doc for the delta contract.
            geometry_derived_cache: HashMap::new(),
        }
    }

    /// Install an auto-resolve event emitter on this session.
    ///
    /// After installation, every `Engine::check` / `edit_check` call that
    /// produces non-empty `resolved_params` fires `start → iteration → complete`
    /// on the emitter.  Replaces any previously installed emitter.
    pub fn set_auto_resolve_emitter(&mut self, emitter: Arc<dyn AutoResolveEmitter>) {
        self.auto_resolve_emitter = Some(emitter);
    }

    /// Install a warm-pool event emitter on this session.
    ///
    /// After installation, every `drain_and_emit_warm_pool_events` call
    /// (which happens after each engine check/build/edit call) forwards
    /// translated IPC [`crate::types::WarmPoolEvent`] values to the emitter.
    /// Replaces any previously installed emitter.
    pub fn set_warm_pool_event_emitter(&mut self, emitter: Arc<dyn WarmPoolEventEmitter>) {
        self.warm_pool_event_emitter = Some(emitter);
    }

    /// Install a fea-case-changed event emitter on this session.
    ///
    /// After installation, every `emit_fea_case_if_any` call — co-located with
    /// `emit_auto_resolve_if_any` inside the single `post_engine_call_telemetry`
    /// choke-point (INV-GUI-2) — fires `changed(FeaCaseChanged)` when a
    /// `MultiCaseResult`-shaped value is detected in `CheckResult.values`.
    /// Replaces any previously installed emitter.
    pub fn set_fea_case_emitter(&mut self, emitter: Arc<dyn FeaCaseEmitter>) {
        self.fea_case_emitter = Some(emitter);
    }

    /// Install a fea-diagnostics-changed event emitter on this session (task #4884).
    ///
    /// After installation, every `emit_fea_diagnostics` call — co-located inside
    /// the single `post_engine_call_telemetry` choke-point (INV-GUI-2) — fires
    /// `changed(Vec<FeaDiagnosticInfo>)` with a full-list snapshot — including
    /// the empty list, to clear a stale overlay. Replaces any previously
    /// installed emitter.
    pub fn set_fea_diagnostics_emitter(&mut self, emitter: Arc<dyn FeaDiagnosticsEmitter>) {
        self.fea_diagnostics_emitter = Some(emitter);
    }

    /// Install a fea-convergence-changed event emitter on this session (task #5032).
    ///
    /// After installation, every `emit_fea_convergence` call — co-located inside
    /// the single `post_engine_call_telemetry` choke-point (INV-GUI-2) — fires
    /// `changed(Option<FeaConvergenceInfo>)` with a full-value snapshot — including
    /// `None`, to clear a stale indicator. Replaces any previously installed emitter.
    pub fn set_fea_convergence_emitter(&mut self, emitter: Arc<dyn FeaConvergenceEmitter>) {
        self.fea_convergence_emitter = Some(emitter);
    }

    /// Install a mode-shape-frame event emitter on this session.
    ///
    /// After installation, every `emit_mode_shape_frames_if_any` call fires
    /// `frame(ModeShapeFrame)` when a `BucklingResult`-shaped value is detected
    /// in `CheckResult.values`. Replaces any previously installed emitter.
    pub fn set_mode_shape_frame_emitter(&mut self, emitter: Arc<dyn ModeShapeFrameEmitter>) {
        self.mode_shape_frame_emitter = Some(emitter);
    }

    // ── Task 3026: active FEA case ────────────────────────────────────────────

    /// Return the explicitly selected FEA case name, or `None` if none has been
    /// set (the lex-first case is used implicitly by `apply_fea_channels`).
    pub fn get_active_fea_case(&self) -> Option<String> {
        self.active_fea_case.clone()
    }

    /// Switch the displayed FEA case and return a rebuilt `GuiState`.
    ///
    /// Stores `name` as the active case and rebuilds the GuiState mesh payload
    /// by cloning the cached tessellation snapshot (`tess_mesh_cache`) and
    /// re-applying `apply_fea_channels` with the new case — **no re-evaluation
    /// and no re-tessellation** occur.  The rest of GuiState (values, constraints,
    /// files, compile diagnostics, tensegrity wires, tessellation diagnostics) is
    /// rebuilt from the already-committed `last_check` / `source_map`.
    ///
    /// Returns `Err` when no module has been loaded yet (no `compiled` or no
    /// `last_check`).  An unknown case name falls back to the lex-first default
    /// (same semantics as `apply_fea_channels`).
    pub fn set_active_fea_case(&mut self, name: &str) -> Result<GuiState, String> {
        if self.core.compiled().is_none() || self.core.last_check().is_none() {
            return Err("Cannot switch FEA case: no module loaded".to_string());
        }
        self.active_fea_case = Some(name.to_string());

        // Clone bare tessellation mesh geometry from cache (O(mesh bytes), no kernel call).
        let mut meshes = self.tess_mesh_cache.clone().unwrap_or_default();

        // Re-apply FEA channels for the new active case.
        {
            let check = self.core.last_check().unwrap();
            apply_fea_channels(&mut meshes, &check.values, self.active_fea_case.as_deref());
        }

        // Re-apply shell channels (pure read from engine cache, no tessellation).
        {
            let shell_views = self.core.engine().shell_gui_mesh_data();
            apply_shell_channels(&mut meshes, &shell_views);
        }

        // Build values, constraints, tensegrity wires + surfaces from the cached
        // check + compiled.
        let (mut values, mut constraints, tensegrity_wires, tensegrity_surfaces) = {
            let compiled = self.core.compiled().unwrap();
            let check = self.core.last_check().unwrap();
            (
                build_values(compiled, check, Some(self.core.engine())),
                build_constraints(compiled, check),
                build_tensegrity_wires(compiled, check),
                build_tensegrity_surfaces(compiled, check),
            )
        };

        // ── Task #5338 amendment: re-surface the geometry-derived cells ────────
        //
        // `build_values` above reads the kernel-LESS `last_check`, so a `: Rigid`
        // body's auto-derived mass-prop cells (`mass` / `centroid` /
        // `moment_of_inertia` / `moi_principal`) come back Undef and the
        // `moi_principal[0] > 0` PD constraint Indeterminate — the same starting
        // point `build_gui_state` has. This path deliberately does NOT re-evaluate
        // or re-tessellate, so there is no delta to read them from; without this
        // call a case switch silently reverted cells that were determined a moment
        // earlier (pre-existing since task 5194, cheap to close only now that the
        // retention cache exists).
        //
        // The delta passed is EMPTY, and that is exact rather than a stand-in: no
        // realization ran this pass, so every retained entry is a delta gap by
        // construction and is replayed, and nothing can be mistaken for a
        // dispatched degeneration. The constraint re-check therefore dispatches
        // against the retained cells alone; it only ever flips an Indeterminate
        // verdict that fully resolves, so a constraint needing any other value
        // stays Indeterminate exactly as it is today.
        //
        // Prune safety is unaffected: `sync_demand` has already dropped every
        // hidden entity's entries, so only demanded entities can be replayed here.
        {
            let cache = &mut self.geometry_derived_cache;
            surface_geometry_derived_cells(
                self.core.engine(),
                &mut values,
                &mut constraints,
                &ValueMap::new(),
                &[],
                cache,
            );
        }

        // Build files and compile diagnostics via shared helpers so both
        // `build_gui_state` and `set_active_fea_case` stay in sync.
        let files = self.build_files_with_live_edit();
        let compile_diagnostics = self.build_compile_diagnostics();

        // Tessellation diagnostics from the cache (no re-tessellation → same diags).
        let tessellation_diagnostics = self.tess_diag_cache.clone();

        // Passive selective-demand measurement (task 4532): mirror build_gui_state
        // so the case-switch path carries the same observational record. Reading
        // it cannot affect evaluation; the immutable engine borrow is released by
        // `.map(..)` before the GuiState literal moves the local fields.
        let demand_prune_measurement = self
            .core
            .engine()
            .last_demand_prune_measurement()
            .map(DemandPruneMeasurementDto::from);

        // FEA structured-diagnostic overlay (R3b-2, #4818): delegates to the
        // shared helper so both GuiState-producing paths cannot diverge.
        let fea_diagnostics = self.build_fea_diagnostics();

        // A-posteriori convergence status of the active case (task 3001):
        // delegates to the shared helper so both GuiState-producing paths
        // cannot diverge.
        let fea_convergence = self.build_fea_convergence();

        Ok(GuiState {
            meshes,
            values,
            constraints,
            files,
            tessellation_diagnostics,
            compile_diagnostics,
            tensegrity_wires,
            tensegrity_surfaces,
            demand_prune_measurement,
            display_panes: Vec::new(),
            display_appearance: Vec::new(),
            fea_diagnostics,
            fea_convergence,
        })
    }

    /// Derive the FEA structured-diagnostic overlay from the most recent check.
    ///
    /// Returns an empty vec when no check has been committed (cold-start or
    /// compile-only path). Called from both `build_gui_state` and
    /// `set_active_fea_case` so the two GuiState-producing paths cannot diverge.
    ///
    /// **Why `structured_detail` is per-check, not per-case:** `structured_detail`
    /// records evaluation-level diagnostics (e.g. rigid-body modes, problem element
    /// sets) produced for the *check as a whole*, not for each named FEA case inside
    /// a multi-case result.  After a case switch, `apply_fea_channels` re-selects
    /// the case-specific scalar contour/displaced positions; the diagnostic overlay
    /// is an evaluation-level property that does not vary per case, so mirroring
    /// the whole-check `structured_detail` here is intentional rather than a bug.
    fn build_fea_diagnostics(&self) -> Vec<crate::types::FeaDiagnosticInfo> {
        self.core
            .last_check()
            .map(|c| crate::types::fea_diagnostics_from_structured(&c.structured_detail))
            .unwrap_or_default()
    }

    /// Derive the a-posteriori convergence status of the active `ElasticResult`
    /// (task 3001) from the most recent check.
    ///
    /// Returns `None` when no check has been committed (cold-start or
    /// compile-only path) or `extract_fea_convergence` finds no `ElasticResult`
    /// for the active case. Called from both `build_gui_state` and
    /// `set_active_fea_case` so the two GuiState-producing paths cannot diverge
    /// (mirrors `build_fea_diagnostics`'s shared-helper structure, including its
    /// safe `.map(..)`-style handling rather than an unguarded `.unwrap()`).
    fn build_fea_convergence(&self) -> Option<crate::types::FeaConvergenceInfo> {
        self.core
            .last_check()
            .and_then(|c| extract_fea_convergence(&c.values, self.active_fea_case.as_deref()))
    }

    /// Inject a `CheckResult` directly into `last_check` for testing.
    ///
    /// Bypasses the full parse/compile/eval cycle — useful for tests that need
    /// to assert on FEA-channel or case-switch behavior with hand-crafted
    /// `MultiCaseResult` values without performing a real FEA solve.
    /// Does NOT clear `tess_mesh_cache` so tessellation geometry from a prior
    /// `load_from_source` / `build_gui_state` call is reused.
    #[cfg(test)]
    pub(crate) fn inject_check_for_test(&mut self, check: reify_eval::CheckResult) {
        self.core.commit_check(check);
    }

    /// Install a solve-cancellation sink on this session (task γ/4086).
    ///
    /// After installation, every call to the `check_with_solve_slot` private
    /// helper (which wraps `engine.check()` at all 4 mutating entry points)
    /// fires `solve_started(handle)` immediately before the check and
    /// `solve_finished()` immediately after.  Replaces any previously installed
    /// sink.
    pub fn set_solve_cancel_sink(&mut self, sink: Arc<dyn SolveCancellationSink>) {
        self.solve_cancel_sink = Some(sink);
    }

    /// Install a solver-progress sink on this session (task 4079).
    ///
    /// Forwards the sink to the underlying `reify_eval::Engine`, which installs
    /// it in the thread-local dispatch context around every trampoline call.
    /// The production sink (`TauriSolverProgressEmitter` in `main.rs`) maps
    /// `SolverProgressUpdate` → `types::SolverProgress` and emits it to the
    /// frontend via the `"solver-progress"` IPC channel.
    pub fn set_solver_progress_sink(&mut self, sink: Arc<dyn reify_eval::SolverProgressSink>) {
        self.solver_progress_sink = Some(Arc::clone(&sink));
        self.core.engine_mut().set_solver_progress_sink(sink);
    }

    /// Expose the engine's current `active_solve_cancel` handle for testing.
    ///
    /// Returns the same `Arc<AtomicBool>` that `with_solve_slot` installs on the
    /// engine before each check.  Tests use this to assert that `H.cancel()`
    /// propagates to the trampoline via the shared atomic.
    #[cfg(test)]
    pub(crate) fn engine_active_solve_cancel_for_test(
        &self,
    ) -> Option<reify_eval::CancellationHandle> {
        self.core.engine().active_solve_cancel()
    }

    /// Wrap any engine operation in the solve-cancellation slot lifecycle.
    ///
    /// If a `SolveCancellationSink` is installed, fires:
    /// 1. `solve_started(handle.clone())` — BEFORE calling `f(self)`.
    /// 2. `solve_finished()` — AFTER `f` returns, guaranteed by
    ///    [`SolveFinishedGuard`] even if `f` short-circuits via `?` or panics.
    ///
    /// The handle is also installed on the inner `Engine` via
    /// `set_active_solve_cancel(Some(handle.clone()))` (task 4079).  A fresh
    /// non-cancelled handle is minted before every check, so stale handles from
    /// prior checks are harmless — the install-before-every-check pattern means
    /// no solve ever executes against a cancelled handle from a prior cycle.
    ///
    /// The Arc clone (cheap) releases the borrow on `self.solve_cancel_sink`
    /// before the mutable borrow of `self` is forwarded to `f` — required to
    /// satisfy the borrow checker.
    fn with_solve_slot<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let handle = CancellationHandle::new();
        let sink_arc = self.solve_cancel_sink.clone(); // cheap Arc clone; releases borrow on self
        if let Some(ref sink) = sink_arc {
            sink.solve_started(handle.clone());
        }
        // Install the same handle on the engine so the trampoline can poll it
        // via the thread-local dispatch context (task 4079 step-10).
        self.core
            .engine_mut()
            .set_active_solve_cancel(Some(handle));
        // Guard fires solve_finished() on drop — covers ? early-returns and panics.
        let _guard = SolveFinishedGuard(sink_arc);
        let result = f(self);
        // Clear the cancel slot now that the solve window has closed.
        // Prevents a stale cancelled handle from spuriously triggering
        // ComputeOutcome::Cancelled on any future dispatch that bypasses
        // with_solve_slot (e.g., a direct engine.eval() call in tests).
        self.core.engine_mut().set_active_solve_cancel(None);
        result
        // _guard drops here → solve_finished() called
    }

    /// Run `engine.check(compiled)` wrapped in the solve-cancellation slot lifecycle.
    ///
    /// Delegates to [`Self::with_solve_slot`]; see there for lifecycle details
    /// and the no-interruption limitation.
    fn check_with_solve_slot(&mut self, compiled: &CompiledModule) -> CheckResult {
        // Task 5212 (bound OCCT native-shape memory across reloads): every
        // whole-file reload entry (load_file / update_source / load_from_source)
        // funnels through here exactly once, so this is the single place to
        // reset the geometry kernels — freeing the previous design's resident
        // native shapes — AND clear the realization cache. Both are required:
        // reloads preserve prior module identity, so build-2 entities collide on
        // entity_id with build-1 and would otherwise cache-hit a build-1
        // KernelHandle whose shape this reset just evicted → InvalidReference →
        // broken render; clearing the cache forces a fresh re-execution against
        // the reset kernel. Idempotent no-op on a cold engine (first load).
        //
        // Placement: the reset must live on THIS funnel, not deeper in the
        // pipeline. tessellate_snapshot / execute_realization_ops also run on
        // every slider tick, so a reset there would wipe warm shapes on each
        // parameter change; reify-eval Engine::check()/build() are also reached
        // by CLI build() and relate_solve sub-builds. The slider path
        // (set_parameter → edit_check) deliberately BYPASSES check_with_solve_slot,
        // so a parameter drag never triggers a reset — exactly the behaviour the
        // reload-wiring regression test pins.
        self.core.engine_mut().reset_geometry_for_reload();
        self.with_solve_slot(|s| s.core.engine_mut().check(compiled))
    }

    /// Install a constraint solver into the underlying Engine for testing.
    ///
    /// Mirrors [`Engine::with_solver`] at the session level.  Keeps production
    /// paths untouched — test-only (pub(crate)) so it cannot be called from
    /// `main.rs` (solver installation in main.rs is a separate future task).
    #[cfg(test)]
    pub(crate) fn with_solver_for_test(mut self, solver: Box<dyn ConstraintSolver>) -> Self {
        self.core = self.core.with_solver(solver);
        self
    }

    /// Run `engine.check(compiled)`, commit the result, then fire all emit-helpers.
    ///
    /// Gives tests a single-call path that exercises the eval+emit pipeline without
    /// going through the full load_from_source / update_source plumbing.  Only for
    /// unit tests; not callable from production code.
    ///
    /// The check result is committed (writing `last_check`) **before** the emitters
    /// fire, mirroring the production ordering invariant (commit first; all four
    /// emitters read from `last_check()`, not the pre-commit result).  This is
    /// required so that `emit_fea_diagnostics()` (which calls
    /// `build_fea_diagnostics()` → `last_check()`) reads the freshly committed
    /// result rather than a stale or absent one.
    #[cfg(test)]
    pub(crate) fn check_and_emit_for_test(&mut self, compiled: &CompiledModule) {
        let r = self.core.engine_mut().check(compiled);
        // Commit first — all emitters below read via last_check(), matching production.
        self.core.commit_check(r);
        self.post_engine_call_telemetry();
    }

    /// Drive `emit_fea_case_if_any` with a pre-built `CheckResult` in tests.
    ///
    /// Mirrors `drain_and_emit_warm_pool_events_for_test`: lets tests inject a
    /// hand-constructed `CheckResult` (including a `multi_case_result_value`-shaped
    /// cell) without needing a full engine eval. Not callable from production code.
    #[cfg(test)]
    pub(crate) fn emit_fea_case_for_test_with_result(&self, check: &CheckResult) {
        self.emit_fea_case_if_any(check);
    }

    /// The single post-engine-call telemetry choke-point (INV-GUI-2).
    ///
    /// Every engine-mutating entry point (`check_and_emit_for_test`,
    /// `load_from_source`, `set_parameter`, `load_file`, `update_source`,
    /// `load_from_compiled`) calls this ONE method after committing state,
    /// instead of hand-rolling its own copy of the five-call emit sequence.
    /// Collapsing to a single call site means every entry point fires the
    /// full quintet in the same order. There is no separate structural
    /// source-introspection guard for this; the invariant is enforced by each
    /// entry point's own behavioral emitter regression test (e.g.
    /// `load_from_compiled_emits_fea_diagnostics`,
    /// `fea_diagnostics_emitter_fires_on_set_parameter`), which fails if that
    /// entry point ever stops routing through here.
    ///
    /// Accepted tradeoff (awareness, not enforcement): the entry-point list
    /// above is not enumerated anywhere the compiler checks. A brand-new
    /// engine-mutating entry point requires two manual steps — call
    /// `self.post_engine_call_telemetry()` after committing state, AND add a
    /// matching `<name>_emits_fea_diagnostics` behavioral test to the
    /// `FeaDiagnosticsEmitter tests` cluster in `tests/engine_tests.rs`
    /// (mirror `load_from_compiled_emits_fea_diagnostics`). Skip either step
    /// and telemetry is silently lost — nothing fails CI until that test
    /// exists.
    ///
    /// Reads `self.core.last_check()` once into a local: every constituent
    /// emit-helper below reads from that same committed `CheckResult`, matching
    /// the ordering invariant each call site already established (emit AFTER
    /// state is committed). Fires, in order: auto-resolve → fea-case →
    /// mode-shape frames → fea-diagnostics → fea-convergence → warm-pool drain.
    ///
    /// Note on the receiver type: this must be `&mut self` (the warm-pool drain
    /// needs `&mut self`), so it deliberately does NOT take `check: &CheckResult`
    /// as a parameter — a caller-supplied borrow of `self.core` would alias the
    /// `&mut self` receiver for the duration of the call. Reading `last_check()`
    /// into a local instead lets NLL end that shared borrow after its last use
    /// (`emit_mode_shape_frames_if_any`), before the `&mut self` drain call.
    /// `emit_fea_diagnostics` and `emit_fea_convergence` each re-read
    /// `last_check()` themselves (via `build_fea_diagnostics`/`build_fea_convergence`)
    /// rather than taking `check`, but that re-borrow is `&self` and ends before
    /// the drain's `&mut self`, so it does not conflict with the `check` local either.
    fn post_engine_call_telemetry(&mut self) {
        let check = self.core.last_check().expect(
            "post_engine_call_telemetry: last_check must be Some after state commit — see cross-cutting ordering invariant",
        );
        self.emit_auto_resolve_if_any(check);
        self.emit_fea_case_if_any(check);
        self.emit_mode_shape_frames_if_any(check);
        self.emit_fea_diagnostics();
        self.emit_fea_convergence();
        self.drain_and_emit_warm_pool_events();
    }

    /// Emit a `fea-diagnostics-changed` event carrying the current full list of
    /// `FeaDiagnosticInfo` derived from `last_check().structured_detail`.
    ///
    /// Full-list snapshot semantics (mirrors tessellation-diagnostics / compile-diagnostics):
    /// the event payload is `build_fea_diagnostics()` — byte-identical to
    /// `GuiState.fea_diagnostics`. Fires on EVERY commit including the empty list
    /// (so a param edit that fixes the FEA problem clears the stale overlay).
    ///
    /// Early-returns silently when no emitter is installed.
    fn emit_fea_diagnostics(&self) {
        let emitter = match &self.fea_diagnostics_emitter {
            Some(e) => e,
            None => return,
        };
        emitter.changed(self.build_fea_diagnostics());
    }

    /// Drive `emit_fea_diagnostics` in tests without a full engine eval.
    ///
    /// Callers must first inject a `CheckResult` via `inject_check_for_test` so
    /// that `build_fea_diagnostics()` reads a non-None `last_check`.
    /// Not callable from production code.
    #[cfg(test)]
    pub(crate) fn emit_fea_diagnostics_for_test(&self) {
        self.emit_fea_diagnostics();
    }

    /// Emit a `fea-convergence-changed` event carrying the current
    /// `Option<FeaConvergenceInfo>` derived from `last_check().values`.
    ///
    /// Full-value snapshot semantics (mirrors `emit_fea_diagnostics`): the event
    /// payload is `build_fea_convergence()` — byte-identical to
    /// `GuiState.fea_convergence`. Fires on EVERY commit including `None` (so a
    /// param edit that clears the FEA problem clears the stale indicator).
    ///
    /// Accepted tradeoff (awareness, not enforcement): this re-derives
    /// `build_fea_convergence()` independently of `build_gui_state()`'s own
    /// call to the same helper, so a single commit pays for the
    /// `extract_fea_convergence` scan twice (once here at the L4 choke-point,
    /// once whenever `build_gui_state` is next requested). This mirrors
    /// `emit_fea_diagnostics`'s identical pre-existing duplication and is
    /// accepted for the same reason: the `ValueMap` scan is small and bounded.
    /// If this choke-point ever becomes hot, compute the FEA-channel snapshots
    /// once per commit and thread them into both `build_gui_state` and these
    /// emit helpers.
    ///
    /// Early-returns silently when no emitter is installed.
    fn emit_fea_convergence(&self) {
        let emitter = match &self.fea_convergence_emitter {
            Some(e) => e,
            None => return,
        };
        emitter.changed(self.build_fea_convergence());
    }

    /// Drive `emit_fea_convergence` in tests without a full engine eval.
    ///
    /// Callers must first inject a `CheckResult` via `inject_check_for_test` so
    /// that `build_fea_convergence()` reads a non-None `last_check`.
    /// Not callable from production code.
    #[cfg(test)]
    pub(crate) fn emit_fea_convergence_for_test(&self) {
        self.emit_fea_convergence();
    }

    /// Drive `emit_mode_shape_frames_if_any` with a pre-built `CheckResult` in tests.
    ///
    /// Lets tests inject a hand-constructed `CheckResult` containing a
    /// `BucklingResult`-shaped cell without needing a full engine eval.
    /// Not callable from production code.
    #[cfg(test)]
    pub(crate) fn emit_mode_shape_frames_for_test_with_result(&self, check: &CheckResult) {
        self.emit_mode_shape_frames_if_any(check);
    }

    /// Expose the engine's warm pool for test-only manipulation (e.g. pre-populating
    /// events before asserting that `drain_and_emit_warm_pool_events` forwards them).
    #[cfg(test)]
    pub(crate) fn warm_pool_mut_for_test(&mut self) -> &mut reify_eval::warm_pool::WarmStatePool {
        self.core.engine_mut().warm_pool_mut()
    }

    /// Trigger a warm-pool drain-and-emit cycle in tests without needing a full
    /// engine check/build call. Used by step-5 tests to verify the emitter
    /// contract in isolation.
    #[cfg(test)]
    pub(crate) fn drain_and_emit_warm_pool_events_for_test(&mut self) {
        self.drain_and_emit_warm_pool_events();
    }

    /// Return a reference to the last `CheckResult` produced by `load_from_source`,
    /// `load_file`, `update_source`, or `set_parameter`.
    ///
    /// Mirrors the established `#[cfg(test)] pub(crate)` test-support pattern
    /// (emit_fea_case_for_test_with_result, drain_and_emit_warm_pool_events_for_test,
    /// warm_pool_mut_for_test) — delegates to `core.last_check()` without exposing
    /// the private `core` field.  Lets GUI tests read raw cell Values from
    /// `CheckResult.values` for B4 / value-cell assertions.
    ///
    /// Note: geometry-let cells (e.g. `let body = box(...)`) are NOT in
    /// `CheckResult.values` — they compile to realization nodes, not value cells.
    /// Use `compiled_for_test()` to inspect a template's `realizations` instead.
    #[cfg(test)]
    pub(crate) fn last_check_for_test(&self) -> Option<&reify_eval::CheckResult> {
        self.core.last_check()
    }

    /// Return a reference to the currently compiled `CompiledModule`, or `None`
    /// if no module has been compiled yet.
    ///
    /// Mirrors `last_check_for_test` — delegates to `core.compiled()` without
    /// exposing the private `core` field.  Lets GUI tests inspect a template's
    /// `realizations` (geometry-let bindings like `let body = box(...)` compile
    /// to realization nodes, not value cells, so they are absent from
    /// `CheckResult.values` but present in `template.realizations`).
    #[cfg(test)]
    pub(crate) fn compiled_for_test(&self) -> Option<&CompiledModule> {
        self.core.compiled()
    }

    /// Emit auto-resolve events if an emitter is installed and the check produced
    /// resolved auto-parameter values.
    ///
    /// Early-returns silently when:
    /// - No emitter is installed (`auto_resolve_emitter` is `None`), or
    /// - `check.resolved_params` is empty (no auto params were resolved).
    ///
    /// When both conditions are met, fires `start → iteration → complete` in order.
    /// Drain the engine's warm-pool event buffer, record each on the journal,
    /// and forward the translated IPC events to the installed
    /// [`WarmPoolEventEmitter`] (if any).
    ///
    /// Called via the single [`Self::post_engine_call_telemetry`] choke-point
    /// (INV-GUI-2) alongside [`Self::emit_auto_resolve_if_any`], after each
    /// engine call site that may produce donations or evictions (check,
    /// edit_check, build, tessellate_snapshot, etc.).
    ///
    /// When no emitter is installed, the drain still records events on the
    /// journal (M-010 wiring) but no IPC emission occurs.
    fn drain_and_emit_warm_pool_events(&mut self) {
        let raw_events = self.core.engine_mut().drain_and_record_warm_pool_events();
        if let Some(emitter) = &self.warm_pool_event_emitter {
            for ev in &raw_events {
                emitter.emit(crate::types::WarmPoolEvent::from_engine_event(ev));
            }
        }
    }

    fn emit_auto_resolve_if_any(&self, check: &CheckResult) {
        let emitter = match &self.auto_resolve_emitter {
            Some(e) => e,
            None => return,
        };
        if check.resolved_params.is_empty() {
            return;
        }

        let parameters = Self::build_parameters_payload(&check.resolved_params);
        let constraints = Self::build_constraints_payload(&check.constraint_results);

        let iter = AutoResolveIteration {
            iteration: 0,
            parameters,
            constraints,
            driving_metric: None,
            driving_metric_value: None,
        };

        emitter.start();
        emitter.iteration(iter);
        emitter.complete();
    }

    /// Detect a `MultiCaseResult`-shaped value in `check.values` and emit a
    /// `fea-case-changed` event on the first match.
    ///
    /// Fire-every-commit semantics (mirrors `emit_auto_resolve_if_any`): fires on
    /// every check that contains a matching cell — NO engine-side dedup.
    /// Values are iterated in sorted `ValueCellId` order for determinism.
    /// Returns after the first matching cell (one event per check, at most).
    ///
    /// Early-returns silently when no emitter is installed or when no cell in
    /// `check.values` matches the `MultiCaseResult` shape.
    fn emit_fea_case_if_any(&self, check: &CheckResult) {
        let emitter = match &self.fea_case_emitter {
            Some(e) => e,
            None => return,
        };

        // Single O(n) pass: find the MultiCaseResult cell with the
        // lexicographically-smallest ValueCellId for determinism.
        // `ValueCellId` derives `Ord` so comparison is direct — no `to_string()`
        // allocation per cell. In the no-match common case (no task-3026 data),
        // `filter_map` yields an empty iterator and `min_by` returns `None`
        // with zero allocations.
        if let Some((_, detected)) = check
            .values
            .iter()
            .filter_map(|(id, value)| {
                reify_eval::multi_load_dispatch::detect_multi_case_result(value)
                    .map(|d| (id, d))
            })
            .min_by(|(a, _), (b, _)| a.cmp(b))
        {
            let payload = crate::types::FeaCaseChanged {
                active_case_id: detected.active_case_id,
                available_cases: detected.available_cases,
            };
            emitter.changed(payload);
        }
    }

    /// Detect a `BucklingResult`-shaped value in `check.values` and emit one
    /// undeformed base frame (phase=0.0) plus one peak frame per mode (phase=1.0).
    ///
    /// Frame ordering: base frame first, then peak frames in ascending mode_index order.
    /// mode_index is the 0-based position of each mode in the modes list.
    ///
    /// Scale normalization (PRD §8): peak nodal displacement is scaled to ~10% of
    /// the node-set bounding-box diagonal so the deformed shape is always visible
    /// regardless of how the eigensolver normalizes eigenvectors.
    ///
    /// Early-returns silently when no emitter is installed or when `check.values`
    /// contains no `BucklingResult`-shaped cell.
    fn emit_mode_shape_frames_if_any(&self, check: &CheckResult) {
        let emitter = match &self.mode_shape_frame_emitter {
            Some(e) => e,
            None => return,
        };

        // Find the first BucklingResult StructureInstance in check.values.
        let (base_f64, modes_displaced, eigenvalues) = match Self::extract_buckling_data(&check.values) {
            Some(d) => d,
            None => return,
        };

        let n = base_f64.len(); // 3 · n_nodes

        // Emit undeformed base frame (phase=0.0, mode_index=0).
        //
        // NOTE: the base frame and the first peak frame (mode 0) intentionally
        // share mode_index=0.  `phase` is the sole discriminator: phase=0.0
        // identifies the undeformed reference; phase=1.0 identifies a mode-peak.
        // Consumers must key on `phase`, not `mode_index`, to distinguish them.
        let base_f32: Vec<f32> = base_f64.iter().map(|&v| v as f32).collect();
        emitter.frame(crate::types::ModeShapeFrame {
            mode_index: 0,
            phase: 0.0_f32,
            displaced_positions: base_f32.clone(),
            eigenvalue: None, // base frame has no associated mode eigenvalue
        });

        // Emit one peak frame per mode (phase=1.0).
        for (k, mode_disp) in modes_displaced.iter().enumerate() {
            // mode_index is u8 on the wire; assert no silent wrapping for large
            // n_modes values (normal buckling analyses are ≤ ~20 modes).
            debug_assert!(k < 256, "mode_index would overflow u8: n_modes={}", k + 1);

            // Displacement vector: displaced − base (per DOF).
            let displacement: Vec<f64> = base_f64
                .iter()
                .zip(mode_disp.iter())
                .map(|(&b, &d)| d - b)
                .collect();

            // Scale factor: normalize max nodal displacement to ~10% of bbox diagonal.
            let scale = Self::mode_shape_scale(&base_f64, &displacement);

            // Scaled peak positions: base + scale · displacement.
            let peak_f32: Vec<f32> = (0..n)
                .map(|i| (base_f64[i] + scale * displacement[i]) as f32)
                .collect();

            emitter.frame(crate::types::ModeShapeFrame {
                mode_index: k as u8,
                phase: 1.0_f32,
                displaced_positions: peak_f32,
                eigenvalue: Some(eigenvalues[k]), // per-mode buckling load multiplier λ
            });
        }
    }

    /// Extract `(base_node_positions: Vec<f64>, modes_displaced_positions: Vec<Vec<f64>>,
    /// eigenvalues: Vec<f64>)` from the first `BucklingResult`-shaped
    /// `Value::StructureInstance` in `values`.
    ///
    /// Returns `None` when:
    /// - no `StructureInstance` with `type_name == "BucklingResult"` is found, or
    /// - `base_node_positions` is absent/malformed, or
    /// - `modes` list is absent/malformed, or
    /// - any mode's `eigenvalue` field is absent or not `Value::Real`.
    #[allow(clippy::type_complexity)]
    fn extract_buckling_data(
        values: &reify_ir::ValueMap,
    ) -> Option<(Vec<f64>, Vec<Vec<f64>>, Vec<f64>)> {
        use reify_ir::Value;

        for (_, value) in values.iter() {
            let data = match value {
                Value::StructureInstance(d) if d.type_name == "BucklingResult" => d,
                _ => continue,
            };

            // Extract base_node_positions.
            let base_list = match data.fields.get(&"base_node_positions".to_string()) {
                Some(Value::List(v)) => v,
                _ => continue,
            };
            let base_f64: Vec<f64> = base_list.iter().filter_map(|v| {
                if let Value::Real(r) = v { Some(*r) } else { None }
            }).collect();
            if base_f64.len() != base_list.len() || base_f64.is_empty() {
                continue;
            }

            // Extract modes list.
            let modes_list = match data.fields.get(&"modes".to_string()) {
                Some(Value::List(v)) => v,
                _ => continue,
            };

            // Extract displaced_positions and eigenvalue for each mode.
            let mut modes_displaced = Vec::with_capacity(modes_list.len());
            let mut eigenvalues = Vec::with_capacity(modes_list.len());
            let mut all_ok = true;
            for mode_val in modes_list.iter() {
                let mode_data = match mode_val {
                    Value::StructureInstance(d) => d,
                    _ => { all_ok = false; break; }
                };
                // Extract eigenvalue (task 4072): must be Value::Real.
                let eigenvalue = match mode_data.fields.get(&"eigenvalue".to_string()) {
                    Some(Value::Real(r)) => *r,
                    _ => { all_ok = false; break; }
                };
                let mode_shape_map = match mode_data.fields.get(&"mode_shape".to_string()) {
                    Some(Value::Map(m)) => m,
                    _ => { all_ok = false; break; }
                };
                let disp_list = match mode_shape_map.get(&Value::String("displaced_positions".to_string())) {
                    Some(Value::List(v)) => v,
                    _ => { all_ok = false; break; }
                };
                let disp_f64: Vec<f64> = disp_list.iter().filter_map(|v| {
                    if let Value::Real(r) = v { Some(*r) } else { None }
                }).collect();
                if disp_f64.len() != base_f64.len() {
                    all_ok = false;
                    break;
                }
                eigenvalues.push(eigenvalue);
                modes_displaced.push(disp_f64);
            }
            if !all_ok || modes_displaced.is_empty() {
                continue;
            }

            return Some((base_f64, modes_displaced, eigenvalues));
        }
        None
    }

    /// Compute the mode-shape scale factor: normalize peak nodal displacement to
    /// ~10% of the node-set bounding-box diagonal (PRD §8).
    ///
    /// Falls back to `1.0` for degenerate inputs (all-zero displacement or
    /// degenerate / single-node bbox).
    fn mode_shape_scale(base: &[f64], displacement: &[f64]) -> f64 {
        // Bounding box of the undeformed node positions.
        let (mut min_x, mut min_y, mut min_z) = (f64::MAX, f64::MAX, f64::MAX);
        let (mut max_x, mut max_y, mut max_z) = (f64::MIN, f64::MIN, f64::MIN);
        for chunk in base.chunks(3) {
            if chunk.len() < 3 { continue; }
            min_x = min_x.min(chunk[0]); max_x = max_x.max(chunk[0]);
            min_y = min_y.min(chunk[1]); max_y = max_y.max(chunk[1]);
            min_z = min_z.min(chunk[2]); max_z = max_z.max(chunk[2]);
        }
        let dx = max_x - min_x;
        let dy = max_y - min_y;
        let dz = max_z - min_z;
        let bbox_diag = (dx * dx + dy * dy + dz * dz).sqrt();

        // Max nodal displacement magnitude (L2 norm per node).
        let max_disp = displacement
            .chunks(3)
            .map(|d| {
                let v = if d.len() >= 3 {
                    d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
                } else {
                    d.iter().map(|x| x * x).sum()
                };
                v.sqrt()
            })
            .fold(0.0_f64, f64::max);

        if max_disp > 0.0 && bbox_diag > 0.0 {
            0.1 * bbox_diag / max_disp
        } else {
            1.0 // degenerate fallback
        }
    }

    /// Build the `parameters` map for an `AutoResolveIteration` payload.
    ///
    /// For `Value::Scalar` resolved params, emits the engineering-unit display
    /// value, formatted number string, and unit symbol.
    ///
    /// For non-Scalar resolved params (which indicate a buggy or unexpected
    /// solver implementation — auto parameters are always physical quantities),
    /// emits a sentinel `{ value: f64::NAN, unit: "", display: "<non-scalar>" }`
    /// so the GUI panel can render an error chip instead of silently omitting the
    /// cell.  The `warn!` log is kept for ops observability.
    fn build_parameters_payload(
        resolved: &HashMap<ValueCellId, Value>,
    ) -> HashMap<String, AutoResolveParameterValue> {
        let mut out = HashMap::new();
        for (cell_id, value) in resolved {
            match value.format_display_triple() {
                Some((display_value, formatted, unit)) => {
                    out.insert(
                        cell_id.to_string(),
                        AutoResolveParameterValue {
                            value: display_value,
                            display: format!("{}{}", formatted, unit),
                            unit,
                        },
                    );
                }
                None => {
                    warn!(
                        "auto-resolve: resolved param {:?} is not a Scalar; emitted NaN sentinel",
                        cell_id
                    );
                    out.insert(
                        cell_id.to_string(),
                        AutoResolveParameterValue {
                            value: f64::NAN,
                            unit: String::new(),
                            display: "<non-scalar>".to_string(),
                        },
                    );
                }
            }
        }
        out
    }

    /// Build the `constraints` map for an `AutoResolveIteration` payload.
    ///
    /// Projects each `ConstraintCheckEntry` to `{ name, value: None, unit: None,
    /// target_lower: None, target_upper: None, satisfied }`.  `value` is `None`
    /// because the kernel does not yet expose per-constraint observed/target
    /// scalars at the CheckResult boundary; emitting `0.0` would be a wire-level
    /// lie (indistinguishable from a genuine zero observation).
    ///
    /// `name` prefers the user-authored `label` over the synthetic `id` so the
    /// GUI panel indicator row shows human-readable names.  The map key is always
    /// `id.to_string()` for stable lookup by the frontend.
    fn build_constraints_payload(
        results: &[reify_eval::ConstraintCheckEntry],
    ) -> HashMap<String, AutoResolveConstraintProgress> {
        let mut out = HashMap::new();
        for r in results {
            let id_str = r.id.to_string();
            let name = r.label.clone().unwrap_or_else(|| id_str.clone());
            out.insert(
                id_str,
                AutoResolveConstraintProgress {
                    name,
                    value: None,
                    unit: None,
                    target_lower: None,
                    target_upper: None,
                    satisfied: matches!(r.satisfaction, Satisfaction::Satisfied),
                },
            );
        }
        out
    }

    /// Create a new EngineSession with the given constraint checker and optional geometry kernel.
    pub fn new(
        checker: Box<dyn ConstraintChecker>,
        kernel: Option<Box<dyn GeometryKernel>>,
    ) -> Self {
        Self::from_engine(Engine::new(checker, kernel))
    }

    /// Create a new EngineSession using the inventory-based kernel registry.
    ///
    /// This is the production-binary boot path. Reads the static
    /// linker-collected set of [`reify_types::KernelRegistration`] records once
    /// at construction, picks the lexicographically smallest entry, and
    /// instantiates the geometry kernel via its registered factory — mirroring
    /// [`Engine::with_registered_kernel`]'s contract exactly.
    ///
    /// When no kernel adapter has submitted a registration (stub-mode build,
    /// `cfg(has_occt)` off), the underlying engine receives `None` as the
    /// geometry kernel, matching `Engine::new(checker, None)` semantics.
    ///
    /// Unit tests that require a mock or failing kernel should continue to
    /// use `EngineSession::new(checker, Some(Box::new(MockGeometryKernel::new())))` —
    /// the kernel-injection seam is preserved for that use-case.
    ///
    /// ## Production solver
    ///
    /// The production solver set (`SolverRegistry::production()`: DimensionalSolver +
    /// geometric SolveSpaceSolver) is installed here — the only place where the
    /// documented production-binary boot path runs.  Deliberately NOT installed in
    /// the shared `from_engine`/`EngineSession::new` path so that `new`-based unit
    /// tests keep `solver = None` and are unperturbed.
    pub fn with_registered_kernel(checker: Box<dyn ConstraintChecker>) -> Self {
        let engine = Engine::with_registered_kernel(checker)
            .with_solver(Box::new(reify_constraints::SolverRegistry::production()));
        Self::from_engine(engine)
    }

    /// Load source code, parse, compile, evaluate, and return full GUI state.
    pub fn load_from_source(
        &mut self,
        source: &str,
        module_name: &str,
    ) -> Result<GuiState, String> {
        let (parsed, compiled) =
            compile_single_file_with_stdlib(source, module_name).map_err(|(msg, diags)| {
                self.record_compile_failure(diags, source, module_name);
                msg
            })?;

        // Evaluate + check constraints (borrows compiled by shared ref, so all
        // field mutations can safely be deferred until after check() returns).
        // check_with_solve_slot fires the SolveCancellationSink lifecycle around
        // engine.check() — publish handle before, clear after (task γ/4086).
        let check_result = self.check_with_solve_slot(&compiled);

        // Atomically commit all state after check() succeeds.
        // Preserve file_path: load_from_source has no file on disk; keep any
        // existing file_path from a prior load_file call.
        self.commit_state(parsed, compiled, check_result, module_name, source, FilePathUpdate::Preserve);

        // Emit auto-resolve events after committing state.
        //
        // Cross-cutting ordering invariant: all four mutating entry points
        // (load_from_source, load_file, update_source, set_parameter) emit AFTER all
        // session state mutations are committed.  Combined with `core.commit_state` /
        // `core.commit_check` writing `last_check` unconditionally, a panic during state
        // commit cannot leak phantom auto-resolve events to the GUI.
        self.post_engine_call_telemetry();

        self.build_gui_state()
    }

    /// The shared cell lookup: the declared [`reify_core::Type`] of the value
    /// cell `cell_id` names in some compiled template, a discriminated `Err`
    /// otherwise.
    ///
    /// Both entry points that mutate a parameter's value run it — the ephemeral
    /// engine-state edit ([`Self::set_parameter`], what the property-panel
    /// slider drives today, which needs the type to make its parse
    /// dimension-aware per task #5757) and the INV-GUI-3 source write-back
    /// ([`Self::resolve_rewritable_default_span`], which
    /// [`Self::apply_param_to_source`] resolves through and which needs only the
    /// existence half, via [`Self::require_known_cell`]). They MUST agree about
    /// what a cell id denotes: a slider that moves a param the write-back
    /// reports as unknown (or the reverse) is a contradiction the user has no
    /// way to make sense of.
    ///
    /// Agreement by ONE function rather than by two hand-copied predicates,
    /// because nothing structural stops one copy from being updated and the
    /// other not — the next time this lookup widens (searching realizations as
    /// well as templates, say) is exactly when they would drift.
    ///
    /// The type is BORROWED out of `compiled()` rather than cloned, so the
    /// existence-only caller ([`Self::require_known_cell`], the write-back's
    /// gate) pays nothing for a value it discards. `set_parameter`, the one
    /// caller that genuinely needs an owned `Type` — it needs `&mut self`
    /// afterwards, which this borrow would block — clones at its own call site.
    fn resolve_known_cell_type(
        &self,
        cell_id: &ValueCellId,
        cell_id_str: &str,
    ) -> Result<&reify_core::Type, String> {
        let compiled = self
            .core
            .compiled()
            .ok_or_else(|| "No module loaded".to_string())?;
        compiled
            .templates
            .iter()
            .find_map(|t| t.value_cells.iter().find(|vc| vc.id == *cell_id))
            .map(|vc| &vc.cell_type)
            .ok_or_else(|| format!("Unknown parameter '{}'", cell_id_str))
    }

    /// The existence half of [`Self::resolve_known_cell_type`]: `Ok(())` when
    /// `cell_id` names a value cell of some compiled template.
    ///
    /// The write-back path splices a source literal and has no use for the
    /// declared type — but it must refuse exactly the cell ids `set_parameter`
    /// refuses, so it asks the same function and discards the type rather than
    /// carrying a second predicate.
    fn require_known_cell(&self, cell_id: &ValueCellId, cell_id_str: &str) -> Result<(), String> {
        self.resolve_known_cell_type(cell_id, cell_id_str).map(|_| ())
    }

    /// Set a parameter value by cell ID string and value string.
    ///
    /// `cell_id_str` is "Entity.member" (e.g., "Bracket.width").
    /// `value_str` is a quantity literal (e.g., "120mm"), a boolean, or — for an
    /// UNDIMENSIONED cell, or one whose dimension no curated ladder covers — a
    /// plain number. A plain number for a covered dimension is refused with a
    /// message naming a rung of that cell's own ladder; `parse_value_string_for_cell`
    /// owns that rule and states why it is keyed on expressibility (task #5757).
    pub fn set_parameter(
        &mut self,
        cell_id_str: &str,
        value_str: &str,
    ) -> Result<GuiState, String> {
        let cell_id = parse_cell_id(cell_id_str)?;

        // Resolve the cell in the compiled module BEFORE parsing (task #5757).
        // The declared type is what makes the parse dimension-aware, and doing
        // the lookup first also keeps "Unknown parameter" ahead of any parse
        // diagnostic — an unknown cell is the more specific complaint.
        //
        // The clone lives HERE, at the one call site that needs an owned type:
        // `with_solve_slot` below needs `&mut self`, which the `compiled()`
        // borrow would block. The shared lookup hands back a borrow so the
        // existence-only caller (`require_known_cell`) pays nothing for it.
        // The lookup itself is `resolve_known_cell_type` so this path and the
        // INV-GUI-3 write-back agree by construction about what a cell id
        // denotes — see that function.
        let cell_type = self.resolve_known_cell_type(&cell_id, cell_id_str)?.clone();

        let value = parse_value_string_for_cell(value_str, &cell_type)?;

        // Task #5338: captured BEFORE `edit_check` consumes `cell_id`. Only the
        // entity half is needed, and the invalidation must happen after the commit
        // below, so the alternative would be cloning the whole `ValueCellId` into
        // the solve closure on every edit.
        let edited_entity = cell_id.entity.clone();

        // with_solve_slot fires the SolveCancellationSink lifecycle around
        // edit_check (task γ/4086): solve_started before, solve_finished after.
        // SolveFinishedGuard inside with_solve_slot ensures solve_finished fires
        // even when edit_check returns Err and the `?` short-circuits.
        let check_result = self.with_solve_slot(|s| {
            s.core
                .engine_mut()
                .edit_check(cell_id, value)
                .map_err(|e| format!("Engine error: {}", e))
        })?;

        // Commit state first; emit_auto_resolve_if_any reads back via last_check()
        // so it fires AFTER all mutations are complete — cross-cutting ordering invariant.
        self.core.commit_check(check_result);
        // Task #5338: the warm-edit invalidation trigger. Placement is load-bearing
        // on BOTH sides — after the commit, so a FAILED edit (the `?` on
        // `with_solve_slot` above short-circuits) leaves retention intact; before
        // the rebuild, so the very pass that would otherwise replay the pre-edit
        // value already finds no entry to replay.
        self.invalidate_geometry_derived_cache_for_entity(&edited_entity);
        self.post_engine_call_telemetry();
        self.build_gui_state()
    }

    /// Write `value` back into the session's canonical `.ri` file as the
    /// source-of-truth edit for `cell_id_str`'s default literal.
    ///
    /// This is the INV-GUI-3 primitive: **the `.ri` source is the canonical
    /// truth of the design for all mutations** (task 5096 γ, PRD
    /// `docs/prds/v0_6/ai-native-editing.md` §6.1, D1/D6/D7). Every durable
    /// value mutation — the MCP write tools (δ) and the GUI slider once it is
    /// re-homed (η) — is meant to land here rather than as an ephemeral
    /// engine-state override, so that a design's on-disk text and the engine's
    /// idea of it can never diverge.
    ///
    /// # Phase order
    ///
    /// **resolve → serialize → splice → confirm disk → recompile → write.**
    ///
    /// 1. **Resolve** the default span this cell may be written through, via
    ///    [`Self::resolve_rewritable_default_span`].
    /// 2. **Serialize** `value` with [`reify_ir::value_to_ri_literal_with_unit`],
    ///    hinted by the unit read off the literal being replaced
    ///    ([`unit_hint_from_default_literal`]) so `80mm` stays millimetres
    ///    instead of hopping to the canonical ladder.
    /// 3. **Splice** by BYTE offset — a minimal replacement of just that span,
    ///    never a re-serialization of the file, so comments, whitespace and
    ///    every other declaration survive byte for byte (D6: no round-tripping
    ///    pretty-printer exists, and inventing one here would silently reformat
    ///    the user's document on every parameter tweak).
    /// 4. **Confirm** the on-disk file still holds the text this session
    ///    compiled, and REFUSE rather than clobber it if not. INV-GUI-3 makes
    ///    the `.ri` canonical for the engine; it does not make this process the
    ///    file's only writer. The rationale, the two ordinary causes and the
    ///    two qualifications are stated once, at the check itself.
    /// 5. **Recompile** in process through [`Self::update_source`]. The
    ///    recompile precedes the write, and that ordering is load-bearing —
    ///    stated once, at the call site.
    /// 6. **Write** the spliced text to disk. Replace-atomic and its residual
    ///    are [`write_file_atomically`]'s contract, stated once there.
    ///
    /// # Atomicity ledger
    ///
    /// Four state surfaces move together or not at all (the §6.1 invariant: on
    /// success they are mutually consistent, on failure NONE are mutated):
    ///
    /// - the on-disk `.ri` file;
    /// - the parse/compile surface — `source_map`, `parsed_cache`, `compiled`,
    ///   `last_check` — which [`Self::update_source`] commits as one unit;
    /// - the failure surfaces `compile_failure` and `last_reload_error`, which
    ///   drive the diagnostics list and the hot-reload staleness banner;
    /// - engine eval state, as read back through [`Self::build_gui_state`].
    ///
    /// Per failure phase:
    ///
    /// - **Resolve** (unknown cell, malformed id, an entity that is not the
    ///   entry file's, no default, non-literal default, a unit the emitter
    ///   cannot put back) returns before ANY of the four is touched.
    /// - **Serialize** (no `.ri` literal re-parses to this value — a non-finite
    ///   real, say) likewise: nothing has been mutated at that point.
    /// - **No file to write** — a `load_from_source` session has no canonical
    ///   `.ri` at all, and is refused rather than degraded to the engine-state
    ///   edit INV-GUI-3 exists to replace — likewise.
    /// - **Disk divergence** (the file no longer holds the text this session
    ///   compiled) likewise: the check runs before the recompile precisely so
    ///   its refusal costs no rollback.
    /// - **Recompile** rejection restores `compile_failure` and
    ///   `last_reload_error` from a snapshot taken immediately before the call,
    ///   so the rejected text leaves no diagnostics behind.
    /// - **Disk-write** failure rolls the engine back by recompiling the
    ///   pre-edit text through [`Self::update_source`], so the engine is never
    ///   left ahead of what is on disk, and restores the SAME snapshot
    ///   afterwards — the rollback recompile succeeds, and a successful
    ///   `commit_state` would otherwise clear a staleness banner this call
    ///   never earned the right to clear.
    ///
    /// Both restores live at their own call sites, with the reason each one
    /// belongs there rather than inside [`Self::update_source`].
    ///
    /// # Exactly one emit (D7)
    ///
    /// There is deliberately NO second emit path here. The frontend
    /// notification rides [`Self::update_source`]'s `post_engine_call_telemetry`
    /// — the one shared gui-state-sync choke-point — including on the
    /// disk-write rollback, which is routed through `update_source` for
    /// precisely that reason. The in-process recompile is authoritative and the
    /// FS-watcher re-fire reloads identical content for an empty delta (D7,
    /// §7 B5), so a second emit would buy nothing and cost the single-source
    /// property. δ and η must not add one either; reconcile any further
    /// divergence at `gui-state-sync`, which owns that seam.
    ///
    /// # Why a non-literal default is refused
    ///
    /// A default that is a `BinOp`, an `Auto`, a call or an identifier is
    /// REFUSED rather than spliced over. `param depth = width * 2` encodes a
    /// user-authored parametric relationship and `param length = auto` encodes
    /// a solver-determined value; overwriting either with a constant destroys
    /// it silently, and the user would discover it only when the design stopped
    /// responding to the parameter it used to follow. Refusing hands the caller
    /// a structured rejection to surface instead — see
    /// [`Self::resolve_rewritable_default_span`] for the discriminated taxonomy
    /// δ maps into its tool results.
    ///
    /// A literal in a unit the emitter cannot put back (`200mil`) is refused on
    /// the same grounds and discriminated apart from this one — see
    /// [`unit_is_emittable_as_written`], which owns that rule.
    pub fn apply_param_to_source(
        &mut self,
        cell_id_str: &str,
        value: &Value,
    ) -> Result<GuiState, String> {
        let span = self.resolve_rewritable_default_span(cell_id_str)?;
        let (_, source) = self
            .resolve_source()
            .ok_or_else(|| "no module loaded".to_string())?;

        let old = &source[span.start as usize..span.end as usize];
        let literal =
            reify_ir::value_to_ri_literal_with_unit(value, unit_hint_from_default_literal(old))
                .map_err(|e| format!("cannot serialize value for '{cell_id_str}': {e}"))?;

        let mut new_source = String::with_capacity(source.len() - old.len() + literal.len());
        new_source.push_str(&source[..span.start as usize]);
        new_source.push_str(&literal);
        new_source.push_str(&source[span.end as usize..]);

        // The PRE-EDIT buffer, owned. It is the rollback text for a failed disk
        // write below, and it has to be owned in any case: `source` borrows
        // `&self`, while the recompile that follows needs `&mut self`.
        let original = source.to_owned();

        let path = self
            .core
            .file_path()
            .ok_or_else(|| "session has no on-disk .ri file to write".to_string())?
            .to_path_buf();
        let path_str = path
            .to_str()
            .ok_or_else(|| format!("path {} is not valid UTF-8", path.display()))?;

        // INV-GUI-3 makes the `.ri` canonical for the ENGINE; it does not make
        // this process the file's only writer. The write below replaces the
        // whole file with the in-memory buffer, so a divergence between the two
        // would be resolved by DESTROYING the disk side wholesale — not just at
        // the spliced span. That divergence has two ordinary causes: an
        // external editor saved the file and the FS-watcher has not re-fired
        // `update_source` yet, or the GUI editor's dirty-buffer path (which
        // calls `update_source` per keystroke and never writes disk) is holding
        // text the user has not saved. Refuse both rather than clobber: a
        // parameter tweak must not discard another writer's edit, and must not
        // force-save a document behind the user's back.
        //
        // Placed BEFORE the recompile so the refusal is a resolve-phase-shaped
        // rejection that costs no rollback. It narrows the race rather than
        // closing it — a writer landing between here and the rename still
        // loses, which only locking the GUI does not take could prevent.
        //
        // A file that cannot be READ is deliberately NOT treated as divergence:
        // there are no bytes to preserve and no comparison to make, so it falls
        // through to the write, which fails and rolls the engine back with an
        // error naming the real problem.
        if let Ok(on_disk) = std::fs::read_to_string(&path)
            && on_disk != original
        {
            return Err(format!(
                "refusing to write '{cell_id_str}' back to {}: the file on disk no longer \
                 matches the source this session compiled (an external edit, or unsaved \
                 editor changes) — reload or save it first",
                path.display()
            ));
        }

        // The recompile runs BEFORE the disk write: it is the step that can
        // legitimately reject the edit (type/dimension mismatch), and writing
        // disk first would leave the on-disk `.ri` holding text the engine
        // rejected — the FS-watcher would then reload the broken file into
        // the GUI. Ordering recompile→write makes that state unreachable.
        //
        // The failure-diagnostic surfaces are snapshotted across that call and
        // restored if it rejects. `update_source`'s failure path calls
        // `record_compile_failure`, which stores the text it failed to compile
        // together with diagnostics indexed into it — load-bearing for the
        // EDITOR path, where that text IS the buffer the user is looking at.
        // The write-back is the opposite case: the failed text was synthesized
        // by this method from a value the caller supplied, was never shown to
        // anyone, and reached neither disk nor `source_map`. Surfacing
        // diagnostics against it would point the user at lines of a document
        // that does not exist. So the restore belongs HERE, at the one call
        // site with that property — do NOT "fix" this by pushing it down into
        // `update_source`, which would blind the editor path.
        //
        // Error path only: a SUCCESSFUL `update_source` clears both fields via
        // `commit_state`, which is exactly right.
        let failure_surface = (self.compile_failure.clone(), self.last_reload_error.clone());
        let state = match self.update_source(path_str, &new_source) {
            Ok(state) => state,
            Err(e) => {
                (self.compile_failure, self.last_reload_error) = failure_surface;
                return Err(e);
            }
        };

        if let Err(e) = write_file_atomically(&path, &new_source) {
            let write_err = format!("Error writing {}: {e}", path.display());
            // The engine committed and disk did not, so the engine is now AHEAD
            // of the canonical `.ri` — the exact inconsistency INV-GUI-3 exists
            // to forbid. Roll it back to the pre-edit text.
            //
            // Routed through `update_source`, NOT a hand-rolled `commit_state`,
            // so the restored state reaches the frontend through the ONE shared
            // choke-point (`post_engine_call_telemetry`) exactly as the forward
            // commit did. A failed write therefore fires two emits, both down
            // the same path, and the LAST one the frontend sees is the pre-edit
            // state.
            //
            // The rollback recompile cannot fail on well-formed input — this is
            // text that compiled moments ago — but its `Err` is still handled
            // rather than ignored: a session left silently inconsistent is
            // worse than a loud combined error.
            //
            // The rollback restores the ENGINE and deliberately does not touch
            // disk. It does not have to: `write_file_atomically` fails BEFORE
            // the rename or not at all, so the on-disk `.ri` still holds the
            // pre-edit text this rollback returns the engine to — the two agree
            // again without a second write down the path that just failed.
            return match self.update_source(path_str, &original) {
                Ok(_) => {
                    // The SAME snapshot restore as the recompile-rejection arm
                    // above, for the same reason and one step later. This call
                    // has failed, so the ledger says none of the four surfaces
                    // may move — but the rollback `update_source` SUCCEEDED,
                    // and a successful `commit_state` clears `compile_failure`
                    // and `last_reload_error` unconditionally, including any
                    // that PREDATE this call. Without the restore a failed
                    // write silently clears a staleness banner the user's
                    // last hot reload really did earn, and `is_stale()` starts
                    // claiming the GUI is in sync with a reload that never
                    // succeeded.
                    //
                    // Restored only on the Ok arm on purpose: if the rollback
                    // recompile itself failed, `record_compile_failure` has
                    // just stored a diagnostic about a REAL, current
                    // inconsistency (the engine is stuck on the post-edit
                    // text), and overwriting it with the pre-edit snapshot
                    // would hide exactly the state the combined error below is
                    // shouting about.
                    (self.compile_failure, self.last_reload_error) = failure_surface;
                    Err(write_err)
                }
                Err(restore_err) => Err(format!(
                    "{write_err}; the engine could not be rolled back to the pre-edit \
                     source either: {restore_err}"
                )),
            };
        }

        Ok(state)
    }

    /// Resolve the byte range [`Self::apply_param_to_source`] may splice over,
    /// or a DISCRIMINATED rejection saying which of the four preconditions
    /// failed (PRD §7 B7 — δ, the MCP `set_parameter` tool, is the consumer
    /// that maps these categories into its tool result).
    ///
    /// α's [`Self::resolve_param_default_span`] collapses every one of these
    /// into one `Option::None`, the right shape for a *resolver* and the wrong
    /// shape for an *entry point*: "you named an entity that does not exist"
    /// and "that param's default is a formula I refuse to overwrite" call for
    /// opposite responses from the caller. The categories, in the order they
    /// are checked:
    ///
    /// 1. **Malformed cell id** — no `.` at all, so it never denoted a cell.
    ///    Propagated verbatim from `parse_cell_id`.
    /// 2. **Unknown parameter** — the cell id is well-formed but names no cell
    ///    in `compiled.templates[].value_cells`. Checked through the SHARED
    ///    [`Self::require_known_cell`] rather than a second copy of the
    ///    predicate, deliberately: this entry point and [`Self::set_parameter`]
    ///    must agree about what a cell id denotes, or the slider and the
    ///    write-back would disagree about which params exist.
    /// 3. **Not the entry file's entity** — the cell exists, but its entity is
    ///    not declared in the module this session can rewrite. The commonest
    ///    case is a param of an IMPORTED `.ri`, whose pub template
    ///    `compile_entry_with_imports` merges into `compiled.templates` (so it
    ///    passes 2) while its text never enters `source_map` or `parsed_cache`
    ///    (so there is nothing here to splice). Discriminated ahead of 4
    ///    because it would otherwise be misreported as "no default expression".
    /// 4. **No default expression** — a real, editable cell whose param has
    ///    nothing to rewrite. This bucket also absorbs the AST-walk refusals α
    ///    documents (a name declared in more than one guarded branch; a param
    ///    reachable only through a port body or an instance path), which is
    ///    why the message hedges rather than asserting "declared without a
    ///    default".
    /// 5. **Non-literal default** — the gate this whole method exists for. See
    ///    [`Self::apply_param_to_source`] for why a `BinOp`/`Auto`/call/ident
    ///    default is REFUSED rather than spliced over.
    /// 6. **Unwritable unit** — the default IS an admitted literal, but its unit
    ///    is not one the emitter can put back (`200mil`, `2km`, a compound
    ///    expression). Discriminated apart from 5 because the default is not a
    ///    formula and saying so would send δ's caller looking for one. See
    ///    [`unit_is_emittable_as_written`], which owns the rule and the reason.
    ///
    /// Every arm returns before any of the four state surfaces is touched.
    fn resolve_rewritable_default_span(
        &self,
        cell_id_str: &str,
    ) -> Result<reify_core::SourceSpan, String> {
        let cell_id = parse_cell_id(cell_id_str)?;

        self.require_known_cell(&cell_id, cell_id_str)?;

        // The existence gate above searches `compiled.templates`, into which
        // `compile_entry_with_imports` MERGES every direct import's pub
        // templates — so a param declared in an imported `.ri` passes it. The
        // default-expression walk below searches `parsed_cache`, which holds
        // the entry module's declarations and nothing else (imported file
        // contents never enter `source_map` either — the v1 limitation
        // documented on `compile_entry_with_imports`). Without this arm an
        // imported param would fall out of the walk as `None` and be reported
        // as "no default expression", which is not what happened and would send
        // δ's caller looking for a default that is right there in the other
        // file.
        //
        // Refusing is the honest answer, not a placeholder: writing back to an
        // imported module would need that module's text in `source_map` (so the
        // splice has bytes to work on) and its own recompile+write, neither of
        // which exists yet.
        let parsed = self.parsed_cache.as_ref().ok_or_else(|| {
            format!("cannot rewrite '{cell_id_str}': this session has no parsed entry source")
        })?;
        if !entry_declares_entity(parsed, &cell_id.entity) {
            return Err(format!(
                "cannot write '{cell_id_str}' back to source: '{}' is not declared as a \
                 top-level structure or occurrence in the entry file (it is declared in an \
                 imported module, or nested in a form the write-back does not reach), and \
                 write-back only rewrites the entry file",
                cell_id.entity
            ));
        }

        let default = self
            .resolve_param_default_expr(cell_id_str)
            .ok_or_else(|| {
                format!(
                    "parameter '{cell_id_str}' has no default expression to rewrite in \
                     source (it may be declared without a default, declared in more than \
                     one guarded branch, or not addressable by its bare name)"
                )
            })?;

        match &default.kind {
            // A quantity literal is admitted only when its unit is one the
            // emitter can WRITE BACK — see `unit_is_emittable_as_written`.
            reify_ast::ExprKind::QuantityLiteral { unit, .. } => {
                if unit_is_emittable_as_written(unit) {
                    Ok(default.span)
                } else {
                    Err(format!(
                        "cannot write '{cell_id_str}' back to source: its default is written \
                         in {}, which the write-back cannot re-emit — rewriting it would \
                         silently replace the unit you authored with a built-in one \
                         ({}), so the existing literal is preserved instead",
                        describe_unit_expr(unit),
                        reify_core::units::BUILTIN_UNITS
                            .iter()
                            .map(|(s, ..)| *s)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ))
                }
            }
            reify_ast::ExprKind::NumberLiteral { .. }
            | reify_ast::ExprKind::StringLiteral(_)
            | reify_ast::ExprKind::BoolLiteral(_) => Ok(default.span),
            other => Err(format!(
                "cannot write '{cell_id_str}' back to source: its default is not a literal \
                 ({}), so the existing expression is preserved rather than overwritten",
                expr_kind_name(other)
            )),
        }
    }

    /// Task #5338: drop the geometry-derived value retention for one ENTITY —
    /// the WARM-EDIT sibling of the `sync_demand` visibility prune and of the
    /// `commit_state` full clear. Those three are the only invalidation triggers
    /// for `geometry_derived_cache`; each is documented at its own site.
    ///
    /// ## Why a warm edit needs its own trigger
    ///
    /// The retention contract's other half — "a fresh non-`Undef` delta entry
    /// always wins over the retained one" — is structurally UNAVAILABLE here. A
    /// warm `set_parameter` can change an input that reaches a mass-prop cell
    /// without changing any geometry-op scalar argument: `mass` is
    /// `volume(geometry) * material.density` and `moment_of_inertia` is
    /// `moment_of_inertia(geometry, body_density)`, so editing a density (or any
    /// param folded into `Material(...)`) moves the answer while leaving every op
    /// arg alone. `RealizationNodeData.input_cone_hash` folds ONLY the
    /// realization's own geometry-op scalar args
    /// (`compute_realization_upstream_values_hash_from_ops`, reify-eval
    /// engine_build.rs), so such a realization stays HASH-EXEMPT: it is dropped
    /// from the scheduled seed, its kernel ops never run, and the delta can carry
    /// NO fresh value to overwrite the retained one. Without this call the panel
    /// would serve the PRE-EDIT mass as `determined` / `freshness = "final"`,
    /// which is strictly worse than the pre-#5338 behaviour.
    ///
    /// Dropping the entry is therefore the only available answer. The cell
    /// degrades to `Undef` — exactly the pre-#5338 reading, i.e. the fail-safe
    /// direction — until the next dispatch resolves it for real. Pinned by
    /// `warm_edit_of_a_non_op_arg_mass_input_does_not_replay_a_stale_mass`.
    ///
    /// ## Why entity-scoped and NOT a blunt `clear()`
    ///
    /// A full clear would drop every UNAFFECTED entity's retention too, and those
    /// entities stay hash-exempt until the next recompile — so their mass-prop
    /// cells would read `Undef` indefinitely after any unrelated edit. That is
    /// #5338 itself, re-opened for every body the user did not touch. Pinned by
    /// `warm_edit_does_not_collaterally_drop_another_bodys_retained_mass_props`,
    /// which fails at its untouched second body if the shortcut is ever taken.
    ///
    /// ## Residual: cross-entity inputs are still not invalidated
    ///
    /// The join is on the EDITED cell's own entity, so an input reached only
    /// through ANOTHER entity — a module-level `body_density` consumed by
    /// `Body.material`, say — leaves `Body`'s entry in place, and `Body` is itself
    /// hash-exempt across that edit, so it can still replay stale. Closing that
    /// exactly needs an eval-side signal this crate does not have: either an
    /// `input_cone_hash` (or sibling hash) that also covers post-process inputs
    /// such as `material.density`, or a forward-dependency query exposed on
    /// `Engine` (`DependencyMap::forward_reachable`, reify-eval deps.rs, exists
    /// but is not reachable through `Engine`'s public surface). Filed as a
    /// follow-up under ticket `tkt_0RS0ZAKJPSH78DVB56QJ5E5V13`; deliberately not
    /// written as a tracked-pattern comment, since the curator assigns the task id
    /// asynchronously and a cite must resolve to a live task to be valid.
    fn invalidate_geometry_derived_cache_for_entity(&mut self, entity: &str) {
        self.geometry_derived_cache
            .retain(|id, _| id.entity != entity);
    }

    /// Return a shared reference to the underlying [`Engine`] for
    /// OBSERVATIONAL reads only (selective-demand ε, task 4741).
    ///
    /// Surfaces the engine's non-gated observational accessors
    /// ([`Engine::last_dispatch_count_by_realization`], [`Engine::last_eval_set`],
    /// [`Engine::demand_is_full_scope`]) to the debug-MCP projections
    /// [`crate::commands::engine_state_json`] / [`crate::commands::demand_dispatch_json`]
    /// without exposing the private `core` field. Reading through this accessor
    /// CANNOT perturb evaluation (it borrows `&self`), mirroring the
    /// already-non-gated `last_eval_set` / `last_demand_prune_measurement`
    /// reads that `build_gui_state` performs internally via `self.core.engine()`.
    pub(crate) fn engine(&self) -> &Engine {
        self.core.engine()
    }

    /// Synchronize the engine's PASSIVE observed-demand registry from the GUI's
    /// current display state (selective-demand precondition, task 4532).
    ///
    /// The three inputs are the spec §3.2 observed-demand sources:
    /// * `visible_realizations` — viewport mesh keys in `RealizationNodeId`
    ///   Display form (`Entity#realization[N]`),
    /// * `displayed_cells` — property-panel cell ids (`Entity.member`),
    /// * `panel_constraints` — constraint-panel ids in `ConstraintNodeId`
    ///   Display form (`Entity#constraint[N]`).
    ///
    /// Each is registered as a root on the engine's side-channel
    /// `observed_demand` registry, then the observed cone is rebuilt. The NEXT
    /// edit records a would-prune [`reify_eval::DemandPruneMeasurement`],
    /// surfaced via [`crate::types::GuiState::demand_prune_measurement`].
    ///
    /// OBSERVATIONAL ONLY. This NEVER touches the production `demand` registry,
    /// and the observed cone is NEVER fed to `compute_eval_set`; registering
    /// observed demand therefore cannot perturb `EvalResult` / `last_eval_set`
    /// (locked by the engine test
    /// `sync_observed_demand_is_zero_behavior_change_and_records_measurement`).
    /// Unparseable entries are skipped with a warning, never a panic. See
    /// `docs/prds/v0_6/selective-demand.md` §G6.
    pub fn sync_observed_demand(
        &mut self,
        visible_realizations: &[String],
        displayed_cells: &[String],
        panel_constraints: &[String],
    ) {
        let engine = self.core.engine_mut();
        // The GUI always sends the COMPLETE current display state, so reset the
        // observed roots first rather than accumulating across syncs.
        engine.reset_observed_demand();

        for key in visible_realizations {
            match parse_realization_key(key) {
                Some(rid) => engine.add_observed_demand(NodeId::Realization(rid)),
                None => warn!(
                    realization_key = %key,
                    "sync_observed_demand: skipping unparseable realization key"
                ),
            }
        }
        for cell in displayed_cells {
            match parse_cell_id(cell) {
                Ok(vc) => engine.add_observed_demand(NodeId::Value(vc)),
                Err(e) => warn!(
                    cell = %cell,
                    error = %e,
                    "sync_observed_demand: skipping unparseable cell"
                ),
            }
        }
        for constraint in panel_constraints {
            match parse_constraint_key(constraint) {
                Some(cid) => engine.add_observed_demand(NodeId::Constraint(cid)),
                None => warn!(
                    constraint_id = %constraint,
                    "sync_observed_demand: skipping unparseable constraint id"
                ),
            }
        }

        engine.rebuild_observed_cone();
    }

    /// Register the GUI's viewport-visible realizations as the PRODUCTION
    /// selective demand (ENFORCEMENT, task 4737 α).
    ///
    /// Parses each `Entity#realization[N]` mesh key with
    /// [`parse_realization_key`] and passes the resulting `Realization` roots to
    /// [`reify_eval::Engine::set_demand_selective`], which REPLACES the demand
    /// roots, turns the cold full-scope override OFF, and rebuilds the cone
    /// against the current snapshot graph — so the next warm `edit_param`
    /// schedules only the backward closure of the visible bodies and prunes any
    /// HIDDEN body's exclusive value cells.
    ///
    /// Unlike [`Self::sync_observed_demand`] — the task-4532 PASSIVE measurement
    /// side-channel, left untouched — this drives the registry
    /// `compute_eval_set` actually reads, so it INTENTIONALLY changes scheduling.
    /// The caller sends the COMPLETE current visible set (`show` + `ghost`,
    /// excluding `hidden`); `set_demand_selective` REPLACES rather than
    /// accumulates, so passing the full set each sync is correct. Unparseable
    /// keys are skipped with a warning, never a panic (mirrors
    /// `sync_observed_demand`).
    ///
    /// ## Task #5338: prune-safety chokepoint for `geometry_derived_cache`
    ///
    /// This is the ONLY place a realization's visibility can change, so it is
    /// also where the geometry-derived value retention cache is prune-filtered:
    /// entries whose entity has no realization in the incoming visible set are
    /// DROPPED, so `surface_geometry_derived_cells` can re-surface a surviving
    /// entry as Final on a delta gap without re-deriving the
    /// HIDDEN-vs-HASH-EXEMPT distinction per cell.
    ///
    /// ### Known limitation: the prune is ENTITY-granular, visibility is
    /// REALIZATION-granular
    ///
    /// A `ValueCellId` is `(entity, member)` — it carries no realization index —
    /// so the prune can only join on the entity half of the incoming
    /// `Entity#realization[N]` keys. Two consequences, both deliberate and both
    /// pinned by tests in `commands_tests.rs`:
    ///
    /// * **Partial hide is not pruned.** An entity with several realizations
    ///   (e.g. the `SelectiveMultiBody` fixture's `#realization[0]` and
    ///   `#realization[1]`) stays in the visible set while ANY of them is
    ///   visible, so ALL of its cached cells survive a hide of just one. This is
    ///   an over-retention: arch §8 is discharged only at ENTITY granularity, not
    ///   per realization. Whole-entity hides — the case the mass-prop cells
    ///   actually care about, since a `: Rigid` body's `mass` / `centroid` /
    ///   `moment_of_inertia` / `moi_principal` are entity-level cells — ARE
    ///   pruned exactly.
    /// * **Contained sub-parts are pruned every sync.** A cell whose entity never
    ///   appears as a realization key on its own is dropped on every
    ///   `sync_demand`, re-opening the delta gap for that cell. The shape this
    ///   actually hits is NOT some exotic assembly-level aggregate — it is an
    ///   ordinary contained body. `MeshSurface.entity_path` is the composed
    ///   CONTAINMENT path for descendants (`Asm.part#realization[0]`, reify-eval
    ///   `geometry_ops.rs`; see the field doc at reify-eval `lib.rs`), while
    ///   `ValueData.entity_path` is `cell.id.entity`, the TEMPLATE name
    ///   (`RigidPart`) — value cells are template-level. So for
    ///   `structure Asm { sub part : RigidPart }` the two sides do not join, and
    ///   the sub-part's mass-prop cells are pruned on every sync.
    ///
    ///   MEASURED, and the reason this is documented rather than repaired here:
    ///   under that same composed key the demand cone resolves to NOTHING — the
    ///   first selective rebuild dispatches no realization and `state.meshes` comes
    ///   back EMPTY, where a flat fixture emits every visible body's mesh. The
    ///   sub-part is not rendered at all, so pruning its cached cells is the
    ///   CORRECT outcome: retaining them would paint a `determined` / `final` mass
    ///   for a body the pass never demanded, which is precisely the arch §8
    ///   violation this prune discharges. Repairing the entity join alone, without
    ///   the upstream key resolution, would make this worse rather than better.
    ///   Both halves are pinned by
    ///   `contained_rigid_sub_part_is_not_served_as_final_under_the_composed_key`
    ///   and its positive twin (commands_tests.rs), which show the same source
    ///   retaining correctly once the demand key resolves. Closing the upstream key
    ///   gap is filed under ticket `tkt_0RSRP0RVHF2SMG12S7QB1F9VHT` — a ticket
    ///   rather than a `#NNNN` cite because the curator assigns the task id
    ///   asynchronously and a cite must resolve to a live task to be valid.
    ///
    ///   Under-retention degrades to the pre-#5338 behaviour (the cell reads
    ///   `Undef`), never to a stale value served as Final, so it fails safe.
    ///
    /// Closing either would need an explicit `ValueCellId` → realization
    /// association rather than a string join, which is a reify-eval-side change
    /// out of this task's scope.
    pub fn sync_demand(&mut self, visible_realizations: &[String]) {
        // Parse ONCE, with `parse_realization_key` as the single definition of a
        // valid key: a key that is malformed for the demand roots must also be
        // malformed for the prune, or the prune would keep cache entries alive for
        // an entity that is then NOT demanded — servable as Final, i.e. the exact
        // failure the prune exists to prevent.
        let visible: Vec<RealizationNodeId> = visible_realizations
            .iter()
            .filter_map(|key| match parse_realization_key(key) {
                Some(rid) => Some(rid),
                None => {
                    warn!(
                        realization_key = %key,
                        "sync_demand: skipping unparseable realization key"
                    );
                    None
                }
            })
            .collect();

        // Prune BEFORE the engine borrow: retain only cells whose entity still has
        // a visible realization. For a ROOT template `ValueCellId`'s entity half is
        // the same string `parse_realization_key` extracts from
        // `Entity#realization[N]` (e.g. `RigidMassSmoke`), so the two sides join
        // directly. For a CONTAINED descendant they do not — the key carries the
        // composed containment path (`Asm.part`) and the cell the template name
        // (`RigidPart`) — and the entry is pruned; see the known-limitation bullet
        // above for why that is the correct outcome there rather than a bug to fix
        // in this line.
        let visible_entities: HashSet<&str> =
            visible.iter().map(|rid| rid.entity.as_str()).collect();
        self.geometry_derived_cache
            .retain(|id, _| visible_entities.contains(id.entity.as_str()));

        let roots: Vec<NodeId> = visible.into_iter().map(NodeId::Realization).collect();
        self.core.engine_mut().set_demand_selective(roots);
    }

    /// Load a .ri file from disk.
    ///
    /// Unlike `load_from_source`, this method wires multi-file import resolution:
    /// it builds a `ModuleResolver` rooted at the file's parent directory and
    /// compiles each `import` declaration via `ModuleDag` before composing the
    /// entry's prelude.  See `compile_entry_with_imports` for the full flow and
    /// for the rationale on why it's GUI-side rather than a direct call into
    /// `reify_compiler::module_dag::compile_project_with_entry_source`.
    pub fn load_file(&mut self, path: &Path) -> Result<GuiState, String> {
        let source = std::fs::read_to_string(path)
            .map_err(|e| format!("Error reading {}: {}", path.display(), e))?;

        let module_name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unnamed");

        let (compiled, parsed) =
            compile_entry_with_imports(path, &source, module_name).map_err(|(msg, diags)| {
                self.record_compile_failure(diags, &source, module_name);
                msg
            })?;
        // check_with_solve_slot fires the SolveCancellationSink lifecycle (task γ/4086).
        let check_result = self.check_with_solve_slot(&compiled);
        // Atomically commit all five core fields in a single call.
        // `path.to_path_buf()` is evaluated as a call argument — before the callee body
        // runs — so a panic in `to_path_buf()` lands in the pre-commit window: none of
        // the five fields are written.  Atomic-commit invariant: see engine.rs:30-44.
        self.commit_state(parsed, compiled, check_result, module_name, &source, FilePathUpdate::Set(path.to_path_buf()));
        // Emit AFTER all state is committed — cross-cutting ordering invariant.
        self.post_engine_call_telemetry();
        self.build_gui_state()
    }

    /// Update source code and re-evaluate from scratch.
    ///
    /// Source changes can alter topology, so we create a fresh parse/compile/eval cycle.
    /// The existing engine state (snapshot, caches) is reused where possible via check().
    ///
    /// On any error (parse, compile, or a panic in check()), the session state is left
    /// completely unchanged — source_map, module_name, compiled, and last_check all
    /// retain their previous values. All mutations are deferred until after check() returns.
    ///
    /// When `self.file_path` is set (i.e. after a prior `load_file`), this method
    /// routes through `compile_entry_with_imports` to preserve the multi-file import
    /// graph resolved at `load_file` time — dirty-buffer edits no longer silently
    /// drop imports.  See task 3318 (item 3).  Both `module_name` and the
    /// project-root anchor are derived from `self.file_path`; the caller's `path`
    /// argument is used only for the single-file fallback (when `self.file_path` is
    /// `None`).  See task 3370.
    ///
    /// When `self.file_path` is `None` (i.e. `load_from_source`-only sessions with
    /// no project-root anchor), the original single-file `parse_with_stdlib +
    /// compile_with_stdlib` path is preserved unchanged.
    pub fn update_source(&mut self, path: &str, content: &str) -> Result<GuiState, String> {
        // When self.file_path is set (i.e. after a prior load_file), derive module_name
        // from it — NOT from the caller's `path` arg.  This keeps module_name in lockstep
        // with the entry-module key established at load_file time, regardless of what
        // path string the caller serialises.  See task 3370 (esc-3318-14, suggestion #1).
        // Owned String releases the self.file_path borrow before the closures below.
        let module_name_owned = self
            .core
            .file_path()
            .unwrap_or_else(|| Path::new(path))
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unnamed")
            .to_owned();
        let module_name = module_name_owned.as_str();

        let (parsed, compiled) = if let Some(entry_path) = self.core.file_path().map(|p| p.to_path_buf()) {
            // Multi-file flow — same as load_file. Preserves the import graph
            // resolved at load_file time so dirty-buffer edits don't silently drop
            // imports.  Both module_name and the project-root anchor come from
            // self.file_path.  See task 3318 (item 3), task 3228, and task 3370.
            let (compiled, parsed) = compile_entry_with_imports(&entry_path, content, module_name)
                .map_err(|(msg, diags)| {
                    self.record_compile_failure(diags, content, module_name);
                    msg
                })?;
            (parsed, compiled)
        } else {
            // Single-file flow — no prior load_file means no project_root anchor;
            // delegate to compile_single_file_with_stdlib (shared with load_from_source).
            compile_single_file_with_stdlib(content, module_name).map_err(|(msg, diags)| {
                self.record_compile_failure(diags, content, module_name);
                msg
            })?
        };

        // Parse+compile succeeded — run check() before mutating any state, so
        // that a panic in check() leaves the session completely unchanged.
        // check_with_solve_slot fires the SolveCancellationSink lifecycle (task γ/4086).
        let check_result = self.check_with_solve_slot(&compiled);

        // Atomically commit all state after check() succeeds.
        // Preserve file_path: update_source does not change which file is loaded;
        // Preserve keeps the file_path set by the prior load_file call.
        self.commit_state(parsed, compiled, check_result, module_name, content, FilePathUpdate::Preserve);

        // Emit AFTER all state is committed — cross-cutting ordering invariant.
        self.post_engine_call_telemetry();

        self.build_gui_state()
    }

    /// Route failure diagnostics into `compile_failure` based on whether a prior successful
    /// compile exists at the time of failure.
    ///
    /// - `compiled is None` → `CompileFailureKind::ColdStart`; `build_gui_state`'s
    ///   early-return branch surfaces these diagnostics.
    /// - `compiled is Some` → `CompileFailureKind::LiveEdit`; `build_gui_state`'s
    ///   append branch surfaces these alongside prior-good-state warnings.
    ///
    /// `Option<CompileFailure>` makes the at-most-one-non-empty invariant a type-level
    /// guarantee — no `debug_assert!` guards are needed.
    ///
    /// `source` is the full entry-file text that was compiled (the same buffer used
    /// to compute `diags` line/col positions).  `module_name` is the bare module name
    /// (without `.ri`); `module_key(module_name)` is stored as `file_key` so
    /// `build_gui_state`'s LiveEdit branch can locate the right `source_map` entry.
    fn record_compile_failure(
        &mut self,
        diags: Vec<DiagnosticInfo>,
        source: &str,
        module_name: &str,
    ) {
        let kind = if self.core.compiled().is_none() {
            CompileFailureKind::ColdStart
        } else {
            CompileFailureKind::LiveEdit
        };
        self.compile_failure = Some(CompileFailure {
            diags,
            kind,
            source: source.to_owned(),
            file_key: module_key(module_name),
        });
    }

    /// Record a hot-reload failure message as the authoritative staleness signal.
    ///
    /// Called from `commands::update_source_impl` AFTER `with_engine_lock` has
    /// caught and converted any `check()` panic to `Err` — so this call is always
    /// panic-safe.  Covers both the compile-error path and the check-panic path
    /// uniformly: any `Err` return from `update_source` triggers this recording.
    ///
    /// Cleared in `commit_state` so a subsequent successful reeval auto-resets
    /// the staleness flag.
    pub fn record_reload_error(&mut self, message: String) {
        self.last_reload_error = Some(message);
    }

    /// Return `true` when a hot-reload failure has been recorded and not yet
    /// cleared by a successful `commit_state` cycle.
    pub fn is_stale(&self) -> bool {
        self.last_reload_error.is_some()
    }

    /// Return the most recently recorded hot-reload error message, or `None`
    /// when the session is not stale.
    pub fn reload_error(&self) -> Option<&str> {
        self.last_reload_error.as_deref()
    }

    /// Atomically commit all session state after a successful parse+compile+check cycle.
    ///
    /// This wrapper first delegates the five-field core commit to
    /// [`CoreState::commit_state`] (see that method's doc for the canonical-field
    /// contract: `source_map`, `module_name`, `compiled`, `last_check`, and optionally
    /// `file_path`), then updates the five cache/failure-tracking fields owned by
    /// `EngineSession`:
    ///
    /// - **Derived caches**: `def_preview_cache`, `parsed_cache`, `line_offsets_cache`, `consumed_idents_cache`
    /// - **Failure-diagnostic state**: `compile_failure`
    ///
    /// ## `file_path` parameter
    ///
    /// Pass `FilePathUpdate::Set(path.to_path_buf())` from `load_file` to commit
    /// `file_path` together with the other four fields in a single call.  Pass
    /// `FilePathUpdate::Preserve` from `load_from_source` and `update_source` to
    /// preserve the existing `file_path`.  See [`FilePathUpdate`] for the full contract.
    ///
    /// Callers **must** only invoke this after both compilation and `check()` have
    /// succeeded — invoking it on a partially-valid state would violate the invariant.
    ///
    /// The field assignment was previously duplicated in `load_from_source`
    /// and `update_source`; centralising it here prevents the two sites from
    /// drifting apart.
    fn commit_state(
        &mut self,
        parsed: reify_ast::ParsedModule,
        compiled: CompiledModule,
        check_result: CheckResult,
        module_name: &str,
        source: &str,
        file_path: FilePathUpdate,
    ) {
        // Commit the five canonical core fields atomically via CoreState::commit_state.
        // A panic between the core commit and the cache updates below leaves core fields
        // consistent (at new values) while caches may be stale — that is tolerated per
        // engine_lock.rs:30-34 ("other fields are caches that tolerate partial state").
        self.core.commit_state(compiled, check_result, module_name, source, file_path);
        // Invalidate def preview cache — new module may have different content hashes.
        self.def_preview_cache.clear();
        // Cache the parse result so get_containing_definition can avoid re-parsing
        // on every cursor/hover call.  Unconditionally overwrites any prior value
        // (never appends) — this is an invalidation, not an accumulation.
        self.parsed_cache = Some(parsed);
        // Cache the line-offset table so get_containing_definition can skip the O(M)
        // newline scan on each call.  Unconditionally overwrites any prior value.
        self.line_offsets_cache = Some(build_line_offsets(source));
        // Invalidate the consumed-idents cache so get_mechanism_descriptors rebuilds
        // it on the next call for the new module.  Same lifecycle as parsed_cache.
        self.consumed_idents_cache = None;
        // Clear the reserved-param-name warn-dedup set so a new module load
        // starts fresh — same lifecycle as consumed_idents_cache.
        self.reserved_param_warned.clear();
        // Clear stored compile failure — the compile succeeded, so any stale failure
        // diagnostics from a prior failed load must not appear in subsequent
        // build_gui_state calls.  `Option<CompileFailure>` means one field covers
        // both the cold-start and live-edit cases; setting it to `None` atomically
        // satisfies the invariant that all fields listed in the doc comment move together.
        self.compile_failure = None;
        // Clear hot-reload staleness signal — a successful commit means the user's
        // source was fully evaluated, so any prior reload-error banner is now stale.
        // Mirrors the compile_failure clear immediately above.
        self.last_reload_error = None;
        // Task #5338: drop the geometry-derived value retention on every recompile.
        //
        // Deliberately UNCONDITIONAL, i.e. not narrowed to `FilePathUpdate::Set`.
        // Clearing here cannot re-open the delta gap, because a recompile also
        // resets the gate that opens it: `input_cone_hash` is a field on the
        // realization node inside `eval_state.snapshot.graph` (reify-eval
        // graph.rs), and `check()` replaces `eval_state` wholesale, so every
        // recompile resets all hashes to `None`. The first selective tessellate
        // after ANY recompile therefore always dispatches and repopulates this
        // cache from a complete delta; only the SECOND and later ones can be
        // hash-exempt, and no `commit_state` runs between them.
        //
        // The reason it stays unconditional rather than narrowing to `Set`:
        // `load_from_source` also commits with `Preserve` (it has no file on
        // disk) and can carry an entirely DIFFERENT module, whose entities may
        // collide with the previous module's on `ValueCellId` (`entity+member`) —
        // two sources each declaring a `Body : Rigid` both key `Body.mass`.
        //
        // MEASURED, so the next reader does not over-trust this line: removing it
        // does NOT by itself make that collision observable through the session
        // API. `a_colliding_second_module_does_not_replay_the_first_modules_mass_props`
        // (commands_tests.rs) drives exactly the two-module sequence, and with this
        // clear deleted it — and all six `rigid_mass_props*` tests — stay GREEN.
        // The reason is the second guard: a recompile resets every
        // `input_cone_hash`, so the first pass after a load dispatches every
        // demanded realization, and `surface_geometry_derived_cells`' dispatched-
        // entity discriminator then DROPS the colliding entry instead of replaying
        // it.
        //
        // So this clear is defence in depth, not the sole guard — and it is
        // load-bearing exactly where that discriminator's own documented limit
        // bites: a colliding realization that dispatches but emits no mesh (a
        // kernel OP failure) reads as a delta gap, and the retained entry from the
        // PREVIOUS module would be replayed as `determined` / `final`. It is also
        // the only guard here that does not depend on the mesh-side dispatch proxy
        // at all. Keeping it costs one `clear()` on a path that has just recompiled
        // a module; narrowing it buys nothing and removes the outer guard.
        self.geometry_derived_cache.clear();
    }

    /// Export geometry to a file.
    pub fn export(&mut self, format: ExportFormat, path: &Path) -> Result<(), String> {
        // split_compiled_and_engine_mut surfaces the compiled-immutable /
        // engine-mutable disjoint-field borrow through the encapsulation boundary.
        let (compiled_opt, engine) = self.core.split_compiled_and_engine_mut();
        let compiled = compiled_opt.ok_or_else(|| "No module loaded".to_string())?;

        let result = engine.build(compiled, format);

        for diag in &result.diagnostics {
            if diag.severity == Severity::Error {
                return Err(format!("Build error: {}", diag.message));
            }
        }

        match result.geometry_output {
            Some(data) => {
                std::fs::write(path, &data)
                    .map_err(|e| format!("Error writing {}: {}", path.display(), e))?;
                Ok(())
            }
            None => Err("No geometry output produced".to_string()),
        }
    }

    /// Resolve the canonical source key and text for the currently loaded module.
    ///
    /// Returns `Some((key, source_text))` where `key` is `"{module_name}.ri"` (a
    /// reference into the map's owned key) and `source_text` is the stored
    /// source for that key (a reference into the map's owned value).  Both
    /// references borrow from `self` and require no allocation on the return path.
    ///
    /// Returns `None` when the session has no loaded module (`compiled` is `None`),
    /// when `module_name` is `None`, or when the source map does not contain the
    /// derived key.  The last two cases indicate a broken invariant (e.g., from a
    /// test helper like `break_module_name_for_test`); callers handle `None`
    /// gracefully instead of panicking.
    fn resolve_source(&self) -> Option<(&str, &str)> {
        self.core.compiled()?;
        let name = self.core.module_name()?;
        let key = module_key(name);
        let (k, v) = self.core.source_map().get_key_value(&key)?;
        Some((k.as_str(), v.as_str()))
    }

    /// Look up source location for either a template name (e.g., `"Bracket"`) or a
    /// cell ID (e.g., `"Bracket.width"`).
    ///
    /// - **Template name** (no `.`) → returns the first value cell's span as a proxy.
    /// - **Cell ID** (`Entity.member`) → returns that cell's span.
    ///
    /// Returns `None` when the entity or member is not found, the compiled module is
    /// not loaded, or when the invariant is broken (e.g., via `break_source_map_for_test`).
    pub fn get_source_location(&self, entity_path: &str) -> Option<SourceLocationInfo> {
        let compiled = self.core.compiled()?;
        // Delegate source key resolution to resolve_source — returns None when
        // no module is loaded or when the invariant is broken (e.g., via
        // break_source_map_for_test), preserving the graceful-degradation contract
        // exercised by get_source_location_returns_none_when_module_name_broken.
        let (file, source) = self.resolve_source()?;
        reify_eval::resolve_entity_source_location(compiled, source, file, entity_path)
    }

    /// Return diagnostics (warnings, info) from the most recently compiled module.
    ///
    /// If no module is loaded, returns an empty vec. Because
    /// [`load_from_source`] and [`update_source`] return `Err` before storing
    /// a module that has compile errors, only warnings and info-level
    /// diagnostics survive here — compile errors are surfaced as `Err` results
    /// from those methods.
    ///
    /// Delegates source key resolution to [`resolve_source`].
    pub fn get_diagnostics(&self) -> Vec<DiagnosticInfo> {
        let compiled = match self.core.compiled() {
            Some(c) => c,
            None => return Vec::new(),
        };

        // Early-exit when there is nothing to map — avoids calling resolve_source
        // when no work is needed.
        if compiled.diagnostics.is_empty() {
            return Vec::new();
        }

        // Resolve file_path and source text via the shared helper.
        // Returns None only when the invariant is broken (module_name or
        // source_map out of sync with compiled) — e.g., via break_*_for_test.
        // In debug builds we catch this loudly so stale-state bugs surface
        // immediately during development; release builds still return an empty
        // vec for graceful degradation (debug_assert is a no-op there).
        // NOTE: Assumes all diagnostic spans refer to the single loaded source
        // file — file_path from multi-file diagnostics would need threading here.
        let (file_path, source) = match self.resolve_source() {
            Some(pair) => pair,
            None => {
                debug_assert!(
                    false,
                    "resolve_source returned None with non-empty diagnostics — invariant broken"
                );
                return Vec::new();
            }
        };

        diagnostics_to_info(&compiled.diagnostics, file_path, source)
    }

    /// Returns `true` once a complete parse+compile+check cycle has been
    /// committed to this session — i.e., both `compiled` and `last_check` are
    /// populated.  This is `false` on a freshly-constructed `EngineSession`
    /// (before the first `load_from_source` or `update_source` call) and
    /// `true` afterward.
    ///
    /// **Note:** a cycle that produced compile or check diagnostics still
    /// returns `true` — this predicate only checks that *a* cycle has
    /// completed, not that it was error-free.
    ///
    /// Used by `handle_wait_for_idle` as a fast pre-check that guards against
    /// false-positive idle responses on a fresh session (where the frontend's
    /// `evalStatus` starts as `'idle'` by default).  The full wait delegates
    /// to the frontend's `evalStatus` polling for the authoritative "idle
    /// including pending UI re-render" signal, because the Rust engine is
    /// fully synchronous — any in-progress work completes before the
    /// Tauri command returns.
    pub fn is_idle(&self) -> bool {
        self.core.compiled().is_some() && self.core.last_check().is_some()
    }

    /// Build the list of source files for `GuiState`, incorporating the live-edit
    /// splice when a `LiveEdit` failure is stored.
    ///
    /// Shared by `build_gui_state` and `set_active_fea_case` to ensure consistent
    /// `files` data across initial load and case-switch paths.
    fn build_files_with_live_edit(&self) -> Vec<FileData> {
        let mut files: Vec<FileData> = self
            .core
            .source_map()
            .iter()
            .map(|(path, content)| FileData {
                path: path.clone(),
                content: content.clone(),
            })
            .collect();

        // One-snapshot invariant: splice in any live-edit failing source so
        // `files[].content` and `compile_diagnostics` are from the same snapshot.
        if let Some(f) = &self.compile_failure
            && f.kind == CompileFailureKind::LiveEdit
        {
            if let Some(entry) = files.iter_mut().find(|fd| fd.path == f.file_key) {
                entry.content = f.source.clone();
            } else {
                files.push(FileData {
                    path: f.file_key.clone(),
                    content: f.source.clone(),
                });
            }
        }

        files
    }

    /// Build compile diagnostics for `GuiState`, appending live-edit failures,
    /// hot-reload errors, and build/realization-time geometry **errors** when
    /// present.
    ///
    /// Shared by `build_gui_state` and `set_active_fea_case` so both paths
    /// produce identical diagnostic data and cannot silently drift.
    ///
    /// # Build-time geometry errors (tasks 5197 / 5208)
    ///
    /// `get_diagnostics` returns only the *static* `compiled.diagnostics` — what
    /// the compiler knew before any geometry ran. Errors raised by the
    /// build/realization pass (`tessellate_snapshot`) land in the separate
    /// `tess_diag_cache` / `GuiState::tessellation_diagnostics` stream, which the
    /// designer-facing diagnostics panel does not read. A program that compiled
    /// cleanly but produced no geometry therefore rendered as an empty viewport
    /// with an EMPTY diagnostics list, and the designer had nothing to act on.
    ///
    /// This became a live concern with task 5208: curated 3-arg
    /// `fillet`/`chamfer` is now genuinely reachable through the production `.ri`
    /// pipeline, so its *residual* failures — a selector that picks zero edges, a
    /// radius the kernel cannot apply, a reference to an unrealized solid — are
    /// ordinary authoring mistakes that must be reported like any other.
    ///
    /// Only the **`Error`** class crosses over. Tessellation `Warning`/`Info`
    /// entries (kernel chatter such as the "no topology extraction fixture"
    /// seeder warning) stay confined to `tessellation_diagnostics`, so folding
    /// does not flood the panel on an otherwise-healthy load.
    ///
    /// Direction matters: the reverse flow stays blocked. Compile diagnostics are
    /// never copied INTO `tessellation_diagnostics` — that half of the
    /// disjointness contract is pinned by
    /// `build_gui_state_compile_diagnostics_populated_from_warning`.
    ///
    /// Ordering: build-time errors are appended LAST, after the static
    /// diagnostics and the live-edit / hot-reload synthetics, so existing
    /// positional expectations over the leading entries are unaffected.
    fn build_compile_diagnostics(&self) -> Vec<DiagnosticInfo> {
        let mut compile_diagnostics = self.get_diagnostics();
        if let Some(f) = &self.compile_failure
            && f.kind == CompileFailureKind::LiveEdit
        {
            compile_diagnostics.extend(f.diags.iter().cloned());
        }
        if self.compile_failure.is_none()
            && let Some(msg) = &self.last_reload_error
        {
            let file_path = self
                .resolve_source()
                .map(|(k, _)| k)
                .unwrap_or("<unknown>");
            compile_diagnostics.push(DiagnosticInfo {
                file_path: file_path.to_owned(),
                line: 1,
                column: 1,
                end_line: 1,
                end_column: 1,
                severity: "Error".to_owned(),
                message: msg.clone(),
                code: Some("hot-reload-error".to_owned()),
                has_location: false,
            });
        }

        // Fold in build/realization-time geometry ERRORS (see the doc comment
        // above). `tess_diag_cache` is refreshed by `build_gui_state` immediately
        // after `tessellate_snapshot` and BEFORE this helper is called, and is
        // reset to empty on the no-tessellation branch — so it always reflects
        // the current snapshot and cannot carry stale errors forward.
        //
        // The identity guard is belt-and-braces: the two sources are disjoint by
        // construction (static compile vs. build pass), so it is a no-op today.
        // It exists so that if a diagnostic ever becomes reachable from both, the
        // designer sees it once rather than twice.
        for diag in self
            .tess_diag_cache
            .iter()
            .filter(|d| d.severity == "Error")
        {
            let already_present = compile_diagnostics
                .iter()
                .any(|c| c.message == diag.message && c.line == diag.line);
            if !already_present {
                compile_diagnostics.push(diag.clone());
            }
        }

        compile_diagnostics
    }

    /// Build the full GUI state from the current engine state.
    ///
    /// # One-snapshot invariant (task 4258)
    ///
    /// `files[].content` and `compile_diagnostics` are from the **same** source
    /// snapshot, with one precision: **Error** diagnostics from the failing compile
    /// have line/col positions guaranteed to index into the overridden
    /// `files[].content`; **Warning/Info** diagnostics carried over from the
    /// last-good compile retain their last-good positions and may be off if the
    /// edit shifted lines.
    ///
    /// **`get_source_location` spans** are resolved against the last-good compiled
    /// source (`source_map`).  They must NOT be applied as indices into
    /// `files[].content` when the session is stale (failed-edit) — the two buffers
    /// differ.  Use `get_source_location` spans only when `compile_failure` is
    /// `None` (i.e. `stale == false` at the MCP layer).
    ///
    /// `meshes` and `values` intentionally remain last-good on failure so the
    /// viewport stays populated.  See `commands::engine_state_json` for the full
    /// contract as exposed by the MCP `engine_state` tool.
    pub fn build_gui_state(&mut self) -> Result<GuiState, String> {
        // When `compiled` is `None` (the session has never completed a successful
        // parse+compile+check cycle), surface the most recent failure diagnostics
        // so users see the error in the diagnostics panel rather than a silent
        // empty viewport.
        //
        // `compile_failure` is populated by `load_from_source`, `update_source`,
        // and `load_file` on the failure path and cleared to `None` by
        // `commit_state` on every successful cycle — so here it always reflects
        // exactly the most-recent failed-load error (or is `None` when no load has
        // been attempted yet).
        //
        // Only `ColdStart` failures belong on this branch: a `LiveEdit` failure
        // can only be stored when `compiled` was `Some` at failure time, which
        // means `compiled` is still `Some` now — so this branch (`compiled is None`)
        // can only carry a `ColdStart` failure or `None`.
        //
        // `last_check is None` while `compiled is Some` cannot occur with the
        // current `commit_state` atomic-commit (both fields are assigned together),
        // so this branch is reached only when `compiled` has never been set.
        if self.core.compiled().is_none() || self.core.last_check().is_none() {
            // Build compile_diagnostics for the cold-start / never-committed path.
            // Factor out the construction so we can append the last_reload_error
            // synthetic diagnostic on this branch too — matching the main-branch
            // synthesis at the bottom of this function.  Without this, a
            // cold-start session where compile() succeeded but check() panicked
            // (compile_failure is None, last_reload_error is Some) would return
            // empty compile_diagnostics from this early-return, silently dropping
            // the staleness signal from the GUI channel.
            let mut compile_diagnostics_early = match &self.compile_failure {
                Some(f) => {
                    // `compiled` is `None` on this branch, so only `ColdStart`
                    // failures are expected.  A `LiveEdit` failure here means
                    // `self.compiled` was set back to `None` without clearing
                    // `compile_failure`, which is an invariant violation.
                    debug_assert!(
                        matches!(f.kind, CompileFailureKind::ColdStart),
                        "LiveEdit failure stored while compiled is None — invariant broken; kind = {:?}",
                        f.kind
                    );
                    f.diags.clone()
                }
                None => Vec::new(),
            };
            // Mirror the main-branch reload-error synthesis: when no structured
            // compile_failure exists but last_reload_error is set (e.g. a
            // cold-start check()-panic), surface the Error diagnostic so a stale
            // cold-start session still shows the diagnostic regardless of path.
            // Gating on compile_failure.is_none() avoids double-reporting just
            // as on the main branch.
            if self.compile_failure.is_none()
                && let Some(msg) = &self.last_reload_error
            {
                let file_path = self
                    .resolve_source()
                    .map(|(k, _)| k)
                    .unwrap_or("<unknown>");
                compile_diagnostics_early.push(DiagnosticInfo {
                    file_path: file_path.to_owned(),
                    line: 1,
                    column: 1,
                    end_line: 1,
                    end_column: 1,
                    severity: "Error".to_owned(),
                    message: msg.clone(),
                    code: Some("hot-reload-error".to_owned()),
                    has_location: false,
                });
            }
            // One-snapshot invariant (task 4258): surface the failing buffer as
            // files[0] so compile_diagnostics (which carry line/col computed
            // against that buffer) can be indexed.  The check()-panic path has no
            // structured CompileFailure, so files stays empty there — the synthetic
            // `last_reload_error` diagnostic has line=1 / col=1 and needs no
            // buffer to index into.
            let files_early = match &self.compile_failure {
                Some(f) => vec![FileData {
                    path: f.file_key.clone(),
                    content: f.source.clone(),
                }],
                None => Vec::new(),
            };
            return Ok(GuiState {
                meshes: Vec::new(),
                values: Vec::new(),
                constraints: Vec::new(),
                files: files_early,
                tessellation_diagnostics: Vec::new(),
                compile_diagnostics: compile_diagnostics_early,
                tensegrity_wires: Vec::new(),
                tensegrity_surfaces: Vec::new(),
                // No edit has run on this cold-start/early-return path.
                demand_prune_measurement: None,
                display_panes: Vec::new(),
                display_appearance: Vec::new(),
                // Cold-start / no-check path: no FEA solve has run yet.
                fea_diagnostics: Vec::new(),
                fea_convergence: None,
            });
        }

        // γ (task 4739): run the demand-prune Pending producer BEFORE
        // `build_values` so the FIRST returned GuiState already reflects a
        // freshly-pruned cell as Pending (with its last-substantive value),
        // not a one-build-stale Final.
        //
        // The producer also runs at the top of `tessellate_snapshot` below (its
        // primary warm pruning surface), but that fires AFTER `build_values` has
        // already read each cell's freshness.  Without this pre-pass the flip
        // would only be observed by the NEXT `build_gui_state` — a one-cycle
        // violation of the arch §8 prune-safety invariant ("a pruned
        // realization's cached result is never served as Final").  The call is
        // idempotent and a cheap no-op under cold full_scope; on the
        // warm/selective path the later `tessellate_snapshot` pass re-checks the
        // now-Pending nodes and flips nothing further (only Final entries are
        // eligible).
        self.core.engine_mut().mark_demand_pruned_pending();

        // Build values and constraints via shared helpers (also used by
        // build_preview_gui_state) so both paths stay in sync.  Scoped block so
        // the immutable borrows on `compiled` and `check` are released before the
        // mutable engine borrow in the tessellation step below.
        let (mut values, mut constraints) = {
            let compiled = self.core.compiled().unwrap();
            let check = self.core.last_check().unwrap();
            (
                build_values(compiled, check, Some(self.core.engine())),
                build_constraints(compiled, check),
            )
        };

        // Build meshes (from tessellation of realizations) and capture any
        // tessellation diagnostics (e.g. OCCT kernel errors).
        // split_compiled_and_engine_mut surfaces the compiled-immutable /
        // engine-mutable disjoint-field borrow through the encapsulation boundary.
        // Scoped so the mutable engine borrow is released before resolve_source()
        // is called inside the diagnostics-mapping branch below.
        let tess_result = {
            let (compiled, engine) = self.core.split_compiled_and_engine_mut();
            compiled.and_then(|c| engine.tessellate_snapshot(c))
        };

        // ── Task 5194: surface kernel-derived geometry/mass-property cells ──────
        //
        // `build_values` / `build_constraints` above read the kernel-LESS
        // `check.values` (the `eval` / warm `edit_param` result), so a `: Rigid`
        // body's auto-derived mass-property cells (mass / centroid /
        // moment_of_inertia / moi_principal) read `Undef` and the
        // `moi_principal[0] > 0` PD constraint is Indeterminate there. Surface the
        // kernel-derived cells / re-checked constraints from the kernel-bearing
        // `tessellate_snapshot` result via a shared helper, so every entry point
        // that rebuilds GuiState (load_file, update_source, set_parameter)
        // surfaces them identically and the load / warm-edit paths cannot diverge
        // (the helper keys on `ValueCellId`, not the reallocated kernel handle).
        // `set_active_fea_case` is the one rebuild that does NOT pass through here
        // — it re-tessellates nothing — and calls the same helper with an empty
        // delta; it is the second and only other call site.
        //
        // ── DURABILITY INVARIANT (task #5338) ────────────────────────────────
        //
        // `tess_result` is an INCREMENTAL DELTA, not a full snapshot. The MESH half
        // is normative upstream — the DELTA CONTRACT block on
        // `Engine::demand_scoped_unified_pass` (reify-eval engine_build.rs), which
        // is deliberately not restated here. That block is silent on `values`; the
        // VALUES half (a hash-exempt cell arrives as an explicit `Undef` ENTRY, not
        // as an absent key) is established at `surface_geometry_derived_cells`
        // below, with the measurement. Read each half at its own site.
        //
        // The consequence this function must honour: an absent (or `Undef`)
        // realization in the delta means "RETAIN the previous value", NOT "the
        // value is gone". Every geometry-derived cell must therefore survive a
        // delta gap. Concretely, all FOUR GUI entry points that LOAD OR REBUILD a
        // module reach this rebuild (`set_active_fea_case` produces a GuiState
        // without reaching it, and surfaces the cells itself) — argv launch
        // (`commands::load_initial_file_impl`), File-Open
        // (`commands::open_file_engine_impl`), watcher reload
        // (`commands::reload_for_watch_impl`) and warm edit
        // (`commands::set_parameter_impl`) — are each followed by the frontend's
        // `sync_demand` + repeated re-renders, and from the SECOND such re-render
        // onward the body is hash-exempt and its mass-prop cells arrive `Undef`.
        // Reading the delta as a snapshot there is what made those cells revert to
        // `Undef` in the shipped GUI. The matrix test
        // `rigid_mass_props_determined_across_all_gui_load_paths`
        // (tests/commands_tests.rs) locks all four entry points against it.
        //
        // Task #5338: the overlay is DELTA-AWARE. `tess_result` is an incremental
        // delta, so a hash-exempt realization's cells arrive `Undef` even though
        // their values are unchanged and still correct; `geometry_derived_cache`
        // retains the last delta-resolved value per cell and re-surfaces it on such
        // a gap. Disjoint-field borrow: the cache and the engine are distinct
        // `EngineSession` fields, so split the borrow rather than cloning.
        if let Some(result) = &tess_result {
            let cache = &mut self.geometry_derived_cache;
            surface_geometry_derived_cells(
                self.core.engine(),
                &mut values,
                &mut constraints,
                &result.values,
                &result.meshes,
                cache,
            );
        }

        let (meshes, tessellation_diagnostics, display_panes, display_appearance) = match tess_result {
            Some(result) => {
                // Map tessellation diagnostics → DiagnosticInfo and emit backend
                // log entries so headless/CI runs still surface these via tracing.
                let tess_diags = if result.diagnostics.is_empty() {
                    Vec::new()
                } else {
                    // Log each diagnostic before mapping so stderr/tracing output
                    // is available even when the GUI channel is not subscribed.
                    for diag in &result.diagnostics {
                        warn!(severity = diag.severity.as_wire_str(), message = %diag.message, "tessellation diagnostic");
                    }
                    // Resolve source for span lookup. When source is unavailable (e.g.
                    // break_*_for_test helpers), we still produce DiagnosticInfo but tag
                    // code = "unresolved-source" so frontends can distinguish reliable from
                    // unreliable positions. Borrows from `self` — no allocation on the
                    // happy path; the "<unknown>"/"" fallback is zero-length static strs.
                    let resolved = self.resolve_source();
                    let unresolved = resolved.is_none();
                    let (file_path, source): (&str, &str) = resolved.unwrap_or(("<unknown>", ""));
                    let mut diags = diagnostics_to_info(&result.diagnostics, file_path, source);
                    if unresolved {
                        for d in &mut diags {
                            if d.code.is_none() {
                                d.code = Some("unresolved-source".to_owned());
                            }
                        }
                    }
                    diags
                };
                // T6 (task 3904) complete: `default_visible` is surfaced to the
                // GUI via the entity-tree realization nodes — NOT through MeshData.
                // `get_entity_tree` → `build_template_node` computes
                // `default_visible = !(aux_ancestor || real.is_aux)` per the
                // shared contract anchor `geometry_ops::surface_subtree`. The frontend
                // `defaultVisibilityFor` reads the realization node's flag and
                // returns 'hidden' for aux bodies, driving `meshManager.setVisibility`
                // and thus `getSceneMeshes()` / `viewport_state.meshCount`.
                // `MeshData` intentionally stays visibility-free: the frontend
                // never consults mesh visibility directly.

                // ── #4898: build material+coating+finish_process→appearance lookup ──
                // (extends β/4771 §7.1 to cover the full §7.3 functional precedence)
                //
                // Pass 1: gather per-member `material`, `coating`, `finish_process`
                // cells from `result.values` by entity string.  We accumulate ANY
                // producer member (do NOT gate on a `material` cell being present)
                // so that coating-only bodies resolve via Layer 1 even without a
                // material cell (precedence: coating > finish_process > material;
                // see resolve_appearance_opt in appearance.rs).
                //
                // Pass 2: for each entity build a synthetic `Body` StructureInstance
                // carrying all gathered producer fields, then call
                // `resolve_appearance_opt`.  Mirrors the 3MF `__self` pattern
                // (resolve_export_body_color in engine_build.rs): one body with all
                // producer fields → resolve_appearance.  `resolve_appearance_opt`
                // ignores non-producer
                // fields, so the synthetic body yields identical results to a full
                // StructureInstance.
                //
                // `result.values` is borrowed here (immutable); `result.meshes` is
                // consumed in the `.into_iter()` below (Rust partial-move is OK).
                let by_entity: HashMap<String, crate::types::MeshAppearance> = {
                    use reify_eval::appearance::resolve_appearance_opt;
                    use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId};
                    let mut app_diags: Vec<Diagnostic> = Vec::new();

                    // Pass 1: gather producer fields by entity.
                    let mut gathered: HashMap<String, Vec<(String, Value)>> =
                        HashMap::new();
                    for (id, cell_val) in result.values.iter() {
                        if matches!(
                            id.member.as_str(),
                            "material" | "coating" | "finish_process"
                        ) {
                            gathered
                                .entry(id.entity.clone())
                                .or_default()
                                .push((id.member.clone(), cell_val.clone()));
                        }
                    }

                    // Pass 2: build synthetic body per entity and resolve appearance.
                    let mut map: HashMap<String, crate::types::MeshAppearance> =
                        HashMap::new();
                    for (entity, fields) in gathered {
                        let body_fields: PersistentMap<String, Value> =
                            fields.into_iter().collect();
                        let body =
                            Value::StructureInstance(Box::new(StructureInstanceData {
                                type_id: StructureTypeId(u32::MAX),
                                type_name: "Body".to_string(),
                                version: 1,
                                fields: body_fields,
                            }));
                        if let Some(app) = resolve_appearance_opt(&body) {
                            map.insert(
                                entity,
                                project_appearance(&app, &mut app_diags),
                            );
                        }
                    }

                    for diag in &app_diags {
                        warn!(
                            severity = diag.severity.as_wire_str(),
                            message = %diag.message,
                            "material appearance diagnostic"
                        );
                    }
                    map
                };

                let mut meshes: Vec<MeshData> = result
                    .meshes
                    .into_iter()
                    .map(|surface| {
                        // Extract entity prefix (everything before "#realization[") to
                        // look up the pre-built material appearance. Mirrors entity_path
                        // join convention from collect_display_routing (:3515-3529).
                        let entity_prefix = match surface.entity_path.find("#realization[") {
                            Some(pos) => surface.entity_path[..pos].to_owned(),
                            None => surface.entity_path.clone(),
                        };
                        let appearance = by_entity.get(&entity_prefix).cloned();
                        MeshData {
                            entity_path: surface.entity_path,
                            vertices: surface.mesh.vertices,
                            indices: surface.mesh.indices,
                            normals: surface.mesh.normals,
                            scalar_channels: std::collections::HashMap::new(),
                            scalar_channel_tags: Default::default(),
                            displaced_positions: None,
                            element_kind: None,
                            region_tags: None,
                            element_index: None,
                            vector_channels: std::collections::HashMap::new(),
                            appearance,
                        }
                    })
                    .collect();
                // Development-time drift guard: warn when a material-bearing entity
                // had no mesh whose entity_path prefix matches its key.  A silent
                // miss causes that entity's material to produce appearance: None
                // instead of Some(…), which is the inverse of the §7.1 intent and
                // invisible without this signal.  Gated to debug builds because the
                // O(|by_entity| × |meshes|) scan is non-trivial on large scenes.
                #[cfg(debug_assertions)]
                for entity_key in by_entity.keys() {
                    let consumed = meshes.iter().any(|m| {
                        m.entity_path
                            .split("#realization[")
                            .next()
                            .is_some_and(|p| p == entity_key)
                    });
                    if !consumed {
                        warn!(
                            entity = %entity_key,
                            "material appearance: entity key matched no mesh prefix — \
                             possible entity_path/material-cell join drift; \
                             appearance will be None for this entity"
                        );
                    }
                }
                // Cache bare tessellation geometry ONLY for FEA scenes: when a
                // MultiCaseResult or single-case ElasticResult is present in the
                // evaluated values, `set_active_fea_case` needs the cached bare-mesh
                // buffers (vertices/indices/normals) to re-source channels without
                // re-tessellating.  Non-FEA scenes skip the O(mesh bytes) clone so
                // they don't pay the cost of a feature they never use.
                // A non-FEA scene that later acquires FEA values must go through
                // `build_gui_state` again (as always happens in production via the
                // normal commit_state → build_gui_state path) before case-switching.
                let has_fea = self.core.last_check()
                    .map(|check| values_have_fea_data(&check.values))
                    .unwrap_or(false);
                if has_fea {
                    self.tess_mesh_cache = Some(meshes.iter().map(|m| MeshData {
                        entity_path: m.entity_path.clone(),
                        vertices: m.vertices.clone(),
                        indices: m.indices.clone(),
                        normals: m.normals.clone(),
                        scalar_channels: std::collections::HashMap::new(),
                        scalar_channel_tags: Default::default(),
                        displaced_positions: None,
                        element_kind: None,
                        region_tags: None,
                        element_index: None,
                        vector_channels: std::collections::HashMap::new(),
                        appearance: None,
                    }).collect());
                } else {
                    // Invalidate any stale cache from a prior FEA scene so a
                    // subsequent set_active_fea_case on a non-FEA scene does not
                    // serve geometry from the wrong model.
                    self.tess_mesh_cache = None;
                }
                self.tess_diag_cache = tess_diags.clone();
                // Populate per-vertex FEA scalar/displacement channels when an
                // ElasticResult is present in the evaluated values.  The helper
                // returns early when no ElasticResult is found (negligible
                // overhead: one ValueMap scan), so non-FEA scenes pay no
                // tessellation-path cost.  Pass active_fea_case so multi-case
                // results sample the correct case; None falls back to lex-first.
                if let Some(check) = self.core.last_check() {
                    apply_fea_channels(&mut meshes, &check.values, self.active_fea_case.as_deref());
                }
                // Populate shell-extract channels (element_kind, region_tags,
                // vonMises_top/mid/bottom, per-face normals) for shell-classified
                // bodies, replacing their displayed geometry with the extraction
                // mid-surface. `shell_gui_mesh_data` returns owned data (the
                // &Engine borrow ends at the call), so it does not conflict with
                // the mutable `meshes` borrow; it scans the engine graph + cache
                // and returns an empty Vec for non-shell scenes (one graph scan),
                // so non-shell scenes are unaffected.
                let shell_views = self.core.engine().shell_gui_mesh_data();
                apply_shell_channels(&mut meshes, &shell_views);
                // Walk Output occurrence subs to build display_panes routing.
                // Uses &result.values (disjoint from the moved result.meshes field).
                // Immutable self.core borrows are safe here: the mutable engine
                // borrow from tessellate_snapshot was released above.
                let (display_panes, display_appearance) = {
                    let compiled = self.core.compiled().unwrap();
                    let prelude = self.core.engine().prelude();
                    collect_display_routing(compiled, prelude, &result.values)
                };
                (meshes, tess_diags, display_panes, display_appearance)
            }
            None => {
                // No tessellation result (no compiled module or no realizations).
                // Populate caches with empty data so set_active_fea_case can
                // safely clone them without checking for None.
                self.tess_mesh_cache = Some(Vec::new());
                self.tess_diag_cache = Vec::new();
                (Vec::new(), Vec::new(), Vec::new(), Vec::new())
            },
        };

        // Build files and compile diagnostics via shared helpers.
        // See `build_files_with_live_edit` and `build_compile_diagnostics` for
        // the full one-snapshot invariant, live-edit splice, and hot-reload-error
        // synthesis logic. Both helpers are also called from `set_active_fea_case`
        // to keep the two paths consistent.
        let files = self.build_files_with_live_edit();
        let compile_diagnostics = self.build_compile_diagnostics();

        // Extract tensegrity wire and surface descriptors from value cells.
        // Single scoped borrow covers both — shared precondition made explicit.
        // Borrow released before GuiState construction.
        let (tensegrity_wires, tensegrity_surfaces) = {
            let compiled = self.core.compiled().unwrap();
            let check = self.core.last_check().unwrap();
            (
                build_tensegrity_wires(compiled, check),
                build_tensegrity_surfaces(compiled, check),
            )
        };

        // Passive selective-demand measurement (task 4532): surface the
        // would-prune record produced by the most recent edit, if any.
        // OBSERVATIONAL ONLY — reading `last_demand_prune_measurement` cannot
        // affect evaluation, and it is `None` until the first edit populates it.
        // The immutable engine borrow is released by `.map(..)` (owned result)
        // before the `GuiState` literal moves the local fields.
        let demand_prune_measurement = self
            .core
            .engine()
            .last_demand_prune_measurement()
            .map(DemandPruneMeasurementDto::from);

        // FEA structured-diagnostic overlay (R3b-2, #4818): delegates to the
        // shared helper so both GuiState-producing paths cannot diverge.
        // Covers BOTH the success-with-warning path AND the failed-solve path
        // (§6.8): on a failed solve apply_fea_channels is a no-op so
        // scalar_channels stay empty, but structured_detail carries the
        // diagnostic → fea_diagnostics is non-empty → overlay can render.
        let fea_diagnostics = self.build_fea_diagnostics();

        // A-posteriori convergence status of the active case (task 3001):
        // delegates to the shared helper so both GuiState-producing paths
        // cannot diverge.
        let fea_convergence = self.build_fea_convergence();

        Ok(GuiState {
            meshes,
            values,
            constraints,
            files,
            tessellation_diagnostics,
            compile_diagnostics,
            tensegrity_wires,
            tensegrity_surfaces,
            demand_prune_measurement,
            display_panes,
            display_appearance,
            fea_diagnostics,
            fea_convergence,
        })
    }

    /// READ-ONLY full-scene snapshot for the debug-MCP `mesh_stats` / `engine_state`
    /// tools (task 5348).
    ///
    /// [`Self::build_gui_state`] returns whatever
    /// [`reify_eval::Engine::tessellate_snapshot`] produces under the CURRENT
    /// production demand. Once the frontend has flipped production demand to
    /// SELECTIVE (via [`Self::sync_demand`]), that result is an incremental DELTA
    /// (the DELTA CONTRACT, engine_build.rs:5440-5465): HIDDEN or HASH-EXEMPT
    /// realizations are ABSENT, so a debug read that treats the delta as a full
    /// snapshot under-reports the realized scene.
    ///
    /// This method forces the cold-path full-scope override
    /// ([`reify_eval::Engine::set_demand_full_scope`], engine_demand.rs:90) for the
    /// duration of ONE `build_gui_state`, so `tessellate_snapshot` takes the
    /// full-schedule branch (engine_build.rs:5415-5424) and returns EVERY
    /// realization's mesh — the complete realized scene, the same set the
    /// frontend/scene holds — then RESTORES the prior scope so the frontend's
    /// selective demand survives the read.
    ///
    /// The override is bracketed so both it and the retention cache are restored on
    /// EVERY exit path — `Ok`, `Err`, and panic. Re-running tessellate is cheap (~0
    /// kernel dispatch: every realization is already cached from the cold build).
    ///
    /// The panic path is bracketed deliberately, not defensively: `with_engine_lock`
    /// CATCHES a panic and keeps the session alive, so an unwind out of the inner
    /// `build_gui_state` would otherwise leave this read's after-effects in place on
    /// a live session. A leaked `full_scope = true` would be perf-only (it self-heals
    /// at the next `sync_demand`, where `DemandRegistry::new` resets it), but a
    /// leaked cache entry is a CORRECTNESS leak — see below. `catch_unwind` +
    /// `resume_unwind` restores both and re-raises the original panic unchanged, so
    /// the failure still surfaces exactly where it did before.
    ///
    /// ## Task #5338: `geometry_derived_cache` and the tessellation caches are
    /// bracketed too
    ///
    /// Forcing full scope makes `tessellate_snapshot` dispatch EVERY realization,
    /// including HIDDEN ones, so the inner `build_gui_state` would write a hidden
    /// entity's freshly-resolved mass-prop cells into the retention cache. Those
    /// writes bypass the [`Self::sync_demand`] prune chokepoint entirely and would
    /// outlive this read: the very next SELECTIVE `build_gui_state` finds the
    /// hidden entity's cell `Undef` in the delta, hits the leaked entry, and paints
    /// it `determined` / `freshness = "final"` — precisely the arch §8 violation
    /// ("a pruned realization's cached result is never served as Final") the prune
    /// discharges. Snapshotting and restoring the cache around the override keeps
    /// this debug projection READ-ONLY with respect to the production posture, in
    /// exactly the way the `full_scope` flag already is. The cache holds a handful
    /// of entries (four per `: Rigid` body), and this path is REIFY_DEBUG-only.
    ///
    /// ## …and so are `tess_mesh_cache` / `tess_diag_cache`
    ///
    /// Same leak, second surface, and it is NOT hypothetical: the inner full-scope
    /// `build_gui_state` overwrites both tessellation caches with FULL-SCENE
    /// meshes and diagnostics, and [`Self::set_active_fea_case`] clones
    /// `tess_mesh_cache` verbatim to re-source per-case channels without
    /// re-tessellating. Left unbracketed, a debug-MCP `engine_state` / `mesh_stats`
    /// read followed by a case switch hands the user meshes for realizations they
    /// have HIDDEN. That predates task #5338, but #5338 is what added the
    /// "READ-ONLY with respect to the production posture" claim and the
    /// `set_active_fea_case` → `surface_geometry_derived_cells` coupling that makes
    /// the case-switch path load-bearing, so the claim is made true here rather
    /// than narrowed.
    ///
    /// Saved by MOVE, not by clone — `build_gui_state` ASSIGNS both fields
    /// unconditionally on every path (the FEA-gated `Some(..)`/`None` branch and
    /// the no-tessellation branch) and never reads them, so handing the inner call
    /// an empty pair costs nothing and the restore is free even for large OCCT
    /// meshes.
    pub fn build_gui_state_full_scene(&mut self) -> Result<GuiState, String> {
        let prev = self.core.engine().demand_is_full_scope();
        let prev_cache = self.geometry_derived_cache.clone();
        let prev_tess_meshes = self.tess_mesh_cache.take();
        let prev_tess_diags = std::mem::take(&mut self.tess_diag_cache);
        self.core.engine_mut().set_demand_full_scope(true);
        // `AssertUnwindSafe`: the only state this closure mutates across the unwind
        // boundary is restored on the very next four lines, which is exactly the
        // obligation the marker asserts.
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.build_gui_state()
        }));
        self.core.engine_mut().set_demand_full_scope(prev);
        self.geometry_derived_cache = prev_cache;
        self.tess_mesh_cache = prev_tess_meshes;
        self.tess_diag_cache = prev_tess_diags;
        match outcome {
            Ok(result) => result,
            // Re-raise the original payload: this bracket exists to restore the four
            // fields, never to swallow a panic.
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }

    /// Return one `MechanismDescriptor` per mechanism cell in the loaded module.
    ///
    /// A cell is included when its post-eval value is a `Value::Map` with
    /// `kind = "mechanism"` and **no** `error` key (errored mechanisms are
    /// filtered out — their `bodies` list may be incomplete and their joint
    /// indices are unreliable).
    ///
    /// Returns an empty vec when:
    /// - no module is loaded (`compiled` is `None`), or
    /// - the loaded module contains no valid mechanism cells.
    ///
    /// AST-based driving-param resolution (`driving_param_cell_id`) is added in
    /// step 12 of the task plan. `current_value_si` is populated in step 24.
    pub fn get_mechanism_descriptors(&mut self) -> Vec<MechanismDescriptor> {
        let (compiled, check) = match (self.core.compiled(), self.core.last_check()) {
            (Some(c), Some(k)) => (c, k),
            _ => return Vec::new(),
        };

        // Lazily populate consumed_idents_cache on first call after commit_state.
        // Only when parsed_cache is Some — if None, the per-template WARN branch
        // below handles the fallback and the cache is left None so the warning
        // fires on every call (regression signal).
        if self.consumed_idents_cache.is_none()
            && let Some(parsed) = self.parsed_cache.as_ref()
        {
            let new_cache: HashMap<String, HashSet<String>> = compiled
                .templates
                .iter()
                .map(|tmpl| {
                    (
                        tmpl.name.clone(),
                        collect_consumed_mechanism_idents(parsed, &tmpl.name),
                    )
                })
                .collect();
            self.consumed_idents_cache = Some(new_cache);
        }

        let mut descriptors = Vec::new();
        // Cache of seen_joints (joint identity sequence) per mechanism cell_id.
        // Populated alongside the descriptor list and passed to
        // resolve_driving_params_from_ast, avoiding a redundant O(B) body-walk
        // inside the AST resolver for every (bind-pair, descriptor) pair.
        let mut seen_joints_cache: HashMap<String, Vec<Value>> = HashMap::new();
        // Shared empty-set fallback for the consumed-idents lookup below.
        // Declared once before the loop so both match arms can return `&HashSet`
        // without cloning — `consumed_idents` is used only immutably (`.contains`),
        // so a reference suffices.
        let empty_consumed: HashSet<String> = HashSet::new();

        // This loop emits one descriptor per **terminal** mechanism cell.
        // A mechanism cell is considered intermediate (and dropped) when its
        // member name appears as the first argument (mech_in) of a `body()` call
        // within the same structure — i.e. it is consumed to build a larger
        // mechanism.  Only `body()` consumption is filtered; `snapshot()`
        // consumption is intentionally excluded (snapshot is a viewer, not a
        // builder, and the snapshotted mechanism is the user-facing logical entity).
        //
        // See design decision: "Terminal-mechanism filter narrows the suggestion
        // text to body() consumption only."
        //
        // When `parsed_cache` is `None` (test-injection without a full parse/compile
        // cycle), the consumed-idents set is empty and every mechanism cell passes —
        // preserving the pre-filter behaviour for legacy test helpers.  A WARN event
        // is emitted *once per call* in this case so a future regression that
        // accidentally drops `parsed_cache` (e.g. a load path that forgets to
        // populate it alongside `compiled`) is surfaced immediately rather than
        // silently re-emitting intermediate mechanism cells to the UI.
        //
        // Note: the WARN fires on the broken-invariant state (compiled Some, both
        // caches None) unconditionally — even for a zero-template compiled module —
        // because the guard precedes the per-template loop.  This is intentional:
        // the signal indicates a broken load path, independent of template count.
        //
        // Errored mechanisms (closed-chain etc.) are suppressed via the `error` key
        // check below.

        // Defensive: after the lazy-populate block above, `consumed_idents_cache.is_none()`
        // already implies `parsed_cache.is_none()` (the block transitions None→Some only
        // when parsed_cache is Some).  The `&& self.parsed_cache.is_none()` clause is
        // therefore logically redundant, but it is kept as belt-and-braces: if a future
        // change to the populate block introduces a case where the cache stays None despite
        // parsed_cache being Some, omitting the clause would suppress the warning silently.
        if self.consumed_idents_cache.is_none() && self.parsed_cache.is_none() {
            tracing::warn!(
                target: "reify_gui::engine",
                "parsed_cache is None while compiled is Some; \
                 terminal-mechanism filter inactive — intermediate mechanism \
                 cells may appear in descriptors"
            );
        }

        // Mechanism-build-time reserved-name collision check (PRD §8.1,
        // W_KinematicReservedParamName).  Walk all compiled templates and emit
        // a WARN for each Param cell whose member name starts with `__joint_`
        // — the prefix reserved for synth-virtual-param names generated by the
        // η-engine literal-bind path.  One WARN per (structure, member) per load
        // (deduped via `reserved_param_warned`); warning-not-error for v0.3 per PRD §14.5.
        for template in &compiled.templates {
            for cell in &template.value_cells {
                if matches!(cell.kind, ValueCellKind::Param)
                    && cell.id.member.starts_with("__joint_")
                {
                    let key = (cell.id.entity.clone(), cell.id.member.clone());
                    if self.reserved_param_warned.insert(key) {
                        tracing::warn!(
                            target: "reify_gui::engine::reserved_param_name",
                            structure = %cell.id.entity,
                            member = %cell.id.member,
                            "user param name matches reserved __joint_* pattern; \
                             W_KinematicReservedParamName — synth-virtual-param promotion \
                             may collide on this name \
                             (PRD docs/prds/v0_3/kinematic-constraints-completion.md §8.1)"
                        );
                    }
                }
            }
        }

        for template in &compiled.templates {
            // Look up the consumed-idents set for this template from the cache,
            // falling back to the shared empty set when the cache is None or has
            // no entry for this template.  `consumed_idents` is only used for
            // `.contains()` below, so a reference to the empty set suffices.
            let consumed_idents: &HashSet<String> = self
                .consumed_idents_cache
                .as_ref()
                .and_then(|c| c.get(&template.name))
                .unwrap_or(&empty_consumed);

            for cell in &template.value_cells {
                let val = check.values.get_or_undef(&cell.id);

                // Check that the value is a mechanism Map with no error field.
                let map = match &val {
                    Value::Map(m) => m,
                    _ => continue,
                };

                let kind_val = map.get(&Value::String("kind".to_string()));
                if kind_val != Some(&Value::String("mechanism".to_string())) {
                    continue;
                }

                // Filter out errored mechanisms (closed-chain etc.).
                if map.contains_key(&Value::String("error".to_string())) {
                    continue;
                }

                // Terminal-mechanism filter: skip intermediate cells consumed as
                // mech_in by a body() call within the same structure.
                if consumed_idents.contains(&cell.id.member) {
                    continue;
                }

                // Extract joints from the bodies list (step-6).
                // Also returns the seen_joints sequence for the AST resolver cache.
                let (joints, seen_joints) = extract_joints_from_mechanism(map);
                let bodies_count = match map.get(&Value::String("bodies".to_string())) {
                    Some(Value::List(bodies)) => bodies.len(),
                    _ => 0,
                };

                let cell_id_str = cell.id.to_string();
                seen_joints_cache.insert(cell_id_str.clone(), seen_joints);

                descriptors.push(MechanismDescriptor {
                    cell_id: cell_id_str,
                    entity_path: cell.id.entity.clone(),
                    name: cell.id.member.clone(),
                    bodies_count,
                    joints,
                });
            }
        }

        // Step-12: best-effort AST traversal to resolve driving param cell ids.
        // Walks snapshot(mech, [bind(joint_ident, param_ident), …]) calls in the
        // cached parsed declarations.  Only the canonical form — both arguments to
        // bind() are bare identifiers and the value side is a Param cell — is
        // resolved; all other forms leave driving_param_cell_id = None.
        if let Some(parsed) = self.parsed_cache.as_ref() {
            resolve_driving_params_from_ast(
                &mut descriptors,
                &seen_joints_cache,
                parsed,
                check,
                compiled,
            );
        }

        descriptors
    }

    /// Return the hierarchical entity tree for the currently loaded module.
    ///
    /// Each root node corresponds to a top-level topology template.  Children
    /// are the template's value cells (params, lets, autos), sub-components,
    /// and ports, in declaration order.
    ///
    /// Returns an empty vec when no module is loaded.
    pub fn get_entity_tree(&self) -> Vec<EntityTreeNode> {
        let compiled = match self.core.compiled() {
            Some(c) => c,
            None => return Vec::new(),
        };

        // Validate template-name uniqueness once (O(N)) rather than inside every
        // build_template_node call (which would be O(N²) across the full tree build).
        // In release builds the first duplicate emits a tracing::warn! and the tree
        // is still built with first-match semantics (graceful degradation).  In debug
        // builds the debug_assert!(false, ...) panics loudly — the panic message
        // begins with "template names must be unique".
        {
            let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
            for t in &compiled.templates {
                if !seen.insert(t.name.as_str()) {
                    warn!(
                        template_name = %t.name,
                        "duplicate template name in compiled module; \
                         get_entity_tree falls back to first-match and may \
                         produce inconsistent tree"
                    );
                    debug_assert!(
                        false,
                        "template names must be unique within a compiled module: duplicate = {}",
                        t.name
                    );
                    break;
                }
            }
        }

        compiled
            .templates
            .iter()
            .map(|t| build_template_node(t, &t.name, compiled, Some(self.core.engine()), false))
            .collect()
    }

    /// Return a map from `entity_path` to `EntityIdentity` for every entity
    /// in the currently loaded module.
    ///
    /// The map contains two kinds of entries:
    ///
    /// - **Template roots** — keyed by `template.name` (e.g. `"Bracket"`).
    ///   `content_hash` = `template.content_hash.to_string()` (32-char hex).
    ///   `structural_fingerprint` = `"{entity_kind}:<root>:{sub_count}:{children_hash}"`.
    ///   `source_span` = `None` (TopologyTemplate has no span in the compiled IR).
    ///
    /// - **Value cells** — keyed by `"{template.name}.{cell.id.member}"`.
    ///   `content_hash` = hex of `ContentHash::of_str(cell_id_string)` (identity hash,
    ///   not a content hash — see `EntityIdentity.content_hash` doc for details).
    ///   `structural_fingerprint` = `"{cell_kind}:{template.name}:0:{cell_type_hash}"`.
    ///   `source_span` = `Some(SourceSpanInfo { start, end })` from `cell.span`.
    ///
    /// Returns an empty map when no module is loaded.
    pub fn get_entity_identity_map(&self) -> HashMap<String, EntityIdentity> {
        let compiled = match self.core.compiled() {
            Some(c) => c,
            None => return HashMap::new(),
        };

        let mut map = HashMap::new();

        for template in &compiled.templates {
            let entity_kind = template.entity_kind.as_label();

            // Template-level entry
            let sub_count = template.sub_components.len();
            let children_hash =
                ContentHash::combine_all(template.sub_components.iter().map(|s| s.content_hash));
            // The second field (parent) uses the '<root>' sentinel for template roots
            // (angle-bracket form is an impossible template identifier, preventing
            // collision with user-defined templates named "root").
            // Format: "{kind}:{parent}:{sub_count}:{hash}".
            let structural_fingerprint = format!(
                "{}:{}:{}:{}",
                entity_kind, "<root>", sub_count, children_hash
            );

            map.insert(
                template.name.clone(),
                EntityIdentity {
                    content_hash: template.content_hash.to_string(),
                    structural_fingerprint,
                    source_span: None,
                },
            );

            // Value-cell entries
            for cell in &template.value_cells {
                let cell_kind = cell_kind_tree_str(cell.kind);
                let cell_path = format!("{}.{}", template.name, cell.id.member);
                let cell_type_hash = ContentHash::of_str(&cell.cell_type.to_string());
                let structural_fingerprint =
                    format!("{}:{}:{}:{}", cell_kind, template.name, 0, cell_type_hash);

                map.insert(
                    cell_path,
                    EntityIdentity {
                        // Identity-hash, not content-hash: see EntityIdentity docs.
                        // Hashes the cell's id string (e.g. "Bracket.width"), not its type or value.
                        content_hash: ContentHash::of_str(&cell.id.to_string()).to_string(),
                        structural_fingerprint,
                        source_span: Some(SourceSpanInfo {
                            start: cell.span.start,
                            end: cell.span.end,
                        }),
                    },
                );
            }
        }

        map
    }

    /// Return a preview `GuiState` for a single named definition, evaluated in
    /// isolation with its default parameter values.
    ///
    /// Looks up the named template in the currently loaded `CompiledModule`,
    /// clones it into a single-template preview module (preserving shared context
    /// such as enums and functions), and evaluates it with a fresh
    /// `SimpleConstraintChecker` engine (no geometry kernel — meshes are omitted).
    ///
    /// Results are cached by `(def_name, template.content_hash)`; the cache is
    /// cleared automatically on every `load_from_source` / `update_source` call.
    ///
    /// # Errors
    /// Returns `Err` when:
    /// - No module is currently loaded.
    /// - `def_name` does not match any template in the loaded module.
    pub fn get_def_preview(&mut self, def_name: &str) -> Result<GuiState, String> {
        // Phase 1: extract content_hash from a shared borrow.  HashMap::get only
        // needs &self, so NLL allows simultaneous immutable borrows of disjoint
        // struct fields — no expensive clone is wasted on a cache hit.
        let content_hash = {
            let compiled = self
                .core
                .compiled()
                .ok_or_else(|| "No module loaded".to_string())?;
            compiled
                .templates
                .iter()
                .find(|t| t.name == def_name)
                .ok_or_else(|| format!("No definition named '{}' in loaded module", def_name))?
                .content_hash
        };

        // Phase 2: check cache before any cloning.
        let cache_key = (def_name.to_string(), content_hash);
        if let Some(cached) = self.def_preview_cache.get(&cache_key) {
            return Ok(cached.clone());
        }

        // Phase 3: cache miss — clone the module now and build the preview.
        // Clone the full module so that shared context (enums, functions, traits,
        // stdlib units, etc.) is available during evaluation, then replace the
        // templates list with only the one definition we want to preview.
        let preview_module = {
            let compiled = self
                .core
                .compiled()
                .expect("compiled was Some in Phase 1");
            let template = compiled
                .templates
                .iter()
                .find(|t| t.name == def_name)
                .expect("template was found in Phase 1");
            let mut preview = compiled.clone();
            preview.templates = vec![template.clone()];
            preview
        };

        // Phase 4: evaluate with a lightweight preview engine (SimpleConstraintChecker, no kernel).
        let mut preview_engine = Engine::new(
            Box::new(reify_constraints::SimpleConstraintChecker),
            None, // no geometry kernel — preview is values + constraints only
        );
        let check_result = preview_engine.check(&preview_module);

        // Phase 5: build GuiState from the check result.
        let gui_state = build_preview_gui_state(&preview_module, &check_result);

        // Phase 6: cache and return.
        self.def_preview_cache.insert(cache_key, gui_state.clone());
        Ok(gui_state)
    }

    /// Find the innermost structure or occurrence definition whose span contains
    /// the given 1-based `(line, col)` position.
    ///
    /// Returns `None` when:
    /// - No module is loaded.
    /// - The position falls outside every declaration's span.
    /// - `line` or `col` are zero.
    ///
    /// # Caching
    /// The parsed syntax tree and line-offset table are cached on the session
    /// (populated in `commit_state`, invalidated on every `load_from_source` or
    /// `update_source`).  The implementation is therefore O(D) where D is the
    /// number of top-level declarations — no re-parse and no O(M) source scan.
    ///
    /// # Caller note
    /// Although each call is now cheap, callers dispatching on mouse-move or
    /// cursor events should debounce (~16–50 ms) to avoid unnecessary Mutex lock
    /// traffic on the `EngineSession` in `commands.rs`.
    /// Implementing the debounce in `commands.rs::get_containing_definition_impl`
    /// is tracked as follow-up work.
    pub fn get_containing_definition(&self, line: u32, col: u32) -> Option<DefInfo> {
        // Documented contract: zero line or column is out-of-range → None.
        // Without this guard, line_col_to_byte_offset_with_offsets returns 0 for
        // zero inputs, which would incorrectly match any definition starting at byte 0.
        if line == 0 || col == 0 {
            return None;
        }
        let (_key, source) = self.resolve_source()?;

        // Both caches must be Some whenever compiled is Some (i.e., whenever
        // resolve_source() succeeds), because commit_state populates them eagerly.
        // This assert fires in debug builds if a new mutation site forgets to
        // populate the caches, surfacing stale-state bugs before they manifest as
        // silent wrong-position returns in release builds.
        debug_assert!(
            self.parsed_cache.is_some() && self.line_offsets_cache.is_some(),
            "cache invariant broken: parsed_cache and line_offsets_cache must be Some \
             whenever compiled is Some (i.e., whenever resolve_source succeeds)"
        );

        // Read the cached parse result and line-offset table.  Guard defensively
        // against None (shouldn't occur, but avoids a panic in release builds).
        let parsed = self.parsed_cache.as_ref()?;
        let line_offsets = self.line_offsets_cache.as_deref()?;

        let offset = line_col_to_byte_offset_with_offsets(source, line, col, line_offsets) as u32;

        // Delegate to the shared helper that is also used by
        // `reify_eval::resolve_entity_at_source_position`.  Using a single
        // implementation prevents the two traversals from drifting if a future
        // `Declaration` variant is added and only one call site is updated.
        reify_eval::source_location::find_parsed_decl_containing_offset(parsed, offset).map(
            |(name, kind, span)| DefInfo {
                name: name.to_string(),
                kind: kind.to_string(),
                span: SourceSpanInfo {
                    start: span.start,
                    end: span.end,
                },
            },
        )
    }

    /// Find the entity (and optionally member) at the given 1-based `(line, col)`
    /// source position.
    ///
    /// Delegates to `reify_eval::resolve_entity_at_source_position`, which uses
    /// a two-layer containment model:
    /// - **Outer span**: the parsed `StructureDef.span` / `OccurrenceDef.span`,
    ///   covering the full `pub structure NAME { ... }` byte range including the
    ///   header line and closing brace.  Fixes the off-by-one where clicking a
    ///   structure name resolved to the previous structure (task 3880).
    /// - **Narrow step**: member-span priority order (value_cells → realizations →
    ///   sub_components → template name).
    ///
    /// Returns:
    /// - `Some("Entity.member")` when the cursor is inside a value cell's span.
    /// - `Some("Entity.name")` when the cursor is inside a realization or
    ///   sub_component declaration body.
    /// - `Some("Entity")` when the cursor is inside the template's source span
    ///   but outside any specific named member (e.g. the header line, a constraint
    ///   line, or the closing brace).
    /// - `None` when `line` or `col` is zero, when no module is loaded, when the
    ///   position is outside every template's source span, or when the position is
    ///   past the end of source.
    ///
    /// # Caching
    /// `parsed_cache` and `line_offsets_cache` are populated in `commit_state`
    /// alongside `compiled` and are threaded through to the resolver so the
    /// parse-span lookup and byte-offset conversion are O(D + log M) rather than
    /// requiring a re-parse on every cursor/hover event.
    pub fn get_entity_at_source_location(&self, line: u32, col: u32) -> Option<String> {
        // Documented contract: zero line or column is out-of-range → None.
        if line == 0 || col == 0 {
            return None;
        }
        let (_key, source) = self.resolve_source()?;

        debug_assert!(
            self.parsed_cache.is_some() && self.line_offsets_cache.is_some(),
            "cache invariant broken: parsed_cache and line_offsets_cache must be Some \
             whenever compiled is Some (i.e., whenever resolve_source succeeds)"
        );

        // Read the cached parse result and line-offset table.  Guard defensively
        // against None (shouldn't occur given the debug_assert above, but avoids
        // a panic in release builds — mirrors the same guard in get_containing_definition).
        let parsed = self.parsed_cache.as_ref()?;
        let line_offsets = self.line_offsets_cache.as_deref()?;
        let compiled = self.core.compiled()?;

        reify_eval::resolve_entity_at_source_position(compiled, parsed, source, line_offsets, line, col)
    }

    /// Resolve a `"Entity.member"` cell id to the byte range of that param's
    /// DEFAULT EXPRESSION in the currently-loaded source.
    ///
    /// Substrate for INV-GUI-3 (PRD `docs/prds/v0_6/ai-native-editing.md` §6.1).
    /// The returned span is the default EXPRESSION range ONLY — never the whole
    /// `param … = …` declaration and never the leading `=` — so a caller can
    /// splice a replacement literal into exactly that range.
    ///
    /// `None` means "no rewritable default literal for this cell", and is the
    /// caller's cue to emit a structured error rather than to guess (PRD §6.1,
    /// §7 B7). It covers every non-resolving case:
    ///
    /// * the cell id is malformed (no `.`),
    /// * no module is loaded, so there is no parse to read,
    /// * the entity is neither a `structure def` nor an `occurrence def` in the
    ///   loaded module,
    /// * the member is not a param, has no default, or is declared more than
    ///   once (see [`reify_ast::find_param_default_span`] for the refusal rule),
    /// * the cell id names an INSTANCE path (`Parent.childinst.member`),
    /// * the member names a param inside a PORT body.
    ///
    /// The instance-path case is worth stating outright: `parse_cell_id` splits
    /// on the FIRST `.`, so `"Parent.childinst.height"` yields the member
    /// `"childinst.height"`, which matches no `ParamDecl.name` because a member
    /// name never contains a `.`. That `None` is correct rather than a gap — a
    /// shared structure's default literal is not one instance's value, and
    /// rewriting it would change every instance.
    ///
    /// The port-body case is likewise correct rather than a gap. The compiler
    /// registers a port member under the COMPOSITE name
    /// `ValueCellId(entity, "<port>.<param>")` and files it in
    /// `CompiledPort.members`, which is never merged into
    /// `TopologyTemplate.value_cells` — the only map [`Self::set_parameter`] and
    /// the property panel key off. So a port-body param is not an editable cell
    /// under EITHER spelling, and returning a span for its bare name would hand
    /// a caller a range it must not splice.
    ///
    /// The entity-bearing variant set — `Structure` and `Occurrence` — is
    /// deliberately identical to
    /// `reify_eval::source_location::find_parsed_decl_containing_offset`, whose
    /// own doc calls out that a single shared variant list is what keeps these
    /// traversals from drifting when a new `Declaration` variant is added. The
    /// OTHER two member-bearing top-level declarations, `TraitDecl` and
    /// `PurposeDef` (which additionally nests `structures: Vec<StructureDef>`),
    /// are deliberately out of reach, matching that same helper: neither is an
    /// entity a `cell_id` names, so reaching into them could only produce a span
    /// belonging to a different declaration than the caller asked about.
    ///
    /// Unlike [`Self::get_containing_definition`] and
    /// [`Self::get_entity_at_source_location`], this method does NOT gate on
    /// `resolve_source()` and carries no `debug_assert!` on the caches: spans
    /// come straight from the AST, and `parsed_cache` is legitimately `None` on
    /// a `load_from_compiled`-injected session. Plain `as_ref()?` is the correct
    /// degradation.
    pub fn resolve_param_default_span(&self, cell_id_str: &str) -> Option<reify_core::SourceSpan> {
        self.resolve_param_default_expr(cell_id_str).map(|e| e.span)
    }

    /// Resolve the default EXPRESSION a `cell_id` names — the `&Expr`-returning
    /// primitive [`Self::resolve_param_default_span`] is a thin
    /// `.map(|e| e.span)` over.
    ///
    /// Every word of that method's contract applies here unchanged (the
    /// entity-variant set, the instance-path and port-body exclusions, the
    /// multiply-declared refusal, the `parsed_cache`-absent degradation); this
    /// is the SAME walk, stopping one step earlier. Splitting it this way —
    /// rather than re-deriving the expression from the span, or hand-rolling a
    /// second member walk in this file — is what keeps the span a caller
    /// splices and the expression it type-checks first from ever disagreeing.
    ///
    /// The caller that needs the expression rather than the span is
    /// [`Self::apply_param_to_source`], whose literal-ness gate must refuse to
    /// overwrite a `BinOp`, an `Auto`, a call or an identifier — which is, as
    /// of γ, EVERY production caller. [`Self::resolve_param_default_span`] is
    /// retained as α's published resolver for the consumers still to come (δ's
    /// MCP tools, η's re-homed slider) and for anything that wants a span
    /// without an AST borrow; it is not dead by oversight, but it has no
    /// in-tree caller today beyond its own tests.
    pub fn resolve_param_default_expr(&self, cell_id_str: &str) -> Option<&reify_ast::Expr> {
        // Reuse `parse_cell_id` — the SAME parse `set_parameter` uses — so this
        // resolver and the entry point that will consume it cannot disagree
        // about what a cell_id denotes.
        let cell = parse_cell_id(cell_id_str).ok()?;
        let parsed = self.parsed_cache.as_ref()?;
        parsed.declarations.iter().find_map(|decl| match decl {
            reify_ast::Declaration::Structure(s) if s.name == cell.entity => {
                reify_ast::find_param_default_expr(&s.members, &cell.member)
            }
            reify_ast::Declaration::Occurrence(o) if o.name == cell.entity => {
                reify_ast::find_param_default_expr(&o.members, &cell.member)
            }
            _ => None,
        })
    }
}

/// Does the ENTRY module's parse declare `entity` as a top-level structure or
/// occurrence?
///
/// The existence half of `EngineSession::resolve_param_default_expr`'s walk,
/// separated out so `resolve_rewritable_default_span` can tell "this cell
/// belongs to an imported module" apart from "this cell's param has no default"
/// — two rejections with different answers for the caller, which the walk's
/// single `Option::None` cannot distinguish.
///
/// The variant set is `Structure | Occurrence`, deliberately IDENTICAL to that
/// walk's: a name this predicate accepted but the walk did not reach (or the
/// reverse) would put `resolve_rewritable_default_span` back to reporting the
/// wrong category, which is the whole point of splitting them.
fn entry_declares_entity(parsed: &reify_ast::ParsedModule, entity: &str) -> bool {
    parsed.declarations.iter().any(|decl| match decl {
        reify_ast::Declaration::Structure(s) => s.name == entity,
        reify_ast::Declaration::Occurrence(o) => o.name == entity,
        _ => false,
    })
}

// ---- GUI-state helpers -------------------------------------------------------

/// Map `ValueCellKind` to its **capitalized** GUI-state string form.
///
/// Used in `build_values` (and therefore in both `build_gui_state` and
/// `build_preview_gui_state`) for the `kind` field of `ValueData`.
///
/// # Capitalization convention
/// The GUI-state API uses capitalized strings (`"Param"`, `"Let"`, `"Auto"`).
/// The entity-tree and identity-map APIs use the lowercase form — see
/// `cell_kind_tree_str`.  The difference is intentional: the two APIs are
/// consumed by different frontend components with different display contracts.
fn cell_kind_gui_str(kind: ValueCellKind) -> &'static str {
    match kind {
        ValueCellKind::Param => "Param",
        ValueCellKind::Let => "Let",
        ValueCellKind::Auto { .. } => "Auto",
    }
}

/// Map `ValueCellKind` to its **lowercase** tree / identity-map string form.
///
/// Used in `build_template_node` and `get_entity_identity_map` for the `kind`
/// field of `EntityTreeNode` and `structural_fingerprint`.
///
/// # Capitalization convention
/// These APIs use lowercase strings (`"param"`, `"let"`, `"auto"`).  The
/// GUI-state API uses the capitalized form — see `cell_kind_gui_str`.
fn cell_kind_tree_str(kind: ValueCellKind) -> &'static str {
    match kind {
        ValueCellKind::Param => "param",
        ValueCellKind::Let => "let",
        ValueCellKind::Auto { .. } => "auto",
    }
}

/// Build the `Vec<ValueData>` shared between `build_gui_state` and
/// `build_preview_gui_state`.
///
/// Iterates every value cell in every template, formats its current value and
/// determinacy state, and returns one `ValueData` per cell.  Extracting this
/// logic ensures that changes to value formatting are applied consistently to
/// both the live GUI state and the def-preview state.
///
/// # Freshness
///
/// When `engine` is `Some`, each cell's freshness is read via
/// `Engine::freshness(&NodeId::Value(cell.id))` — the stable always-public
/// accessor (arch §7.1 lines 716-728).  `CacheStore::freshness` returns
/// `Freshness::Final` for unknown nodes, so the default is safe.
///
/// When `engine` is `None` (preview path — `build_preview_gui_state` passes
/// `None` because the preview engine is a throwaway instance that is not
/// retained beyond the `get_def_preview` call), all cells default to
/// `"final"`.  The preview surface only shows values and constraints;
/// freshness badges are not meaningful for a single-definition preview
/// evaluated in isolation.
/// Extract the raw SI magnitude and dimension of a scalar value, for the
/// per-cell display-unit picker (task #5199). Recurses through
/// `Value::Option(Some(inner))` (an auto-resolve/optional wrapper) to the
/// underlying scalar. Returns `None` for anything else — non-scalar values,
/// `Value::Undef`, `Value::Option(None)` — which surface as `dimension: ""`,
/// `si_value: None` (no ladder, so the GUI keeps the static unit badge).
fn display_scalar(v: &reify_ir::Value) -> Option<(f64, DimensionVector)> {
    match v {
        reify_ir::Value::Scalar {
            si_value,
            dimension,
        } => Some((*si_value, *dimension)),
        reify_ir::Value::Option(Some(inner)) => display_scalar(inner),
        _ => None,
    }
}

/// Format the four display fields a `ValueData` cell derives directly from its
/// `Value`: the default-unit `value` / `unit` pair (`format_value`) plus the
/// per-cell canonical `si_value` + `dimension` name (`display_scalar`, task
/// #5199). Returns `(value, unit, si_value, dimension)`.
///
/// Shared by `build_values` (every cell) and `surface_geometry_derived_cells`
/// (each cell surfaced from `Undef` → Determined) so the two sites cannot drift
/// if the formatting rules change (e.g. dimension naming). For a non-scalar or
/// `Undef` value, `display_scalar` yields `None`, so `si_value` is `None` and
/// `dimension` is `""` — the GUI keeps the static unit badge with no ladder,
/// exactly as `format_value` renders the value itself.
fn format_determined_cell(val: &Value) -> (String, String, Option<f64>, String) {
    let (value, unit) = format_value(val);
    let (si_value, dim) = match display_scalar(val) {
        Some((s, d)) => (Some(s), Some(d)),
        None => (None, None),
    };
    let dimension = dim.and_then(|d| d.canonical_name()).unwrap_or("").to_string();
    (value, unit, si_value, dimension)
}

fn build_values(
    compiled: &reify_compiler::CompiledModule,
    check: &CheckResult,
    engine: Option<&Engine>,
) -> Vec<ValueData> {
    let mut values = Vec::new();
    for template in &compiled.templates {
        for cell in &template.value_cells {
            let val = check.values.get_or_undef(&cell.id);
            let (formatted_value, unit, si_value, dimension) = format_determined_cell(&val);
            let determinacy = match &val {
                reify_ir::Value::Undef => {
                    if cell.kind.is_auto() {
                        DeterminacyState::Auto
                    } else {
                        DeterminacyState::Undetermined
                    }
                }
                _ => DeterminacyState::Determined,
            };
            let freshness = engine
                .map(|e| {
                    let node = NodeId::Value(cell.id.clone());
                    String::from(format_freshness(&e.freshness(&node)))
                })
                .unwrap_or_else(|| String::from("final"));
            // γ (task #4739) demand-prune prior-value surface: when a cell is
            // `"pending"` (its body was demand-pruned by a warm selective build),
            // surface its last **substantive** cached value via the resolver so
            // the GUI displays the last good number, NOT the current
            // un-recomputed one (arch §8 prune-safety scenario 3 — "the displayed
            // number equals the last good value"). Computed ONLY for Pending
            // cells; final/intermediate/failed cells carry `None`. Formatted via
            // `format_value(..).0` (value part only), matching `value` above.
            let last_substantive_value = if freshness == "pending" {
                engine.and_then(|e| {
                    e.last_substantive_value(&NodeId::Value(cell.id.clone()))
                        .map(|v| format_value(&v).0)
                })
            } else {
                None
            };
            // Undef-cause reconstruction: for each Undef cell, walk the
            // dependency graph forward to reconstruct the root-cause set
            // (PRD A3: the origins side-map records only direct origins;
            // propagated cells are resolved by graph traversal).
            //
            // Performance note: `trace_undef_causes` is heavier than the
            // adjacent O(1) `freshness` lookup — each call reconstructs the
            // root-cause set by walking forward dependencies through undef
            // cells against the current snapshot (roughly O(dependency walk)
            // per cell). This is fine for typical models. For a model with
            // many Undef cells (e.g. a freshly-loaded model with most params
            // unbound), this becomes O(Undef cells × dependency walk) per
            // `build_values` call. If profiling identifies this as a hotspot,
            // consider a single-pass batch over the origins side-map + graph.
            let reason = match &val {
                reify_ir::Value::Undef => engine.and_then(|e| {
                    reify_eval::format_undef_causes(&e.trace_undef_causes(&cell.id))
                }),
                _ => None,
            };
            // `si_value` / `dimension` (task #5199, the substrate for the GUI's
            // per-cell display-unit picker) are computed once alongside
            // `value` / `unit` by `format_determined_cell` above.
            values.push(ValueData {
                cell_id: cell.id.to_string(),
                name: cell.id.member.clone(),
                value: formatted_value,
                unit,
                determinacy: format_determinacy(determinacy),
                entity_path: cell.id.entity.clone(),
                kind: cell_kind_gui_str(cell.kind).to_string(),
                freshness,
                reason,
                last_substantive_value,
                dimension,
                si_value,
            });
        }
    }
    values
}

/// Build the `Vec<ConstraintData>` shared between `build_gui_state` and
/// `build_preview_gui_state`.
///
/// Iterates the check result's constraint entries, cross-references the compiled
/// constraint for its expression text and value refs, and returns one
/// `ConstraintData` per entry.  Extracting this logic ensures that changes to
/// constraint formatting are applied consistently to both call sites.
///
/// The returned Vec is sorted by `node_id` ascending.  This imposes a
/// deterministic order at the production boundary: the upstream
/// `constraint_results` order can vary across independent engine constructions
/// due to HashMap/HashSet iteration seed variance.  `node_id` is the same key
/// `diff_gui_state` uses to match constraints and is unique per constraint, so
/// this is a total, stable order.  `diff_gui_state` and `delta_to_events`
/// preserve the Vec order, making `GuiState.constraints`, the diff deltas, and
/// the emitted "constraint-update" events all deterministic.
pub(crate) fn build_constraints(
    compiled: &reify_compiler::CompiledModule,
    check: &CheckResult,
) -> Vec<ConstraintData> {
    let mut constraints = Vec::new();
    for entry in &check.constraint_results {
        let status = match entry.satisfaction {
            Satisfaction::Satisfied => "Satisfied",
            Satisfaction::Violated => "Violated",
            Satisfaction::Indeterminate => "Indeterminate",
        };
        let (expression, parameter_ids) = compiled
            .templates
            .iter()
            .find_map(|t| {
                t.constraints
                    .iter()
                    .find(|c| c.id == entry.id)
                    .map(|c| (format_expr(&c.expr), collect_value_refs(&c.expr)))
            })
            .unwrap_or_default();
        constraints.push(ConstraintData {
            node_id: entry.id.to_string(),
            expression,
            status: status.to_string(),
            label: entry.label.clone(),
            parameter_ids,
        });
    }
    // Sort by node_id ascending (lexicographic on the stringified
    // "{entity}#constraint[{index}]" key) for a deterministic order.
    // Note: for an entity with >9 constraints, "[10]" sorts before "[2]"
    // — a total, stable, deterministic order is achieved either way, and
    // the GUI constraint-panel use case is not sensitive to numeric index order.
    constraints.sort_by(|a, b| a.node_id.cmp(&b.node_id));
    constraints
}

/// Task 5194: surface the kernel-derived geometry / mass-property cells and the
/// post-geometry constraint verdicts from a `tessellate_snapshot` `result` onto
/// the panel `values` / `constraints`.
///
/// `build_values` / `build_constraints` read the kernel-LESS `check.values` (the
/// `eval` / warm `edit_param` result), where the geometry-query post-processes
/// never ran. A `: Rigid` body's auto-derived `mass = volume(geometry) *
/// material.density`, `centroid`, `moment_of_inertia`, and `moi_principal` cells
/// therefore read `Undef` there, and the `moi_principal[0] > 0` PD constraint reads
/// Indeterminate. The kernel-bearing `tessellate_snapshot` DID run
/// `run_post_processes`, so `result.values` already carries those computed cells
/// (the same values CLI `reify eval` reads from `build().values`).
///
/// This helper is invoked ONCE from `build_gui_state`, the shared rebuild path for
/// EVERY GuiState entry point (load_file, update_source, set_parameter), so the
/// load and warm-edit paths cannot diverge. It keys on `ValueCellId`
/// (entity+member), which is stable across rebuilds, so a warm edit that clears the
/// realization cache and re-executes geometry under a fresh `GeometryHandleId`
/// still re-surfaces the same cells. It adds no kernel query — the constraint
/// re-check runs the kernel-less checker against the already-resolved
/// `result.values` — so the P0 kernel-less edit-latency gate is untouched.
///
/// Value overlay: for each cell the kernel-less pass left `Undef`
/// (`determinacy != "determined"`) that `result.values` resolves to a non-`Undef`
/// value, recompute `value` / `unit` / `determinacy` / `reason` / `si_value` /
/// `dimension` from the surfaced value (mirroring `build_values`' determined-cell
/// path). Already-`determined` cells keep their eval-computed values.
///
/// Constraint re-check: `tessellate_snapshot`'s own `result.constraint_results`
/// were checked BEFORE `run_post_processes` patched the mass-property cells, so a
/// constraint over such a cell still reads Indeterminate there. Re-check the
/// active constraints against the now-complete `result.values` and adopt any
/// verdict that resolved from Indeterminate → Satisfied / Violated. This mirrors
/// the post-geometry constraint re-check in `Engine::build` (engine_build.rs): a
/// previously Satisfied/Violated constraint cannot regress because the re-check
/// only ADDS now-resolved geometry cells, so only Indeterminate entries are
/// touched. Skipped entirely when this pass surfaced no cell, or when nothing is
/// Indeterminate (the common cases). Narrowing the dispatch further — to only the
/// constraints that reference a cell surfaced this pass — would need a subset-checking
/// `Engine` API in reify-eval (out of this task's scope); for a `: Rigid` body the
/// `moi_principal[0] > 0` PD constraint references a surfaced cell, so its re-check is
/// inherently required on every warm edit regardless.
///
/// ## Task #5338: the delta contract
///
/// `delta_values` / `delta_meshes` are the value and mesh halves of an INCREMENTAL
/// DELTA, **not** a full snapshot. The two halves have DIFFERENT normative homes,
/// and this is the site that states so — do not collapse them into one pointer:
///
/// * **Mesh half — normative upstream.** The DELTA CONTRACT block on
///   `Engine::demand_scoped_unified_pass` (reify-eval `engine_build.rs`) owns it:
///   a realization absent from `meshes` is HIDDEN *or* HASH-EXEMPT, and absence
///   means "retain the previously rendered mesh". Read it there; it is deliberately
///   not restated here.
/// * **Values half — established HERE.** That block is exclusively about `.meshes`
///   and says nothing about `values`, so the claim this function's discriminator
///   rests on is stated in this crate and nowhere else: a hash-exempt cell arrives
///   as an EXPLICIT `Value::Undef` ENTRY, never as an absent key. MEASURED on this
///   branch (2026-08-22) by printing `delta_values.get(&id)` alongside
///   `delta_meshes.len()` at the lookup site in the body below while running
///   `contained_rigid_sub_part_retention_works_once_the_demand_key_resolves`: the
///   two hash-exempt re-renders report `entry=Some(Undef) meshes=0` for all four
///   mass-prop cells, against `entry=Some(Scalar/Point/Tensor/List) meshes=1` on
///   the dispatched pass. Re-measure the same way if you need to re-verify it.
///
/// What this function must honour: an absent/`Undef` entry means "retain the
/// previous value", NOT "the value is gone". Task 5194's original overlay read the values as a snapshot
/// and left delta-omitted cells undetermined, so a `: Rigid` body's mass-prop cells
/// reverted to `Undef` from the SECOND selective re-render onward — on argv-launch,
/// watcher-reload and warm-edit alike. `cache` closes that: a delta-resolved value
/// is retained per `ValueCellId` and re-surfaced when the current delta omits it,
/// while a fresh non-`Undef` entry always wins over the retained one.
///
/// "The fresh one wins" only bites when the realization actually runs, and a warm
/// edit of a non-op-arg input (a density folded into `Material(...)`) leaves it
/// hash-exempt, so no fresh entry can exist to win. `EngineSession::set_parameter`
/// closes that half via `invalidate_geometry_derived_cache_for_entity` — the second
/// half of the guarantee, not an optional extra.
///
/// Retention is NOT applied blindly to every `Undef`: a hash-exempt gap and a
/// genuine degeneration both write `Undef`, and replaying a stale value for the
/// latter would paint a pre-edit mass as `determined`/`final`. The
/// `dispatched_entities` binding in the body separates them — see it for what that
/// signal can and cannot see.
///
/// Prune safety (arch §8) is discharged upstream at `EngineSession::sync_demand`,
/// so every entry reaching this function belongs to a demanded ENTITY and is safe
/// to serve as Final; see that method for the granularity limitation, and the
/// `geometry_derived_cache` field doc for why cell `freshness` is not a usable
/// HIDDEN-vs-HASH-EXEMPT discriminator.
///
/// The delta is passed as its two borrowed halves rather than as a whole
/// `TessellateResult` so the no-re-tessellation rebuild (`set_active_fea_case`)
/// can call this with an EMPTY delta — every cached entry is a gap by
/// construction there — without fabricating a result struct.
fn surface_geometry_derived_cells(
    engine: &Engine,
    values: &mut [ValueData],
    constraints: &mut [ConstraintData],
    delta_values: &ValueMap,
    delta_meshes: &[reify_eval::MeshSurface],
    cache: &mut HashMap<ValueCellId, Value>,
) {
    // Track whether this pass surfaced any cell from Undef → Determined. If it
    // did not, `result.values` resolved nothing the kernel-less panel was
    // missing, so the constraint re-check below cannot flip any verdict (every
    // constraint input is a panel cell, and an Indeterminate constraint only
    // resolves once one of its Undef inputs is surfaced here) — skip it and
    // spare the full active-constraint dispatch on every warm rebuild.
    let mut surfaced_any = false;
    // Task #5338: `(id, value)` for each cell this pass served from the retention
    // cache because the delta omitted it. `delta_values` still reads `Undef` for
    // these, so the constraint re-check below must dispatch against a merged view
    // or it would leave the `moi_principal[0] > 0` PD constraint Indeterminate
    // while the panel shows the very value that satisfies it. The value rides
    // along with the id so building that overlay needs no second cache lookup.
    let mut cache_sourced: Vec<(ValueCellId, Value)> = Vec::new();
    // Task #5338: the entities whose realizations ran in this pass, read off the
    // delta's own mesh side (one `MeshSurface` per realization that produced a
    // terminal handle; `entity_path` is the same join key `sync_demand` uses, and
    // is parsed with the SAME `parse_realization_key` so "what counts as a valid
    // realization key" has one definition).
    //
    // Built LAZILY — only once some cell actually presents an `Undef`/absent delta
    // entry — so the common warm rebuild, where every cell is already determined,
    // parses no mesh keys and allocates nothing.
    //
    // This is what separates the two states an `Undef` delta entry can encode:
    //   * `Undef` + realization ABSENT from the meshes → the HASH-EXEMPT delta gap
    //     this cache exists to bridge → RETAIN the previous value;
    //   * `Undef` + realization PRESENT → its geometry query genuinely resolved to
    //     nothing this pass → the retained value is now STALE and replaying it as
    //     `determined`/`final` would present a pre-edit mass as authoritative →
    //     DROP it and leave the cell undetermined.
    // A hash-exempt cell arrives as an EXPLICIT `Undef` ENTRY, never as an absent
    // key, so "absent vs present" in the value map alone cannot discriminate; the
    // mesh side can, and it is the same signal the DELTA CONTRACT already uses for
    // meshes. An unparseable key is simply not counted as dispatched, which fails
    // toward RETAIN rather than toward dropping a still-correct value.
    //
    // LIMIT OF THE SIGNAL, part 1: the join is the same string join `sync_demand`
    // does, so it inherits the same containment blindness — a CONTAINED body's mesh
    // key is `Asm.part#realization[0]` while its cells key on the template name
    // `RigidPart`, so a contained realization never registers as dispatched and its
    // `Undef` is always read as a gap. Reachable only inside a full-scope session,
    // since `sync_demand` prunes those entries outright (see its known-limitation
    // bullet); the containment case is pinned by
    // `contained_rigid_sub_part_is_not_served_as_final_under_the_composed_key`.
    //
    // LIMIT OF THE SIGNAL, part 2 (do not read the above as exact): mesh presence is
    // a PROXY for "the realization ran", and the two diverge in one shape — a
    // realization that IS dispatched but whose kernel OPS fail emits no terminal
    // handle, hence no mesh, while its geometry-query cells still arrive `Undef`.
    // That is indistinguishable here from hash-exempt, so the retained value is
    // replayed as Final: stale. The probe behind this discriminator (over the
    // `rigid_mass_props*` tests) was taken against the MOCK kernel, where the
    // induced failure is a POST-tessellation query that still yields a mesh, so it
    // never covered the op-failure shape. What bounds the residue: `commit_state`
    // clears on recompile and `invalidate_geometry_derived_cache_for_entity` drops
    // the edited entity, leaving reachable only an op failure in an entity OTHER
    // than the edited one — an extension of the cross-entity residual documented on
    // that method. Closing it needs reify-eval to report the dispatched-realization
    // set on `TessellateResult` directly (out of this task's locked scope); filed as
    // a follow-up under ticket `tkt_0RSMMRJ9E6HZHWYYYRFJP028ST`, not as a
    // tracked-pattern comment, since the curator assigns the task id asynchronously
    // and a cite must resolve to a live task to be valid.
    let mut dispatched_entities: Option<HashSet<String>> = None;
    for cell in values.iter_mut() {
        // Leave already-resolved cells untouched; only surface the ones the
        // kernel-less panel left Undef (undetermined / auto).
        if cell.determinacy == "determined" {
            continue;
        }
        let id = ValueCellId::new(&cell.entity_path, &cell.name);
        let fresh = delta_values.get(&id);
        // Task #5338: the delta is authoritative when it carries a real value —
        // adopt it and REFRESH the retention entry (a fresh delta wins, so a
        // recompute that reaches here is never masked by a stale replay; the case
        // where no fresh delta entry can exist at all is handled upstream by
        // `invalidate_geometry_derived_cache_for_entity`). When the delta omits the
        // cell or holds `Undef`, fall back to the retained value: that is the
        // HASH-EXEMPT "no geometry change occurred, keep the previous value" case.
        let val: Value = match fresh {
            Some(fresh) if !matches!(fresh, Value::Undef) => {
                cache.insert(id.clone(), fresh.clone());
                fresh.clone()
            }
            _ => {
                // An explicit `Undef` whose OWN realization ran this pass is a
                // genuine degeneration, not a delta gap (see `dispatched_entities`).
                if fresh.is_some()
                    && dispatched_entities
                        .get_or_insert_with(|| {
                            delta_meshes
                                .iter()
                                .filter_map(|m| {
                                    parse_realization_key(&m.entity_path).map(|rid| rid.entity)
                                })
                                .collect()
                        })
                        .contains(id.entity.as_str())
                {
                    cache.remove(&id);
                    continue;
                }
                match cache.get(&id) {
                    Some(retained) => {
                        let retained = retained.clone();
                        cache_sourced.push((id.clone(), retained.clone()));
                        retained
                    }
                    None => continue,
                }
            }
        };
        let (value, unit, si_value, dimension) = format_determined_cell(&val);
        cell.value = value;
        cell.unit = unit;
        cell.determinacy = format_determinacy(DeterminacyState::Determined);
        cell.reason = None;
        cell.dimension = dimension;
        cell.si_value = si_value;
        // The kernel-bearing build resolved this cell to a real, FINAL value —
        // not a demand-pruned "pending" one. `build_values` sourced `freshness`
        // (and any `last_substantive_value`) from the kernel-less snapshot, where
        // an auto-derived geometry cell can read `"pending"` with a stale prior
        // value; left as-is, the panel would show a Determined value under a
        // "pending" badge with a stale prior-value surface. Reset both so the
        // freshness badge and prior-value surface match the surfaced value.
        cell.freshness = "final".to_string();
        cell.last_substantive_value = None;
        surfaced_any = true;
    }

    // Task #5338: dispatch the re-check against the delta OVERLAID with whatever
    // this pass served from the retention cache — the same complete value set the
    // panel now shows. Without the overlay a hash-exempt rebuild would leave the
    // PD constraint Indeterminate next to a Determined `moi_principal`, which is
    // exactly the incoherence the dogfood retest reported.
    //
    // COST, stated honestly (an earlier revision of this comment claimed "the
    // overlay is built ONLY when the delta actually left a gap, so the no-gap warm
    // path is unchanged", which reads as "this is rare" and is misleading): with
    // retention in place the GAP path IS the warm path. Every hash-exempt
    // re-render of a `: Rigid` body serves its four mass-prop cells from the cache,
    // so `cache_sourced` is non-empty and `surfaced_any` is true on every such
    // pass; and the `moi_principal[0] > 0` PD constraint comes out of
    // `build_constraints` Indeterminate every time, because that panel is built
    // from the kernel-less snapshot. Both guard clauses therefore pass on the
    // STEADY-STATE re-render, not rarely. Pre-#5338 this pass surfaced nothing and
    // the dispatch was skipped outright, so this is a real move from a cold path to
    // a warm one.
    //
    // What that buys and what it costs: it buys constraint/panel coherence, which
    // is a correctness property, not a nicety — the alternative is a Satisfied-able
    // constraint permanently reading Indeterminate beside the value that satisfies
    // it. It costs, per re-render, one `im::HashMap` clone (O(1), structural
    // sharing) plus one insert per cache-sourced cell, and one
    // `check_constraints_with_values` — a kernel-LESS `values.clone()` +
    // active-constraint scan + dispatch over the active constraints
    // (reify-eval `engine_constraints.rs`). No kernel query, so the P0 kernel-less
    // edit-latency gate is untouched; the load is proportional to the constraint
    // graph, not to mesh size.
    //
    // Narrowing it further needs something this scope does not have. Skipping the
    // dispatch when only cache-sourced cells were surfaced is NOT sound on its own
    // (the verdicts would have to come from somewhere, and dropping them is the
    // incoherence above); memoizing verdicts on the merged value set needs an
    // equality/fingerprint over the whole `ValueMap` AND an argument that
    // `active_constraint_ids` cannot move while those values hold still — active
    // constraints are derived from the engine's own snapshot, not from the values
    // passed in, so that argument is not available here. A per-constraint subset
    // re-check API in reify-eval would close it properly; out of this task's locked
    // scope.
    //
    // The overlay is built INSIDE the guard so a pass that surfaces cells but has
    // no Indeterminate constraint left — the non-`Rigid` majority — pays neither
    // the clone nor the dispatch.
    if surfaced_any && constraints.iter().any(|c| c.status == "Indeterminate") {
        let merged: Option<ValueMap> = if cache_sourced.is_empty() {
            None
        } else {
            let mut merged = delta_values.clone();
            for (id, retained) in cache_sourced {
                merged.insert(id, retained);
            }
            Some(merged)
        };
        let recheck_values = merged.as_ref().unwrap_or(delta_values);

        if let Ok((recheck, _diags)) = engine.check_constraints_with_values(recheck_values) {
            for c in constraints.iter_mut() {
                if c.status != "Indeterminate" {
                    continue;
                }
                let Some(new_sat) = recheck
                    .iter()
                    .find(|e| e.id.to_string() == c.node_id)
                    .map(|e| e.satisfaction)
                else {
                    continue;
                };
                if new_sat == Satisfaction::Indeterminate {
                    continue;
                }
                c.status = match new_sat {
                    Satisfaction::Satisfied => "Satisfied",
                    Satisfaction::Violated => "Violated",
                    Satisfaction::Indeterminate => "Indeterminate",
                }
                .to_string();
            }
        }
    }
}

// ── PRD-3 γ: DisplayOutput occurrence walk → display_panes ────────────────────

/// Walk every Output occurrence sub in the compiled module and return one
/// `DisplayDirective` per recognised Output occurrence (PRD-3 γ, task 4765).
///
/// Replicates `Engine::build_outputs`' four-gate enumeration using the same
/// already-public primitives:
///
/// 1. `find_template` — module-first, prelude-fallback sub-component resolution.
/// 2. `entity_kind == EntityKind::Occurrence` — discard structure templates.
/// 3. `conforms_to_output` — only occurrences that conform to the `Output` trait.
/// 4. `values.get(ValueCellId::new(template, sub))` + `extract_output_export_spec`
///    — discard subs with no hydrated StructureInstance or no export spec.
///
/// Subject resolution follows `build_outputs` verbatim (sub.args `"subject"` →
/// `CompiledExprKind::ValueRef` → `Value::GeometryHandle.realization_ref`) but
/// reads `realization_ref` instead of `kernel_handle`, giving the entity-path
/// string that equals `MeshData.entity_path` by construction (inv.1).
///
/// A subject that does not resolve to a realized `GeometryHandle` yields no
/// entity path — the directive is **dropped** with a `warn!` so silent drops
/// are observable (inv.1 no-dangling; mirrors `build_tensegrity_wires`).
///
/// Only `OutputTarget::DisplayDeferred` occurrences (i.e. `DisplayOutput`) are
/// emitted — file Outputs (STL/STEP/3MF) are excluded at Gate 4 so a module with
/// no `DisplayOutput` yields an empty vec (inv.2).
fn collect_display_routing(
    module: &CompiledModule,
    prelude: &[CompiledModule],
    values: &ValueMap,
) -> (Vec<DisplayDirective>, Vec<AppearanceDirective>) {
    // Merge module + prelude trait_defs (mirrors engine_build.rs::build_outputs_with_result).
    let mut merged_trait_defs = module.trait_defs.clone();
    for pm in prelude {
        merged_trait_defs.extend(pm.trait_defs.iter().cloned());
    }

    let mut directives = Vec::new();
    let mut appearances = Vec::new();

    for template in &module.templates {
        for sub in &template.sub_components {
            // Gate 1: resolve occurrence template — module first, then prelude.
            let Some(occ_template) = find_template(&module.templates, &sub.structure_name)
                .or_else(|| {
                    prelude
                        .iter()
                        .find_map(|pm| find_template(&pm.templates, &sub.structure_name))
                })
            else {
                continue;
            };

            // Gate 2: must be an occurrence, not a structure.
            if occ_template.entity_kind != EntityKind::Occurrence {
                continue;
            }

            // Gate 3: must conform to the Output trait.
            if !conforms_to_output(&occ_template.trait_bounds, &merged_trait_defs) {
                continue;
            }

            // Gate 4: must have a hydrated StructureInstance whose export spec
            // is DisplayDeferred (i.e. a DisplayOutput, not STL/STEP/3MF).
            let instance_id = ValueCellId::new(&template.name, &sub.name);
            let Some(instance) = values.get(&instance_id) else {
                continue;
            };
            match extract_output_export_spec(instance) {
                Some(spec) if spec.format == OutputTarget::DisplayDeferred => {}
                _ => continue,
            }

            // Read pane from the instance's `pane` field (Int, default 0).
            let pane = if let Value::StructureInstance(d) = instance {
                match d.fields.get("pane") {
                    Some(Value::Int(i)) => *i as i32,
                    _ => 0,
                }
            } else {
                0
            };

            // Resolve subject: sub.args "subject" → ValueRef → GeometryHandle.realization_ref.
            let subject_path = sub
                .args
                .iter()
                .find_map(|(k, e)| (k.as_str() == "subject").then_some(e))
                .and_then(|e| match &e.kind {
                    CompiledExprKind::ValueRef(id) => values.get(id),
                    _ => None,
                })
                .and_then(|v| match v {
                    Value::GeometryHandle { realization_ref, .. } => {
                        Some(realization_ref.to_string())
                    }
                    _ => None,
                });

            // Gate: emit AppearanceDirective only when `style` is EXPLICITLY present
            // in sub.args at the call site (decision-3 / inv "empty when absent").
            // The hydrated instance.fields always contain `style` (defaulted), so we
            // cannot use instance.fields to distinguish explicit from defaulted — we
            // must check sub.args, which holds only call-site-provided arguments.
            let has_explicit_style = sub.args.iter().any(|(k, _)| k.as_str() == "style");

            match subject_path {
                Some(subject) => {
                    directives.push(DisplayDirective { subject: subject.clone(), pane });
                    if has_explicit_style {
                        let style = extract_display_style_data(instance);
                        appearances.push(AppearanceDirective { subject, style });
                    }
                }
                None => {
                    warn!(
                        template = %template.name,
                        sub = %sub.name,
                        "DisplayOutput subject unresolved/unrealized — directive dropped"
                    );
                }
            }
        }
    }

    (directives, appearances)
}

/// Extract a `DisplayStyleData` from a hydrated `DisplayOutput` StructureInstance.
///
/// Reads the nested `style: DisplayStyle` field from the DisplayOutput instance,
/// then extracts `color` (nested `Color` struct → r/g/b), `opacity`, `finish`
/// (Enum variant Matte→0/Satin→1/Gloss→2), and `wireframe`.  Alpha = opacity
/// (decision-4).  Tolerates both `Value::Real` and dimensionless `Value::Scalar`
/// for numeric fields (build_tensegrity_wires precedent).
fn extract_display_style_data(display_output: &Value) -> DisplayStyleData {
    fn to_f32(v: &Value) -> f32 {
        match v {
            Value::Real(f) => *f as f32,
            Value::Scalar { si_value, .. } => *si_value as f32,
            _ => 0.0,
        }
    }

    let (mut r, mut g, mut b, mut opacity) = (0.0_f32, 0.0_f32, 0.0_f32, 1.0_f32);
    let mut finish = 1u8; // Satin default
    let mut wireframe = false;

    // The DisplayOutput instance has a `style: DisplayStyle` field — read it first.
    if let Value::StructureInstance(outer) = display_output {
        match outer.fields.get("style") {
            Some(Value::StructureInstance(d)) => {
                if let Some(v) = d.fields.get("opacity") {
                    opacity = to_f32(v);
                }
                if let Some(Value::StructureInstance(cd)) = d.fields.get("color") {
                    if let Some(v) = cd.fields.get("r") { r = to_f32(v); }
                    if let Some(v) = cd.fields.get("g") { g = to_f32(v); }
                    if let Some(v) = cd.fields.get("b") { b = to_f32(v); }
                }
                if let Some(Value::Enum { variant, .. }) = d.fields.get("finish") {
                    finish = match variant.as_str() {
                        "Matte" => 0,
                        "Satin" => 1,
                        "Gloss" => 2,
                        _ => 1,
                    };
                }
                if let Some(Value::Bool(v)) = d.fields.get("wireframe") {
                    wireframe = *v;
                }
            }
            other => {
                // `style` field absent or not a StructureInstance.  Gate-4 proves
                // that `display_output` is a hydrated DisplayOutput StructureInstance,
                // so this branch indicates a hydration regression rather than a
                // normal missing-field.  Log it so the silent all-default appearance
                // is observable in diagnostics.
                warn!(
                    type_name = %outer.type_name,
                    style_present = other.is_some(),
                    "DisplayOutput `style` field absent or non-struct while \
                     explicit-style directive was requested — emitting \
                     all-default AppearanceDirective"
                );
            }
        }
    }

    DisplayStyleData { color: [r, g, b, opacity], finish, opacity, wireframe }
}

/// Project an `Appearance` StructureInstance to a `MeshAppearance` value.
///
/// Reads `color` (nested `Color` StructureInstance) via `resolve_color`, then
/// `metalness`/`roughness` as `Real`/`Scalar` f32 (defaults 0.0/0.5), and
/// `finish` as an Enum variant (Matte=0/Satin=1/Gloss=2, default 1).
///
/// This is the β/PRD-2 layer-2 per-mesh material channel projection, distinct
/// from `extract_display_style_data` which handles the layer-3 DisplayStyle
/// override channel.  Color is normalized `resolve_color` bytes/255 with
/// alpha=1.0 (opacity is a DisplayStyle field, not a material property).
pub(crate) fn project_appearance(appearance: &Value, diags: &mut Vec<Diagnostic>) -> crate::types::MeshAppearance {
    use reify_eval::appearance::resolve_color;
    use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId};

    // Returns Some(f32) when the value is a parseable Real or Scalar, None
    // otherwise.  The caller keeps its prior default on None, so a
    // present-but-wrong-type field (e.g. Int or Undef) does not silently
    // override the intended default (0.5 for roughness, 0.0 for metalness).
    fn to_f32(v: &Value) -> Option<f32> {
        match v {
            Value::Real(f) => Some(*f as f32),
            Value::Scalar { si_value, .. } => Some(*si_value as f32),
            _ => None,
        }
    }

    // Default: neutral black Color (resolve_color on named="" with r=g=b=0 → Rgb8{0,0,0}).
    // Overridden by the `color` field when the Appearance StructureInstance carries one.
    let mut color_val: Value = Value::StructureInstance(Box::new(StructureInstanceData {
        type_id: StructureTypeId(u32::MAX),
        type_name: "Color".to_string(),
        version: 1,
        fields: [
            ("named".to_string(), Value::String(String::new())),
            ("r".to_string(), Value::Real(0.0)),
            ("g".to_string(), Value::Real(0.0)),
            ("b".to_string(), Value::Real(0.0)),
        ]
        .into_iter()
        .collect::<PersistentMap<String, Value>>(),
    }));
    let mut metalness = 0.0_f32;
    let mut roughness = 0.5_f32;
    let mut finish = 1u8; // Satin default

    if let Value::StructureInstance(app) = appearance {
        if let Some(c) = app.fields.get("color") {
            color_val = c.clone();
        }
        if let Some(v) = app.fields.get("metalness")
            && let Some(f) = to_f32(v)
        {
            metalness = f;
        }
        if let Some(v) = app.fields.get("roughness")
            && let Some(f) = to_f32(v)
        {
            roughness = f;
        }
        if let Some(Value::Enum { variant, .. }) = app.fields.get("finish") {
            finish = match variant.as_str() {
                "Matte" => 0,
                "Satin" => 1,
                "Gloss" => 2,
                _ => 1,
            };
        }
    }

    let rgb = resolve_color(&color_val, diags);
    crate::types::MeshAppearance {
        color: [rgb.r as f32 / 255.0, rgb.g as f32 / 255.0, rgb.b as f32 / 255.0, 1.0],
        metalness,
        roughness,
        finish,
    }
}

// ---- Tensegrity wire extraction (T0b) ----------------------------------------

/// Extract `TensegrityWireData` records from every value cell in `compiled`.
///
/// Iterates the same cell loop as `build_values`, reads the post-eval `Value`
/// for each cell, and collects every `Value::StructureInstance` with
/// `type_name == "TensegrityWire"` found either:
/// - directly as the cell's value (standalone wire), or
/// - as elements of a `Value::List` (the typical `tensegrity_wires()` output).
///
/// For each wire instance, the six endpoint coords are flattened from
/// `Value::Scalar{si_value, ..}` or `Value::Real(v)` to `f64` SI.  Wires
/// with malformed or missing fields are skipped and logged at `warn!` level so
/// silent drops are observable in logs without changing the no-panic contract.
///
/// The owning entity is taken from `cell.id.entity` (e.g. `"TPrism"`).
///
/// # Limitations (T0b scope)
///
/// **Template-level extraction only**: `entity_path` is the *template* name
/// (e.g. `"TPrism"`), not a per-instance path.  If a `TPrism` is instantiated
/// multiple times in an assembly, all instances contribute wires with the same
/// `entity_path` and local-frame coordinates — per-instance placement/transforms
/// are NOT applied.  A future instancing task must address this.
///
/// **Aliased-cell double-counting**: if the same wire list is reachable via two
/// value cells (e.g. `let w2 = wires`), wires are extracted twice.  This is
/// unlikely in practice because T0a binds the wire list to one cell; if it
/// becomes an issue, deduplicate by `(entity_path, x1, y1, z1, x2, y2, z2)`.
///
/// **Second iteration over value cells**: this function walks the same
/// `compiled.templates → template.value_cells` loop and calls
/// `check.values.get_or_undef` for each cell, independently of `build_values`.
/// For large modules this means each cell's `Value` is cloned twice per
/// `build_gui_state` call.  The separation is intentional for clarity and
/// matches the T0b scope boundary; fold into `build_values` if profiling shows
/// the duplication as a bottleneck.
fn build_tensegrity_wires(
    compiled: &reify_compiler::CompiledModule,
    check: &CheckResult,
) -> Vec<TensegrityWireData> {
    let mut wires = Vec::new();
    for template in &compiled.templates {
        for cell in &template.value_cells {
            let val = check.values.get_or_undef(&cell.id);
            let entity_path = &cell.id.entity;
            collect_wires_from_value(&val, entity_path, &mut wires);
        }
    }
    wires
}

/// Collect `TensegrityWireData` records from a single cell `Value`.
///
/// Matches either a standalone `TensegrityWire` instance or a
/// `List` of `TensegrityWire` instances (the output of `tensegrity_wires()`).
/// All other variants are silently ignored.
///
/// Logs a `warn!` when a `TensegrityWire` instance is found but has malformed
/// or missing fields (i.e. `wire_data_from_instance` returns `None`), so silent
/// drops are observable in logs without changing the no-panic contract.
fn collect_wires_from_value(val: &Value, entity_path: &str, out: &mut Vec<TensegrityWireData>) {
    match val {
        Value::StructureInstance(data) if data.type_name == "TensegrityWire" => {
            if let Some(wire) = wire_data_from_instance(&data.fields, entity_path) {
                out.push(wire);
            } else {
                warn!(
                    entity = %entity_path,
                    "skipping malformed TensegrityWire instance (missing or non-numeric field)"
                );
            }
        }
        Value::List(items) => {
            for item in items.iter() {
                if let Value::StructureInstance(data) = item
                    && data.type_name == "TensegrityWire"
                {
                    if let Some(wire) = wire_data_from_instance(&data.fields, entity_path) {
                        out.push(wire);
                    } else {
                        warn!(
                            entity = %entity_path,
                            "skipping malformed TensegrityWire instance in list (missing or non-numeric field)"
                        );
                    }
                }
            }
        }
        _ => {}
    }
}

/// Extract a `TensegrityWireData` from a `TensegrityWire` instance's fields.
///
/// Returns `None` if `kind` is missing/non-string or any coordinate field is
/// missing/non-numeric — the caller silently drops malformed wires.
fn wire_data_from_instance(
    fields: &reify_ir::PersistentMap<String, Value>,
    entity_path: &str,
) -> Option<TensegrityWireData> {
    let kind = match fields.get(&"kind".to_string()) {
        Some(Value::String(s)) => s.clone(),
        _ => return None,
    };
    let x1 = scalar_to_f64(fields.get(&"x1".to_string())?)?;
    let y1 = scalar_to_f64(fields.get(&"y1".to_string())?)?;
    let z1 = scalar_to_f64(fields.get(&"z1".to_string())?)?;
    let x2 = scalar_to_f64(fields.get(&"x2".to_string())?)?;
    let y2 = scalar_to_f64(fields.get(&"y2".to_string())?)?;
    let z2 = scalar_to_f64(fields.get(&"z2".to_string())?)?;
    Some(TensegrityWireData {
        entity_path: entity_path.to_string(),
        kind,
        x1,
        y1,
        z1,
        x2,
        y2,
        z2,
    })
}

// ---- Tensegrity surface (membrane) extraction (β/task 4413) ─────────────────

/// Walk every value cell in the compiled module and collect `TensegritySurface`
/// instances (as emitted by the `tensegrity_surfaces()` builtin, α/task 4412).
///
/// Each cell is inspected via `collect_surfaces_from_value`; the entity path
/// comes from `cell.id.entity`.
///
/// Malformed facets (missing or wrong-typed fields) are **skipped with a
/// `warn!` log** — no panic.  Duplicate-facet suppression is left to callers;
/// unlikely in practice because α binds the surface list to one cell.
fn build_tensegrity_surfaces(
    compiled: &reify_compiler::CompiledModule,
    check: &CheckResult,
) -> Vec<TensegritySurfaceData> {
    let mut surfaces = Vec::new();
    for template in &compiled.templates {
        for cell in &template.value_cells {
            let val = check.values.get_or_undef(&cell.id);
            let entity_path = &cell.id.entity;
            collect_surfaces_from_value(&val, entity_path, &mut surfaces);
        }
    }
    surfaces
}

/// Collect `TensegritySurfaceData` records from a single cell `Value`.
///
/// Matches either a standalone `TensegritySurface` instance or a
/// `List` of `TensegritySurface` instances (the output of `tensegrity_surfaces()`).
/// All other variants are silently ignored.
///
/// Logs a `warn!` when a `TensegritySurface` instance is found but has malformed
/// or missing fields (i.e. `surface_data_from_instance` returns `None`), so silent
/// drops are observable in logs without changing the no-panic contract.
fn collect_surfaces_from_value(
    val: &Value,
    entity_path: &str,
    out: &mut Vec<TensegritySurfaceData>,
) {
    match val {
        Value::StructureInstance(data) if data.type_name == "TensegritySurface" => {
            if let Some(surface) = surface_data_from_instance(&data.fields, entity_path) {
                out.push(surface);
            } else {
                warn!(
                    entity = %entity_path,
                    "skipping malformed TensegritySurface instance (missing or wrong-typed field)"
                );
            }
        }
        Value::List(items) => {
            for item in items.iter() {
                if let Value::StructureInstance(data) = item
                    && data.type_name == "TensegritySurface"
                {
                    if let Some(surface) = surface_data_from_instance(&data.fields, entity_path) {
                        out.push(surface);
                    } else {
                        warn!(
                            entity = %entity_path,
                            "skipping malformed TensegritySurface instance in list (missing or wrong-typed field)"
                        );
                    }
                }
            }
        }
        _ => {}
    }
}

/// Extract a `TensegritySurfaceData` from a `TensegritySurface` instance's fields.
///
/// Returns `None` if `kind` is missing/non-string, any of `i0/i1/i2` is
/// missing/non-integer, or any coordinate field is missing/non-numeric — the
/// caller silently drops malformed facets (no-panic contract).
///
/// Exposed as `pub(crate)` so tests in the sibling `tests/` module can pin the
/// malformed-field / no-panic contract without round-tripping through Reify source.
pub(crate) fn surface_data_from_instance(
    fields: &reify_ir::PersistentMap<String, Value>,
    entity_path: &str,
) -> Option<TensegritySurfaceData> {
    let kind = match fields.get(&"kind".to_string()) {
        Some(Value::String(s)) => s.clone(),
        _ => return None,
    };
    let i0 = match fields.get(&"i0".to_string()) {
        Some(Value::Int(i)) => *i,
        _ => return None,
    };
    let i1 = match fields.get(&"i1".to_string()) {
        Some(Value::Int(i)) => *i,
        _ => return None,
    };
    let i2 = match fields.get(&"i2".to_string()) {
        Some(Value::Int(i)) => *i,
        _ => return None,
    };
    let x0 = scalar_to_f64(fields.get(&"x0".to_string())?)?;
    let y0 = scalar_to_f64(fields.get(&"y0".to_string())?)?;
    let z0 = scalar_to_f64(fields.get(&"z0".to_string())?)?;
    let x1 = scalar_to_f64(fields.get(&"x1".to_string())?)?;
    let y1 = scalar_to_f64(fields.get(&"y1".to_string())?)?;
    let z1 = scalar_to_f64(fields.get(&"z1".to_string())?)?;
    let x2 = scalar_to_f64(fields.get(&"x2".to_string())?)?;
    let y2 = scalar_to_f64(fields.get(&"y2".to_string())?)?;
    let z2 = scalar_to_f64(fields.get(&"z2".to_string())?)?;
    Some(TensegritySurfaceData {
        entity_path: entity_path.to_string(),
        kind,
        i0,
        i1,
        i2,
        x0,
        y0,
        z0,
        x1,
        y1,
        z1,
        x2,
        y2,
        z2,
    })
}

// ---- Mechanism descriptor helpers -------------------------------------------

/// Extract joint descriptors and their identity sequence from a valid (non-errored) mechanism Map.
///
/// Returns `(joints, seen_joints)` where:
/// - `joints` is the ordered `Vec<JointDescriptor>` for this mechanism.
/// - `seen_joints` is the parallel `Vec<Value>` of joint Maps in first-encounter order,
///   used by `resolve_driving_params_from_ast` to look up joint indices without
///   re-walking the bodies list.
///
/// Walks the `bodies` list and collects the `"at"` field of each body record.
/// Deduplicates by structural equality (same joint Map referenced from multiple
/// bodies gets one descriptor).  Assigns `joint_index` in first-encounter order.
///
/// Non-Map `"at"` values (malformed source) are silently skipped; no phantom
/// "unknown" joint row is added.  `seen_joints` and `joints` always have
/// matching indices so the AST resolver can use `seen_joints[i]` → `joints[i]`.
///
/// `driving_param_cell_id` and `current_value_si` are left `None` here; they
/// are populated by `resolve_driving_params_from_ast` (step-12 / step-24).
///
/// Exposed as `pub(crate)` so unit tests in the sibling `tests/` module can
/// pin the malformed-shape contract directly without round-tripping through
/// Reify source.  The contract — non-Map `"at"` produces no descriptor, axis
/// length ≠ 3 produces `axis = None` — is already enforced by
/// `extract_joint_descriptor` and `extract_axis`; these tests lock it down.
pub(crate) fn extract_joints_from_mechanism(
    map: &std::collections::BTreeMap<Value, Value>,
) -> (Vec<JointDescriptor>, Vec<Value>) {
    let bodies = match map.get(&Value::String("bodies".to_string())) {
        Some(Value::List(b)) => b,
        _ => return (Vec::new(), Vec::new()),
    };

    let mut seen_joints: Vec<Value> = Vec::new();
    let mut joints = Vec::new();

    for body in bodies {
        let body_map = match body {
            Value::Map(b) => b,
            _ => continue,
        };

        let joint_val = match body_map.get(&Value::String("at".to_string())) {
            Some(v) => v,
            None => continue,
        };

        // Skip world sentinel (not a real joint).
        if is_world_sentinel(joint_val) {
            continue;
        }

        // Deduplicate by structural equality.
        if seen_joints.iter().any(|j| j == joint_val) {
            continue;
        }

        // Build the descriptor before committing to seen_joints so that only
        // valid joint Maps are indexed.  Non-Map "at" values (None path) are
        // simply skipped; seen_joints and joints stay in sync.
        let joint_index = seen_joints.len();
        if let Some(descriptor) = extract_joint_descriptor(joint_val, joint_index) {
            seen_joints.push(joint_val.clone());
            joints.push(descriptor);
        }
    }

    (joints, seen_joints)
}

/// Returns `true` if `val` is the world sentinel Map (`{ "kind": "world" }`).
fn is_world_sentinel(val: &Value) -> bool {
    match val {
        Value::Map(m) => {
            m.get(&Value::String("kind".to_string())) == Some(&Value::String("world".to_string()))
        }
        _ => false,
    }
}

/// Build a `JointDescriptor` from a single joint `Value::Map`.
///
/// Returns `None` if `joint_val` is not a `Value::Map` (e.g. a malformed `"at"`
/// field), so the caller can skip the slot rather than surfacing a phantom
/// `kind="unknown"` row in the UI.
///
/// Extracts `kind`, `axis`, `range`, and `dimension` from the joint Map.
/// Coupling and fixed joints have no axis/range; their descriptors carry `None`
/// for those fields.  `driving_param_cell_id` and `current_value_si` are always
/// `None` at this point (populated by later steps).
fn extract_joint_descriptor(joint_val: &Value, joint_index: usize) -> Option<JointDescriptor> {
    let joint_map = match joint_val {
        Value::Map(m) => m,
        // Non-Map "at" values (malformed source) are skipped; no phantom row.
        _ => return None,
    };

    let kind = match joint_map.get(&Value::String("kind".to_string())) {
        Some(Value::String(k)) => k.clone(),
        _ => "unknown".to_string(),
    };

    let (dimension, axis, range_lower_si, range_upper_si) = match kind.as_str() {
        "prismatic" => {
            let axis = extract_axis(joint_map);
            let (lo, hi) = extract_range(joint_map);
            ("length".to_string(), axis, lo, hi)
        }
        "revolute" => {
            let axis = extract_axis(joint_map);
            let (lo, hi) = extract_range(joint_map);
            ("angle".to_string(), axis, lo, hi)
        }
        // coupling and fixed have no independent motion variable.
        _ => ("dimensionless".to_string(), None, None, None),
    };

    // Default binding keyed off joint kind.  Prismatic/revolute default to
    // LiteralBound with a joint_index-based synth name; the AST resolver
    // (resolve_driving_params_from_ast) promotes this to ParamBound or refines
    // the synth name when a `bind()` call is found.
    let binding = match kind.as_str() {
        "fixed" => JointBinding::FixedNoMotion,
        "coupling" => JointBinding::CouplingDerived {
            source_joint: String::new(), // source detection deferred to ζ work
        },
        "prismatic" | "revolute" => JointBinding::LiteralBound {
            synth_param_name: format!("__joint_{joint_index}_v"),
            initial_value_si: None,
            scrubbable: true,
        },
        _ => JointBinding::FixedNoMotion, // conservative default for unknown kinds
    };

    Some(JointDescriptor {
        joint_index,
        kind,
        dimension,
        range_lower_si,
        range_upper_si,
        axis,
        driving_param_cell_id: None,
        current_value_si: None,
        binding,
    })
}

/// Extract a 3-component axis from a joint Map's `"axis"` field.
///
/// The axis is stored as `Value::Vector([Real(x), Real(y), Real(z)])` (or
/// Scalar components — any variant accepted by the joints stdlib validator).
/// Returns `None` if the field is missing or malformed.
fn extract_axis(joint_map: &std::collections::BTreeMap<Value, Value>) -> Option<[f64; 3]> {
    let axis_val = joint_map.get(&Value::String("axis".to_string()))?;
    match axis_val {
        Value::Vector(items) if items.len() == 3 => {
            let x = scalar_to_f64(&items[0])?;
            let y = scalar_to_f64(&items[1])?;
            let z = scalar_to_f64(&items[2])?;
            Some([x, y, z])
        }
        _ => None,
    }
}

/// Extract the lower and upper SI bounds from a joint Map's `"range"` field.
///
/// The range is stored as `Value::Range { lower, upper, .. }` where each bound
/// (when `Some`) is a `Value::Scalar { si_value, .. }`.  Returns `(None, None)`
/// if the field is missing or malformed.
fn extract_range(
    joint_map: &std::collections::BTreeMap<Value, Value>,
) -> (Option<f64>, Option<f64>) {
    let range_val = match joint_map.get(&Value::String("range".to_string())) {
        Some(v) => v,
        None => return (None, None),
    };
    match range_val {
        Value::Range { lower, upper, .. } => {
            let lo = lower.as_deref().and_then(scalar_to_f64);
            let hi = upper.as_deref().and_then(scalar_to_f64);
            (lo, hi)
        }
        _ => (None, None),
    }
}

/// Extract the SI numeric value from a `Value::Scalar` or `Value::Real`.
fn scalar_to_f64(val: &Value) -> Option<f64> {
    match val {
        Value::Scalar { si_value, .. } => Some(*si_value),
        Value::Real(f) => Some(*f),
        Value::Int(i) => Some(*i as f64),
        _ => None,
    }
}

// ---- driving-param resolution (step-12) ----------------------------------------

/// Represents the value side of a `bind(joint, value)` expression inside a `snapshot()` call.
///
/// Returned by [`collect_snapshot_bind_pairs`] after the η-engine extension:
/// - `Param`: the value side is a bare identifier; downstream resolved against Param cells.
/// - `Literal`: the value side is a literal expression (`QuantityLiteral` or `NumberLiteral`)
///   whose SI value is computed from `reify_core::unit_symbol_to_si` — the DSL's own
///   built-in symbol table, since this site's universe is .ri SOURCE tokens rather
///   than the GUI's curated display labels (task #5757). A compound unit expression
///   (`UnitExpr::Mul`/`Div`/`Pow`) still yields no SI value; that resolver is task γ (#3803).
enum BindValue {
    /// A bare identifier referring to a Param cell (e.g. `bind(j, y_pos)`).
    Param(String),
    /// A literal expression providing an immediate SI-convertible value
    /// (e.g. `bind(j, 50mm)` or `bind(j, 0.5)`).
    Literal(reify_ast::Expr),
}

/// Walk the parsed declarations looking for `snapshot(mech, [bind(joint, param), …])`
/// invocations and populate `driving_param_cell_id` on the matching joint descriptor.
///
/// Only the canonical form is resolved:
/// - Both arguments to `bind()` must be bare identifiers (`Ident`).
/// - The value-side identifier must refer to a `Param` cell in the same structure.
///
/// Joints whose binding expression is a literal or a complex sub-expression remain
/// with `driving_param_cell_id = None` (read-only in the slider panel).
///
/// This is best-effort and matches by **textual function name** — a user-defined
/// function named `snapshot` or `bind` in the same module would shadow the stdlib
/// versions and produce incorrect results.  The resolver does not verify that the
/// matched names refer to stdlib symbols.  Widening the name check to use the stdlib
/// registry is left as future work; for v0.1 the canonical usage pattern (stdlib
/// `snapshot`/`bind` in a structure body) is the only supported case.
///
/// `seen_joints_cache` maps each mechanism `cell_id` string to the ordered
/// `Vec<Value>` produced by `extract_joints_from_mechanism` for that mechanism.
/// Using the cache avoids the O(B) body re-walk that the earlier implementation
/// performed for every `(bind-pair, descriptor)` pair.
fn resolve_driving_params_from_ast(
    descriptors: &mut [MechanismDescriptor],
    seen_joints_cache: &HashMap<String, Vec<Value>>,
    parsed: &reify_ast::ParsedModule,
    check: &CheckResult,
    compiled: &CompiledModule,
) {
    for decl in &parsed.declarations {
        let structure = match decl {
            reify_ast::Declaration::Structure(s) => s,
            _ => continue,
        };
        let structure_name = &structure.name;

        // Find the compiled template for this structure.
        let template = match compiled
            .templates
            .iter()
            .find(|t| t.name == *structure_name)
        {
            Some(t) => t,
            None => continue,
        };

        // Collect (joint_ident, bind_value) pairs from all snapshot() calls.
        let mut bind_pairs: Vec<(String, BindValue)> = Vec::new();
        for member in &structure.members {
            let expr = match member {
                reify_ast::MemberDecl::Let(l) => &l.value,
                _ => continue,
            };
            collect_snapshot_bind_pairs(expr, &mut bind_pairs);
        }

        // Resolve each pair.
        for (joint_cell_name, bind_value) in bind_pairs {
            // Look up the joint Map value by cell id.
            let joint_cell_id = ValueCellId::new(structure_name, &joint_cell_name);
            let joint_val = check.values.get_or_undef(&joint_cell_id);
            if matches!(joint_val, Value::Undef) {
                continue;
            }

            match bind_value {
                BindValue::Param(value_cell_name) => {
                    // The value side must be a Param cell (not a Let or Auto).
                    let is_param = template
                        .value_cells
                        .iter()
                        .any(|c| c.id.member == value_cell_name && matches!(c.kind, ValueCellKind::Param));
                    if !is_param {
                        continue;
                    }

                    let param_cell_id_str = format!("{}.{}", structure_name, value_cell_name);

                    // Scan descriptors from this structure and find the matching joint slot.
                    for desc in descriptors.iter_mut() {
                        if desc.entity_path != *structure_name {
                            continue;
                        }

                        let seen_joints = match seen_joints_cache.get(&desc.cell_id) {
                            Some(sj) => sj,
                            None => continue,
                        };

                        let joint_index = match seen_joints.iter().position(|j| j == &joint_val) {
                            Some(idx) => idx,
                            None => continue,
                        };

                        if let Some(jd) = desc.joints.get_mut(joint_index)
                            && jd.driving_param_cell_id.is_none()
                        {
                            jd.driving_param_cell_id = Some(param_cell_id_str.clone());
                            tracing::debug!(
                                target: "reify_gui::engine::param_resolution",
                                structure = %structure_name,
                                joint = %joint_cell_name,
                                param_cell = %param_cell_id_str,
                                "resolved driving param via snapshot+bind AST match"
                            );
                            let param_cell_id = ValueCellId::new(structure_name, &value_cell_name);
                            let param_val = check.values.get_or_undef(&param_cell_id);
                            jd.current_value_si = scalar_to_f64(&param_val);
                            // Promote binding to ParamBound when the joint is drive-able
                            // (binding is any LiteralBound — prismatic/revolute default or
                            // one already refined from a prior literal bind() arg).
                            // FixedNoMotion / CouplingDerived joints are NOT promoted: their
                            // structural binding is authoritative; the flat `driving_param_cell_id`
                            // field may be set anyway for those joints if a user writes
                            // bind(fixed_j, param), but `binding` correctly stays at the
                            // structural default — callers should treat `binding` as authoritative
                            // and `driving_param_cell_id` as best-effort for non-LiteralBound cases.
                            if matches!(jd.binding, JointBinding::LiteralBound { .. }) {
                                jd.binding = JointBinding::ParamBound {
                                    param_cell_id: param_cell_id_str.clone(),
                                    current_value_si: jd.current_value_si,
                                };
                            }
                        }
                    }
                }

                BindValue::Literal(literal_expr) => {
                    // Evaluate the literal expression to an SI value.
                    use reify_ast::ExprKind;
                    let initial_value_si = match &literal_expr.kind {
                        ExprKind::QuantityLiteral { value, unit } => {
                            // Only bare units resolve here; compound unit expressions
                            // (Mul/Div/Pow) get their registry resolver in task γ (3803).
                            match unit {
                                reify_ast::UnitExpr::Unit(unit) => {
                                    // Resolve through `reify_core::unit_symbol_to_si`, the DSL's
                                    // own built-in symbol table (task #5757).
                                    //
                                    // DELIBERATELY NOT the ladder-backed `COMPOSED_UNIT_INDEX` the
                                    // parameter-editor path uses. This site consumes an
                                    // already-parsed `reify_ast::UnitExpr` from .ri SOURCE, so its
                                    // admissible tokens are exactly what the LEXER AND REGISTRY can
                                    // produce; admitting curated DISPLAY labels like `L` or `mm³`,
                                    // which no `.ri` file can even carry here, would let the GUI
                                    // resolve a literal the compiler rejects outright.
                                    //
                                    // That is the only line being held. Everything else this arm
                                    // declines is a GAP, not a boundary: the compiler resolves both
                                    // user-declared registry units (`km`, `ft`, `psi`, …) and the
                                    // SI-derived prefixed units `si_units.rs` generates (`MPa`,
                                    // `kN`, `kJ`, …), and all of them land in the `None` arm below.
                                    // Closing that is task γ (#3803)'s registry work.
                                    match reify_core::unit_symbol_to_si(unit.as_str()) {
                                        Some((factor, _)) => Some(value * factor),
                                        None => {
                                            // Unknown unit: emit debug so the silent value-loss is observable.
                                            tracing::debug!(
                                                target: "reify_gui::engine::literal_bind",
                                                joint = %joint_cell_name,
                                                unit = %unit,
                                                "bind(joint, <quantity>) with a unit symbol outside \
                                                 reify_core::BUILTIN_UNITS (a module-declared unit, or a \
                                                 display-only label); initial_value_si will be None"
                                            );
                                            None
                                        }
                                    }
                                }
                                reify_ast::UnitExpr::Mul(..)
                                | reify_ast::UnitExpr::Div(..)
                                | reify_ast::UnitExpr::Pow(..) => {
                                    tracing::debug!(
                                        target: "reify_gui::engine::literal_bind",
                                        joint = %joint_cell_name,
                                        "bind(joint, <quantity>) with a compound unit expression — \
                                         not yet supported; resolver lands in task γ (3803); \
                                         initial_value_si will be None"
                                    );
                                    None
                                }
                            }
                        }
                        ExprKind::NumberLiteral { value, .. } => Some(*value),
                        _ => None, // complex expression — conservatively no initial value
                    };

                    // Scan descriptors from this structure and find the matching joint slot.
                    for desc in descriptors.iter_mut() {
                        if desc.entity_path != *structure_name {
                            continue;
                        }

                        let seen_joints = match seen_joints_cache.get(&desc.cell_id) {
                            Some(sj) => sj,
                            None => continue,
                        };

                        let joint_index = match seen_joints.iter().position(|j| j == &joint_val) {
                            Some(idx) => idx,
                            None => continue,
                        };

                        if let Some(jd) = desc.joints.get_mut(joint_index) {
                            // Refine the binding to LiteralBound using the joint cell name
                            // (not the index-based default) — first-wins guard.
                            if matches!(jd.binding, JointBinding::LiteralBound { initial_value_si: None, .. }) {
                                jd.binding = JointBinding::LiteralBound {
                                    synth_param_name: format!("__joint_{joint_cell_name}_v"),
                                    initial_value_si,
                                    scrubbable: true,
                                };
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Generic recursive AST walker that invokes `on_call(name, args)` for each
/// `FunctionCall` node reachable through `FunctionCall` args, `BinOp`
/// operands, `UnOp` operands, `Conditional` branches, and `ListLiteral`
/// elements only.  `FunctionCall`s embedded in `MapLiteral`, `SetLiteral`,
/// `Match`, `MemberAccess`, or `IndexAccess` are **not** visited; widen the
/// recursion **here** to fix all callers at once.
///
/// # Motivation
///
/// `collect_snapshot_bind_pairs` and `collect_consumed_mechanism_idents` both
/// need to walk the same subset of `ExprKind` variants and previously each
/// carried an identical ~25-line recursion body.  `walk_function_calls`
/// centralises that skeleton so a third AST-driven feature can register its
/// match logic via the callback without duplicating the traversal again.
fn walk_function_calls(
    expr: &reify_ast::Expr,
    on_call: &mut dyn FnMut(&str, &[reify_ast::Expr]),
) {
    use reify_ast::ExprKind;
    match &expr.kind {
        ExprKind::FunctionCall { name, args, .. } => {
            on_call(name, args);
            // Recurse into all args so nested calls are also visited.
            for arg in args {
                walk_function_calls(arg, on_call);
            }
        }
        ExprKind::BinOp { left, right, .. } => {
            walk_function_calls(left, on_call);
            walk_function_calls(right, on_call);
        }
        ExprKind::UnOp { operand, .. } => {
            walk_function_calls(operand, on_call);
        }
        ExprKind::Conditional {
            condition,
            then_branch,
            else_branch,
        } => {
            walk_function_calls(condition, on_call);
            walk_function_calls(then_branch, on_call);
            walk_function_calls(else_branch, on_call);
        }
        ExprKind::ListLiteral(elems) => {
            for elem in elems {
                walk_function_calls(elem, on_call);
            }
        }
        // Leaf nodes and other compound variants (MapLiteral, SetLiteral,
        // Match, MemberAccess, IndexAccess) are not recursed; widen here if
        // a future feature needs coverage.
        _ => {}
    }
}

/// Recursively search `expr` for `snapshot(mech_expr, [bind(joint, value), …])`.
/// For each `bind(Ident(joint_name), <value>)` where `<value>` is either an
/// `Ident` (Param ref) or a `QuantityLiteral`/`NumberLiteral` (immediate value),
/// append `(joint_name, BindValue)` to `pairs`.
///
/// Delegates all AST recursion to [`walk_function_calls`].
///
/// **Name-shadowing caveat:** matching is by textual function name only.  A
/// user-defined function named `snapshot` or `bind` in the same module would
/// match this search and produce incorrect (false-positive) bind pairs.  The
/// caller (`resolve_driving_params_from_ast`) therefore relies on the assumption
/// that `snapshot`/`bind` are stdlib-only names in well-formed Reify source.
///
/// **Telemetry:** emits a `tracing::debug!` event at target
/// `"reify_gui::engine::snapshot_bind_pairs"` for two anomalous sub-cases:
///
/// * **(a)** `args[1]` is **not** a `ListLiteral` — likely a user-shadowed
///   `snapshot` function or a malformed call.
/// * **(c)** `args[1]` **is** a non-empty `ListLiteral` but none of its
///   elements are valid `bind(Ident, Ident|Literal)` pairs — malformed bind
///   syntax or user-shadowed `bind`.
///
/// Sub-case **(b)** — an empty `ListLiteral` — is **silent**; `snapshot(m, [])`
/// is valid stdlib usage (a snapshot with no bound parameters) and must not be
/// flagged as anomalous.
///
/// Calls with fewer than two arguments (`args.len() < 2`) are also **silent**
/// — they cannot contribute pairs regardless of shadowing, so they are
/// excluded from the anomaly surface intentionally.
fn collect_snapshot_bind_pairs(expr: &reify_ast::Expr, pairs: &mut Vec<(String, BindValue)>) {
    use reify_ast::ExprKind;
    walk_function_calls(expr, &mut |name, args| {
        if name != "snapshot" || args.len() < 2 {
            return;
        }

        if let ExprKind::ListLiteral(elems) = &args[1].kind {
            // Case (b): empty list — valid stdlib usage, stay silent.
            if elems.is_empty() {
                return;
            }

            // Case (c) candidate: non-empty list; extract bind pairs.
            let pairs_before = pairs.len();
            for elem in elems {
                let (bind_name, bind_args) = match &elem.kind {
                    ExprKind::FunctionCall { name, args, .. } => (name, args),
                    _ => continue,
                };
                if bind_name != "bind" || bind_args.len() != 2 {
                    continue;
                }
                let joint_ident = match &bind_args[0].kind {
                    ExprKind::Ident(s) => s.clone(),
                    _ => continue,
                };
                // Match the value side: Ident → Param ref; QuantityLiteral/NumberLiteral
                // → Literal; complex expressions (BinOp, FunctionCall, etc.) → skip.
                let bind_value = match &bind_args[1].kind {
                    ExprKind::Ident(s) => BindValue::Param(s.clone()),
                    ExprKind::QuantityLiteral { .. } | ExprKind::NumberLiteral { .. } => {
                        BindValue::Literal(bind_args[1].clone())
                    }
                    _ => continue, // complex expr — not directly resolvable to Param or Literal
                };
                pairs.push((joint_ident, bind_value));
            }

            // Case (c): non-empty list but no resolvable bind(Ident, Ident|Literal) pairs
            // survived (malformed bind syntax or user-shadowed bind).
            if pairs.len() == pairs_before {
                tracing::debug!(
                    target: "reify_gui::engine::snapshot_bind_pairs",
                    arg_count = args.len(),
                    "snapshot() bind list contained no resolvable bind(Ident, Ident|Literal) pairs \
                     (malformed bind syntax or user-shadowed bind)"
                );
            }
        } else {
            // Case (a): args[1] is not a ListLiteral at all.
            tracing::debug!(
                target: "reify_gui::engine::snapshot_bind_pairs",
                arg_count = args.len(),
                "snapshot() second arg is not a ListLiteral \
                 (potential user-shadowed snapshot or malformed call)"
            );
        }
    });
}

// ---- terminal-mechanism filter helpers ----------------------------------------

/// Return the set of mechanism member names consumed as `mech_in` (first
/// argument) by any `body()` call within the named structure.
///
/// Walks every `MemberDecl::Let` expression in the first structure whose name
/// matches `structure_name` and delegates the per-expression AST traversal to
/// [`walk_function_calls`].
///
/// The returned set is used by `get_mechanism_descriptors` to skip intermediate
/// mechanism cells — only the terminal cell (not consumed by any `body()` call)
/// survives into the returned `Vec<MechanismDescriptor>`.
///
/// **Design narrowing:** only `body()` consumption is collected; `snapshot()`
/// consumption is intentionally excluded.  See design decision:
/// "Terminal-mechanism filter narrows the suggestion text to body() consumption
/// only."
fn collect_consumed_mechanism_idents(
    parsed: &reify_ast::ParsedModule,
    structure_name: &str,
) -> HashSet<String> {
    use reify_ast::ExprKind;
    let mut consumed = HashSet::new();

    for decl in &parsed.declarations {
        let structure = match decl {
            reify_ast::Declaration::Structure(s) if s.name == structure_name => s,
            _ => continue,
        };

        for member in &structure.members {
            let expr = match member {
                reify_ast::MemberDecl::Let(l) => &l.value,
                _ => continue,
            };
            walk_function_calls(expr, &mut |name, args| {
                if name == "body"
                    && let Some(first_arg) = args.first()
                    && let ExprKind::Ident(s) = &first_arg.kind
                {
                    consumed.insert(s.clone());
                }
            });
        }
        // Stop at the first matching structure; structure names are unique within
        // a module — enforced by
        // reify_compiler::compile_builder::pre_pass::collect_decl_refs (which
        // calls record_or_report_duplicate to emit a hard Diagnostic::error).
        break;
    }

    consumed
}

// ---- build_preview_gui_state -------------------------------------------------

/// Build a `GuiState` from a preview evaluation result.
///
/// Used by `get_def_preview` to convert a `CheckResult` into the same
/// `GuiState` format returned by `build_gui_state`, but with:
/// - **No meshes** — geometry tessellation is skipped (no kernel available).
/// - **No files** — file list is not meaningful for a single-def preview.
///
/// Delegates to `build_values` and `build_constraints` — the same helpers used
/// by `build_gui_state` — so both paths stay in sync automatically.
fn build_preview_gui_state(
    compiled: &reify_compiler::CompiledModule,
    check: &CheckResult,
) -> GuiState {
    // Pass `None` for the engine: the preview engine is a throwaway instance
    // that is not retained beyond the `get_def_preview` call, and freshness
    // badges are not meaningful for single-definition previews evaluated in
    // isolation.  All cells default to `"final"` on the preview surface
    // (see `build_values` doc comment for the full rationale).
    GuiState {
        meshes: Vec::new(),
        values: build_values(compiled, check, None),
        constraints: build_constraints(compiled, check),
        files: Vec::new(),
        tessellation_diagnostics: Vec::new(),
        compile_diagnostics: Vec::new(),
        tensegrity_wires: Vec::new(),
        tensegrity_surfaces: Vec::new(),
        // Single-definition previews are evaluated in isolation; no edit
        // measurement is meaningful here.
        demand_prune_measurement: None,
        // Preview path: no tessellation result, no display routing, no FEA solve.
        display_panes: Vec::new(),
        display_appearance: Vec::new(),
        fea_diagnostics: Vec::new(),
        fea_convergence: None,
    }
}

/// Build an `EntityTreeNode` for a topology template.
///
/// `entity_path` is the dot-separated path used as the root of this node's
/// children (e.g. `"Bracket"` → children are `"Bracket.width"`, etc.).
///
/// `aux_ancestor` is `true` when any containing sub-component on the path from
/// the root to this template was declared `aux`. This mirrors the aux-inheritance
/// rule in the surfacing walk — shared contract anchor:
/// `geometry_ops::surface_subtree` / `geometry_ops::realization_is_aux`
/// (rule: `!(aux_ancestor || realization_is_aux(realization))`). Pass `false` for
/// top-level templates (`get_entity_tree`); pass `aux_ancestor || sub.is_aux`
/// when recursing into sub-components.
///
/// When a sub-component's child template has `is_recursive = true` (set by the
/// compiler's Tarjan SCC pass), this function emits an empty `children` vec for
/// that sub node rather than recursing — preventing infinite recursion for
/// self-referential and mutually-recursive structure definitions.
///
/// # Freshness
///
/// When `engine` is `Some`, each value cell's freshness is read via
/// `Engine::freshness(&NodeId::Value(cell.id))` and each realization's
/// freshness via `Engine::freshness(&NodeId::Realization(real.id))`.
/// Both delegate to `CacheStore::freshness` which returns `Freshness::Final`
/// for unknown nodes, so the default is always safe (arch §7.1).
///
/// When `engine` is `None` (test helpers that call `build_template_node`
/// directly without a live session), all nodes default to `"final"`.
/// Tests that specifically exercise freshness pass the engine explicitly.
///
/// # Preconditions
/// Caller must ensure `compiled.templates` has no duplicate names — the compiler
/// guarantees this for well-formed modules. `get_entity_tree` performs a runtime
/// uniqueness check (O(N)) before iterating templates, emitting a `tracing::warn!`
/// in release builds and panicking via `debug_assert!` in debug builds.
/// Collect the names of realizations that are consumed as an operand by some
/// other realization in the SAME template (#5195).
///
/// A consumed realization is intermediate construction geometry — `let body`
/// and `let holes` in `param geometry = difference(body, holes)` — and is
/// hidden by default so the viewport shows only the finished part.
///
/// # Why the VALUE-CELL layer, not `RealizationDecl.operations` (esc-5195-1)
/// The obvious-looking source — scanning each realization's `operations` for a
/// bare `GeomRef::Sub(name)` operand — is NOT viable, and this was MEASURED
/// rather than assumed. The compiler lowers a sibling-let reference to
/// `GeomRef::Sub` only on the Modify/Transform/Pattern argument path (the
/// #4668 sibling pre-check, `reify-compiler/src/geometry.rs:1374-1385`). The
/// Boolean argument path (`resolve_boolean_arg`,
/// `reify-compiler/src/geometry_boolean.rs:28-107`) instead INLINES the
/// operand's initializer as extra ops and refers to it by `GeomRef::Step(n)`,
/// so the sibling NAME is absent from the consuming realization entirely —
/// Booleans early-return at `geometry.rs:1290-1308`, above that pre-check.
/// For `param geometry : Solid = difference(body, holes)` an operations-scan
/// yields only `{hole}` (the Pattern operand); `body` and `holes` are
/// invisible to it. That inlining is PINNED by an existing test
/// (`reify-compiler/tests/harness_langcore/let_scope_tests.rs:552-603`), so it
/// is deliberate behaviour, not an oversight to patch here.
/// `reify_eval::deps::extract_realization_edges` gates every arm on
/// `GeomRef::Sub` and so shares this blind spot — it is deliberately NOT the
/// contract anchor for this rule.
///
/// `template.value_cells` does carry the names: since #4954 every geometry
/// binding emits a value cell alongside its realization, and that cell's
/// `default_expr` holds the un-inlined `ValueRef(ValueCellId)` operands.
///
/// # Only GEOMETRY-VALUED cells count as consumers
/// This filter is load-bearing, not a tidiness rule. A `: Rigid` structure
/// auto-derives `mass`/`centroid`/`moment_of_inertia` lets that all reference
/// `geometry` — but they are `Scalar`/`Point3`/`Tensor`-typed, not
/// geometry-valued. Counting them would mark the TERMINAL realization as
/// consumed and hide the finished part: the exact inverse of this feature.
///
/// "Geometry-valued" is [`is_geometry_valued`], NOT an exact `Type::Geometry`
/// match: a binding that COLLECTS siblings (`let ribs = [rib_a, rib_b]`, typed
/// `List<Geometry>`) consumes them just as surely as `union(rib_a, rib_b)`
/// does. Under an exact-variant match such a realization stayed classified as
/// a product and rendered as a stray body beside the finished part.
///
/// # Traversal
/// Recursion over `CompiledExpr` is delegated to the existing exhaustive
/// [`reify_ir::CompiledExpr::collect_value_refs`], the repo's
/// dependency-tracking walk — it covers every nesting variant (`FunctionCall`
/// args, `BinOp`, `IndexAccess`, `StructureInstanceCtor`, lambda `captures`,
/// …) and treats `CrossSubGeometryRef` as a `ValueRef` leaf. Reusing it means
/// `difference(body, translate(holes, ...))` is handled for free, and this
/// notion of "consumption" cannot drift from the dependency graph's.
///
/// # Cost
/// O(geometry_cells × expr_size) plus one O(value_cells) index build; the
/// per-reference membership test is O(1). `build_template_node` recurses per
/// sub-component INSTANCE, so an assembly holding N instances of the same
/// template repeats this scan N times. That is deliberate for now — the scan
/// is linear and template expressions are small — but if wide assemblies of
/// repeated instances become common, memoize the result per template name in
/// the caller rather than making the scan itself cleverer.
fn collect_consumed_sibling_names(template: &reify_compiler::TopologyTemplate) -> HashSet<&str> {
    // Index over this template's own value cells (member → owning entity),
    // built ONCE. The walk below yields OWNED `ValueCellId`s, so a hit here
    // does triple duty at O(1): it confirms the referent really is a value cell
    // of this template, it re-borrows the member name with `template`'s
    // lifetime (via `get_key_value`), and it checks the OWNING ENTITY too — the
    // same pair the sibling guard tests, so this existence check can never be
    // weaker than that guard. Value-cell members are unique within a template,
    // so keying on the member alone loses nothing.
    //
    // Keyed on `str` rather than a `(&str, &str)` tuple deliberately: a tuple
    // key would force the lookup's short-lived `id.member.as_str()` borrow to
    // unify with the index's `template` lifetime (E0515).
    let cells_by_member: HashMap<&str, &str> = template
        .value_cells
        .iter()
        .map(|c| (c.id.member.as_str(), c.id.entity.as_str()))
        .collect();

    let mut consumed: HashSet<&str> = HashSet::new();
    for cell in &template.value_cells {
        // Non-geometry consumers (the `: Rigid` mass/centroid/moment_of_inertia
        // lets) must NOT count — see the doc-comment above.
        if !is_geometry_valued(&cell.cell_type) {
            continue;
        }
        let Some(expr) = &cell.default_expr else {
            continue;
        };
        for id in expr.collect_value_refs() {
            // Same-template siblings only. Both `id.entity` and
            // `cell.id.entity` are TEMPLATE-scoped (not instance paths), so a
            // sibling reference shares the consuming cell's entity string;
            // anything else is a cross-sub/cross-entity ref naming no member
            // of `template.realizations`. A cell is not a consumer of itself.
            if id.entity != cell.id.entity || id.member == cell.id.member {
                continue;
            }
            if let Some((&member, &owner)) = cells_by_member.get_key_value(id.member.as_str())
                && owner == id.entity
            {
                consumed.insert(member);
            }
        }
    }
    consumed
}

/// True when a value of this type is (or contains) realized geometry (#5195).
///
/// Used to decide whether a value cell counts as a CONSUMER of its sibling
/// realizations. Container variants recurse on the element type because
/// geometry-ness rides on the element, not the container: `let ribs = [rib_a,
/// rib_b]` is typed `List<Geometry>` and consumes both ribs.
///
/// Deliberately NOT geometry-valued: `Scalar`/`Point`/`Tensor` (the `: Rigid`
/// auto-derived `mass`/`centroid`/`moment_of_inertia` lets, which reference
/// `geometry` without consuming it) and `Type::Feature` (a structured identity
/// token, not a realized-geometry handle — see `reify_core::ty::Type::Feature`).
fn is_geometry_valued(ty: &reify_core::ty::Type) -> bool {
    use reify_core::ty::Type;
    match ty {
        Type::Geometry => true,
        Type::List(inner)
        | Type::Set(inner)
        | Type::Option(inner)
        | Type::Keyed(inner)
        | Type::Map(_, inner) => is_geometry_valued(inner),
        _ => false,
    }
}

pub(crate) fn build_template_node(
    template: &reify_compiler::TopologyTemplate,
    entity_path: &str,
    compiled: &reify_compiler::CompiledModule,
    engine: Option<&Engine>,
    aux_ancestor: bool,
) -> EntityTreeNode {
    let kind = template.entity_kind.as_label();

    let mut children = Vec::new();

    // Shared by BOTH the value-cell loop and the realization loop below, so the
    // two sibling nodes a geometry binding emits (#4954) agree on
    // `trait_geometry` (#5195). Hoisted out of the value-cell loop, where it
    // used to be recomputed per cell.
    //
    // KNOWN LIMITATION (pre-existing, shared by both call sites, out of scope
    // for #5195): `trait_bounds` holds DECLARED trait names only, so this fires
    // for `structure def X : Physical` but NOT for `: Rigid` — even though
    // `Rigid : Physical` refines it (stdlib/structural_physical.ri:76). A
    // correct check would resolve the refinement chain
    // (`reify_eval::conforms_to_trait`) and needs the merged module + prelude
    // trait_defs threaded in here; that is a separable follow-up. The
    // consumed-intermediate observable does NOT depend on this flag: the
    // terminal `geometry` realization is consumed by nothing, so it stays
    // `default_visible == true` and renders either way.
    let parent_has_physical = template.trait_bounds.iter().any(|b| b.contains("Physical"));

    // Value cells: param, let, auto
    for cell in &template.value_cells {
        let cell_kind = cell_kind_tree_str(cell.kind);
        let member = &cell.id.member;
        let cell_path = format!("{}.{}", entity_path, member);
        let is_geometry_member = member == "geometry";
        // Use entity_path (the instance path, e.g. "Parent.rib") rather than
        // cell.id.entity (the template name, e.g. "Child") when constructing
        // the NodeId for the freshness lookup.  Sub-component cells are keyed
        // in the engine cache by their instance-scoped path
        // (`ValueCellId { entity: "Parent.rib", member: "height" }`), which is
        // what elaborate_child_instance writes via scoped_entity (unfold.rs:326).
        // Using cell.id.entity would always return Freshness::Final (the
        // default for unknown nodes) for any sub-component cell.
        let freshness = engine
            .map(|e| {
                let node = NodeId::Value(ValueCellId::new(entity_path, &cell.id.member));
                String::from(format_freshness(&e.freshness(&node)))
            })
            .unwrap_or_else(|| String::from("final"));
        children.push(EntityTreeNode {
            entity_path: cell_path,
            kind: cell_kind.to_string(),
            type_name: Some(cell.cell_type.to_string()),
            display_name: None,
            has_mesh: false,
            trait_geometry: is_geometry_member && parent_has_physical,
            children: vec![],
            freshness,
            default_visible: true,
        });
    }

    // Realizations (geometry-producing bindings: Solid-typed lets/params).
    //
    // Since #4954 a geometry binding emits BOTH a value cell (above, the
    // scalar/typed view: `kind: "let"`, `has_mesh: false`) and a realization
    // node here (`kind: "realization"`, `has_mesh: true`). Meshes key on the
    // realization node, so this loop emits exactly the entries the user wants
    // to toggle visibility on (`let body`, `let hole`, `param geometry: Solid`,
    // …); the value-cell siblings carry no mesh.
    //
    // `entity_path` is the mesh key form (`Entity#realization[N]`) so it
    // matches `engineStore.meshes` and `viewStateStore` directly. The
    // user-friendly binding name is carried in `display_name`. Realizations
    // without a name (test-helper-only code path — see `RealizationDecl.name`
    // doc) fall back to deriving one from the path.

    // Sibling realizations consumed downstream by another realization in this
    // same template are INTERMEDIATE construction geometry (`let body` feeding
    // `param geometry = difference(body, holes)`), so they are hidden by
    // default — the viewport shows the finished part, and the outline toggle
    // reveals the construction steps (#5195).
    let consumed = collect_consumed_sibling_names(template);

    // FLOOR on the consumed rule (#5195). The rule classifies by "some
    // geometry-valued sibling references this name", which a DERIVATION of the
    // finished part satisfies just as well as a step toward it — `let clearance
    // = translate(geometry, ...)` or `let envelope = hull(geometry)` kept for a
    // clearance check both put `geometry` in `consumed`. Without a floor that
    // hides the part and leaves only the helper on screen: a blank-looking
    // viewport with no diagnostic. Two guards, applied in order:
    //
    //  1. The trait terminal `geometry` is NEVER consumed-hidden. It is the
    //     structure's published product (`Physical::geometry`), so a sibling
    //     referencing it is by construction a derivation OF the part.
    //  2. If the rule would STILL leave this template with zero visible
    //     realizations, drop it wholesale here. Aux hiding is deliberately NOT
    //     floored — an all-aux template legitimately shows nothing.
    //
    // Unnamed realizations (test-helper-only path) are never consumed-hidden:
    // `consumed` holds names, so there is nothing to match them against.
    let is_consumed_intermediate = |name: Option<&str>| -> bool {
        match name {
            Some("geometry") | None => false,
            Some(n) => consumed.contains(n),
        }
    };
    let apply_consumed_rule = template
        .realizations
        .iter()
        .any(|r| !(aux_ancestor || r.is_aux || is_consumed_intermediate(r.name.as_deref())));

    for real in &template.realizations {
        let real_path = format!("{}#realization[{}]", entity_path, real.id.index);
        let display_name = real.name.clone();
        let freshness = engine
            .map(|e| {
                let node = NodeId::Realization(real.id.clone());
                String::from(format_freshness(&e.freshness(&node)))
            })
            .unwrap_or_else(|| String::from("final"));
        children.push(EntityTreeNode {
            entity_path: real_path,
            kind: "realization".to_string(),
            type_name: None,
            display_name,
            has_mesh: true,
            // Mirrors the value-cell heuristic above so the two sibling nodes a
            // geometry binding emits (#4954) agree — see `parent_has_physical`
            // for the shared `: Rigid` limitation (#5195).
            trait_geometry: real.name.as_deref() == Some("geometry") && parent_has_physical,
            children: vec![],
            freshness,
            // Extends the surfacing-walk rule — shared contract anchor:
            // `geometry_ops::surface_subtree` / `geometry_ops::realization_is_aux`
            // (rule: `!(aux_ancestor || realization_is_aux(realization))`) — with
            // the consumed-intermediate rule (#5195). aux_ancestor is inherited
            // from any containing `aux sub` up the tree; `is_consumed_intermediate`
            // means some geometry-valued sibling cell in this template takes this
            // realization as an operand, subject to the floor documented above.
            default_visible: !(aux_ancestor
                || real.is_aux
                || (apply_consumed_rule && is_consumed_intermediate(real.name.as_deref()))),
        });
    }

    // Sub-components
    for sub in &template.sub_components {
        let sub_path = format!("{}.{}", entity_path, sub.name);
        let type_name = if sub.is_collection {
            format!("List<{}>", sub.structure_name)
        } else {
            sub.structure_name.clone()
        };
        // Try to find the child template for recursive tree building
        let sub_children = if let Some(child_template) = compiled
            .templates
            .iter()
            .find(|t| t.name == sub.structure_name)
        {
            // Guard against infinite recursion: if the child template is part of
            // a recursive cycle (detected by the compiler's Tarjan SCC pass and
            // stored in `is_recursive`), emit an empty children vec instead of
            // recursing.  This covers self-reference (A → A), mutual recursion
            // (A → B → A), and longer cycles — all correctly tagged by the
            // compiler.
            if child_template.is_recursive {
                vec![]
            } else {
                // Thread aux_ancestor: if this sub is aux OR an ancestor was aux,
                // all descendants inherit default_visible = false.
                build_template_node(child_template, &sub_path, compiled, engine, aux_ancestor || sub.is_aux).children
            }
        } else {
            vec![]
        };
        // Sub-component container nodes aggregate their children; freshness
        // roll-up across children is out of scope for this task.  We emit
        // the sentinel `"aggregate"` rather than `"final"` to make it clear
        // on the wire that this node has no *individual* freshness — consumers
        // should inspect the children array directly.  The frontend suppresses
        // the badge for `"aggregate"` the same as for `"final"` (no badge
        // until a future task implements parent-level roll-up).
        children.push(EntityTreeNode {
            entity_path: sub_path,
            kind: "sub".to_string(),
            type_name: Some(type_name),
            display_name: None,
            has_mesh: false,
            trait_geometry: false,
            children: sub_children,
            freshness: "aggregate".to_string(),
            default_visible: true,
        });
    }

    // Ports
    for port in &template.ports {
        let port_path = format!("{}.{}", entity_path, port.name);
        children.push(EntityTreeNode {
            entity_path: port_path,
            kind: "port".to_string(),
            type_name: Some(port.type_name.clone()),
            display_name: None,
            has_mesh: false,
            trait_geometry: false,
            children: vec![],
            freshness: "final".to_string(),
            default_visible: true,
        });
    }

    EntityTreeNode {
        entity_path: entity_path.to_string(),
        kind: kind.to_string(),
        type_name: None,
        display_name: None,
        has_mesh: !template.realizations.is_empty(),
        trait_geometry: false,
        children,
        freshness: "final".to_string(),
        default_visible: true,
    }
}

/// Test helpers — compiled out of production binaries.
#[cfg(test)]
impl EngineSession {
    /// Return a reference to the `CoreState` for structural inspection in tests.
    ///
    /// Used by the structural lock-in test to verify that `CoreState` exposes
    /// the expected read accessors after the refactor.
    pub(crate) fn core_state_for_test(&self) -> &CoreState {
        &self.core
    }

    /// Inject a diagnostic directly into the compiled module's diagnostics vec,
    /// enabling tests to exercise the `diag.labels.first() == None` fallback path
    /// without needing the compiler to produce such a diagnostic.
    ///
    /// # Panics
    /// Panics if no module is currently loaded (`self.compiled` is `None`).
    pub(crate) fn inject_diagnostic_for_test(&mut self, diag: reify_core::Diagnostic) {
        self.core.inject_diagnostic(diag);
    }

    /// Thin wrapper around `resolve_source` for use in tests.
    ///
    /// Exposes the private method so tests can call it directly and verify
    /// that `None` is returned when no module is loaded or when the invariant
    /// is deliberately broken via `break_module_name_for_test` or
    /// `break_source_map_for_test`.
    pub(crate) fn resolve_source_for_test(&self) -> Option<(&str, &str)> {
        self.resolve_source()
    }

    /// Deliberately break the compiled/module_name/source_map invariant by
    /// clearing `module_name` while leaving `compiled` intact.
    ///
    /// After this call, `resolve_source` returns `None` (via the `?` on
    /// `module_name.as_deref()`).  Callers that rely on `resolve_source` —
    /// `get_source_location` and `get_diagnostics` — degrade gracefully rather
    /// than panicking (matching the struct-level invariant doc).  In debug
    /// builds, `get_diagnostics` additionally trips a `debug_assert!` when the
    /// diagnostics vec is non-empty.
    ///
    /// Tests exercising these paths:
    /// - `resolve_source_returns_none_when_module_name_broken` (graceful `None`)
    /// - `get_source_location_returns_none_when_module_name_broken` (graceful `None`)
    /// - `get_diagnostics_debug_asserts_when_module_name_broken` (debug-build loud path)
    pub(crate) fn break_module_name_for_test(&mut self) {
        self.core.break_module_name();
    }

    /// Deliberately break the compiled/module_name/source_map invariant by
    /// clearing `source_map` while leaving `compiled` and `module_name` intact.
    ///
    /// After this call, `resolve_source` returns `None` (via the `?` on
    /// `source_map.get_key_value(&key)`).  Callers that rely on `resolve_source`
    /// — `get_source_location` and `get_diagnostics` — degrade gracefully rather
    /// than panicking (matching the struct-level invariant doc).  In debug
    /// builds, `get_diagnostics` additionally trips a `debug_assert!` when the
    /// diagnostics vec is non-empty.
    ///
    /// Tests exercising these paths:
    /// - `resolve_source_returns_none_when_source_map_broken` (graceful `None`)
    /// - `resolve_source_fallback_when_source_map_missing` (graceful `None`)
    /// - `get_diagnostics_debug_asserts_when_source_map_broken` (debug-build loud path)
    pub(crate) fn break_source_map_for_test(&mut self) {
        self.core.break_source_map();
    }

    /// Return a reference to the cached `ParsedModule`, or `None` if no module
    /// has been loaded yet.
    ///
    /// Intended only for tests that need to inspect cache state without widening
    /// the production API.
    pub(crate) fn parsed_cache_for_test(&self) -> Option<&reify_ast::ParsedModule> {
        self.parsed_cache.as_ref()
    }

    /// Return a slice of the cached line-offset table, or `None` if no module
    /// has been loaded yet.
    ///
    /// Each element is the byte offset of a `\n` in the current source text.
    /// Intended only for tests that need to inspect cache state.
    pub(crate) fn line_offsets_cache_for_test(&self) -> Option<&[usize]> {
        self.line_offsets_cache.as_deref()
    }

    /// Replace the cached `ParsedModule` with `parsed`, for testing purposes.
    ///
    /// Used by `get_containing_definition_reads_from_parsed_cache` to inject a
    /// stripped `ParsedModule` (with `declarations: Vec::new()`) and verify that
    /// `get_containing_definition` reads from the cache rather than re-parsing
    /// the source text.
    pub(crate) fn override_parsed_cache_for_test(&mut self, parsed: reify_ast::ParsedModule) {
        self.parsed_cache = Some(parsed);
    }

    /// Replace the cached line-offset table with `offsets`, for testing purposes.
    ///
    /// Used by `get_containing_definition_reads_from_line_offsets_cache` to inject
    /// a deliberately wrong newline table and verify that `get_containing_definition`
    /// uses the cached table rather than recomputing it from the source text.
    pub(crate) fn override_line_offsets_cache_for_test(&mut self, offsets: Vec<usize>) {
        self.line_offsets_cache = Some(offsets);
    }

    /// Return a reference to the cached consumed-idents map, or `None` if the
    /// cache has not yet been populated (fresh session or just after `commit_state`).
    ///
    /// Intended only for tests that need to inspect cache state without widening
    /// the production API.  Mirrors the style of `parsed_cache_for_test`.
    pub(crate) fn consumed_idents_cache_for_test(
        &self,
    ) -> Option<&HashMap<String, HashSet<String>>> {
        self.consumed_idents_cache.as_ref()
    }

    /// Replace the consumed-idents cache with `cache`, for testing purposes.
    ///
    /// Used by `get_mechanism_descriptors_reads_from_consumed_idents_cache` to
    /// inject a deliberately-empty consumed-idents map for "Kinematic" and verify
    /// that the descriptor build consults the cache (terminal-mechanism filter sees
    /// zero consumed → emits all mechanism cells) rather than re-walking the AST.
    /// Mirrors the style of `override_parsed_cache_for_test`.
    pub(crate) fn override_consumed_idents_cache_for_test(
        &mut self,
        cache: HashMap<String, HashSet<String>>,
    ) {
        self.consumed_idents_cache = Some(cache);
    }

    /// Return the stored compile failure (if any).
    ///
    /// `None` when no failure is stored (after construction or any successful
    /// `commit_state` cycle).  `Some(_)` after a failed parse/compile in
    /// `load_from_source`, `update_source`, or `load_file`.  The `kind` discriminant
    /// distinguishes cold-start from live-edit failures.
    ///
    /// Used by tests that need to inspect field state without calling `build_gui_state`.
    pub(crate) fn compile_failure_for_test(&self) -> Option<&CompileFailure> {
        self.compile_failure.as_ref()
    }

    /// Directly inject a `CompiledModule` as the session's current compiled state,
    /// bypassing parse / compile / check.
    ///
    /// Allows tests to exercise functions that operate on `self.compiled` with
    /// synthetic or intentionally malformed modules (e.g. duplicate template names)
    /// that the normal compiler pipeline would never produce.
    ///
    /// Note: `module_name`, `source_map`, and `last_check` are NOT updated, so the
    /// session's invariant is intentionally broken.  Functions that rely on those
    /// fields (e.g. `get_diagnostics`, `resolve_source`) degrade gracefully.
    pub(crate) fn inject_compiled_for_test(&mut self, compiled: CompiledModule) {
        self.core.inject_compiled(compiled);
    }

    /// Load a pre-compiled module into the session, bypassing source parsing and
    /// compilation.
    ///
    /// Source-bypassing analog of [`EngineSession::load_from_source`]: runs
    /// `check_with_solve_slot`, commits the core fields via
    /// [`CoreState::commit_state`], then immediately clears `module_name` via
    /// [`CoreState::break_module_name`] so that `resolve_source()` returns `None`.
    /// Emits the four event families and returns [`GuiState`].
    ///
    /// Distinct from [`EngineSession::inject_compiled_for_test`], which sets
    /// `compiled` ONLY.  Use `load_from_compiled` when tests need `compiled` and
    /// `last_check` both populated (e.g. to drive `get_entity_tree`).
    ///
    /// **Parse-dependent APIs degrade gracefully to `None`/`[]` on an injected
    /// session.**  Because `break_module_name` is called after `commit_state`,
    /// `resolve_source()` returns `None`, so [`get_containing_definition`],
    /// [`get_entity_at_source_location`], and [`get_source_location`] all
    /// short-circuit at their `resolve_source()?` guard — before the
    /// `debug_assert!(parsed_cache.is_some() && line_offsets_cache.is_some(), …)`
    /// in those methods.  The production invariant "resolve_source succeeds ⟹
    /// parsed_cache and line_offsets_cache are `Some`" remains vacuously true.
    ///
    /// `get_diagnostics` also degrades safely because it early-exits on empty
    /// diagnostics before calling `resolve_source`.  **Injected modules must
    /// therefore carry empty diagnostics** (`compiled.diagnostics.is_empty()`);
    /// a non-empty diagnostics list causes `get_diagnostics` to hit its own
    /// `debug_assert` (the "resolve_source returned None with non-empty
    /// diagnostics" path exercised by
    /// `get_diagnostics_debug_asserts_when_module_name_broken`).
    ///
    /// `build_gui_state` explicitly tolerates `parsed_cache = None` for
    /// test-injected sessions (the `parsed_cache` check in `build_gui_state` is
    /// warn-only), so [`GuiState`] is still returned correctly.
    pub(crate) fn load_from_compiled(
        &mut self,
        compiled: CompiledModule,
        module_name: &str,
    ) -> Result<GuiState, String> {
        let check_result = self.check_with_solve_slot(&compiled);
        // Commit the five core fields via CoreState::commit_state.
        // Empty source: there is no on-disk text; source_map gets
        // module_key(module_name) -> "".
        self.core.commit_state(
            compiled,
            check_result,
            module_name,
            "",
            FilePathUpdate::Preserve,
        );
        // Immediately break module_name so resolve_source() returns None.
        // This ensures get_containing_definition / get_entity_at_source_location /
        // get_source_location short-circuit at resolve_source()? BEFORE their
        // debug_assert on the (None) parse caches, preserving the invariant
        // "resolve_source succeeds => parsed_cache & line_offsets_cache are Some"
        // vacuously.  source_map is left intact so build_gui_state's `files`
        // output is byte-identical to before.
        self.core.break_module_name();
        // Replicate the cache resets from the commit_state cache-reset block.
        // parsed_cache and line_offsets_cache stay None (no parse available).
        self.def_preview_cache.clear();
        self.parsed_cache = None;
        self.line_offsets_cache = None;
        self.consumed_idents_cache = None;
        self.reserved_param_warned.clear();
        self.compile_failure = None;
        self.last_reload_error = None;
        // Emit ordering mirrors the emit-ordering block in load_from_source.
        self.post_engine_call_telemetry();
        self.build_gui_state()
    }

    /// Register a cell to panic during the next eval cycle.
    ///
    /// Thin wrapper around [`reify_eval::Engine::set_panic_on_eval`] for
    /// integration tests that need to drive a specific value cell to
    /// `Freshness::Failed` without bypassing the `EngineSession` wrapper.
    ///
    /// Only callable when the `test-instrumentation` feature is active on
    /// `reify-eval` (enabled unconditionally for `gui/src-tauri` dev-deps
    /// per task #2337 pre-1).  Call `recheck_for_test` after this to
    /// re-run the evaluation with the forced panic in effect.
    pub(crate) fn set_panic_on_eval_for_test(&mut self, cell: reify_core::ValueCellId) {
        self.core.engine_mut().set_panic_on_eval(cell);
    }

    /// Re-run `engine.check` on the current compiled module and update `last_check`.
    ///
    /// Used by tests that inject test-instrumentation state (e.g. via
    /// `set_panic_on_eval_for_test`) and then need to trigger a fresh
    /// evaluation so the injected state takes effect before calling
    /// `build_gui_state`.
    ///
    /// Clones `self.compiled` to avoid the borrow conflict between
    /// `self.engine` (needs `&mut`) and `self.compiled` (provides
    /// `&CompiledModule` for the check call) — the clone cost is acceptable
    /// in test code.  No-op when no module is loaded.
    pub(crate) fn recheck_for_test(&mut self) {
        self.core.recheck();
    }

    /// Trigger the full build path (check + geometry ops) without writing any
    /// output file, so that realization `NodeId`s are marked `Freshness::Failed`
    /// in the engine cache when a kernel error occurs.
    ///
    /// `build_gui_state` uses `tessellate_snapshot`, which does NOT propagate
    /// kernel errors into `Freshness::Failed` (arch §9.1 / engine_build.rs
    /// comment "Tessellate paths do not propagate kernel errors into
    /// `Freshness::Failed` today — build path only").  This helper provides
    /// the build path so integration tests can drive a realization to Failed
    /// and then verify that `get_entity_tree()` surfaces that freshness.
    ///
    /// The `ExportFormat::Step` format is arbitrary — only the cache side-effect
    /// (marking `NodeId::Realization(...)` as `Freshness::Failed`) matters.
    /// The `BuildResult` is intentionally discarded; call `get_entity_tree()`
    /// or `engine.freshness(node)` after this to inspect the cache.
    ///
    /// No-op when no module is loaded.
    pub(crate) fn build_for_freshness_test(&mut self) {
        if let Some(compiled) = self.core.compiled().cloned() {
            // Discards the BuildResult — callers read freshness via get_entity_tree().
            // compiled() borrow is released after cloned(), so engine_mut() is safe.
            let _ = self.core.engine_mut().build(&compiled, ExportFormat::Step);
        }
    }

    /// Directly mark a value cell as `Freshness::Failed` in the engine cache.
    ///
    /// Use this when you need to inject a Failed state for nodes that cannot be
    /// forced to fail via `set_panic_on_eval` — specifically, sub-component param
    /// and let cells that are evaluated inside `elaborate_child_lets_only` /
    /// `elaborate_child_params_only` (unfold.rs), which bypass the
    /// `panic_on_eval_cells` check in `evaluate_let_bindings` (engine_eval.rs).
    ///
    /// The cell must already exist in the engine cache (i.e. `load_from_source`
    /// or an equivalent evaluation must have run first); `mark_failed` returns
    /// `false` for unknown nodes and this method does nothing in that case.
    ///
    /// Requires the `test-instrumentation` feature on `reify-eval` (enabled for
    /// `gui/src-tauri` dev-deps unconditionally per task #2337 pre-1).
    pub(crate) fn mark_value_cell_failed_for_test(
        &mut self,
        cell: reify_core::ValueCellId,
        error_msg: &str,
    ) {
        let node = reify_eval::cache::NodeId::Value(cell);
        self.core
            .engine_mut()
            .cache_store_mut()
            .mark_failed(&node, reify_ir::ErrorRef::new(error_msg));
    }
}

/// Parse a "Entity.member" string into a ValueCellId.
fn parse_cell_id(s: &str) -> Result<ValueCellId, String> {
    let parts: Vec<&str> = s.splitn(2, '.').collect();
    if parts.len() != 2 {
        return Err(format!(
            "Invalid cell ID '{}': expected 'Entity.member' format",
            s
        ));
    }
    Ok(ValueCellId::new(parts[0], parts[1]))
}

/// Parse a realization mesh key of the form `Entity#realization[N]` — the
/// `RealizationNodeId` Display form (reify-core/identity.rs) — into a
/// `RealizationNodeId`. Returns `None` for malformed keys so `sync_observed_demand`
/// can skip-and-warn rather than panic on stale/foreign frontend input.
fn parse_realization_key(key: &str) -> Option<RealizationNodeId> {
    let (entity, rest) = key.split_once("#realization[")?;
    let index: u32 = rest.strip_suffix(']')?.parse().ok()?;
    if entity.is_empty() {
        return None;
    }
    Some(RealizationNodeId::new(entity, index))
}

/// Parse a constraint-panel id of the form `Entity#constraint[N]` — the
/// `ConstraintNodeId` Display form — into a `ConstraintNodeId`. Returns `None`
/// for malformed ids (callers skip-and-warn).
fn parse_constraint_key(key: &str) -> Option<ConstraintNodeId> {
    let (entity, rest) = key.split_once("#constraint[")?;
    let index: u32 = rest.strip_suffix(']')?.parse().ok()?;
    if entity.is_empty() {
        return None;
    }
    Some(ConstraintNodeId::new(entity, index))
}

/// The ASCII normal form of a CURATED unit label — the Rust twin of
/// `normalizeUnitLabel` in `gui/src/stores/unitLadder.ts`.
///
/// Rewrites the two superscript exponent glyphs the curated ladder tables use
/// (U+00B2 → `^2`, U+00B3 → `^3`) and touches nothing else.
///
/// NO CROSS-LANGUAGE CHECK EXISTS. The twins are held together by two
/// mirror-image goldens —
/// `normalize_unit_label_rewrites_only_the_superscript_exponent_glyphs` here,
/// the `normalizeUnitLabel` block in `gui/src/__tests__/unitLadder.test.ts`
/// there — so a deliberate change to either side must edit its own golden, while
/// an accidental one-sided change is NOT caught. A TS-only edit adding an arm
/// for a glyph this function lacks would put a spelling into the panel's
/// alphabet, and into its edit-buffer seed, that the engine then refuses.
///
/// What bounds that surface is the curated data, and that IS gated:
/// `curated_unit_labels_carry_no_glyph_outside_the_shared_normalizer_alphabet`
/// asserts every rung label is ASCII apart from U+00B2/U+00B3, so a rung
/// introducing a third glyph fails there — at which point both twins and both
/// goldens must be extended together.
///
/// Handed unit LABELS only. The `×10ⁿ` engineering-notation superscripts
/// `reify-ir` produces format a MAGNITUDE, not a unit, and are out of scope.
pub(crate) fn normalize_unit_label(label: &str) -> String {
    label.replace('\u{00B2}', "^2").replace('\u{00B3}', "^3")
}

/// The SUPERSCRIPT spelling of a curated unit label — the inverse of
/// [`normalize_unit_label`] over the same two-glyph alphabet.
///
/// WHY IT EXISTS. Until task λ (#5788) the curated ladders were themselves
/// spelled with U+00B2/U+00B3, so [`COMPOSED_UNIT_INDEX`] got the superscript
/// spelling for free — it was the rung's own label, and only the ASCII form had
/// to be synthesized. λ relabelled the tables to the ASCII `^`-exponent
/// alphabet, which flips that around: the label IS the normal form now, and it
/// is the legacy spelling that has to be synthesized or it silently leaves the
/// index. Registering it keeps the accept-set from NARROWING as a side effect of
/// a display relabel — a value a user could commit before λ still commits after.
///
/// Identity on a label carrying no caret exponent, so the builder can push it
/// unconditionally and let `push`'s dedup collapse the no-op.
///
/// This is a legacy-input alias, NOT a second display spelling: nothing renders
/// its output, `reify_core::display_units::ascii_label_spelling` remains the one
/// direction the compiler's `@display` hint speaks, and the frontend gate still
/// admits only the ASCII form.
pub(crate) fn superscript_label_spelling(label: &str) -> String {
    label.replace("^2", "\u{00B2}").replace("^3", "\u{00B3}")
}

/// One resolvable unit spelling in [`composed_unit_index`].
#[derive(Debug)]
pub(crate) struct ComposedUnit {
    /// The exact spelling matched, suffix-wise, by `parse_value_string`.
    pub(crate) label: String,
    /// Conversion to canonical SI: `si_value = magnitude * si_scale`. Same
    /// direction as `reify_core::unit_symbol_to_si`'s factor and as
    /// `UnitOption::si_scale`, so composing the two sources needs no inversion.
    pub(crate) si_scale: f64,
    pub(crate) dimension: DimensionVector,
}

/// Resolve a curated ladder's canonical dimension NAME back to its vector.
///
/// A [`crate::display_units::DimensionLadder`] carries its dimension as a name,
/// not a vector. This is the same first-match `NAMED_DIMENSIONS` scan
/// reify-compiler's `resolve_dimension_type` uses (that function is
/// `pub(crate)` there, so it cannot be called from here).
///
/// Totality across the curated table is already guaranteed by reify-core's
/// `every_ladder_dimension_round_trips_through_canonical_name`, so a `None`
/// means that guard has itself broken. Both callers degrade by SKIPPING the
/// ladder rather than panicking inside a `LazyLock`, which would poison it for
/// every later caller on this process.
///
/// Shared by [`COMPOSED_UNIT_INDEX`] and [`LADDER_COVERAGE`] precisely so the
/// two cannot disagree: a dimension is recorded as covered exactly when the
/// index actually registered that ladder's rungs for it.
fn ladder_dimension(name: &str) -> Option<DimensionVector> {
    reify_core::NAMED_DIMENSIONS
        .iter()
        .find(|(_, candidate)| *candidate == name)
        .map(|(dim, _)| *dim)
}

/// What [`COMPOSED_UNIT_INDEX`] can EXPRESS, per dimension: for each curated
/// ladder it resolved, the `(vector, canonical name, example rung)` triple.
///
/// This is the table [`dimension_requires_unit`] reads, and therefore the table
/// that decides whether a bare number is refused for a cell (task #5757
/// amendment). Built by the SAME [`ladder_dimension`] scan as the index itself,
/// so coverage can never claim a dimension whose rungs the index failed to
/// register — the failure mode that would brick a cell while telling the user
/// to type a unit for it.
///
/// The example rung is stored in its [`normalize_unit_label`] ASCII form. Both
/// spellings parse here, but the frontend's typed-input gate admits only the
/// ASCII one, so suggesting `mm³` would name a literal the panel then refuses
/// inline. The `is_default` rung is preferred — it is the one the cell is
/// already being DISPLAYED in, so `80` → `80mm` is the edit the user is
/// actually making — with the first rung as a fallback for a hypothetical
/// ladder with no default (reify-core's `every_ladder_has_exactly_one_default`
/// makes that unreachable today).
static LADDER_COVERAGE: std::sync::LazyLock<Vec<(DimensionVector, String, String)>> =
    std::sync::LazyLock::new(|| {
        let mut covered = Vec::new();
        for ladder in crate::display_units::unit_ladders() {
            let Some(dimension) = ladder_dimension(&ladder.dimension) else {
                continue; // Already logged by the index builder.
            };
            let Some(rung) = ladder
                .units
                .iter()
                .find(|u| u.is_default)
                .or_else(|| ladder.units.first())
            else {
                continue; // A rungless ladder expresses nothing.
            };
            covered.push((
                dimension,
                ladder.dimension.clone(),
                normalize_unit_label(&rung.label),
            ));
        }
        covered
    });

/// Whether a cell of this dimension can EXPRESS a unit, and if so what to call
/// it: `Some((canonical dimension name, an example rung label))`, else `None`.
///
/// `Some` is the precondition for refusing a bare number in
/// [`parse_value_string_for_cell`], and it doubles as the message's vocabulary —
/// the same lookup yields both the gate and the words, so the refusal cannot
/// name a unit the index could not have parsed.
///
/// That totality is executable, not merely argued:
/// `every_curated_ladder_dimension_is_gated_and_names_a_rung_that_parses` walks
/// `unit_ladders()` and, for EVERY curated ladder, asserts this returns `Some`,
/// that the name it reports is the ladder's own, and that the rung it names
/// parses back through `parse_value_string` to that same dimension.
pub(crate) fn dimension_requires_unit(
    dimension: &DimensionVector,
) -> Option<(&'static str, &'static str)> {
    LADDER_COVERAGE
        .iter()
        .find(|(dim, _, _)| dim == dimension)
        .map(|(_, name, rung)| (name.as_str(), rung.as_str()))
}

/// Every unit spelling the GUI parameter editor can parse, LONGEST LABEL FIRST.
///
/// Composed once from the two Rust-authored tables the GUI already depends on
/// rather than hand-maintained (task #5757 — this replaces the five-entry
/// `UNIT_TABLE` the PRD's open question 5 asks to retire):
///
///   * `reify_core::display_units::unit_ladders()` — the curated per-dimension
///     DISPLAY ladders. The frontend derives its typed-input alphabet from this
///     same table over the `get_unit_ladders` IPC command (`quantityUnitAlphabet`
///     in `gui/src/stores/unitLadder.ts`), so reading it here is what makes the
///     two ends AGREE by construction instead of by lockstep maintenance.
///
///   * `reify_core::BUILTIN_UNITS` — the DSL's bare built-in symbols, which
///     `reify_core::unit_symbol_to_si` is a faithful view of. Contributes the SI
///     bases no ladder carries (`s`, `K`, `A`, `mol`, `cd`).
///
/// BOTH SPELLINGS of every curated rung are registered — the ASCII one
/// [`normalize_unit_label`] produces, which is the only one the frontend gate
/// admits (`normalizeUnitLabel` is one-way) and, since task λ (#5788), also the
/// one the curated tables themselves carry; and the superscript one
/// [`superscript_label_spelling`] produces, which the picker showed before λ and
/// which a pre-λ file or copy-paste can still carry. Registering both makes this
/// a strict SUPERSET of the frontend gate, so no frontend-accepted spelling can
/// be refused on commit, and keeps the accept-set from narrowing when a display
/// relabel moves which of the two the table happens to hold. Pinned by
/// `parse_value_string_accepts_every_curated_ladder_rung_in_both_spellings` and
/// `parse_value_string_also_accepts_the_raw_superscript_ladder_spellings`.
///
/// ORDERING IS LOAD-BEARING: [`resolve_quantity_suffix`] takes the first
/// matching suffix, so descending label length is what stops `m` shadowing `cm`
/// and `m^3` shadowing `kg/m^3`. BYTE length, not char count — `strip_suffix`
/// works on bytes and the superscript glyphs are two bytes each. Pinned by the
/// `debug_assert!` in [`parse_value_string`] and, for release builds, by
/// `unit_table_ordering_invariant_holds`.
///
/// ONE LABEL, ONE ANSWER: at most one entry per spelling, so a match needs no
/// disambiguation and the lookup can be FLAT, with no narrowing by the edited
/// cell's dimension. Pinned by
/// `composed_unit_index_holds_at_most_one_entry_per_spelling` — the direct guard
/// is the load-bearing one because the builder's dedup key is
/// `(label, dimension)`, and because the three source-table guards
/// (`curated_ladder_labels_are_unique_across_every_dimension`, reify-core's
/// `builtin_unit_symbols_are_unique`,
/// `curated_ladders_and_builtin_units_agree_bit_for_bit_where_they_overlap`) are
/// each one-sided: none compares a NORMALIZED curated label to a builtin symbol.
///
/// A CROSS-DIMENSION LITERAL IS NOT REFUSED HERE. The frontend's per-cell
/// alphabet always carries the `BASE_UNIT_LABELS` floor, so the panel admits a
/// Length literal in a Volume cell; refusing it here would re-open the same
/// frontend/backend disagreement in the opposite direction. It resolves to its
/// OWN dimension and is refused by reify-eval's `DimensionMismatch`, which names
/// both.
///
/// DELIBERATELY NOT the compiler's per-module `UnitRegistry` (`km`, `ft`, `psi`,
/// `degC`, and arbitrary compounds). Reaching it needs prelude-seeding from
/// `compile_builder/units_phase.rs`, affine-offset handling, and a
/// `&str -> reify_ast::UnitExpr` parser that exists nowhere in the workspace —
/// all to admit spellings the frontend gate rejects outright.
///
/// WHAT THIS INDEX COVERS ALSO DECIDES WHERE THE BARE-NUMBER GATE FIRES:
/// [`parse_value_string_for_cell`] refuses a bare number only for a dimension
/// [`LADDER_COVERAGE`] records. A dimension this index cannot express keeps the
/// SI-number path, because refusing there would remove the cell's last accepted
/// input rather than disambiguate anything. The bounded in-tree case is `Money`,
/// whose `examples/*.ri` literals spell `NUSD` and whose `USD` is reachable only
/// through the excluded `UnitRegistry`.
static COMPOSED_UNIT_INDEX: std::sync::LazyLock<Vec<ComposedUnit>> =
    std::sync::LazyLock::new(|| {
        /// Register one spelling. `(label, dimension)` is the identity: the
        /// eight labels carried by BOTH source tables would otherwise appear
        /// twice. Which copy wins is unobservable today — the two tables agree
        /// bit-for-bit, and
        /// `curated_ladders_and_builtin_units_agree_bit_for_bit_where_they_overlap`
        /// is what keeps it that way.
        fn push(
            entries: &mut Vec<ComposedUnit>,
            label: String,
            si_scale: f64,
            dimension: DimensionVector,
        ) {
            if entries
                .iter()
                .any(|e| e.label == label && e.dimension == dimension)
            {
                return;
            }
            entries.push(ComposedUnit {
                label,
                si_scale,
                dimension,
            });
        }

        let mut entries: Vec<ComposedUnit> = Vec::new();

        // Curated display ladders first, so that on a same-length collision the
        // spelling the user is being SHOWN wins.
        for ladder in crate::display_units::unit_ladders() {
            let Some(dimension) = ladder_dimension(&ladder.dimension) else {
                tracing::debug!(
                    target: "reify_gui::engine::unit_index",
                    dimension = %ladder.dimension,
                    "curated unit ladder names a dimension with no NAMED_DIMENSIONS \
                     entry — its rungs will not be parseable"
                );
                continue;
            };
            for opt in ladder.units {
                // Three spellings, pushed unconditionally and deduped by
                // `push`: the rung's own label, its ASCII normal form and its
                // superscript form. Which two of the three collapse depends on
                // how the curated table is spelled TODAY — before task λ
                // (#5788) the label was the superscript one, since λ it is the
                // ASCII one — and the index must not care, because what it
                // owes its callers is that BOTH spellings resolve either way.
                push(
                    &mut entries,
                    normalize_unit_label(&opt.label),
                    opt.si_scale,
                    dimension,
                );
                push(
                    &mut entries,
                    superscript_label_spelling(&opt.label),
                    opt.si_scale,
                    dimension,
                );
                push(&mut entries, opt.label, opt.si_scale, dimension);
            }
        }

        // Then the DSL's bare built-in symbols.
        for &(symbol, factor, dimension) in reify_core::BUILTIN_UNITS {
            push(&mut entries, symbol.to_string(), factor, dimension);
        }

        // `sort_by_key` is stable, so equal-length entries keep insertion order
        // (curated ladders before builtins).
        entries.sort_by_key(|e| std::cmp::Reverse(e.label.len()));
        entries
    });

/// The composed unit index, longest label first. See [`COMPOSED_UNIT_INDEX`].
pub(crate) fn composed_unit_index() -> &'static [ComposedUnit] {
    &COMPOSED_UNIT_INDEX
}

/// The suffix scan: first entry of `index` that `s` ends with AND whose stripped
/// remainder is a number, as a `Scalar` in that entry's dimension.
///
/// TWO INDEPENDENT DEFENCES against a shorter label shadowing a longer one:
///
///   1. `index` order. Callers pass it longest-label-first, so `m` is reached
///      only after `cm`, and `m^3` only after `kg/m^3`.
///   2. The remainder check. A candidate wins only if what precedes the label
///      parses as a number, so `m^3` matching `"5kg/m^3"` is rejected on the
///      `"5kg/"` remainder and the scan continues regardless of order.
///
/// Taking `index` as a parameter rather than reading the static is what makes
/// (2) separately observable:
/// `parse_value_string_remainder_guard_disambiguates_without_the_ordering`
/// passes a deliberately reverse-sorted copy, where defence (1) is inverted and
/// only (2) can produce the right answer. Production has exactly one caller,
/// [`parse_value_string`], which passes [`composed_unit_index`].
///
/// ONE LABEL, ONE ANSWER: the index holds at most one entry per spelling (see
/// [`COMPOSED_UNIT_INDEX`]), so a match needs no disambiguation and no second
/// lookup.
pub(crate) fn resolve_quantity_suffix(index: &[ComposedUnit], s: &str) -> Option<Value> {
    for entry in index {
        let Some(num_str) = s.strip_suffix(entry.label.as_str()) else {
            continue;
        };
        let Ok(v) = num_str.trim().parse::<f64>() else {
            continue;
        };
        return Some(Value::Scalar {
            si_value: v * entry.si_scale,
            dimension: entry.dimension,
        });
    }
    None
}

/// Parse a value string into a Value, with no cell context.
///
/// Supported formats:
/// - Quantity literals: a number plus any unit in the composed unit index — every
///   rung of every curated display ladder, in both its raw superscript and its
///   ASCII spelling (`80mm`, `5L`, `5mm^3`, `5mm³`, `2kg/m^3`, `10MPa`, `750N`),
///   unioned with the DSL's bare built-in symbols (`3in`, `2s`, `300K`, `5A`,
///   `3mol`, `7cd`). The set is DERIVED, so this list is illustrative, never
///   exhaustive — read the index, not this comment (task #5757).
/// - Plain numbers: "5.0" → Real, "5" → Int
/// - Booleans: "true", "false"
///
/// Deliberately dimension-AGNOSTIC, for callers with no cell context. A bare
/// number is a perfectly good `Value::Int`/`Value::Real` here; refusing one for a
/// DIMENSIONED cell is `parse_value_string_for_cell`'s job, because only it
/// knows the declared type. (Both are crate-private, hence named rather than
/// intra-doc linked from this `pub` item.)
pub fn parse_value_string(s: &str) -> Result<Value, String> {
    let s = s.trim();

    // Booleans, ahead of the suffix scan.
    if s == "true" {
        return Ok(Value::Bool(true));
    }
    if s == "false" {
        return Ok(Value::Bool(false));
    }

    // Quantity literals (number + unit suffix). The debug_assert! enforces the
    // index ordering at call-time in debug builds; #[test]
    // unit_table_ordering_invariant_holds covers release builds.
    let index = composed_unit_index();
    debug_assert!(
        index
            .windows(2)
            .all(|w| w[0].label.len() >= w[1].label.len()),
        "composed unit index must be sorted by descending suffix byte length"
    );
    if let Some(v) = resolve_quantity_suffix(index, s) {
        return Ok(v);
    }

    // Plain integer
    if let Ok(i) = s.parse::<i64>() {
        return Ok(Value::Int(i));
    }

    // Plain float
    if let Ok(f) = s.parse::<f64>() {
        return Ok(Value::Real(f));
    }

    Err(format!("Cannot parse value '{}'", s))
}

/// Peel `Option<…>` wrappers down to the type a supplied value must satisfy.
///
/// `param parasitic_error : Option<Length> = none` declares a cell that takes a
/// `Length`, and a bare number is exactly as ambiguous there as in a plain
/// `Length` cell — so [`parse_value_string_for_cell`] gates through this rather
/// than matching `Type::Scalar` directly. Without it those cells skipped the
/// gate and fell through to reify-eval's generic `TypeKindMismatch` instead of
/// the actionable message. Nothing accepted is lost: `none` is not parseable by
/// `parse_value_string` on any path, so the gate can only ever be reached with a
/// value the Option cell had to satisfy anyway. Looped rather than one-shot
/// because the type is structural; `Option<Option<T>>` is not expected in tree.
fn unwrap_optional(ty: &reify_core::Type) -> &reify_core::Type {
    let mut ty = ty;
    while let reify_core::Type::Option(inner) = ty {
        ty = inner;
    }
    ty
}

/// Parse a value string for a SPECIFIC declared cell type (task #5757).
///
/// The one thing the context-free [`parse_value_string`] cannot do: **refuse a
/// bare number for a dimensioned cell.** This is the GUI-side fix for the
/// silent 1000× hazard: `parse_value_string("120")` yields `Value::Int(120)`,
/// and reify-eval then ACCEPTS it into a `Length` cell —
/// `value_type_kind_matches` maps Int/Real onto `Type::Scalar { .. }` with a
/// dimension WILDCARD, and `validate_param_override` guards its dimension
/// comparison on `let Value::Scalar { .. } = value`, which an Int never
/// matches, so the dimension check is skipped entirely and `120` becomes 120
/// METRES.
///
/// Fixed HERE rather than in reify-eval deliberately: that Int/Real coercion is
/// load-bearing there (it is what lets `edit_param` emit a Warning rather than a
/// hard error) and sits on the hot path for every param override in the
/// workspace, not just GUI edits.
///
/// Unit RESOLUTION is unchanged by the cell type — [`parse_value_string`] does
/// all of it against one flat index; see [`COMPOSED_UNIT_INDEX`].
///
/// THE GATE KEYS ON EXPRESSIBILITY, not on dimensionedness: it fires only where
/// [`dimension_requires_unit`] reports a curated ladder, because the 1000×
/// ambiguity it targets presupposes a unit COULD have been typed. Where none
/// can be, refusing removes the cell's last accepted input and makes the row
/// permanently uneditable through `set_parameter` — in the panel AND on the GUI
/// MCP surface.
///
/// Each conjunct, and what pins it:
///
///   * `unwrap_optional` first, so an `Option<Length>` cell is gated like a
///     `Length` one — `parse_value_string_for_cell_gates_through_an_option_wrapper`;
///   * only `Value::Int` / `Value::Real` are refused; every other variant falls
///     through to reify-eval's own `TypeKindMismatch` / `DimensionMismatch`,
///     notably `Value::Bool`, whose message an existing test depends on;
///   * `dimension_requires_unit(..).is_some()` — NAMEDNESS IS NOT THE KEY. A
///     COMPOSED dimension and a NAMED-but-unladdered one (`Money`, `Torque`) are
///     both ungated for the same reason, pinned together by
///     `parse_value_string_for_cell_keys_the_gate_on_expressibility_not_on_namedness`
///     because keying on `canonical_name().is_some()` would read as an
///     equivalent refactor and split them;
///   * `!dimension.is_dimensionless()` is explicit even though every covered
///     dimension is non-dimensionless by construction, so a future curated
///     ladder for a dimensionless quantity cannot silently start gating every
///     ratio slider. `param x : Real` compiles to
///     `Type::Scalar { DIMENSIONLESS }` and falls on the permissive side.
///
/// THE MESSAGE IS BUILT FROM THE LADDER DATA THE GATE JUST CONSULTED, so it can
/// only name a rung this index parses — pinned across every curated ladder by
/// `every_curated_ladder_dimension_is_gated_and_names_a_rung_that_parses`. The
/// input is quoted TRIMMED so the suggested literal also satisfies the
/// frontend's whitespace-free grammar. It does NOT point at a unit picker:
/// `pickerLadder` renders no `<select>` for a ladder of fewer than two rungs,
/// and `Force`/`Energy`/`Power` carry exactly one each.
pub(crate) fn parse_value_string_for_cell(
    s: &str,
    cell_type: &reify_core::Type,
) -> Result<Value, String> {
    // The same trim `parse_value_string` performs internally, hoisted so the
    // refusal below quotes the input and builds its suggested literal in the
    // CANONICAL spelling. Without it `" 120 "` is suggested back as
    // `' 120 mm'` — whitespace between magnitude and unit, which this backend
    // happens to accept but which the frontend's deliberately stricter
    // `buildQuantityRe` (no whitespace, mirroring the .ri grammar's
    // `token.immediate`) refuses outright, so the message would be handing the
    // user a literal the panel then rejects inline.
    let s = s.trim();
    let value = parse_value_string(s)?;

    if let reify_core::Type::Scalar { dimension } = unwrap_optional(cell_type)
        && !dimension.is_dimensionless()
        && matches!(value, Value::Int(_) | Value::Real(_))
        && let Some((expected, rung)) = dimension_requires_unit(dimension)
    {
        return Err(format!(
            "expects {expected}, got the bare number '{s}'; pass a dimensioned \
             {expected} literal such as '{s}{rung}'"
        ));
    }

    Ok(value)
}

/// Extract the unit symbol trailing a default-literal slice, as an ADVISORY
/// hint for [`reify_ir::value_to_ri_literal_with_unit`]'s `preferred_unit`
/// parameter (task 5096 γ, INV-GUI-3 write-back).
///
/// Deliberately LEXICAL, not a parser: it reads the symbol off the literal
/// being replaced and hands it to `value_to_ri_literal_with_unit` as a hint
/// only — that function honours the hint exclusively when it resolves as a
/// bare built-in, its dimension matches the value being written, and the
/// magnitude is bit-exact, silently falling back to the canonical unit
/// ladder otherwise. So a false positive here (e.g. reading `mm` off the
/// identifier `x2mm`) can only change WHICH exact literal
/// [`EngineSession::apply_param_to_source`] writes, never WHETHER the write
/// is exact.
///
/// Algorithm: take the longest trailing run of `is_ascii_alphabetic` chars
/// in the trimmed slice, then look at the character immediately before that
/// run (skipping ASCII whitespace). The run is returned as the hint only
/// when that predecessor is an ASCII digit or `.` — that is what rejects a
/// bare identifier (`width`), `auto`, and `true`/`false`, none of which have
/// a digit/`.` anchoring their trailing letters.
pub(crate) fn unit_hint_from_default_literal(default_slice: &str) -> Option<&str> {
    let trimmed = default_slice.trim();
    let mut alpha_start = trimmed.len();
    for (i, c) in trimmed.char_indices().rev() {
        if !c.is_ascii_alphabetic() {
            break;
        }
        alpha_start = i;
    }
    // alpha_start == trimmed.len(): the trailing run is empty (the last char
    // isn't alphabetic). alpha_start == 0: the run reaches the start of the
    // string, so there is no predecessor to check (a bare identifier like
    // "width" or "auto").
    if alpha_start == 0 || alpha_start == trimmed.len() {
        return None;
    }
    match trimmed[..alpha_start].trim_end().chars().next_back() {
        Some(c) if c.is_ascii_digit() || c == '.' => Some(&trimmed[alpha_start..]),
        _ => None,
    }
}

/// Reports whether a quantity literal's `unit` is one
/// [`EngineSession::apply_param_to_source`] can write back WITHOUT changing the
/// unit vocabulary the user authored — i.e. a bare symbol that
/// [`reify_core::units::unit_symbol_to_si`] resolves.
///
/// The write-back's serializer, [`reify_ir::value_to_ri_literal_with_unit`],
/// validates its `preferred_unit` hint against the BUILT-IN table only: bare
/// `mm`/`m`/`in`/`deg`/… and nothing else. User-declared units (`unit mil :
/// Length = 0.0000254`, or `km` and `ft` out of `std.units`) live exclusively
/// in the compiler's per-module `UnitRegistry`, which that layer deliberately
/// has no view of. A hint it cannot resolve is silently DROPPED and the value
/// goes out on the canonical ladder instead — so without this gate, tweaking
/// `param thickness: Length = 200mil` would rewrite it as `5.08mm`. The number
/// is right; the vocabulary the user chose for their own document is gone, and
/// nothing told them.
///
/// That is the same class of silent destruction the non-literal gate exists to
/// prevent (see [`EngineSession::apply_param_to_source`]), so it gets the same
/// answer: REFUSE, with a discriminated message δ can surface, rather than
/// canonicalize behind the user's back. Compound unit expressions (`kN*m`,
/// `kg/m^3`) are refused for the same reason plus a second one — the emitter's
/// ladder covers no compound dimension at all, so they could only ever be
/// rejected one phase later with a much vaguer message.
///
/// This is a LIMITATION of the emitter, not a policy: the day
/// `value_to_ri_literal_with_unit` can resolve a hint through the compiled
/// module's `UnitRegistry`, this gate should widen to match rather than stay.
fn unit_is_emittable_as_written(unit: &reify_ast::UnitExpr) -> bool {
    match unit {
        reify_ast::UnitExpr::Unit(symbol) => {
            reify_core::units::unit_symbol_to_si(symbol).is_some()
        }
        reify_ast::UnitExpr::Mul(..)
        | reify_ast::UnitExpr::Div(..)
        | reify_ast::UnitExpr::Pow(..) => false,
    }
}

/// Describe `unit` for the rejection [`EngineSession::resolve_rewritable_default_span`]
/// returns when [`unit_is_emittable_as_written`] refuses it.
///
/// Names the offending SYMBOL for a bare unit, because that is the word the
/// user will search their `.ri` for; a compound expression is described by
/// shape rather than reconstructed, since the reader has the span in front of
/// them and a half-faithful re-rendering would be worse than none.
fn describe_unit_expr(unit: &reify_ast::UnitExpr) -> String {
    match unit {
        reify_ast::UnitExpr::Unit(symbol) => {
            format!("the unit '{symbol}', which is not a built-in unit symbol")
        }
        reify_ast::UnitExpr::Mul(..)
        | reify_ast::UnitExpr::Div(..)
        | reify_ast::UnitExpr::Pow(..) => {
            "a compound unit expression, which has no bare-literal form".to_string()
        }
    }
}

/// Name the `ExprKind` variant `kind` is, as its Rust identifier (`"BinOp"`,
/// `"Auto"`, `"FunctionCall"`, …), for the rejection message
/// [`EngineSession::resolve_rewritable_default_span`] returns when a param's
/// default is not a literal it may splice over (task 5096 γ, INV-GUI-3).
///
/// Written as an EXHAUSTIVE match with NO `_` arm ON PURPOSE. A future
/// `ExprKind` variant must be classified deliberately — is it splice-safe
/// (another literal form the write-back may overwrite) or not? — and the
/// missing-arm compile error is what forces that decision. A catch-all would
/// silently absorb the new variant into a generic label, which reads as an
/// answered question when it is an unasked one.
///
/// The names are the Rust variant identifiers rather than user-facing prose
/// because the consumer is a diagnostic aimed at someone reading the `.ri`
/// alongside this code; `resolve_rewritable_default_span`'s taxonomy tests
/// assert on these substrings.
fn expr_kind_name(kind: &reify_ast::ExprKind) -> &'static str {
    use reify_ast::ExprKind as K;
    match kind {
        K::NumberLiteral { .. } => "NumberLiteral",
        K::QuantityLiteral { .. } => "QuantityLiteral",
        K::StringLiteral(_) => "StringLiteral",
        K::BoolLiteral(_) => "BoolLiteral",
        K::Ident(_) => "Ident",
        K::BinOp { .. } => "BinOp",
        K::UnOp { .. } => "UnOp",
        K::FunctionCall { .. } => "FunctionCall",
        K::MemberAccess { .. } => "MemberAccess",
        K::EnumAccess { .. } => "EnumAccess",
        K::Conditional { .. } => "Conditional",
        K::ListLiteral(_) => "ListLiteral",
        K::SetLiteral(_) => "SetLiteral",
        K::MapLiteral(_) => "MapLiteral",
        K::IndexAccess { .. } => "IndexAccess",
        K::Match { .. } => "Match",
        K::Auto { .. } => "Auto",
        K::Undef => "Undef",
        K::Lambda { .. } => "Lambda",
        K::Quantifier { .. } => "Quantifier",
        K::AdHocSelector { .. } => "AdHocSelector",
        K::QualifiedAccess { .. } => "QualifiedAccess",
        K::InstanceQualifiedAccess { .. } => "InstanceQualifiedAccess",
        K::Range { .. } => "Range",
        K::TraitMethodCall { .. } => "TraitMethodCall",
        K::TraitStaticCall { .. } => "TraitStaticCall",
        K::VariantConstruct { .. } => "VariantConstruct",
        K::InterpolatedString(_) => "InterpolatedString",
    }
}

/// Replace `path`'s contents with `content` ATOMICALLY: write a sibling temp
/// file, sync it, then `rename` it over `path`.
///
/// The write-back ([`EngineSession::apply_param_to_source`]) is the caller. A
/// plain `fs::write` truncates in place, so a write that fails part-way
/// (ENOSPC, EIO, a killed process) leaves a TRUNCATED `.ri` on disk — a corrupt
/// design that the FS-watcher would then dutifully reload into the GUI. A
/// rename within one directory is atomic, so every reader sees the whole old
/// file or the whole new one and that failure mode does not exist.
///
/// Four details that are load-bearing rather than incidental:
///
/// * **Symlinks are followed, not replaced.** `rename(2)` does NOT follow a
///   symlink at its destination, so renaming straight over `path` would DELETE
///   a symlinked `.ri` and leave a regular file in its place, while the file
///   the link pointed at kept the pre-edit content forever — and the caller's
///   divergence guard could not notice, because `fs::read_to_string` follows
///   the link and would keep comparing against the (matching) target's bytes.
///   `path` is therefore resolved through [`std::fs::canonicalize`] first and
///   the write goes to the RESOLVED file, matching the write-through behaviour
///   a plain `fs::write` would have had. A path that cannot be canonicalized
///   (it does not exist yet, say) falls back to itself unchanged.
/// * **Same directory.** `rename` is only atomic within a filesystem, so the
///   temp lives beside the resolved target — not in `/tmp`, and not beside the
///   symlink when those differ.
/// * **`.tmp` suffix, plus pid AND a process-local sequence number.** The GUI's
///   watcher filters on the `.ri` extension (`watcher.rs`), so a `.tmp` sibling
///   never reads as a design file appearing in the project. The pid keeps two
///   PROCESSES writing the same design off one temp path, and the sequence
///   number does the same for two `EngineSession`s inside ONE process, which
///   share a pid. The rename itself DOES fire a watch event for the resolved
///   path — the watcher accepts `Modify(_)`, which covers inotify's
///   rename-into-place — so hot reload still works; through a symlink the event
///   lands on the target's directory rather than the link's, which the
///   authoritative in-process recompile (D7) already covers.
/// * **Permissions are carried over** from the file being replaced, so a
///   design the user made read-only-for-group (or otherwise chmod'd) does not
///   silently come back with the process umask's mode. Best-effort: a
///   permission read/write failure is not worth failing the edit over.
///
/// The temp is removed on every failure path, so a failed write leaves no
/// litter next to the user's design.
///
/// Contents are `sync_all`ed before the rename, and the containing DIRECTORY is
/// synced after it, so the replacement survives a power loss rather than merely
/// being atomic against concurrent readers. The directory sync is best-effort:
/// not every filesystem permits opening a directory for sync, and a design edit
/// that already reached the page cache is not worth failing over one.
fn write_file_atomically(path: &Path, content: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Disambiguates temp paths between two `EngineSession`s in ONE process,
    /// which the pid alone cannot: they would otherwise race on the same name
    /// and one would `create`-truncate the other's half-written temp.
    static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

    // See the doc comment: rename(2) would otherwise destroy a symlinked `.ri`
    // and orphan its target. Fall back to `path` when it cannot be resolved —
    // there is then no link to follow and nothing this can improve on.
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());

    let file_name = target.file_name().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "path has no file name to write",
        )
    })?;
    let mut tmp_name = file_name.to_os_string();
    tmp_name.push(format!(
        ".{}.{}.tmp",
        std::process::id(),
        TMP_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let tmp_path = target.with_file_name(tmp_name);

    let write_and_rename = || -> std::io::Result<()> {
        let mut file = std::fs::File::create(&tmp_path)?;
        file.write_all(content.as_bytes())?;
        // Order the contents before the rename: a rename that lands while the
        // data is still only in the page cache would publish an empty or
        // partial file to a reader that crosses the same crash.
        file.sync_all()?;
        drop(file);

        // Best-effort mode preservation — see the doc comment. Deliberately
        // ignores errors: failing an otherwise-good edit because a mode could
        // not be copied would be the wrong trade.
        if let Ok(meta) = std::fs::metadata(&target)
            && meta.is_file()
        {
            let _ = std::fs::set_permissions(&tmp_path, meta.permissions());
        }

        std::fs::rename(&tmp_path, &target)?;

        // Durability, not atomicity: the rename is already atomic against a
        // concurrent reader, but the DIRECTORY entry it rewrote can still be
        // lost to a power cut until the directory itself is synced. Mirrors
        // `reify_eval::persistent_cache::write_entry`. Best-effort by design —
        // see the doc comment.
        if let Some(parent) = target.parent()
            && let Ok(dir) = std::fs::File::open(parent)
        {
            let _ = dir.sync_all();
        }
        Ok(())
    };

    let result = write_and_rename();
    if result.is_err() {
        // The rename is what consumes the temp, so any failure before it (or a
        // failed rename itself) leaves the temp behind. Remove it rather than
        // littering the user's project directory with one file per failure.
        let _ = std::fs::remove_file(&tmp_path);
    }
    result
}

/// Reports whether `s` looks like a source identifier (`[A-Za-z_][A-Za-z0-9_]*`).
///
/// Used only by `format_expr` to decide how to pretty-print a string-literal
/// `IndexAccess` key — a display heuristic, not a real disambiguation between
/// member access and `Map<String, _>` lookup (see the `IndexAccess` arm).
fn is_identifier_like(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Format a compiled expression as a human-readable string.
fn format_expr(expr: &reify_ir::CompiledExpr) -> String {
    use reify_ir::CompiledExprKind;

    match &expr.kind {
        CompiledExprKind::Literal(v) => {
            // Unit-bearing literals render "{val} {unit}" (space-separated).
            // This is deliberately different from AutoResolveParameterValue's
            // `display` field, built as "{val}{unit}" with no space
            // (build_parameters_payload above, pinned by the "4.2mm" golden
            // in tests/engine_tests.rs) — the two are independently-evolved,
            // known-divergent value+unit display surfaces pending
            // unification under docs/prds/display-unit-preference.md §7c
            // (task 5234). Not an oversight; do not "fix" one to match the
            // other here.
            let (val, unit) = crate::types::format_value(v);
            if unit.is_empty() {
                val
            } else {
                format!("{} {}", val, unit)
            }
        }
        CompiledExprKind::ValueRef(id) | CompiledExprKind::CrossSubGeometryRef(id) => {
            // CrossSubGeometryRef formats identically to ValueRef — both name the
            // member on the synthetic cross-sub entity stamp (task-3508).
            id.member.clone()
        }
        CompiledExprKind::BinOp { op, left, right } => {
            let op_str = match op {
                reify_ir::BinOp::Add => "+",
                reify_ir::BinOp::Sub => "-",
                reify_ir::BinOp::Mul => "*",
                reify_ir::BinOp::Div => "/",
                reify_ir::BinOp::Mod => "%",
                reify_ir::BinOp::Pow => "**",
                reify_ir::BinOp::Eq => "==",
                reify_ir::BinOp::Ne => "!=",
                reify_ir::BinOp::Lt => "<",
                reify_ir::BinOp::Le => "<=",
                reify_ir::BinOp::Gt => ">",
                reify_ir::BinOp::Ge => ">=",
                reify_ir::BinOp::And => "&&",
                reify_ir::BinOp::Or => "||",
                reify_ir::BinOp::Implies => "implies",
            };
            format!("{} {} {}", format_expr(left), op_str, format_expr(right))
        }
        CompiledExprKind::UnOp { op, operand } => {
            let op_str = match op {
                reify_ir::UnOp::Neg => "-",
                reify_ir::UnOp::Not => "!",
            };
            format!("{}{}", op_str, format_expr(operand))
        }
        CompiledExprKind::FunctionCall { function, args } => {
            let arg_strs: Vec<String> = args.iter().map(format_expr).collect();
            format!("{}({})", function.name, arg_strs.join(", "))
        }
        CompiledExprKind::Conditional {
            condition,
            then_branch,
            else_branch,
        } => {
            format!(
                "if {} then {} else {}",
                format_expr(condition),
                format_expr(then_branch),
                format_expr(else_branch)
            )
        }
        CompiledExprKind::Match { discriminant, arms } => {
            let arm_strs: Vec<String> = arms
                .iter()
                .map(|arm| {
                    let pat_strs: Vec<String> = arm
                        .patterns
                        .iter()
                        .map(|p| match p.tag_name() {
                            Some(name) => name.to_string(),
                            None => "_".to_string(),
                        })
                        .collect();
                    format!("{} => {}", pat_strs.join(" | "), format_expr(&arm.body))
                })
                .collect();
            format!(
                "match {} {{ {} }}",
                format_expr(discriminant),
                arm_strs.join(", ")
            )
        }
        CompiledExprKind::UserFunctionCall {
            function_name,
            args,
        } => {
            let arg_strs: Vec<String> = args.iter().map(format_expr).collect();
            format!("{}({})", function_name, arg_strs.join(", "))
        }
        CompiledExprKind::Lambda { .. } => "<lambda>".to_string(),
        // ReflectiveCellList shares identical surface formatting with ListLiteral —
        // the variant distinction is internal to the evaluator (task-2458).
        CompiledExprKind::ListLiteral(elems) | CompiledExprKind::ReflectiveCellList(elems) => {
            let elem_strs: Vec<String> = elems.iter().map(format_expr).collect();
            format!("[{}]", elem_strs.join(", "))
        }
        CompiledExprKind::SetLiteral(elems) => {
            let elem_strs: Vec<String> = elems.iter().map(format_expr).collect();
            format!("set{{{}}}", elem_strs.join(", "))
        }
        CompiledExprKind::MapLiteral(entries) => {
            let entry_strs: Vec<String> = entries
                .iter()
                .map(|(k, v)| format!("{} => {}", format_expr(k), format_expr(v)))
                .collect();
            format!("map{{{}}}", entry_strs.join(", "))
        }
        CompiledExprKind::IndexAccess { object, index } => {
            // Member access (`obj.member`) is lowered to `IndexAccess` with a
            // string-literal index — recover the source-form dotted access,
            // but only when the key looks like an identifier. A `Map` keyed
            // by an arbitrary string (e.g. `config["max load"]`) compiles to
            // the identical IR shape, so a non-identifier key falls back to
            // the quoted bracket form instead of the syntactically-invalid
            // `config.max load`. An identifier-shaped map key (e.g.
            // `config["mode"]`) is still indistinguishable from real member
            // access at this level and will render as `config.mode` — a
            // known display-only limitation, not re-parsed.
            match &index.kind {
                CompiledExprKind::Literal(reify_ir::Value::String(member))
                    if is_identifier_like(member) =>
                {
                    format!("{}.{}", format_expr(object), member)
                }
                CompiledExprKind::Literal(reify_ir::Value::String(member)) => {
                    format!("{}[\"{}\"]", format_expr(object), member)
                }
                _ => format!("{}[{}]", format_expr(object), format_expr(index)),
            }
        }
        CompiledExprKind::MethodCall {
            object,
            method,
            args,
        } => {
            if args.is_empty() {
                format!("{}.{}", format_expr(object), method)
            } else {
                let arg_strs: Vec<String> = args.iter().map(format_expr).collect();
                format!(
                    "{}.{}({})",
                    format_expr(object),
                    method,
                    arg_strs.join(", ")
                )
            }
        }
        CompiledExprKind::Quantifier {
            kind,
            variable,
            collection,
            predicate,
            ..
        } => {
            let keyword = match kind {
                reify_ast::QuantifierKind::ForAll => "forall",
                reify_ast::QuantifierKind::Exists => "exists",
            };
            format!(
                "{} {} in {}: {}",
                keyword,
                variable,
                format_expr(collection),
                format_expr(predicate)
            )
        }
        CompiledExprKind::OptionSome(inner) => format!("some({})", format_expr(inner)),
        CompiledExprKind::OptionNone => "none".to_string(),
        CompiledExprKind::MetaAccess { entity, key } => format!("{}.meta.{}", entity, key),
        CompiledExprKind::DeterminacyPredicate { kind, cell } => {
            let fn_name = match kind {
                reify_ir::DeterminacyPredicateKind::Determined => "determined",
                reify_ir::DeterminacyPredicateKind::Undetermined => "undetermined",
                reify_ir::DeterminacyPredicateKind::Constrained => "constrained",
                reify_ir::DeterminacyPredicateKind::PartiallyDetermined => {
                    "partially_determined"
                }
            };
            format!("{}({})", fn_name, cell.member)
        }
        CompiledExprKind::RangeConstructor {
            lower,
            upper,
            lower_inclusive,
            upper_inclusive,
        } => match (lower, upper) {
            (Some(lo), Some(hi)) => {
                let op = if *upper_inclusive { ".." } else { "..<" };
                format!("{}{}{}", format_expr(lo), op, format_expr(hi))
            }
            (Some(bound), None) => {
                let op = if *lower_inclusive { ">=" } else { ">" };
                format!("{}{}", op, format_expr(bound))
            }
            (None, Some(bound)) => {
                let op = if *upper_inclusive { "<=" } else { "<" };
                format!("{}{}", op, format_expr(bound))
            }
            (None, None) => "..".to_string(),
        },
        CompiledExprKind::AdHocSelector {
            base,
            selector_kind,
            args,
        } => {
            let kind_str = match selector_kind {
                reify_ir::SelectorKind::Face => "face",
                reify_ir::SelectorKind::Point => "point",
                reify_ir::SelectorKind::Edge => "edge",
            };
            let args_str: Vec<String> = args.iter().map(format_expr).collect();
            format!(
                "{} @ {}({})",
                format_expr(base),
                kind_str,
                args_str.join(", ")
            )
        }
        // Reflective-aggregation placeholder (task-2289): renders as the
        // source-level shape "<param_name>.<query_kind>" for hover/debug.
        // Once activate_purpose runs, this variant is replaced by a populated
        // ListLiteral, so the GUI normally only encounters it in pre-activation
        // debug views.
        CompiledExprKind::PurposeReflectiveAggregation {
            param_name,
            query_kind,
        } => format!("{}.{}", param_name, query_kind),
        // task 3540 (SIR-α): exhaustiveness-forced adapter arm for the new
        // shared-enum variant (step-16). Renders as the source-level
        // constructor shape `TypeName(arg1, arg2, ...)` — same surface form
        // as FunctionCall/UserFunctionCall for hover/debug views.
        CompiledExprKind::StructureInstanceCtor {
            type_name,
            ordered_args,
            ..
        } => {
            let arg_strs: Vec<String> =
                ordered_args.iter().map(|(_, e)| format_expr(e)).collect();
            format!("{}({})", type_name, arg_strs.join(", "))
        }
        // task 4118 (γ): the Selector→List<Geometry> coercion is compiler-
        // inserted and invisible in source, so format transparently as the
        // inner selector (the user wrote `faces(b)`, not a coercion wrapper).
        CompiledExprKind::ResolveSelector { selector } => format_expr(selector),
    }
}

/// Collect all ValueCellId references from a compiled expression.
fn collect_value_refs(expr: &reify_ir::CompiledExpr) -> Vec<String> {
    let mut refs: Vec<String> = expr
        .collect_value_refs()
        .into_iter()
        .map(|id| id.to_string())
        .collect();
    refs.sort();
    refs.dedup();
    refs
}

/// Map a slice of [`Diagnostic`] to `Vec<DiagnosticInfo>`.
///
/// `file_path` is the source file name used for all produced `DiagnosticInfo`
/// entries.  When no file is available (e.g. tessellation errors without a
/// known source location), pass `"<unknown>"` and an empty string for `source`.
///
/// Each diagnostic's first label span is used for line/column resolution.
/// Diagnostics without labels (labelless fallback) produce `(1, 1, 1, 1)`.
///
/// # Severity format
///
/// `DiagnosticInfo::severity` is serialized as PascalCase (`"Error"`,
/// `"Warning"`, `"Info"`).  The canonical mapping lives on
/// [`reify_types::Severity::as_wire_str`] and the `Serialize` derive on
/// `Severity` — not in this helper.  Both `get_diagnostics` (compile-time)
/// and the tessellation path (wire + `warn!` log) call `as_wire_str()`.
/// The wire format is pinned by tests; the log field shares the same call
/// but is not separately asserted.
/// MCP consumers and TypeScript code must compare against PascalCase strings.
fn diagnostics_to_info(
    diagnostics: &[Diagnostic],
    file_path: &str,
    source: &str,
) -> Vec<DiagnosticInfo> {
    if diagnostics.is_empty() {
        return Vec::new();
    }
    // Build the newline table once (O(M)) so each span lookup is O(log M).
    let line_offsets = build_line_offsets(source);
    diagnostics
        .iter()
        .map(|diag| {
            // Use the first label's span if available; otherwise default to (1,1,1,1).
            let (line, column, end_line, end_column) = if let Some(label) = diag.labels.first() {
                let (l, c) =
                    offset_to_line_col_fast(source, &line_offsets, label.span.start as usize);
                let (el, ec) =
                    offset_to_line_col_fast(source, &line_offsets, label.span.end as usize);
                (l as u32, c as u32, el as u32, ec as u32)
            } else {
                (1, 1, 1, 1)
            };
            DiagnosticInfo {
                file_path: file_path.to_owned(),
                line,
                column,
                end_line,
                end_column,
                severity: diag.severity.as_wire_str().to_owned(),
                message: diag.message.clone(),
                code: None,
                has_location: !diag.labels.is_empty(),
            }
        })
        .collect()
}

// `build_line_offsets` and `line_col_to_byte_offset_with_offsets` have been
// moved to `reify_types::source_location` so that `reify-eval` can use them
// without depending on `reify-gui`.  Re-export here as `pub(crate)` so all
// existing callers inside this crate (and engine_tests.rs) compile unchanged.
pub(crate) use reify_core::{build_line_offsets, line_col_to_byte_offset_with_offsets};

/// Binary-search for the (line, column) of `offset` using a pre-built newline table.
///
/// `source` is the original source string; `line_offsets` must be the result of
/// [`build_line_offsets`] for the same `source`.  Both line and column are 1-based
/// and count **Unicode codepoints**, matching the semantics of [`reify_types::byte_offset_to_line_col`].
///
/// Line lookup is O(log M).  Column computation is O(line_length) for codepoint
/// counting — far cheaper than the O(M) full-source scan of the naive implementation.
///
/// - If `offset == `[`reify_types::SourceSpan::PRELUDE_SENTINEL_OFFSET`]` (i.e.
///   `u32::MAX as usize`, the [`SourceSpan::prelude()`] sentinel), returns `(1, 1)` —
///   matching `reify_types::byte_offset_to_line_col` so the two convergent
///   implementations agree at the sentinel (cross-validated in `engine_tests.rs`).
///
/// [`SourceSpan::prelude()`]: reify_types::SourceSpan::prelude
pub(crate) fn offset_to_line_col_fast(
    source: &str,
    line_offsets: &[usize],
    offset: usize,
) -> (usize, usize) {
    // Prelude-sentinel early return: SourceSpan::PRELUDE_SENTINEL_OFFSET
    // (u32::MAX as usize) is used by SourceSpan::prelude() to mark spans that
    // have no meaningful byte-offset in the current compilation unit (e.g.
    // cross-prelude collision warnings).  Return (1, 1) — matching
    // reify_types::byte_offset_to_line_col so the two convergent
    // implementations agree at the sentinel.
    if offset == reify_core::SourceSpan::PRELUDE_SENTINEL_OFFSET {
        return (1, 1);
    }
    // Count newlines that appear *strictly before* `offset`.
    let line_idx = line_offsets.partition_point(|&nl| nl < offset);
    let line = line_idx + 1;
    // Byte offset of the first character on this line.
    let line_start = if line_idx == 0 {
        0
    } else {
        line_offsets[line_idx - 1] + 1
    };
    // Clamp offset to source length, then snap to the nearest char boundary
    // (walking backward at most 3 bytes). This guards against non-boundary
    // byte offsets from buggy span generation without panicking.
    let clamped = offset.min(source.len());
    let effective = if source.is_char_boundary(clamped) {
        clamped
    } else {
        (0..clamped)
            .rev()
            .find(|&i| source.is_char_boundary(i))
            .unwrap_or(0)
    };
    // Count codepoints from line_start to effective offset for 1-based column.
    let col = source[line_start..effective].chars().count() + 1;
    (line, col)
}

// ── Task 4087: FEA result-model δ — surface-vertex sampling helpers ───────────

/// Sample a 3D regular-grid sampled field at the nearest grid node.
///
/// Returns a borrowed slice into `sf.data` for the stride-element window at
/// that node as `Some(&[f64])`, or `None` if the point is outside the field
/// bounds (±`tol`) or if the nearest node's window contains any non-finite
/// value (NaN or ±inf — the reify-solver-elastic out-of-solid sentinel).
///
/// Returning a slice (rather than `Vec<f64>`) avoids a heap allocation per
/// vertex lookup; callers consume the window immediately and need no ownership.
///
/// # Layout
///
/// `sf.data` is stored row-major with axis-0 outermost:
/// flat index = `((ix * ny + iy) * nz + iz) * stride`
/// where stride = `data.len() / node_count`.
///
/// # Tolerance
///
/// `tol` is added to the per-axis `[bounds_min, bounds_max]` interval before
/// the bounds check.  Use a small fraction of the minimum grid spacing so that
/// vertices that lie exactly on the boundary (floating-point rounding) are not
/// incorrectly rejected.
pub(crate) fn sample_stride_field_nearest(
    sf: &reify_ir::SampledField,
    point: [f64; 3],
    tol: f64,
) -> Option<&[f64]> {
    // Axis counts (number of nodes per axis).
    let nx = sf.axis_grids[0].len();
    let ny = sf.axis_grids[1].len();
    let nz = sf.axis_grids[2].len();
    let node_count = nx * ny * nz;
    if node_count == 0 {
        return None;
    }
    let stride = sf.data.len() / node_count;
    if stride == 0 {
        return None;
    }

    // Bounds check with tolerance.
    for ((&p, &mn), &mx) in point
        .iter()
        .zip(sf.bounds_min.iter())
        .zip(sf.bounds_max.iter())
    {
        if p < mn - tol || p > mx + tol {
            return None;
        }
    }

    // Nearest-node index per axis: round((c - min) / spacing), clamped to [0, len-1].
    let snap = |c: f64, min: f64, sp: f64, len: usize| -> usize {
        let raw = ((c - min) / sp).round() as isize;
        raw.clamp(0, (len as isize) - 1) as usize
    };

    let ix = snap(point[0], sf.bounds_min[0], sf.spacing[0], nx);
    let iy = snap(point[1], sf.bounds_min[1], sf.spacing[1], ny);
    let iz = snap(point[2], sf.bounds_min[2], sf.spacing[2], nz);

    let flat = ((ix * ny + iy) * nz + iz) * stride;
    let window = &sf.data[flat..flat + stride];

    // Return None if any value in the window is non-finite (NaN or ±inf).
    // NaN is the reify-solver-elastic out-of-solid sentinel; ±inf would also
    // overflow compute_von_mises_3x3 cast-to-f32 and break the FiniteF32MapRef
    // wire guard, so we treat all non-finite values as out-of-solid here.
    if window.iter().any(|v| !v.is_finite()) {
        return None;
    }

    Some(window)
}

/// Sample von Mises stress at the nearest grid node.
///
/// Returns `crate::types::SCALAR_CHANNEL_OOB_SENTINEL` when the point is
/// out-of-bounds, out-of-solid (NaN window), or the stress window has fewer
/// than 9 elements.
pub(crate) fn von_mises_sample(
    stress_sf: &reify_ir::SampledField,
    point: [f64; 3],
    tol: f64,
) -> f32 {
    match sample_stride_field_nearest(stress_sf, point, tol) {
        Some(w) if w.len() >= 9 => reify_stdlib::compute_von_mises_3x3(w) as f32,
        _ => crate::types::SCALAR_CHANNEL_OOB_SENTINEL,
    }
}

/// Sample displaced position at the nearest grid node (warp = 1).
///
/// Returns `[x + dx, y + dy, z + dz]` when the point maps to an in-solid grid
/// node with a stride-≥3 displacement window, or the original `[x, y, z]` cast
/// to f32 when the point is OOB or out-of-solid.
pub(crate) fn displaced_sample(
    disp_sf: &reify_ir::SampledField,
    point: [f64; 3],
    tol: f64,
) -> [f32; 3] {
    match sample_stride_field_nearest(disp_sf, point, tol) {
        Some(w) if w.len() >= 3 => [
            (point[0] + w[0]) as f32,
            (point[1] + w[1]) as f32,
            (point[2] + w[2]) as f32,
        ],
        _ => [point[0] as f32, point[1] as f32, point[2] as f32],
    }
}

/// Sample the a-posteriori error indicator at the nearest grid node (task 3001).
///
/// Mirrors [`von_mises_sample`] but the error indicator is already a scalar
/// (`Field<Point3<Length>, Pressure>`, stride 1), so the raw sample window
/// value is returned directly instead of computing a von-Mises invariant.
///
/// Returns `crate::types::SCALAR_CHANNEL_OOB_SENTINEL` when the point is
/// out-of-bounds, out-of-solid (NaN window), or the window is empty.
pub(crate) fn error_indicator_sample(
    sf: &reify_ir::SampledField,
    point: [f64; 3],
    tol: f64,
) -> f32 {
    match sample_stride_field_nearest(sf, point, tol) {
        Some(w) if !w.is_empty() => w[0] as f32,
        _ => crate::types::SCALAR_CHANNEL_OOB_SENTINEL,
    }
}

/// Extract stress, displacement, and (optional) error-indicator `SampledField`
/// references from a `ValueMap` containing an `ElasticResult` `StructureInstance`.
///
/// Iterates `values` and returns the first entry whose type_name is
/// `"ElasticResult"` and both `"stress"` and `"displacement"` fields resolve
/// to `Value::Field { source: Sampled, lambda: Arc<Value::SampledField(_)> }`.
/// The 3rd tuple element is `Some` when `"error_indicator"` is a populated
/// `Value::Option(Some(Value::Field { source: Sampled, .. }))` (task 3001),
/// and `None` when it is `Value::Option(None)` (the non-adaptive default) or
/// absent.
///
/// Returns `None` if no such result is found or either of stress/displacement
/// is absent/Undef. Mirrors `extract_buckling_data` for the ElasticResult
/// variant. Delegates to `resolve_elastic_result_sampled_fields` for
/// per-value resolution.
pub(crate) fn extract_elastic_result_fields(
    values: &reify_ir::ValueMap,
) -> Option<(
    &reify_ir::SampledField,
    &reify_ir::SampledField,
    Option<&reify_ir::SampledField>,
)> {
    for (_, value) in values.iter() {
        if let Some(triple) = resolve_elastic_result_sampled_fields(value) {
            return Some(triple);
        }
    }
    None
}

/// Returns `true` if `values` contains any top-level `ElasticResult`
/// or any `MultiCaseResult`-shaped cell — indicating a scene with FEA data
/// that warrants caching the tessellation geometry for case-switching.
fn values_have_fea_data(values: &reify_ir::ValueMap) -> bool {
    extract_elastic_result_fields(values).is_some()
        || values.iter().any(|(_, v)| {
            reify_eval::multi_load_dispatch::detect_multi_case_result(v).is_some()
        })
}

/// Extract stress and displacement `SampledField` references from a single
/// `Value::StructureInstance("ElasticResult")` value.
///
/// Returns `None` if the value is not an `ElasticResult` or either `"stress"`/
/// `"displacement"` field is absent or not a `Sampled` `SampledField`. The 3rd
/// tuple element resolves `"error_indicator"` (task 3001) — `Some` only when
/// it is a populated `Value::Option(Some(Value::Field { source: Sampled, .. }))`.
/// Used by both the single-case path (`extract_elastic_result_fields`) and the
/// multi-case path (`try_extract_from_multi_case_cell`).
fn resolve_elastic_result_sampled_fields(
    value: &reify_ir::Value,
) -> Option<(
    &reify_ir::SampledField,
    &reify_ir::SampledField,
    Option<&reify_ir::SampledField>,
)> {
    use reify_ir::{FieldSourceKind, Value};

    let data = match value {
        Value::StructureInstance(d) if d.type_name == "ElasticResult" => d,
        _ => return None,
    };

    let stress_sf = match data.fields.get("stress") {
        Some(Value::Field { source: FieldSourceKind::Sampled, lambda, .. }) => {
            match lambda.as_ref() {
                Value::SampledField(sf) => sf,
                _ => return None,
            }
        }
        _ => return None,
    };
    let disp_sf = match data.fields.get("displacement") {
        Some(Value::Field { source: FieldSourceKind::Sampled, lambda, .. }) => {
            match lambda.as_ref() {
                Value::SampledField(sf) => sf,
                _ => return None,
            }
        }
        _ => return None,
    };
    // error_indicator is Option<Field<Point3<Length>, Pressure>> in the DSL
    // (stdlib/solver_elastic.ri): Value::Option(None) for a non-adaptive solve
    // (elastic_static.rs aposteriori_nonadaptive_default_fields), or
    // Value::Option(Some(Value::Field{source:Sampled, ..})) once populated by
    // the adaptive loop (task 2997). Any other shape (absent key, non-Sampled
    // source) resolves to None here.
    let error_indicator_sf = match data.fields.get("error_indicator") {
        Some(Value::Option(Some(boxed))) => match boxed.as_ref() {
            Value::Field { source: FieldSourceKind::Sampled, lambda, .. } => {
                match lambda.as_ref() {
                    Value::SampledField(sf) => Some(sf),
                    _ => None,
                }
            }
            _ => None,
        },
        _ => None,
    };
    Some((stress_sf, disp_sf, error_indicator_sf))
}

/// Resolve the active case's raw `Value` from a single `Value::Map` cell that
/// carries a `MultiCaseResult` shape (`Map{"cases" -> Map{name -> ElasticResult}}`).
///
/// `active_case` selects which case to use:
/// - `Some(name)` if the name is present in the cases map, otherwise lex-first.
/// - `None` → lex-first (matching `detect_multi_case_result`'s default).
///
/// Returns `None` if `cell_val` is not a `MultiCaseResult` shape or the active
/// case is absent from the cases map. Shared by `try_extract_from_multi_case_cell`
/// (sampled-field resolution) and `resolve_active_elastic_result` (raw-value
/// resolution for `extract_fea_convergence`, task 3001) so both stay in sync.
fn resolve_active_multi_case_value<'a>(
    cell_val: &'a reify_ir::Value,
    active_case: Option<&str>,
) -> Option<&'a reify_ir::Value> {
    use reify_ir::Value;

    // Must be a MultiCaseResult-shaped map.
    let detected = reify_eval::multi_load_dispatch::detect_multi_case_result(cell_val)?;

    // Resolve the case name to use: the requested name if it exists, else lex-first.
    let case_name_to_use: String = match active_case {
        Some(name) if detected.available_cases.contains(&name.to_string()) => {
            name.to_string()
        }
        _ => detected.active_case_id,
    };

    // Navigate into Map{"cases" -> Map{name -> ElasticResult}}.
    let outer = match cell_val {
        Value::Map(m) => m,
        _ => return None,
    };
    let cases_map = match outer.get(&Value::String("cases".to_string())) {
        Some(Value::Map(m)) => m,
        _ => return None,
    };
    cases_map.get(&Value::String(case_name_to_use))
}

/// Try to extract stress/displacement/error-indicator fields from a single
/// `Value::Map` cell that carries a `MultiCaseResult` shape
/// (`Map{"cases" -> Map{name -> ElasticResult}}`).
///
/// `active_case` selects which case's `ElasticResult` to use — see
/// `resolve_active_multi_case_value`.
///
/// Returns `None` if `cell_val` is not a `MultiCaseResult` shape, the active case
/// has no `ElasticResult`, or either `"stress"`/`"displacement"` field is absent/Undef.
fn try_extract_from_multi_case_cell<'a>(
    cell_val: &'a reify_ir::Value,
    active_case: Option<&str>,
) -> Option<(
    &'a reify_ir::SampledField,
    &'a reify_ir::SampledField,
    Option<&'a reify_ir::SampledField>,
)> {
    let case_val = resolve_active_multi_case_value(cell_val, active_case)?;
    // Extract SampledFields from the active case's ElasticResult.
    resolve_elastic_result_sampled_fields(case_val)
}

/// Resolve the active `ElasticResult` `StructureInstance` `Value` from `values`,
/// for callers (e.g. `extract_fea_convergence`, task 3001) that read scalar
/// fields directly rather than sampled stress/displacement data.
///
/// Mirrors `apply_fea_channels`'s source-resolution order:
/// 1. A top-level `Value::StructureInstance("ElasticResult")` (single-case;
///    first match wins, matching `extract_elastic_result_fields`).
/// 2. A `MultiCaseResult`-shaped `Value::Map` cell's active case (via
///    `resolve_active_multi_case_value`).
///
/// Unlike `resolve_elastic_result_sampled_fields`, this does NOT require
/// `"stress"`/`"displacement"` to be valid `Sampled` fields — only that the
/// value's `type_name == "ElasticResult"`.
fn resolve_active_elastic_result<'a>(
    values: &'a reify_ir::ValueMap,
    active_case: Option<&str>,
) -> Option<&'a reify_ir::Value> {
    use reify_ir::Value;

    for (_, value) in values.iter() {
        if let Value::StructureInstance(d) = value
            && d.type_name == "ElasticResult"
        {
            return Some(value);
        }
    }

    // Multi-case fallback: validate the resolved case is an ElasticResult
    // StructureInstance, matching the top-level branch above and
    // `resolve_elastic_result_sampled_fields`'s type_name check (task 3001
    // amendment) — otherwise a MultiCaseResult case cell holding an unrelated
    // StructureInstance that happens to carry a `convergence_status`-shaped
    // field could be misread by `extract_fea_convergence`.
    values.iter().find_map(|(_, cell_val)| {
        let case_val = resolve_active_multi_case_value(cell_val, active_case)?;
        match case_val {
            Value::StructureInstance(d) if d.type_name == "ElasticResult" => Some(case_val),
            _ => None,
        }
    })
}

/// Extract the a-posteriori convergence status of the active `ElasticResult`
/// (task 3001), surfaced as `GuiState.fea_convergence`.
///
/// Reads `"convergence_status"` (the `ConvergenceStatus` DCE enum —
/// `Converged{final_indicator}` / `NotConverged{reason:BudgetReason}`,
/// mirroring `elastic_static.rs::aposteriori_nonadaptive_default_fields`):
/// - `"Converged"` → `FeaConvergenceInfo{converged:true, reason:None}`.
/// - `"NotConverged"` → `FeaConvergenceInfo{converged:false, reason:Some(name)}`,
///   `name` being the `BudgetReason` payload's variant name (`None` if the
///   payload is missing or malformed).
///
/// Returns `None` when no `ElasticResult` is found (active or otherwise) or
/// `"convergence_status"` is absent or not the expected `Value::Enum` shape.
pub(crate) fn extract_fea_convergence(
    values: &reify_ir::ValueMap,
    active_case: Option<&str>,
) -> Option<crate::types::FeaConvergenceInfo> {
    use reify_ir::Value;

    let data = match resolve_active_elastic_result(values, active_case)? {
        Value::StructureInstance(d) => d,
        _ => return None,
    };
    match data.fields.get("convergence_status") {
        Some(Value::Enum { variant, .. }) if variant == "Converged" => {
            Some(crate::types::FeaConvergenceInfo { converged: true, reason: None })
        }
        Some(Value::Enum { variant, payload, .. }) if variant == "NotConverged" => {
            let reason = payload.iter().find_map(|(name, v)| {
                if name != "reason" {
                    return None;
                }
                match v {
                    Value::Enum { variant, .. } => Some(variant.clone()),
                    _ => None,
                }
            });
            Some(crate::types::FeaConvergenceInfo { converged: false, reason })
        }
        _ => None,
    }
}

/// Fill per-vertex FEA scalar/displacement channels on all meshes.
///
/// `active_case` selects which case to render for multi-case scenes:
/// - `None` (or an unknown name) → lex-first case, matching the
///   `detect_multi_case_result` default.
/// - `Some(name)` → that case's `ElasticResult`, if present; falls back to
///   lex-first when the name is absent from the cases map.
///
/// **Source resolution order** (first match wins):
/// 1. A top-level `Value::StructureInstance("ElasticResult")` in `values`
///    (the single-case path, unchanged from task 4087).
/// 2. A `MultiCaseResult`-shaped `Value::Map` cell (`Map{"cases" -> Map{…}}`),
///    where the active case's value is a `Value::StructureInstance("ElasticResult")`.
///
/// If no `ElasticResult` is found via either path, the meshes are left untouched
/// (non-FEA meshes keep empty `scalar_channels` and `None` `displaced_positions`).
///
/// Per-vertex channels set when an `ElasticResult` is found:
/// - `mesh.scalar_channels["vonMises"]` (length = vertex_count): von-Mises stress
///   sampled at each vertex; OOB/out-of-solid vertices receive
///   `SCALAR_CHANNEL_OOB_SENTINEL`.
///
/// Both `"vonMises"` and (when populated) `"errorIndicator"` are stamped into
/// `mesh.scalar_channel_tags` as
/// [`ScalarChannelTag::pressure()`](crate::types::ScalarChannelTag::pressure) —
/// unit [`PRESSURE_CHANNEL_UNIT`](crate::types::PRESSURE_CHANNEL_UNIT), unsigned.
/// That is the declared GUI-boundary unit for these channels (the runtime
/// `SampledField` carries no dimension, so it cannot be derived); `unsigned` is
/// honest because both samplers return a norm — √(a quadratic form) and a field
/// norm respectively — or exactly `SCALAR_CHANNEL_OOB_SENTINEL`.
/// - `mesh.displaced_positions` (length = `vertices.len()`): vertex positions
///   plus warp = 1 displacement; OOB/out-of-solid vertices keep their original
///   position.
///
/// The sampling tolerance is 1% of the minimum grid spacing (or 1e-9 if spacing
/// cannot be determined), so that surface vertices lying exactly on the field
/// boundary are not misclassified as OOB due to floating-point rounding.
pub(crate) fn apply_fea_channels(
    meshes: &mut [crate::types::MeshData],
    values: &reify_ir::ValueMap,
    active_case: Option<&str>,
) {
    // Try single-case path first (top-level ElasticResult).
    // If not found, try multi-case path (MultiCaseResult cell).
    // `error_indicator_sf` (task 3001 step-4): Some when the ElasticResult
    // carries a populated a-posteriori error indicator field; wired into a
    // per-vertex `scalar_channels["errorIndicator"]` entry below.
    let (stress_sf, disp_sf, error_indicator_sf) =
        if let Some(triple) = extract_elastic_result_fields(values) {
            triple
        } else {
            // Scan all cells for the first MultiCaseResult-shaped value.
            let multi_triple = values.iter().find_map(|(_, cell_val)| {
                try_extract_from_multi_case_cell(cell_val, active_case)
            });
            match multi_triple {
                Some(triple) => triple,
                None => return,
            }
        };

    // Tolerance: 1% of the minimum grid spacing (or a small absolute fallback).
    let min_spacing = stress_sf
        .spacing
        .iter()
        .chain(disp_sf.spacing.iter())
        .cloned()
        .filter(|s| s.is_finite() && *s > 0.0)
        .fold(f64::MAX, f64::min);
    let tol = if min_spacing < f64::MAX { min_spacing * 0.01 } else { 1e-9 };

    for mesh in meshes.iter_mut() {
        let vertex_count = mesh.vertices.len() / 3;
        let mut vm_vec: Vec<f32> = Vec::with_capacity(vertex_count);
        let mut disp_vec: Vec<f32> = Vec::with_capacity(mesh.vertices.len());
        let mut ei_vec: Vec<f32> = if error_indicator_sf.is_some() {
            Vec::with_capacity(vertex_count)
        } else {
            Vec::new()
        };

        for chunk in mesh.vertices.chunks_exact(3) {
            let point = [chunk[0] as f64, chunk[1] as f64, chunk[2] as f64];
            vm_vec.push(von_mises_sample(stress_sf, point, tol));
            let [dx, dy, dz] = displaced_sample(disp_sf, point, tol);
            disp_vec.push(dx);
            disp_vec.push(dy);
            disp_vec.push(dz);
            if let Some(eind_sf) = error_indicator_sf {
                ei_vec.push(error_indicator_sample(eind_sf, point, tol));
            }
        }

        mesh.scalar_channels.insert("vonMises".to_string(), vm_vec);
        mesh.scalar_channel_tags.insert(
            "vonMises".to_string(),
            crate::types::ScalarChannelTag::pressure(),
        );
        mesh.displaced_positions = Some(disp_vec);
        // The tag is stamped INSIDE this conditional, not beside it: a tag for a
        // channel that was never inserted is an orphan and hard-fails
        // MeshData::serialize.
        if error_indicator_sf.is_some() {
            mesh.scalar_channels.insert("errorIndicator".to_string(), ei_vec);
            mesh.scalar_channel_tags.insert(
                "errorIndicator".to_string(),
                crate::types::ScalarChannelTag::pressure(),
            );
        }
    }
}

/// Match a tessellated `MeshData.entity_path` against a shell view's template
/// `entity_path`.
///
/// The tessellation path carries a `#realization[N]` suffix
/// (`RealizationNodeId` Display form, e.g. `"FeaShellFlexure#realization[0]"`),
/// while the engine-side accessor keys its view by the bare compute-node entity
/// (`"FeaShellFlexure"`). Comparing the prefix before the first `#` on BOTH
/// sides reconciles them (and degrades to plain equality when neither carries a
/// suffix), so the populator binds to the right body instead of silently
/// no-op-ing.
fn shell_entity_matches(mesh_path: &str, view_path: &str) -> bool {
    fn template(p: &str) -> &str {
        p.split('#').next().unwrap_or(p)
    }
    template(mesh_path) == template(view_path)
}

/// Build the per-face **identity** element-index vector `[0, 1, …, face_count-1]`.
///
/// Each entry maps a surface triangle to the element id that owns it. For
/// shell bodies every mid-surface triangle is its own shell element, so the
/// identity mapping is exact. The vector length equals `face_count`, which is
/// the same invariant enforced by the `MeshData` manual `Serialize` impl for
/// the `element_index` field (`len == indices.len()/3`).
fn identity_element_index(face_count: usize) -> Vec<u32> {
    (0..face_count as u32).collect()
}

/// Populate shell-extract MeshData channels from the engine-side
/// [`reify_eval::ShellGuiMeshData`] views produced by
/// [`reify_eval::Engine::shell_gui_mesh_data`] (PRD
/// `docs/prds/v0_4/shell-extract-engine-bridge.md` §9 Phase 6 task θ).
///
/// For each view, the [`MeshData`](crate::types::MeshData) whose `entity_path`
/// matches (by [`shell_entity_matches`]) has the shell representation installed:
///
/// - `vertices` / `indices` are **replaced** by the view's extraction
///   mid-surface mesh. Per PRD §11 OQ-2 the v0.4 stress solver's internal
///   flat-plate mesh ≠ the extraction mid-surface, so the displayed shell uses
///   the mid-surface geometry; this also makes every length contract close by
///   construction (`region_tags` / `element_kind` are per-mid-triangle, the
///   recovered von Mises is per-mid-vertex).
/// - `element_kind` = `view.element_kind` (all `1` = shell triangle),
///   `region_tags` = `view.region_tags` (`SegmentationResult` labels).
/// - `scalar_channels` gains `vonMises_top` / `vonMises_mid` /
///   `vonMises_bottom` (recovered per-vertex; `len == vertex_count`), each
///   stamped into `scalar_channel_tags` as
///   [`ScalarChannelTag::pressure()`](crate::types::ScalarChannelTag::pressure)
///   — unit [`PRESSURE_CHANNEL_UNIT`](crate::types::PRESSURE_CHANNEL_UNIT),
///   unsigned, since recovered von Mises is a norm.
/// - `vector_channels` gains `shell_normal_per_face` — the
///   [`PER_FACE_CHANNEL_SUFFIX`](crate::types::PER_FACE_CHANNEL_SUFFIX) makes
///   the serialize-time length check use `3 * face_count`.
///
/// Non-matching meshes (tet / non-FEA bodies) are left untouched. The accessor
/// returns an empty slice for non-shell scenes, so this is a no-op there.
pub(crate) fn apply_shell_channels(
    meshes: &mut [crate::types::MeshData],
    views: &[reify_eval::ShellGuiMeshData],
) {
    for view in views {
        let Some(mesh) = meshes
            .iter_mut()
            .find(|m| shell_entity_matches(&m.entity_path, &view.entity_path))
        else {
            continue;
        };

        // Swap the displayed solid tessellation for the extraction mid-surface
        // so the per-triangle / per-vertex shell channels line up exactly.
        mesh.vertices = view.vertices.clone();
        mesh.indices = view.indices.clone();
        mesh.element_kind = Some(view.element_kind.clone());
        mesh.region_tags = Some(view.region_tags.clone());
        // task #4883: populate per-face identity element_index for shell bodies.
        // Each extracted mid-surface triangle is its own shell element, so its
        // element id is its face index (0..face_count). Computed locally from
        // view.indices to avoid extending ShellGuiMeshData / reify-eval.
        mesh.element_index = Some(identity_element_index(view.indices.len() / 3));

        mesh.scalar_channels
            .insert("vonMises_top".to_string(), view.von_mises_top.clone());
        mesh.scalar_channels
            .insert("vonMises_mid".to_string(), view.von_mises_mid.clone());
        mesh.scalar_channels
            .insert("vonMises_bottom".to_string(), view.von_mises_bottom.clone());
        for key in ["vonMises_top", "vonMises_mid", "vonMises_bottom"] {
            mesh.scalar_channel_tags
                .insert(key.to_string(), crate::types::ScalarChannelTag::pressure());
        }

        mesh.vector_channels.insert(
            format!("shell_normal{}", crate::types::PER_FACE_CHANNEL_SUFFIX),
            view.shell_normals_per_face.clone(),
        );
    }
}

// ── Unit tests for extract_display_style_data (Value::Scalar branch) ──────────

#[cfg(test)]
mod display_style_extract_tests {
    //! Unit tests for `extract_display_style_data` in isolation.
    //!
    //! DSL-based integration tests (engine_tests.rs) exercise only `Value::Real`
    //! literals.  These inline tests construct `Value::StructureInstance` objects
    //! directly to cover the `Value::Scalar` (dimensionless) branch of the nested
    //! `to_f32` helper — a regression in that branch would otherwise pass CI silently.

    use super::extract_display_style_data;
    use reify_core::DimensionVector;
    use reify_ir::{PersistentMap, StructureInstanceData, StructureTypeId, Value};

    /// Build a `Value::StructureInstance` from a list of `(name, value)` pairs.
    fn make_si(type_name: &str, pairs: &[(&str, Value)]) -> Value {
        let fields: PersistentMap<String, Value> =
            pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        Value::StructureInstance(Box::new(StructureInstanceData {
            type_id: StructureTypeId(0),
            type_name: type_name.to_string(),
            version: 0,
            fields,
        }))
    }

    /// Exercises the `Value::Scalar{si_value}` (dimensionless) branch of `to_f32`.
    ///
    /// In the Reify evaluator a `Real`-typed param can evaluate to either
    /// `Value::Real` or a dimensionless `Value::Scalar` (e.g. when the call-site
    /// expression is SI-unit arithmetic that cancels to dimensionless).
    /// This test pins the Scalar branch explicitly: if `to_f32` were changed to
    /// only handle `Value::Real`, the color/opacity fields would silently become 0.0
    /// instead of the actual values — this test makes that regression visible.
    #[test]
    fn extract_display_style_data_tolerates_dimensionless_scalar_fields() {
        let dim = DimensionVector::DIMENSIONLESS;
        let scalar = |v: f64| Value::Scalar { si_value: v, dimension: dim };

        // Color(r=0.8, g=0.6, b=0.4) expressed via dimensionless Scalar values.
        let color_si = make_si(
            "Color",
            &[("r", scalar(0.8)), ("g", scalar(0.6)), ("b", scalar(0.4))],
        );
        // DisplayStyle(color=..., opacity=0.75) with Scalar opacity + Color.
        let style_si = make_si(
            "DisplayStyle",
            &[
                ("color", color_si),
                ("opacity", scalar(0.75)),
                ("finish", Value::Enum { type_name: "Finish".to_string(), variant: "Gloss".to_string(), payload: vec![] }),
                ("wireframe", Value::Bool(false)),
            ],
        );
        // Wrap in a minimal DisplayOutput StructureInstance.
        let display_output = make_si("DisplayOutput", &[("style", style_si)]);

        let result = extract_display_style_data(&display_output);

        let eps = 1e-4_f32;
        assert!(
            (result.color[0] - 0.8_f32).abs() < eps,
            "r from Scalar: expected ~0.8, got {}",
            result.color[0]
        );
        assert!(
            (result.color[1] - 0.6_f32).abs() < eps,
            "g from Scalar: expected ~0.6, got {}",
            result.color[1]
        );
        assert!(
            (result.color[2] - 0.4_f32).abs() < eps,
            "b from Scalar: expected ~0.4, got {}",
            result.color[2]
        );
        // color[3] = opacity
        assert!(
            (result.color[3] - 0.75_f32).abs() < eps,
            "opacity (color[3]) from Scalar: expected ~0.75, got {}",
            result.color[3]
        );
        assert_eq!(result.finish, 2u8, "Gloss must map to finish==2");
        assert!(!result.wireframe, "wireframe must be false");
    }
}

// ── Unit tests for format_expr (constraint pretty-printer) ────────────────

#[cfg(test)]
mod format_expr_tests {
    //! Unit tests for the private `format_expr` constraint pretty-printer.
    //!
    //! DSL-based integration tests (engine_tests.rs) exercise `format_expr`
    //! only indirectly, through full constraint strings. These inline tests
    //! build `CompiledExpr` fixtures directly to pin three specific defects:
    //! (1) member access — lowered to `IndexAccess` with a string-literal
    //! index — was rendering as `obj[index]` instead of `obj.index`; (2)
    //! unit-bearing literals were rendering with no space and the bare "SI"
    //! fallback instead of a space plus the composed base-unit label; and
    //! (3) a genuine `Map` lookup keyed by a non-identifier string (which
    //! lowers to the same `IndexAccess` shape as member access) was
    //! mis-rendered as invalid dotted syntax instead of a quoted bracket.

    use super::format_expr;
    use reify_core::{DimensionVector, Type, ValueCellId};
    use reify_ir::{BinOp, CompiledExpr, Value};

    /// Build the `IndexAccess` node member access lowers to: `material.density`.
    fn material_density_access() -> CompiledExpr {
        let object = CompiledExpr::value_ref(
            ValueCellId::new("material_1", "material"),
            Type::dimensionless_scalar(),
        );
        let index = CompiledExpr::literal(Value::String("density".to_string()), Type::String);
        CompiledExpr::index_access(object, index, Type::dimensionless_scalar())
    }

    #[test]
    fn index_access_with_string_literal_index_renders_as_member_access() {
        assert_eq!(format_expr(&material_density_access()), "material.density");
    }

    #[test]
    fn index_access_with_numeric_index_keeps_bracket_form() {
        let object = CompiledExpr::value_ref(
            ValueCellId::new("arr_1", "arr"),
            Type::dimensionless_scalar(),
        );
        let index = CompiledExpr::literal(Value::Int(0), Type::Int);
        let expr = CompiledExpr::index_access(object, index, Type::dimensionless_scalar());

        assert_eq!(format_expr(&expr), "arr[0]");
    }

    /// A `Map<String, _>` lookup by a non-identifier key lowers to the same
    /// `IndexAccess { index: Literal(String(..)) }` shape as member access,
    /// but must not render as invalid dotted syntax (`config.max load`).
    #[test]
    fn index_access_with_non_identifier_string_key_keeps_quoted_bracket_form() {
        let object = CompiledExpr::value_ref(
            ValueCellId::new("config_1", "config"),
            Type::dimensionless_scalar(),
        );
        let index = CompiledExpr::literal(Value::String("max load".to_string()), Type::String);
        let expr = CompiledExpr::index_access(object, index, Type::dimensionless_scalar());

        assert_eq!(format_expr(&expr), "config[\"max load\"]");
    }

    #[test]
    fn index_access_with_empty_string_key_keeps_quoted_bracket_form() {
        let object = CompiledExpr::value_ref(
            ValueCellId::new("config_1", "config"),
            Type::dimensionless_scalar(),
        );
        let index = CompiledExpr::literal(Value::String(String::new()), Type::String);
        let expr = CompiledExpr::index_access(object, index, Type::dimensionless_scalar());

        assert_eq!(format_expr(&expr), "config[\"\"]");
    }

    #[test]
    fn dimensioned_literal_in_binop_renders_space_and_composed_unit() {
        let right = CompiledExpr::literal(
            Value::Scalar {
                si_value: 0.0,
                dimension: DimensionVector::MASS_DENSITY,
            },
            Type::Scalar {
                dimension: DimensionVector::MASS_DENSITY,
            },
        );
        let expr = CompiledExpr::binop(
            BinOp::Gt,
            material_density_access(),
            right,
            Type::dimensionless_scalar(),
        );

        assert_eq!(format_expr(&expr), "material.density > 0 kg\u{b7}m^-3");
    }

    #[test]
    fn dimensionless_literal_in_binop_has_no_trailing_space() {
        let left = CompiledExpr::value_ref(
            ValueCellId::new("x_1", "x"),
            Type::dimensionless_scalar(),
        );
        let right = CompiledExpr::literal(Value::Real(0.0), Type::dimensionless_scalar());
        let expr = CompiledExpr::binop(BinOp::Gt, left, right, Type::dimensionless_scalar());

        assert_eq!(format_expr(&expr), "x > 0");
    }
}

