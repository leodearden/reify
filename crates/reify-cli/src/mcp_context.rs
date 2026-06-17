// CliToolContext — real engine-backed implementation of ReifyToolContext for CLI mode

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use reify_compiler::ValueCellKind;
use reify_ir::{DeterminacyState, Value};
use reify_mcp::{
    ConstraintInfo, DiagnosticInfo, EvalStatusInfo, OpenFileInfo, ParameterInfo, ReifyToolContext,
    SelectionInfo, SetParamResult, SourceContent, SourceLocationInfo, ToolError, UpdateResult,
};

/// Tracks the state of an open file.
struct FileEntry {
    content: String,
    dirty: bool,
}

/// Internal mutable state behind a Mutex.
struct CliState {
    engine: Option<reify_eval::Engine>,
    compiled: Option<reify_compiler::CompiledModule>,
    files: HashMap<String, FileEntry>,
    active_file: Option<String>,
    _project_dir: PathBuf,
    /// Counts how many times `Engine::new(...)` has been called during this
    /// context's lifetime.  Only compiled in test builds; used to assert that
    /// engine construction happens at most once (lazy-init) rather than on
    /// every load/update/open call.
    #[cfg(test)]
    engine_construction_count: usize,
}

/// CLI-mode implementation of ReifyToolContext.
///
/// Backed by a real Engine with interior mutability via Mutex<CliState>.
pub struct CliToolContext {
    state: Mutex<CliState>,
}

impl CliToolContext {
    pub fn new(project_dir: PathBuf) -> Self {
        Self {
            state: Mutex::new(CliState {
                engine: None,
                compiled: None,
                files: HashMap::new(),
                active_file: None,
                _project_dir: project_dir,
                #[cfg(test)]
                engine_construction_count: 0,
            }),
        }
    }

    /// Return the number of times `Engine::new(...)` has been called during this
    /// context's lifetime.  Used in unit tests to assert that engine construction
    /// occurs at most once (lazy-init) rather than on every load/update/open call.
    #[cfg(test)]
    pub fn engine_construction_count(&self) -> usize {
        self.lock_state().engine_construction_count
    }

    /// Lock the internal state, recovering from a poisoned mutex.
    ///
    /// If a previous request panicked while holding the lock, the mutex becomes
    /// poisoned. Rather than cascading panics that kill the server, we recover
    /// the inner guard and continue operating on the (potentially inconsistent
    /// but non-crashed) state.
    fn lock_state(&self) -> MutexGuard<'_, CliState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Load a .ri file: read from disk, parse, compile, eval.
    pub fn load_file(&self, path: &str) -> Result<(), String> {
        let source = std::fs::read_to_string(path).map_err(|e| e.to_string())?;

        let module_name = std::path::Path::new(path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unnamed");

        // Prelude-aware parse for AST-shape consistency across reify-lsp/-cli;
        // see task 2525.
        let parsed =
            reify_compiler::parse_with_stdlib(&source, reify_core::ModulePath::single(module_name));

        if !parsed.errors.is_empty() {
            let msgs: Vec<String> = parsed.errors.iter().map(|e| e.message.clone()).collect();
            return Err(format!("Parse errors: {}", msgs.join("; ")));
        }

        let compiled = reify_compiler::compile(&parsed);

        let abs_path = std::fs::canonicalize(path)
            .unwrap_or_else(|_| PathBuf::from(path))
            .to_string_lossy()
            .to_string();

        let mut state = self.lock_state();
        // load_file semantically starts over with a new file — clear any prior
        // user overrides so get_parameters() returns fresh module defaults.
        let engine = ensure_engine(&mut state);
        engine.clear_param_overrides();
        engine.eval(&compiled);
        // Maintain state.files.keys() == {active_file} — see task 3183 audit.
        state.files.retain(|key, _| key == &abs_path);
        state.files.insert(
            abs_path.clone(),
            FileEntry {
                content: source,
                dirty: false,
            },
        );
        state.active_file = Some(abs_path);
        state.compiled = Some(compiled);

        Ok(())
    }
}

/// Lazily initialize `state.engine`, creating it at most once per context lifetime.
///
/// Returns a mutable reference to the engine.  Callers then call `.eval(&compiled)`
/// on the returned reference — the same engine instance is reused on every subsequent
/// call, preserving `prelude_functions` and avoiding repeated stdlib loading.
///
/// # Locking note
///
/// This function is always called while `CliState` is already held under the `Mutex`.
/// The first call (when `state.engine` is `None`) constructs the engine, which loads
/// the stdlib prelude via `reify_compiler::stdlib_loader::load_stdlib()`.  Any
/// concurrent MCP request will therefore block on that first construction.  In practice
/// the CLI MCP server has low concurrent traffic and the stdlib is parsed only once per
/// context lifetime, so this is acceptable.  If warm-start latency becomes a concern,
/// move `Engine::new(...)` into `CliToolContext::new()` so the stdlib load happens
/// before any request arrives.
///
/// # param_overrides semantics
///
/// As of task 2017, `Engine::eval()` consults `self.param_overrides` directly:
/// values written via `Engine::set_param_and_invalidate` survive subsequent
/// `eval()` calls without the CLI having to shadow-track them.  `load_file`
/// and `open_file` call `Engine::clear_param_overrides` before evaluating
/// because opening a new file semantically starts over.  Type-kind /
/// dimension mismatches between a stored override and the current cell type
/// land in `EvalResult.diagnostics` as `Severity::Warning` entries.
fn ensure_engine(state: &mut CliState) -> &mut reify_eval::Engine {
    #[cfg(test)]
    if state.engine.is_none() {
        state.engine_construction_count += 1;
    }
    state.engine.get_or_insert_with(|| {
        reify_eval::Engine::new(Box::new(reify_constraints::SimpleConstraintChecker), None)
    })
}

/// Format a dimension as a human-readable unit string.
fn dimension_unit(ty: &reify_core::ty::Type) -> String {
    match ty {
        reify_core::ty::Type::Scalar { dimension } => format!("{}", dimension),
        _ => String::new(),
    }
}

impl ReifyToolContext for CliToolContext {
    fn get_source(&self, file_path: Option<&str>) -> Result<SourceContent, ToolError> {
        let state = self.lock_state();
        let path = file_path
            .map(|s| s.to_string())
            .or_else(|| state.active_file.clone())
            .ok_or_else(|| ToolError::EngineError("no active file".to_string()))?;

        let entry = state
            .files
            .get(&path)
            .ok_or_else(|| ToolError::EngineError(format!("file not open: {path}")))?;

        Ok(SourceContent {
            content: entry.content.clone(),
            file_path: path,
        })
    }

    fn get_open_files(&self) -> Result<Vec<OpenFileInfo>, ToolError> {
        let state = self.lock_state();
        Ok(state
            .files
            .iter()
            .map(|(path, entry)| OpenFileInfo {
                path: path.clone(),
                language: "reify".to_string(),
                dirty: entry.dirty,
            })
            .collect())
    }

    /// Returns `Ok(vec![])` when `compiled` or `active_file` is absent (nothing to
    /// report).  This differs intentionally from `get_source_location`, which returns
    /// `Err` for the same states — see the doc comment on that method for the
    /// rationale.  Callers that need to distinguish "not ready" from "no diagnostics"
    /// must check whether `get_source_location` succeeds, not inspect the vec length.
    fn get_diagnostics(&self) -> Result<Vec<DiagnosticInfo>, ToolError> {
        let state = self.lock_state();

        let compiled = match &state.compiled {
            Some(c) => c,
            None => return Ok(vec![]),
        };
        let file_path = match &state.active_file {
            Some(p) => p,
            None => return Ok(vec![]),
        };
        let source = state
            .files
            .get(file_path)
            .map(|f| f.content.as_str())
            .ok_or_else(|| {
                ToolError::EngineError(format!("active_file {file_path} not in files map"))
            })?;

        let mut result = Vec::new();
        for diag in &compiled.diagnostics {
            // Use the first label's span if available, otherwise default to (1,1)
            let (line, column, end_line, end_column) = if let Some(label) = diag.labels.first() {
                let (l, c) = reify_core::byte_offset_to_line_col(source, label.span.start as usize);
                let (el, ec) = reify_core::byte_offset_to_line_col(source, label.span.end as usize);
                (l as u32, c as u32, el as u32, ec as u32)
            } else {
                (1, 1, 1, 1)
            };
            result.push(DiagnosticInfo {
                file_path: file_path.clone(),
                line,
                column,
                end_line,
                end_column,
                severity: diag.severity.as_wire_str().to_owned(),
                message: diag.message.clone(),
                code: None,
                has_location: !diag.labels.is_empty(),
            });
        }

        Ok(result)
    }

    fn get_parameters(&self) -> Result<Vec<ParameterInfo>, ToolError> {
        let state = self.lock_state();
        let snapshot = match state.engine.as_ref().and_then(|e| e.snapshot()) {
            Some(s) => s,
            None => return Ok(vec![]),
        };

        let compiled = match &state.compiled {
            Some(c) => c,
            None => return Ok(vec![]),
        };

        let mut params = Vec::new();

        // Iterate through all templates to get cell metadata (kind, type)
        for template in &compiled.templates {
            for cell_decl in &template.value_cells {
                let id = &cell_decl.id;
                let (value, determinacy) = snapshot
                    .values
                    .get(id)
                    .cloned()
                    .unwrap_or((Value::Undef, DeterminacyState::Undetermined));

                let kind_str = match cell_decl.kind {
                    ValueCellKind::Param => "Param",
                    ValueCellKind::Let => "Let",
                    ValueCellKind::Auto { free: true } => "Auto(free)",
                    ValueCellKind::Auto { free: false } => "Auto",
                };

                let det_str = match determinacy {
                    DeterminacyState::Determined => "determined",
                    DeterminacyState::Undetermined => "undetermined",
                    DeterminacyState::Provisional => "provisional",
                    DeterminacyState::Auto => "auto",
                };

                params.push(ParameterInfo {
                    cell_id: format!("{}", id),
                    name: id.member.clone(),
                    value: format!("{}", value),
                    unit: dimension_unit(&cell_decl.cell_type),
                    kind: kind_str.to_string(),
                    entity_path: id.entity.clone(),
                    determinacy: det_str.to_string(),
                });
            }
        }

        Ok(params)
    }

    fn get_constraints(&self) -> Result<Vec<ConstraintInfo>, ToolError> {
        let state = self.lock_state();
        let compiled = match &state.compiled {
            Some(c) => c,
            None => return Ok(vec![]),
        };

        // We need to run check to get constraint satisfaction status.
        // If the engine has been initialized, use the snapshot's constraint data.
        let mut result = Vec::new();

        // Get constraint results from the engine if available
        // For now, use the compiled constraints with "unknown" status,
        // then we'll upgrade when we have check results
        for template in &compiled.templates {
            for constraint in &template.constraints {
                result.push(ConstraintInfo {
                    node_id: format!("{}", constraint.id),
                    expression: format!("{:?}", constraint.expr),
                    status: "unknown".to_string(),
                    label: constraint.label.clone(),
                    parameter_ids: vec![],
                });
            }
        }

        Ok(result)
    }

    fn get_eval_status(&self) -> Result<EvalStatusInfo, ToolError> {
        let state = self.lock_state();
        let phase = if state.engine.is_some() {
            "ready"
        } else {
            "idle"
        };
        Ok(EvalStatusInfo {
            phase: phase.to_string(),
            progress: None,
            dirty_count: 0,
        })
    }

    fn get_selection(&self) -> Result<SelectionInfo, ToolError> {
        Ok(SelectionInfo::default())
    }

    /// Returns `Err` (not `Ok` with a sentinel) when `compiled` or `active_file` is
    /// absent.  This differs from `get_diagnostics`, which returns `Ok(vec![])` for
    /// the same states.  The asymmetry is intentional: `get_diagnostics` has a
    /// natural "nothing to report" empty-vec value, while `get_source_location`
    /// returns a single `SourceLocationInfo` with no natural empty equivalent — an
    /// absent active file is a caller error that deserves a visible `Err`.  Callers
    /// that want to distinguish "not ready" from "entity not found" can inspect the
    /// error message; both arms use `ToolError::EngineError`.
    fn get_source_location(&self, entity_path: &str) -> Result<SourceLocationInfo, ToolError> {
        let state = self.lock_state();
        let compiled = state
            .compiled
            .as_ref()
            .ok_or_else(|| ToolError::EngineError("no compiled module".to_string()))?;
        let file_path = state
            .active_file
            .as_ref()
            .ok_or_else(|| ToolError::EngineError("no active file".to_string()))?;
        let source = state
            .files
            .get(file_path)
            .map(|f| f.content.as_str())
            .ok_or_else(|| {
                ToolError::EngineError(format!("active_file {file_path} not in files map"))
            })?;
        reify_eval::resolve_entity_source_location(compiled, source, file_path, entity_path)
            .ok_or_else(|| ToolError::EngineError(format!("entity not found: {entity_path}")))
    }

    fn update_source(&self, file_path: &str, content: &str) -> Result<UpdateResult, ToolError> {
        // Canonicalize path to match open_file/load_file key convention.
        let canonical = std::fs::canonicalize(file_path)
            .unwrap_or_else(|_| PathBuf::from(file_path))
            .to_string_lossy()
            .to_string();

        // Stage content locally — do NOT modify state.files until after successful pipeline.
        // This preserves the invariant that files, compiled, and engine always reflect
        // the same successful state.

        // Re-parse, compile, and eval from scratch (topology may change)
        let module_name = std::path::Path::new(file_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unnamed");

        // Prelude-aware parse for AST-shape consistency across reify-lsp/-cli;
        // see task 2525.
        let parsed =
            reify_compiler::parse_with_stdlib(content, reify_core::ModulePath::single(module_name));

        if !parsed.errors.is_empty() {
            // Parse failed — return failure WITHOUT modifying any state.
            // get_source() continues to return the last known-good content.
            return Ok(UpdateResult {
                success: false,
                diagnostics_count: parsed.errors.len() as u32,
            });
        }

        let compiled = reify_compiler::compile(&parsed);
        let diag_count = compiled.diagnostics.len() as u32;

        // Pipeline succeeded — commit file content and eval on the long-lived engine.
        // Engine::eval() reads self.param_overrides directly (task 2017), so
        // overrides previously written via set_parameter survive topology-
        // preserving edits without an explicit re-apply pass.  Orphaned entries
        // (cells removed by the new module) are pruned inside eval().
        let mut state = self.lock_state();
        ensure_engine(&mut state).eval(&compiled);
        // Drop entries for any previously-loaded file: state.files must reflect
        // the same single canonical source as compiled and active_file.  See
        // task 3100 and the loud-failure invariant landed in task 3054.
        state.files.retain(|key, _| key == &canonical);
        if let Some(entry) = state.files.get_mut(&canonical) {
            entry.content = content.to_string();
            entry.dirty = true;
        } else {
            state.files.insert(
                canonical.clone(),
                FileEntry {
                    content: content.to_string(),
                    dirty: true,
                },
            );
        }
        state.compiled = Some(compiled);
        // After the retain() above, state.files contains at most one entry whose key
        // matches `canonical`.  Combined with this unconditional set, compiled,
        // active_file, and state.files always describe a single canonical source.
        // get_or_insert_with was previously used to preserve a prior load_file/open_file
        // selection, but that leaves active_file pointing at "a.ri" while compiled holds
        // b.ri's module — byte-span offsets from b's diagnostics would then be resolved
        // against a's source, producing wrong line/column numbers.
        //
        // Uniform singleton invariant (task 3183 — multi-document consumer audit):
        // All three state-mutating entry points now maintain state.files.keys() == {p}
        // via a retain(|key, _| key == &p) call immediately before their insert:
        //   • update_source(p) → files.keys() == {p}   (retain here, line ~429)
        //   • load_file(p)     → files.keys() == {p}   (retain added by task 3183)
        //   • open_file(p)     → files.keys() == {p}   (retain added by task 3183)
        // The prior asymmetry (load_file/open_file accumulated; update_source pruned)
        // was resolved by task 3183.  Future multi-document support requires lifting
        // the single-compiled / single-active_file engine assumption alongside this
        // invariant — that is strictly out of scope for an audit task.
        state.active_file = Some(canonical.clone());

        Ok(UpdateResult {
            success: true,
            diagnostics_count: diag_count,
        })
    }

    fn set_parameter(&self, cell_id: &str, value: &str) -> Result<SetParamResult, ToolError> {
        let mut state = self.lock_state();

        if state.engine.is_none() {
            return Err(ToolError::EngineError("no engine initialized".to_string()));
        }

        // Parse cell_id: "Entity.member"
        let (entity, member) = cell_id.split_once('.').ok_or_else(|| {
            ToolError::InvalidParams(format!(
                "cell_id must be 'Entity.member' format, got: {cell_id}"
            ))
        })?;

        let cell_id_obj = reify_core::ValueCellId::new(entity, member);

        // Parse the value as f64
        let numeric_val: f64 = value
            .parse()
            .map_err(|e| ToolError::InvalidParams(format!("cannot parse value as number: {e}")))?;

        // Look up the cell's type from the compiled module to determine dimension
        let compiled = state
            .compiled
            .as_ref()
            .ok_or_else(|| ToolError::EngineError("no compiled module".to_string()))?;

        let mut cell_type = None;
        for template in &compiled.templates {
            for cell_decl in &template.value_cells {
                if cell_decl.id == cell_id_obj {
                    cell_type = Some(cell_decl.cell_type.clone());
                    break;
                }
            }
        }

        let ty = cell_type
            .ok_or_else(|| ToolError::InvalidParams(format!("cell not found: {}", cell_id_obj)))?;

        // Construct the appropriate Value based on the cell's type
        let new_value = match &ty {
            reify_core::ty::Type::Scalar { dimension } if !dimension.is_dimensionless() => {
                Value::Scalar { si_value: numeric_val, dimension: *dimension }
            }
            reify_core::ty::Type::Int => Value::Int(numeric_val as i64),
            _ => Value::Real(numeric_val),
        };

        // Apply the parameter change via incremental edit.
        // edit_param must succeed before mutating engine state via set_param_and_invalidate:
        // if edit_param fails, param_overrides and cache remain in their last consistent state.
        //
        // As of task 2017, set_param_and_invalidate's write into
        // Engine::param_overrides is the only bookkeeping needed: Engine::eval
        // now consults that map directly, so the value survives a subsequent
        // update_source without CLI-side shadow state.
        let engine = state.engine.as_mut().unwrap();
        engine
            .edit_param(cell_id_obj.clone(), new_value.clone())
            .map_err(|e| ToolError::EngineError(format!("incremental eval failed: {e}")))?;
        engine.set_param_and_invalidate(&cell_id_obj, new_value.clone());

        let unit = dimension_unit(&ty);
        Ok(SetParamResult {
            success: true,
            new_value: format!("{}", new_value),
            unit,
        })
    }

    fn open_file(&self, file_path: &str) -> Result<OpenFileInfo, ToolError> {
        let source = std::fs::read_to_string(file_path)
            .map_err(|e| ToolError::EngineError(format!("cannot read file: {e}")))?;

        let abs_path = std::fs::canonicalize(file_path)
            .unwrap_or_else(|_| PathBuf::from(file_path))
            .to_string_lossy()
            .to_string();

        // If it's a .ri file, parse/compile BEFORE committing state.
        // On parse failure, still register the file but don't update compiled/engine.
        let pipeline_result: Option<reify_compiler::CompiledModule> = if file_path.ends_with(".ri")
        {
            let module_name = std::path::Path::new(file_path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unnamed");

            // Prelude-aware parse for AST-shape consistency across reify-lsp/-cli;
            // see task 2525.
            let parsed = reify_compiler::parse_with_stdlib(
                &source,
                reify_core::ModulePath::single(module_name),
            );

            if parsed.errors.is_empty() {
                Some(reify_compiler::compile(&parsed))
            } else {
                None
            }
        } else {
            None
        };

        let mut state = self.lock_state();
        // Maintain state.files.keys() == {active_file} — see task 3183 audit.
        state.files.retain(|key, _| key == &abs_path);
        state.files.insert(
            abs_path.clone(),
            FileEntry {
                content: source,
                dirty: false,
            },
        );
        state.active_file = Some(abs_path.clone());

        if let Some(compiled) = pipeline_result {
            // open_file semantically starts over with a new file — clear any
            // prior user overrides so get_parameters() returns fresh module defaults.
            let engine = ensure_engine(&mut state);
            engine.clear_param_overrides();
            engine.eval(&compiled);
            state.compiled = Some(compiled);
        } else {
            // parse failed or non-.ri file — compiled no longer reflects active_file.
            // Clear it so get_diagnostics() / get_source_location() do not resolve
            // byte-spans from a prior module against the new file's source
            // (wrong line/column numbers).  Task 3183 review: the retain() added
            // above prunes state.files to {abs_path}; without clearing compiled, a
            // stale CompiledModule from the prior file would remain while state.files
            // no longer contains that file's source — the same mismatch that
            // update_source's parse-fail-no-mutation rule (lines ~409-416) prevents
            // for inline edits.
            state.compiled = None;
        }

        Ok(OpenFileInfo {
            path: abs_path,
            language: "reify".to_string(),
            dirty: false,
        })
    }

    fn save_file(&self, file_path: Option<&str>) -> Result<bool, ToolError> {
        let mut state = self.lock_state();
        let path = file_path
            .map(|s| s.to_string())
            .or_else(|| state.active_file.clone())
            .ok_or_else(|| ToolError::EngineError("no active file".to_string()))?;

        let entry = state
            .files
            .get(&path)
            .ok_or_else(|| ToolError::EngineError(format!("file not open: {path}")))?;

        std::fs::write(&path, &entry.content)
            .map_err(|e| ToolError::EngineError(format!("cannot write file: {e}")))?;

        // Reset dirty flag after successful write
        if let Some(entry) = state.files.get_mut(&path) {
            entry.dirty = false;
        }

        Ok(true)
    }

    fn export(&self, _format: &str, _output_path: &str) -> Result<bool, ToolError> {
        // Export requires geometry kernel which is not initialized in CLI mode by default
        Err(ToolError::EngineError(
            "export not available in headless CLI mode (no geometry kernel)".to_string(),
        ))
    }

    fn focus_entity(&self, _entity_path: &str) -> Result<bool, ToolError> {
        Ok(false)
    }

    fn navigate_to_source(
        &self,
        _file: &str,
        _line: u32,
        _column: u32,
        _end_line: u32,
        _end_column: u32,
    ) -> Result<bool, ToolError> {
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BRACKET_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/bracket.ri");
    const BRACKET_COMPILE_ERROR_PATH: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/bracket_compile_error.ri"
    );
    const BRACKET_PARSE_ERROR_PATH: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/bracket_parse_error.ri"
    );

    /// Obviously-nonsense Reify source: a single top-level `{` with no matching
    /// close brace.  No token in the Reify grammar begins a top-level declaration
    /// with `{`, so this input is overwhelmingly unlikely to ever become
    /// parseable.  It is kept as a named constant so the precondition self-check
    /// test and the `update_source_invalid_content_does_not_construct_new_engine`
    /// regression test are guaranteed to exercise the exact same input string.
    const INVALID_PARSE_INPUT: &str = "{";

    /// Returns a fresh `CliToolContext` rooted at the default `tests/fixtures`
    /// directory.  Use this in unit tests that don't need a custom `project_dir`;
    /// tests that do need one should call `CliToolContext::new(...)` directly.
    fn fresh_ctx() -> CliToolContext {
        CliToolContext::new(PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures"
        )))
    }

    /// Verify that `update_source` reuses the Engine instance rather than constructing
    /// a new one.  Fails against pre-fix code because `update_source` calls
    /// `Engine::new` unconditionally, incrementing `engine_construction_count` to 2.
    #[test]
    fn update_source_reuses_engine_instance() {
        let ctx = fresh_ctx();

        // Establish an initial engine via load_file (count → 1).
        ctx.load_file(BRACKET_PATH)
            .expect("load_file should succeed");
        let count_after_load = ctx.engine_construction_count();
        assert_eq!(
            count_after_load, 1,
            "load_file should construct exactly one engine"
        );

        // Minor valid edit: add a trailing space — topology is unchanged.
        let source = std::fs::read_to_string(BRACKET_PATH).expect("fixture must be readable");
        let modified = format!("{source} ");
        let result = ctx
            .update_source(BRACKET_PATH, &modified)
            .expect("update_source should not error");
        assert!(
            result.success,
            "update_source should succeed with valid content"
        );

        let count_after_update = ctx.engine_construction_count();
        assert_eq!(
            count_after_update, 1,
            "update_source must reuse the engine, not construct a new one \
             (got engine_construction_count={count_after_update})"
        );
    }

    /// Verify that repeated `load_file` calls reuse the Engine instance rather
    /// than constructing a new one each time.  Fails against pre-fix code because
    /// `load_file` calls `Engine::new` unconditionally.
    #[test]
    fn load_file_reuses_engine_across_subsequent_calls() {
        let ctx = fresh_ctx();

        // First load — engine is created (count → 1).
        ctx.load_file(BRACKET_PATH)
            .expect("first load_file should succeed");
        let count_after_first = ctx.engine_construction_count();
        assert_eq!(
            count_after_first, 1,
            "first load_file should construct exactly one engine"
        );

        // Second load of the same path — engine must be reused (count stays at 1).
        ctx.load_file(BRACKET_PATH)
            .expect("second load_file should succeed");
        let count_after_second = ctx.engine_construction_count();
        assert_eq!(
            count_after_second, 1,
            "second load_file must reuse the engine, not construct a new one \
             (got engine_construction_count={count_after_second})"
        );
    }

    /// Verify that repeated `open_file` calls reuse the Engine instance, and that
    /// a subsequent `update_source` also reuses it.  Fails against pre-fix code
    /// because `open_file` calls `Engine::new` unconditionally.
    #[test]
    fn open_file_reuses_engine_across_subsequent_calls() {
        let ctx = fresh_ctx();

        // First open — engine is created (count → 1).
        ctx.open_file(BRACKET_PATH)
            .expect("first open_file should succeed");
        let count_after_first = ctx.engine_construction_count();
        assert_eq!(
            count_after_first, 1,
            "first open_file should construct exactly one engine"
        );

        // Second open of the same path — engine must be reused.
        ctx.open_file(BRACKET_PATH)
            .expect("second open_file should succeed");
        let count_after_second = ctx.engine_construction_count();
        assert_eq!(
            count_after_second, 1,
            "second open_file must reuse the engine \
             (got engine_construction_count={count_after_second})"
        );

        // update_source after open_file must also reuse the same engine.
        let source = std::fs::read_to_string(BRACKET_PATH).expect("fixture must be readable");
        let modified = format!("{source} ");
        ctx.update_source(BRACKET_PATH, &modified)
            .expect("update_source should succeed");
        let count_after_update = ctx.engine_construction_count();
        assert_eq!(
            count_after_update, 1,
            "update_source after open_file must reuse the engine \
             (got engine_construction_count={count_after_update})"
        );
    }

    /// Verify that a parameter override set via `set_parameter` persists across
    /// a topology-preserving `update_source` (e.g. adding trailing whitespace).
    /// This documents the "reuse" benefit of the long-lived Engine: the user's
    /// value survives a save/edit cycle.
    #[test]
    fn set_parameter_persists_across_topology_preserving_update_source() {
        let ctx = fresh_ctx();

        ctx.load_file(BRACKET_PATH)
            .expect("load_file should succeed");

        // Record the default width value.
        let params_default = ctx.get_parameters().expect("get_parameters should succeed");
        let default_width = params_default
            .iter()
            .find(|p| p.name == "width")
            .expect("bracket.ri should have a 'width' param")
            .value
            .clone();

        // Override width (120mm in SI = 0.12 m).
        ctx.set_parameter("Bracket.width", "0.12")
            .expect("set_parameter should succeed");

        let params_overridden = ctx.get_parameters().expect("get_parameters should succeed");
        let overridden_width = params_overridden
            .iter()
            .find(|p| p.name == "width")
            .expect("width should still exist")
            .value
            .clone();

        assert_ne!(
            overridden_width, default_width,
            "set_parameter should change the value from the module default"
        );

        // Topology-preserving update: append trailing whitespace.
        let source = std::fs::read_to_string(BRACKET_PATH).expect("fixture must be readable");
        let modified = format!("{source} ");
        ctx.update_source(BRACKET_PATH, &modified)
            .expect("topology-preserving update_source should succeed");

        // Override must survive the update.
        let params_after = ctx.get_parameters().expect("get_parameters should succeed");
        let width_after = params_after
            .iter()
            .find(|p| p.name == "width")
            .expect("width should exist after topology-preserving edit")
            .value
            .clone();

        assert_eq!(
            width_after, overridden_width,
            "set_parameter override must persist across topology-preserving update_source"
        );
    }

    /// Regression guard: multiple parameter overrides must all survive a
    /// topology-preserving `update_source` (only whitespace added, so all params
    /// survive).  Override persistence is now handled inside `Engine::eval()`
    /// via `self.param_overrides`; this test locks the outward behaviour so
    /// future refactors of the engine's override path cannot silently drop one
    /// of several concurrently-set overrides.
    ///
    /// Three distinct `Scalar[LENGTH]` overrides are set on width / height /
    /// thickness.  After a topology-preserving `update_source` (trailing
    /// whitespace), all three must read back at their overridden values, not
    /// the module defaults.
    #[test]
    fn multiple_overrides_survive_topology_preserving_update_source() {
        let ctx = fresh_ctx();
        ctx.load_file(BRACKET_PATH)
            .expect("load_file should succeed");

        // Record module-default values so we can assert overrides differ.
        let params_default = ctx.get_parameters().expect("get_parameters should succeed");
        let default_width = params_default
            .iter()
            .find(|p| p.name == "width")
            .expect("bracket.ri should have a 'width' param")
            .value
            .clone();
        let default_height = params_default
            .iter()
            .find(|p| p.name == "height")
            .expect("bracket.ri should have a 'height' param")
            .value
            .clone();
        let default_thickness = params_default
            .iter()
            .find(|p| p.name == "thickness")
            .expect("bracket.ri should have a 'thickness' param")
            .value
            .clone();

        // Set distinct non-default Scalar[LENGTH] overrides on all three params.
        // The literal values (0.12 m, 0.15 m, 0.004 m) are chosen to differ from
        // bracket.ri's current module defaults.  If the fixture is ever edited such
        // that a default coincides with one of these literals, the `assert_ne!`
        // sanity checks below will trigger — update the literals together with any
        // such fixture change so this test continues to exercise the override path.
        ctx.set_parameter("Bracket.width", "0.12")
            .expect("set_parameter width should succeed");
        ctx.set_parameter("Bracket.height", "0.15")
            .expect("set_parameter height should succeed");
        ctx.set_parameter("Bracket.thickness", "0.004")
            .expect("set_parameter thickness should succeed");

        // Capture the overridden values as strings (avoids format-string brittleness).
        let params_overridden = ctx.get_parameters().expect("get_parameters should succeed");
        let overridden_width = params_overridden
            .iter()
            .find(|p| p.name == "width")
            .expect("width should exist after override")
            .value
            .clone();
        let overridden_height = params_overridden
            .iter()
            .find(|p| p.name == "height")
            .expect("height should exist after override")
            .value
            .clone();
        let overridden_thickness = params_overridden
            .iter()
            .find(|p| p.name == "thickness")
            .expect("thickness should exist after override")
            .value
            .clone();

        // Sanity: overrides must differ from module defaults.
        assert_ne!(
            overridden_width, default_width,
            "width override must differ from default"
        );
        assert_ne!(
            overridden_height, default_height,
            "height override must differ from default"
        );
        assert_ne!(
            overridden_thickness, default_thickness,
            "thickness override must differ from default"
        );

        // Topology-preserving update: append trailing whitespace.  `Engine::eval`
        // re-applies all three entries from its internal `param_overrides` map;
        // none of the three params is removed by the edit so nothing is purged.
        let source = std::fs::read_to_string(BRACKET_PATH).expect("fixture must be readable");
        let modified = format!("{source} ");
        let result = ctx
            .update_source(BRACKET_PATH, &modified)
            .expect("topology-preserving update_source should succeed");
        assert!(result.success, "update_source should succeed");

        // All three overrides must survive the re-eval.
        let params_after = ctx.get_parameters().expect("get_parameters should succeed");
        let width_after = params_after
            .iter()
            .find(|p| p.name == "width")
            .expect("width should exist after topology-preserving update")
            .value
            .clone();
        let height_after = params_after
            .iter()
            .find(|p| p.name == "height")
            .expect("height should exist after topology-preserving update")
            .value
            .clone();
        let thickness_after = params_after
            .iter()
            .find(|p| p.name == "thickness")
            .expect("thickness should exist after topology-preserving update")
            .value
            .clone();

        assert_eq!(
            width_after, overridden_width,
            "width override must survive topology-preserving update_source"
        );
        assert_eq!(
            height_after, overridden_height,
            "height override must survive topology-preserving update_source"
        );
        assert_eq!(
            thickness_after, overridden_thickness,
            "thickness override must survive topology-preserving update_source"
        );
    }

    /// Verify that `load_file` clears prior parameter overrides.  Because
    /// `load_file` semantically starts over with a fresh file, overrides
    /// set before it must not leak into the re-evaluated snapshot.
    #[test]
    fn load_file_clears_param_overrides() {
        let ctx = fresh_ctx();

        ctx.load_file(BRACKET_PATH)
            .expect("first load_file should succeed");

        // Record the default width value.
        let params_default = ctx.get_parameters().expect("get_parameters should succeed");
        let default_width = params_default
            .iter()
            .find(|p| p.name == "width")
            .expect("bracket.ri should have a 'width' param")
            .value
            .clone();

        // Override width.
        ctx.set_parameter("Bracket.width", "0.12")
            .expect("set_parameter should succeed");

        let params_overridden = ctx.get_parameters().expect("get_parameters should succeed");
        let overridden_width = params_overridden
            .iter()
            .find(|p| p.name == "width")
            .expect("width should exist")
            .value
            .clone();
        assert_ne!(overridden_width, default_width, "value must have changed");

        // Reload the file — overrides must be cleared.
        ctx.load_file(BRACKET_PATH)
            .expect("second load_file should succeed");

        let params_reloaded = ctx.get_parameters().expect("get_parameters should succeed");
        let width_reloaded = params_reloaded
            .iter()
            .find(|p| p.name == "width")
            .expect("width should exist after reload")
            .value
            .clone();

        assert_eq!(
            width_reloaded, default_width,
            "load_file must clear param overrides and restore module defaults"
        );
    }

    /// Precondition guard: verifies that `INVALID_PARSE_INPUT` actually produces
    /// a parse error under the current Reify grammar, AND that `update_source`
    /// treats it as a failure (returns `success=false`).  If this test ever fails
    /// it means the grammar or `update_source` semantics have changed and the
    /// constant must be updated — look for a new "obviously nonsense" input.
    /// Failing here is far preferable to the downstream
    /// `update_source_invalid_content_does_not_construct_new_engine` test
    /// silently exercising the wrong branch.
    #[test]
    fn invalid_parse_input_is_actually_unparseable() {
        // Check 1: the syntax parser itself reports errors.
        let parsed =
            reify_syntax::parse(INVALID_PARSE_INPUT, reify_core::ModulePath::single("probe"));
        assert!(
            !parsed.errors.is_empty(),
            "INVALID_PARSE_INPUT must produce a parse error; the grammar may have changed. \
             parsed.errors={:?} declarations={:?}",
            parsed.errors,
            parsed.declarations,
        );

        // Check 2: `update_source` reports failure for this input — the real
        // contract the regression test depends on.  A grammar change that still
        // emits errors but that `update_source` treats as recoverable would pass
        // Check 1 yet break the regression test; this assertion catches it first.
        let ctx = fresh_ctx();
        let update_result = ctx
            .update_source(BRACKET_PATH, INVALID_PARSE_INPUT)
            .expect("update_source should return Ok (not Err) even for invalid content");
        assert!(
            !update_result.success,
            "INVALID_PARSE_INPUT must cause update_source to return success=false; \
             grammar or update_source semantics may have changed. result={:?}",
            update_result,
        );
    }

    /// Regression guard: `update_source` with invalid content must not construct a
    /// new Engine and must leave prior valid state intact (files/compiled/engine
    /// unchanged).  The "invalid input preserves state" invariant is separately
    /// tested via JSON-RPC in tests/mcp_integration.rs; this unit test adds a
    /// fast in-process guard that also verifies the engine is never touched on
    /// parse failure.
    #[test]
    fn update_source_invalid_content_does_not_construct_new_engine() {
        let ctx = fresh_ctx();

        // Load a valid file to get known-good state.
        ctx.load_file(BRACKET_PATH)
            .expect("load_file should succeed");
        let count_before = ctx.engine_construction_count();
        assert_eq!(count_before, 1);

        // Prior valid state: get_parameters returns results.
        let params_before = ctx.get_parameters().expect("get_parameters should succeed");
        assert!(
            !params_before.is_empty(),
            "should have parameters after loading bracket.ri"
        );

        // Attempt an update with content guaranteed-unparseable by
        // `invalid_parse_input_is_actually_unparseable`.
        let result = ctx
            .update_source(BRACKET_PATH, INVALID_PARSE_INPUT)
            .expect("update_source should return Ok (not Err) even on parse failure");
        assert!(
            !result.success,
            "update_source should report failure for invalid content"
        );

        // Engine must not have been (re)constructed.
        let count_after = ctx.engine_construction_count();
        assert_eq!(
            count_after, 1,
            "invalid update_source must not construct a new engine \
             (got engine_construction_count={count_after})"
        );

        // Prior valid parameters must still be accessible.
        let params_after = ctx
            .get_parameters()
            .expect("get_parameters should still work");
        assert_eq!(
            params_before.len(),
            params_after.len(),
            "parameter list must be unchanged after a failed update_source"
        );
    }

    /// Smoke test: `fresh_ctx()` is rooted at the real `tests/fixtures`
    /// directory so standard fixture files are loadable.
    #[test]
    fn fresh_ctx_provides_default_fixture_dir_context() {
        fresh_ctx()
            .load_file(BRACKET_PATH)
            .expect("fresh_ctx must point at the real fixtures dir so bracket.ri is loadable");
    }

    /// Behavior guard: `fresh_ctx()` must return a context whose engine has
    /// not been constructed yet.  Guards against a future refactor that eagerly
    /// builds an engine inside `CliToolContext::new`, which would break the
    /// lazy-init invariants verified by `*_reuses_engine_*` tests.
    #[test]
    fn fresh_ctx_returns_pristine_context() {
        let ctx = fresh_ctx();
        assert_eq!(
            ctx.engine_construction_count(),
            0,
            "fresh_ctx must return a context whose engine has not been constructed yet"
        );
    }

    /// Baseline guard: on a freshly-constructed context (no `update_source` /
    /// `load_file` / `open_file` call), the two "not-ready" code paths must behave
    /// as documented:
    ///
    /// - `get_diagnostics` → `Ok(vec![])` (nothing to report; the `compiled = None`
    ///   early-return short-circuits before any active-file check).
    /// - `get_source_location` → `Err(EngineError("no compiled module"))` (no natural
    ///   empty value; the method documents that it returns `Err` on missing state).
    ///
    /// This test locks in the intentional asymmetry described in the doc comments on
    /// both methods.  A future refactor that accidentally changes either branch will
    /// fail here before reaching callers.
    #[test]
    fn fresh_ctx_not_ready_baseline() {
        let ctx = fresh_ctx();

        // get_diagnostics returns Ok(vec![]) — "nothing to report" sentinel.
        let diags = ctx
            .get_diagnostics()
            .expect("get_diagnostics on fresh ctx must return Ok, not Err");
        assert!(
            diags.is_empty(),
            "get_diagnostics on a fresh ctx (no compiled module) must return Ok(vec![]), got: {:?}",
            diags
        );

        // get_source_location returns Err — no natural empty sentinel exists.
        let err = ctx
            .get_source_location("AnyEntity")
            .expect_err("get_source_location on fresh ctx must return Err (no compiled module)");
        match &err {
            ToolError::EngineError(msg) => assert!(
                msg.contains("no compiled module"),
                "Err message must say 'no compiled module', got: {msg:?}"
            ),
            other => panic!("expected ToolError::EngineError, got {other:?}"),
        }
    }

    /// Positive guard: `update_source` on a fresh context (no prior `load_file` /
    /// `open_file`) must enable `get_source_location` to return `Ok` with a
    /// meaningful span.
    ///
    /// `update_source` achieves this via an unconditional
    /// `state.active_file = Some(canonical.clone())`.  `get_or_insert_with` was
    /// rejected because it would leave `active_file` pointing at a stale prior
    /// file when `update_source` switches files — see the production-code comment
    /// above the unconditional-set site for the full rationale, and
    /// `update_source_after_load_file_switches_active_file` for the regression
    /// guard.
    #[test]
    fn update_source_enables_get_source_location_without_load_file() {
        let ctx = fresh_ctx();
        let source = std::fs::read_to_string(BRACKET_PATH).expect("fixture must be readable");
        let update = ctx
            .update_source(BRACKET_PATH, &source)
            .expect("update_source should succeed with valid content");
        assert!(
            update.success,
            "update_source must succeed so compiled=Some is set"
        );
        // No load_file / open_file — update_source alone must enable get_source_location.
        let loc = ctx
            .get_source_location("Bracket")
            .expect("get_source_location should return Ok after update_source alone");
        assert!(
            !loc.file_path.is_empty(),
            "file_path must be non-empty after update_source, got {:?}",
            loc.file_path
        );
        assert!(loc.line >= 1, "line must be 1-based, got {}", loc.line);
        assert!(
            loc.column >= 1,
            "column must be 1-based, got {}",
            loc.column
        );
    }

    /// Happy-path counter-case: `get_source_location` must return `Ok` with a
    /// non-empty `file_path` when `load_file` is called first.  Locks the
    /// success arm of the `active_file` guard added in step-2.
    #[test]
    fn get_source_location_succeeds_after_load_file() {
        let ctx = fresh_ctx();
        ctx.load_file(BRACKET_PATH)
            .expect("load_file should succeed for bracket.ri");
        let loc = ctx
            .get_source_location("Bracket")
            .expect("get_source_location should return Ok after load_file");
        assert!(
            !loc.file_path.is_empty(),
            "file_path must be non-empty after load_file, got {:?}",
            loc.file_path
        );
        assert!(loc.line >= 1, "line must be 1-based, got {}", loc.line);
        assert!(
            loc.column >= 1,
            "column must be 1-based, got {}",
            loc.column
        );
    }

    /// Positive guard: `update_source` on a fresh context (no prior `load_file` /
    /// `open_file`) must enable `get_diagnostics` to return `Ok` with non-empty
    /// diagnostics carrying a non-empty `file_path` that matches the canonicalized
    /// source path.
    ///
    /// Uses `bracket_compile_error.ri` (not `bracket.ri`) because `bracket.ri` has
    /// zero diagnostics — an empty-vec result would pass even against broken code.
    /// `bracket_compile_error.ri` is parse-clean but emits at least one Error
    /// diagnostic, making the assertion meaningful.
    #[test]
    fn update_source_enables_get_diagnostics_without_load_file() {
        let ctx = fresh_ctx();
        let source =
            std::fs::read_to_string(BRACKET_COMPILE_ERROR_PATH).expect("fixture must be readable");
        let update = ctx
            .update_source(BRACKET_COMPILE_ERROR_PATH, &source)
            .expect("update_source should succeed with valid content");
        assert!(
            update.success,
            "update_source must succeed so compiled=Some is set"
        );
        // No load_file / open_file — update_source alone must enable get_diagnostics.
        let diags = ctx
            .get_diagnostics()
            .expect("get_diagnostics should return Ok after update_source alone");
        assert!(
            !diags.is_empty(),
            "bracket_compile_error.ri should produce at least one diagnostic"
        );
        // Use get_source(None) to read back the exact path update_source stored,
        // rather than re-deriving it via std::fs::canonicalize — which may produce a
        // different normalisation (symlink trees, macOS case-folding, etc.) and make
        // the test flaky.
        let expected_path = ctx
            .get_source(None)
            .expect("active_file should be set after update_source")
            .file_path;
        let has_matching_path = diags.iter().any(|d| d.file_path == expected_path);
        assert!(
            has_matching_path,
            "at least one diagnostic must have file_path={:?}; got {:?}",
            expected_path,
            diags
                .iter()
                .map(|d| d.file_path.as_str())
                .collect::<Vec<_>>()
        );
    }

    /// Regression guard: when `load_file("a.ri")` is followed by
    /// `update_source("b.ri", ...)`, `active_file` must switch to b.ri so that
    /// `get_diagnostics` returns spans against b's source — not a's.
    ///
    /// Before the amendment that replaced `get_or_insert_with` with an unconditional
    /// set, `active_file` would remain "a.ri" after the `update_source` call even
    /// though `compiled` had been replaced by b's module.  Any diagnostic whose
    /// label spans index into b's source bytes would then be resolved against a's
    /// source, producing wrong line/column numbers (or panics on out-of-range spans).
    #[test]
    fn update_source_after_load_file_switches_active_file() {
        let ctx = fresh_ctx();
        // Step 1: load a.ri (bracket.ri — parse- and compile-clean).
        ctx.load_file(BRACKET_PATH)
            .expect("load_file should succeed for bracket.ri");

        // Step 2: update_source with b.ri (bracket_compile_error.ri — has diagnostics).
        let source =
            std::fs::read_to_string(BRACKET_COMPILE_ERROR_PATH).expect("fixture must be readable");
        let update = ctx
            .update_source(BRACKET_COMPILE_ERROR_PATH, &source)
            .expect("update_source should succeed for bracket_compile_error.ri");
        assert!(
            update.success,
            "update_source must succeed so compiled=Some is set"
        );

        // Use get_source(None) to read back the exact path update_source stored,
        // rather than re-deriving it via std::fs::canonicalize — which may produce a
        // different normalisation (symlink trees, macOS case-folding, etc.) and make
        // the test flaky.
        let active_path = ctx
            .get_source(None)
            .expect("active_file should be set after update_source")
            .file_path;
        // Independent oracle: active_file must now name b.ri, not the prior a.ri.
        // Uses ends_with rather than std::fs::canonicalize to avoid the symlink /
        // case-folding flake risk noted in the comment above, while still providing
        // an assertion that is decoupled from the all_b coherence check below.
        assert!(
            active_path.ends_with("/bracket_compile_error.ri"),
            "active_file must point to b.ri after update_source(b.ri); got {:?}",
            active_path
        );

        // Diagnostics must be for b.ri (bracket_compile_error has at least one Error).
        let diags = ctx
            .get_diagnostics()
            .expect("get_diagnostics should return Ok");
        assert!(
            !diags.is_empty(),
            "bracket_compile_error.ri should produce at least one diagnostic"
        );
        // Independent diagnostic oracle: both `active_path` and `d.file_path` derive
        // from `state.active_file`, so comparing them would be tautological — the
        // filename suffix is a third independent oracle.
        assert!(
            diags
                .iter()
                .all(|d| d.file_path.ends_with("/bracket_compile_error.ri")),
            "all diagnostics must carry b.ri's path (ends_with bracket_compile_error.ri), got: {:?}",
            diags
                .iter()
                .map(|d| d.file_path.as_str())
                .collect::<Vec<_>>()
        );
    }

    /// Regression guard: `get_diagnostics` must emit severity strings in
    /// PascalCase wire format (`"Error"`, `"Warning"`, `"Info"`) via
    /// `Severity::as_wire_str()`, not the lowercase `Display` form
    /// (`"error"`, `"warning"`, `"info"`) intended for human-readable CLI output.
    ///
    /// This test mirrors the GUI-side pin at
    /// `gui/src-tauri/src/tests/engine_tests.rs::get_diagnostics_severity_strings_match_as_wire_str`.
    /// By tying both transport tests to the same `Severity::as_wire_str()`
    /// source-of-truth, identical wire output is guaranteed transitively.
    ///
    /// `bracket_compile_error.ri` parses cleanly so `load_file` succeeds; it
    /// contains an unresolved-name reference (`unknown_name`) that produces a
    /// compile-time Error diagnostic. `check_compile_error_exits_failure` in
    /// `crates/reify-cli/tests/cli_check.rs` confirms the fixture causes a
    /// non-zero exit — that is informational linkage only (it checks exit
    /// status / stderr, not `Severity::Error` specifically). The `!diags.is_empty()`
    /// and `any(severity == "Error")` assertions below are the authoritative guards
    /// for this test; if `unknown_name` resolution were ever relaxed to a Warning
    /// the final assertion below would fail explicitly here.
    #[test]
    fn get_diagnostics_severity_is_pascal_case_wire_format() {
        use reify_core::Severity;

        let ctx = fresh_ctx();
        ctx.load_file(BRACKET_COMPILE_ERROR_PATH)
            .expect("load_file should succeed for bracket_compile_error.ri (parse-clean fixture)");

        let diags = ctx
            .get_diagnostics()
            .expect("get_diagnostics should succeed");

        assert!(
            !diags.is_empty(),
            "bracket_compile_error.ri must produce at least one diagnostic; \
             fixture or grammar may have changed"
        );

        for d in &diags {
            assert!(
                d.severity == Severity::Error.as_wire_str()
                    || d.severity == Severity::Warning.as_wire_str()
                    || d.severity == Severity::Info.as_wire_str(),
                "get_diagnostics must emit PascalCase wire format (\"Error\"/\"Warning\"/\"Info\") \
                 but got {:?} — do not use Display (which returns lowercase) for MCP wire output",
                d.severity,
            );
        }

        assert!(
            diags
                .iter()
                .any(|d| d.severity == Severity::Error.as_wire_str()),
            "at least one diagnostic must have severity == \"Error\" (PascalCase wire format); \
             bracket_compile_error.ri is known to produce a compile-time Error",
        );
    }

    /// CLI `get_diagnostics` (producer #4: `mcp_context.rs:239`) must set
    /// `has_location == true` for diagnostics that carry a real source span
    /// (non-empty labels), pinning the `!diag.labels.is_empty()` predicate at
    /// the CLI construction site (`mcp_context.rs:230` branch).
    ///
    /// Uses the same `fresh_ctx()` + `BRACKET_COMPILE_ERROR_PATH` fixture as
    /// `get_diagnostics_severity_is_pascal_case_wire_format` (line 1386) — that
    /// fixture is known to produce at least one labelled compile Error with a
    /// real source span.
    ///
    /// **False-branch coverage note:** the `has_location == false` path (labelless
    /// diagnostic → false) is NOT tested here.  The CLI has no `inject_diagnostic_for_test`
    /// equivalent, and no existing real-compiler fixture is known to produce a
    /// labelless `compiled.diagnostics` entry (compile-time name-resolution errors,
    /// which `bracket_compile_error.ri` triggers, always carry a label).  The false
    /// branch is covered by the engine-side test
    /// `get_diagnostics_labelless_fallback_unchanged_after_optimization`
    /// (engine_tests.rs), which exercises the IDENTICAL predicate
    /// `!diag.labels.is_empty()`.  The CLI site at `mcp_context.rs:248` uses
    /// exactly that expression; a regression that hardcodes `true` there would be
    /// caught by a code review of the one changed line.
    #[test]
    fn get_diagnostics_has_location_true_for_spanned_error() {
        let ctx = fresh_ctx();
        ctx.load_file(BRACKET_COMPILE_ERROR_PATH)
            .expect("load_file should succeed for bracket_compile_error.ri");

        let diags = ctx
            .get_diagnostics()
            .expect("get_diagnostics should succeed");

        assert!(
            !diags.is_empty(),
            "bracket_compile_error.ri must produce at least one diagnostic"
        );

        // The fixture is known to produce a labelled compile Error with a real source span.
        // At least one diagnostic must have has_location == true (non-empty labels path).
        assert!(
            diags.iter().any(|d| d.has_location),
            "at least one diagnostic from bracket_compile_error.ri must have has_location = true \
             (labelled Error ⇒ non-empty labels ⇒ real source span)"
        );
    }

    /// Regression guard: `update_source` must prune `state.files` to exactly the
    /// new active canonical key on each call, so `get_open_files()` never returns
    /// more than one entry across repeated file switches.
    ///
    /// Exercises the load_file → update_source(different_file) → update_source(back)
    /// lifecycle introduced in task 3100.  Before the fix (`state.files.retain(…)`),
    /// state.files accumulates one entry per unique path ever passed to update_source
    /// or load_file, growing unboundedly and leaving stale entries reachable via
    /// `get_source(Some(prior_path))`.
    ///
    /// Aligns with the loud-failure invariant from task 3054: `get_diagnostics` and
    /// `get_source_location` already require `active_file ⊆ files.keys()`; this test
    /// strengthens to `files.keys() == {active_file}` post-update_source.
    ///
    /// Path assertions use `ends_with` rather than exact equality or
    /// `std::fs::canonicalize` — same flake-avoidance rationale as
    /// `update_source_after_load_file_switches_active_file` (line 1271-1273): avoids
    /// symlink / case-folding mismatches while remaining an independent oracle.
    #[test]
    fn update_source_drops_prior_files_map_entries() {
        let ctx = fresh_ctx();

        // Read fixture content upfront — we need it for the loop-guard switch-back.
        let content_a =
            std::fs::read_to_string(BRACKET_PATH).expect("bracket.ri fixture must be readable");
        let content_b = std::fs::read_to_string(BRACKET_COMPILE_ERROR_PATH)
            .expect("bracket_compile_error.ri fixture must be readable");

        // --- Phase 1: load_file(a.ri) ---
        ctx.load_file(BRACKET_PATH)
            .expect("load_file should succeed for bracket.ri");

        // Baseline: exactly one file open after load_file.
        assert_eq!(
            ctx.get_open_files().unwrap().len(),
            1,
            "baseline: exactly one file open after load_file"
        );

        // --- Phase 2: update_source(b.ri) — files-map must shrink/stay at 1 ---
        let update = ctx
            .update_source(BRACKET_COMPILE_ERROR_PATH, &content_b)
            .expect("update_source should succeed for bracket_compile_error.ri");
        assert!(update.success, "update_source must succeed for b.ri");

        // Core bound assertion: state.files must not grow across the switch.
        assert_eq!(
            ctx.get_open_files().unwrap().len(),
            1,
            "state.files must be pruned to a single entry after update_source; \
             prior bracket.ri entry must be dropped (not 2)"
        );

        // Independent oracle: the surviving entry names b.ri, not a.ri.
        let open_files = ctx.get_open_files().unwrap();
        assert!(
            open_files[0].path.ends_with("bracket_compile_error.ri"),
            "surviving open file must be bracket_compile_error.ri after update_source; \
             got {:?}",
            open_files[0].path
        );

        // Prior path must no longer be reachable via get_source.
        // ToolError::EngineError("file not open: …") formats as "engine error: file not open: …".
        let err = ctx.get_source(Some(BRACKET_PATH)).unwrap_err();
        let err_str = err.to_string();
        assert!(
            err_str.contains("file not open"),
            "get_source for the prior bracket.ri path must fail with 'file not open'; got: {:?}",
            err
        );
        assert!(
            err_str.contains("bracket.ri"),
            "error message must mention the prior bracket.ri path; got: {:?}",
            err
        );

        // --- Phase 3: loop guard — update_source back to a.ri ---
        let update2 = ctx
            .update_source(BRACKET_PATH, &content_a)
            .expect("update_source back to bracket.ri should succeed");
        assert!(
            update2.success,
            "update_source back to bracket.ri must succeed"
        );

        assert_eq!(
            ctx.get_open_files().unwrap().len(),
            1,
            "state.files must remain bounded after a second update_source switch back"
        );

        let open_files2 = ctx.get_open_files().unwrap();
        assert!(
            open_files2[0].path.ends_with("bracket.ri"),
            "surviving open file must be bracket.ri after switching back; got {:?}",
            open_files2[0].path
        );

        let err2 = ctx
            .get_source(Some(BRACKET_COMPILE_ERROR_PATH))
            .unwrap_err();
        let err2_str = err2.to_string();
        assert!(
            err2_str.contains("file not open"),
            "get_source for the prior bracket_compile_error.ri path must fail with 'file not open'; \
             got: {:?}",
            err2
        );
        assert!(
            err2_str.contains("bracket_compile_error.ri"),
            "error message must mention the prior bracket_compile_error.ri path; got: {:?}",
            err2
        );

        // --- Phase 4: fresh_ctx → update_source(a) → update_source(b) ---
        // Exercises the pure update_source pathway (no preceding load_file) — the
        // most common MCP client path where the editor calls update_source directly
        // without ever going through load_file.  Confirms the retain() bound holds
        // even when state.files starts empty.
        let ctx2 = fresh_ctx();

        ctx2.update_source(BRACKET_PATH, &content_a)
            .expect("update_source(a) on fresh ctx should succeed");
        assert_eq!(
            ctx2.get_open_files().unwrap().len(),
            1,
            "phase 4: exactly one file open after first update_source on fresh ctx"
        );

        ctx2.update_source(BRACKET_COMPILE_ERROR_PATH, &content_b)
            .expect("update_source(b) after update_source(a) should succeed");

        let open3 = ctx2.get_open_files().unwrap();
        assert_eq!(
            open3.len(),
            1,
            "phase 4: state.files must be pruned to 1 after update_source switch on fresh ctx"
        );
        assert!(
            open3[0].path.ends_with("bracket_compile_error.ri"),
            "phase 4: surviving entry must be bracket_compile_error.ri; got {:?}",
            open3[0].path
        );

        let err3 = ctx2.get_source(Some(BRACKET_PATH)).unwrap_err();
        let err3_str = err3.to_string();
        assert!(
            err3_str.contains("file not open"),
            "phase 4: get_source for prior bracket.ri path must fail with 'file not open'; \
             got: {:?}",
            err3
        );
        assert!(
            err3_str.contains("bracket.ri"),
            "phase 4: error message must mention the prior bracket.ri path; got: {:?}",
            err3
        );
    }

    /// Shared three-phase invariant checker for singleton-`files` pruning tests.
    ///
    /// Invokes `op` in place of `load_file` / `open_file` and asserts that
    /// `state.files.keys() == {active_file}` holds after every call — i.e.
    /// the entry for the prior path is dropped (not accumulated).
    ///
    /// Phase structure (a = BRACKET_PATH, b = BRACKET_COMPILE_ERROR_PATH):
    ///   Phase 1 — op(a): baseline, len == 1.
    ///   Phase 2 — op(b): len still 1; surviving entry ends_with(b);
    ///             get_source(a) errors with "file not open".
    ///   Phase 3 — op(a) back: len still 1; surviving entry ends_with(a);
    ///             get_source(b) errors with "file not open".
    ///
    /// Path assertions use `ends_with` rather than exact equality — same
    /// flake-avoidance rationale as `update_source_after_load_file_switches_active_file`.
    fn assert_singleton_files_invariant(ctx: &CliToolContext, op: impl Fn(&CliToolContext, &str)) {
        // --- Phase 1: op(a) ---
        op(ctx, BRACKET_PATH);
        assert_eq!(
            ctx.get_open_files().unwrap().len(),
            1,
            "baseline: exactly one file open after first op call"
        );

        // --- Phase 2: op(b) — files-map must NOT grow to 2 ---
        op(ctx, BRACKET_COMPILE_ERROR_PATH);
        assert_eq!(
            ctx.get_open_files().unwrap().len(),
            1,
            "state.files must be pruned to a single entry after op(b); \
             prior bracket.ri entry must be dropped (not 2)"
        );
        let open_files = ctx.get_open_files().unwrap();
        assert!(
            open_files[0].path.ends_with("bracket_compile_error.ri"),
            "surviving entry must be bracket_compile_error.ri after op(b); got {:?}",
            open_files[0].path
        );
        // ToolError::EngineError("file not open: …") formats as "engine error: file not open: …".
        let err = ctx.get_source(Some(BRACKET_PATH)).unwrap_err();
        let err_str = err.to_string();
        assert!(
            err_str.contains("file not open"),
            "get_source(bracket.ri) must fail with 'file not open' after op(b); got: {:?}",
            err
        );
        assert!(
            err_str.contains("bracket.ri"),
            "error must mention bracket.ri; got: {:?}",
            err
        );

        // --- Phase 3: loop guard — op(a) back ---
        op(ctx, BRACKET_PATH);
        assert_eq!(
            ctx.get_open_files().unwrap().len(),
            1,
            "state.files must remain bounded after switching back to a"
        );
        let open_files2 = ctx.get_open_files().unwrap();
        assert!(
            open_files2[0].path.ends_with("bracket.ri"),
            "surviving entry must be bracket.ri after switching back; got {:?}",
            open_files2[0].path
        );
        let err2 = ctx
            .get_source(Some(BRACKET_COMPILE_ERROR_PATH))
            .unwrap_err();
        let err2_str = err2.to_string();
        assert!(
            err2_str.contains("file not open"),
            "get_source(bracket_compile_error.ri) must fail with 'file not open'; got: {:?}",
            err2
        );
        assert!(
            err2_str.contains("bracket_compile_error.ri"),
            "error must mention bracket_compile_error.ri; got: {:?}",
            err2
        );
    }

    /// Verify that `load_file` prunes `state.files` to a singleton on every call,
    /// so that `get_open_files()` never advertises more than one file.
    ///
    /// Pre-fix, `load_file`'s `state.files.insert(...)` accumulated entries — calling
    /// `load_file(a)` then `load_file(b)` left both `a` and `b` in `state.files`,
    /// while `compiled` / `active_file` reflected only `b`.
    ///
    /// Post-fix: all three state-mutating entry points (`update_source`, `load_file`,
    /// `open_file`) maintain `state.files.keys() == {active_file}` — see task 3183.
    #[test]
    fn load_file_drops_prior_files_map_entries() {
        let ctx = fresh_ctx();
        assert_singleton_files_invariant(&ctx, |ctx, path| {
            ctx.load_file(path).expect("load_file should succeed");
        });
    }

    /// Verify that `open_file` prunes `state.files` to a singleton on every call,
    /// so that `get_open_files()` never advertises more than one file.
    ///
    /// Pre-fix, `open_file`'s `state.files.insert(...)` accumulated entries — calling
    /// `open_file(a)` then `open_file(b)` left both `a` and `b` in `state.files`,
    /// while `compiled` / `active_file` reflected only `b`.
    ///
    /// Post-fix: all three state-mutating entry points (`update_source`, `load_file`,
    /// `open_file`) maintain `state.files.keys() == {active_file}` — see task 3183.
    #[test]
    fn open_file_drops_prior_files_map_entries() {
        let ctx = fresh_ctx();
        assert_singleton_files_invariant(&ctx, |ctx, path| {
            ctx.open_file(path).expect("open_file should succeed");
        });
    }

    /// Regression: when `open_file` opens a parse-failing .ri file after a
    /// successful prior load, `state.compiled` must be cleared — NOT left as a
    /// stale pointer to the previous module.
    ///
    /// Phase 1 uses `BRACKET_COMPILE_ERROR_PATH` (parse-clean, compile-time
    /// diagnostics present) rather than a fully-clean fixture.  That matters
    /// because a fully-clean fixture produces no diagnostics in `state.compiled`,
    /// so `get_diagnostics()` returns `[]` even if `state.compiled` is stale —
    /// the core assertion would pass regardless of the line-607 fix and the test
    /// would not catch the regression.  With a compile-error fixture, `state.compiled`
    /// carries labelled diagnostics whose byte-spans target `bracket_compile_error.ri`'s
    /// source; if the subsequent `open_file(BRACKET_PARSE_ERROR_PATH)` leaves
    /// `state.compiled` intact those stale spans are resolved against the parse-error
    /// file's source and `get_diagnostics()` returns non-empty (wrong-line/col) output.
    /// Setting `state.compiled = None` on the non-pipeline path (line 607) keeps
    /// (active_file, files, compiled) mutually consistent and makes the core assertion
    /// pass correctly.
    #[test]
    fn open_file_parse_error_clears_compiled() {
        let ctx = fresh_ctx();

        // Open bracket_compile_error.ri — parse-clean but carries compile-time
        // diagnostics.  After this call state.compiled holds a module whose
        // byte-spans target bracket_compile_error.ri's source.
        ctx.open_file(BRACKET_COMPILE_ERROR_PATH)
            .expect("open_file should succeed for bracket_compile_error.ri");

        let open_files_1 = ctx.get_open_files().unwrap();
        assert_eq!(
            open_files_1.len(),
            1,
            "sanity: one file open after bracket_compile_error.ri"
        );

        // Precondition: confirm that the compile-error fixture actually produces
        // non-empty diagnostics — otherwise the core assertion below could pass
        // even if state.compiled is NOT cleared (the stale module's diagnostic
        // list would also be empty, producing a false green).
        let pre_diags = ctx
            .get_diagnostics()
            .expect("get_diagnostics should succeed after open_file(bracket_compile_error.ri)");
        assert!(
            !pre_diags.is_empty(),
            "precondition: bracket_compile_error.ri must produce non-empty diagnostics \
             so the byte-spans would misresolve against the parse-error fixture's source \
             if state.compiled were not cleared; got {pre_diags:?}"
        );

        // Open a parse-failing .ri file — open_file should still succeed (the file
        // is registered), but compiled must be cleared.
        ctx.open_file(BRACKET_PARSE_ERROR_PATH)
            .expect("open_file should succeed (registers file) for bracket_parse_error.ri");

        let open_files_2 = ctx.get_open_files().unwrap();
        assert_eq!(
            open_files_2.len(),
            1,
            "one file open after bracket_parse_error.ri"
        );
        assert!(
            open_files_2[0].path.ends_with("bracket_parse_error.ri"),
            "active file must be bracket_parse_error.ri; got {:?}",
            open_files_2[0].path
        );

        // Core assertion: get_diagnostics() must return an empty Vec — NOT
        // diagnostics from the prior bracket_compile_error.ri module resolved against
        // bracket_parse_error.ri's source (which would produce wrong line/columns).
        let diagnostics = ctx
            .get_diagnostics()
            .expect("get_diagnostics should succeed after open_file(parse_error.ri)");
        assert!(
            diagnostics.is_empty(),
            "get_diagnostics() must be empty after opening a parse-failing .ri: \
             compiled must be cleared to prevent stale-span resolution; got {diagnostics:?}"
        );
    }
}
