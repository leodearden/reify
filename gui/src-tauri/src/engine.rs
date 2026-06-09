// EngineSession — wraps Engine + CompiledModule + source text

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use tracing::warn;

use reify_compiler::{CompiledModule, ValueCellKind, Visibility};
use reify_eval::cache::NodeId;
use reify_eval::{CancellationHandle, CheckResult, Engine};
use reify_core::{ContentHash, DimensionVector, ModulePath, Severity, ValueCellId};
use reify_ir::{ConstraintChecker, DeterminacyState, ExportFormat, GeometryKernel, Satisfaction, Value};

#[cfg(test)]
use reify_ir::ConstraintSolver;

use reify_core::{Diagnostic, DiagnosticInfo, DiagnosticLabel, SourceLocationInfo, SourceSpan};

use crate::types::{
    AutoResolveConstraintProgress, AutoResolveIteration, AutoResolveParameterValue, ConstraintData,
    DefInfo, EntityIdentity, EntityTreeNode, FileData, GuiState, JointBinding, JointDescriptor,
    MechanismDescriptor, MeshData, SourceSpanInfo, TensegrityWireData, ValueData,
    format_determinacy, format_freshness, format_value,
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
    let compiled = reify_compiler::compile_with_stdlib(&parsed);
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
    let mut dag = reify_compiler::module_dag::ModuleDag::new();

    // Collect import paths from the parsed module (top-level Import declarations only).
    let import_paths: Vec<String> = parsed
        .declarations
        .iter()
        .filter_map(|decl| {
            if let reify_ast::Declaration::Import(imp) = decl {
                Some(imp.path.clone())
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
    let mut compiled = reify_compiler::compile_with_prelude_context(&parsed, &ctx);

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
    // Visibility filter: only Visibility::Public templates are merged.  Private
    // structures from imported modules must not be reachable to the eval engine,
    // mirroring compile-time import semantics.
    //
    // Std.* modules are excluded: stdlib structures are not expected to appear as
    // top-level GUI entities.
    //
    // De-duplication: first-wins/warns — skip any imported template whose name
    // already exists in `compiled.templates` (either declared by the entry or
    // merged from an earlier import), and emit a Diagnostic::warning so the user
    // sees the shadowing instead of a silent skip.  Mirrors the compiler's
    // cross-prelude alias collision policy — see the `pub_alias_collision_warnings`
    // loop inside `compile_with_prelude_context` in reify-compiler/src/lib.rs.
    //
    // `templates_origin` maps each template name to the module path that first
    // declared it.  It is pre-seeded with the entry's already-compiled templates
    // (origin = module_name) before the import loop runs, so entry-vs-import
    // collisions also emit a Warning naming both sides.
    //
    // v1 limitation: only DIRECT imports are merged.  If helper.ri itself imports
    // util.ri, Util's template will be absent from this list and find_template will
    // fail at eval for any sub referencing Util.  A future fix should iterate all
    // dag.modules entries instead of just import_paths.
    let mut templates_origin: HashMap<String, String> = HashMap::new();
    // Pre-seed with entry-declared templates so entry-vs-import collisions are
    // detected and warned, mirroring the import-vs-import path below.
    for tmpl in &compiled.templates {
        templates_origin.insert(tmpl.name.clone(), module_name.to_string());
    }
    for import_path in &import_paths {
        if is_stdlib_path(import_path) {
            continue;
        }
        if let Some(imported_module) = dag.modules.get(import_path) {
            for template in &imported_module.templates {
                if template.visibility != Visibility::Public {
                    continue;
                }
                if let Some(prior_origin) = templates_origin.get(&template.name) {
                    // Collision: emit a warning naming both the prior declarer and the
                    // colliding import, mirroring the `pub_alias_collision_warnings`
                    // wording inside `compile_with_prelude_context`.
                    //
                    // The `templates_origin` invariant guarantees every name present in
                    // `compiled.templates` is also present in the map (seeded from entry
                    // templates before the loop, updated on every successful merge), so
                    // `if let Some(...)` is both the O(1) membership test and the origin
                    // lookup — no separate `iter().any(...)` scan or fallback needed.
                    compiled.diagnostics.push(
                        Diagnostic::warning(format!(
                            "imported pub structure '{}' declared in both '{}' and '{}'; first-wins",
                            template.name, prior_origin, import_path
                        ))
                        .with_label(DiagnosticLabel::new(
                            SourceSpan::prelude(),
                            "cross-import collision",
                        )),
                    );
                    continue;
                }
                compiled.templates.push(template.clone());
                templates_origin.insert(template.name.clone(), import_path.clone());
            }
        }
    }

    Ok((compiled, parsed))
}

impl EngineSession {
    /// Shared field-initializer from a pre-constructed `Engine`.
    ///
    /// Both `new` and `with_registered_kernel` delegate here so the field list
    /// stays in one place and the two constructors cannot drift.
    ///
    /// CRITICAL: `register_compute_fns` is called HERE (once) rather than in
    /// `new` or `with_registered_kernel` individually.  Both public constructors
    /// delegate to this method (`new` → `from_engine(Engine::new(..))`,
    /// `with_registered_kernel` → `from_engine(Engine::with_registered_kernel(..))`),
    /// so registering here covers both paths.  `register_compute_fns` **panics on
    /// duplicate registration** (compute_targets/mod.rs:89); calling it in `new`
    /// *and* here would register twice on the same `Engine` → guaranteed panic.
    /// PRD §4.5 / esc-2962-66 root cause.
    fn from_engine(mut engine: Engine) -> Self {
        // Install FEA / buckling / modal compute trampolines once at session
        // construction.  This is the single registration site — see doc above.
        reify_eval::compute_targets::register_compute_fns(&mut engine);
        // Register the shell-extract trampoline so shell-classified bodies
        // can produce a ShellExtractionResult (task θ / #3598 pre-1).
        reify_eval::register_shell_extract_compute_fns(&mut engine);

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
            mode_shape_frame_emitter: None,
            solve_cancel_sink: None,
            solver_progress_sink: None,
            last_reload_error: None,
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
    /// After installation, every `emit_fea_case_if_any` call (co-located with
    /// `emit_auto_resolve_if_any` at all 4 production sites + the test helper)
    /// fires `changed(FeaCaseChanged)` when a `MultiCaseResult`-shaped value is
    /// detected in `CheckResult.values`. Replaces any previously installed emitter.
    pub fn set_fea_case_emitter(&mut self, emitter: Arc<dyn FeaCaseEmitter>) {
        self.fea_case_emitter = Some(emitter);
    }

    /// Install a mode-shape-frame event emitter on this session.
    ///
    /// After installation, every `emit_mode_shape_frames_if_any` call fires
    /// `frame(ModeShapeFrame)` when a `BucklingResult`-shaped value is detected
    /// in `CheckResult.values`. Replaces any previously installed emitter.
    pub fn set_mode_shape_frame_emitter(&mut self, emitter: Arc<dyn ModeShapeFrameEmitter>) {
        self.mode_shape_frame_emitter = Some(emitter);
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

    /// Run `engine.check(compiled)`, fire the emit-helper.
    ///
    /// Gives tests a single-call path that exercises the eval+emit pipeline without
    /// going through the full load_from_source / update_source plumbing.  Only for
    /// unit tests; not callable from production code.
    ///
    /// `CheckResult` does not implement `Clone`, so `last_check` is not updated by
    /// this helper (the test only cares about emitted events, not stored state).
    #[cfg(test)]
    pub(crate) fn check_and_emit_for_test(&mut self, compiled: &CompiledModule) {
        let r = self.core.engine_mut().check(compiled);
        self.emit_auto_resolve_if_any(&r);
        self.emit_fea_case_if_any(&r);
        self.emit_mode_shape_frames_if_any(&r);
        self.drain_and_emit_warm_pool_events();
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
    /// Called after each engine call site that may produce donations or
    /// evictions (check, edit_check, build, tessellate_snapshot, etc.) — the
    /// same sites that invoke [`Self::emit_auto_resolve_if_any`].
    ///
    /// When no emitter is installed, the drain still records events on the
    /// journal (M-010 wiring) but no IPC emission occurs.
    ///
    /// # Design note (follow-up opportunity)
    ///
    /// The five call sites that pair `emit_auto_resolve_if_any` + this method
    /// are shaping into a "post-engine-call telemetry drain" pattern.  A future
    /// refactor could extract a single `post_engine_call_telemetry(&self, check:
    /// &CheckResult)` helper so new engine entry points can't forget to drain
    /// warm-pool events and silently lose telemetry.  Tracked in task review
    /// suggestion #4 (task 3541 amendment pass).
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
        self.emit_auto_resolve_if_any(self.core.last_check().expect(
            "emit_auto_resolve_if_any: last_check must be Some after commit_state — see ordering invariant",
        ));
        self.emit_fea_case_if_any(self.core.last_check().expect(
            "emit_fea_case_if_any: last_check must be Some after commit_state — see ordering invariant",
        ));
        self.emit_mode_shape_frames_if_any(self.core.last_check().expect(
            "emit_mode_shape_frames_if_any: last_check must be Some after commit_state — see ordering invariant",
        ));
        self.drain_and_emit_warm_pool_events();

        self.build_gui_state()
    }

    /// Set a parameter value by cell ID string and value string.
    ///
    /// `cell_id_str` is "Entity.member" (e.g., "Bracket.width").
    /// `value_str` is a quantity literal (e.g., "120mm"), plain number, or boolean.
    pub fn set_parameter(
        &mut self,
        cell_id_str: &str,
        value_str: &str,
    ) -> Result<GuiState, String> {
        let cell_id = parse_cell_id(cell_id_str)?;
        let value = parse_value_string(value_str)?;

        // Validate cell exists in compiled module
        let compiled = self
            .core
            .compiled()
            .ok_or_else(|| "No module loaded".to_string())?;
        let cell_exists = compiled
            .templates
            .iter()
            .any(|t| t.value_cells.iter().any(|vc| vc.id == cell_id));
        if !cell_exists {
            return Err(format!("Unknown parameter '{}'", cell_id_str));
        }

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
        self.emit_auto_resolve_if_any(self.core.last_check().expect(
            "emit_auto_resolve_if_any: last_check must be Some after commit_check — see ordering invariant",
        ));
        self.emit_fea_case_if_any(self.core.last_check().expect(
            "emit_fea_case_if_any: last_check must be Some after commit_check — see ordering invariant",
        ));
        self.emit_mode_shape_frames_if_any(self.core.last_check().expect(
            "emit_mode_shape_frames_if_any: last_check must be Some after commit_check — see ordering invariant",
        ));
        self.drain_and_emit_warm_pool_events();
        self.build_gui_state()
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
        self.emit_auto_resolve_if_any(self.core.last_check().expect(
            "emit_auto_resolve_if_any: last_check must be Some after commit_state — see ordering invariant",
        ));
        self.emit_fea_case_if_any(self.core.last_check().expect(
            "emit_fea_case_if_any: last_check must be Some after commit_state — see ordering invariant",
        ));
        self.emit_mode_shape_frames_if_any(self.core.last_check().expect(
            "emit_mode_shape_frames_if_any: last_check must be Some after commit_state — see ordering invariant",
        ));
        self.drain_and_emit_warm_pool_events();
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
        self.emit_auto_resolve_if_any(self.core.last_check().expect(
            "emit_auto_resolve_if_any: last_check must be Some after commit_state — see ordering invariant",
        ));
        self.emit_fea_case_if_any(self.core.last_check().expect(
            "emit_fea_case_if_any: last_check must be Some after commit_state — see ordering invariant",
        ));
        self.emit_mode_shape_frames_if_any(self.core.last_check().expect(
            "emit_mode_shape_frames_if_any: last_check must be Some after commit_state — see ordering invariant",
        ));
        self.drain_and_emit_warm_pool_events();

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
            });
        }

        // Build values and constraints via shared helpers (also used by
        // build_preview_gui_state) so both paths stay in sync.  Scoped block so
        // the immutable borrows on `compiled` and `check` are released before the
        // mutable engine borrow in the tessellation step below.
        let (values, constraints) = {
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

        let (meshes, tessellation_diagnostics) = match tess_result {
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
                let mut meshes: Vec<MeshData> = result
                    .meshes
                    .into_iter()
                    .map(|surface| MeshData {
                        entity_path: surface.entity_path,
                        vertices: surface.mesh.vertices,
                        indices: surface.mesh.indices,
                        normals: surface.mesh.normals,
                        scalar_channels: std::collections::HashMap::new(),
                        displaced_positions: None,
                        element_kind: None,
                        region_tags: None,
                        vector_channels: std::collections::HashMap::new(),
                    })
                    .collect();
                // Populate per-vertex FEA scalar/displacement channels when an
                // ElasticResult is present in the evaluated values.  The helper
                // returns early when no ElasticResult is found (negligible
                // overhead: one ValueMap scan), so non-FEA scenes pay no
                // tessellation-path cost.
                if let Some(check) = self.core.last_check() {
                    apply_fea_channels(&mut meshes, &check.values, None);
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
                (meshes, tess_diags)
            }
            None => (Vec::new(), Vec::new()),
        };

        // Build files from the last-good source_map.
        let mut files: Vec<FileData> = self
            .core
            .source_map()
            .iter()
            .map(|(path, content)| FileData {
                path: path.clone(),
                content: content.clone(),
            })
            .collect();

        // One-snapshot invariant (task 4258): when a LiveEdit failure is stored,
        // override the matching files entry's content with the failing buffer so
        // `files[].content` and `compile_diagnostics` are computed from the same
        // source snapshot.  meshes/values/get_source_location intentionally stay
        // last-good (they describe the last successfully compiled module) — only
        // the SOURCE text is retargeted to the failing buffer.
        //
        // The `else` branch handles the unlikely edge case where the failing file
        // was not present in source_map (e.g. a brand-new file on first load
        // without a prior success for that key) — push a new entry so diagnostics
        // can always be indexed against a source.
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

        // Collect compile diagnostics (errors, warnings, info) from the most
        // recently compiled module. Called after tessellate_snapshot so the
        // mutable engine borrow is already released.  Takes &self — coexists
        // safely with the existing immutable borrows of compiled/check/files.
        //
        // Also append any live compile failures (from a failed live edit while a
        // prior good compile was still in `self.compiled`).  Appending rather than
        // replacing preserves warnings/info from the last good state; Error entries
        // from a `LiveEdit` failure follow them, so frontends sorting by severity
        // will surface errors first.  Only `LiveEdit` failures reach this branch
        // (a `ColdStart` failure is stored only when `compiled` is `None`, which
        // short-circuits above — so here `compiled` is `Some` and any stored failure
        // is `LiveEdit`).
        let mut compile_diagnostics = self.get_diagnostics();
        if let Some(f) = &self.compile_failure
            && f.kind == CompileFailureKind::LiveEdit
        {
            compile_diagnostics.extend(f.diags.iter().cloned());
        }
        // Synthesize a reload-error DiagnosticInfo when the session is stale due
        // to a hot-reload failure that did NOT produce a structured compile_failure
        // (i.e. the check()-panic path where compile_failure is None).  Gating on
        // `compile_failure.is_none()` avoids double-reporting: in the compile-error
        // path both `compile_failure` (structured diags) and `last_reload_error`
        // (joined message) are set; the structured diags already reach the frontend
        // via the LiveEdit append above, so adding the message again would duplicate.
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
            });
        }

        // Extract tensegrity wire descriptors from value cells.
        // Scoped borrow released before GuiState construction.
        let tensegrity_wires = {
            let compiled = self.core.compiled().unwrap();
            let check = self.core.last_check().unwrap();
            build_tensegrity_wires(compiled, check)
        };

        Ok(GuiState {
            meshes,
            values,
            constraints,
            files,
            tessellation_diagnostics,
            compile_diagnostics,
            tensegrity_wires,
        })
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
fn build_values(
    compiled: &reify_compiler::CompiledModule,
    check: &CheckResult,
    engine: Option<&Engine>,
) -> Vec<ValueData> {
    let mut values = Vec::new();
    for template in &compiled.templates {
        for cell in &template.value_cells {
            let val = check.values.get_or_undef(&cell.id);
            let (formatted_value, unit) = format_value(&val);
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
            values.push(ValueData {
                cell_id: cell.id.to_string(),
                name: cell.id.member.clone(),
                value: formatted_value,
                unit,
                determinacy: format_determinacy(determinacy),
                entity_path: cell.id.entity.clone(),
                kind: cell_kind_gui_str(cell.kind).to_string(),
                freshness,
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
fn build_constraints(
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
    constraints
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
///   whose SI value can be computed from [`UNIT_TABLE`].
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
                    // Evaluate the literal expression to SI value using UNIT_TABLE.
                    use reify_ast::ExprKind;
                    let initial_value_si = match &literal_expr.kind {
                        ExprKind::QuantityLiteral { value, unit } => {
                            // Only bare units resolve here; compound unit expressions
                            // (Mul/Div/Pow) get their registry resolver in task γ (3803).
                            match unit {
                                reify_ast::UnitExpr::Unit(unit) => {
                                    // Look up the unit in UNIT_TABLE for SI scale.
                                    match UNIT_TABLE.iter().find(|(u, _, _)| *u == unit.as_str()) {
                                        Some((_, scale, _)) => Some(value * scale),
                                        None => {
                                            // Unknown unit: emit debug so the silent value-loss is observable.
                                            // Supported units: mm, cm, m, deg, rad.
                                            tracing::debug!(
                                                target: "reify_gui::engine::literal_bind",
                                                joint = %joint_cell_name,
                                                unit = %unit,
                                                "bind(joint, <quantity>) with unsupported unit — not in UNIT_TABLE; \
                                                 initial_value_si will be None (supported units: mm, cm, m, deg, rad)"
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
        ExprKind::FunctionCall { name, args } => {
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
                    ExprKind::FunctionCall { name, args } => (name, args),
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
pub(crate) fn build_template_node(
    template: &reify_compiler::TopologyTemplate,
    entity_path: &str,
    compiled: &reify_compiler::CompiledModule,
    engine: Option<&Engine>,
    aux_ancestor: bool,
) -> EntityTreeNode {
    let kind = template.entity_kind.as_label();

    let mut children = Vec::new();

    // Value cells: param, let, auto
    for cell in &template.value_cells {
        let cell_kind = cell_kind_tree_str(cell.kind);
        let member = &cell.id.member;
        let cell_path = format!("{}.{}", entity_path, member);
        let is_geometry_member = member == "geometry";
        let parent_has_physical = template.trait_bounds.iter().any(|b| b.contains("Physical"));
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
    // These are NOT in `value_cells` — the compiler routes Solid-typed
    // bindings into `RealizationDecl` so they can be tessellated. Without
    // this loop the outline omits exactly the entries the user wants to
    // toggle visibility on (`let body`, `let hole`, `param geometry: Solid`,
    // …) and shows only scalar params, which can't be hidden in 3D.
    //
    // `entity_path` is the mesh key form (`Entity#realization[N]`) so it
    // matches `engineStore.meshes` and `viewStateStore` directly. The
    // user-friendly binding name is carried in `display_name`. Realizations
    // without a name (test-helper-only code path — see `RealizationDecl.name`
    // doc) fall back to deriving one from the path.
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
            trait_geometry: false,
            children: vec![],
            freshness,
            // Mirrors the surfacing-walk rule — shared contract anchor:
            // `geometry_ops::surface_subtree` / `geometry_ops::realization_is_aux`
            // (rule: `!(aux_ancestor || realization_is_aux(realization))`).
            // aux_ancestor is inherited from any containing `aux sub` up the tree.
            default_visible: !(aux_ancestor || real.is_aux),
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

/// Unit suffixes ordered by descending length — longest match first.
///
/// Exported as `pub(crate)` so tests can directly verify the ordering invariant
/// without duplicating the table. The `debug_assert!` inside `parse_value_string`
/// checks the same invariant at call-time in debug builds.
pub(crate) const UNIT_TABLE: &[(&str, f64, DimensionVector)] = &[
    ("deg", std::f64::consts::PI / 180.0, DimensionVector::ANGLE),
    ("rad", 1.0, DimensionVector::ANGLE),
    ("mm", 0.001, DimensionVector::LENGTH),
    ("cm", 0.01, DimensionVector::LENGTH),
    ("m", 1.0, DimensionVector::LENGTH),
];

/// Parse a value string into a Value.
///
/// Supported formats:
/// - Quantity literals: "80mm", "100cm", "1.5m", "90deg", "1.57rad"
/// - Plain numbers: "5.0" → Real, "5" → Int
/// - Booleans: "true", "false"
pub fn parse_value_string(s: &str) -> Result<Value, String> {
    let s = s.trim();

    // Booleans
    if s == "true" {
        return Ok(Value::Bool(true));
    }
    if s == "false" {
        return Ok(Value::Bool(false));
    }

    // Try quantity literals (number + unit suffix)
    // Units ordered by descending suffix length — longest match first.
    // debug_assert! enforces this invariant; #[test] unit_table_ordering_invariant_holds
    // covers release builds via UNIT_TABLE.
    debug_assert!(
        UNIT_TABLE.windows(2).all(|w| w[0].0.len() >= w[1].0.len()),
        "UNIT_TABLE must be sorted by descending suffix length"
    );
    for &(unit, scale, dimension) in UNIT_TABLE {
        if let Some(num_str) = s.strip_suffix(unit) {
            let num_str = num_str.trim();
            if let Ok(v) = num_str.parse::<f64>() {
                return Ok(Value::Scalar {
                    si_value: v * scale,
                    dimension,
                });
            }
        }
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

/// Format a compiled expression as a human-readable string.
fn format_expr(expr: &reify_ir::CompiledExpr) -> String {
    use reify_ir::CompiledExprKind;

    match &expr.kind {
        CompiledExprKind::Literal(v) => {
            let (val, unit) = crate::types::format_value(v);
            if unit.is_empty() {
                val
            } else {
                format!("{}{}", val, unit)
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
                .map(|arm| format!("{} => {}", arm.patterns.join(" | "), format_expr(&arm.body)))
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
            format!("{}[{}]", format_expr(object), format_expr(index))
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

/// Extract stress and displacement `SampledField` references from a
/// `ValueMap` containing an `ElasticResult` `StructureInstance`.
///
/// Iterates `values` and returns the first entry whose type_name is
/// `"ElasticResult"` and both `"stress"` and `"displacement"` fields resolve
/// to `Value::Field { source: Sampled, lambda: Arc<Value::SampledField(_)> }`.
///
/// Returns `None` if no such result is found or either field is absent/Undef.
/// Mirrors `extract_buckling_data` for the ElasticResult variant.
/// Delegates to `resolve_elastic_result_sampled_fields` for per-value resolution.
pub(crate) fn extract_elastic_result_fields(
    values: &reify_ir::ValueMap,
) -> Option<(&reify_ir::SampledField, &reify_ir::SampledField)> {
    for (_, value) in values.iter() {
        if let Some(pair) = resolve_elastic_result_sampled_fields(value) {
            return Some(pair);
        }
    }
    None
}

/// Extract stress and displacement `SampledField` references from a single
/// `Value::StructureInstance("ElasticResult")` value.
///
/// Returns `None` if the value is not an `ElasticResult` or either `"stress"`/
/// `"displacement"` field is absent or not a `Sampled` `SampledField`.
/// Used by both the single-case path (`extract_elastic_result_fields`) and the
/// multi-case path (`try_extract_from_multi_case_cell`).
fn resolve_elastic_result_sampled_fields<'a>(
    value: &'a reify_ir::Value,
) -> Option<(&'a reify_ir::SampledField, &'a reify_ir::SampledField)> {
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
    Some((stress_sf, disp_sf))
}

/// Try to extract stress/displacement fields from a single `Value::Map` cell that
/// carries a `MultiCaseResult` shape (`Map{"cases" -> Map{name -> ElasticResult}}`).
///
/// `active_case` selects which case's `ElasticResult` to use:
/// - `Some(name)` if the name is present in the cases map, otherwise lex-first.
/// - `None` → lex-first (matching `detect_multi_case_result`'s default).
///
/// Returns `None` if `cell_val` is not a `MultiCaseResult` shape, the active case
/// has no `ElasticResult`, or either `"stress"`/`"displacement"` field is absent/Undef.
fn try_extract_from_multi_case_cell<'a>(
    cell_val: &'a reify_ir::Value,
    active_case: Option<&str>,
) -> Option<(&'a reify_ir::SampledField, &'a reify_ir::SampledField)> {
    use reify_ir::Value;

    // Must be a MultiCaseResult-shaped map.
    let detected =
        reify_eval::multi_load_dispatch::detect_multi_case_result(cell_val)?;

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
    let case_val = cases_map.get(&Value::String(case_name_to_use))?;

    // Extract SampledFields from the active case's ElasticResult.
    resolve_elastic_result_sampled_fields(case_val)
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
    let (stress_sf, disp_sf) = if let Some(pair) = extract_elastic_result_fields(values) {
        pair
    } else {
        // Scan all cells for the first MultiCaseResult-shaped value.
        let multi_pair = values
            .iter()
            .find_map(|(_, cell_val)| try_extract_from_multi_case_cell(cell_val, active_case));
        match multi_pair {
            Some(pair) => pair,
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

        for chunk in mesh.vertices.chunks_exact(3) {
            let point = [chunk[0] as f64, chunk[1] as f64, chunk[2] as f64];
            vm_vec.push(von_mises_sample(stress_sf, point, tol));
            let [dx, dy, dz] = displaced_sample(disp_sf, point, tol);
            disp_vec.push(dx);
            disp_vec.push(dy);
            disp_vec.push(dz);
        }

        mesh.scalar_channels.insert("vonMises".to_string(), vm_vec);
        mesh.displaced_positions = Some(disp_vec);
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
///   `vonMises_bottom` (recovered per-vertex; `len == vertex_count`).
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

        mesh.scalar_channels
            .insert("vonMises_top".to_string(), view.von_mises_top.clone());
        mesh.scalar_channels
            .insert("vonMises_mid".to_string(), view.von_mises_mid.clone());
        mesh.scalar_channels
            .insert("vonMises_bottom".to_string(), view.von_mises_bottom.clone());

        mesh.vector_channels.insert(
            format!("shell_normal{}", crate::types::PER_FACE_CHANNEL_SUFFIX),
            view.shell_normals_per_face.clone(),
        );
    }
}

