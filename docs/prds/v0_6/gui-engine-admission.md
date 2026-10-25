# PRD: GUI engine admission — one admission per seam, owned by the engine session and the write types

**Status:** active — authored 2026-10-18 by the gap-fill stream session for gui-engine-admission (plans/gap-fill-deliberation-reify-20261016.md §K7-D1, with §K7-D2, the uncontested block's K7-D5/K7-D8/K7-D10 (1), and the fk-0ea9b4872587 rider). Decomposed in the same session (2026-10-19): α #9387 (high, gate #9171), β #9388, γ #9389, δ #9390, ε #9391, ζ #9392, η #9393, ω #9394 (close); absorbed and cancelled: #8471 → γ, #8574 → ε + S25 ρ #8954

Approach: **B+H** (contract §6, boundary tests §7). Milestone: v0_6.

**Code anchors** verified against main `b77273634a` (2026-10-18). The design and scout seats read main
`5d07b395db`; `git diff --stat 5d07b395db..b77273634a` over every file this stream names is empty (the one
intervening commit touches `dark-factory-orchestrator.yaml` only). The R11/R12 revision (2026-10-19) re-measured
every delivered check at main `eb0a855dad`: `git diff --stat b77273634a..eb0a855dad` over this stream's files
touches only `gui/sidecar/src/session.ts` (S26 ζ #9067 split it into `claudeArgs.ts` and `streamEvents.ts`) and
`tests/infra/adoption-ratchet.manifest` (other streams' rows); every count is unchanged. The critic fold (2026-10-19) re-measured
every check again at main `ff82a1049b`; nothing in this stream's files changed since `eb0a855dad`. Main moves fast: cite
by symbol, and re-locate lines at implementation time. This document keeps no line numbers.

**Finding keys (6, all hotspot-survey-reify-20261015):** fk-efe244a6697a (high), fk-0ea9b4872587 (rider),
fk-e9fa33e06157, fk-9f1803be198c, fk-33bb6efd23ac, fk-2513539f4913. Statements and proposals:
`docs/notes/bug-hotspot-survey-2026-10-15-full-findings.json`. §2 maps each key to its leaf.

Vocabulary: this PRD is written in the vocabulary of
`docs/prds/v0_6/gui-backend-engine-modules-events-tool-contract.md` (S25) and composes with S25's end state:
S25 ζ #8942 (one `EngineSession::reload()`), S25 η #8943 (`assemble_gui_state`), λ #8947 (`SessionEvents`, including
the backend-only `design_loaded(canonical_path)`), ξ #8950
(`TauriToolContext<'e>`, `delegated_tool_job`), ο #8951 (the debug server delegates the five `reify_*`
tools), π #8953 / ρ #8954 (`QueuedEngine`), and S26 ι #9070 (`gui/src/bindings.ts`, rule 4a).

Leaves: **α** EntryDocument · **β** entry marker · **γ** WriteTarget · **δ** PreviewState · **ε** DesignWatch ·
**ζ** `SidecarState::require_ready` · **η** sidecar temp grant · **ω** close. (S25's own ζ and η are always
written "S25 ζ #8942" and "S25 η #8943".)

---

## §1 Goal and user-observable surface

Admission lives in the engine. Today each GUI door (the editor, the FS watcher, the AI host, the debug server,
the image tools, the open funnels, the chat commands) decides for itself whether a request may touch the
engine, the disk or the sidecar, and only some doors decide at all. After this PRD every admission is a
property of one type or one method, minted in one place and re-checked where it is used (heuristic 10,
"clear invariants informatively, redundantly, uniformly enforced"):

- a source write names the session's entry (`EntryDocument`, minted by `EngineSession::resolve_target`);
- a persisted buffer is one the engine accepted (`EngineSession::persistable_source`);
- a live parameter preview is explicit engine state with one transition table (`PreviewState`);
- a caller-named write target is admitted by origin (`WriteTarget::admit`);
- the FS watcher follows the engine's loaded design (`DesignWatch::reconcile`, driven by `design_loaded`);
- a sidecar command reaches only a Ready sidecar (`SidecarState::require_ready`);
- the assistant's temp root is one value, `write_confinement::assistant_temp_root()`, which both the GUI's
  write admission and the sidecar's landlock grant use.

What a user observes after each seam lands:

| seam | leaf | observable after landing |
|---|---|---|
| source-write admission | α | Typing in an imported module's tab (reached by go-to-definition) no longer recompiles that module's text as the entry: the viewport keeps the entry's design, and Re-evaluate shows a toast naming both files. Over the debug MCP, `reify_update_source {file_path: <import>}` is refused as `invalid parameters: <import> is not the entry …`. A stale FS-watcher event for a previously open design no longer replaces the new design. `reify_save_file` refuses a buffer the engine rejected, on every host. |
| entry marker | β | Non-entry tabs carry a "not live" badge with a tooltip; typing there triggers no re-evaluation and no error toast; Re-evaluate on such a tab says why it does nothing. |
| write admission | γ | In a `REIFY_DEBUG=1` launch, `screenshot {save_path: "/home/<u>/x.png"}` is refused naming the admitted roots, while `<design_dir>/x.png` and `/tmp/x.png` are written. `reify_export {output_path: "/tmp/part.stl"}` is admitted. File → Save and File → Export are unchanged. |
| preview state | δ | Export in the middle of a slider drag no longer leaves the viewport geometry disagreeing with the property panel; the drag continues from the previewed value. A refused edit-box commit on another parameter no longer resets the slider. The debug `engine_state` read shows `preview_live {cell_id, value}` during a drag and `null` after release. |
| watch target | ε | After a debug-MCP `open_file`, a sidecar `reify_open_file` or File-Open, an external edit of the newly opened `.ri` hot-reloads, and an edit of the previously open file does nothing. A chat sidecar first spawned after a File-Open gets that design's directory as its workspace. |
| sidecar readiness | ζ | Sending a chat message, a permission decision or a "new conversation" while the sidecar is still starting shows "sidecar not ready (still starting)"; after a crash, "sidecar crashed: …". |
| sidecar temp grant | η | With `TMPDIR` set to a non-`/tmp` directory when the GUI is launched, the chat assistant can write a scratch file in `$TMPDIR` (today its landlocked `claude` child is refused there); an image-tool `save_path` under that `$TMPDIR` is admitted, and one under `/tmp` is refused. |

## §2 Background — findings ledger

| key | finding id | severity | verdict | anchor | closing leaf | x_finding_run |
|---|---|---|---|---|---|---|
| fk-efe244a6697a | gui-bridge.1 | high | confirmed | `gui/src-tauri/src/engine.rs::EngineSession.update_source` | α (`x_finding_key`); β (`x_finding_part`) | hotspot-survey-reify-20261015 |
| fk-0ea9b4872587 | gui-bridge.7 | low | weakened (11 sites, not 12) | `gui/src-tauri/src/mcp_context.rs::TauriToolContext` | α (`x_finding_key`, R5; the ratchet row) | hotspot-survey-reify-20261015 |
| fk-e9fa33e06157 | gui-bridge.3 | medium | confirmed (export premise refuted, §3 row 1) | `gui/src-tauri/src/engine.rs::EngineSession.commit_parameter` | δ | hotspot-survey-reify-20261015 |
| fk-9f1803be198c | gui-bridge.5 | low | confirmed | `gui/src-tauri/src/screenshot_save.rs::take_save_path` | γ (`x_finding_key`); η (`x_finding_part`) | hotspot-survey-reify-20261015 |
| fk-33bb6efd23ac | gui-bridge.2 | medium | confirmed | `gui/src-tauri/src/main.rs::watch_file` | ε | hotspot-survey-reify-20261015 |
| fk-2513539f4913 | gui-bridge.6 | low | confirmed | `gui/src-tauri/src/claude_bridge.rs::claude_send_message_impl` | ζ | hotspot-survey-reify-20261015 |

fk-bf57b4017285 (review-reify-20261015, K7-D3) is #9279's key and is not stamped here. This stream orders
#9279 after ε, ζ and η (§9).

History, not scope: #5097 found that `reify_update_source(file_path=lib.ri)` overwrote the entry module and
guarded only the debug-server door (`debug_server::update_source_target_matches_active`). That is the defect
class α closes on every door.

No task carried any of the six keys in `x_finding_key` or `x_finding_part` when this PRD was authored (key
index, 2026-10-18). fk-33bb6efd23ac is named in #8947's gap-fill paragraph but is not stamped there.

## §3 Premise corrections (G6)

Each row: what the finding or brief says, then what the code says at `b77273634a`. Leo was told on 2026-10-18
that the export premise (row 1) is refuted, and ruled R2 on that basis.

1. **"Export mid-preview builds from declared defaults" (fk-e9fa33e06157).** True, but not the defect.
   `EngineSession::export` → `Engine::build` → `check` → `Engine::eval` (`Snapshot::from_compiled_module`)
   re-derives values from the committed module, and the committed value equals the declared default because
   `commit_parameter` persists by splicing the literal into the source (`apply_param_to_source`). So export
   emits the last committed value, consistent with disk. The real defect is downstream: `Engine::build`
   replaces `Engine.eval_state` with the default-based snapshot while `CoreState::last_check` keeps the
   override, so after an export mid-drag `build_gui_state` publishes values from the override and meshes from
   the defaults. R2: no refusal; the session re-establishes the override after the build.
2. **"The override lives only in `last_check`."** It lives in two places: `CoreState::last_check` (written by
   `CoreState::commit_check`) and the reify-eval `Engine`'s own `eval_state.snapshot` (`Engine::edit_check` →
   `edit_param` installs the edited snapshot). `PreviewState` transitions on the writers of both. The doc prose
   on `preview_parameter` and `commit_parameter` understates this.
3. **`reload_would_be_a_no_op` is preview-blind** (`compile_failure.is_none() && last_reload_error.is_none()
   && source == content`). Confirmed. Ruled in the δ table (§6.2, ruling (a)): keep the skip.
4. **`gui/src/bindings.ts` does not exist at main**, and no manifest carries specta or tauri-specta. Rule 4a
   binds only after #9070. β and δ, the two wire-changing leaves, depend on #9070 and regenerate the file. α,
   γ, ε and ζ change no command signature, wire type or event payload.
5. **`screenshot_save::take_save_path` makes three checks** (a string, absolute, `.png`), not two. None is a
   confinement.
6. **There is no `commit_parameter` Tauri command.** The durable commit is `set_parameter` (main.rs) →
   `commands::set_parameter_impl` → `EngineSession::commit_parameter`. The transition table names the engine
   method.
7. **There is no `SidecarHandle::shutdown`.** The primitive is `SidecarHandle::kill`; the module fn
   `claude_bridge::shutdown_sidecar` locks the slot, kills and clears. K7-D3's "shuts the sidecar down" maps to
   those.
8. **Landlock is applied per `claude --print` child, per turn.** `gui/sidecar/src/sandbox.ts::wrapClaudeArgs`
   wraps every spawn as `landlock_exec.py --writable <workspace> --writable ~/.claude --writable /tmp`;
   `session.ts` re-spawns per turn with `--resume <sessionId>`; the sidecar process itself is unconfined;
   `REIFY_WORKSPACE` is read once at sidecar start (`index.ts`). This is R6's note on #9279.
9. **`source_map` holds only the entry.** `CoreState::commit_state` clears it and inserts the entry, so
   `GuiState.files` has one entry and `initApp` opens one tab. An import becomes the active tab only through
   `navigateToDiagnostic`, go-to-definition (`Editor.tsx::onCrossFileNavigate`), `FileBrowser`/`FileTabs`
   clicks, or the debug `open_file`. (A scout's claim that `initApp` opens imports is wrong.) The defect still
   reproduces after any of those navigations: `Editor.tsx`'s `updateListener` sends
   `updateSource(activeFile, …)` and `App.tsx::handleReEvaluate` sends `bridgeUpdateSource(file.path, …)`,
   and `EngineSession::update_source` ignores `path` whenever `core.file_path()` is set.
10. **Raw `.lock()` sites in `mcp_context.rs`: 11**, not 12 (the verifier is right). S25 ξ #8950 adopts
    `with_engine_lock` in the host (its §4.10 placement row 13), so the ENGINE-lock count is 0 after ξ; the
    `.lock()` sites that remain are the host's own `produced` slot (S25 §4.10). α adds the row with that named
    residue as its ceiling (R5; §4 lead decision; Appendix A).
11. **File-Open re-arms the watcher with the raw path string.** `main.rs::open_file_engine` passes
    `Path::new(&path)`, not the canonical path the engine adopted; argv passes the canonical one. This is a
    third per-funnel divergence the finding did not list. ε removes it by construction (one source:
    `design_loaded(canonical_path)`).
12. **`docs/invariants.md`'s row grammar is about to change** (corrects the design's "no four-site lockstep
    procedure exists"). At `b77273634a` the file has the INV-META-1 header and the 5-column table, but S24 ξ #8821
    (released for dispatch on 2026-10-14) adds a Pointer column (`| ID | Invariant | Enforcement | Pointer | Status
    | …`), the pointer grammar `item:<path>::<Symbol>; test:<path>`, the `reify-audit --pattern PREGISTRY` gate
    member (mapped to `docs/invariants.md` in `scripts/verify-pipeline-infra-tests.txt`, so it runs on the
    docs-only pre-commit too) and the four-site lockstep (the INV row, the design-invariants census row, a
    CLAUDE.md Pointers row, the program-doc pointer). α lands after S25 ζ/λ/ξ/ο, so #8821 may land first. §6.6
    therefore gives the INV-GUI-4 row in both forms. Of the four sites, α writes the registry row only (INV-GUI-4
    gates no declared surface, so it has no census row, and it adds no CLAUDE.md Pointers row; the program-doc
    pointer is the lead's); ω refreshes the row.
13. **A private `fn resolve_target` already exists in `write_confinement.rs`.** α's delivered check is scoped
    to `engine.rs`; γ renames the private helper `resolve_requested`, so the symbol is unique in the crate.
14. **#8947's metadata does not stamp fk-33bb6efd23ac** (it is named in its gap-fill paragraph only). ε closes
    it.
15. **`holds_rejected_source` has exactly one production consumer**
    (`debug_server::reify_save_file_on_engine_and_refresh_baseline`). `TauriToolContext::save_file` writes
    `build_gui_state().files[0].content` unguarded and unconfined today. ξ moves the interlock to the host
    (placement row 7); α replaces it with `persistable_source`.
16. **The "fourth funnel" (`TauriToolContext::open_file`, #8574) is deleted by rewrite upstream.** ο deletes
    `mcp_tool_call`; ξ placement row 9 routes the host's `open_file` through `canonicalize_debug_open_path` +
    `load_initial_file_impl`. After ρ the debug open funnel is the same impl. Every funnel reaches
    `design_loaded` through one body (`load_initial_file_impl` → `load_file` → `reload()`), so ε needs no
    per-funnel code.
17. **`AppState.initial_file` has one writer (argv, main.rs) and one reader** (`claude_send_message` →
    `claude_bridge::resolve_workspace_dir`).
18. **The framing seat's scratch file is gone.** Its counts were re-derived from code: 1 of 4 source-write
    doors guarded, 2 of 4 open funnels re-arm the watcher, 11 raw locks. All confirmed.
19. **`CheckResult` carries `values: ValueMap` and `resolved_params`** (reify-eval). It is the source of the
    committed value a `LivePreview` remembers (§6.2).
20. **Freshness.** `git diff 04be9302ea..5d07b395db` touches only `gui/src/App.tsx` (S26 β–ε landings) and
    `tests/infra/adoption-ratchet.manifest` among this stream's files; no anchor in the Rust files moved.
    `handleReEvaluate`, `navigateToDiagnostic`, `loadPathIntoStores` and `initApp` were re-read.
21. **The watcher door's path is not canonical until ε lands** (corrects the design note "editor/watcher paths
    are canonical by construction"). `FileWatcher` watches the directory it was given, and notify reports
    `<that dir>/<name>`. File-Open arms the watcher with the raw path (row 11), so a watcher event after a
    File-Open through a symlinked or otherwise non-canonical spelling names a path that is neither the
    canonical entry nor its stem key. Under α's admission that event would be refused and hot-reload would
    stop working for that design until ε. So the shared door body `commands::update_source_impl` asks
    `resolve_target` with the requested spelling first and, on a `NotTheEntry` refusal, once more with
    `path_key::canonicalize_document_key`'s spelling — the order the debug original
    (`update_source_target_matches_active`) uses. Raw first matters for the host: `files[].path` is a stem-only
    key (`"main.ri"`) that clients round-trip, and canonicalising it first would resolve it against the GUI
    process's cwd. Editor paths are canonical already: the entry's tab comes from `open_file_impl` /
    `rewrite_files_to_abs` / `canonicalize_debug_open_path`. (Lead decisions, §4.)
22. **The `initial_file` ERE in the design's ratchet row matches function names.** A bare `initial_file`
    pattern counts `load_initial_file_impl`, `resolve_initial_file_path`, `begin_initial_file_load` and
    `initial_file_evaluation`, which ε does not rename, so the design's ceiling (5) would red on ε's own diff.
    The row is re-scoped to the `AppState` field's constructs (Appendix A, `app-state-initial-file`): today 4,
    ceiling 0. `resolve_workspace_dir`'s `initial_file` parameter is #9279's (R4: ε does not edit
    `claude_bridge.rs`).
23. **`esc-7281-4` appears twice in `engine.rs`**: once in `preview_parameter`'s doc paragraph and once inside
    `commit_parameter`'s refusal comment block. δ deletes both sites' prose (§6.2 deletions).
24. **`door-raw-fs-write` residue.** Of today's 8 matches in `gui/src-tauri/src/*.rs`, 4 are caller-named door
    writes (`commands::save_file_impl`, `EngineSession::export`, `TauriToolContext::save_file`,
    `screenshot_save::save_image_result`) and 4 are not doors: `commands::write_view_sidecar_impl` (1), two
    writes in `lsp_bridge.rs`'s inline tests, and one in `debug_server.rs`'s inline tests. The design's residue
    list omitted the last. γ measures the residue at landing (§6.8).
25. **The sidecar's landlock bound also includes `~/.claude`.** R8's admitted roots are the design directory and
    the system temp directory; `~/.claude` is deliberately not an admitted write root.
26. **The sidecar's temp grant is the literal `/tmp`, not the system temp directory.** `sandbox.ts::wrapClaudeArgs`
    passes `--writable /tmp`, and the vendored `gui/src-tauri/sandbox/landlock_exec.py` grants `/tmp` itself.
    Rust's `std::env::temp_dir()` and Node's `os.tmpdir()` both follow `TMPDIR`. So today a GUI launched with a
    non-`/tmp` `TMPDIR` leaves the landlocked `claude` child unable to write its own temp files, and R8's "the
    assistant's admitted roots match its landlock bound" would not hold. R12 makes one value, `assistant_temp_root()`,
    feed both sides (η), so R8's temp root is exactly the child's temp grant.
27. **The sidecar's vitest suite is not a merge-gate member.** `scripts/verify.sh`'s sidecar block runs
    `npm run typecheck` and `npm run typecheck:test` only ("sidecar has no vitest side"), and `gui/vitest.config.ts`
    excludes `sidecar/**`. η's `sandbox.test.ts` case runs under `npm test` in `gui/sidecar` and is type-checked by
    the gate, but its assertions are not gated; η's gated covers are the Rust `claude_bridge_tests` case, the
    mandatory new `tests/infra` landlock member (BT18'') and η's delivered checks (§10 η).
28. **`commands::engine_state_json` is a hand-built projection, not the `GuiState`.** It names `meshes`,
    `values`, `constraints`, `files`, the diagnostics, `stale`, `reload_error`, `demand_prune_measurement` and
    `last_dispatch_count_post_refresh`, and `engine_state_view` takes its valid field names from that payload. A
    new `GuiState` field therefore does not reach the debug `engine_state` read on its own: δ adds
    `"preview_live"` to the projection. `GuiState.demand_prune_measurement` is the precedent for a field whose
    only reader is that read (`full_reload_only("observability-only via MCP engine_state_json …")`).
29. **`engine::source_key_matches_path` is `#[cfg(feature = "gui")]`** ("gated to match its consumers":
    `debug_server` only). `resolve_target` in the ungated session core calls it, so α lifts the gate and its
    comment, or the default-feature build's `clippy -D warnings` pass fails.
30. **After S25 η #8943 there are three `GuiState` literals**, not two: `assemble_gui_state`, the cold-start
    literal and `build_preview_gui_state` (#8943: "cold-start and build_preview_gui_state keep their own
    literals"). β and δ populate their field in all three (a missing field is a compile error).
31. **`main.rs` builds and `.manage()`s `AppState` before `.setup()`**, but `create_watcher`'s body needs the
    `AppHandle`, which exists only inside `setup()` (where today's emitters are installed). §6.4 says where
    `DesignWatch` is built.

## §4 Rulings

### From the record (binding; full text in `plans/gap-fill-deliberation-reify-20261016.md`)

- **K7-D1** (human, 2026-10-16): **B**, against the lead's recommendation A. A new stream
  `gui-engine-admission`: GUI admission policy lives in `EngineSession` and the write-confinement types, not in
  per-door guards, designed across all five seams in one PRD. It follows S25 ζ #8942, λ #8947 and ξ #8950; ξ keeps
  its active-file guard behind one named function, which this stream re-homes onto `resolve_target` and
  deletes; bindings regenerate under #9070's rule 4a; this stream is the G4 owner of the `claude_bridge.rs`
  `SidecarState` table.
- **K7-D2** (human, 2026-10-16): **A**. λ #8947 gains the backend-only `design_loaded(canonical_path)` event; a
  `DesignWatch::reconcile(path)` driven by it re-arms the watcher only when the path differs, on every open
  funnel; the explicit `watch_file` calls are deleted; #8574 is subsumed (its editor-notify half to ο/ρ's
  `open_file` push).
- **K7-D3** (human, 2026-10-16): **A + condition**. The sidecar is keyed on its spawned workspace, derived from
  the engine's design; a workspace change respawns it, resuming the CLI session where possible, and a lost
  session shows a reset message. Filed as #9279, which follows this stream's DesignWatch leaf.
- **Uncontested block** (human, 2026-10-16), as it binds here: K7-D5 (PreviewState, fk-e9fa33e06157),
  K7-D8 (`WriteTarget::admit`, #8471 absorbed, fk-9f1803be198c) and K7-D10 (1) (`SidecarState::require_ready`,
  fk-2513539f4913) are this stream's scope; and the rider: fk-0ea9b4872587 is covered by #8950 plus an
  adoption-ratchet row for raw `.lock()` in `mcp_context.rs`, which is this stream's to place.
- **G1** (human, 2026-10-16): **hybrid**. #8612's transitive dependency closure is protected: no gap-fill leaf
  becomes a prerequisite of a closure task; substantive widenings of a closure leaf take follow-on form; the two
  named exceptions are not this stream's.

### This session (Leo, 2026-10-18)

Each ruling gives the option set considered and the consequence that decided it.

- **R1 — refusal UX on the editor and watcher doors: a marker leaf in-stream.** Options: an in-stream frontend
  marker leaf; a follow-up; no marker. Consequence: a refusal that is honest but unexplained is a UX regression
  from "silently wrong", so the published state names the engine's entry, `FileTabs` badges non-entry tabs
  "not live" with a tooltip, and `Editor`'s `updateListener` and `App`'s `handleReEvaluate` stop sending
  `updateSource` for non-entry tabs. The backend refusal (α) stays the authority: a stray call is refused. One
  medium frontend leaf (β) after α and #9070.
- **R2 — export while a preview is live: export committed; the preview survives.** Options: a typed
  `PreviewLive` refusal; a flag on the reply; export committed and re-establish the preview. Consequence: the
  probe refuted the "export builds from declared defaults" premise (§3 row 1), so there is no wrong export to
  refuse; the real defects are the `eval_state`/`last_check` split after export and the preview-blind
  `reload_would_be_a_no_op`. No refusal and no wire change on export. `PreviewState` is an input to export's
  transition: after the build the session re-applies the live override, so values and meshes agree and the drag
  continues. Save already writes committed source.
- **R3 — invariant placement: a new INV-GUI-4 row owned by this stream.** Options: extend INV-GUI-2 (S25 ρ
  #8954's final row); a new row. Consequence: one property per row (heuristic 6). The leaf that delivers the
  enforcement adds the row and flips its status in the same diff; the close leaf refreshes it. INV-GUI-2 is
  not edited.
- **R4 — order on contended files: edges where code builds on code.** Options: edges; any order with
  adapt-second. Consequence: `claude_bridge.rs`: ζ ← #9076 (both edit `SidecarHandle::send_message`; the
  closure leaf lands first and this stream adapts); #9279 ← ζ and #9279 ← ε; #9284 is file-lock only (disjoint
  constructs). `watcher.rs`: ε re-arms by replacing the `FileWatcher` instance and does not edit
  `watcher.rs`, so #9285 is file-lock only.
- **R5 — the fk-0ea9b4872587 rider goes on α and closes the key.** Options: on α, closing the key; its own low
  leaf after S25 ξ; a sentence-sized widening of S25 ξ #8950. Consequence: α adds row
  `mcp-context-raw-engine-lock` (ceiling 0 for the engine mutex; the row's ceiling is the named residue of the
  host's own `produced` slot — see the lead decision below) and carries fk-0ea9b4872587 in `x_finding_key`
  beside fk-efe244a6697a (same run).
- **R6 — #9279's premise: append a note to #9279.** Consequence: landlock is applied per `claude --print`
  child per turn; the sidecar process is unconfined; `REIFY_WORKSPACE` is read once at sidecar start;
  `--resume <sessionId>` is already passed per turn. #9279's architect picks respawn or in-place re-key, still
  bound by K7-D3's condition (a visible reset message whenever the session cannot be resumed).
- **R7 — #9174 (use-time O_NOFOLLOW re-check, fk-19a30824e41b): edge + note.** Options: edge + note; absorb;
  no relation. Consequence: #9174 ← γ, with a note re-targeting it to a use-time check inside
  `WriteTarget::write`. Keys stay on separate leaves (no mixed-run leaf).
- **R8 — Assistant-origin admitted roots (γ): the design directory ∪ the system temp directory, for every
  Assistant write** (image-tool `save_path`, host `save_file` save-as, host `export`). Options: one root set
  for every Assistant write; the temp dir for image tools only (an origin × kind matrix); the design dir only.
  Consequence: one axis (heuristic 3), matching the sidecar child's own landlock bound, so it grants nothing
  the assistant lacks. It widens the documented `reify_save_file`/`reify_export` rule (design dir only) to
  include the temp dir; γ records that in `docs/debug-mcp-contract.md`.
- **R9 — `AppState.initial_file`: ε deletes it now.** Options: ε deletes it; #9279 deletes it. Consequence:
  one home immediately (heuristic 11). ε deletes the field and its argv writer, and `claude_send_message` reads
  `DesignWatch::design_path()`. Side effect: a sidecar first spawned after a File-Open gets that design's
  directory; pinning after the first spawn stays until #9279.
- **R10 — β's priority: medium, per R1.** An `x_finding_part` key does not set priority. α is the high leaf and
  the #9171 gate dependency; β gets no gate edge.
- **R11 — α's `update_source` error: a typed error from `engine.rs`.** Options: keep `Result<GuiState, String>`
  (a `Superseded` refusal at use comes back as a string the doors cannot tell from a reload failure); a typed
  error defined in `engine.rs`. Consequence: `EngineSession::update_source` returns
  `Result<GuiState, SourceUpdateError>` with `SourceUpdateError { NotActiveEntry(NotActiveEntry), Reload(String) }`
  beside `EntryDocument`; `commands::update_source_impl` maps it, and only `Reload` records the reload error
  (heuristic 12), so a `Superseded` refusal at use never raises the staleness banner. One error type, one home
  (heuristic 11): the doors use it directly, with no second refusal enum in `commands.rs`.
- **R12 — the temp root (refines R8): one home, and the vendored helper is patched too.** Leo: "replace the
  hard-coded /tmp with something that always matches std::env::temp_dir(), or explain why that's infeasible."
  Options: document the `/tmp` mismatch; pin γ's root to the literal `/tmp`; one home that both sides read.
  Consequence (feasible, ruled one home): `write_confinement::assistant_temp_root()` (canonical
  `std::env::temp_dir()`) is the one home and γ's `WriteTarget::admit` uses it; a new low leaf η makes
  `claude_bridge::compute_sidecar_env` export it as `TMPDIR`, makes `sandbox.ts::wrapClaudeArgs` grant
  `os.tmpdir()` instead of the `'/tmp'` literal, and makes `landlock_exec.py`'s built-in grant read `TMPDIR`
  (fallback `/tmp`, Rust's rule), recorded as a second REIFY DIVERGENCE in its vendor header. η depends on γ and
  ζ (the `claude_bridge.rs` chain #9076 → ζ → η → #9279); #9279 ← η. Side benefit: a non-`/tmp` `TMPDIR` no
  longer leaves the landlocked child unable to write its own temp files (§3 row 26).

### Lead decisions (this session, 2026-10-18/19; no fork put to Leo)

- **α's doors fall back to the canonicalised spelling** (`path_key::canonicalize_document_key`) when the
  requested spelling is not the entry (§3 row 21), rather than an α ← ε dependency edge. Reason: File-Open arms
  the watcher with the raw path until ε lands, and an α ← ε edge would hold the high leaf (and the #9171 gate)
  behind ρ #8954, which ε depends on. The raw spelling is asked first (critic C7), so a stem key is never
  resolved against the GUI's cwd.
- **R5's row keeps its ERE; its ceiling names a residue (critic C1).** `mcp-context-raw-engine-lock` stays
  `\.lock\(\)` on `mcp_context.rs`, but S25 ξ's host owns a non-engine mutex by design
  (`produced: Mutex<Option<GuiState>>`, S25 §4.10), so a zero ceiling would red α's own diff. The ceiling is the
  post-S25-ξ residue, named: the host's `produced` slot's lock sites, measured when α lands, with the
  engine-mutex count 0 (`with_engine_lock` everywhere). R5's "ceiling 0" intent — no raw ENGINE lock in the host —
  is preserved: a new raw engine lock pushes the count past the ceiling. (The lead informs Leo.)
- **`preview_live` is `full_reload_only` (critic C8).** Its only readers in this stream are the debug
  `engine_state` read and δ's tests, so a delta event channel would have no consumer (umbrella
  `nothing-vacuous-and-unowned`). δ classifies it `full_reload_only("observability-only via engine_state_json")`
  (the `demand_prune_measurement` precedent) and adds it to the `engine_state_json` projection (§3 row 28); δ
  has no μ #8948 edge and adds no event.
- **γ alone changes `EngineSession::export`'s signature (critic C6).** γ already names every caller's file and
  follows ο/ρ; δ's re-apply sits inside `export` whatever its signature is.

## §5 Sketch of approach

Five seams, one leaf each, plus a frontend leaf, a sidecar temp-grant leaf and a close. Every leaf follows S25 ζ #8942, λ #8947 and ξ #8950,
so each lands on S25's final shape of the session core, the event sink and the AI host.

- **α** makes the source-write door a type. `resolve_target` is the only way to obtain an `EntryDocument`, and
  `update_source` accepts only that. The three doors (editor, watcher, host) share one body,
  `commands::update_source_impl`, that resolves and writes under one lock. `persistable_source` replaces the
  `holds_rejected_source` convention. ξ's named guard function is deleted.
- **β** publishes the entry (`GuiState.entry_path`) so the frontend can say which tab is live and stop asking
  for writes the engine will refuse.
- **γ** makes the write target a type. `WriteTarget::admit(origin, requested, design)` is the only way to
  obtain one, and every caller-named write (Tauri save/export, host save/export, image `save_path`) takes it.
  `WriteTarget::write` is the one write, which #9174 later hardens.
- **δ** makes preview liveness explicit engine state with one transition table; export re-applies the live
  override; a refused commit discards only the matching preview.
- **ε** makes the watcher follow the engine: `design_loaded` (emitted by `reload()` whenever a ticket loads a
  design) drives `DesignWatch::reconcile`, the one arm site.
- **ζ** gives the sidecar's state table one refusal owner, `SidecarState::require_ready`, checked at the handle
  and at the slot.
- **η** makes the sidecar's temp grant the GUI's admitted temp root: the GUI exports `assistant_temp_root()` as
  `TMPDIR`, and the sidecar and the landlock helper grant what `TMPDIR` names.
- **ω** stamps the PRD terminal.

## §6 Contract (H)

All Rust paths are under `gui/src-tauri/src/`. Where #9070 has moved a Tauri command from `main.rs` to
`tauri_commands.rs`, the edit lands there instead; the architect re-acquires the file.

### §6.1 Seam 1 — source-write admission (α)

```rust
// engine.rs (session core; after S25 ζ #8942 and ξ #8950)

/// Proof that a source write names this session's entry. Minted ONLY by
/// `EngineSession::resolve_target`; consumed by `update_source`. Not Clone, not Copy:
/// one admission, one write, no stash-and-reuse across locks.
#[derive(Debug)]
pub struct EntryDocument { target: EntryTarget }           // all fields private
#[derive(Debug, Clone, PartialEq, Eq)]
enum EntryTarget {
    /// A `load_file` session: the canonical entry path the session holds now.
    OnDisk(PathBuf),
    /// A `load_from_source` session (no `file_path`): the module name the caller's path stem names.
    Fileless { module_name: String },
}

/// Informative refusal (heuristic 10 "informatively").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotActiveEntry {
    /// `requested` is not the entry: neither its canonical spelling nor its stem key.
    NotTheEntry { requested: PathBuf, active: PathBuf },
    /// The document was minted for `minted_for`, but at use the session holds `active`
    /// (or none). Re-checked at use; unreachable while mint and use share one lock.
    Superseded { minted_for: PathBuf, active: Option<PathBuf> },
}
impl fmt::Display for NotActiveEntry {
    // NotTheEntry: "{requested} is not the entry of this session ({active}): only the entry's text is recompiled"
    // Superseded:  "{minted_for} was resolved as the entry, but {active} is open now"
}
impl std::error::Error for NotActiveEntry {}

/// Why `update_source` did not recompile (R11). One error type, one home: the doors use it directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceUpdateError {
    /// Admission refused; the engine is untouched. Never raises the staleness banner.
    NotActiveEntry(NotActiveEntry),
    /// The recompile itself failed (compile error, or a panic converted by `with_engine_lock`).
    Reload(String),
}
impl fmt::Display for SourceUpdateError { /* the inner value's Display */ }
impl std::error::Error for SourceUpdateError {}

/// The session holds text it REJECTED, so `files[].content` is not persistable.
/// Replaces the `holds_rejected_source` convention.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectedBuffer { pub file_key: String, pub diagnostics: usize }
impl fmt::Display for RejectedBuffer {
    // "refusing to persist {file_key}: the in-memory buffer does not compile ({diagnostics} diagnostic(s));
    //  recover with a source update that compiles, or edit the file and let the watcher reload it"
}

impl EngineSession {
    /// PURE: no filesystem I/O; callable under the engine lock. Accepts exactly:
    ///  * a `load_file` session: `requested == canonical_file_path()` verbatim, or the stem-only
    ///    module key via `source_key_matches_path(requested, entry)` (the spelling `get_diagnostics` stamps);
    ///  * a fileless session: anything (`update_source` honours the caller's stem there).
    /// The stem key is accepted verbatim; canonicalisation is a door fallback (§6.1 doors).
    pub fn resolve_target(&self, requested: &Path) -> Result<EntryDocument, NotActiveEntry>;

    /// The one recompile-from-text entry. Re-checks the document against the live entry at use
    /// (`EntryTarget::OnDisk(p)` requires `core.file_path() == Some(p)`; otherwise
    /// `Err(SourceUpdateError::NotActiveEntry(NotActiveEntry::Superseded{..}))`, engine untouched),
    /// then routes to S25 ζ's `reload()` with `FilePathUpdate::Preserve`; a failed reload is
    /// `Err(SourceUpdateError::Reload(msg))`. The ignored `path: &str` parameter is gone.
    pub fn update_source(&mut self, doc: EntryDocument, content: &str) -> Result<GuiState, SourceUpdateError>;

    /// `Ok(committed entry source)` when the session holds no `compile_failure`; otherwise the
    /// rejected-buffer fact. A consumer that persists text calls THIS, never `files[].content`.
    pub fn persistable_source(&self) -> Result<&str, RejectedBuffer>;
}
```

Every internal caller of `update_source` (today `apply_param_to_source`'s write and its rollback, and
`discard_parameter_preview`; after S25 ζ #8942, whatever still calls it) mints its document with
`self.resolve_target(<the entry path or fileless key>)` immediately before the call, and maps either error to
its own `String` error with `to_string()`.

**Deleted by α:** `EngineSession::holds_rejected_source` and the gating comment about its one consumer; ξ's
named host guard function in `mcp_context.rs` (whatever ξ names it; #8950's details require one named
function) and its tests; the `update_source` doc text "path is used only for the single-file fallback"; the
`(path, source)` key-selection commentary in `discard_parameter_preview` (δ rewrites that fn; α leaves it
minting through `resolve_target`). **Kept:** `engine::source_key_matches_path` (its admission caller becomes
`resolve_target`; the diagnostics filter still uses it). The both-sides `std::fs::canonicalize` arm of the
debug original (`update_source_target_matches_active`, which ο deletes) is not re-created: the entry is
canonical by construction (`load_initial_file_impl`'s precondition; ξ placement row 9 for the host;
`canonicalize_debug_open_path` for the debug funnel), and the shared door body falls back to the canonical
spelling when the requested one is refused.

**Doors — all three go through `commands::update_source_impl`:**

```rust
// commands.rs
// (no refusal enum here: the doors use `engine::SourceUpdateError`, R11)
/// Computes `canonical = path_key::canonicalize_document_key(requested)` BEFORE the lock (filesystem
/// I/O), then, under ONE `with_engine_lock`: `resolve_target(requested)`, and on `NotTheEntry`
/// `resolve_target(canonical)` (when it differs), reporting the FIRST refusal (it names the caller's
/// spelling); then `update_source`, no window between. Only
/// `SourceUpdateError::Reload` calls `record_reload_error`; a `NotActiveEntry` (either variant)
/// never raises the entry's staleness banner. A panic converted by `with_engine_lock` is recorded
/// as today and returned as `Reload(panic)`.
pub fn update_source_impl(engine: &Mutex<EngineSession>, requested: &Path, content: &str)
    -> Result<GuiState, SourceUpdateError>;
pub fn reload_for_watch_impl(..)   // Reload(_) → today's fallback publish; NotActiveEntry → Err, no publish
pub fn editor_source_edit(..)      // door 1
pub fn disk_reload_edit(..)        // door 2
```

- **Door 1 (editor).** The Tauri `update_source(path, content, order)` signature is unchanged.
  `editor_source_edit` replies `Err(refusal.to_string())` and publishes nothing. The refusal reaches
  `Editor.tsx`'s `.catch` (console) and `handleReEvaluate`'s toast ("Re-evaluation failed: <NotActiveEntry
  text>"). β stops the frontend sending it; α's refusal remains the authority (R1).
- **Door 2 (watcher).** `disk_reload_edit` reads the file and passes the event path; the shared fallback
  admits a non-canonical spelling of the entry (§3 row 21). A `FileEvent::Changed(old_design)` after a
  funnel switch names a path that is no longer the entry: it is refused with
  `tracing::warn!(requested, active, "stale watcher event refused")`, the reply is `Ok(())`, nothing is
  published and the engine is unchanged. This closes the "stale watcher clobbers the new design" half of
  fk-33bb6efd23ac independently of ε; ε removes the stale watcher itself.
- **Door 3 (AI host, after ξ).** `TauriToolContext::update_source(file_path, content)` calls
  `commands::update_source_impl(self.engine, Path::new(file_path), content)`, which takes the lock, asks with
  the raw spelling (so the stem key `"main.ri"` that `files[].path` advertises is admitted whatever the GUI's
  cwd holds) and falls back to the canonical one (so a `..` spelling of the absolute entry path is admitted). `NotActiveEntry` → `ToolError::InvalidParams(e.to_string())`
  (the caller named the wrong file: a parameter error). `Reload` → `ToolError::EngineError` as today (S25
  placement row 15: the hot-reload banner behaviour stays).
- **`persistable_source` consumers.** The host `save_file` (ξ placement row 7): α replaces "interlock +
  `build_gui_state().files[0].content`" with
  `let text = s.persistable_source().map_err(|e| ToolError::EngineError(e.to_string()))?` (no state rebuild).
  The Tauri `save_file` carries the frontend's own content, consults no engine state, and is unchanged.
  `apply_param_to_source` keeps its own disk-versus-session divergence check (a different predicate).

**Enforcement** (INV-GUI-4, §6.6). Redundant: at construction (`resolve_target` is the only minter;
`EntryDocument` has no public constructor, so a door cannot forge one) and at use (`update_source` re-checks
`OnDisk(p)` against `core.file_path()`). Uniform: one predicate, one error type, three doors calling one
`update_source_impl`.

**Regression test** (α, `tests/commands_tests.rs`, real engine over a tempdir two-file design shaped like
`engine_tests.rs`'s task-3228 import fixture, `helper.ri` + `main.ri`): load `main.ri`;
`update_source_impl(engine, &dir.join("helper.ri"), "pub structure Helper { param x: Length = 99mm }")` →
`Err(SourceUpdateError::NotActiveEntry(NotTheEntry{requested: helper, active: main}))`; `build_gui_state().values["Helper.x"]`
is still 10mm; `files[0].content` unchanged; `is_stale() == false`; the same call naming `main.ri` → `Ok`.
The host and watcher doors are BT2/BT3 (§7).

### §6.2 Seam 2 — PreviewState (δ, with R2)

```rust
// engine.rs (session core), beside `compile_failure`
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PreviewState { None, Live(LivePreview) }
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LivePreview {
    cell: ValueCellId,        // parsed once by preview_parameter
    cell_id: String,          // the "Entity.member" spelling, for the wire and for refusal matching
    committed: Value,         // last_check.values[cell] at the None→Live transition
    previewed: Value,         // the override installed now; re-applied after export
}
impl EngineSession {
    pub(crate) fn preview_state(&self) -> &PreviewState;
    /// `edit_check(cell, committed)` → `commit_check` → `PreviewState::None`. No recompile, so no
    /// `(compile_failure, last_reload_error)` snapshot/restore. Replaces `discard_parameter_preview`.
    fn discard_live_preview(&mut self) -> Result<(), String>;
    /// δ adds the re-apply (transition table, row `export`) inside `export`, whatever its signature is
    /// when δ lands; γ alone changes the signature to return the bytes (§6.3).
    pub fn export(..) -> ..;
}
```

```rust
// types.rs — wire field (after ι #9070); observability-only, like demand_prune_measurement
full_reload_only("observability-only via engine_state_json")
#[serde(default, skip_serializing_if = "Option::is_none")]
pub preview_live: Option<PreviewLiveInfo>,

// commands.rs::engine_state_json — the projection gains one key (§3 row 28):
//     "preview_live": gui_state.preview_live,      // null when no preview is live

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]  // + the specta derive per #9070's convention
pub struct PreviewLiveInfo { pub cell_id: String, pub value: String }   // `value` rendered as ValueData.value is
```

**Export's signature is γ's alone** (lead decision, §4). δ's re-apply sits inside `EngineSession::export`, so it
does not depend on the signature, and γ and δ have no edge between them.

**Invariant.** `PreviewState::Live(p)` ⇔ `last_check` and `Engine.eval_state` carry exactly one override, of
`p.cell`, equal to `p.previewed`; `PreviewState::None` ⇔ both derive from the committed compiled module alone.
At most one live preview exists; `PreviewState` owns it; it changes only through the transitions below.
Enforcement: the field is private; its writers are `preview_parameter` (→ Live), `commit_state` (→ None,
covering every reload, load, commit and rollback), `discard_live_preview` (→ None) and `export` (Live kept,
override re-applied). The Engine's two override writers are reachable only through those paths (`edit_check`
via `preview_parameter`, `discard_live_preview` and export's re-apply; cold `eval` via `reload()` and `build`).
Tests over the real eval engine (`engine_tests.rs` fixtures) assert `preview_live` around every row, and the
debug MCP observes it through the `engine_state` projection.

**Transition table** (rows are events; "before → after"; what else moves; what is published):

| event | before → after | side effects | published |
|---|---|---|---|
| `preview_parameter(c, v)` ok | None → Live{c, committed=last_check[c], previewed=v} | `edit_check(c,v)`, `commit_check`, cache invalidation (as today) | state with the override, `preview_live=Some` |
| `preview_parameter(c, v')` ok, Live{c} | Live{c,…,v} → Live{c,…,v'} | `committed` kept from the first frame | as above |
| `preview_parameter(d, w)` ok, Live{c≠d} | Live{c} → Live{d} | `discard_live_preview()` FIRST (c back to committed), then `edit_check(d,w)` | override of d only |
| `preview_parameter` refused (unknown cell, parse, `edit_check` Err) | unchanged | none (the `?` short-circuits before `commit_check`) | nothing (Err) |
| `commit_parameter` accepted (any cell) | Live/None → None | `apply_param_to_source` → `update_source` → `reload()` → `commit_state` (the unconditional recompile stays: it reconciles a drag released at the original value) | committed state, `preview_live=None` |
| `commit_parameter` refused, Live{c}, refused cell == c | Live{c} → None | `discard_live_preview()`; the refusal first in the message, a discard Err appended (as today). No `failure_surface` snapshot: nothing cleared the banners | `restored` state (via `set_parameter_impl`), `preview_live=None` |
| `commit_parameter` refused, Live{c} with refused cell ≠ c, or None | unchanged | no recompile (the unconditional discard is gone) | `restored` = the current state, `preview_live` unchanged |
| `update_source` ok / `load_file` ok / `load_from_source` ok | any → None | `commit_state` | committed state |
| `update_source` compile failure (LiveEdit) | unchanged (Live stays Live) | `record_compile_failure` only; the engine still holds the override, so the state is reported honestly | nothing from the engine path; editor banner as today |
| `export(fmt)` (R2) | Live{c,…,v} → Live{c,…,v}; None → None | the build resets `eval_state`; then, if Live, `edit_check(c, v)` + `commit_check` + cache invalidation, so values and meshes agree again. A re-apply Err moves to None and calls `record_reload_error(msg)` (staleness banner; the next reload heals); the export's own result is unaffected | nothing (`engine_call` publishes nothing; the next publish is consistent) |
| disk echo with identical text (`reload_would_be_a_no_op` true) | unchanged | skipped | nothing |
| `apply_param_to_source` write failure → rollback `update_source(original)` | → None | `commit_state` (the pre-edit text); the fn's own two `failure_surface` restores stay (a different reason: synthesized text never shown) | rolled-back state |

**Rulings inside the table.**
(a) *A watcher echo keeps the preview.* The predicate is about committed source. An external identical save
carries no new committed state, and recompiling mid-drag for no state change is the task-1861 regression
class. The commit's own echo never meets a live preview, because the commit's recompile cleared it. δ states
this in `reload_would_be_a_no_op`'s doc; no code change.
(b) *Discard is `edit_check` back to `committed`*, not a recompile. It is the same incremental path the
preview used, so the discarded state is internally consistent (values and meshes from one `edit_check`) and
costs what a preview frame costs, and it removes the snapshot/restore because no `commit_state` runs. The next
commit or reload reconciles with a cold eval, as every accepted drag frame does today.
(c) *Export re-applies* rather than refuses (R2): one `edit_check` per export while a preview is live.

**Deletions** (each verified at `b77273634a`): the `commit_parameter` comment block that begins "This method
cannot know whether a preview is live" and ends "esc-7281-4 relocated to the error path"; the
`failure_surface` snapshot and restore in `commit_parameter` (the two in `apply_param_to_source` stay);
`discard_parameter_preview` and its key-selection commentary; the `preview_parameter` doc paragraph naming
"the esc-7281-4 defect when it is the ONLY thing a user gesture does", replaced by a pointer to this table (the
test `preview_parameter_leaves_the_ri_file_untouched` keeps its own esc cite); the comment in
`commands::set_parameter_impl` explaining why `restored` is rebuilt (its premise, that the discard clears the
banners, is gone; `restored` stays, now simply "the state after the refusal"; the test
`set_parameter_impl_refusal_restores_a_state_that_still_shows_the_banner` stays green).

**Frontend (δ).** `gui/src/types.ts` gains `preview_live?: PreviewLiveInfo | null` and `bindings.ts` is
regenerated. No UI consumes the field in this stream; its consumers are the debug `engine_state` read (through
the `"preview_live"` key δ adds to `engine_state_json`, named in `docs/debug-mcp-contract.md`'s `engine_state`
description), δ's Rust tests and the visual harness (`gui/test/visual/smoke_slider_write_back_e2e.mjs` may add an
EXPORT-DURING-HOLD phase; not required). The abandoned-preview paths (MechanismPanel unmount mid-drag;
focus or pointer loss; export mid-drag) become visible (`preview_live`) and bounded (at most one, cleared by
the next preview, commit or reload). No discard IPC is added (§11).

### §6.3 Seam 3 — `WriteTarget::admit` (γ, absorbing #8471, with R7 and R8)

```rust
// write_confinement.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteOrigin { User, Assistant }
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteTargetRefusal {
    NoDesignOpen { requested: String },          // Assistant with no design: the temp root alone does not admit
    NotAbsolute  { requested: String },
    Unresolvable { requested: String, reason: String },
    OutsideAdmittedRoots { requested: String, resolved: PathBuf, roots: Vec<PathBuf> },   // replaces OutsideDesignDir
}                                                // DesignChanged is gone (ο deletes it with path_while_open)
/// A path a write may go to. No public constructor: only `admit` and `the_design` produce one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteTarget { requested: String, resolved: PathBuf, origin: WriteOrigin }
impl WriteTarget {
    /// User: pass-through (`resolved = requested`; no filesystem resolution — the user names it).
    /// Assistant: absolute; resolved through symlinks (existing target) or through its existing parent
    /// (new target); must start with one of the admitted roots (R8): the design's canonical directory and
    /// `assistant_temp_root()` (R12).
    pub fn admit(origin: WriteOrigin, requested: &str, design: Option<&Path>) -> Result<WriteTarget, WriteTargetRefusal>;
    /// The open design itself (the host's `save_file` with no `file_path`): always admitted.
    pub fn the_design(design: &Path) -> WriteTarget;
    pub fn path(&self) -> &Path;
    pub fn origin(&self) -> WriteOrigin;
    /// THE write for caller-named targets. `std::fs::write` today; #9174 adds the use-time
    /// O_NOFOLLOW / write-temp-then-`renameat` re-check here (R7). One home for both.
    pub fn write(&self, bytes: &[u8]) -> Result<(), String>;
}
/// The ONE home of the assistant's temp root (R12): the canonical `std::env::temp_dir()`
/// (which follows `TMPDIR`, falling back to `/tmp`). `admit` uses it for R8's temp root, and
/// η exports it to the sidecar as `TMPDIR`, so the GUI's admitted temp root is exactly the
/// temp directory the sidecar's landlocked `claude` child is granted.
pub fn assistant_temp_root() -> PathBuf;
```

The `Display` of `WriteTargetRefusal` states the rule once: "AI write targets must lie inside the open
design's directory or the system temp directory", followed by the variant's facts (`OutsideAdmittedRoots`
names `resolved` and every root).

**Deleted by γ:** `confine_to_design_dir`; `ConfinedTarget` and whatever accessor ξ left on it after ο removed
`path_while_open`; `take_save_path` (its absolute-path check goes; its "must be a string" and `.png` rules move
into `save_target`); the duplicated absolute-path checks. The private `resolve_target` is renamed
`resolve_requested`.

**Takers of a `WriteTarget` (never a raw path):**
- `commands::save_file_impl(target: &WriteTarget, content: &str)`. The Tauri `save_file(path, content)` admits
  `WriteTarget::admit(User, &path, None)`; its wire signature is unchanged.
- γ changes `EngineSession::export(format) -> Result<Vec<u8>, String>` (the gate and the build stay inside; the
  file write moves to the door), and updates every caller.
- `commands::export_impl(engine, format: &str, target: &WriteTarget)`:
  `target.write(&with_engine_lock(engine, |s| s.export(fmt))??)`. The Tauri `export` admits `User`.
- Host (ξ's `TauriToolContext::save_file` / `export`, S25 placement row 6):
  `WriteTarget::admit(Assistant, requested, s.canonical_file_path())` inside the job; a save with no
  `file_path` is `WriteTarget::the_design(active)`; the text comes from `persistable_source()` (α).
- Image tools: `screenshot_save::save_target(params: Value, design: Option<&Path>) ->
  Result<(Option<WriteTarget>, Value), String>` keeps "must be a string" and `.png`, then
  `admit(Assistant, …)`; `save_image_result(result, target: &WriteTarget)`. `debug_server::handle_image_tool`
  reads the design path only when the call carries a `save_path` (a plain screenshot never waits on the
  engine), through the post-ρ capability:
  `state.engine.call(|m| with_engine_lock(m, |s| s.canonical_file_path().map(Path::to_path_buf)))`, an ordered
  read (the harness already waits for idle before screenshots).
- `write_file_atomically` (the design file's own atomic rewrite in `apply_param_to_source`) is not a
  caller-named target and stays.

**Rule recorded in `docs/debug-mcp-contract.md`** (S25 owns the doc; γ lands after ο/ρ's rewrite). Under "AI
write tools", after the `reify_save_file`/`reify_export` confinement paragraph, which γ widens per R8 to "the
open design's directory or the system temp directory": "Image tools' `save_path` takes the same admission
(`WriteTarget::admit(Assistant, …)`): an absolute `.png` path inside the open design's directory or the system
temp directory, i.e. the sidecar's own landlock bound". The `save_path` bullet under "Large reads" gains
"(admitted like an AI write target, see AI write tools)". `SAVE_PATH_DESCRIPTION` (in `debug_server.rs`, or
wherever S25 τ #8955 moved it) says the same.

**Ported from #8471** verbatim into γ's record: the OBSERVED paragraph, FIX OPTIONS, and the note "check
gui/test/visual/* callers of save_path before choosing", with this measurement:
`git grep -n save_path -- gui/test gui/sidecar gui/src scripts tests gui/e2e` is empty at `b77273634a`, so no
in-repo caller constrains the root. γ re-measures at landing.

### §6.4 Seam 4 — DesignWatch (ε, absorbing #8574's watcher half, with R9)

```rust
// design_watch.rs — NEW top-level module. Imports: std and `crate::watcher::FileWatcher` only
// (downward; watcher.rs imports nothing from the crate). Imported by main.rs (constructs it;
// TauriSessionEvents calls it) and commands.rs (the AppState field). INV-ARCH-1: no SCC.
pub struct DesignWatch {
    arm: Box<dyn Fn(&Path) -> Option<FileWatcher> + Send + Sync>,   // main.rs::create_watcher's body
    armed: Mutex<Option<Armed>>,
}
struct Armed { design: PathBuf, watcher: FileWatcher }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reconciled { Unchanged, Rearmed, Failed }
impl DesignWatch {
    pub fn new(arm: impl Fn(&Path) -> Option<FileWatcher> + Send + Sync + 'static) -> Self;
    /// Idempotent. The same path → `Unchanged` (no watcher churn). A different path → build the new
    /// watcher FIRST, then replace (dropping the old one joins its worker; its callback never waits on
    /// the engine, since `EvalQueue::submit` never blocks) → `Rearmed`. Arming failed → slot cleared,
    /// `tracing::warn!`, `Failed` (a watcher on the OLD design is never kept).
    pub fn reconcile(&self, canonical_design: &Path) -> Reconciled;
    /// The design the watcher follows; equals the engine's loaded design after every loading ticket.
    /// The workspace source for #9279 (K7-D3); replaces `AppState.initial_file` (R9).
    pub fn design_path(&self) -> Option<PathBuf>;
}
```

**Wiring.** `DesignWatch` is constructed inside `main.rs`'s `setup()`, where the `AppHandle` the `arm` closure
needs exists, and `AppState` is built and `app.manage()`d there too (precedent: `app.manage(lsp_bridge)`; §3 row
31). `TauriSessionEvents` (λ, main.rs) holds `Arc<DesignWatch>`;
`fn design_loaded(&self, canonical_path: &Path) { self.design_watch.reconcile(canonical_path); }`. This runs
inside `reload()`'s telemetry on the engine lane: constructing a `FileWatcher` is an inotify add plus a thread
spawn, and dropping one is a flag plus a join bounded by the callback's read, submit and emit.
`AppState.watcher: Mutex<Option<FileWatcher>>` becomes `design_watch: Arc<DesignWatch>`. `main.rs::watch_file`
and both of its call sites are deleted; `main.rs::create_watcher` becomes the `arm` closure with its body
unchanged. `AppState.initial_file` and its argv writer are deleted (R9); `claude_send_message` passes
`state.design_watch.design_path().as_deref()` to `claude_bridge::resolve_workspace_dir` (whose parameter name
stays; ε does not edit `claude_bridge.rs`, R4; #9279 rewrites that fn). The watcher's callback still queues
`commands::disk_reload_edit(path)`: a stale event is refused by α's door 2, and ε removes the stale watcher.

**Invariant.** After every ticket that loads a design, `DesignWatch::design_path() ==
EngineSession::canonical_file_path()`. Enforced by construction (one arm site, `reconcile`; one caller,
`design_loaded`; one emitter, `reload()` on a `FilePathUpdate::Set`) and by test: `commands_tests.rs` drives each
funnel body — `open_file_engine_impl` (File-Open), `load_initial_file_impl` (argv), the host's `open_file` via
`delegated_tool_job` (ξ) and the debug open as `QueuedEngine::snapshot` over `load_initial_file_impl` (ρ) —
with a `RecordingSessionEvents` and a `DesignWatch` whose `arm` is a recording closure that builds a real
`FileWatcher` over a tempdir. It asserts one `design_loaded(canonical)` per funnel and `design_path() ==
canonical`; a second load of the same path is `Unchanged`; a switch is `Rearmed`, after which the slot holds
the watcher built for the new path and the old one has been dropped (observable without timing: `Drop` joins the
worker, and the recording closure hands back a watcher whose drop flips a flag); a touch of the NEW file
produces one `disk_reload_edit` ticket. No test asserts the absence of a ticket for the old file over real
inotify (a timing-dependent negative, the #9285 flake class); that assertion waits for #9285's injectable source
(note to #9285, §10).

**#8574 port.** Paragraph (1) (the watcher is never re-targeted on the MCP open) goes into ε's record. Paragraph
(2) (the editor is not told) is ρ #8954's `open_file {path, content, published}` push; nothing is added to ρ.

R4: ε re-arms by replacing the `FileWatcher` instance and edits no `watcher.rs` line. #7704 (read under the lock)
keeps the callback's read where it is; ε changes no read placement.

### §6.5 Seam 5 — `SidecarState::require_ready` (ζ)

```rust
// claude_bridge.rs (after S26 ξ #9076, which deletes MSG_COUNTER in send_message)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidecarUnavailable { NotStarted, Starting, Crashed(String) }
impl fmt::Display for SidecarUnavailable {   // the ONE home of the three refusal texts, bytes as today
    // NotStarted → "sidecar not started"; Starting → "sidecar not ready (still starting)"; Crashed(m) → "sidecar crashed: {m}"
}
impl SidecarState { pub fn require_ready(&self) -> Result<(), SidecarUnavailable>; }   // Ready → Ok
impl SidecarHandle {
    async fn require_ready(&self) -> Result<(), SidecarUnavailable>;   // self.state.lock().await.require_ready()
    pub async fn send_message(&mut self, text, context) -> Result<String, String>;   // require_ready FIRST (use site)
    pub async fn permission_decision(&mut self, d) -> Result<(), String>;            // same
    pub async fn clear_session(&mut self) -> Result<(), String>;                     // same: a "new conversation" while
                                                     // Starting/Crashed is refused honestly, not written into a pipe that may be closed
    pub async fn abort(&mut self) -> Result<(), String>;   // UNCHANGED: the documented idempotent exception (cancelling a
                                                     // dead sidecar is already complete); its two-variant `matches!` is the one
                                                     // legitimate non-`require_ready` read, stated in its doc
    pub async fn wait_ready(&self, timeout) -> Result<(), String>;  // post-notification arm →
                                                     // `self.require_ready().await.map_err(|e| e.to_string())`
}
/// The slot's handle if it is Ready; a `None` slot → `NotStarted` (the slot-level check exists only for None).
fn ready_handle(slot: &mut Option<SidecarHandle>) -> Result<&mut SidecarHandle, SidecarUnavailable>;
pub async fn claude_send_message_impl(..)        { let mut g = sidecar.lock().await; ready_handle(&mut g)?.send_message(text, context).await }
pub async fn claude_permission_decision_impl(..) { .. ready_handle(&mut g)?.permission_decision(d).await }
pub async fn claude_clear_session_impl(..)       { .. ready_handle(&mut g)?.clear_session().await }
pub async fn claude_abort_impl(..)               { None → Err(SidecarUnavailable::NotStarted.to_string()); Some → handle.abort() }
```

`ensure_sidecar_ready`: the Phase 1 and Phase 3 `matches!(.., Ready)` become `require_ready().is_ok()`; the
Phase 5 match becomes `require_ready()` with the existing compare-and-kill on `Err`. The text "sidecar not ready
after notification: Starting" becomes "sidecar not ready (still starting)". **Deleted:** both four-arm matches
(in `claude_send_message_impl` and `claude_permission_decision_impl`), both "The caller is responsible for
ensuring the sidecar is in the Ready state" doc lines, and the two `other =>` "not ready after notification:
{:?}" arms. Every string literal of the three texts outside the `Display` impl is gone (row
`sidecar-state-refusal-literal`, ceiling 2). Redundant enforcement: at the handle (use) and at the slot
(construction of the call). Wire: every `claude_*` command keeps `Result<_, String>`; no bindings change.

### §6.6 INV-GUI-4 row (`docs/invariants.md`; α adds it enforced in the same diff, ω refreshes the owner cell)

The row's form depends on whether S24 ξ #8821 (the Pointer column and `reify-audit --pattern PREGISTRY`) has
landed when α lands (§3 row 12) — the conditional form S25 uses for INV-GUI-2. Either way the pointer is
`item:gui/src-tauri/src/engine.rs::EntryDocument; test:gui/src-tauri/src/tests/commands_tests.rs`, and α's and ω's done-when include `reify-audit --pattern PREGISTRY` green whenever that pattern
exists.

**If #8821 has landed** (6 columns: `| ID | Invariant | Enforcement | Pointer | Status | Owner |`):

```
| INV-GUI-4 | Every source write into a session names that session's entry: `EngineSession::update_source` takes only an `EntryDocument`, which `EngineSession::resolve_target` alone mints from the requested path (entry verbatim or its stem key; anything else is `NotActiveEntry{requested, active}`) and which `update_source` re-checks against the live entry at use — so the editor, FS-watcher and AI-host doors cannot recompile a non-entry file's text as the entry (the #5097 class) | type (`EntryDocument` has no public constructor and is not Clone; `update_source(EntryDocument, &str)` is the sole recompile-from-text entry; the three doors share `commands::update_source_impl`, which resolves and writes under one lock) + test (`commands_tests`: import-tab edit refused with the entry unchanged; stale watcher event refused after a funnel switch; `mcp_context_tests`: host `update_source` on a non-entry path is `InvalidParams`) | item:gui/src-tauri/src/engine.rs::EntryDocument; test:gui/src-tauri/src/tests/commands_tests.rs | enforced(type+test) | gui-engine-admission (α) |
```

**If #8821 has not landed** (today's 5 columns; the pointer sits inline in the Enforcement cell, the S24
INV-DOC-1 precedent, and #8821's migration moves it into the Pointer column):

```
| INV-GUI-4 | Every source write into a session names that session's entry: `EngineSession::update_source` takes only an `EntryDocument`, which `EngineSession::resolve_target` alone mints from the requested path (entry verbatim or its stem key; anything else is `NotActiveEntry{requested, active}`) and which `update_source` re-checks against the live entry at use — so the editor, FS-watcher and AI-host doors cannot recompile a non-entry file's text as the entry (the #5097 class) | type (`EntryDocument` has no public constructor and is not Clone; `update_source(EntryDocument, &str)` is the sole recompile-from-text entry; the three doors share `commands::update_source_impl`, which resolves and writes under one lock) + test (`commands_tests`: import-tab edit refused with the entry unchanged; stale watcher event refused after a funnel switch; `mcp_context_tests`: host `update_source` on a non-entry path is `InvalidParams`); pointer `item:gui/src-tauri/src/engine.rs::EntryDocument; test:gui/src-tauri/src/tests/commands_tests.rs` | enforced(type+test) | gui-engine-admission (α) |
```

### §6.7 Wire changes (each regenerates `gui/src/bindings.ts` under #9070's rule 4a)

| leaf | change | classification and shape |
|---|---|---|
| β | `GuiState.entry_path: Option<String>`, declared `pub entry_path: Option<String>,` — the canonical absolute path of the session's entry; `None` for a fileless session | `full_reload_only("set by the open funnels; the editor reads it from whole-state replies (get_initial_state, open_file_engine, the debug open_file push)")` — the class of `files`, which it qualifies; `#[serde(default, skip_serializing_if = "Option::is_none")]`; populated from `canonical_file_path()` in all three `GuiState` literals (`assemble_gui_state`, the cold-start literal, `build_preview_gui_state`; §3 row 30), so it rides in `PublishedState{generation, state}` and is generation-fenced by `applyPublishedState`; TS `entry_path?: string \| null` |
| δ | `GuiState.preview_live: Option<PreviewLiveInfo>`, declared `pub preview_live: Option<PreviewLiveInfo>,` | `full_reload_only("observability-only via engine_state_json")` (the `demand_prune_measurement` precedent; lead decision, §4); populated in all three `GuiState` literals; `engine_state_json` gains the `"preview_live"` key; TS `preview_live?: PreviewLiveInfo \| null`; no event, no `docs/gui-event-channels.md` row |
| α γ ε ζ η | none | the Tauri `update_source(path, content, order)`, `save_file`, `export` and `claude_*` commands keep their signatures; η adds a sidecar environment variable, not a wire field |

### §6.8 Adoption-ratchet rows

The rows, their EREs, today's counts and ceilings are Appendix A (one home). Each owner leaf adds its rows to
`tests/infra/adoption-ratchet.manifest` at the post-change count and names them in its done criteria.

### §6.9 Seam 6 — the sidecar's temp grant follows the GUI's admitted temp root (η, R12)

```rust
// claude_bridge.rs (after ζ)
pub fn compute_sidecar_env(workspace: &Path, landlock_exec: Option<&Path>, debug_mcp_url: Option<&str>)
    -> Vec<(String, String)>;
// always includes ("TMPDIR", write_confinement::assistant_temp_root()), beside REIFY_WORKSPACE;
// the doc's list of emitted variables gains TMPDIR. The sidecar process and every claude child it
// spawns inherit it, so Rust's temp_dir(), Node's os.tmpdir() and the helper all read one value.
```

- **`gui/sidecar/src/sandbox.ts::wrapClaudeArgs`** grants `os.tmpdir()` instead of the `'/tmp'` literal
  (`'--writable', os.tmpdir(),`). Its doc comment and the `SessionConfig.landlockExec` doc comment in
  `gui/sidecar/src/session.ts` that restates the argument list say `--writable <os.tmpdir()>`.
- **`gui/src-tauri/sandbox/landlock_exec.py`** (vendored): the built-in temp grant reads
  `os.environ.get('TMPDIR') or '/tmp'` (Rust's rule: `TMPDIR`, else `/tmp`) instead of the `'/tmp'` literal,
  keeping `fs_writable_all`; the module docstring's "``/tmp``" names the temp root the same way. This is a
  targeted patch (CLAUDE.md "Vendored sandbox helpers"): the vendor header gains a second REIFY DIVERGENCE
  paragraph ("the temp grant follows `TMPDIR`, which reify's GUI exports as its admitted temp root; upstream
  grants the `/tmp` literal"), and its first DIVERGENCE paragraph's list of the variables `compute_sidecar_env`
  emits gains `TMPDIR`. The `VENDORED_FROM` SHA is unchanged (no upstream hunk is ported).
- **Invariant.** The assistant's admitted temp root (γ) equals the temp directory its landlocked `claude` child
  may write: one producer (`assistant_temp_root()`), one transport (`TMPDIR` in `compute_sidecar_env`), and
  two readers that read only `TMPDIR` for the grant (`wrapClaudeArgs`, `landlock_exec.py`). Enforced by
  construction and by the tests in η's leaf detail.
- `tests/infra/test_landlock_exec_refer.sh` mints its fixture under `${TMPDIR:-/tmp}` and already excludes
  `$TMPDIR` from assertion (a)'s root; η re-reads its trap #2/#3 header, and it stays green with `TMPDIR` unset
  and with `TMPDIR` set to a directory outside `/tmp`.

## §7 Boundary tests (two-way)

| id | scenario | preconditions | postconditions | leaf | side |
|---|---|---|---|---|---|
| BT1 | Editor door: `update_source_impl(engine, helper.ri, text)` on a two-file design | `main.ri` imports `helper.ri`, loaded via `load_file` | `Err(SourceUpdateError::NotActiveEntry(NotTheEntry{requested: helper, active: main}))`; `Helper.x` still 10mm; `files[0].content` unchanged; `is_stale()==false`; no publish | α | consumer (door) |
| BT1' | The same call naming `main.ri`, and naming the stem key `"main.ri"` | as BT1 | `Ok`, recompiled | α | producer |
| BT2 | Stale watcher event after a funnel switch | load A then B (`load_initial_file_impl`); `disk_reload_edit(A)` runs | reply `Ok(())`; nothing published; a `tracing` warn names A and B; the engine holds B's source | α | consumer (watcher door) |
| BT2' | Watcher event through a non-canonical spelling of the entry | design loaded canonically; `disk_reload_edit(<symlinked dir>/main.ri)` with changed text | refused raw, admitted by the canonical fallback; `Ok`, recompiled | α | producer |
| BT3 | Host `update_source` on a non-entry path | ξ host over `ManualQueue`; a design with an import | `ToolError::InvalidParams(<NotActiveEntry text>)`; no state produced | α | consumer (host door) |
| BT3' | Host `update_source` with a `..` spelling of the ABSOLUTE entry path | as BT3 | refused raw, admitted by the canonical fallback; `Ok` | α | producer |
| BT3'' | Host `update_source` with the stem key while the cwd holds an unrelated file of that name | as BT3; the test process's cwd is a tempdir containing an unrelated `main.ri`; `file_path: "main.ri"` | admitted by the raw spelling (the stem key) before any canonicalisation; `Ok`, the entry recompiled | α | producer |
| BT4 | `persistable_source` after a rejected compile | `update_source` with broken text (LiveEdit failure) | `Err(RejectedBuffer{file_key, diagnostics ≥ 1})`; the host `save_file` → `EngineError` naming it, nothing written; after a compiling update → `Ok(text)` | α | producer + consumer (host save) |
| BT5 | `EntryDocument` cannot be forged | the type has no public constructor (asserted by review; no `trybuild`) | — | α | type |
| BT5' | A document is re-checked at use | mint for `main.ri`; `load_file(other.ri)` in the same session; `update_source(doc, text)` | `Err(SourceUpdateError::NotActiveEntry(Superseded{minted_for: main, active: Some(other)}))`; the engine holds `other.ri`; `is_stale()==false` (no reload error recorded) | α | producer |
| BT6 | The preview survives export; values agree with meshes | preview `Bracket.width=120mm`; `export(Stl)` | `preview_live==Some{width,120mm}`; `build_gui_state().values[width]==120mm`; the mesh for `Bracket` equals the mesh of a fresh session that only previewed 120mm (same kernel) | δ | producer |
| BT7 | A refused commit on an unrelated cell keeps the live preview | preview cell A; `commit_parameter(B, "garbage")` | `Err`; `preview_live==Some{A}`; A's override still in values; no recompile (`RecordingSessionEvents` shows no `solve_started`) | δ | producer |
| BT7' | A refused commit on the live cell discards it without touching banners | `last_reload_error` set (a prior hot-reload failure); preview A; `commit_parameter(A, "garbage")` | `preview_live==None`; A back to committed; `is_stale()` still true | δ | producer |
| BT8 | A second cell's preview displaces the first | preview A, then preview B | `preview_live==Some{B}`; A's value committed again | δ | producer |
| BT9 | An identical-text disk echo keeps the preview | preview A; `reload_for_watch_if_changed_impl(entry, same_text)` | `Ok(None)`; `preview_live==Some{A}` | δ | consumer (watcher) |
| BT10 | `preview_live` reaches the debug read | preview cell A, then commit | `engine_state_json(session)["preview_live"]` is `{cell_id: A, value}` during the preview and `null` after the commit | δ | consumer (debug MCP) |
| BT11 | An Assistant `save_path` outside the admitted roots is refused | design open in `/d`; `save_target({save_path: "/home/x/a.png"}, Some(design))` | `Err(OutsideAdmittedRoots{roots: [/d, temp]})`; nothing written | γ | producer |
| BT11' | An Assistant `save_path` inside the design dir or the temp dir is admitted; `.png` still enforced | as above with `/d/shot.png`, `<temp>/shot.png`, `/d/shot.jpg` | the first two `Ok(WriteTarget)`, PNG written via `write`; the third refused by the `.png` rule | γ | producer |
| BT12 | Host `export` outside the roots refused; inside admitted | ξ host; `export("stl", "/etc/x.stl")` vs `<design_dir>/x.stl` and `<temp>/x.stl` | the refusal names the resolved path and the roots; the admitted ones write the bytes `EngineSession::export(fmt)` returned | γ | consumer (host) |
| BT12' | Host save-as with a `..` escape | ξ host; `save_file("<design_dir>/../x.ri")` | resolved outside both roots → `OutsideAdmittedRoots`; nothing written | γ | consumer (host) |
| BT13 | User pass-through | Tauri `save_file("/anywhere/a.ri", …)` → `admit(User, …, None)` | `Ok`, written (no roots consulted) | γ | consumer (Tauri door) |
| BT14 | Every funnel leaves the watcher on the engine's canonical path: File-Open | `open_file_engine_impl` | `design_loaded(canonical)` recorded once; `design_path()==canonical_file_path()` | ε | consumer |
| BT14' | argv launch | `load_initial_file_impl(canonical)` | the same | ε | consumer |
| BT14'' | host `open_file` (ξ) | `delegated_tool_job("reify_open_file")` | the same | ε | consumer |
| BT14''' | debug `open_file` / `load_fixture` (ρ) | `QueuedEngine::snapshot` over the open funnel | the same | ε | consumer |
| BT15 | Re-arm only on change, by construction | `arm` is a recording closure over real `FileWatcher`s in a tempdir; load A, load A, load B; touch B | `Unchanged` then `Rearmed`; the slot holds the watcher built for B and A's watcher was dropped (its drop flag is set; `Drop` joins the worker, so no timing); one `disk_reload_edit(B)` ticket. No absence-of-ticket assertion for A (the #9285 flake class; note to #9285) | ε | producer |
| BT15' | An arming failure clears the slot | `arm` returns `None` | `Failed`; `design_path()==None`; a warn logged | ε | producer |
| BT15'' | The chat workspace follows the design | argv-free start; File-Open `/d/main.ri`; `claude_send_message` with no `current_file` | `resolve_workspace_dir` receives `Some(/d/main.ri)` from `design_path()` | ε | consumer (#9279's input) |
| BT16 | `require_ready` texts through the handle | handle in NotStarted / Starting / Crashed("boom") / Ready | `send_message`, `permission_decision`, `clear_session` → `Err("sidecar not started")`, `Err("sidecar not ready (still starting)")`, `Err("sidecar crashed: boom")`, `Ok` | ζ | producer |
| BT16' | `*_impl` with an empty slot; `abort` tolerance unchanged | slot `None`; a Crashed handle | `Err("sidecar not started")` from every `*_impl` including `claude_abort_impl`; `abort` on the Crashed handle → `Ok(())` | ζ | consumer |
| BT17 | FileTabs badge and the editor's silence | `entry_path=/d/main.ri`; tabs `/d/main.ri`, `/d/helper.ri` | the helper tab shows `not-live-indicator` with its tooltip; typing in helper sends no `update_source`; `handleReEvaluate` on helper shows the "not live" info toast instead of invoking; typing in main sends as today | β | consumer (frontend) |
| BT18 | The sidecar receives the admitted temp root | `compute_sidecar_env(ws, None, None)` | the list contains `("TMPDIR", write_confinement::assistant_temp_root())` | η | producer |
| BT18' | The sidecar grants `os.tmpdir()` (NOT gated: `gui/sidecar`'s vitest suite is type-checked but not run by the merge gate, §3 row 27) | `TMPDIR` set to a non-`/tmp` directory D (vitest, `gui/sidecar/src/__tests__/sandbox.test.ts`) | `wrapClaudeArgs(args, ws, exec).args` contains `'--writable', D` and no `'/tmp'` grant | η | consumer (sidecar) |
| BT18'' | The helper grants `TMPDIR` (mandatory gated infra member) | `tests/infra/test_landlock_exec_tmpdir_grant.py` runs `landlock_exec.py` with `TMPDIR=D`, D outside `/tmp`; skips only where the kernel lacks Landlock, as `test_landlock_exec_refer.sh` does | a write under D succeeds; a write under `/tmp` is refused; with `TMPDIR` unset, a write under `/tmp` succeeds | η | consumer (helper) |
| BT17' | `entry_path` on the wire, generation-fenced | an `open_file_engine` reply | `PublishedState.state.entry_path == canonical`; an older generation's reply does not overwrite it | β | producer |

## §8 Pre-conditions for activating

Every leaf depends on S25 ζ #8942 (one `reload()`), λ #8947 (`SessionEvents`, `design_loaded`) and ξ #8950
(`TauriToolContext<'e>`, `delegated_tool_job`, the named guard function). Per-leaf extras:

| leaf | extra upstream edges | why |
|---|---|---|
| α | #8951 | ο deletes the debug `update_source`/`save_file` originals and the debug guard; without it α's signature change and the `holds_rejected_source` deletion would not compile against `debug_server.rs`, which α must not edit |
| β | α, #9070, #8943 | β publishes the entry α enforces; a wire field regenerates `bindings.ts` (rule 4a); after S25 η the three `GuiState` literals are `assemble_gui_state`, the cold-start literal and `build_preview_gui_state` (§3 row 30) |
| γ | #8951, #8954 | γ edits `debug_server::handle_image_tool`, and ο deletes `confined_write_target`/`DesignChanged`; γ reads the design path through `QueuedEngine`, since before ρ the only road is `run_on_engine`, which ρ's `debug-engine-handle` row forbids |
| δ | #8943, #9070 | `preview_live` joins `assemble_gui_state`'s inputs; a `GuiState` field is a wire change, so `bindings.ts` regenerates (rule 4a); no μ #8948 edge (`full_reload_only`, lead decision, §4) |
| ε | #8954 | after ρ the debug open funnel is a snapshot job over `load_initial_file_impl` with no watcher, so it reaches `design_loaded` |
| ζ | #9076 | R4: both edit `SidecarHandle::send_message`; the closure leaf lands first |
| η | γ, ζ | `assistant_temp_root()` is γ's; R4's `claude_bridge.rs` chain #9076 → ζ → η → #9279 |
| ω | α β γ δ ε ζ η | close; a cancelled sibling counts as satisfied |

**G1.** No leaf of this stream becomes a prerequisite of a task in #8612's dependency closure, and no seam here
needs a closure leaf to wait on this stream. The upstream ids above are all in the closure, which is allowed:
this stream follows them. The three tasks that gain an edge onto a leaf here (#9171, #9174, #9279; #9279 on
ε, ζ and η) are all outside the closure (§13 G1 table).

**Gate.** α carries fk-efe244a6697a (high, hotspot-survey-reify-20261015), so #9171 (the hotspot-survey
trigger gate) depends on α. #9171 is never committed or re-statused by this stream. No leaf carries a high
review key, so #9242 gains nothing.

## §9 Cross-PRD relationship and seam owners (G4)

| artefact | owner | this stream's relation |
|---|---|---|
| `engine.rs` session core, `reload()` (S25 ζ #8942), `assemble_gui_state` (S25 η #8943) | S25 | α, β, δ add methods and fields after them and never edit those leaves' scope; `discard_live_preview` and export's re-apply use `edit_check`, not a third `check_with_solve_slot` (S25 ζ's `gui-reload-pipeline` ceiling 2 holds) |
| `SessionEvents` incl. `design_loaded` (λ #8947) | S25 | ε consumes it; no new event |
| `TauriToolContext<'e>`, `delegated_tool_job`, the named guard fn (ξ #8950) | S25 | α re-homes the guard onto `resolve_target` and deletes it; γ turns `confine_to_design_dir` into `WriteTarget::admit(Assistant, …)`; α's `persistable_source` replaces the host interlock |
| debug-server delegation and deleted originals (ο #8951); `QueuedEngine` and the open funnel (π/ρ #8953/#8954) | S25 | α, γ, ε follow; γ edits `handle_image_tool` only; the debug funnel reaches `design_loaded` through `load_initial_file_impl` |
| `gui/src/bindings.ts` + ipc (ι #9070, rule 4a) | S26 | β and δ regenerate in the same diff; S26 owns `generation` |
| `GuiState.entry_path`, `GuiState.preview_live` | **this stream** (β, δ) | new wire fields; INV-GUI-1 classification `full_reload_only` for both |
| `docs/debug-mcp-contract.md` "AI write tools", the `save_path` bullet; the sidecar allowlist | S25 | γ adds the `save_path` admission paragraph and the R8 widening after ο/ρ's rewrite; the allowlist is unchanged |
| `write_confinement.rs` | **this stream** (γ) | `WriteTarget::admit`; #9174 adds the use-time check inside `WriteTarget::write` |
| `claude_bridge.rs` `SidecarState` table | **this stream** (ζ) | order below |
| `claude_bridge::compute_sidecar_env` (the sidecar environment) | this stream (η, `TMPDIR`) / #9279 (the workspace key) | η lands first (#9279 ← η); #9279 keeps the `TMPDIR` entry |
| `gui/sidecar/src/sandbox.ts::wrapClaudeArgs` | S26 (the sidecar) | η replaces the `'/tmp'` grant with `os.tmpdir()`; no other change |
| `gui/src-tauri/sandbox/landlock_exec.py` | vendored from dark-factory (targeted-patch rule) | η adds a second REIFY DIVERGENCE (the temp grant reads `TMPDIR`); dark-factory is not changed |
| `watcher.rs` | #9285 (K7-D10 (4)) | ε never edits it (R4) |
| `design_watch.rs` | **this stream** (ε) | new; #9279 consumes `design_path()` |
| INV-GUI-2 row | S25 ρ #8954 | untouched (R3) |
| INV-GUI-4 row | **this stream** (α; ω refreshes) | new row, §6.6 |
| `FileTabs.tsx`, `Editor.tsx`, `App.tsx` (`handleReEvaluate`) | S26 (frontend port) / this stream (β) | β's edits are component-local; S26 β–ε have landed in `App.tsx`; file lock with S26 ν/ξ/ο |
| `MechanismPanel.tsx` | S26; not edited | δ changes no frontend choreography; the abandoned-preview residue is recorded with its trigger (§11) |
| `types.rs` as the bindings source; `gui_state_schema.rs` | S26 (shared) / μ₂ #9278 | β and δ add fields only and no macro arm; file lock with #9278 |

**This stream is the G4 owner of:** the `claude_bridge.rs` `SidecarState` table (and `require_ready`),
`write_confinement.rs` (including `assistant_temp_root()`, the one home of the assistant's temp root),
`design_watch.rs`, the INV-GUI-4 row, and the two new wire fields `GuiState.entry_path` and
`GuiState.preview_live`.

**File order on contended files (R4):**
- `claude_bridge.rs`: #9076 → ζ → η → #9279 (real edges). #9284 (`InboundMessage::ToolResult`, `McpConfig`) is
  file-lock only: it touches disjoint constructs and has no edge in either direction.
- `gui/sidecar/src/session.ts`: η edits one doc comment; #9279 and #9284 also edit the file; file locks only
  (η precedes #9279 by its edge).
- `watcher.rs`: ε never edits it; #9285 is file-lock only.
- `engine.rs` among α, β, δ, γ: file locks serialise them. β follows α by a real edge. γ and δ have no edge
  between them: γ alone changes `EngineSession::export`'s signature, and δ's re-apply sits inside `export`
  whatever the signature is (lead decision, §4).
- `main.rs` / `tauri_commands.rs`: if #9070 has moved the Tauri commands before γ or ε lands, γ's `save_file`/
  `export` edits and ε's `open_file_engine`/`claude_send_message` edits land in `tauri_commands.rs`; the
  architect re-acquires. No edge is added onto #9070 for γ or ε.

## §10 Decomposition plan

Common to every leaf: `prd_path docs/prds/v0_6/gui-engine-admission.md`; `metadata.source "hotspot-survey"`;
`x_finding_run "hotspot-survey-reify-20261015"`; `grammar_confirmed true` (no `.ri` syntax); `human_decomposed
true`; `task_kind "normal"`; dependencies on #8942, #8947 and #8950; `python3 scripts/assert-module-scc.py
check` green with no baseline row changed; ratchet rows added at the post-change count and named in the done
criteria (Appendix A). Priority is the strongest key's severity, except β (R10).

| label | id | title | prio | depends on | user-observable signal |
|---|---|---|---|---|---|
| **α** | #9387 | EntryDocument: `resolve_target` admits every source write; `persistable_source` | **high**, gate #9171 | #8942, #8947, #8950, #8951 | an import tab's edit is refused with a message naming both files and the viewport keeps the entry's design |
| **β** | #9388 | Published state names the entry; FileTabs badges non-entry tabs "not live" | medium | α, #9070, #8943, #8942, #8947, #8950 | a non-entry tab carries a "not live" badge and typing there triggers nothing |
| **γ** | #9389 | `WriteTarget::admit`: one admission for every caller-named write (absorbs #8471) | low | #8942, #8947, #8950, #8951, #8954 | `screenshot {save_path}` outside the design dir and temp dir is refused naming the roots |
| **δ** | #9390 | PreviewState: one transition table for the live preview | medium | #8942, #8943, #8947, #8950, #9070 | export mid-drag leaves values and meshes in agreement and the drag continues |
| **ε** | #9391 | DesignWatch: the watcher follows `design_loaded` (absorbs #8574's watcher half) | medium | #8942, #8947, #8950, #8954 | after a debug `open_file`, the new file hot-reloads and the old one does not |
| **ζ** | #9392 | `SidecarState::require_ready` owns the not-ready refusal | low | #8942, #8947, #8950, #9076 | a chat send while the sidecar is starting says "sidecar not ready (still starting)" |
| **η** | #9393 | Sidecar temp grant follows the GUI's admitted temp root | low | #8942, #8947, #8950, γ, ζ | with a non-`/tmp` `TMPDIR`, the chat assistant can write a scratch file in `$TMPDIR` |
| **ω** | #9394 | PRD close: SHIPPED header, landed ids, AS-AUTHORED freeze | low | α, β, γ, δ, ε, ζ, η | the committed header |

**Downstream edges onto other tasks:** `#9171 ← α` (survey gate; high); `#9174 ← γ` (R7); `#9279 ← ε`;
`#9279 ← ζ` (R4); `#9279 ← η` (R12). All four dependents are outside #8612's closure.

### Leaf detail

**α — EntryDocument** (`x_finding_key [fk-efe244a6697a, fk-0ea9b4872587]`; `x_gapfill_ruling
"plans/gap-fill-deliberation-reify-20261016.md §K7-D1"`; high; gate #9171).
- **Files:** `gui/src-tauri/src/engine.rs`, `gui/src-tauri/src/commands.rs`, `gui/src-tauri/src/mcp_context.rs`,
  `gui/src-tauri/src/tests/engine_tests.rs`, `gui/src-tauri/src/tests/commands_tests.rs`,
  `gui/src-tauri/src/tests/mcp_context_tests.rs`, `docs/invariants.md`, `tests/infra/adoption-ratchet.manifest`.
- **Signal (G2):** in the GUI, after go-to-definition into an imported module's tab, typing there no longer
  replaces the viewport with the import compiled as the entry; the backend refuses with a message naming both
  files (visible through Re-evaluate's toast). Over the debug MCP, `reify_update_source {file_path: <import>}`
  is refused as `invalid parameters: <import> is not the entry …`. BT1–BT5' (with BT2', BT3'') run in CI over the real engine and a
  real two-file fixture.
- **Consumer:** the editor (`update_source` command), the FS watcher (`disk_reload_edit`), the AI host
  (`TauriToolContext::update_source`/`save_file`); β.
- **Done when:** §6.1 is implemented (`resolve_target`, `EntryDocument`, `NotActiveEntry`, `RejectedBuffer`,
  `persistable_source`, `update_source(EntryDocument, &str) -> Result<GuiState, SourceUpdateError>` (R11), the
  three doors and their raw-then-canonical spelling fallback); `engine::source_key_matches_path` loses its
  `#[cfg(feature = "gui")]` gate and the comment explaining it (§3 row 29); ξ's named guard fn and its tests are
  deleted; `holds_rejected_source` is deleted; the INV-GUI-4 row (§6.6, in the form that matches #8821's state)
  is added as `enforced(type+test)` and `reify-audit --pattern PREGISTRY` is green if that pattern exists; ratchet
  rows `mcp-context-raw-engine-lock` (ceiling: the named `produced`-slot residue, Appendix A),
  `update-source-string-path`, `rejected-source-convention` are added; the BT1–BT5' rows are green.
- **Invariant established:** INV-GUI-4, enforced by type (sole minter, no public constructor, not Clone) and test.
- **Delivered checks:** `resolve-target-present` (grep `fn resolve_target\(` present in `engine.rs`);
  `update-source-path-param-gone` (grep `pub fn update_source\(&mut self, path: &str` absent in `engine.rs`);
  `rejected-buffer-convention-gone` (grep `fn holds_rejected_source` absent in `engine.rs`);
  `source-update-error-one-home` (grep `enum SourceUpdateError` present in `engine.rs`).

**β — entry marker** (`x_finding_part [fk-efe244a6697a]`; `x_gapfill_ruling "… §K7-D1"`; medium per R10; no
gate edge).
- **Files:** `gui/src-tauri/src/types.rs`, `gui/src-tauri/src/engine.rs`, `gui/src/bindings.ts`,
  `gui/src/types.ts`, `gui/src/stores/engineStore.ts`, `gui/src/editor/FileTabs.tsx`,
  `gui/src/editor/FileTabs.module.css`, `gui/src/editor/Editor.tsx`, `gui/src/App.tsx`,
  `gui/src/__tests__/FileTabs.test.tsx`, `gui/src/__tests__/Editor.test.tsx`,
  `gui/src-tauri/src/tests/engine_tests.rs`.
- **Design:** `GuiState.entry_path` (§6.7). `engineStore` exposes `entryPath()`. `FileTabs` takes
  `entryPath: () => string | null` and, when it is non-null, renders `data-testid="not-live-indicator"` with
  the tooltip "`<path>` — not live: only the entry (`<entry>`) is recompiled; edits here are saved but not
  evaluated" on every tab whose `canonicalizeKey(path) !== canonicalizeKey(entryPath)`. `Editor.tsx`'s
  `updateListener` and `App.tsx::handleReEvaluate` skip `updateSource` for a non-entry active file;
  Re-evaluate shows an info toast instead. Dirty tracking, save and LSP `didChange` continue for non-entry
  tabs (they are real files).
- **Signal:** open a design with an import and go-to-definition into the import: the tab carries the "not live"
  badge, typing there triggers no re-evaluation and no error toast, and Re-evaluate says why. vitest BT17 and
  Rust BT17'.
- **Consumer:** the user at the tab bar; α's doors (a stray call is still refused).
- **Done when:** the field and its classification exist, populated in all three `GuiState` literals (§3 row 30);
  `bindings.ts` is regenerated
  (`REIFY_UPDATE_BINDINGS=1 cargo test -p reify-gui --features gui bindings_ts_is_current`); the badge and the
  two skips exist; BT17 and BT17' are green (`scripts/gui-test.sh` for the frontend checkpoint).
- **Delivered checks:** `entry-path-wire-field` (grep `pub entry_path: Option<String>` present in `types.rs`);
  `not-live-badge` (grep `not-live-indicator` present in `gui/src/editor/FileTabs.tsx`).

**γ — WriteTarget** (`x_finding_key [fk-9f1803be198c]`; `x_gapfill_ruling "… §Uncontested block → K7-D8"`;
low; absorbs #8471).
- **Files:** `gui/src-tauri/src/write_confinement.rs`, `gui/src-tauri/src/screenshot_save.rs`,
  `gui/src-tauri/src/commands.rs`, `gui/src-tauri/src/mcp_context.rs`, `gui/src-tauri/src/engine.rs`,
  `gui/src-tauri/src/debug_server.rs`, `gui/src-tauri/src/main.rs`,
  `gui/src-tauri/src/tests/write_confinement_tests.rs`, `gui/src-tauri/src/tests/screenshot_save_tests.rs`,
  `gui/src-tauri/src/tests/commands_tests.rs`, `gui/src-tauri/src/tests/mcp_context_tests.rs`,
  `gui/src-tauri/src/tests/engine_tests.rs`, `docs/debug-mcp-contract.md`,
  `tests/infra/adoption-ratchet.manifest` (`tauri_commands.rs` instead of `main.rs` if #9070 moved the
  commands).
- **Signal:** over the debug MCP in a `REIFY_DEBUG=1` launch, `screenshot {save_path: "/home/<u>/x.png"}` is
  refused naming the admitted roots, while `<design_dir>/x.png` and `/tmp/x.png` are written;
  `reify_export {output_path: "/tmp/part.stl"}` is admitted; File → Save and Export are unchanged. BT11–BT13 in
  CI.
- **Consumer:** the image tools (`handle_image_tool`), the host `save_file`/`export`, the Tauri `save_file`/
  `export`; #9174; η (`assistant_temp_root()`).
- **Done when:** §6.3 is implemented, including `assistant_temp_root()` as the only source of the temp root;
  `EngineSession::export(format) -> Result<Vec<u8>, String>` with every caller updated (γ alone makes this
  change); `handle_image_tool` reads the design path only when `save_path` is present; `ConfinedTarget`, `confine_to_design_dir` and `take_save_path` are deleted; the
  contract-doc paragraph and the R8 widening are written; ratchet rows `raw-write-target-admission` and
  `door-raw-fs-write` are added; #8471's content is ported, then #8471 is cancelled with
  `x_subsumed_by = γ`.
- **Delivered checks:** `write-target-admit-present` (grep `pub fn admit\(` present in
  `write_confinement.rs`); `design-dir-confinement-fn-gone` (grep `fn confine_to_design_dir` absent in
  `write_confinement.rs`); `save-path-shape-check-gone` (grep `fn take_save_path` absent in
  `screenshot_save.rs`); `temp-root-one-home` (grep `fn assistant_temp_root\(` present in
  `write_confinement.rs`).

**δ — PreviewState** (`x_finding_key [fk-e9fa33e06157]`; `x_gapfill_ruling "… §Uncontested block → K7-D5"`;
medium).
- **Files:** `gui/src-tauri/src/engine.rs`, `gui/src-tauri/src/types.rs`, `gui/src-tauri/src/commands.rs`,
  `gui/src-tauri/src/engine_state_view.rs`, `gui/src/bindings.ts`, `gui/src/types.ts`,
  `gui/src-tauri/src/tests/engine_tests.rs`, `gui/src-tauri/src/tests/commands_tests.rs`,
  `docs/debug-mcp-contract.md`, `tests/infra/adoption-ratchet.manifest`. (#9278 edits `gui_state_schema.rs`/`types.rs`: file lock only; δ
  adds no macro arm.)
- **Signal:** in the GUI, drag a mechanism slider, export mid-drag (File → Export) and keep dragging: the
  viewport geometry and the property panel agree (no snap to the committed value). A refused edit-box commit on
  another parameter does not reset the slider. Over the debug MCP, `engine_state` shows
  `preview_live {cell_id, value}` during a drag and `null` after release. BT6–BT10 in CI over the real eval
  engine.
- **Consumer:** `commit_parameter`, `export`, the `engine_state` read; MechanismPanel reads the restored state
  as today.
- **Done when:** `PreviewState`, `LivePreview`, `discard_live_preview`, export's re-apply (inside `export`, whatever
  its signature), the `full_reload_only` wire field populated in all three `GuiState` literals, and bindings
  exist; `engine_state_json` carries `"preview_live"`, `engine_state_view` accepts `fields: ["preview_live"]` (its
  valid names come from the payload; it is edited only if it keeps a hard-coded list), and
  `docs/debug-mcp-contract.md`'s `engine_state` description names the key; no event and no
  `docs/gui-event-channels.md` row are added; the §6.2 deletions are made; ratchet row `preview-failure-surface-snapshot` is added; the
  invariant is named with its enforcement (private field, four writers, tests).
- **Delivered checks:** `preview-state-present` (grep `enum PreviewState` present in `gui/src-tauri/src`);
  `preview-liveness-guess-gone` (grep `cannot know whether a preview is live` absent in `engine.rs`);
  `preview-live-wire-field` (grep `pub preview_live:` present in `types.rs`); `engine-state-preview-key` (grep
  `"preview_live":` present in `commands.rs`).

**ε — DesignWatch** (`x_finding_key [fk-33bb6efd23ac]`; `x_gapfill_ruling "… §K7-D2"`; medium; absorbs #8574's
watcher half).
- **Files:** `gui/src-tauri/src/design_watch.rs` (new), `gui/src-tauri/src/lib.rs`, `gui/src-tauri/src/main.rs`,
  `gui/src-tauri/src/commands.rs`, `gui/src-tauri/src/tests/design_watch_tests.rs` (new),
  `gui/src-tauri/src/tests/mod.rs`, `gui/src-tauri/src/tests/commands_tests.rs`,
  `gui/src-tauri/src/tests/main_helpers_tests.rs`, `gui/src-tauri/src/tests/claude_bridge_tests.rs`,
  `tests/infra/adoption-ratchet.manifest`. Not `watcher.rs` and not `claude_bridge.rs` (R4).
  (`tauri_commands.rs` instead of `main.rs` for moved commands, if #9070 landed first.)
- **Signal:** `open_file` over the debug MCP, then edit the newly opened `.ri` externally: it hot-reloads; edit
  the previously open file: nothing happens. File-Open and the sidecar's `reify_open_file` behave the same.
  BT14–BT15'' in CI with real `FileWatcher`s over a tempdir (BT15 asserts the re-arm by construction, with no
  absence-of-event timing).
- **Consumer:** every open funnel (through `reload()`), the watcher callback, `claude_send_message` now and
  #9279 later (`design_path()`).
- **Done when:** the module with `reconcile`/`design_path` exists; `TauriSessionEvents::design_loaded` calls
  `reconcile`; `watch_file` and both calls are deleted; `AppState.watcher` becomes `design_watch`;
  `AppState.initial_file` and its argv writer are deleted and `claude_send_message` reads `design_path()`
  (R9); ratchet rows `watch-file-call` and `app-state-initial-file` are added; the invariant is stated with its
  enforcement; #8574 is ported, then cancelled with `x_subsumed_by = [ε, 8954]`.
- **Delivered checks:** `design-watch-module` (path `gui/src-tauri/src/design_watch.rs` present);
  `reconcile-present` (grep `pub fn reconcile\(` present in `design_watch.rs`); `watch-file-gone` (grep
  `watch_file\(` absent in `main.rs` and `tauri_commands.rs`).

**ζ — require_ready** (`x_finding_key [fk-2513539f4913]`; `x_gapfill_ruling "… §Uncontested block → K7-D10
(1)"`; low).
- **Files:** `gui/src-tauri/src/claude_bridge.rs`, `gui/src-tauri/src/tests/claude_bridge_tests.rs`,
  `tests/infra/adoption-ratchet.manifest`.
- **Signal:** in the GUI chat, sending a message while the sidecar is still starting shows "sidecar not ready
  (still starting)"; after a crash, "sidecar crashed: …"; a permission decision or "new conversation" in the
  same states shows the same text. BT16/BT16' in CI through the handle.
- **Consumer:** the `claude_send_message`, `claude_permission_decision`, `claude_clear_session` and
  `claude_abort` commands; #9279 (keeps the table's shape).
- **Done when:** §6.5 is implemented (`SidecarUnavailable`, `SidecarState::require_ready`, the handle-level
  checks, `ready_handle`); both matches and both notes are deleted; the `wait_ready`/`ensure_sidecar_ready` arms
  are folded; ratchet row `sidecar-state-refusal-literal` is added.
- **Delivered checks:** `require-ready-present` (grep `fn require_ready\(` present in `claude_bridge.rs`);
  `caller-duty-note-gone` (grep `caller is responsible for ensuring the sidecar` absent in
  `claude_bridge.rs`); `duplicated-ready-match-gone` (grep `SidecarState::Starting => Err\("sidecar not ready` absent in
  `claude_bridge.rs`; the literal moves into the `Display` impl, so `require_ready`'s own natural
  `SidecarState::Starting => Err(SidecarUnavailable::Starting)` arm does not match).

**η — sidecar temp grant** (`x_finding_part [fk-9f1803be198c]`; `x_finding_run
"hotspot-survey-reify-20261015"`; `source "hotspot-survey"`; `x_gapfill_ruling "… §Uncontested block → K7-D8"`;
session ruling R12; low).
- **Files:** `gui/src-tauri/src/claude_bridge.rs`, `gui/src-tauri/src/tests/claude_bridge_tests.rs`,
  `gui/sidecar/src/sandbox.ts`, `gui/sidecar/src/session.ts`, `gui/sidecar/src/__tests__/sandbox.test.ts`,
  `gui/src-tauri/sandbox/landlock_exec.py`, `tests/infra/test_landlock_exec_tmpdir_grant.py` (new),
  `tests/infra/test_landlock_exec_tmpdir_grant.sh` (new), `tests/infra/run-all-classification.manifest`.
- **Design:** §6.9.
- **Signal:** with `TMPDIR` set to a non-`/tmp` directory when the GUI is launched, the chat assistant can write
  a scratch file in `$TMPDIR` (today the landlocked child is refused there); an image-tool `save_path` under that
  `$TMPDIR` is admitted by γ, and one under `/tmp` is refused when `TMPDIR` differs. Gated coverage: BT18
  (`cargo test -p reify-gui`), BT18'' (the new infra member) and the delivered checks. BT18' runs under
  `npm test` in `gui/sidecar` and is NOT gated: the merge gate type-checks that suite but does not run it (§3
  row 27).
- **Consumer:** the sidecar process and every `claude --print` child it spawns (`wrapClaudeArgs`,
  `landlock_exec.py`); #9279 (keeps the `TMPDIR` entry when it re-keys the workspace).
- **Done when:** `compute_sidecar_env` exports `("TMPDIR", assistant_temp_root())` and its doc lists it;
  `wrapClaudeArgs` grants `os.tmpdir()` and both doc comments say so; `landlock_exec.py`'s temp grant reads
  `TMPDIR` with the `/tmp` fallback and its vendor header carries the second REIFY DIVERGENCE;
  `tests/infra/test_landlock_exec_refer.sh` is green with `TMPDIR` unset and with `TMPDIR` set outside `/tmp`;
  BT18, BT18' and BT18'' are green. BT18'' is a NEW infra member, so per reify CLAUDE.md "Infra tests" it is
  stdlib-`unittest` Python (`test_landlock_exec_tmpdir_grant.py`, no pytest, no wall-clock upper bound) with a
  thin `test_landlock_exec_tmpdir_grant.sh` wrapper (an anchored `python3` invocation) and its
  `tests/infra/run-all-classification.manifest` row, all in the same diff.
- **Invariant established:** the assistant's admitted temp root equals its landlocked child's temp grant (§6.9).
- **Delivered checks:** `sidecar-tmpdir-exported` (grep `"TMPDIR"` present in `claude_bridge.rs`);
  `sidecar-tmp-literal-grant-gone` (grep `'--writable', '/tmp'` absent in `gui/sidecar/src/sandbox.ts`);
  `helper-temp-grant-reads-tmpdir` (grep `TMPDIR` present in `gui/src-tauri/sandbox/landlock_exec.py`; a
  present check, so the natural inline fix `os.environ.get('TMPDIR') or '/tmp'` satisfies it).

**ω — PRD close** (no key; low; files: `docs/prds/v0_6/gui-engine-admission.md`,
`docs/prds/v0_6/gui-engine-admission.capability-manifest.md`,
`docs/prds/v0_6/gui-engine-admission.capability-manifest.yaml`, and `docs/invariants.md` for the INV-GUI-4 owner
cell). This leaf follows the overlay's "PRD terminal status" rule. Its deliverable is
the terminal stamp: set the Status marker to the terminal token (`SHIPPED`, or `WITHDRAWN` if every leaf is
cancelled), name the landed leaf ids, add the AS-AUTHORED freeze paragraph and the LIVE/AS-AUTHORED map, and
apply the matching header to `docs/prds/v0_6/gui-engine-admission.capability-manifest.md`. It also refreshes the
INV-GUI-4 row's owner cell with α's id, leaving `reify-audit --pattern PREGISTRY` green if that pattern exists.
Its user-observable signal is the committed header. A cancelled sibling
counts as satisfied for its dependency edges. **Delivered check:** `prd-shipped` (grep with the ERE
`^\*\*Status:\*\* \*\*SHIPPED`, expect present, in this file).

### Absorbed tasks and notes to other tasks

- **#8471 → γ.** Port the OBSERVED and FIX OPTIONS text and the "check gui/test/visual callers of save_path"
  note with the `b77273634a` measurement (§6.3), then cancel with `x_subsumed_by = γ`. No task carries
  `spawned_from` #8471 (see the children check below).
- **#8574 → ε + #8954.** The watcher half goes to ε; the editor-notify half is already ρ's `open_file` push.
  Cancel with `x_subsumed_by = [ε, 8954]` after the port. No task carries `spawned_from` #8574.
- **Spawned-children check (verified).** Scout C paged every task carrying `spawned_from`, across all statuses
  (done and cancelled included), from id 8458 upward, reading the stored value with `include_value`, and found no
  `"8471"` or `"8574"` (positive controls in the same scan: #8471 → `"7816"`, #8574 → `"6257"`, #9241 → `"8949"`).
  A child's id always exceeds its parent's, so that range is complete; the string-typed storage the critic raised
  (C19) is covered because the scan read stored values rather than querying by value.
- **#9279:** `add_dependency(9279 ← ε)`, `add_dependency(9279 ← ζ)` and `add_dependency(9279 ← η)`. Append R6's premise note (landlock per
  `claude --print` child per turn; the sidecar unconfined; `REIFY_WORKSPACE` read once; `--resume <sessionId>`
  already passed per turn; the architect picks respawn or in-place re-key under K7-D3's visible-reset
  condition) and R9's: "ε deletes `AppState.initial_file`; consume `DesignWatch::design_path()`, and lower
  `resolve_workspace_dir`'s `initial_file` parameter; the `SidecarState` table and `require_ready` are ζ's —
  build on them".
- **#9174:** `add_dependency(9174 ← γ)`; note (R7): "re-target: the use-time O_NOFOLLOW / temp+`renameat`
  re-check goes inside `WriteTarget::write` (write_confinement.rs), the one write for every caller-named target
  after γ, for `origin == Assistant` only (a User target has no admitted root and keeps a plain write); the
  `confined_write_target` / `run_on_engine` anchors are deleted by ο/ρ/γ".
- **#8950** (sentence-sized): "α deletes the named guard function; keep it `pub(crate)` with no other caller".
- **#8943:** "two follow-on fields arrive after S25 η: `entry_path` (β) and `preview_live` (δ); keep the builder's
  inputs a struct so a field is one site".
- **#9285** (adjacency, no edge): "after this task, ε's re-arm test can drive the OLD design's event source and
  assert that it produces no `disk_reload_edit` ticket — the negative BT15 leaves out because over real inotify
  it is timing-dependent".
- **#8951:** "α and γ follow ο and delete nothing ο deletes; γ edits `handle_image_tool` only".
- **#6257** (adjacency, no edge): "α's `resolve_target` compares against `canonical_file_path()`; a funnel that
  sets a non-canonical `file_path` surfaces as a `NotActiveEntry` naming both spellings".
- **#7704** (adjacency, no edge): "ε moves `create_watcher`'s body into `DesignWatch`'s `arm` closure; the read
  stays in the callback".

## §11 Out of scope

- **Option C — a module-keyed `update_source` with overlay buffers**, so an import tab's edits are evaluated
  live. Rejected in K7-D1. It is the future generalisation of `resolve_target` (an `EntryDocument` would then
  name a module, not only the entry). Owner: a future PRD if multi-module editing is taken up.
- **Multi-module editing** (evaluating edits to imports before they are saved). Same trigger and owner as
  option C.
- **A `discard_preview` IPC for abandoned previews.** This stream makes an abandoned preview visible and
  bounded (§6.2). Trigger: a reported stuck preview → file a `discard_preview` command task.
- **#9174's use-time check** (O_NOFOLLOW / temp+`renameat` inside `WriteTarget::write`): #9174's own work, after
  γ.
- **#6257** (canonical `files[].path` on every open funnel): its own task; adjacency note only.
- **#7704** (move the watcher's file read under the engine lock): its own task; adjacency note only.
- **The sidecar workspace re-key** (respawn or in-place re-key; resume; reset message): #9279.
- **#9284** (dead inbound `tool_result` arm; `McpConfig` rename) and **#9285** (injectable `FileWatcher` event
  source and clock): their own tasks; file locks only (R4).

## §12 Open (tactical) questions

1. **The "not live" toast text on Re-evaluate (β).** Suggested: reuse the tab tooltip's sentence. Decide in β.
2. **Where `RecordingSessionEvents` records `design_loaded` for BT14''' (ε).** λ #8947 adds the recorder; if its
   shape records events as an enum, ε asserts on that variant; otherwise ε adds the field. Decide in ε.
3. **Whether `PreviewLiveInfo.value` carries a unit separately (δ).** Suggested: no; `value` is rendered exactly
   as `ValueData.value` is, which already includes the unit spelling. Decide in δ.
4. **The `door-raw-fs-write` ceiling (γ).** Measured at landing: the non-door residue is
   `commands::write_view_sidecar_impl`, `WriteTarget::write`, the two `lsp_bridge.rs` test writes, and the
   `debug_server.rs` test-module write if it has not moved (expected 5, or 4). Decide in γ.

## §13 Gate walk

**G1 — consumers.** Every mechanism has a named consumer: `EntryDocument` → `update_source`;
`NotActiveEntry` → the three doors' displays; `RejectedBuffer` → the host `save_file`; `entry_path` → FileTabs,
Editor, App; `WriteTarget` → five write doors; `WriteOrigin::User` → the pass-through arm;
`PreviewState`/`preview_live` → `commit_parameter`, `export`, `engine_state`; `DesignWatch::reconcile` →
`design_loaded`; `design_path` → `claude_send_message` and #9279; `require_ready` → the handle and the slot; `SourceUpdateError` → `update_source_impl`'s mapping;
`assistant_temp_root()` → `WriteTarget::admit` and `compute_sidecar_env`; the exported `TMPDIR` →
`wrapClaudeArgs` and `landlock_exec.py`.
Closure check (`#8612` closure computed 2026-10-18T16:03:50Z, 240 non-terminal tasks): every task that gains a
dependency on a leaf of this stream is outside the closure.

| dependent X | on leaf | X in #8612's closure? |
|---|---|---|
| #9171 (hotspot-survey gate) | α | no |
| #9174 | γ | no |
| #9279 | ε, ζ, η | no |

The closure tasks this stream depends on (allowed): #8942, #8943, #8947, #8950, #8951, #8954, #9070,
#9076. #8471 and #8574 are absorbed; #9284, #9285, #9278, #7704 and #6257 are outside the closure and get no
edge. No seam needs a closure leaf to wait on this stream.

**G2 — signals.** α: the import-tab refusal and the debug-MCP `InvalidParams`. β: the "not live" badge and the
silent tab. γ: `screenshot {save_path}` refused outside the roots, admitted inside. δ: export mid-drag keeps
values and meshes in agreement; `engine_state.preview_live`. ε: the newly opened file hot-reloads and the old one
does not. ζ: the chat's not-ready and crashed texts. η: with a non-`/tmp` `TMPDIR`, the assistant writes a scratch file there,
and `save_path` admission follows the same root. ω: the committed header.

**G3 — substrate.** Upstream producers, each a filed task this stream depends on: `design_loaded` from λ #8947;
`assemble_gui_state` from S25 η #8943; `gui/src/bindings.ts` and rule 4a
from #9070; the named host guard fn and the host (`TauriToolContext<'e>`, `delegated_tool_job`) from ξ #8950;
the debug originals' deletion from ο #8951; `QueuedEngine` and the snapshot open funnel from ρ #8954;
`MSG_COUNTER`'s deletion from #9076. Existing substrate, verified at `b77273634a`: reify-eval's
`Engine::edit_check`; `watcher::FileWatcher::new`; `engine_lock::with_engine_lock`;
`path_key::canonicalize_document_key`; `engine::source_key_matches_path`; the task-3228 two-file fixture shape in
`engine_tests.rs`; for η, `claude_bridge::compute_sidecar_env`, `sandbox.ts::wrapClaudeArgs` and
`landlock_exec.py`'s built-in temp grant (re-read at `eb0a855dad`), with γ as the in-batch producer of
`assistant_temp_root()`. `grammar_confirmed = true` everywhere: no `.ri` syntax is involved.

**G4 — seams.** §9. No reciprocal "the other owns it" remains: S25's gap-fill amendment names this stream for
admission, #9279 and #9284 name it for the `SidecarState` table, and #8950's details name it for the guard.

**G5 — B+H.** Five seams across eight Rust modules, the frontend and the sidecar, nine-plus cross-PRD consumers and
producers, and an invariant that three doors must keep alike: the contract (§6) and the two-way boundary tests
(§7) are what let eight leaves land independently in any order their edges allow.

**G6 — premises.** §3. The export premise is refuted (row 1), so no leaf promises a typed export refusal; R2
re-applies the preview instead. Rows 21 and 22 are code-over-design corrections made in this PRD (row 21 confirmed by
the lead decision in §4); rows 26 and 27 are the facts behind R12 and η's test placement.

**G7 — design invariants.**
- `nothing-vacuous-and-unowned`: every new type has a reader (`EntryDocument` consumed by `update_source`;
  `NotActiveEntry` displayed at three doors; `RejectedBuffer` read by the host save; `entry_path` read by the
  frontend; `WriteOrigin::User` dispatched on; `OutsideAdmittedRoots.roots` displayed; `preview_live` read by
  `engine_state` consumers and tests; `LivePreview.committed` read by the discard and `previewed` by export's
  re-apply; `Reconciled` returned and asserted; `design_path()` read by `claude_send_message`; every
  `SidecarUnavailable` variant produced and displayed). No new declared surface needs a census row (the ratchet
  rows are gates, not surfaces).
- INV-SF-1/3/5/6: no new `Diagnostic`, undef or placeholder; refusals are typed errors with `Display`, as the
  existing `WriteTargetRefusal` already is. INV-SF-2: no CLI exit code is involved.
- INV-ARCH-1: `design_watch` imports only `watcher` (downward); `engine.rs` gains no new `crate::` import;
  `write_confinement.rs` still imports nothing from the crate; `commands.rs` → `design_watch` is a new downward
  edge. `assert-module-scc.py check` stays green; no baseline row changes.
- INV-GUI-1: both new fields carry a classification token. INV-GUI-2: unchanged; every door stays on the queue.
  INV-GUI-3: strengthened (the discard returns the engine to the source value by `edit_check(committed)`, and
  export emits the committed value and then restores the preview).
- Heuristics driving the design: 10 (α construction + use; ζ handle + slot; γ admit + `write` for #9174);
  11 SPOT (one admission predicate, one refusal-text home, one watch owner, one write primitive, one
source-update error type (R11), one temp root read by the GUI and the sidecar (R12)); 12 structured
  data (`EntryDocument`/`WriteTarget` instead of a path string that may or may not have been checked;
  `SidecarUnavailable` instead of string literals); 7 stateless interactions and 5 minimum lifetimes
  (DesignWatch takes the path from the event; `initial_file`, a transient promoted to instance lifetime, is
  deleted); 3 orthogonal dimensions (`WriteOrigin` × roots is one axis, R8); 6 well-defined purpose
  (`PreviewState`: "which single override the engine holds"); 9 deep modules (`DesignWatch` hides the watcher
  lifecycle behind two methods). Comments stance: §6.2's deletions remove reviewer-answering prose whose
  premise disappears.
- Waivers: `abort` keeps its own two-variant state read (the documented exception, stated beside
  `require_ready`). δ's `preview_live` has no UI consumer in this stream (consumers: the debug read and tests;
  trigger recorded in §11).

**META.** Yes: each seam has a typed contract with construction and use enforcement points, a transition table
or reconcile rule where state moves, two-way boundary tests, and an explicit order on every contended file, so
decomposing and queueing this PRD needs no further design decision.

## Appendix A — Adoption-ratchet rows

Grammar: `pattern-id | ERE | path | today | ceiling | owner` (the manifest row omits `today`). "Today" is
`git grep -E -c` at `b77273634a` over the files the path glob matches (`tests/infra/adoption_ratchet.py`
expands the glob non-recursively, so `gui/src-tauri/src/*.rs` excludes `tests/` and `debug_server/tests/`), and
each count was confirmed by running the engine itself over these rows. `\|` is escaped for this table only; the
manifest row carries a bare `|`.

| pattern-id | ERE | path | today | ceiling | owner |
|---|---|---|---|---|---|
| mcp-context-raw-engine-lock | `\.lock\(\)` | `gui/src-tauri/src/mcp_context.rs` | 11 (all `self.engine.lock()`) | the post-S25-ξ residue, named: the lock sites of the host's own `produced: Mutex<Option<GuiState>>` slot (S25 §4.10), measured when α lands; the engine-mutex count is 0 (`with_engine_lock` everywhere) | α (R5; closes fk-0ea9b4872587; lead decision, §4) |
| update-source-string-path | `fn update_source\(&mut self, path: &str` | `gui/src-tauri/src/engine.rs` | 1 | 0 | α |
| rejected-source-convention | `holds_rejected_source` | `gui/src-tauri/src/*.rs` | 5 (engine 2, debug_server 3; ο deletes the latter) | 0 | α |
| preview-failure-surface-snapshot | `let failure_surface = ` | `gui/src-tauri/src/engine.rs` | 2 | 1 (`apply_param_to_source`'s) | δ |
| raw-write-target-admission | `confine_to_design_dir\(\|take_save_path\(` | `gui/src-tauri/src/*.rs` | 4 (debug_server 2, screenshot_save 1, write_confinement 1) | 0 | γ |
| door-raw-fs-write | `std::fs::write\(` | `gui/src-tauri/src/*.rs` | 8 (commands 2, debug_server 1, engine 1, lsp_bridge 2, mcp_context 1, screenshot_save 1) | the measured post-γ residue (§12 OQ4; expected 5 or 4) | γ |
| watch-file-call | `watch_file\(` | `gui/src-tauri/src/*.rs` | 3 (main.rs) | 0 | ε |
| app-state-initial-file | `state\.initial_file\|pub initial_file:\|initial_file: Mutex` | `gui/src-tauri/src/*.rs` | 4 (commands 1, main 3) | 0 | ε |
| sidecar-state-refusal-literal | `"sidecar not (started\|ready)` | `gui/src-tauri/src/claude_bridge.rs` | 10 | 2 (the `Display` impl) | ζ |

Corrections against the design seat's table: `watch-file-call`'s path is widened from `main.rs` to
`gui/src-tauri/src/*.rs` so that #9070 moving `open_file_engine` into `tauri_commands.rs` cannot hide a
surviving call (today's count is unchanged at 3). `app-state-initial-file` is re-scoped to the field's
constructs (§3 row 22): the design's bare `initial_file` row measured 28 and would never have reached its
ceiling of 5, because the function names `load_initial_file_impl`, `resolve_initial_file_path`,
`begin_initial_file_load` and `initial_file_evaluation` stay. `mcp-context-raw-engine-lock`'s ceiling is the
named post-S25-ξ residue rather than 0, because S25 ξ's host owns a non-engine mutex (`produced`) by design
(critic C1; lead decision, §4). Every other count matches the design.
