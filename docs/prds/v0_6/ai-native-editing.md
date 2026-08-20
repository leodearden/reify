# AI-native editing: sidecar engine-mutation tools with source-canonical, choke-point-synced writes

**Milestone:** v0_6 · **Status:** deferred bookmark (planning_mode; sequenced *after* the hotspot-hardening waves) · **Authored:** 2026-07-06 · **Shape:** B+H (contract + two-way boundary tests) · **Provenance:** bug-hotspot survey `docs/notes/bug-hotspot-survey-2026-07-05.md` §H4 (GUI state bridge) proposal C5 (feature half).

**Invariants established:** `INV-GUI-2` (AI/MCP entry point half) and `INV-GUI-3` (source-canonical) — see `docs/invariants.md`.

---

## 1. Goal

Make the in-app Claude sidecar's engine-mutation tools *real*, with the **same live frontend sync as user-driven GUI edits**, and with the **`.ri` file as the single source of truth for every mutation**.

**User-observable end state (G1 consumer = Leo dogfooding the in-app assistant on `prj/printer_v01`):**
Claude, chatting in the ChatPanel, issues `reify_set_parameter` (or `reify_update_source`) against the open design; the **viewport and property panel update live without a file reload**, and the **`.ri` file on disk now reflects the new value** — so the next thing Claude (or the user) reads from that file is its own edit, and a subsequent save is a no-op.

Today none of this works: the tools are advertised to the model but unreachable, and the one interception path that *could* reach them mutates shared engine state with **zero** frontend sync and never touches source. This PRD wires the tools as a live, properly-namespaced MCP surface, routes them through the shared state-sync choke-point, and gives them source-canonical write-back.

## 2. Background

### 2.1 The orphaned tools (premise — code-confirmed, G6 verified)

- `gui/sidecar/src/system-prompt.ts:67–83` advertises `reify_set_parameter` / `reify_update_source` (bare names) to the sidecar assistant.
- `gui/sidecar/src/session.ts:81` `ALLOWED_TOOLS = 'Read Edit Write Bash Glob Grep mcp__reify-debug__*'` — cannot grant a bare `reify_set_parameter` (wrong namespace) and the debug server does not expose it. The advertised tools are **doubly unreachable**.
- `gui/src-tauri/src/claude_bridge.rs:429–487` intercepts `tool_name.starts_with("reify_")` and runs the tool via `mcp_tool_call_impl` against a `TauriToolContext`, mutating the shared `EngineSession` — but this path returns only the tool result to the sidecar; it calls **neither `compute_delta` nor `emit_delta`** (contrast `main.rs mcp_tool_call`, which does). So even if reachable it would silently desync the viewport.
- The real engine-mutating tools live in `crates/reify-mcp/src/tools/write.rs`: `reify_set_parameter`, `reify_update_source`, `reify_open_file`, `reify_save_file`, `reify_export`. They are implemented and unit-tested but **orphaned** from the live UI (C-10/C-02 producer-orphan shape).

A separate **Wave-1 hygiene task** (survey §H4 C5 option (a)) rewrites `system-prompt.ts` to the real tool surface and deletes the dead `claude_bridge.rs` interception. **This PRD is the feature half (option (b)).**

### 2.2 The state-sync choke-point (exists, but incomplete — the reason this is deferred)

The choke-point already exists: `compute_delta(&AppState.last_state, new_state)` (`gui/src-tauri/src/diff.rs:289`) → `emit_delta` (`main.rs:34`). Both user Tauri commands (`set_parameter`, `update_source`, …) and the FS-watcher (`main.rs:242`) funnel through it, and `compute_delta` advances `last_state` as a side effect (`diff.rs:295`). But:

1. `diff_gui_state` covers only **5 of the 13** `GuiState` fields (`diff.rs:65–175`; `types.rs:162`). `tensegrity_wires/surfaces`, `display_panes/appearance`, `fea_convergence`, `demand_prune_measurement`, `files`, `fea_diagnostics` sync only on a full file-open. An AI edit that touches any of them silently desyncs — the exact "computed-but-never-surfaced" class fixed one field at a time (tasks 1764/3351/3386/4252/4884).
2. `debug_server.rs` mutations (`open_file`, `set_fea_case`) use a *different* channel (`query_frontend("apply_gui_state")`) and never touch `AppState.last_state` (latent bug #7 — the next real command diffs against a stale baseline).
3. The `claude_bridge.rs` interception reaches the choke-point not at all (§2.1).

**Closing (1) and (2), and extracting one `apply_engine_mutation` choke-point that all of GUI/debug/FS-watcher route through, is owned by the `gui-state-sync` PRD** (INV-GUI-1 + INV-GUI-2 core). This PRD **depends on** that work and completes INV-GUI-2 for the AI/MCP entry point.

### 2.3 Source of truth today (the finding that fixes the design)

The user GUI slider (`EngineSession::set_parameter`, `engine.rs:1928`) calls `edit_check` → `commit_check` (`engine.rs:1956–1962`), which writes **only** engine eval state (`last_check`) — **not** `source_map`, **not** disk. So a param tweak already creates a **second live source**: engine `values` reflect the new value while `files`/`source_map`/disk keep the old text (agent-confirmed). Claude's *persistent* edits, by contrast, go through native Write/Edit → the FS-watcher → full recompile (`reload_for_watch_impl` → `update_source` → `commit_state` rebuilds `source_map`, `engine.rs:274`) — there the `.ri` file **is** the source of truth.

**Design decision (Leo, 2026-07-06):** the ephemeral-second-source model is *wrong*. **The `.ri` source is the canonical truth of the design for all mutations** (`INV-GUI-3`). Both `reify_set_parameter` **and** the user GUI slider must write back to source. This eliminates the divergence, makes structured AI edits reconcile with the FS-watcher (they *become* file edits), and means Claude always reads its own edits back from the file.

## 3. Why deferred / activation status

Deferred bookmark, sequenced **after** the hotspot-hardening waves, because it has hard upstream dependencies that must land first (see §7 / §8):

- **`gui-state-sync` PRD** — the single `apply_engine_mutation` choke-point (INV-GUI-2 core) + compile-time-forced full-field `GuiState` coverage (INV-GUI-1). AI mutation must **not** go live until every `GuiState` field has forced sync coverage, or AI edits silently desync the viewport.
- **Wave-1 hygiene task** — corrects `system-prompt.ts` to the real tool surface and deletes the dead `claude_bridge.rs` interception (otherwise widening the allowlist reactivates the unsynced class).
- **Source-write-back substrate** (owned here, Phase 1) — three primitives that do not exist today (G3, §6).

## 4. Sketch of approach

Three moves, in dependency order:

1. **Source-write-back substrate (Phase 1, owned here).** Build the missing primitives so a structured "set parameter X = V" becomes a surgical `.ri` source edit: resolve the param default's byte span, serialize `V` to a unit-preserving `.ri` literal, splice, write disk, and recompile through the choke-point. `update_source`/`commit_state` is the existing write-back *sink*; the retained `parsed_cache` already holds the exact default span. (Full substrate audit in §6.)
2. **AI/MCP exposure (Phase 2, owned here).** Register the reify-mcp write tools on the **reify-debug HTTP MCP server** (`debug_server.rs tool_defs()`), so they become `mcp__reify-debug__*` — already covered by the sidecar allowlist glob and reachable over the existing transport. Route every one through the `gui-state-sync` `apply_engine_mutation` choke-point; `reify_set_parameter`/`reify_update_source` additionally go through the Phase-1 write-back path. Extend `docs/debug-mcp-contract.md` + parity tests.
3. **User-slider durable-edit fix + invariant enforcement (Phase 3, owned here).** Re-home `EngineSession::set_parameter` (GUI slider/edit-box) onto the same write-back path (perf-preserving — §9 Q3), and add the INV-GUI-2 (AI-path) architecture test and the INV-GUI-3 source-canonical enforcement test, following the registry's warn→enforce rollout with a break-glass knob.

## 5. Resolved design decisions

| # | Decision | Rationale |
|---|---|---|
| D1 | **`reify_set_parameter` writes back to `.ri` source** (not an ephemeral engine-state override). | `INV-GUI-3`: source is canonical. Structured AI edits become file edits and reconcile with the FS-watcher; Claude reads its own edits back. |
| D2 | **The user GUI slider is *also* wrong and must write durable source edits** through the same path. | Same invariant; the ephemeral-second-source model is a pre-existing bug, filed here as the companion slider-fix task (η). |
| D3 | **MCP home = the reify-debug HTTP server** (`mcp__reify-debug__*`). | Lowest friction: the sidecar already writes an MCP config pointing at reify-debug (`session.ts:330`) and the allowlist is `mcp__reify-debug__*` (`session.ts:81`). Aligns with Wave-1 pointing the prompt at `mcp__reify-debug__*`. No new server/transport/allowlist entry. |
| D4 | **All AI/MCP mutations route through the `gui-state-sync` `apply_engine_mutation` choke-point** (not a private emit). | `INV-GUI-2`: every AI-driven mutation path shares the same state-sync choke-point as user-driven mutations. |
| D5 | **`gui-state-sync` owns the choke-point extraction + full-field coverage + GUI/debug/watcher routing; this PRD depends on it** and owns only the AI/MCP entry point + source-write-back + slider fix. | Matches `docs/invariants.md` INV-GUI-1/INV-GUI-2 ownership; keeps this bookmark scoped and sequenced after the hardening waves. |
| D6 | **Write-back = span-splice + `update_source`, not a full AST re-serializer.** | No round-tripping pretty-printer exists (§6); the exact default span is retained and `update_source` accepts full text. Splice is minimal and preserves user formatting/comments. |
| D7 | **The in-process recompile is authoritative; the disk write's FS-watcher re-fire is idempotent.** | `reify_set_parameter` writes disk **and** recompiles in-process (so the tool result carries diagnostics synchronously and the viewport updates live); the watcher re-fire reloads identical content → empty delta (agent-confirmed same pattern as editor save). Answers the hard-req-2 FS-watcher reconciliation. |

## 6. Contract — the source-write-back seam (owned here) + the choke-point seam (consumed)

### 6.1 Source-write-back primitives (Phase 1 — G3: none exist today, all queued here)

Audit verdict (2026-07-06): the write-back **sink** (`update_source`→`commit_state`, `engine.rs:2158`/`:274`) and the **default-literal byte span** (`ParamDecl.default.span` in the live `parsed_cache`, `engine.rs:464`; `crates/reify-ast/src/decl.rs:237`) already exist. Missing:

- **`resolve_param_default_span(cell_id) -> Option<SourceSpan>`** — splits `cell_id` into `(entity, member)`, walks `parsed_cache` declarations → structure members → `MemberDecl::Param(p) if p.name == member` → `p.default.as_ref().map(|e| e.span)`. Returns `None` (structured error) if the param has no default literal to rewrite. *Invariant:* the returned span is the default **expression** range only, never the whole `param … = …` decl.
- **`value_to_ri_literal(&Value) -> Result<String>`** — serializes a runtime `Value` to a **round-trippable `.ri` literal**. Int/Bool round-trip via `Display` already; **dimensioned `Scalar` does not** (`Display` emits `0.08 m`, not `80mm` — `value.rs:3447`). Must select/preserve a unit and emit the no-space literal form. *Invariant:* `parse(value_to_ri_literal(v))` re-evaluates to a value dimensionally equal to `v` (round-trip property test).
- **`EngineSession::apply_param_to_source(cell_id, &Value) -> Result<()>`** — the orchestration glue: `resolve_param_default_span` + `value_to_ri_literal` + splice into the current `source_map` buffer + write disk + recompile via the choke-point (D7). *Invariant:* on success the on-disk `.ri`, `source_map`, and engine eval state are mutually consistent; on failure none are mutated (all-or-nothing, mirroring `commit_state`'s atomic rebuild).

### 6.2 The choke-point seam (owned by `gui-state-sync`; specified here so the two PRDs agree — G4)

This PRD consumes, and must not fork, the single mutation choke-point. Expected shape (owner: `gui-state-sync`):

```
fn apply_engine_mutation<F>(engine, last_state, emit_sink, mutate: F) -> Result<GuiState>
where F: FnOnce(&mut EngineSession) -> Result<()>
// runs `mutate`, builds the new GuiState, compute_delta(last_state, new) [advances last_state],
// emit_delta(emit_sink, delta) covering ALL 13 GuiState fields, returns the snapshot.
```

Invariants this PRD relies on: (a) `last_state` advanced exactly once per mutation (no stale-baseline race — fixes bug #7 for debug-hosted tools too); (b) the delta covers **every** `GuiState` field (INV-GUI-1) so an AI edit cannot leave any field stale; (c) the `emit_sink` is reachable from the debug-server thread (the reify-debug tools run there). If `gui-state-sync`'s signature diverges, reconcile at that PRD (it is the seam owner) — **do not** add a second emit path here.

### 6.3 MCP tool exposure (Phase 2)

Add the five reify-mcp write tools to `debug_server.rs tool_defs()` + dispatch, namespaced under reify-debug. Each dispatch handler wraps its mutation in `apply_engine_mutation` (§6.2); `reify_set_parameter`/`reify_update_source` route through `apply_param_to_source` / `update_source` respectively. Reconcile with the **existing** debug `open_file`/`set_fea_case` (which `gui-state-sync` re-homes onto the choke-point) — do not duplicate `open_file`. Update `docs/debug-mcp-contract.md` §0 and the parity tests (`debugParity.test.ts`, `debugContract.test.ts`, `debug_boundary_tests.rs`).

## 7. Boundary-test sketch (B+H — the integration gate's observable signal)

| # | Scenario | Preconditions | Postconditions (asserted) |
|---|---|---|---|
| B1 | **AI param edit → live sync (the G2 leaf).** Claude issues `mcp__reify-debug__reify_set_parameter(cellId, V)` in the ChatPanel. | `prj/printer_v01` open; `gui-state-sync` choke-point + coverage landed; Wave-1 prompt landed. | Viewport (`meshes`) and property panel (`values`) update live via emitted deltas — **no file reload**. Observable via debug MCP (`mesh_stats`/`engine_state` delta). |
| B2 | **Source-canonical (INV-GUI-3).** Same edit as B1. | — | `prj/printer_v01/printer.ri` **on disk** now contains `V` in the param's default (unit-preserving literal); `reify_get_source` returns text containing `V`; a subsequent `reify_save_file` is a no-op (content already matches). |
| B3 | **Full-field coverage (INV-GUI-1 dependency).** AI edit to a design using `display_panes`/`fea_convergence`/tensegrity. | Design exercises ≥1 of the 8 previously-uncovered fields. | Those fields sync after the AI edit (not just `meshes`/`values`) — no stale field. Fails if `gui-state-sync` coverage is absent. |
| B4 | **Choke-point / no stale baseline (INV-GUI-2, bug #7).** AI mutation via debug tool, then a normal user command. | — | The user command's delta diffs against the AI-advanced baseline (not a pre-AI stale one); no spurious over-reporting. |
| B5 | **FS-watcher reconciliation (D7).** AI `reify_set_parameter` writes disk; watcher re-fires. | 100ms debounce window. | The watcher reload produces an **empty** delta (idempotent); no double-apply, no loop, no viewport flicker. |
| B6 | **User-slider parity (INV-GUI-3, η).** User drags the slider / commits the edit-box. | Slider re-homed onto the write-back path. | On commit, `.ri` on disk reflects the new value durably; viewport syncs through the same choke-point. Drag-preview perf within budget (§9 Q3). |
| B7 | **Rejection / no-default param.** `reify_set_parameter` on a param with no default literal, or a type/dimension-mismatched `V`. | — | Structured MCP error; **no** partial mutation of disk/source_map/engine (§6.1 atomicity). |

## 8. Pre-conditions for activating

1. `gui-state-sync` PRD landed: `apply_engine_mutation` choke-point (INV-GUI-2 core) + compile-time-forced full-field `GuiState` coverage (INV-GUI-1) + GUI/debug/FS-watcher routing (incl. bug #7 fix). **Real `add_dependency` edges wired at decompose time once `gui-state-sync`'s tasks exist.**
2. Wave-1 hygiene task landed: `system-prompt.ts` names the real `mcp__reify-debug__*` tools; dead `claude_bridge.rs` interception deleted.
3. Phase-1 substrate (α/β/γ) — owned here, upstream of the AI wiring.

## 9. Cross-PRD relationship (G4)

| Other PRD / work | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `gui-state-sync` (to be authored) | consumes | `apply_engine_mutation` choke-point (§6.2); full-field `GuiState` delta coverage | **gui-state-sync** | blocked-on (edge at decompose) |
| Wave-1 hygiene task | consumes | `system-prompt.ts` real tool surface; dead `claude_bridge.rs` interception deleted | **Wave-1** | blocked-on (edge at decompose) |
| `docs/debug-mcp-contract.md` (reify-debug) | extends | new write-tool `ToolDef`s in `tool_defs()`; reconcile with existing `open_file`/`set_fea_case` | **this PRD** | queued |
| `docs/invariants.md` | establishes | `INV-GUI-2` (AI/MCP entry point half), `INV-GUI-3` (source-canonical) | **this PRD** | registered (proposed) |

No reciprocal-ownership ambiguity: the choke-point and coverage are unambiguously `gui-state-sync`'s; this PRD is a strict downstream consumer + the AI/MCP entry point.

## 10. Decomposition plan

All tasks: `planning_mode=True`, filed as **deferred bookmarks** (exclude `commit_planning`), sequenced after the hardening waves. Each cites the `INV` it serves (registry usage contract INV-META-1). Greek labels are placeholders; task IDs assigned at decompose.

**Phase 1 — source-write-back substrate (owned here; G3 prerequisites; intermediates roped to the ζ gate):**
- **α — `resolve_param_default_span(cell_id)`** · modules: `gui/src-tauri/src/engine.rs` (parsed_cache walk), maybe `reify-ast` helper. · *unlocks:* γ. · INV-GUI-3.
- **β — `value_to_ri_literal(&Value)` unit-preserving serializer** · modules: `crates/reify-ir/src/value.rs` or `reify-core`. · *unlocks:* γ (round-trip property test is its internal check, **not** its leaf signal). · INV-GUI-3.
- **γ — `EngineSession::apply_param_to_source(cell_id, &Value)`** (splice + disk-write + recompile through the choke-point, atomic) · modules: `gui/src-tauri/src/engine.rs`. · *unlocks:* δ, η, ι. · **depends on** gui-state-sync choke-point. · INV-GUI-3.

**Phase 2 — AI/MCP exposure (owned here; INV-GUI-2 AI path; intermediate → ζ):**
- **δ — expose reify-mcp write tools on the reify-debug MCP server**, routed through `apply_engine_mutation`; `reify_set_parameter`→γ; extend contract + parity tests; reconcile with existing `open_file` · modules: `gui/src-tauri/src/debug_server.rs`, `gui/src-tauri/src/mcp_context.rs`, `docs/debug-mcp-contract.md`, `gui/src/__tests__/debugContract.test.ts`. · *unlocks:* ζ, θ. · **depends on** gui-state-sync, Wave-1, γ. · INV-GUI-2.

**Phase 3 — integration gate + slider fix + enforcement (leaves):**
- **ζ — integration gate (PRIMARY LEAF, the G2 signal + §7 B1–B5, B7).** Claude issues `mcp__reify-debug__reify_set_parameter` in the ChatPanel on `prj/printer_v01` → viewport + property panel update live without a file reload **and** `printer.ri` on disk reflects the new value. · *signal:* debug-MCP mesh/value delta + on-disk file assertion (§7). · **depends on** δ, γ, gui-state-sync, Wave-1. · INV-GUI-2 + INV-GUI-3.
- **η — user-slider durable-edit fix (LEAF; §7 B6).** Re-home `EngineSession::set_parameter` onto γ; perf-preserving (§9 Q3). · *signal:* user GUI param edit → `.ri` on disk updates durably; viewport syncs (debug-MCP drag + file assertion). · **depends on** γ, gui-state-sync. · INV-GUI-3.
- **θ — INV-GUI-2 architecture test (AI/MCP entry point) (LEAF).** Assert every reify-debug write tool flows through `apply_engine_mutation`; warn→enforce rollout + break-glass env knob. · *signal:* the arch test fails if a write tool bypasses the choke-point. · **depends on** δ. · INV-GUI-2.
- **ι — INV-GUI-3 source-canonical enforcement (LEAF).** Test/lint asserting every value-mutation path writes back to source (`.ri` reflects; no ephemeral-only durable path). · *signal:* the test fails on a mutation path that skips write-back. · **depends on** γ, η. · INV-GUI-3.

DAG: `α,β → γ → {δ,η,ι}`; `δ → {ζ,θ}`; `{gui-state-sync, Wave-1} → {γ?,δ,ζ,η}`. ζ is the primary integration leaf (C-as-integration-gate); η/θ/ι are additional leaves.

## 11. Out of scope

- Choke-point extraction, full-field `GuiState` coverage, debug_server re-homing, the drifted 6th emit-quintet copy → **`gui-state-sync`** (INV-GUI-1 + INV-GUI-2 core).
- System-prompt rewrite + dead `claude_bridge.rs` interception deletion → **Wave-1 hygiene task**.
- `demand_prune_measurement` dead-on-arrival field (read by nothing) → `gui-state-sync` coverage.
- Non-param structural edits by AI (add/remove members, geometry) via structured tools — Claude uses native Write/Edit → FS-watcher for those; only `reify_set_parameter`/`reify_update_source` are structured here. A future PRD may add structured structural edits.
- An incremental `edit_source`-based write-back (avoiding full recompile) is a **later optimization** (§9 Q3), not required for activation.

## 12. Open questions (tactical — deferred, not design-blocking)

1. **Exact MCP tool naming under reify-debug** — keep the `reify_` prefix (`mcp__reify-debug__reify_set_parameter`) or debug-native bare (`mcp__reify-debug__set_parameter`)? Both are allowlist-covered. **Suggested:** keep `reify_` to preserve reify-mcp tool identity and avoid clashing with debug-native semantics; must agree with the Wave-1 prompt. Decide during δ.
2. **Unit selection in `value_to_ri_literal`** — which unit to emit when the original literal's unit isn't recoverable from the runtime `Value` (only SI base is retained)? **Suggested:** prefer the unit of the *existing* default literal (available at the resolved span) so the edit is minimal and unit-stable; fall back to a canonical unit per dimension. Decide during β.
3. **Slider write-back cadence / perf (η).** A full `update_source` recompile per drag-tick is a regression (cf. task 1861). **Suggested:** ephemeral incremental `edit_check` *preview* during drag + a single source write-back on commit/release; or an incremental `edit_source`-based write-back. Decide during η.
4. **FS-watcher debounce vs in-process recompile ordering (D7).** Confirm the 100ms debounce never races an idempotent reload against a rapid second edit. **Suggested:** in-process recompile is authoritative; the watcher reload is a pure idempotent confirmation. Verify during δ/ζ.
