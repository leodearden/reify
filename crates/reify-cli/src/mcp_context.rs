// CliToolContext — real engine-backed implementation of ReifyToolContext for CLI mode

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use reify_compiler::ValueCellKind;
use reify_mcp::{
    ConstraintInfo, DiagnosticInfo, EvalStatusInfo, OpenFileInfo, ParameterInfo, ReifyToolContext,
    SelectionInfo, SetParamResult, SourceContent, SourceLocationInfo, ToolError, UpdateResult,
};
use reify_types::{DeterminacyState, Value};

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
        let parsed = reify_compiler::parse_with_stdlib(
            &source,
            reify_types::ModulePath::single(module_name),
        );

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
                    let (l, c) =
                        reify_types::byte_offset_to_line_col(source, label.span.start as usize);
                    let (el, ec) =
                        reify_types::byte_offset_to_line_col(source, label.span.end as usize);
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
                    let (line, column) =
                        reify_types::byte_offset_to_line_col(source, cell.span.start as usize);
                    let (end_line, end_column) =
                        reify_types::byte_offset_to_line_col(source, cell.span.end as usize);
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
                    let (line, column) =
                        reify_types::byte_offset_to_line_col(source, cell.span.start as usize);
                    let (end_line, end_column) =
                        reify_types::byte_offset_to_line_col(source, cell.span.end as usize);
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

        // Prelude-aware parse for AST-shape consistency across reify-lsp/-cli;
        // see task 2525.
        let parsed = reify_compiler::parse_with_stdlib(
            content,
            reify_types::ModulePath::single(module_name),
        );

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
                reify_types::ModulePath::single(module_name),
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
        assert_ne!(overridden_width, default_width, "width override must differ from default");
        assert_ne!(overridden_height, default_height, "height override must differ from default");
        assert_ne!(overridden_thickness, default_thickness, "thickness override must differ from default");

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
        let parsed = reify_syntax::parse(
            INVALID_PARSE_INPUT,
            reify_types::ModulePath::single("probe"),
        );
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
}
