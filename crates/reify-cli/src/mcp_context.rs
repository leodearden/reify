// CliToolContext — real engine-backed implementation of ReifyToolContext for CLI mode

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

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
    project_dir: PathBuf,
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
                project_dir,
            }),
        }
    }

    /// Load a .ri file: read from disk, parse, compile, eval.
    pub fn load_file(&self, path: &str) -> Result<(), String> {
        let source = std::fs::read_to_string(path).map_err(|e| e.to_string())?;

        let module_name = std::path::Path::new(path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unnamed");

        let parsed =
            reify_syntax::parse(&source, reify_types::ModulePath::single(module_name));

        if !parsed.errors.is_empty() {
            let msgs: Vec<String> = parsed.errors.iter().map(|e| e.message.clone()).collect();
            return Err(format!("Parse errors: {}", msgs.join("; ")));
        }

        let compiled = reify_compiler::compile(&parsed);

        let checker = reify_constraints::SimpleConstraintChecker;
        let mut engine = reify_eval::Engine::new(Box::new(checker), None);
        engine.eval(&compiled);

        let abs_path = std::fs::canonicalize(path)
            .unwrap_or_else(|_| PathBuf::from(path))
            .to_string_lossy()
            .to_string();

        let mut state = self.state.lock().unwrap();
        state.files.insert(
            abs_path.clone(),
            FileEntry {
                content: source,
                dirty: false,
            },
        );
        state.active_file = Some(abs_path);
        state.compiled = Some(compiled);
        state.engine = Some(engine);

        Ok(())
    }
}

impl ReifyToolContext for CliToolContext {
    fn get_source(&self, file_path: Option<&str>) -> Result<SourceContent, ToolError> {
        let state = self.state.lock().unwrap();
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
        let state = self.state.lock().unwrap();
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
        Ok(vec![])
    }

    fn get_parameters(&self) -> Result<Vec<ParameterInfo>, ToolError> {
        Ok(vec![])
    }

    fn get_constraints(&self) -> Result<Vec<ConstraintInfo>, ToolError> {
        Ok(vec![])
    }

    fn get_eval_status(&self) -> Result<EvalStatusInfo, ToolError> {
        let state = self.state.lock().unwrap();
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
        Ok(SelectionInfo {
            selected_entity: None,
            hovered_entity: None,
        })
    }

    fn get_source_location(&self, _entity_path: &str) -> Result<SourceLocationInfo, ToolError> {
        Err(ToolError::EngineError(
            "source location lookup not yet implemented".to_string(),
        ))
    }

    fn update_source(&self, _file_path: &str, _content: &str) -> Result<UpdateResult, ToolError> {
        Err(ToolError::NotImplemented)
    }

    fn set_parameter(&self, _cell_id: &str, _value: &str) -> Result<SetParamResult, ToolError> {
        Err(ToolError::NotImplemented)
    }

    fn open_file(&self, _file_path: &str) -> Result<OpenFileInfo, ToolError> {
        Err(ToolError::NotImplemented)
    }

    fn save_file(&self, _file_path: Option<&str>) -> Result<bool, ToolError> {
        Err(ToolError::NotImplemented)
    }

    fn export(&self, _format: &str, _output_path: &str) -> Result<bool, ToolError> {
        Err(ToolError::NotImplemented)
    }

    fn focus_entity(&self, _entity_path: &str) -> Result<bool, ToolError> {
        Ok(false)
    }

    fn navigate_to_source(
        &self,
        _file: &str,
        _line: u32,
        _column: u32,
    ) -> Result<bool, ToolError> {
        Ok(false)
    }
}
