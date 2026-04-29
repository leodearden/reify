// EngineSession — wraps Engine + CompiledModule + source text

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tracing::warn;

use reify_compiler::{CompiledModule, ValueCellKind};
use reify_eval::{CheckResult, Engine};
use reify_eval::cache::NodeId;
use reify_types::{
    ConstraintChecker, ContentHash, DeterminacyState, DimensionVector, ExportFormat, GeometryKernel,
    ModulePath, Satisfaction, Severity, Value, ValueCellId,
};

use reify_types::{Diagnostic, DiagnosticInfo, SourceLocationInfo};

use crate::types::{
    ConstraintData, DefInfo, EntityIdentity, EntityTreeNode, FileData, GuiState, JointDescriptor,
    MechanismDescriptor, MeshData, SourceSpanInfo, ValueData, format_determinacy, format_freshness,
    format_value,
};

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
/// **Current safe mutation sites:** `load_from_source` and `update_source` both
/// delegate all field writes to `commit_state`, which is the single atomic commit
/// point.  Neither method touches `compiled`/`module_name`/`source_map` until
/// after parse, compile, and check have all succeeded.
pub struct EngineSession {
    engine: Engine,
    compiled: Option<CompiledModule>,
    source_map: HashMap<String, String>,
    file_path: Option<PathBuf>,
    last_check: Option<CheckResult>,
    module_name: Option<String>,
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
    parsed_cache: Option<reify_syntax::ParsedModule>,
    /// Cached line-offset table for the currently-loaded source.
    ///
    /// Each entry is the byte position of a `\n` character in the source text.
    /// Populated by `commit_state` via `build_line_offsets(source)` in the same
    /// atomic block as `parsed_cache`.  Set to `None` until the first load;
    /// overwritten on every `commit_state` call.  Used by `get_containing_definition`
    /// to skip the O(M) newline scan on every cursor/hover call.
    line_offsets_cache: Option<Vec<usize>>,
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

impl EngineSession {
    /// Create a new EngineSession with the given constraint checker and optional geometry kernel.
    pub fn new(
        checker: Box<dyn ConstraintChecker>,
        kernel: Option<Box<dyn GeometryKernel>>,
    ) -> Self {
        Self {
            engine: Engine::new(checker, kernel),
            compiled: None,
            source_map: HashMap::new(),
            file_path: None,
            last_check: None,
            module_name: None,
            def_preview_cache: HashMap::new(),
            parsed_cache: None,
            line_offsets_cache: None,
        }
    }

    /// Load source code, parse, compile, evaluate, and return full GUI state.
    pub fn load_from_source(
        &mut self,
        source: &str,
        module_name: &str,
    ) -> Result<GuiState, String> {
        // Parse (prelude-aware so stdlib enum references like `CorrosionClass.C5`
        // disambiguate to `EnumAccess` rather than `MemberAccess`; pairs with
        // `compile_with_stdlib` below). See task 2525.
        let parsed = reify_compiler::parse_with_stdlib(source, ModulePath::single(module_name));

        if !parsed.errors.is_empty() {
            let msgs: Vec<String> = parsed.errors.iter().map(|e| e.message.clone()).collect();
            return Err(format!("Parse errors: {}", msgs.join("; ")));
        }

        // Compile
        let compiled = reify_compiler::compile_with_stdlib(&parsed);

        // Check for compile errors
        let has_errors = compiled
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error);
        if has_errors {
            let msgs: Vec<String> = compiled
                .diagnostics
                .iter()
                .filter(|d| d.severity == Severity::Error)
                .map(|d| d.message.clone())
                .collect();
            return Err(format!("Compile errors: {}", msgs.join("; ")));
        }

        // Evaluate + check constraints (borrows compiled by shared ref, so all
        // field mutations can safely be deferred until after check() returns).
        let check_result = self.engine.check(&compiled);

        // Atomically commit all state after check() succeeds.
        self.commit_state(parsed, compiled, check_result, module_name, source);

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
            .compiled
            .as_ref()
            .ok_or_else(|| "No module loaded".to_string())?;
        let cell_exists = compiled
            .templates
            .iter()
            .any(|t| t.value_cells.iter().any(|vc| vc.id == cell_id));
        if !cell_exists {
            return Err(format!("Unknown parameter '{}'", cell_id_str));
        }

        let check_result = self
            .engine
            .edit_check(cell_id, value)
            .map_err(|e| format!("Engine error: {}", e))?;

        self.last_check = Some(check_result);
        self.build_gui_state()
    }

    /// Load a .ri file from disk.
    pub fn load_file(&mut self, path: &Path) -> Result<GuiState, String> {
        let source = std::fs::read_to_string(path)
            .map_err(|e| format!("Error reading {}: {}", path.display(), e))?;

        let module_name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unnamed");

        self.file_path = Some(path.to_path_buf());
        self.load_from_source(&source, module_name)
    }

    /// Update source code and re-evaluate from scratch.
    ///
    /// Source changes can alter topology, so we create a fresh parse/compile/eval cycle.
    /// The existing engine state (snapshot, caches) is reused where possible via check().
    ///
    /// On any error (parse, compile, or a panic in check()), the session state is left
    /// completely unchanged — source_map, module_name, compiled, and last_check all
    /// retain their previous values. All mutations are deferred until after check() returns.
    pub fn update_source(&mut self, path: &str, content: &str) -> Result<GuiState, String> {
        let module_name = Path::new(path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unnamed");

        // Re-parse and re-compile from scratch (topology may have changed)
        // All state mutation is deferred until after successful parse+compile.
        // Prelude-aware parse so stdlib enum references disambiguate correctly;
        // pairs with `compile_with_stdlib` below. See task 2525.
        let parsed = reify_compiler::parse_with_stdlib(content, ModulePath::single(module_name));

        if !parsed.errors.is_empty() {
            let msgs: Vec<String> = parsed.errors.iter().map(|e| e.message.clone()).collect();
            return Err(format!("Parse errors: {}", msgs.join("; ")));
        }

        let compiled = reify_compiler::compile_with_stdlib(&parsed);

        let has_errors = compiled
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error);
        if has_errors {
            let msgs: Vec<String> = compiled
                .diagnostics
                .iter()
                .filter(|d| d.severity == Severity::Error)
                .map(|d| d.message.clone())
                .collect();
            return Err(format!("Compile errors: {}", msgs.join("; ")));
        }

        // Parse+compile succeeded — run check() before mutating any state, so
        // that a panic in check() leaves the session completely unchanged.
        let check_result = self.engine.check(&compiled);

        // Atomically commit all state after check() succeeds.
        self.commit_state(parsed, compiled, check_result, module_name, content);

        self.build_gui_state()
    }

    /// Atomically commit all session state after a successful parse+compile+check cycle.
    ///
    /// This helper enforces the invariant that `compiled`, `module_name`, and
    /// `source_map` always change together: either all five fields are updated or
    /// none are.  Callers **must** only invoke this after both compilation and
    /// `check()` have succeeded — invoking it on a partially-valid state would
    /// violate the invariant.
    ///
    /// The five-field assignment was previously duplicated in `load_from_source`
    /// and `update_source`; centralising it here prevents the two sites from
    /// drifting apart.
    fn commit_state(
        &mut self,
        parsed: reify_syntax::ParsedModule,
        compiled: CompiledModule,
        check_result: CheckResult,
        module_name: &str,
        source: &str,
    ) {
        self.source_map.clear();
        self.source_map
            .insert(module_key(module_name), source.to_string());
        self.module_name = Some(module_name.to_string());
        self.compiled = Some(compiled);
        self.last_check = Some(check_result);
        // Invalidate def preview cache — new module may have different content hashes.
        self.def_preview_cache.clear();
        // Cache the parse result so get_containing_definition can avoid re-parsing
        // on every cursor/hover call.  Unconditionally overwrites any prior value
        // (never appends) — this is an invalidation, not an accumulation.
        self.parsed_cache = Some(parsed);
        // Cache the line-offset table so get_containing_definition can skip the O(M)
        // newline scan on each call.  Unconditionally overwrites any prior value.
        self.line_offsets_cache = Some(build_line_offsets(source));
    }

    /// Export geometry to a file.
    pub fn export(&mut self, format: ExportFormat, path: &Path) -> Result<(), String> {
        let compiled = self
            .compiled
            .as_ref()
            .ok_or_else(|| "No module loaded".to_string())?;

        let result = self.engine.build(compiled, format);

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
        self.compiled.as_ref()?;
        let name = self.module_name.as_deref()?;
        let key = module_key(name);
        let (k, v) = self.source_map.get_key_value(&key)?;
        Some((k.as_str(), v.as_str()))
    }

    /// Look up source location for an entity path (e.g., "Bracket.width").
    pub fn get_source_location(&self, entity_path: &str) -> Option<SourceLocationInfo> {
        let compiled = self.compiled.as_ref()?;
        let cell_id = parse_cell_id(entity_path).ok()?;

        // Find the span for this cell
        let span = compiled.templates.iter().find_map(|t| {
            t.value_cells
                .iter()
                .find(|vc| vc.id == cell_id)
                .map(|vc| vc.span)
        })?;

        // Delegate source key resolution to resolve_source — returns None when
        // no module is loaded or when the invariant is broken (e.g., via
        // break_source_map_for_test), eliminating duplicated fallible lookup.
        let (file, source) = self.resolve_source()?;

        let (line, col) = reify_types::byte_offset_to_line_col(source, span.start as usize);
        let (end_line, end_col) = reify_types::byte_offset_to_line_col(source, span.end as usize);

        Some(SourceLocationInfo {
            file_path: file.to_owned(),
            line: line as u32,
            column: col as u32,
            end_line: end_line as u32,
            end_column: end_col as u32,
        })
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
        let compiled = match self.compiled.as_ref() {
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

    /// Build the full GUI state from the current engine state.
    pub fn build_gui_state(&mut self) -> Result<GuiState, String> {
        let (compiled, check) = match (self.compiled.as_ref(), self.last_check.as_ref()) {
            (Some(c), Some(k)) => (c, k),
            _ => {
                return Ok(GuiState {
                    meshes: Vec::new(),
                    values: Vec::new(),
                    constraints: Vec::new(),
                    files: Vec::new(),
                    tessellation_diagnostics: Vec::new(),
                });
            }
        };

        // Build values and constraints via shared helpers (also used by
        // build_preview_gui_state) so both paths stay in sync.
        let values = build_values(compiled, check, Some(&self.engine));
        let constraints = build_constraints(compiled, check);

        // Build meshes (from tessellation of realizations) and capture any
        // tessellation diagnostics (e.g. OCCT kernel errors).
        let (meshes, tessellation_diagnostics) = match self.engine.tessellate_snapshot(compiled) {
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
                    let (file_path, source): (&str, &str) =
                        resolved.unwrap_or(("<unknown>", ""));
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
                let meshes = result
                    .meshes
                    .into_iter()
                    .map(|(entity_path, mesh)| MeshData {
                        entity_path,
                        vertices: mesh.vertices,
                        indices: mesh.indices,
                        normals: mesh.normals,
                    })
                    .collect();
                (meshes, tess_diags)
            }
            None => (Vec::new(), Vec::new()),
        };

        // Build files
        let files: Vec<FileData> = self
            .source_map
            .iter()
            .map(|(path, content)| FileData {
                path: path.clone(),
                content: content.clone(),
            })
            .collect();

        Ok(GuiState {
            meshes,
            values,
            constraints,
            files,
            tessellation_diagnostics,
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
    /// Joint extraction and AST-based driving-param resolution are added in
    /// subsequent steps (steps 5–12 of the task plan).
    pub fn get_mechanism_descriptors(&self) -> Vec<MechanismDescriptor> {
        let (compiled, check) = match (self.compiled.as_ref(), self.last_check.as_ref()) {
            (Some(c), Some(k)) => (c, k),
            _ => return Vec::new(),
        };

        let mut descriptors = Vec::new();

        for template in &compiled.templates {
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

                // Count bodies for the descriptor (bodies_count).
                let bodies_count = match map.get(&Value::String("bodies".to_string())) {
                    Some(Value::List(bodies)) => bodies.len(),
                    _ => 0,
                };

                descriptors.push(MechanismDescriptor {
                    cell_id: cell.id.to_string(),
                    entity_path: cell.id.entity.clone(),
                    name: cell.id.member.clone(),
                    bodies_count,
                    // Joint extraction added in steps 5–6.
                    joints: Vec::new(),
                });
            }
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
        let compiled = match self.compiled.as_ref() {
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
            .map(|t| build_template_node(t, &t.name, compiled, Some(&self.engine)))
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
        let compiled = match self.compiled.as_ref() {
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
            let structural_fingerprint =
                format!("{}:{}:{}:{}", entity_kind, "<root>", sub_count, children_hash);

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
                .compiled
                .as_ref()
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
                .compiled
                .as_ref()
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
        self.def_preview_cache
            .insert(cache_key, gui_state.clone());
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

        // Walk top-level declarations and find the innermost (smallest span) that
        // contains the given byte offset.
        let mut best: Option<DefInfo> = None;
        for decl in &parsed.declarations {
            let (name, kind, span) = match decl {
                reify_syntax::Declaration::Structure(s) => {
                    (s.name.as_str(), "structure", s.span)
                }
                reify_syntax::Declaration::Occurrence(o) => {
                    (o.name.as_str(), "occurrence", o.span)
                }
                _ => continue,
            };
            if offset >= span.start && offset < span.end {
                let is_smaller = best.as_ref().is_none_or(|b| {
                    (span.end - span.start) < (b.span.end - b.span.start)
                });
                if is_smaller {
                    best = Some(DefInfo {
                        name: name.to_string(),
                        kind: kind.to_string(),
                        span: SourceSpanInfo {
                            start: span.start,
                            end: span.end,
                        },
                    });
                }
            }
        }
        best
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
                reify_types::Value::Undef => {
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
    }
}

/// Build an `EntityTreeNode` for a topology template.
///
/// `entity_path` is the dot-separated path used as the root of this node's
/// children (e.g. `"Bracket"` → children are `"Bracket.width"`, etc.).
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
                build_template_node(child_template, &sub_path, compiled, engine).children
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
    }
}

/// Test helpers — compiled out of production binaries.
#[cfg(test)]
impl EngineSession {
    /// Inject a diagnostic directly into the compiled module's diagnostics vec,
    /// enabling tests to exercise the `diag.labels.first() == None` fallback path
    /// without needing the compiler to produce such a diagnostic.
    ///
    /// # Panics
    /// Panics if no module is currently loaded (`self.compiled` is `None`).
    pub(crate) fn inject_diagnostic_for_test(&mut self, diag: reify_types::Diagnostic) {
        self.compiled
            .as_mut()
            .expect("inject_diagnostic_for_test: no compiled module loaded")
            .diagnostics
            .push(diag);
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
        self.module_name.take();
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
        self.source_map.clear();
    }

    /// Return a reference to the cached `ParsedModule`, or `None` if no module
    /// has been loaded yet.
    ///
    /// Intended only for tests that need to inspect cache state without widening
    /// the production API.
    pub(crate) fn parsed_cache_for_test(&self) -> Option<&reify_syntax::ParsedModule> {
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
    pub(crate) fn override_parsed_cache_for_test(&mut self, parsed: reify_syntax::ParsedModule) {
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
        self.compiled = Some(compiled);
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
    pub(crate) fn set_panic_on_eval_for_test(&mut self, cell: reify_types::ValueCellId) {
        self.engine.set_panic_on_eval(cell);
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
        if let Some(compiled) = self.compiled.as_ref().cloned() {
            let check_result = self.engine.check(&compiled);
            self.compiled = Some(compiled);
            self.last_check = Some(check_result);
        }
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
        if let Some(compiled) = self.compiled.as_ref().cloned() {
            // Discards the BuildResult — callers read freshness via get_entity_tree().
            let _ = self.engine.build(&compiled, ExportFormat::Step);
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
        cell: reify_types::ValueCellId,
        error_msg: &str,
    ) {
        let node = reify_eval::cache::NodeId::Value(cell);
        self.engine
            .cache_store_mut()
            .mark_failed(&node, reify_types::ErrorRef::new(error_msg));
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
fn format_expr(expr: &reify_types::CompiledExpr) -> String {
    use reify_types::CompiledExprKind;

    match &expr.kind {
        CompiledExprKind::Literal(v) => {
            let (val, unit) = crate::types::format_value(v);
            if unit.is_empty() {
                val
            } else {
                format!("{}{}", val, unit)
            }
        }
        CompiledExprKind::ValueRef(id) => id.member.clone(),
        CompiledExprKind::BinOp { op, left, right } => {
            let op_str = match op {
                reify_types::BinOp::Add => "+",
                reify_types::BinOp::Sub => "-",
                reify_types::BinOp::Mul => "*",
                reify_types::BinOp::Div => "/",
                reify_types::BinOp::Mod => "%",
                reify_types::BinOp::Pow => "**",
                reify_types::BinOp::Eq => "==",
                reify_types::BinOp::Ne => "!=",
                reify_types::BinOp::Lt => "<",
                reify_types::BinOp::Le => "<=",
                reify_types::BinOp::Gt => ">",
                reify_types::BinOp::Ge => ">=",
                reify_types::BinOp::And => "&&",
                reify_types::BinOp::Or => "||",
            };
            format!("{} {} {}", format_expr(left), op_str, format_expr(right))
        }
        CompiledExprKind::UnOp { op, operand } => {
            let op_str = match op {
                reify_types::UnOp::Neg => "-",
                reify_types::UnOp::Not => "!",
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
                reify_types::QuantifierKind::ForAll => "forall",
                reify_types::QuantifierKind::Exists => "exists",
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
                reify_types::DeterminacyPredicateKind::Determined => "determined",
                reify_types::DeterminacyPredicateKind::Undetermined => "undetermined",
                reify_types::DeterminacyPredicateKind::Constrained => "constrained",
                reify_types::DeterminacyPredicateKind::PartiallyDetermined => {
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
                reify_types::SelectorKind::Face => "face",
                reify_types::SelectorKind::Point => "point",
                reify_types::SelectorKind::Edge => "edge",
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
    }
}

/// Collect all ValueCellId references from a compiled expression.
fn collect_value_refs(expr: &reify_types::CompiledExpr) -> Vec<String> {
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
fn diagnostics_to_info(diagnostics: &[Diagnostic], file_path: &str, source: &str) -> Vec<DiagnosticInfo> {
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

/// Pre-compute byte positions of all `\n` characters in `source` in O(M).
///
/// Returns a sorted `Vec<usize>` of the byte offset of each newline.
/// Pass this to [`offset_to_line_col_fast`] to binary-search for line/col
/// in O(log M) instead of the O(M) scan done by [`reify_types::byte_offset_to_line_col`].
pub(crate) fn build_line_offsets(source: &str) -> Vec<usize> {
    source
        .bytes()
        .enumerate()
        .filter_map(|(i, b)| if b == b'\n' { Some(i) } else { None })
        .collect()
}

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
    if offset == reify_types::SourceSpan::PRELUDE_SENTINEL_OFFSET {
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

/// Convert a 1-based `(line, col)` pair to a byte offset using a pre-built
/// newline table.
///
/// `line_offsets` must be the result of [`build_line_offsets`] for the same
/// `source`.  Both `line` and `col` are 1-based and count **Unicode codepoints**.
///
/// - If `line` or `col` is 0, returns 0 as a safe fallback.
/// - If `line` exceeds the number of lines, returns `source.len()`.
/// - If `col` exceeds the line length, clamps to the end of the line.
pub(crate) fn line_col_to_byte_offset_with_offsets(
    source: &str,
    line: u32,
    col: u32,
    line_offsets: &[usize],
) -> usize {
    if line == 0 || col == 0 {
        return 0;
    }
    let line = line as usize;
    let col = col as usize;

    // Byte index of the first character on the target line.
    let line_start = if line <= 1 {
        0
    } else {
        match line_offsets.get(line - 2) {
            Some(&nl) => nl + 1, // byte after the preceding newline
            None => return source.len(), // line is beyond end of source
        }
    };

    // Slice to the end of the target line (not end of source) so that an
    // out-of-bounds col clamps to the line boundary rather than counting
    // codepoints past the '\n' into subsequent lines.
    let line_end = source[line_start..]
        .find('\n')
        .map(|i| line_start + i)
        .unwrap_or(source.len());
    let line_text = &source[line_start..line_end];
    line_start
        + line_text
            .char_indices()
            .nth(col - 1)
            .map(|(i, _)| i)
            .unwrap_or(line_text.len())
}
