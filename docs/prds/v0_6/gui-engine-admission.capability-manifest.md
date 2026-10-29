# Capability manifest — gui-engine-admission

PRD: `docs/prds/v0_6/gui-engine-admission.md`. Machine-readable twin:
`docs/prds/v0_6/gui-engine-admission.capability-manifest.yaml` (task ids stamped by `commit_planning`).

**Code anchors** verified against main `b77273634a` (2026-10-18). The design and scout seats read `5d07b395db`;
`git diff --stat 5d07b395db..b77273634a` over this stream's files is empty. The R11/R12 revision (2026-10-19)
re-measured every delivered check and ratchet count at `eb0a855dad`, where the only changes over this stream's
files are S26 ζ #9067's split of `gui/sidecar/src/session.ts` and other streams' ratchet rows; every count is
unchanged. The critic fold (2026-10-19) re-measured every check again at `ff82a1049b` (no change over this
stream's files since `eb0a855dad`). Cite by symbol; re-locate lines at implementation time.

**D3 disposition.** `scripts/prd-decompose-verify.mjs` was **not run**. Every leaf's signal is a GUI/Tauri-backend
or frontend behaviour (an editor refusal, a tab badge, a debug-MCP write refusal, a viewport/property-panel
agreement, a watcher re-arm, a chat refusal text). The D3 workflow's α probe vectors are `reify check`,
`reify eval` and tree-sitter, which cannot observe any of these, so each leaf would come back UNENUMERATED
(`INCOMPLETE`, which is not a pass). This follows the recorded precedent for GUI-only leaves (memory
`procedural_prd_d3_workflow_reach_limits`; the meshing-service PRD's scoped run), which sanctions recording the
reach limit instead of running the workflow over leaves it cannot probe. The substrate was instead bound by hand below: every upstream capability to
a filed producer task, every existing substrate symbol by `git grep` at `b77273634a`, and every leaf's own
deliverable by a delivered check whose polarity was measured. Each leaf's behaviour cover is the PRD's two-way
boundary tests (§7), which run in CI under `cargo test -p reify-gui`, `scripts/gui-test.sh` and `tests/infra`
(η's new Python member BT18''), except η's sidecar case BT18': `gui/sidecar`'s vitest suite is type-checked by
the merge gate but not run (PRD §3 row 27).
`grammar_confirmed = true` on every leaf: no `.ri` syntax is involved, and no fixture is committed.

**Verdict key.** PASS: the binding holds at authoring time — an upstream producer is a filed task this leaf
depends on, an existing symbol is present, or a delivered check measured RED at `b77273634a`. OPEN: the
leaf's own work decides the binding. Counts below are at `b77273634a` and were re-measured unchanged at
`eb0a855dad`; the R11/R12 checks (`source-update-error-one-home`, `temp-root-one-home`, η's three) were
measured at `eb0a855dad`; the checks the critic fold changed or added (`duplicated-ready-match-gone`,
`helper-temp-grant-reads-tmpdir`, `engine-state-preview-key`) were measured at `ff82a1049b`.

**Delivered-check polarity.** Every `expect: present` pattern matched 0 times and every `expect: absent`
pattern matched ≥ 1 at `b77273634a` (`git grep -E -c` over the listed paths), so each check is RED now and goes
GREEN when its leaf lands. Two absent checks can turn green early without being wedged: if S25 ζ #8942 rewrites
`update_source`'s signature, `update-source-path-param-gone` goes green before α, and if an S25 leaf deletes the
`commit_parameter` comment, `preview-liveness-guess-gone` goes green before δ; each leaf's positive check and
boundary tests still bind it. Absent patterns are construct-scoped (a signature line, a definition, a
match arm, a doc sentence), not bare tokens, and none is a natural test-fixture shape.

**Adoption ratchets.** The rows, EREs, today's counts and ceilings are the PRD's Appendix A (one home; not
restated here). `mcp-context-raw-engine-lock` keeps its ERE, but its ceiling is the named post-S25-ξ residue
(the host's `produced: Mutex<Option<GuiState>>` slot), measured when α lands, with the engine-mutex count 0
(critic C1; PRD §4 lead decisions). Each count was confirmed by running `tests/infra/adoption_ratchet.py`'s own evaluator over the
rows at `b77273634a`.

## α #9387 — EntryDocument: `resolve_target` admits every source write; `persistable_source`

| capability | evidence binding | verdict | delivered check (polarity at `b77273634a` → after α) |
|---|---|---|---|
| resolve-target-present | producer-self — 0 matches in `engine.rs` (the private `fn resolve_target` in `write_confinement.rs` is outside the path) | PASS | grep `fn resolve_target\(` present in `gui/src-tauri/src/engine.rs`: 0, RED → GREEN |
| update-source-path-param-gone | producer-self — 1 match, the signature line | PASS | grep `pub fn update_source\(&mut self, path: &str` absent in `gui/src-tauri/src/engine.rs`: 1, RED → GREEN |
| rejected-buffer-convention-gone | producer-self — 1 match, the definition | PASS | grep `fn holds_rejected_source` absent in `gui/src-tauri/src/engine.rs`: 1, RED → GREEN |
| source-update-error-one-home | producer-self — 0 matches (R11: `update_source` returns it; no second refusal enum in `commands.rs`); cover BT5' | PASS | grep `enum SourceUpdateError` present in `gui/src-tauri/src/engine.rs`: 0, RED → GREEN |
| door-spelling-fallback | producer-self — `update_source_impl` asks with the raw spelling first, then the canonical one (PRD §3 row 21; lead decisions, PRD §4); cover BT2', BT3', BT3'' | OPEN | manual |
| named-guard-fn-deleted | producer-self — #8950 names the function at its landing; α deletes it | OPEN | manual |
| one-reload-entry | producer:#8942 upstream (S25 ζ: one `EngineSession::reload()`) | PASS | — |
| session-events | producer:#8947 upstream (λ: `SessionEvents`) | PASS | — |
| host-tool-context | producer:#8950 upstream (ξ: `TauriToolContext<'e>`, `delegated_tool_job`, the named host guard fn, `with_engine_lock` in the host, the host save interlock) | PASS | — |
| debug-originals-deleted | producer:#8951 upstream (ο: deletes the debug `update_source`/`save_file` originals, `update_source_target_matches_active`, the debug `holds_rejected_source` consumer) | PASS | — |

## β #9388 — Published state names the entry; FileTabs badges non-entry tabs "not live"

| capability | evidence binding | verdict | delivered check |
|---|---|---|---|
| entry-path-wire-field | producer-self — 0 matches; `gui_state!` fields are declared `pub <name>: <type>,` (e.g. `pub files: Vec<FileData>,`) | PASS | grep `pub entry_path: Option<String>` present in `gui/src-tauri/src/types.rs`: 0, RED → GREEN |
| not-live-badge | producer-self — 0 matches (the only `not-live` in `gui/src` is a test label in `sidecarReifyDebugGating.test.ts`, outside the path) | PASS | grep `not-live-indicator` present in `gui/src/editor/FileTabs.tsx`: 0, RED → GREEN |
| entry-admission | producer:α in-batch (a stray non-entry write is still refused) | PASS | — |
| committed-bindings | producer:#9070 upstream (`gui/src/bindings.ts`, rule 4a) | PASS | — |
| gui-state-builder | producer:#8943 upstream (S25 η: `assemble_gui_state`) | PASS | — |

## γ #9389 — `WriteTarget::admit`: one admission for every caller-named write

| capability | evidence binding | verdict | delivered check |
|---|---|---|---|
| write-target-admit-present | producer-self — 0 matches | PASS | grep `pub fn admit\(` present in `gui/src-tauri/src/write_confinement.rs`: 0, RED → GREEN |
| design-dir-confinement-fn-gone | producer-self — 1 match, the definition | PASS | grep `fn confine_to_design_dir` absent in `gui/src-tauri/src/write_confinement.rs`: 1, RED → GREEN |
| save-path-shape-check-gone | producer-self — 1 match, the definition | PASS | grep `fn take_save_path` absent in `gui/src-tauri/src/screenshot_save.rs`: 1, RED → GREEN |
| temp-root-one-home | producer-self — 0 matches (R12: the one home of the assistant temp root; η consumes it) | PASS | grep `fn assistant_temp_root\(` present in `gui/src-tauri/src/write_confinement.rs`: 0, RED → GREEN |
| door-raw-fs-write-ceiling | producer-self — measured at landing (PRD §12 OQ4) | OPEN | — |
| host-save-export | producer:#8950 upstream (ξ: host `save_file`/`export` inside the job, placement row 6) | PASS | — |
| debug-confined-write-target-deleted | producer:#8951 upstream (ο: deletes `confined_write_target`, `DesignChanged`, `path_while_open`) | PASS | — |
| queued-engine-read | producer:#8954 upstream (ρ: `QueuedEngine`) | PASS | — |

## δ #9390 — PreviewState: one transition table for the live preview

| capability | evidence binding | verdict | delivered check |
|---|---|---|---|
| preview-state-present | producer-self — 0 matches across `gui/src-tauri/src` | PASS | grep `enum PreviewState` present in `gui/src-tauri/src`: 0, RED → GREEN |
| preview-liveness-guess-gone | producer-self — 1 match, the `commit_parameter` comment | PASS | grep `cannot know whether a preview is live` absent in `gui/src-tauri/src/engine.rs`: 1, RED → GREEN |
| preview-live-wire-field | producer-self — 0 matches | PASS | grep `pub preview_live:` present in `gui/src-tauri/src/types.rs`: 0, RED → GREEN |
| engine-state-preview-key | producer-self — 0 matches; `engine_state_json` is a hand-built projection (PRD §3 row 28); cover BT10 | PASS | grep `"preview_live":` present in `gui/src-tauri/src/commands.rs`: 0, RED → GREEN |
| gui-state-builder | producer:#8943 upstream (S25 η: `assemble_gui_state`) | PASS | — |
| committed-bindings | producer:#9070 upstream (rule 4a) | PASS | — |
| incremental-edit-check | existing substrate — reify-eval `Engine::edit_check` present at `b77273634a` | PASS | — |

## ε #9391 — DesignWatch: the watcher follows `design_loaded`

| capability | evidence binding | verdict | delivered check |
|---|---|---|---|
| design-watch-module | producer-self — the file is absent | PASS | path `gui/src-tauri/src/design_watch.rs` present: absent, RED → GREEN |
| reconcile-present | producer-self — 0 matches (no file) | PASS | grep `pub fn reconcile\(` present in `gui/src-tauri/src/design_watch.rs`: 0, RED → GREEN |
| watch-file-gone | producer-self — 3 matches in `main.rs` (definition + 2 calls); `tauri_commands.rs` listed because #9070 may move `open_file_engine` there first | PASS | grep `watch_file\(` absent in `gui/src-tauri/src/main.rs`, `gui/src-tauri/src/tauri_commands.rs`: 3, RED → GREEN |
| design-loaded-event | producer:#8947 upstream (λ: backend-only `design_loaded(canonical_path)`) | PASS | — |
| debug-open-funnel | producer:#8954 upstream (ρ: the debug open is a snapshot job over `load_initial_file_impl`) | PASS | — |
| host-open-funnel | producer:#8950 upstream (ξ placement row 9) | PASS | — |
| file-watcher | existing substrate — `watcher::FileWatcher::new` present | PASS | — |

## ζ #9392 — `SidecarState::require_ready` owns the not-ready refusal

| capability | evidence binding | verdict | delivered check |
|---|---|---|---|
| require-ready-present | producer-self — 0 matches | PASS | grep `fn require_ready\(` present in `gui/src-tauri/src/claude_bridge.rs`: 0, RED → GREEN |
| caller-duty-note-gone | producer-self — 2 matches (the `send_message` and `permission_decision` docs) | PASS | grep `caller is responsible for ensuring the sidecar` absent in `gui/src-tauri/src/claude_bridge.rs`: 2, RED → GREEN |
| duplicated-ready-match-gone | producer-self — 2 matches (the literal-bearing `Starting` arms of the two four-arm matches); scoped to the literal so `require_ready`'s own natural arm does not match | PASS | grep `SidecarState::Starting => Err\("sidecar not ready` absent in `gui/src-tauri/src/claude_bridge.rs`: 2, RED → GREEN |
| msg-counter-deleted | producer:#9076 upstream (S26 ξ deletes `MSG_COUNTER` in `send_message`) | PASS | — |

## η #9393 — Sidecar temp grant follows the GUI's admitted temp root

| capability | evidence binding | verdict | delivered check |
|---|---|---|---|
| sidecar-tmpdir-exported | producer-self — 0 matches; cover BT18 | PASS | grep `"TMPDIR"` present in `gui/src-tauri/src/claude_bridge.rs`: 0, RED → GREEN |
| sidecar-tmp-literal-grant-gone | producer-self — 1 match (the `wrapClaudeArgs` grant); `sandbox.test.ts` carries the same literal but is outside the path; cover BT18' (sidecar vitest, not gated) | PASS | grep `'--writable', '/tmp'` absent in `gui/sidecar/src/sandbox.ts`: 1, RED → GREEN |
| helper-temp-grant-reads-tmpdir | producer-self — 0 matches; a present check, so the natural inline fix satisfies it; cover BT18'' (mandatory new Python infra member) and `tests/infra/test_landlock_exec_refer.sh` | PASS | grep `TMPDIR` present in `gui/src-tauri/sandbox/landlock_exec.py`: 0, RED → GREEN |
| assistant-temp-root | producer:γ in-batch (`write_confinement::assistant_temp_root`) | PASS | — |
| sidecar-state-table | producer:ζ in-batch (R4 `claude_bridge.rs` order) | PASS | — |
| sidecar-env-builder | existing substrate — `claude_bridge::compute_sidecar_env`, `sandbox.ts::wrapClaudeArgs`, `landlock_exec.py`'s built-in temp grant, present at `eb0a855dad` | PASS | — |

## ω #9394 — PRD close stamp

| capability | evidence binding | verdict | delivered check |
|---|---|---|---|
| prd-shipped | producer-self — 0 matches; the PRD's Status line reads `active` and no line of the PRD begins with the stamped form | PASS | grep (ERE in the sidecar, anchored at line start) present in `docs/prds/v0_6/gui-engine-admission.md`: 0, RED → GREEN |

No binding FAILs. No leaf asserts a numeric bound, so the G6 numeric-floor branch does not fire.
