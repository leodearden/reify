use std::sync::{Arc, Mutex, RwLock};

use reify_constraints::SimpleConstraintChecker;
use reify_mcp::{ReifyToolContext, SelectionInfo};
use reify_test_support::{MockGeometryKernel, bracket_source, warn_source_with_unknown_port_type};

use crate::engine::EngineSession;
use crate::mcp_context::{TauriToolContext, mcp_tool_call_impl};

fn make_loaded_session() -> EngineSession {
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let mut session = EngineSession::new(Box::new(checker), Some(Box::new(kernel)));
    session
        .load_from_source(bracket_source(), "bracket")
        .expect("initial load");
    session
}

fn make_tauri_context() -> TauriToolContext {
    let session = make_loaded_session();
    let engine = Arc::new(Mutex::new(session));
    TauriToolContext::builder(engine).build()
}

/// Helper for step-9: create a TauriToolContext loaded with arbitrary source.
/// Mirrors make_loaded_session() but accepts parameterized source and module name.
fn make_tauri_context_with_source(source: &str, module_name: &str) -> TauriToolContext {
    let checker = SimpleConstraintChecker;
    let mut session = EngineSession::new(Box::new(checker), None);
    session
        .load_from_source(source, module_name)
        .expect("load_from_source should succeed");
    let engine = Arc::new(Mutex::new(session));
    TauriToolContext::builder(engine).build()
}

// --- Read method tests ---

#[test]
fn get_source_returns_bracket_content() {
    let ctx = make_tauri_context();
    let source = ctx.get_source(None).expect("get_source should succeed");
    assert!(
        source.content.contains("structure Bracket"),
        "source should contain bracket structure"
    );
    assert_eq!(source.file_path, "bracket.ri");
}

#[test]
fn get_open_files_returns_bracket_file() {
    let ctx = make_tauri_context();
    let files = ctx.get_open_files().expect("get_open_files should succeed");
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, "bracket.ri");
    assert_eq!(files[0].language, "reify");
    assert!(!files[0].dirty);
}

#[test]
fn get_parameters_returns_bracket_params() {
    let ctx = make_tauri_context();
    let params = ctx.get_parameters().expect("get_parameters should succeed");
    assert!(!params.is_empty(), "should have parameters");

    // Check that expected params exist
    let width = params.iter().find(|p| p.name == "width");
    assert!(width.is_some(), "should have width parameter");
    let width = width.unwrap();
    assert_eq!(width.cell_id, "Bracket.width");
    assert_eq!(width.value, "80");
    assert_eq!(width.unit, "mm");
    assert_eq!(width.kind, "Param");
    assert_eq!(width.determinacy, "determined");

    let height = params.iter().find(|p| p.name == "height");
    assert!(height.is_some(), "should have height parameter");
    let height = height.unwrap();
    assert_eq!(height.cell_id, "Bracket.height");
    assert_eq!(height.value, "100");
    assert_eq!(height.unit, "mm");

    let thickness = params.iter().find(|p| p.name == "thickness");
    assert!(thickness.is_some(), "should have thickness parameter");
    let thickness = thickness.unwrap();
    assert_eq!(thickness.cell_id, "Bracket.thickness");
    assert_eq!(thickness.value, "5");
    assert_eq!(thickness.unit, "mm");
}

#[test]
fn get_constraints_returns_satisfied() {
    let ctx = make_tauri_context();
    let constraints = ctx
        .get_constraints()
        .expect("get_constraints should succeed");
    assert!(!constraints.is_empty(), "should have constraints");

    // All bracket constraints should be satisfied at default values
    for c in &constraints {
        assert_eq!(
            c.status, "Satisfied",
            "constraint {} should be satisfied",
            c.node_id
        );
    }
}

#[test]
fn get_eval_status_returns_idle() {
    let ctx = make_tauri_context();
    let status = ctx
        .get_eval_status()
        .expect("get_eval_status should succeed");
    assert_eq!(status.phase, "idle");
    assert_eq!(status.dirty_count, 0);
}

#[test]
fn get_selection_returns_empty() {
    let ctx = make_tauri_context();
    let selection = ctx.get_selection().expect("get_selection should succeed");
    assert!(selection.selected_entity.is_none());
    assert!(selection.hovered_entity.is_none());
}

#[test]
fn get_source_location_for_width() {
    let ctx = make_tauri_context();
    let loc = ctx
        .get_source_location("Bracket.width")
        .expect("get_source_location should succeed for Bracket.width");
    assert_eq!(loc.file_path, "bracket.ri");
    assert!(loc.line >= 1, "line should be positive");
}

#[test]
fn get_source_location_nonexistent_returns_error() {
    let ctx = make_tauri_context();
    let result = ctx.get_source_location("Nonexistent.param");
    assert!(
        result.is_err(),
        "should return error for nonexistent entity"
    );
}

/// Regression guard: get_diagnostics returns empty for a clean-compiled module.
/// Renamed from get_diagnostics_returns_empty to clarify it tests real engine wiring
/// (step-5 replaced the Ok(Vec::new()) stub with a real EngineSession delegate).
#[test]
fn get_diagnostics_returns_empty_for_clean_source() {
    let ctx = make_tauri_context();
    let diags = ctx
        .get_diagnostics()
        .expect("get_diagnostics should succeed");
    assert!(
        diags.is_empty(),
        "bracket source has no warnings → diagnostics should be empty"
    );
}

// --- Write method tests ---

#[test]
fn update_source_with_valid_source_succeeds() {
    let ctx = make_tauri_context();
    let new_source = bracket_source().replace("80mm", "120mm");
    let result = ctx
        .update_source("bracket.ri", &new_source)
        .expect("update_source should succeed");
    assert!(result.success);
}

#[test]
fn update_source_with_invalid_source_returns_error() {
    let ctx = make_tauri_context();
    let result = ctx.update_source("bracket.ri", "this is not valid reify source {{{");
    assert!(result.is_err(), "should return error for invalid source");
}

#[test]
fn set_parameter_succeeds() {
    let ctx = make_tauri_context();
    let result = ctx
        .set_parameter("Bracket.width", "100mm")
        .expect("set_parameter should succeed");
    assert!(result.success);
    assert_eq!(result.new_value, "100");
    assert_eq!(result.unit, "mm");
}

#[test]
fn set_parameter_invalid_cell_returns_error() {
    let ctx = make_tauri_context();
    let result = ctx.set_parameter("Nonexistent.param", "100mm");
    assert!(result.is_err(), "should return error for invalid cell_id");
}

#[test]
fn open_file_reads_from_disk() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test_open.ri");
    std::fs::write(&path, bracket_source()).unwrap();

    let ctx = make_tauri_context();
    let result = ctx
        .open_file(path.to_str().unwrap())
        .expect("open_file should succeed");
    assert_eq!(result.path, path.to_str().unwrap());
    assert_eq!(result.language, "reify");
    assert!(!result.dirty);
}

#[test]
fn save_file_writes_to_disk() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test_save.ri");

    let ctx = make_tauri_context();
    // save_file writes the source_map content for the first file to the given path
    let result = ctx
        .save_file(Some(path.to_str().unwrap()))
        .expect("save_file should succeed");
    assert!(result);

    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("structure Bracket"));
}

#[test]
fn export_writes_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test_export.step");

    let ctx = make_tauri_context();
    let result = ctx
        .export("step", path.to_str().unwrap())
        .expect("export should succeed");
    assert!(result);
    assert!(path.exists(), "exported file should exist");
}

// --- Navigation/event method tests ---

#[test]
fn focus_entity_with_emitter_records_event() {
    let session = make_loaded_session();
    let engine = Arc::new(Mutex::new(session));
    let events: Arc<Mutex<Vec<(String, serde_json::Value)>>> = Arc::new(Mutex::new(Vec::new()));
    let events_clone = events.clone();

    let ctx = TauriToolContext::builder(engine)
        .with_event_emitter(move |name, payload| {
            events_clone
                .lock()
                .unwrap()
                .push((name.to_string(), payload));
        })
        .build();

    let result = ctx
        .focus_entity("Bracket.width")
        .expect("focus_entity should succeed");
    assert!(result);

    let recorded = events.lock().unwrap();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].0, "focus-entity");
}

#[test]
fn navigate_to_source_with_emitter_records_event() {
    let session = make_loaded_session();
    let engine = Arc::new(Mutex::new(session));
    let events: Arc<Mutex<Vec<(String, serde_json::Value)>>> = Arc::new(Mutex::new(Vec::new()));
    let events_clone = events.clone();

    let ctx = TauriToolContext::builder(engine)
        .with_event_emitter(move |name, payload| {
            events_clone
                .lock()
                .unwrap()
                .push((name.to_string(), payload));
        })
        .build();

    let result = ctx
        .navigate_to_source("bracket.ri", 5, 1, 20, 7)
        .expect("navigate_to_source should succeed");
    assert!(result);

    let recorded = events.lock().unwrap();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].0, "navigate-to-source");
    assert_eq!(recorded[0].1["file"], "bracket.ri");
    assert_eq!(recorded[0].1["line"], 5);
    assert_eq!(recorded[0].1["column"], 1);
    assert_eq!(recorded[0].1["end_line"], 20);
    assert_eq!(recorded[0].1["end_column"], 7);
}

#[test]
fn focus_entity_without_emitter_succeeds() {
    let ctx = make_tauri_context();
    let result = ctx
        .focus_entity("Bracket.width")
        .expect("focus_entity without emitter should succeed");
    assert!(result);
}

#[test]
fn navigate_to_source_without_emitter_succeeds() {
    let ctx = make_tauri_context();
    let result = ctx
        .navigate_to_source("bracket.ri", 5, 1, 20, 7)
        .expect("navigate_to_source without emitter should succeed");
    assert!(result);
}

#[test]
fn navigate_to_source_via_registry_emits_end_position() {
    // Build a context with event emitter over the real bracket session
    let session = make_loaded_session();
    let engine = Arc::new(Mutex::new(session));
    let events: Arc<Mutex<Vec<(String, serde_json::Value)>>> = Arc::new(Mutex::new(Vec::new()));
    let events_clone = events.clone();

    let ctx = TauriToolContext::builder(engine)
        .with_event_emitter(move |name, payload| {
            events_clone
                .lock()
                .unwrap()
                .push((name.to_string(), payload));
        })
        .build();

    // Get expected source location from the real engine
    let loc = ctx
        .get_source_location("Bracket.width")
        .expect("get_source_location should succeed for Bracket.width");
    assert!(
        loc.end_line >= loc.line,
        "end_line ({}) should be >= line ({})",
        loc.end_line,
        loc.line
    );

    // Dispatch through the tool registry (exercises the full tool-handler path)
    let result = mcp_tool_call_impl(
        "reify_navigate_to_source",
        serde_json::json!({"entity_path": "Bracket.width"}),
        &ctx,
    )
    .expect("mcp_tool_call_impl should succeed");
    assert_eq!(result["success"], true);

    // The emitted event payload must carry the full range, not just the start point
    let recorded = events.lock().unwrap();
    assert_eq!(recorded.len(), 1, "exactly one event should be emitted");
    assert_eq!(recorded[0].0, "navigate-to-source");
    assert_eq!(recorded[0].1["line"], loc.line);
    assert_eq!(recorded[0].1["column"], loc.column);
    assert_eq!(
        recorded[0].1["end_line"], loc.end_line,
        "end_line must propagate through tool handler"
    );
    assert_eq!(
        recorded[0].1["end_column"], loc.end_column,
        "end_column must propagate through tool handler"
    );
}

// --- Selection state tests ---

#[test]
fn get_selection_returns_selected_entity_from_arc() {
    let session = make_loaded_session();
    let engine = Arc::new(Mutex::new(session));
    let selection = Arc::new(RwLock::new(SelectionInfo {
        selected_entity: Some("Bracket".to_string()),
        selected_entities: vec![],
        hovered_entity: None,
    }));
    let ctx = TauriToolContext::builder(engine)
        .with_selection(selection)
        .build();
    let result = ctx.get_selection().expect("get_selection should succeed");
    assert_eq!(result.selected_entity, Some("Bracket".to_string()));
    assert_eq!(result.hovered_entity, None);
}

#[test]
fn get_selection_returns_both_selected_and_hovered() {
    let session = make_loaded_session();
    let engine = Arc::new(Mutex::new(session));
    let selection = Arc::new(RwLock::new(SelectionInfo {
        selected_entity: Some("Bracket".to_string()),
        selected_entities: vec![],
        hovered_entity: Some("Bracket.width".to_string()),
    }));
    let ctx = TauriToolContext::builder(engine)
        .with_selection(selection)
        .build();
    let result = ctx.get_selection().expect("get_selection should succeed");
    assert_eq!(result.selected_entity, Some("Bracket".to_string()));
    assert_eq!(result.hovered_entity, Some("Bracket.width".to_string()));
}

#[test]
fn get_selection_reflects_live_arc_updates() {
    let session = make_loaded_session();
    let engine = Arc::new(Mutex::new(session));
    let selection = Arc::new(RwLock::new(SelectionInfo::default()));
    let ctx = TauriToolContext::builder(engine)
        .with_selection(selection.clone())
        .build();

    // Initially empty
    let result = ctx.get_selection().expect("get_selection should succeed");
    assert_eq!(result.selected_entity, None);

    // Update the Arc externally (simulating frontend invoke)
    {
        let mut sel = selection.write().unwrap();
        sel.selected_entity = Some("Bracket.height".to_string());
        sel.hovered_entity = Some("Bracket.thickness".to_string());
    }

    // Subsequent call reflects the update
    let result = ctx.get_selection().expect("get_selection should succeed");
    assert_eq!(result.selected_entity, Some("Bracket.height".to_string()));
    assert_eq!(result.hovered_entity, Some("Bracket.thickness".to_string()));
}

// --- Builder tests ---

#[test]
fn builder_with_no_options_matches_new() {
    let session = make_loaded_session();
    let engine = Arc::new(Mutex::new(session));
    let ctx = TauriToolContext::builder(engine).build();

    // Selection should be empty (matches `new()` behavior)
    let selection = ctx.get_selection().expect("get_selection should succeed");
    assert!(selection.selected_entity.is_none());
    assert!(selection.hovered_entity.is_none());

    // focus_entity should succeed without an emitter (no-op)
    let result = ctx
        .focus_entity("Bracket.width")
        .expect("focus_entity without emitter should succeed");
    assert!(result);
}

#[test]
fn builder_with_selection_matches_new_with_selection() {
    let session = make_loaded_session();
    let engine = Arc::new(Mutex::new(session));
    let selection = Arc::new(RwLock::new(SelectionInfo {
        selected_entity: Some("Bracket".to_string()),
        selected_entities: vec![],
        hovered_entity: None,
    }));
    let ctx = TauriToolContext::builder(engine)
        .with_selection(selection)
        .build();

    let result = ctx.get_selection().expect("get_selection should succeed");
    assert_eq!(result.selected_entity, Some("Bracket".to_string()));
    assert_eq!(result.hovered_entity, None);
}

#[test]
fn builder_with_event_emitter_records_events() {
    let session = make_loaded_session();
    let engine = Arc::new(Mutex::new(session));
    let events: Arc<Mutex<Vec<(String, serde_json::Value)>>> = Arc::new(Mutex::new(Vec::new()));
    let events_clone = events.clone();

    let ctx = TauriToolContext::builder(engine)
        .with_event_emitter(move |name, payload| {
            events_clone
                .lock()
                .unwrap()
                .push((name.to_string(), payload));
        })
        .build();

    let result = ctx
        .focus_entity("Bracket.width")
        .expect("focus_entity should succeed");
    assert!(result);

    let recorded = events.lock().unwrap();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].0, "focus-entity");
}

#[test]
fn builder_with_both_options() {
    let session = make_loaded_session();
    let engine = Arc::new(Mutex::new(session));
    let events: Arc<Mutex<Vec<(String, serde_json::Value)>>> = Arc::new(Mutex::new(Vec::new()));
    let events_clone = events.clone();
    let selection = Arc::new(RwLock::new(SelectionInfo {
        selected_entity: Some("Bracket.height".to_string()),
        selected_entities: vec![],
        hovered_entity: None,
    }));

    let ctx = TauriToolContext::builder(engine)
        .with_event_emitter(move |name, payload| {
            events_clone
                .lock()
                .unwrap()
                .push((name.to_string(), payload));
        })
        .with_selection(selection)
        .build();

    // Verify selection
    let sel = ctx.get_selection().expect("get_selection should succeed");
    assert_eq!(sel.selected_entity, Some("Bracket.height".to_string()));

    // Verify event emitter
    ctx.focus_entity("Bracket.width")
        .expect("focus_entity should succeed");
    let recorded = events.lock().unwrap();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].0, "focus-entity");
}

// --- McpConfig struct tests ---

#[test]
fn mcp_config_struct_stores_fields() {
    use crate::claude_bridge::McpConfig;

    let session = make_loaded_session();
    let engine = Arc::new(Mutex::new(session));
    let selection = Arc::new(RwLock::new(SelectionInfo::default()));
    let sink_called = Arc::new(Mutex::new(false));
    let sink_clone = sink_called.clone();

    let config = McpConfig {
        engine: engine.clone(),
        event_emitter: Arc::new(move |_name: String, _payload: serde_json::Value| {
            *sink_clone.lock().unwrap() = true;
        }),
        selection: selection.clone(),
    };

    // Assert all three fields are accessible and hold the right values
    assert!(Arc::ptr_eq(&config.engine, &engine));
    assert!(Arc::ptr_eq(&config.selection, &selection));
    (config.event_emitter)("test".to_string(), serde_json::json!({}));
    assert!(*sink_called.lock().unwrap());
}

// --- Compile-time trait assertions ---

#[test]
fn tauri_tool_context_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<TauriToolContext>();
}

#[test]
fn tauri_tool_context_implements_reify_tool_context() {
    let ctx = make_tauri_context();
    let _dyn_ctx: Arc<dyn ReifyToolContext> = Arc::new(ctx);
}

// --- Task 827: get_diagnostics wiring tests ---

/// Step-9 (REVIEW FIX — renamed): Accurately named replacement for the previous
/// `get_diagnostics_maps_fields_correctly` test which only asserted an empty vec.
///
/// Bracket source has no warnings so the real implementation returns Ok([]).
/// This test confirms: result is Ok and no DiagnosticInfo entries exist.
#[test]
fn get_diagnostics_clean_source_returns_empty() {
    let ctx = make_tauri_context();
    let diags = ctx
        .get_diagnostics()
        .expect("get_diagnostics should succeed for a clean source");

    // bracket_source() compiles cleanly → no diagnostics expected
    assert!(
        diags.is_empty(),
        "bracket source has no warnings; expected empty DiagnosticInfo vec, got: {:?}",
        diags
    );
}

/// Thin wrapping-path smoke test for [`TauriToolContext::get_diagnostics`].
///
/// `mcp_context.rs:127-133` is a 4-line passthrough — it locks the engine and
/// returns `Ok(session.get_diagnostics())`. The `Diagnostic → DiagnosticInfo`
/// mapping closure (including the `offset_to_line_col_fast` span conversion and the
/// hardcoded `code: None`) lives entirely in `engine.rs`.
/// `engine_get_diagnostics_returns_populated_warning` in `engine_tests.rs` only
/// range-checks span fields (`line >= 1`, `column >= 1`, `end_line >= line`,
/// `end_column >= 1`); it does NOT pin exact coordinates.
///
/// This test therefore focuses on two things the engine test cannot cover: (a) that
/// the wrapping path returns `Result::Ok`, and (b) that every field passes through
/// without a field-swap bug. The pinned exact-coordinate span assertions
/// (line/column/end_line/end_column) are the only ones in the suite that pin literal
/// exact-coordinate values for a real compiler-emitted span.
/// `get_diagnostics_multi_diagnostic_stress_matches_reference` in `engine_tests.rs`
/// pins coordinates against a `byte_offset_to_line_col` reference oracle for synthetic
/// injected spans; this test provides complementary coverage on a real `port_decl` span
/// shape. Together they catch any accidental transposition that a bare `>= 1` range
/// check would miss. If a future change improves diagnostic locality (e.g. narrowing
/// the span to point at just `NonExistentTrait` instead of the whole `port_decl`),
/// update the pinned values to match the new coordinates — do NOT revert to range
/// checks, as that would silently remove the span regression coverage.
/// The `code: None` assertion confirms the `Option<String>` field passes through
/// unchanged (the mapping closure hardcodes `None`; a future code-extraction change
/// that starts populating it will break this assertion and prompt the implementing
/// agent to update both the assertion and this doc comment).
#[test]
fn get_diagnostics_maps_warning_fields_to_diagnostic_info() {
    let source = warn_source_with_unknown_port_type();

    let ctx = make_tauri_context_with_source(source, "test_warn");
    let diags = ctx
        .get_diagnostics()
        .expect("get_diagnostics should return Ok for a source with warnings");

    assert!(
        !diags.is_empty(),
        "expected at least one DiagnosticInfo for unknown port type warning, got empty"
    );

    let first = &diags[0];

    // file_path must match the module name passed to load_from_source
    assert_eq!(
        first.file_path, "test_warn.ri",
        "expected file_path 'test_warn.ri', got '{}'",
        first.file_path
    );

    // severity must be "Warning"
    assert_eq!(
        first.severity, "Warning",
        "expected severity 'Warning', got '{}'",
        first.severity
    );

    // message field passes through completely — spot-check both substrings
    assert!(
        first.message.contains("unknown port type"),
        "expected message to contain 'unknown port type', got: '{}'",
        first.message
    );
    assert!(
        first.message.contains("NonExistentTrait"),
        "expected message to mention 'NonExistentTrait', got: '{}'",
        first.message
    );

    // Pinned exact coordinates for the `port mount : NonExistentTrait` fixture.
    // The port_decl tree-sitter node spans from the `port` keyword (L2:C5) to the
    // exclusive end byte one past the closing `}` of the port body (line 4: 4 spaces
    // + `}`). `offset_to_line_col_fast` computes chars().count() of
    // `source[line_start..end_byte]` and adds 1: that prefix is "    }" = 5 chars,
    // so end_column = 5 + 1 = 6. (The `}` glyph itself sits at column 5; end_column
    // 6 is the exclusive one-past position.) This is a multi-line span, so the former
    // `if end_line == line` same-line guard was always false for this fixture.
    assert_eq!(
        first.line, 2,
        "`port` keyword starts at line 2 of the fixture"
    );
    assert_eq!(
        first.column, 5,
        "`port` keyword starts at column 5 (1-indexed)"
    );
    assert_eq!(
        first.end_line, 4,
        "closing `}}` of port body ends at line 4 of the fixture"
    );
    assert_eq!(
        first.end_column, 6,
        "exclusive end byte past `}}` maps to column 6 (chars().count() + 1)"
    );

    // code field passthrough: hardcoded None in the mapping closure (see doc comment)
    assert!(first.code.is_none(), "expected code to be None");

    // has_location must be true for a spanned warning (non-empty labels → real source span).
    // RED until step-5 adds the field to DiagnosticInfo and sets it in diagnostics_to_info.
    assert!(
        first.has_location,
        "spanned warning (non-empty labels) must set has_location = true"
    );
}

/// Cross-transport parity test (step-7): the MCP-dispatch surface for
/// `reify_get_source_location` must accept both a plain template name
/// ("Bracket") and a full cell ID ("Bracket.width"), returning identical
/// `(line, column, end_line, end_column)` tuples for both inputs.
///
/// This mirrors the CLI-side test `mcp_server_get_source_location_accepts_template_name_and_cell_id`
/// (step-5) and proves that both transports use the same unified semantics.
///
/// The test passes after step-4 (GUI engine refactor) because
/// `EngineSession::get_source_location("Bracket")` now returns `Some` — before
/// step-4 it returned `None` and the trait impl mapped that to a ToolError,
/// which `mcp_tool_call_impl` converts to a JSON error object, causing the
/// assertion to fail.
#[test]
fn get_source_location_through_mcp_dispatch_accepts_template_name_and_cell_id() {
    let ctx = make_tauri_context();

    // (1) plain template name
    let loc_name = mcp_tool_call_impl(
        "reify_get_source_location",
        serde_json::json!({"entity_path": "Bracket"}),
        &ctx,
    )
    .expect(
        "mcp_tool_call_impl('reify_get_source_location', {entity_path:'Bracket'}) must succeed",
    );

    // (2) full cell ID
    let loc_width = mcp_tool_call_impl(
        "reify_get_source_location",
        serde_json::json!({"entity_path": "Bracket.width"}),
        &ctx,
    )
    .expect(
        "mcp_tool_call_impl('reify_get_source_location', {entity_path:'Bracket.width'}) must succeed",
    );

    // Both must resolve to a real location.
    for (loc, label) in [(&loc_name, "Bracket"), (&loc_width, "Bracket.width")] {
        assert_eq!(
            loc["file_path"].as_str().unwrap_or(""),
            "bracket.ri",
            "{label}: file_path must be 'bracket.ri', got: {:?}",
            loc["file_path"]
        );
        let line = loc["line"].as_u64().unwrap_or(0);
        assert!(line >= 1, "{label}: line must be >= 1, got {line}");
        let column = loc["column"].as_u64().unwrap_or(0);
        assert!(column >= 1, "{label}: column must be >= 1, got {column}");
        let end_line = loc["end_line"].as_u64().unwrap_or(0);
        assert!(
            end_line >= line,
            "{label}: end_line ({end_line}) must be >= line ({line})"
        );
    }

    // Template-name proxy must return the same span as the first value cell (width).
    assert_eq!(
        (
            loc_name["line"].as_u64(),
            loc_name["column"].as_u64(),
            loc_name["end_line"].as_u64(),
            loc_name["end_column"].as_u64(),
        ),
        (
            loc_width["line"].as_u64(),
            loc_width["column"].as_u64(),
            loc_width["end_line"].as_u64(),
            loc_width["end_column"].as_u64(),
        ),
        "template-name 'Bracket' must proxy to the first value cell (width) \
         — spans must be identical.\nBracket={loc_name:?}\nBracket.width={loc_width:?}"
    );
}
