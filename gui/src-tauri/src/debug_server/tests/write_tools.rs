//! Task 5097 δ: tests for the five `reify_*` AI write tools.
//!
//! Split out of `debug_server.rs`'s `mod tests` for SIZE alone — that file was
//! 6.9k lines, over half of them tests. The PRODUCTION cluster deliberately
//! stays in `debug_server.rs` (see the co-location note in its header); this
//! move does not touch that argument.
//!
//! A CHILD of `debug_server::tests` rather than a sibling under `src/tests/`,
//! so `use super::*` still reaches both the private production items and the
//! fixtures the parent shares with the #5193 open-funnel tests
//! (`write_and_canonicalize`, `launch_via_load_file`) — nothing had to be
//! widened to make the move.

use super::*;

// ── Task 5097 δ step-5: RED — the ONE write seam every `reify_*` write
// tool routes through (PRD §6.2 pair) ──
//
// `write_on_engine_and_refresh_baseline` (added in step-6) runs an
// arbitrary engine mutation through `run_on_engine` and then refreshes
// `last_state` via `compute_delta`, exactly as the two landed 5035
// wrappers above do for their own fixed mutations. Uniformity is the
// point: with all five write tools on one seam, θ's structural test has a
// single anchor and no exceptions to enumerate.
//
// FAILS TO COMPILE until step-6 adds
// `write_on_engine_and_refresh_baseline`.

/// Fixture for the δ write-tool cluster: a `.ri` whose `width` default is
/// a plain quantity literal (so the INV-GUI-3 write-back can splice it)
/// beside a second declaration that must survive any splice byte for byte.
fn ai_write_source() -> &'static str {
    r#"// task 5097 δ fixture
structure def Part {
    param width: Length = 80mm
    param depth: Length = 40mm

    let body = box(width, width, depth)
}"#
}

/// A tempdir-backed engine LAUNCHED ON an on-disk `part.ri`, plus that
/// file's canonical path. The launch matters: `apply_param_to_source`
/// refuses a session with no canonical `.ri` to write back to, so a
/// `load_from_source` engine cannot exercise this cluster at all.
fn ai_write_engine(dir: &std::path::Path) -> (Arc<Mutex<EngineSession>>, String) {
    let canonical = write_and_canonicalize(dir, "part.ri", ai_write_source());
    let engine = crate::tests::make_test_engine();
    launch_via_load_file(&engine, &canonical);
    (engine, canonical)
}

/// The GuiState the frontend is currently holding — S0, the baseline a
/// later `compute_delta` must diff against.
fn current_gui_state(engine: &Arc<Mutex<EngineSession>>) -> crate::types::GuiState {
    crate::engine_lock::with_engine_lock(engine, |s| s.build_gui_state())
        .and_then(std::convert::identity)
        .expect("build_gui_state must succeed")
}

#[tokio::test]
async fn write_helper_refreshes_the_delta_baseline() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, _canonical) = ai_write_engine(dir.path());

    let s0 = current_gui_state(&engine);
    let last_state: std::sync::Mutex<Option<crate::types::GuiState>> =
        std::sync::Mutex::new(Some(s0.clone()));

    let s1 = write_on_engine_and_refresh_baseline(&engine, &last_state, |s| {
        s.apply_param_to_source_str("Part.width", "120mm")
    })
    .await
    .expect("write_on_engine_and_refresh_baseline must return Ok");

    let width = s1
        .values
        .iter()
        .find(|v| v.cell_id == "Part.width")
        .expect("Part.width must be present in the returned GuiState");
    assert_eq!(
        (width.value.as_str(), width.unit.as_str()),
        ("120", "mm"),
        "the seam must return the POST-mutation GuiState"
    );

    // The bug-#7 stale-baseline guard: the baseline must now be S1, not
    // the S0 it was seeded with. A debug mutation that advances the engine
    // without advancing `last_state` makes the NEXT normal command diff
    // against a state the frontend no longer has.
    assert_eq!(
        *last_state.lock().unwrap(),
        Some(s1.clone()),
        "the seam must refresh last_state to S1 in the same call"
    );
    assert_ne!(
        *last_state.lock().unwrap(),
        Some(s0),
        "the baseline must have MOVED — S0 and S1 differ by the width edit"
    );

    // …and advanced EXACTLY once (§6.2 invariant (a)): re-diffing the same
    // state against the refreshed baseline yields nothing at all.
    let redelta = crate::diff::compute_delta(&last_state, &s1);
    let events: Vec<String> = crate::diff::delta_to_events(&redelta)
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert!(
        events.is_empty(),
        "the baseline must have advanced exactly once — a second diff \
         against S1 must be empty; got {events:?}"
    );
}

#[tokio::test]
async fn write_helper_leaves_the_baseline_untouched_when_the_mutation_fails() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, _canonical) = ai_write_engine(dir.path());

    let s0 = current_gui_state(&engine);
    let last_state: std::sync::Mutex<Option<crate::types::GuiState>> =
        std::sync::Mutex::new(Some(s0.clone()));

    let err = write_on_engine_and_refresh_baseline(&engine, &last_state, |_s| {
        Err("mutation refused".to_string())
    })
    .await
    .expect_err("a failing mutation must propagate as Err");
    assert!(
        err.contains("mutation refused"),
        "the mutation's own error must reach the caller verbatim, got: {err}"
    );

    // A refused mutation left the engine where it was, so advancing the
    // baseline would desync it from the frontend in the OTHER direction.
    assert_eq!(
        *last_state.lock().unwrap(),
        Some(s0),
        "a failed mutation must leave the baseline at S0"
    );
}

// ── Task 5097 δ step-7: RED — `reify_set_parameter`, the INV-GUI-3 AI
// write path (PRD §6.1/§6.3) ──
//
// FAILS TO COMPILE until step-8 adds
// `reify_set_parameter_on_engine_and_refresh_baseline` and
// `reify_write_str_param`.

#[tokio::test]
async fn reify_set_parameter_writes_disk_and_refreshes_the_baseline() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, canonical) = ai_write_engine(dir.path());

    let s0 = current_gui_state(&engine);
    let last_state: std::sync::Mutex<Option<crate::types::GuiState>> =
        std::sync::Mutex::new(Some(s0.clone()));

    let s1 = reify_set_parameter_on_engine_and_refresh_baseline(
        &engine,
        &last_state,
        "Part.width",
        "120mm",
    )
    .await
    .expect("reify_set_parameter_on_engine_and_refresh_baseline must return Ok");

    // INV-GUI-3: the `.ri` on disk is the canonical truth, so the AI edit
    // must have LANDED there — and nowhere else in the file.
    let disk = std::fs::read_to_string(&canonical).expect("part.ri must be readable");
    assert_eq!(
        disk,
        ai_write_source().replace("80mm", "120mm"),
        "only the width default may change on disk — the comment, the depth \
         declaration and the body must survive byte for byte"
    );

    // eval state ≡ source: the returned GuiState already reports the edit.
    let width = s1
        .values
        .iter()
        .find(|v| v.cell_id == "Part.width")
        .expect("Part.width must be present in the returned GuiState");
    assert_eq!((width.value.as_str(), width.unit.as_str()), ("120", "mm"));

    // §6.2 invariant (a): the baseline advanced with the mutation.
    assert_eq!(
        *last_state.lock().unwrap(),
        Some(s1),
        "the tool must route through the shared seam, which refreshes last_state"
    );
}

#[tokio::test]
async fn reify_set_parameter_rejection_mutates_nothing() {
    // PRD §7 B7 atomicity, on the two rejection shapes an AI client will
    // actually hit: a cell that does not exist, and a bare number on a
    // dimensioned cell (the #5757 ladder rung). Neither may leave a trace
    // on disk OR advance the baseline half a step.
    let cases: &[(&str, &str, &str)] = &[
        ("Part.nope", "1mm", "Unknown parameter"),
        ("Part.width", "120", "bare number"),
    ];

    for (cell_id, value, expected) in cases {
        let dir = tempfile::tempdir().unwrap();
        let (engine, canonical) = ai_write_engine(dir.path());

        let s0 = current_gui_state(&engine);
        let last_state: std::sync::Mutex<Option<crate::types::GuiState>> =
            std::sync::Mutex::new(Some(s0.clone()));

        let err = reify_set_parameter_on_engine_and_refresh_baseline(
            &engine,
            &last_state,
            cell_id,
            value,
        )
        .await
        .expect_err(&format!("({cell_id}, {value}) must be REFUSED"));
        assert!(
            err.contains(expected),
            "({cell_id}, {value}) should be refused with {expected:?}, got: {err}"
        );

        assert_eq!(
            std::fs::read_to_string(&canonical).expect("part.ri must be readable"),
            ai_write_source(),
            "a refused write must leave the on-disk bytes IDENTICAL"
        );
        assert_eq!(
            *last_state.lock().unwrap(),
            Some(s0),
            "a refused write must leave the baseline at S0"
        );
    }
}

#[test]
fn reify_write_params_reject_missing_fields() {
    // The param extraction is the AI client's first contact with the tool,
    // so its refusals keep the reify-mcp spelling verbatim
    // (crates/reify-mcp/src/tools/write.rs): a client that learned the
    // message on one surface must not meet a different one here.
    let missing = reify_write_str_param(&json!({}), "cell_id")
        .expect_err("an absent field must be refused");
    assert_eq!(missing, "cell_id is required");

    // A field of the WRONG TYPE is refused identically — `as_str()` on a
    // number is `None`, and inventing a second message for it would tell
    // the client the field is absent when it is merely mistyped.
    let mistyped = reify_write_str_param(&json!({ "value": 120 }), "value")
        .expect_err("a non-string field must be refused");
    assert_eq!(mistyped, "value is required");

    assert_eq!(
        reify_write_str_param(&json!({ "value": "120mm" }), "value"),
        Ok("120mm".to_string())
    );
}

/// The fifth drift surface the cluster's four guards do NOT cover: a
/// handler reading a param name its own `ToolDef` does not advertise.
///
/// Every param name a handler reads is a bare string literal, and no test
/// can drive the handlers themselves (they need a
/// `DebugServerState`/`AppHandle`). So if `handle_reify_export` read
/// `params["path"]` while its schema advertised `output_path`, the
/// schema-parity test (`tool_defs()` only), the envelope test (pure
/// envelope fns only), `reify_write_params_reject_missing_fields` (passes
/// its own field name) and debugParity cases (f)/(g) (names only) would
/// ALL stay green while every AI client got `output_path is required`.
///
/// This closes it from the other side: for each of the five tools, read
/// the property names out of ITS OWN `input_schema` and feed a params
/// object built from exactly those keys through the extractor the handler
/// actually calls. Schema and handler cannot disagree without reddening
/// this (task #5097 δ, review finding).
#[test]
fn reify_write_tool_params_match_their_advertised_schemas() {
    // Each entry adapts one tool's extractor to a common shape — the
    // RETURN values are pinned by the envelope test; what is under test
    // here is purely which KEYS each one reads.
    type Extract = fn(&Value) -> Result<(), String>;
    let tools: Vec<(&str, Extract)> = vec![
        ("reify_set_parameter", |p| {
            reify_set_parameter_params(p).map(|_| ())
        }),
        ("reify_update_source", |p| {
            reify_update_source_params(p).map(|_| ())
        }),
        // The funnel's shared extractor — the one `handle_reify_open_file`
        // and `handle_open_file` both call.
        ("reify_open_file", |p| open_file_path_param(p).map(|_| ())),
        ("reify_save_file", |p| reify_save_file_params(p).map(|_| ())),
        ("reify_export", |p| reify_export_params(p).map(|_| ())),
    ];

    let defs = tool_defs();
    for (name, extract) in tools {
        let def = defs
            .iter()
            .find(|d| d.name == name)
            .unwrap_or_else(|| panic!("{name} must be advertised in tool_defs()"));

        let props = def.input_schema["properties"]
            .as_object()
            .unwrap_or_else(|| panic!("{name}'s schema must declare properties"));
        assert!(
            !props.is_empty(),
            "{name}'s schema must declare at least one property"
        );

        // (a) A call that supplies EXACTLY what the schema advertises must
        // be accepted. This is the direction that catches a handler
        // reading a name the schema does not carry.
        let full: serde_json::Map<String, Value> = props
            .keys()
            .map(|k| {
                // Every advertised property on these five tools is a
                // string; a non-string one would need its own sample
                // value here, so assert the assumption rather than
                // silently feeding a string to an integer param.
                assert_eq!(
                    def.input_schema["properties"][k]["type"].as_str(),
                    Some("string"),
                    "{name}.{k} is not a string param — this test's sample \
                     value needs widening"
                );
                (k.clone(), json!("sample"))
            })
            .collect();
        extract(&Value::Object(full)).unwrap_or_else(|e| {
            panic!(
                "{name}: the handler refused a params object built from its \
                 OWN advertised property names — schema and handler have \
                 drifted: {e}"
            )
        });

        // (b) A call that supplies exactly the schema's `required` list —
        // and nothing more — must ALSO be accepted, or the handler is
        // demanding a field it advertises as optional. `reify_save_file`
        // has no `required` at all, so this feeds it `{}`: the "save the
        // ACTIVE file" default, which must not be refused at the params
        // boundary.
        let required: Vec<&str> = def.input_schema["required"]
            .as_array()
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let minimal: serde_json::Map<String, Value> = required
            .iter()
            .map(|k| ((*k).to_string(), json!("sample")))
            .collect();
        extract(&Value::Object(minimal)).unwrap_or_else(|e| {
            panic!(
                "{name}: the handler refused a params object carrying every \
                 REQUIRED field ({required:?}) — it is demanding a field its \
                 schema advertises as optional: {e}"
            )
        });

        // (c) Every name in `required` must actually BE a property, or the
        // schema is self-contradictory and (b) proves nothing.
        for k in &required {
            assert!(
                props.contains_key(*k),
                "{name}: `required` names {k}, which is not among its properties"
            );
        }
    }
}

/// The push-reply half of the frontend contract: `query_frontend` resolves
/// `Ok(Value)` for the bridge's in-band `{error}` envelope just as readily
/// as for a real result, so every δ write tool inspects it via
/// [`frontend_ok`] before answering. Pinned over literals here; the same
/// discrimination is pinned across a real `DebugTransport` round-trip by
/// `frontend_error_envelope_is_refused_after_the_transport`.
#[test]
fn frontend_ok_refuses_the_bridge_error_envelope() {
    // The shape `bridge.ts` actually produces for an unregistered command
    // and for any handler that throws.
    let refused = frontend_ok(
        json!({ "error": "unknown command: apply_gui_state" }),
        "apply_gui_state",
    )
    .expect_err("an {error} reply must not be read as a landed push");
    assert_eq!(
        refused,
        "apply_gui_state push refused by the frontend: unknown command: apply_gui_state"
    );

    // A NON-STRING error is refused too. Here "wrong type" and "absent"
    // end in opposite outcomes, so folding them (as the required-param
    // helper deliberately does) would restore the silent-success hole.
    let mistyped = frontend_ok(json!({ "error": { "code": 7 } }), "apply_gui_state")
        .expect_err("a non-string error must still be refused");
    assert!(
        mistyped.starts_with("apply_gui_state push refused by the frontend: "),
        "unexpected refusal: {mistyped}"
    );

    // A real reply passes through VERBATIM — the debug-native `open_file`
    // twin hands this object straight to the visual-regression harness.
    let ok = json!({ "ok": true, "path": "/tmp/part.ri" });
    assert_eq!(
        frontend_ok(ok.clone(), "open_file"),
        Ok(ok),
        "a successful reply must survive the check unchanged"
    );

    // An explicit `null` error, an absent one, and a non-object reply all
    // mean "no error" — the frontend has no other way to spell it.
    assert_eq!(
        frontend_ok(json!({ "error": Value::Null }), "apply_gui_state"),
        Ok(json!({ "error": Value::Null }))
    );
    assert_eq!(frontend_ok(json!({}), "apply_gui_state"), Ok(json!({})));
    assert_eq!(frontend_ok(json!(true), "apply_gui_state"), Ok(json!(true)));
}

// ── Task 5097 δ step-11: RED — `reify_update_source`, the in-memory
// whole-buffer AI edit (PRD §6.3) ──
//
// FAILS TO COMPILE until step-12 adds
// `reify_update_source_on_engine_and_refresh_baseline`.

#[tokio::test]
async fn reify_update_source_recompiles_and_refreshes_the_baseline() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, canonical) = ai_write_engine(dir.path());

    let s0 = current_gui_state(&engine);
    let last_state: std::sync::Mutex<Option<crate::types::GuiState>> =
        std::sync::Mutex::new(Some(s0.clone()));

    let edited = ai_write_source().replace("param depth: Length = 40mm", "param depth: Length = 65mm");
    let s1 = reify_update_source_on_engine_and_refresh_baseline(
        &engine,
        &last_state,
        &canonical,
        &edited,
    )
    .await
    .expect("reify_update_source_on_engine_and_refresh_baseline must return Ok");

    let depth = s1
        .values
        .iter()
        .find(|v| v.cell_id == "Part.depth")
        .expect("Part.depth must be present in the returned GuiState");
    assert_eq!(
        (depth.value.as_str(), depth.unit.as_str()),
        ("65", "mm"),
        "the recompiled GuiState must reflect the edited source"
    );

    assert_eq!(
        *last_state.lock().unwrap(),
        Some(s1),
        "the tool must route through the shared seam, which refreshes last_state"
    );

    // §6.3 routes this tool through the IN-MEMORY `update_source`, so it
    // writes NO disk — that is `reify_set_parameter`'s job, and durable
    // structural edits are §11-out-of-scope (Claude uses its own
    // Write/Edit tools plus the FS-watcher for those). Pinned here rather
    // than left implicit, because a future "helpful" disk write would
    // silently start clobbering the user's unsaved editor buffer.
    assert_eq!(
        std::fs::read_to_string(&canonical).expect("part.ri must be readable"),
        ai_write_source(),
        "reify_update_source must not write disk"
    );
}

#[tokio::test]
async fn reify_update_source_compile_failure_leaves_the_baseline_stale_and_reports_the_error() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, canonical) = ai_write_engine(dir.path());

    let s0 = current_gui_state(&engine);
    let last_state: std::sync::Mutex<Option<crate::types::GuiState>> =
        std::sync::Mutex::new(Some(s0.clone()));

    let err = reify_update_source_on_engine_and_refresh_baseline(
        &engine,
        &last_state,
        &canonical,
        "structure def Part { param width: Length = ",
    )
    .await
    .expect_err("source that does not compile must be REFUSED");
    assert!(
        !err.is_empty(),
        "the compile rejection must reach the AI client with a message"
    );

    // No HALF-advanced baseline: `update_source` leaves the session
    // completely unchanged on a compile failure, so the frontend still
    // holds S0 and the next normal command must diff against S0.
    assert_eq!(
        *last_state.lock().unwrap(),
        Some(s0),
        "a rejected recompile must leave the baseline at S0"
    );
}

// ── Task 5097 δ: `reify_update_source` must REFUSE a non-active
// `file_path` rather than silently redirect the write (review finding) ──

#[tokio::test]
async fn reify_update_source_refuses_a_non_active_file_path() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, canonical) = ai_write_engine(dir.path());

    let s0 = current_gui_state(&engine);
    let last_state: std::sync::Mutex<Option<crate::types::GuiState>> =
        std::sync::Mutex::new(Some(s0.clone()));

    // A sibling module in the SAME project — the shape a multi-file
    // caller actually produces. `update_source` would ignore this path
    // and overwrite part.ri's buffer with this text while answering
    // `success: true`; a later `reify_save_file` would then write it to
    // part.ri ON DISK.
    let sibling = write_and_canonicalize(dir.path(), "lib.ri", ai_write_source());

    let err = reify_update_source_on_engine_and_refresh_baseline(
        &engine,
        &last_state,
        &sibling,
        "structure def Other { param q: Length = 1mm }",
    )
    .await
    .expect_err("a file_path that is not the active file must be REFUSED");
    assert!(
        err.contains("reify_update_source can only update the active file")
            && err.contains(&canonical),
        "the refusal must name the active file so the client can correct \
         itself; got {err:?}"
    );

    // Nothing moved: not the baseline, not the session, not the disk.
    assert_eq!(
        *last_state.lock().unwrap(),
        Some(s0.clone()),
        "a refused call must leave the baseline at S0"
    );
    // The session still holds part.ri's buffer, not the sibling's text.
    // Compared field-wise rather than whole-`GuiState`: rebuilding state
    // mints fresh `GeometryHandleId`s, so the tessellation-diagnostic
    // message carries a monotonic counter that differs on every build and
    // says nothing about whether the session moved.
    let after = current_gui_state(&engine);
    assert_eq!(
        after.files, s0.files,
        "a refused call must not replace the active buffer's content"
    );
    assert_eq!(
        after.values, s0.values,
        "a refused call must not recompile the session"
    );
    assert_eq!(
        std::fs::read_to_string(&canonical).expect("part.ri must be readable"),
        ai_write_source(),
        "a refused call must not touch the active file on disk"
    );
}

#[test]
fn update_source_target_matches_active_accepts_only_the_active_file() {
    let active = std::path::Path::new("/proj/main.ri");

    // The active path verbatim.
    assert!(update_source_target_matches_active(
        Some(active),
        "/proj/main.ri"
    ));
    // The stem-only module key `get_diagnostics` stamps — the spelling an
    // AI client is most likely to echo back.
    assert!(update_source_target_matches_active(Some(active), "main.ri"));

    // A DIFFERENT file, in either spelling, is refused — this is the whole
    // point of the guard.
    assert!(!update_source_target_matches_active(
        Some(active),
        "/proj/lib.ri"
    ));
    assert!(!update_source_target_matches_active(Some(active), "lib.ri"));
    // A same-stem file in another project is NOT the active file.
    assert!(!update_source_target_matches_active(
        Some(active),
        "/elsewhere/main.ri"
    ));

    // No prior `load_file`: `update_source` honours the caller's path in
    // that flow, so there is no redirect to guard against.
    assert!(update_source_target_matches_active(None, "/anything.ri"));
}

#[test]
fn update_source_target_matches_active_sees_through_on_disk_spellings() {
    // The `canonicalize` arm: a relative spelling of the active file
    // resolves to it, so it must be ACCEPTED rather than refused for
    // failing the string comparison.
    let dir = tempfile::tempdir().unwrap();
    let active = write_and_canonicalize(dir.path(), "part.ri", ai_write_source());
    let indirect = format!("{}/./sub/../part.ri", dir.path().display());
    std::fs::create_dir_all(dir.path().join("sub")).unwrap();

    assert!(
        update_source_target_matches_active(Some(std::path::Path::new(&active)), &indirect),
        "a spelling that canonicalizes to the active file must be accepted"
    );

    // A path that does not exist cannot be proven equal, so it stays a
    // refusal — the fallback never accepts on ambiguity.
    assert!(!update_source_target_matches_active(
        Some(std::path::Path::new(&active)),
        &format!("{}/absent-elsewhere.txt", dir.path().display()),
    ));
}

// ── Task 5097 δ step-23: RED — `reify_update_source`'s frontend push must
// carry the SESSION'S canonical path, never the caller's raw spelling
// (review finding, correctness) ──
//
// `handle_reify_update_source` pushes
// `write_tool_frontend_payload(&gs, Some((&file_path, &content)))` with
// `file_path` the UNMODIFIED `reify_write_str_param(&params, "file_path")`,
// while the sibling funnel `open_path_into_engine` runs
// `crate::path_key::canonicalize_debug_open_path(raw_path)` BEFORE building
// its `open_file` push for exactly this reason ("fixes bug #3892:
// duplicate tabs via debug bridge").
//
// The two interact badly because `update_source_target_matches_active`
// DELIBERATELY accepts non-canonical spellings — the stem-only module key
// `"part.ri"`, and any relative/`..`/symlink spelling that `canonicalize`s
// to the active file. On the frontend `editorStore.openFile` keys tabs by
// `canonicalizeKey(file.path)`, and `canonicalizeKey` returns any
// NON-absolute path unchanged (`gui/src/utils/pathUtils.ts` — `if
// (!p.startsWith('/')) return p;`), so an accepted non-canonical spelling
// opens a SECOND tab under a different key and makes it active, while the
// real tab keyed by the absolute path keeps the STALE text — precisely the
// silent editor desync the `file` member was added to close (bug #3893).
//
// Tested against the extractable helper rather than the handler:
// `DebugServerState` is an Arc-of-Mutex bundle this module itself records
// as impractical to build in `mod tests`, which is why every test in this
// cluster drives the `*_on_engine_and_refresh_baseline` cores instead.
//
// FAILS TO COMPILE until step-24 adds `resolve_update_source_push_path`.

#[test]
fn resolve_update_source_push_path_uses_the_sessions_canonical_path() {
    // The named review case: ALL THREE spellings the guard accepts must
    // land on the SAME wire key — the session's own canonical path, which
    // is byte-for-byte the key `open_path_into_engine` already pushed as
    // `open_file` (it loads the engine from an ALREADY-canonicalized
    // `path`), and therefore the key `editorStore` already holds the tab
    // under.
    let dir = tempfile::tempdir().unwrap();
    let active = write_and_canonicalize(dir.path(), "part.ri", ai_write_source());
    let active_path = std::path::Path::new(&active);

    // A relative/dot-segment spelling of the same file — accepted by
    // `update_source_target_matches_active`'s `canonicalize` arm.
    std::fs::create_dir_all(dir.path().join("sub")).unwrap();
    let indirect = format!("{}/./sub/../part.ri", dir.path().display());

    for requested in ["part.ri", indirect.as_str(), active.as_str()] {
        assert_eq!(
            resolve_update_source_push_path(Some(active_path), requested),
            active,
            "every spelling the guard accepts must push the SESSION's \
             canonical path, not the caller's; {requested:?} did not"
        );
    }
}

#[test]
fn resolve_update_source_push_path_canonicalizes_when_no_file_is_loaded() {
    // The `load_from_source` arm: the guard accepts ANY path there (see
    // `update_source_target_matches_active`'s `None` case) and there is no
    // session path to fall back on — so this arm must canonicalize the
    // caller's spelling itself. Otherwise it is the one remaining way to
    // put a relative, tab-forking key on the wire.
    //
    // `Cargo.toml` is resolved relative to the test process CWD, which
    // cargo fixes at the package root — a real on-disk relative spelling
    // without mutating global CWD state (which would race sibling tests).
    let canonical = std::fs::canonicalize("Cargo.toml")
        .expect("the package root must contain Cargo.toml")
        .to_string_lossy()
        .into_owned();

    let pushed = resolve_update_source_push_path(None, "Cargo.toml");
    assert!(
        pushed.starts_with('/'),
        "a relative spelling must be resolved to an ABSOLUTE key — that is \
         exactly what the frontend's `canonicalizeKey` cannot do for \
         itself; got {pushed:?}"
    );
    assert_eq!(
        pushed, canonical,
        "the no-session arm must push the on-disk canonical form"
    );

    // The branch that `Cargo.toml` cannot reach: a `load_from_source`
    // session routinely names a file that does NOT exist on disk — that is
    // what distinguishes the flow — and `canonicalize_debug_open_path`
    // answers such a path with the caller's spelling UNCHANGED. Without
    // the CWD join this arm hands the frontend `"no-such-dir/part.ri"`,
    // which `canonicalizeKey` passes through verbatim: a second tab under
    // a key the real one never had (#3892/#3893).
    let absent = resolve_update_source_push_path(None, "no-such-dir/part.ri");
    assert!(
        absent.starts_with('/'),
        "a NON-EXISTENT relative spelling must still be absolutised — \
         canonicalize cannot resolve it, so the CWD join is the only \
         thing standing between it and a tab-forking key; got {absent:?}"
    );
    assert!(
        absent.ends_with("no-such-dir/part.ri"),
        "absolutising must preserve the caller's spelling, not rewrite it; \
         got {absent:?}"
    );

    // An absolute spelling that does not exist is already a usable key and
    // must survive byte-for-byte — the CWD join must never prefix it.
    let absolute_absent = resolve_update_source_push_path(None, "/no-such-dir/part.ri");
    assert_eq!(
        absolute_absent, "/no-such-dir/part.ri",
        "an already-absolute path must pass through unchanged"
    );
}

#[tokio::test]
async fn reify_update_source_pushes_the_canonical_path_for_a_stem_only_key() {
    // End-to-end over a REAL session: the stem-only key `"part.ri"` is the
    // spelling `get_diagnostics` stamps and therefore the one an AI client
    // is most likely to echo back. The guard accepts it (by design), so
    // the push is the only thing standing between it and a forked tab.
    let dir = tempfile::tempdir().unwrap();
    let (engine, canonical) = ai_write_engine(dir.path());

    let s0 = current_gui_state(&engine);
    let last_state: std::sync::Mutex<Option<crate::types::GuiState>> =
        std::sync::Mutex::new(Some(s0));

    let edited =
        ai_write_source().replace("param depth: Length = 40mm", "param depth: Length = 55mm");
    reify_update_source_on_engine_and_refresh_baseline(
        &engine,
        &last_state,
        "part.ri",
        &edited,
    )
    .await
    .expect("the stem-only module key names the active file and must be accepted");

    // What the handler would put on the wire for this call.
    let active = crate::engine_lock::with_engine_lock(&engine, |s| {
        Ok(s.canonical_file_path().map(|p| p.to_path_buf()))
    })
    .and_then(std::convert::identity)
    .expect("the launched session must expose a canonical path");
    let pushed = resolve_update_source_push_path(active.as_deref(), "part.ri");

    // Absoluteness is the load-bearing property: it is precisely what
    // `canonicalizeKey` needs in order to hit the tab
    // `open_path_into_engine` already opened.
    assert!(
        pushed.starts_with('/'),
        "the pushed key must be ABSOLUTE, got {pushed:?}"
    );
    assert_eq!(
        pushed, canonical,
        "the pushed key must be the session's canonical path"
    );
}

// ── Task 5097 δ step-18: RED — `reify_update_source`'s diagnostics filter
// must not be vacuous (review finding) ──
//
// `EngineSession::get_diagnostics` (engine.rs:3294) stamps EVERY
// `DiagnosticInfo.file_path` from `resolve_source()` (engine.rs:3259-3265),
// which returns the SOURCE_MAP KEY `module_key(module_name)` = `"<stem>.ri"`
// (engine.rs:764) — never a filesystem path. The landed
// `engine_get_diagnostics_returns_populated_warning` (engine_tests.rs:3616)
// already asserts exactly that (`first.file_path == "test_warn.ri"`), and
// commands.rs:539 documents the same stem-only-key fact.
//
// `handle_reify_update_source` filters `d.file_path == file_path` against
// the CALLER's spelling — which its ToolDef documents as a real path, and
// which every test drives as an absolute `&canonical`. So the filter drops
// EVERYTHING and the tool reports `diagnostics_count: 0` with an empty
// array while advertising "filtered to the named file". Because
// `update_source` returns `Err` on hard compile errors, what is silently
// swallowed is exactly the warning/info stream an AI client edits against.
//
// FAILS TO COMPILE until step-19 adds `filter_diagnostics_for_file`.

#[tokio::test]
async fn reify_update_source_diagnostics_survive_an_absolute_path_filter() {
    use reify_test_support::warn_source_with_unknown_port_type;

    let dir = tempfile::tempdir().unwrap();
    let warn_canonical =
        write_and_canonicalize(dir.path(), "warn.ri", warn_source_with_unknown_port_type());

    let engine = crate::tests::make_test_engine();
    // Launched ON the file, so `self.file_path == Some(warn.ri)` and the
    // stamped source key is `"warn.ri"` — the precondition the real tool
    // always runs under.
    launch_via_load_file(&engine, &warn_canonical);

    let s0 = current_gui_state(&engine);
    let last_state: std::sync::Mutex<Option<crate::types::GuiState>> =
        std::sync::Mutex::new(Some(s0));

    reify_update_source_on_engine_and_refresh_baseline(
        &engine,
        &last_state,
        &warn_canonical,
        warn_source_with_unknown_port_type(),
    )
    .await
    .expect("the warning fixture compiles successfully — this must return Ok");

    let diags = crate::engine_lock::with_engine_lock(&engine, |s| s.get_diagnostics())
        .expect("with_engine_lock must not fail");

    // Half 1 — WHY a naive `==` is vacuous: the engine stamps the stem-only
    // module key, NOT the caller's path. Pinned here so a future refactor
    // that changes the stamping breaks this test loudly instead of quietly
    // making the filter below trivially true.
    assert!(
        !diags.is_empty(),
        "warn.ri must produce at least one diagnostic"
    );
    assert!(
        diags.iter().all(|d| d.file_path == "warn.ri"),
        "the engine must stamp the stem-only module key `warn.ri`, got: {:?}",
        diags.iter().map(|d| &d.file_path).collect::<Vec<_>>()
    );
    assert_ne!(
        warn_canonical.as_str(),
        "warn.ri",
        "the caller's spelling must be an ABSOLUTE path, or this test proves nothing"
    );

    // Half 2 — the exact expression the handler will evaluate must SURVIVE
    // the caller's absolute-path spelling.
    let kept = filter_diagnostics_for_file(diags, &warn_canonical);
    assert!(
        kept.iter().any(|d| d.message.contains("unknown port type")),
        "the unknown-port-type warning must survive the absolute-path filter, got: {:?}",
        kept.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// The filter's DISCRIMINATION, as a pure test over hand-built
/// `DiagnosticInfo` literals — no engine, no tempdir, no I/O. The KEPT rows
/// cover every spelling a caller can legitimately supply (the debug-server
/// absolute path, the bare module key a reify-mcp caller may pass, and a
/// `./`-relative path); the DROPPED rows are what prove the predicate is a
/// real discriminator and not a degenerate keep-everything.
///
/// FAILS TO COMPILE until step-19 adds `filter_diagnostics_for_file`.
#[test]
fn filter_diagnostics_for_file_matches_either_spelling() {
    fn stamped(key: &str) -> reify_core::DiagnosticInfo {
        reify_core::DiagnosticInfo {
            file_path: key.to_string(),
            line: 2,
            column: 5,
            end_line: 2,
            end_column: 9,
            severity: "Warning".to_string(),
            message: "unknown port type 'NonExistentTrait'".to_string(),
            code: None,
            has_location: true,
        }
    }

    for requested in ["/tmp/x/part.ri", "part.ri", "./part.ri"] {
        let kept = filter_diagnostics_for_file(vec![stamped("part.ri")], requested);
        assert_eq!(
            kept.len(),
            1,
            "a diagnostic stamped `part.ri` must be KEPT for requested `{requested}`"
        );
    }

    for requested in ["/tmp/x/other.ri", ""] {
        let kept = filter_diagnostics_for_file(vec![stamped("part.ri")], requested);
        assert!(
            kept.is_empty(),
            "a diagnostic stamped `part.ri` must be DROPPED for requested `{requested}`, got: {:?}",
            kept.iter().map(|d| &d.file_path).collect::<Vec<_>>()
        );
    }
}

// ── Task 5097 δ step-13: RED — the two PURE-I/O write tools
// (`reify_save_file`, `reify_export`) ──
//
// Neither commits new engine state, but both still route through the
// shared seam so §6.2 invariant (a) holds UNIFORMLY across all five tools
// and θ's structural anchor has no exceptions to enumerate.
//
// FAILS TO COMPILE until step-14 adds the two seams and
// `crate::commands::parse_export_format`.

#[tokio::test]
async fn reify_save_file_writes_the_session_buffer() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, canonical) = ai_write_engine(dir.path());

    let s0 = current_gui_state(&engine);
    let last_state: std::sync::Mutex<Option<crate::types::GuiState>> =
        std::sync::Mutex::new(Some(s0.clone()));

    // (i) No target: saves the ACTIVE file — the canonical `.ri` this
    // session was launched from.
    std::fs::write(&canonical, "// clobbered, must be restored by the save")
        .expect("the pre-save scribble must be writable");
    let s1 = reify_save_file_on_engine_and_refresh_baseline(&engine, &last_state, None)
        .await
        .expect("reify_save_file with no target must save the active file");
    assert_eq!(
        std::fs::read_to_string(&canonical).expect("part.ri must be readable"),
        ai_write_source(),
        "the save must write the session's in-memory source to its canonical path"
    );

    // Uniform §6.2 routing: the baseline IS refreshed even though pure I/O
    // changed no engine state.
    //
    // Asserted after EACH call rather than once at the end, because a
    // rebuild is not bit-identical under `MockGeometryKernel`: its
    // per-tessellation `GeometryHandleId(N)` counter advances, so call
    // (ii)'s `tessellation_diagnostics` differ from call (i)'s by that
    // artifact alone (the same mock-only artifact
    // `apply_param_to_source_reload_of_the_written_file_is_an_empty_delta`
    // accounts for). Comparing the FINAL baseline against `s1` would fail
    // on that artifact and say nothing about the refresh.
    assert_eq!(
        *last_state.lock().unwrap(),
        Some(s1),
        "the seam must refresh last_state even for a pure-I/O tool"
    );

    // (ii) An explicit target writes THERE and leaves the original alone.
    let other_dir = tempfile::tempdir().unwrap();
    let other = other_dir.path().join("copy.ri").to_string_lossy().into_owned();
    let s2 = reify_save_file_on_engine_and_refresh_baseline(
        &engine,
        &last_state,
        Some(other.clone()),
    )
    .await
    .expect("reify_save_file with an explicit target must succeed");
    assert_eq!(
        std::fs::read_to_string(&other).expect("copy.ri must be readable"),
        ai_write_source(),
        "an explicit target must receive the session's in-memory source"
    );
    assert_eq!(
        std::fs::read_to_string(&canonical).expect("part.ri must be readable"),
        ai_write_source(),
        "an explicit target must leave the original file untouched"
    );
    assert_eq!(
        *last_state.lock().unwrap(),
        Some(s2.clone()),
        "the second save must refresh the baseline too"
    );

    // …and the refresh is FREE: nothing changed, so the delta is empty.
    let redelta = crate::diff::compute_delta(&last_state, &s2);
    let events: Vec<String> = crate::diff::delta_to_events(&redelta)
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert!(
        events.is_empty(),
        "a pure-I/O tool must produce an EMPTY delta; got {events:?}"
    );
}

#[tokio::test]
async fn reify_save_file_errors_when_no_source_is_loaded() {
    // A session that has never compiled anything has no buffer to save.
    // The message is the one the reify-mcp surface already uses for this
    // condition, so a client cannot meet two spellings of it.
    let engine = Arc::new(Mutex::new(EngineSession::new(
        Box::new(reify_constraints::SimpleConstraintChecker),
        Some(Box::new(reify_test_support::MockGeometryKernel::new())),
    )));
    let last_state: std::sync::Mutex<Option<crate::types::GuiState>> =
        std::sync::Mutex::new(None);

    let err = reify_save_file_on_engine_and_refresh_baseline(&engine, &last_state, None)
        .await
        .expect_err("a session with nothing loaded must refuse to save");
    assert!(
        err.contains("No source loaded"),
        "expected the reify-mcp 'No source loaded' refusal, got: {err}"
    );
}

// ── Task 5097 δ amendment (review finding): the two ways
// `reify_save_file` could pick a target it should never have picked ──

/// A `load_from_source` session HAS a buffer but no canonical path, so the
/// "save the ACTIVE file" default has nothing to resolve to.
/// `GuiState.files[0].path` is the stem-only `source_map` key
/// (`"bracket.ri"`), so falling back to it would write a stray RELATIVE
/// file into whatever CWD the GUI process happens to have — and report
/// `success: true`. Refusing is the only defensible answer.
#[tokio::test]
async fn reify_save_file_refuses_when_the_session_has_no_canonical_path() {
    // `make_test_engine` is exactly this shape: `EngineSession::new` +
    // `load_from_source(bracket_source(), "bracket")`, never `load_file`.
    let engine = crate::tests::make_test_engine();
    let s0 = current_gui_state(&engine);
    let last_state: std::sync::Mutex<Option<crate::types::GuiState>> =
        std::sync::Mutex::new(Some(s0.clone()));

    // Precondition: there IS a buffer (so this is not the "No source
    // loaded" arm) and it is keyed by a stem-only, non-absolute path.
    let stem_key = s0
        .files
        .first()
        .expect("a load_from_source session must still expose files[0]")
        .path
        .clone();
    assert!(
        !std::path::Path::new(&stem_key).is_absolute(),
        "the fixture's premise is that files[0].path is a stem-only \
         source_map key, not a filesystem path; got {stem_key}"
    );

    let err = reify_save_file_on_engine_and_refresh_baseline(&engine, &last_state, None)
        .await
        .expect_err("a session with no canonical path must refuse the default target");
    assert!(
        err.contains("no active file to save"),
        "expected the no-canonical-path refusal, got: {err}"
    );

    // The refusal is ATOMIC: no stray file, no baseline advance.
    assert!(
        !std::path::Path::new(&stem_key).exists(),
        "the refusal must not have written {stem_key} into the process CWD"
    );
    assert_eq!(
        *last_state.lock().unwrap(),
        Some(s0),
        "a refused save must leave the baseline where it was"
    );

    // …and an EXPLICIT target still works on the very same session, so the
    // refusal is about the missing target, not about the session kind.
    let dir = tempfile::tempdir().unwrap();
    let explicit = dir.path().join("saved.ri").to_string_lossy().into_owned();
    reify_save_file_on_engine_and_refresh_baseline(
        &engine,
        &last_state,
        Some(explicit.clone()),
    )
    .await
    .expect("an explicit target must still save a load_from_source session");
    assert!(
        std::fs::read_to_string(&explicit)
            .expect("the explicit target must be readable")
            .contains("structure def"),
        "the explicit target must receive the session's in-memory source"
    );
}

/// `reify_save_file`'s `file_path` is the ONE optional write-tool param, so
/// "absent" is a live semantic (= the active file) and a WRONG-TYPED field
/// must not be folded into it. Folding would turn an intended save-as into
/// an overwrite of the user's canonical `.ri` — which is exactly what the
/// bare `params["file_path"].as_str().map(...)` this replaced would do.
#[test]
fn reify_write_optional_str_param_separates_absent_from_mistyped() {
    // Absent — and its JSON synonym, an explicit null — mean "no target".
    assert_eq!(
        reify_write_optional_str_param(&json!({}), "file_path"),
        Ok(None),
        "a missing field must read as ABSENT"
    );
    assert_eq!(
        reify_write_optional_str_param(&json!({ "file_path": null }), "file_path"),
        Ok(None),
        "an explicit null must read as ABSENT — JSON has no other spelling"
    );

    // A value of the wrong type is a caller ERROR, never a silent default.
    for mistyped in [
        json!({ "file_path": 120 }),
        json!({ "file_path": true }),
        json!({ "file_path": ["/tmp/a.ri"] }),
        json!({ "file_path": { "path": "/tmp/a.ri" } }),
    ] {
        let err = reify_write_optional_str_param(&mistyped, "file_path")
            .expect_err("a wrong-typed file_path must be REFUSED, not read as absent");
        assert_eq!(
            err, "file_path must be a string",
            "the refusal must name the field and the expected type"
        );
    }

    // The happy path is unchanged.
    assert_eq!(
        reify_write_optional_str_param(&json!({ "file_path": "/tmp/a.ri" }), "file_path"),
        Ok(Some("/tmp/a.ri".to_string())),
    );
}

// ── Task 5097 δ step-25: RED — a failed `reify_update_source` followed by
// `reify_save_file` OVERWRITES the user's canonical `.ri` with source that
// does not compile (review finding, data-loss) ──
//
// `EngineSession::update_source` calls `record_compile_failure(diags,
// content, module_name)` on BOTH failure arms with the REJECTED `content`;
// `record_compile_failure` classifies it `CompileFailureKind::LiveEdit`
// whenever `self.core.compiled().is_some()`, which it is after a
// successful launch; `build_files_with_live_edit` then SPLICES that
// rejected source into the matching `files[]` entry to hold its
// one-snapshot invariant; and
// `reify_save_file_on_engine_and_refresh_baseline` writes exactly
// `gs.files.first().content` to the ACTIVE canonical path.
//
// Correct for a READ-ONLY `engine_state` snapshot — the editor must be
// able to see the text it just failed to compile. Catastrophic for a
// WRITE-BACK. `reify_update_source_compile_failure_leaves_the_baseline_stale…`
// only pins that `last_state` did not move; nothing covers the SAVE that
// follows.
//
// FAILS until step-26 adds `EngineSession::holds_rejected_source` and the
// interlock in `reify_save_file_on_engine_and_refresh_baseline`.

/// Source that does not parse — the shape a mid-edit AI buffer actually
/// has. Shared by the three save-interlock tests so they all provoke the
/// SAME recorded `CompileFailure`.
fn ai_write_rejected_source() -> &'static str {
    "structure def Part { param width: Length = "
}

#[tokio::test]
async fn reify_save_file_refuses_after_a_failed_update_source() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, canonical) = ai_write_engine(dir.path());

    let s0 = current_gui_state(&engine);
    let last_state: std::sync::Mutex<Option<crate::types::GuiState>> =
        std::sync::Mutex::new(Some(s0.clone()));

    let on_disk_before = std::fs::read_to_string(&canonical).expect("part.ri must be readable");

    reify_update_source_on_engine_and_refresh_baseline(
        &engine,
        &last_state,
        &canonical,
        ai_write_rejected_source(),
    )
    .await
    .expect_err("source that does not compile must be REFUSED");

    // The session is now holding a REJECTED buffer that `build_gui_state`
    // deliberately surfaces in `files[0].content`. Persisting it would
    // destroy the user's canonical document.
    reify_save_file_on_engine_and_refresh_baseline(&engine, &last_state, None)
        .await
        .expect_err("saving a buffer the engine itself rejected must be REFUSED");

    assert_eq!(
        std::fs::read_to_string(&canonical).expect("part.ri must be readable"),
        on_disk_before,
        "the refused save must leave the canonical .ri BYTE-IDENTICAL"
    );
    assert_eq!(
        *last_state.lock().unwrap(),
        Some(s0),
        "the refusal must be atomic — no half-advanced baseline either"
    );
}

#[tokio::test]
async fn reify_save_file_refuses_a_rejected_buffer_for_an_explicit_target_too() {
    // Writing non-compiling text to a NEW path while answering
    // `success: true` is the same lie, just less destructive — so the
    // "save as" arm is interlocked identically.
    let dir = tempfile::tempdir().unwrap();
    let (engine, canonical) = ai_write_engine(dir.path());

    let s0 = current_gui_state(&engine);
    let last_state: std::sync::Mutex<Option<crate::types::GuiState>> =
        std::sync::Mutex::new(Some(s0));

    reify_update_source_on_engine_and_refresh_baseline(
        &engine,
        &last_state,
        &canonical,
        ai_write_rejected_source(),
    )
    .await
    .expect_err("source that does not compile must be REFUSED");

    let other_dir = tempfile::tempdir().unwrap();
    let other = other_dir.path().join("copy.ri");
    reify_save_file_on_engine_and_refresh_baseline(
        &engine,
        &last_state,
        Some(other.to_string_lossy().into_owned()),
    )
    .await
    .expect_err("an explicit target must not receive a rejected buffer either");

    assert!(
        !other.exists(),
        "the refused save-as must not have created {}",
        other.display()
    );
}

#[tokio::test]
async fn reify_save_file_succeeds_again_once_the_buffer_compiles() {
    // The interlock is a TRANSIENT one, not a permanent wedge: a
    // successful `reify_update_source` clears `compile_failure` via
    // `commit_state`, and the save then writes the new text. Without this
    // companion the fix could regress into "reify_save_file is dead after
    // any typo".
    let dir = tempfile::tempdir().unwrap();
    let (engine, canonical) = ai_write_engine(dir.path());

    let s0 = current_gui_state(&engine);
    let last_state: std::sync::Mutex<Option<crate::types::GuiState>> =
        std::sync::Mutex::new(Some(s0));

    reify_update_source_on_engine_and_refresh_baseline(
        &engine,
        &last_state,
        &canonical,
        ai_write_rejected_source(),
    )
    .await
    .expect_err("source that does not compile must be REFUSED");
    reify_save_file_on_engine_and_refresh_baseline(&engine, &last_state, None)
        .await
        .expect_err("the rejected buffer must not reach disk");

    // Recover: a buffer that DOES compile.
    let repaired =
        ai_write_source().replace("param depth: Length = 40mm", "param depth: Length = 25mm");
    reify_update_source_on_engine_and_refresh_baseline(
        &engine,
        &last_state,
        &canonical,
        &repaired,
    )
    .await
    .expect("a buffer that compiles must be accepted");

    reify_save_file_on_engine_and_refresh_baseline(&engine, &last_state, None)
        .await
        .expect("the save must work again once the buffer compiles");
    assert_eq!(
        std::fs::read_to_string(&canonical).expect("part.ri must be readable"),
        repaired,
        "the recovered save must persist the REPAIRED text"
    );
}

#[tokio::test]
async fn reify_export_writes_a_non_empty_file() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, _canonical) = ai_write_engine(dir.path());

    let s0 = current_gui_state(&engine);
    let last_state: std::sync::Mutex<Option<crate::types::GuiState>> =
        std::sync::Mutex::new(Some(s0));

    let out = dir.path().join("part.step").to_string_lossy().into_owned();
    let s1 = reify_export_on_engine_and_refresh_baseline(&engine, &last_state, "step", &out)
        .await
        .expect("reify_export('step') must succeed");

    let written = std::fs::metadata(&out).expect("the export target must exist");
    assert!(written.len() > 0, "the exported file must not be empty");

    assert_eq!(
        *last_state.lock().unwrap(),
        Some(s1),
        "the seam must refresh last_state even for a pure-I/O tool"
    );
}

#[tokio::test]
async fn reify_export_rejects_an_unknown_format() {
    let dir = tempfile::tempdir().unwrap();
    let (engine, _canonical) = ai_write_engine(dir.path());

    let s0 = current_gui_state(&engine);
    let last_state: std::sync::Mutex<Option<crate::types::GuiState>> =
        std::sync::Mutex::new(Some(s0.clone()));

    let out = dir.path().join("part.obj").to_string_lossy().into_owned();
    let err = reify_export_on_engine_and_refresh_baseline(&engine, &last_state, "obj", &out)
        .await
        .expect_err("an unsupported format must be REFUSED");
    assert!(
        err.contains("obj"),
        "the refusal must name the format it does not know, got: {err}"
    );
    assert!(
        !std::path::Path::new(&out).exists(),
        "a refused export must not leave a file behind"
    );
    assert_eq!(
        *last_state.lock().unwrap(),
        Some(s0),
        "a refused export must leave the baseline untouched"
    );
}

#[test]
fn parse_export_format_maps_the_shipped_spellings() {
    // ONE map, shared with `commands::export_impl`, so the AI write tool
    // and the Tauri command cannot drift on which spellings are accepted.
    use reify_ir::ExportFormat;
    assert_eq!(crate::commands::parse_export_format("step"), Ok(ExportFormat::Step));
    assert_eq!(crate::commands::parse_export_format("stp"), Ok(ExportFormat::Step));
    assert_eq!(crate::commands::parse_export_format("stl"), Ok(ExportFormat::Stl));

    let err = crate::commands::parse_export_format("obj")
        .expect_err("an unshipped spelling must be refused");
    assert!(
        err.contains("obj"),
        "the refusal must name the offending format, got: {err}"
    );
}

// ── Task 5097 (δ) step-15: RED — the AI write-tool SURFACE ──
//
// The five engine-side write handlers landed in steps 1-14; what is
// still missing is their advertisement. These two tests pin the surface:
// (a) all five names are in `tool_defs()` with the reify-mcp schemas, and
// (b) `reify_open_file` is a second NAME over the existing `open_file`
//     funnel, not a second implementation.
//
// Both FAIL until step-16 adds the ToolDefs, the dispatch arms, and the
// shared `open_file_path_param` helper.

/// Every one of the five reify-mcp write tools must be advertised exactly
/// once, with the reify-mcp property names and `required` lists — an AI
/// client that learned the tool on the `reify-mcp` surface must be able to
/// call it here with the identical arguments (§12 Q1: the `reify_` prefix
/// preserves that identity without clashing with the debug-native bare
/// names).
#[test]
fn tool_defs_registers_the_five_ai_write_tools() {
    let defs = tool_defs();

    // (name, [required...]) — mirrors `crates/reify-mcp/src/tools/write.rs`.
    let expected: &[(&str, &[&str])] = &[
        ("reify_set_parameter", &["cell_id", "value"]),
        ("reify_update_source", &["file_path", "content"]),
        ("reify_open_file", &["file_path"]),
        ("reify_save_file", &[]),
        ("reify_export", &["format", "output_path"]),
    ];

    for (name, required) in expected {
        let matches: Vec<_> = defs.iter().filter(|t| t.name == *name).collect();
        assert_eq!(
            matches.len(),
            1,
            "{name} must be advertised EXACTLY once in tool_defs(), found {}",
            matches.len()
        );
        let entry = matches[0];

        assert!(
            !entry.description.is_empty(),
            "{name}: description must be non-empty"
        );

        let schema = &entry.input_schema;
        assert_eq!(
            schema["type"].as_str(),
            Some("object"),
            "{name}: input_schema.type must be 'object'"
        );

        // Every required field must also be a declared string property —
        // a `required` naming a property the schema never declares is the
        // drift this catches.
        for field in *required {
            assert_eq!(
                schema["properties"][field]["type"].as_str(),
                Some("string"),
                "{name}: properties.{field}.type must be 'string'"
            );
        }

        match schema.get("required") {
            Some(v) => {
                let listed: Vec<&str> = v
                    .as_array()
                    .unwrap_or_else(|| panic!("{name}: input_schema.required must be an array"))
                    .iter()
                    .filter_map(|x| x.as_str())
                    .collect();
                assert_eq!(
                    listed, *required,
                    "{name}: required list must match the reify-mcp schema"
                );
            }
            // reify_save_file's `file_path` is OPTIONAL ("save the active
            // file"), so it ships no `required` key at all — an empty
            // array would be equivalent, absent is what reify-mcp does.
            None => assert!(
                required.is_empty(),
                "{name}: input_schema must declare required {required:?}"
            ),
        }
    }

    // reify_save_file's optional param must still be DECLARED, or an AI
    // client has no way to learn the "save as" arm exists.
    let save = defs
        .iter()
        .find(|t| t.name == "reify_save_file")
        .expect("reify_save_file must be present");
    assert_eq!(
        save.input_schema["properties"]["file_path"]["type"].as_str(),
        Some("string"),
        "reify_save_file: properties.file_path.type must be 'string' even though it is optional"
    );
}

/// `reify_open_file` and `open_file` are ONE funnel under two names — the
/// reify-mcp identity and the debug-native identity — never two
/// implementations. This pins both halves of that: the bare `open_file`
/// def is not duplicated, and the pure param helper both arms share
/// accepts either spelling.
#[test]
fn reify_open_file_shares_the_open_file_funnel() {
    let defs = tool_defs();
    assert_eq!(
        defs.iter().filter(|t| t.name == "open_file").count(),
        1,
        "the debug-native `open_file` def must not be duplicated when \
         `reify_open_file` is added — one funnel, two names"
    );

    // reify-mcp spelling.
    assert_eq!(
        open_file_path_param(&json!({"file_path": "/tmp/a.ri"})),
        Ok("/tmp/a.ri".to_string()),
        "the reify-mcp spelling `file_path` must be accepted"
    );
    // debug-native spelling (what handle_open_file has always taken).
    assert_eq!(
        open_file_path_param(&json!({"path": "/tmp/b.ri"})),
        Ok("/tmp/b.ri".to_string()),
        "the debug-native spelling `path` must be accepted"
    );
    // Both present: the reify-mcp spelling wins, deterministically.
    assert_eq!(
        open_file_path_param(&json!({"file_path": "/tmp/a.ri", "path": "/tmp/b.ri"})),
        Ok("/tmp/a.ri".to_string()),
        "`file_path` must win when both spellings are supplied"
    );
    // Neither: the debug-native refusal, unchanged, so existing callers'
    // error strings do not shift.
    assert_eq!(
        open_file_path_param(&json!({})),
        Err("path is required".to_string()),
        "neither spelling present must produce the existing refusal"
    );
    // Wrong type takes the same arm as absent (mirrors reify_write_str_param).
    assert_eq!(
        open_file_path_param(&json!({"file_path": 7})),
        Err("path is required".to_string()),
        "a non-string path must be refused like an absent one"
    );
}

/// …and the one thing the two names legitimately DO NOT share: the result
/// envelope. `reify_open_file` carries the reify-mcp identity, so a client
/// that learned `result.success` / `result.source` on
/// `crates/reify-mcp/src/tools/write.rs` must find the same two keys here
/// — the debug-native `{ok, path}` reply would give it neither (task #5097
/// δ amendment, review finding).
#[test]
fn reify_open_file_envelope_matches_the_reify_mcp_shape() {
    let source = "structure def Part { param width: Length = 80mm }";
    let envelope = reify_open_file_envelope(source);

    assert_eq!(
        envelope["success"].as_bool(),
        Some(true),
        "reify-mcp's reify_open_file answers `success: true`"
    );
    assert_eq!(
        envelope["source"].as_str(),
        Some(source),
        "`source` must carry the opened file's text verbatim"
    );
    // EXACTLY those two keys: an extra one is envelope drift from the
    // surface this identity was carried over from.
    // Sorted rather than taken in map order, so the assertion does not
    // silently depend on serde_json's `preserve_order` feature.
    let mut keys: Vec<&str> = envelope
        .as_object()
        .expect("the envelope must be a JSON object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec!["source", "success"],
        "the envelope must be exactly {{success, source}} — an extra key \
         is drift from the reify-mcp surface this identity came from"
    );
}

/// The other four write tools' response SHAPES — the half of the contract
/// an AI client actually reads, and the half nothing covered: every Rust
/// test drives the `*_on_engine_and_refresh_baseline` cores (which return
/// a `GuiState`, not an envelope), and `debugParity.test.ts` only checks
/// that the NAMES are advertised. Dropping `new_value`, or letting
/// `diagnostics_count` drift from `diagnostics`, changed the wire shape
/// with the whole suite green (task #5097 δ amendment, review finding).
#[test]
fn reify_write_tool_envelopes_match_the_reify_mcp_shapes() {
    /// Exact key set, sorted so the assertion does not depend on
    /// serde_json's `preserve_order` feature.
    fn keys(v: &Value) -> Vec<&str> {
        let mut k: Vec<&str> = v
            .as_object()
            .expect("every envelope must be a JSON object")
            .keys()
            .map(String::as_str)
            .collect();
        k.sort_unstable();
        k
    }

    fn stamped(message: &str) -> reify_core::DiagnosticInfo {
        reify_core::DiagnosticInfo {
            file_path: "part.ri".to_string(),
            line: 2,
            column: 5,
            end_line: 2,
            end_column: 9,
            severity: "Warning".to_string(),
            message: message.to_string(),
            code: None,
            has_location: true,
        }
    }

    // `reify_set_parameter` — {success, new_value, unit, diagnostics}.
    let set_param = reify_set_parameter_envelope(
        Some("120".to_string()),
        Some("mm".to_string()),
        vec![stamped("a warning")],
    );
    assert_eq!(
        keys(&set_param),
        vec!["diagnostics", "new_value", "success", "unit"]
    );
    assert_eq!(set_param["success"].as_bool(), Some(true));
    assert_eq!(set_param["new_value"].as_str(), Some("120"));
    assert_eq!(set_param["unit"].as_str(), Some("mm"));
    assert_eq!(set_param["diagnostics"].as_array().map(Vec::len), Some(1));

    // An absent cell keeps the KEY SET invariant — `null`, never missing,
    // so a client can read `result.new_value` unconditionally.
    let absent = reify_set_parameter_envelope(None, None, vec![]);
    assert_eq!(
        keys(&absent),
        vec!["diagnostics", "new_value", "success", "unit"],
        "an uncommitted cell must null the values, not drop the keys"
    );
    assert!(absent["new_value"].is_null() && absent["unit"].is_null());

    // `reify_update_source` — {success, diagnostics_count, diagnostics},
    // and the count is DERIVED, so the two can never disagree.
    for diags in [
        vec![],
        vec![stamped("one")],
        vec![stamped("one"), stamped("two")],
    ] {
        let expected = diags.len();
        let env = reify_update_source_envelope(diags);
        assert_eq!(
            keys(&env),
            vec!["diagnostics", "diagnostics_count", "success"]
        );
        assert_eq!(env["diagnostics_count"].as_u64(), Some(expected as u64));
        assert_eq!(
            env["diagnostics"].as_array().map(Vec::len),
            Some(expected),
            "diagnostics_count must equal the list it reports on"
        );
    }

    // The two pure-I/O tools.
    assert_eq!(keys(&reify_save_file_envelope()), vec!["success"]);
    assert_eq!(
        reify_save_file_envelope()["success"].as_bool(),
        Some(true)
    );
    let export = reify_export_envelope("/tmp/out.step");
    assert_eq!(keys(&export), vec!["path", "success"]);
    assert_eq!(
        export["path"].as_str(),
        Some("/tmp/out.step"),
        "`path` must echo the caller's output_path verbatim"
    );
}
