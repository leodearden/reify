// CliToolContext — real engine-backed implementation of ReifyToolContext for CLI mode

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use reify_compiler::ValueCellKind;
use reify_mcp::{
    ConstraintInfo, DiagnosticInfo, EvalStatusInfo, OpenFileInfo, ParameterInfo, ReifyToolContext,
    SelectionInfo, SetParamResult, SourceContent, SourceLocationInfo, ToolError, UpdateResult,
};
use reify_types::{DeterminacyState, Value, ValueCellId};

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
    /// User-set parameter overrides, tracked so they can be re-applied after
    /// `eval()` (which rebuilds the snapshot from module defaults and does not
    /// consult `Engine::param_overrides`).  Cleared by `load_file` and
    /// `open_file` which semantically start over with a new file.
    user_overrides: Vec<(ValueCellId, Value)>,
    /// Dedupe set for `reapply_user_overrides` warn events.  Tracks `(cell_id,
    /// error-variant)` pairs that have already been warned about so that
    /// repeated save cycles with a stale override downgrade subsequent
    /// occurrences to `tracing::debug!`.  Cleared alongside `user_overrides`
    /// on `load_file` / `open_file`.  When a cell applies successfully its
    /// entry is removed so a future re-type-change warns again.
    warned_overrides: std::collections::HashSet<(ValueCellId, &'static str)>,
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
                user_overrides: Vec::new(),
                warned_overrides: std::collections::HashSet::new(),
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

        let parsed = reify_syntax::parse(&source, reify_types::ModulePath::single(module_name));

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
        state.user_overrides.clear();
        state.warned_overrides.clear();
        ensure_engine(&mut state).eval(&compiled);
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
/// `Engine::eval()` rebuilds the snapshot from module defaults and does **not** read
/// the engine's internal `param_overrides` map.  Consequently, after every `eval()`
/// call `get_parameters()` returns module defaults, not user-set overrides.  The
/// internal `param_overrides` map is only consulted by `eval_cached()`, which is not
/// used by the MCP context.  User overrides are instead tracked in
/// `CliState::user_overrides` and re-applied explicitly after `eval()` via
/// `reapply_user_overrides()`.  `load_file` and `open_file` clear `user_overrides`
/// because those operations semantically start over with a fresh file.
fn ensure_engine(state: &mut CliState) -> &mut reify_eval::Engine {
    #[cfg(test)]
    if state.engine.is_none() {
        state.engine_construction_count += 1;
    }
    state
        .engine
        .get_or_insert_with(|| reify_eval::Engine::new(Box::new(reify_constraints::SimpleConstraintChecker), None))
}

/// Re-apply tracked user overrides to the engine after `eval()` has rebuilt the snapshot
/// from module defaults.
///
/// Three cases are distinguished:
///
/// - **`Ok`** — the cell exists and accepted the value; `set_param_and_invalidate` is
///   called to commit the override into the engine.  Any existing warn-dedupe entry for
///   this cell is cleared so a future mismatch (after the user re-types the param) will
///   warn again.
/// - **`Err(CellNotFound)`** — topology changed (the param was deleted or renamed);
///   skip silently and keep the override in `user_overrides` so it can reappear if
///   the topology reverts in a subsequent edit.
/// - **Any other `Err`** (`TypeKindMismatch`, `DimensionMismatch`, `NotInitialized`) —
///   the cell exists but the stored value is incompatible with its current type.  The
///   **first** occurrence emits a `tracing::warn!` event with the cell id and error for
///   diagnostic inspection; subsequent occurrences of the same `(cell_id, error_variant)`
///   pair are downgraded to `tracing::debug!` to avoid log spam when a stale override
///   persists across repeated saves.  The override is kept in `user_overrides` so the
///   user's intent survives a transient mismatch and reapplies when the type becomes
///   compatible again.
fn reapply_user_overrides(state: &mut CliState) {
    if state.user_overrides.is_empty() {
        return;
    }
    let overrides: Vec<(ValueCellId, Value)> = state.user_overrides.clone();

    // Phase 1: apply overrides to the engine; collect outcomes without accessing
    // `state.warned_overrides` while the engine is mutably borrowed.
    let mut succeeded: Vec<ValueCellId> = Vec::new();
    // (cell_id, error-variant tag, Display string of the error)
    let mut mismatches: Vec<(ValueCellId, &'static str, String)> = Vec::new();

    if let Some(engine) = state.engine.as_mut() {
        for (cell_id, value) in overrides {
            match engine.edit_param(cell_id.clone(), value.clone()) {
                Ok(_) => {
                    engine.set_param_and_invalidate(&cell_id, value);
                    succeeded.push(cell_id);
                }
                Err(reify_eval::EngineError::CellNotFound { .. }) => {
                    // Topology changed — skip silently without removing the override
                    // so it can be re-applied if the cell returns to the topology in
                    // a subsequent edit.
                }
                Err(err) => {
                    let variant_tag: &'static str = match &err {
                        reify_eval::EngineError::TypeKindMismatch { .. } => "TypeKindMismatch",
                        reify_eval::EngineError::DimensionMismatch { .. } => "DimensionMismatch",
                        reify_eval::EngineError::NotInitialized => "NotInitialized",
                        reify_eval::EngineError::CellNotFound { .. } => "CellNotFound",
                    };
                    mismatches.push((cell_id, variant_tag, format!("{err}")));
                }
            }
        }
    }

    // Phase 2: update the warn-dedupe set and emit log events.
    // For cells that applied OK, clear their dedupe entries so a future mismatch
    // (after the user re-types the param) will warn again rather than being silenced.
    for cell_id in succeeded {
        state.warned_overrides.retain(|(id, _)| id != &cell_id);
    }

    // Emit warn on first occurrence of a (cell_id, variant) pair; downgrade
    // repeats to debug to reduce log noise across repeated saves.
    for (cell_id, variant_tag, err_display) in mismatches {
        if state.warned_overrides.insert((cell_id.clone(), variant_tag)) {
            tracing::warn!(
                cell_id = %cell_id,
                error = %err_display,
                "reapply_user_overrides: edit_param failed with non-CellNotFound error; \
                 override retained in user_overrides so it can reapply when the topology \
                 becomes compatible"
            );
        } else {
            tracing::debug!(
                cell_id = %cell_id,
                error = %err_display,
                "reapply_user_overrides: mismatch previously warned, downgraded to debug; \
                 override retained in user_overrides"
            );
        }
    }
}

/// Format a dimension as a human-readable unit string.
fn dimension_unit(ty: &reify_types::ty::Type) -> String {
    match ty {
        reify_types::ty::Type::Scalar { dimension } => format!("{}", dimension),
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

    fn get_diagnostics(&self) -> Result<Vec<DiagnosticInfo>, ToolError> {
        let state = self.lock_state();
        let mut result = Vec::new();

        if let Some(compiled) = &state.compiled {
            let file_path = state.active_file.clone().unwrap_or_default();
            let source = state
                .files
                .get(&file_path)
                .map(|f| f.content.as_str())
                .unwrap_or("");

            for diag in &compiled.diagnostics {
                // Use the first label's span if available, otherwise default to (1,1)
                let (line, column, end_line, end_column) = if let Some(label) = diag.labels.first()
                {
                    let (l, c) = reify_types::byte_offset_to_line_col(source, label.span.start as usize);
                    let (el, ec) = reify_types::byte_offset_to_line_col(source, label.span.end as usize);
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
                    severity: format!("{}", diag.severity),
                    message: diag.message.clone(),
                    code: None,
                });
            }
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

    fn get_source_location(&self, entity_path: &str) -> Result<SourceLocationInfo, ToolError> {
        let state = self.lock_state();
        let compiled = state
            .compiled
            .as_ref()
            .ok_or_else(|| ToolError::EngineError("no compiled module".to_string()))?;
        let file_path = state.active_file.clone().unwrap_or_default();
        let source = state
            .files
            .get(&file_path)
            .map(|f| f.content.as_str())
            .unwrap_or("");

        // Search templates for matching entity
        for template in &compiled.templates {
            if template.name == entity_path {
                // Return the span of the first value cell as a proxy for the entity
                if let Some(cell) = template.value_cells.first() {
                    let (line, column) = reify_types::byte_offset_to_line_col(source, cell.span.start as usize);
                    let (end_line, end_column) = reify_types::byte_offset_to_line_col(source, cell.span.end as usize);
                    return Ok(SourceLocationInfo {
                        file_path,
                        line: line as u32,
                        column: column as u32,
                        end_line: end_line as u32,
                        end_column: end_column as u32,
                    });
                }
            }

            // Also check for entity.member pattern
            for cell in &template.value_cells {
                let cell_id_str = format!("{}", cell.id);
                if cell_id_str == entity_path || cell.id.member == entity_path {
                    let (line, column) = reify_types::byte_offset_to_line_col(source, cell.span.start as usize);
                    let (end_line, end_column) = reify_types::byte_offset_to_line_col(source, cell.span.end as usize);
                    return Ok(SourceLocationInfo {
                        file_path,
                        line: line as u32,
                        column: column as u32,
                        end_line: end_line as u32,
                        end_column: end_column as u32,
                    });
                }
            }
        }

        Err(ToolError::EngineError(format!(
            "entity not found: {entity_path}"
        )))
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

        let parsed = reify_syntax::parse(content, reify_types::ModulePath::single(module_name));

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
        let mut state = self.lock_state();
        ensure_engine(&mut state).eval(&compiled);
        // Re-apply user overrides for cells that still exist in the new topology.
        // This preserves parameter values set via set_parameter across topology-
        // preserving edits (e.g. whitespace changes, comment updates).  Overrides
        // for cells removed by a topology-changing edit are silently skipped.
        reapply_user_overrides(&mut state);
        if let Some(entry) = state.files.get_mut(&canonical) {
            entry.content = content.to_string();
            entry.dirty = true;
        } else {
            state.files.insert(
                canonical,
                FileEntry {
                    content: content.to_string(),
                    dirty: true,
                },
            );
        }
        state.compiled = Some(compiled);

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

        let cell_id_obj = reify_types::ValueCellId::new(entity, member);

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
            reify_types::ty::Type::Scalar { dimension } => Value::Scalar {
                si_value: numeric_val,
                dimension: *dimension,
            },
            reify_types::ty::Type::Int => Value::Int(numeric_val as i64),
            reify_types::ty::Type::Real => Value::Real(numeric_val),
            _ => Value::Real(numeric_val),
        };

        // Apply the parameter change via incremental edit.
        // edit_param must succeed before mutating engine state via set_param_and_invalidate:
        // if edit_param fails, param_overrides and cache remain in their last consistent state.
        let engine = state.engine.as_mut().unwrap();
        engine
            .edit_param(cell_id_obj.clone(), new_value.clone())
            .map_err(|e| ToolError::EngineError(format!("incremental eval failed: {e}")))?;
        engine.set_param_and_invalidate(&cell_id_obj, new_value.clone());
        // NLL: `engine` borrow of `state.engine` ends here (last use above).

        // Track override so update_source can re-apply it after eval().
        // Only reached on success — edit_param failure returns early via `?`.
        state.user_overrides.retain(|(id, _)| id != &cell_id_obj);
        state.user_overrides.push((cell_id_obj.clone(), new_value.clone()));

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
        let pipeline_result: Option<reify_compiler::CompiledModule> = if file_path.ends_with(".ri") {
            let module_name = std::path::Path::new(file_path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unnamed");

            let parsed = reify_syntax::parse(&source, reify_types::ModulePath::single(module_name));

            if parsed.errors.is_empty() {
                Some(reify_compiler::compile(&parsed))
            } else {
                None
            }
        } else {
            None
        };

        let mut state = self.lock_state();
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
            state.user_overrides.clear();
            state.warned_overrides.clear();
            ensure_engine(&mut state).eval(&compiled);
            state.compiled = Some(compiled);
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

    fn navigate_to_source(&self, _file: &str, _line: u32, _column: u32) -> Result<bool, ToolError> {
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BRACKET_PATH: &str =
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/bracket.ri");

    // ── Source fixtures for reapply_user_overrides tests ─────────────────────
    // Each variant modifies bracket.ri's `param width` in a way that triggers a
    // different `EngineError` when the `Scalar[LENGTH]` override (0.12 m) is
    // re-applied after `update_source`.

    /// `width` changed from `Scalar` (LENGTH) to `Int` — triggers `TypeKindMismatch`.
    const BRACKET_INT_WIDTH: &str = "\
structure Bracket {
    param width: Int = 80
    param height: Scalar = 100mm
    param thickness: Scalar = 5mm
    param fillet_radius: Scalar = 3mm
    param hole_diameter: Scalar = 6mm

    let volume = height * thickness

    constraint thickness > 2mm
    constraint hole_diameter < thickness * 2

    let body = box(height, height, thickness)
}
";

    /// `width` changed from `Scalar` (LENGTH) to `Mass` — triggers `DimensionMismatch`.
    const BRACKET_MASS_WIDTH: &str = "\
structure Bracket {
    param width: Mass = 5kg
    param height: Scalar = 100mm
    param thickness: Scalar = 5mm
    param fillet_radius: Scalar = 3mm
    param hole_diameter: Scalar = 6mm

    let volume = height * thickness

    constraint thickness > 2mm
    constraint hole_diameter < thickness * 2

    let body = box(height, height, thickness)
}
";

    /// `width` param removed entirely — triggers `CellNotFound` (topology change).
    const BRACKET_NO_WIDTH: &str = "\
structure Bracket {
    param height: Scalar = 100mm
    param thickness: Scalar = 5mm
    param fillet_radius: Scalar = 3mm
    param hole_diameter: Scalar = 6mm

    let volume = height * thickness

    constraint thickness > 2mm
    constraint thickness < height / 4
    constraint hole_diameter < thickness * 2

    let body = box(height, height, thickness)
}
";

    /// Set up a fresh `CliToolContext` with `Bracket.width` overridden to
    /// `0.12` (a `Scalar[LENGTH]` value), then call `update_source` with the
    /// given `replacement` source string.  Returns the number of `WARN` events
    /// emitted by `reify::mcp_context` during `update_source`.
    ///
    /// Uses `CountingSubscriberBuilder::target_prefix("reify::mcp_context")`
    /// (`reify` is the binary crate name from `[[bin]] name = "reify"`) so
    /// only warns from `reapply_user_overrides` are counted — unrelated warns
    /// from the compiler, engine, or constraint solver do not affect the result.
    fn run_reapply_with_source(replacement: &str) -> usize {
        use std::sync::atomic::Ordering;
        use reify_test_support::CountingSubscriberBuilder;

        let (subscriber, counters) = CountingSubscriberBuilder::new()
            .count_level(tracing::Level::WARN)
            .target_prefix("reify::mcp_context")
            .build();
        let _guard = tracing::subscriber::set_default(subscriber);
        let counter = counters[&tracing::Level::WARN].clone();

        let project_dir = PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures"
        ));
        let ctx = CliToolContext::new(project_dir);
        ctx.load_file(BRACKET_PATH).expect("load_file should succeed");
        // Override width to 0.12 m — stores Value::Scalar[LENGTH].
        ctx.set_parameter("Bracket.width", "0.12")
            .expect("set_parameter should succeed");

        let before = counter.load(Ordering::Acquire);

        let result = ctx
            .update_source(BRACKET_PATH, replacement)
            .expect("update_source should return Ok even on override mismatch");
        assert!(result.success, "update_source should succeed (parse/compile passed)");

        counter.load(Ordering::Acquire) - before
    }

    /// Verify that `update_source` reuses the Engine instance rather than constructing
    /// a new one.  Fails against pre-fix code because `update_source` calls
    /// `Engine::new` unconditionally, incrementing `engine_construction_count` to 2.
    #[test]
    fn update_source_reuses_engine_instance() {
        let project_dir = PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures"
        ));
        let ctx = CliToolContext::new(project_dir);

        // Establish an initial engine via load_file (count → 1).
        ctx.load_file(BRACKET_PATH).expect("load_file should succeed");
        let count_after_load = ctx.engine_construction_count();
        assert_eq!(count_after_load, 1, "load_file should construct exactly one engine");

        // Minor valid edit: add a trailing space — topology is unchanged.
        let source =
            std::fs::read_to_string(BRACKET_PATH).expect("fixture must be readable");
        let modified = format!("{source} ");
        let result = ctx
            .update_source(BRACKET_PATH, &modified)
            .expect("update_source should not error");
        assert!(result.success, "update_source should succeed with valid content");

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
        let project_dir = PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures"
        ));
        let ctx = CliToolContext::new(project_dir);

        // First load — engine is created (count → 1).
        ctx.load_file(BRACKET_PATH).expect("first load_file should succeed");
        let count_after_first = ctx.engine_construction_count();
        assert_eq!(count_after_first, 1, "first load_file should construct exactly one engine");

        // Second load of the same path — engine must be reused (count stays at 1).
        ctx.load_file(BRACKET_PATH).expect("second load_file should succeed");
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
        let project_dir = PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures"
        ));
        let ctx = CliToolContext::new(project_dir);

        // First open — engine is created (count → 1).
        ctx.open_file(BRACKET_PATH).expect("first open_file should succeed");
        let count_after_first = ctx.engine_construction_count();
        assert_eq!(count_after_first, 1, "first open_file should construct exactly one engine");

        // Second open of the same path — engine must be reused.
        ctx.open_file(BRACKET_PATH).expect("second open_file should succeed");
        let count_after_second = ctx.engine_construction_count();
        assert_eq!(
            count_after_second, 1,
            "second open_file must reuse the engine \
             (got engine_construction_count={count_after_second})"
        );

        // update_source after open_file must also reuse the same engine.
        let source =
            std::fs::read_to_string(BRACKET_PATH).expect("fixture must be readable");
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
        let project_dir = PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures"
        ));
        let ctx = CliToolContext::new(project_dir);

        ctx.load_file(BRACKET_PATH).expect("load_file should succeed");

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
        let source =
            std::fs::read_to_string(BRACKET_PATH).expect("fixture must be readable");
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

    /// Verify that `reapply_user_overrides` emits exactly one WARN (from
    /// `reify::mcp_context`) when `edit_param` returns `TypeKindMismatch`.
    ///
    /// Uses `run_reapply_with_source` with `BRACKET_INT_WIDTH` which changes
    /// `width` from `Scalar` (LENGTH) to `Int`, making the stored
    /// `Scalar[LENGTH]` override incompatible.  The target-filtered counter
    /// ensures only warns from `reapply_user_overrides` are counted.
    #[test]
    fn reapply_user_overrides_warns_on_type_kind_mismatch() {
        assert_eq!(
            run_reapply_with_source(BRACKET_INT_WIDTH),
            1,
            "TypeKindMismatch must emit exactly one warn from reify_cli::mcp_context"
        );
    }

    /// Verify that `reapply_user_overrides` emits exactly one WARN (from
    /// `reify::mcp_context`) when `edit_param` returns `DimensionMismatch`.
    ///
    /// Uses `run_reapply_with_source` with `BRACKET_MASS_WIDTH` which changes
    /// `width` from `Scalar` (LENGTH) to `Mass`, making the stored `Scalar[LENGTH]`
    /// override dimensionally incompatible.  The target-filtered counter ensures
    /// only warns from `reapply_user_overrides` are counted.
    ///
    /// Note: changing only the unit suffix (e.g. `Scalar = 5kg`) does NOT change
    /// the dimension — `"Scalar"` always resolves to `Type::length()` regardless
    /// of the default literal's unit.  The type annotation itself must be `Mass`.
    #[test]
    fn reapply_user_overrides_warns_on_dimension_mismatch() {
        assert_eq!(
            run_reapply_with_source(BRACKET_MASS_WIDTH),
            1,
            "DimensionMismatch must emit exactly one warn from reify_cli::mcp_context"
        );
    }

    /// Verify that `reapply_user_overrides` emits NO WARN (from
    /// `reify::mcp_context`) when `edit_param` returns `CellNotFound`.
    ///
    /// Uses `run_reapply_with_source` with `BRACKET_NO_WIDTH` which removes the
    /// `width` param entirely.  The silent-skip path must produce delta == 0,
    /// regardless of any unrelated warns from other modules.
    #[test]
    fn reapply_user_overrides_cell_not_found_is_silent() {
        assert_eq!(
            run_reapply_with_source(BRACKET_NO_WIDTH),
            0,
            "CellNotFound must NOT emit a warn — silent skip"
        );
    }

    /// Verify that `load_file` clears prior parameter overrides.  Because
    /// `load_file` semantically starts over with a fresh file, overrides
    /// set before it must not leak into the re-evaluated snapshot.
    #[test]
    fn load_file_clears_param_overrides() {
        let project_dir = PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures"
        ));
        let ctx = CliToolContext::new(project_dir);

        ctx.load_file(BRACKET_PATH).expect("first load_file should succeed");

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
        ctx.load_file(BRACKET_PATH).expect("second load_file should succeed");

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

    /// Regression guard: `update_source` with invalid content must not construct a
    /// new Engine and must leave prior valid state intact (files/compiled/engine
    /// unchanged).  The "invalid input preserves state" invariant is separately
    /// tested via JSON-RPC in tests/mcp_integration.rs; this unit test adds a
    /// fast in-process guard that also verifies the engine is never touched on
    /// parse failure.
    #[test]
    fn update_source_invalid_content_does_not_construct_new_engine() {
        let project_dir = PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures"
        ));
        let ctx = CliToolContext::new(project_dir);

        // Load a valid file to get known-good state.
        ctx.load_file(BRACKET_PATH).expect("load_file should succeed");
        let count_before = ctx.engine_construction_count();
        assert_eq!(count_before, 1);

        // Prior valid state: get_parameters returns results.
        let params_before = ctx.get_parameters().expect("get_parameters should succeed");
        assert!(
            !params_before.is_empty(),
            "should have parameters after loading bracket.ri"
        );

        // Attempt an update with invalid content (parse error).
        let result = ctx
            .update_source(BRACKET_PATH, "enum { }")
            .expect("update_source should return Ok (not Err) even on parse failure");
        assert!(!result.success, "update_source should report failure for invalid content");

        // Engine must not have been (re)constructed.
        let count_after = ctx.engine_construction_count();
        assert_eq!(
            count_after, 1,
            "invalid update_source must not construct a new engine \
             (got engine_construction_count={count_after})"
        );

        // Prior valid parameters must still be accessible.
        let params_after = ctx.get_parameters().expect("get_parameters should still work");
        assert_eq!(
            params_before.len(),
            params_after.len(),
            "parameter list must be unchanged after a failed update_source"
        );
    }
}
