# reify-debug MCP Contract

*Task 4293 (τ0) — maintained alongside the boundary tests in
`gui/src/__tests__/debugContract.test.ts` and
`gui/src-tauri/src/tests/debug_boundary_tests.rs`.*

## How this contract is validated

| Section | Guarding test |
|---------|--------------|
| §0 Shipped tool surface + parity | `debugParity.test.ts` — tool_defs↔buildHandlers parity |
| §1 Tool-def → dispatch → handler wiring | [step-3] `debugContract.test.ts` — error-envelope + wiring |
| §2 JSON error envelope | [step-3] same file |
| §2d Image + trailing-text envelope | EMISSION: `debug_server.rs` `mcp_content_blocks_*` tests. DECODE: `rpc.test.ts` case 4b + `rpcEnvelope.test.ts`'s branch-3 fall-through case — the same success envelope through both JS decoders |
| §3 Coordinate convention | [step-5] `debugContract.test.ts` — coordinate convention |
| §4 Synthetic-event fidelity gaps | [step-7] `debugContract.test.ts` — pick↔raycast |
| §5 pick\_entity\_at ↔ raycast convention | [step-7] same file |

The Rust transport seam (query\_frontend ↔ resolve round-trip) is validated
separately by `gui/src-tauri/src/tests/debug_boundary_tests.rs` (steps 1–2).

---

## §0 Shipped tool surface

### Source of truth

**`tool_defs()` in `gui/src-tauri/src/debug_server.rs`** is the canonical,
authoritative list of advertised MCP tools (currently **66**).  Every `ToolDef`
entry there becomes visible to MCP clients via `tools/list`.

Do **not** maintain a separate exhaustive list here — that list would itself be
a drift surface.  The invariant is enforced at test time (see below).

### At-a-glance grouping (illustrative — tool\_defs() is authoritative)

| Group | Tools |
|-------|-------|
| Liveness / engine | `health`, `engine_state`, `mesh_stats`, `morph_stats`, `mesh_morph_stats`, `load_fixture` |
| Screenshots | `screenshot`, `screenshot_window`, `element_screenshot` |
| DOM / style / layout / window | `dom_query`, `query_selector`, `query_selector_all`, `get_computed_style`, `get_layout_metrics`, `active_element`, `list_elements`, `get_window_state`, `ui_outline` |
| Interaction | `click_element`, `click_at`, `type_in_editor`, `keyboard`, `press_tab`, `tab_order`, `focus_element`, `scroll`, `drag`, `hover`, `hover_at`, `orbit_camera`, `pan_camera`, `zoom_camera`, `resize_panes`, `set_window_size` |
| Viewport / selection | `viewport_state`, `select_entity`, `pick_entity_at`, `fit_to_view`, `set_camera`, `set_test_mode` |
| Editor / LSP | `editor_content`, `open_file`, `completion_at`, `definition_at` |
| Menus | `open_menu`, `menu_state` |
| Tree | `expand_tree_node`, `collapse_tree_node` |
| Diagnostics / wait / state | `get_diagnostics`, `inject_diagnostics`, `reset_app_state`, `list_console_errors`, `store_state`, `wait_for`, `wait_for_idle`, `wait_for_selector` |
| AI write tools (task 5097) | `reify_set_parameter`, `reify_update_source`, `reify_open_file`, `reify_save_file`, `reify_export` |

> **Full realized scene (task 5348):** `engine_state` and `mesh_stats` report the
> FULL realized scene — one mesh entry per rendered body, a full-scope snapshot via
> `EngineSession::build_gui_state_full_scene` — so their mesh list stays consistent
> with `viewport_state.meshCount` even while the frontend's delta path runs
> selective demand (where the plain `build_gui_state` returns only the incremental
> delta subset, per the engine_build.rs DELTA CONTRACT).
> That consistency is ENFORCED end-to-end (task 5367) by
> `gui/test/visual/smoke_mesh_count_parity_e2e.mjs` — live-only,
> `npm --prefix gui run test:smoke:mesh-count-parity` — which asserts
> `viewport_state.meshCount === mesh_stats.meshes.length === engine_state.meshes.length`;
> its decision logic is CI-covered by `gui/test/visual/meshCountParity.test.ts`.
> The smoke first requires `demand_dispatch.full_scope === false`, because under
> full scope `build_gui_state` and `build_gui_state_full_scene` agree by
> construction, so the parity would be trivially true and prove nothing.
> The equality is CONDITIONAL on every realized body being in `show` visibility
> state: `viewport_state.meshCount` counts `meshManager.getSceneMeshes()`, which
> excludes ghosted and hidden meshes — and aux realizations (`default_visible:
> false`) are default-hidden — whereas the two debug reads always report the full
> realized scene. Hiding a body, or a fixture carrying an aux component, breaks
> the equality legitimately.

### AI write tools (INV-GUI-2 AI path, task 5097)

Five tools carry the **reify-mcp tool identities**
(`crates/reify-mcp/src/tools/write.rs`) onto this surface, because the GUI's
Claude sidecar reaches the reify-debug HTTP MCP server — not the reify-mcp
registry — via the `mcp__reify-debug__*` allowlist in
`gui/sidecar/src/session.ts`.

| Tool | Writes disk? | Routes to |
|------|--------------|-----------|
| `reify_set_parameter` | **Yes** — rewrites the parameter's default literal in the `.ri` | `EngineSession::apply_param_to_source_str` |
| `reify_update_source` | No — in-memory recompile only | `EngineSession::update_source` |
| `reify_open_file` | No | the existing `open_path_into_engine` funnel |
| `reify_save_file` | **Yes** — the session buffer | `commands::save_file_impl` |
| `reify_export` | **Yes** — the export artifact | `EngineSession::export` |

**The `reify_` prefix is deliberate** (PRD §12 Q1). It preserves the tool
identities an AI client may already have learned on the reify-mcp surface,
without clashing with the debug-native bare names (`open_file`,
`engine_state`, …) that the visual-regression harness depends on.

**Two seams, ONE stated exception, no second emit path.** The FOUR
engine-mutating/I-O tools (`reify_set_parameter`, `reify_update_source`,
`reify_save_file`, `reify_export`) route their engine work through
`write_on_engine_and_refresh_baseline`. The fifth, `reify_open_file`, shares
the `open_file` funnel and so reaches the same refresh through
`open_source_into_engine_and_refresh_baseline` — it must, because that path
runs `UnresolvedGuiState::resolve` (`std::fs::canonicalize`) AFTER the engine
lock is released (#5193), an ordering a closure returning a `GuiState` from
inside the lock cannot express. So the structural claim to anchor on is
"every write tool refreshes the baseline through one of the two shared
`*_and_refresh_baseline` seams", with `reify_open_file` the one name to
enumerate — *not* "all five route through `write_on_engine_and_refresh_baseline`".

Both seams refresh the delta baseline via `crate::diff::compute_delta` (§6.2
invariant (a)) and deliberately DISCARD the returned `StateDelta` — the full
`GuiState` reaches the frontend through the caller's synchronous
`query_frontend("apply_gui_state", …)` push, never `emit_delta`. There is no
private emit path on the debug surface; do not add one.

**`reify_open_file` and `open_file` are ONE funnel under two names**, not two
implementations. Both resolve their path with `open_file_path_param` — which
accepts either the reify-mcp spelling `file_path` or the debug-native `path`
(preferring `file_path` when both are supplied) and keeps the debug-native
`"path is required"` refusal — and both do their engine work in
`open_path_into_engine`.

What the two names legitimately DO differ in is the **result envelope**, and
they must: `open_file` answers with the frontend handler's own `{ok, path}`
reply (what the visual-regression harness reads), while `reify_open_file`
answers `{success: true, source}` — the two keys
`crates/reify-mcp/src/tools/write.rs` returns for that tool name. Carrying the
reify-mcp identities onto this surface is pointless if a client that learned
`result.success` / `result.source` finds neither. `source` is the text the
funnel already read off disk, so the envelope costs no extra I/O and no extra
engine call (`reify_open_file_envelope`, pinned by
`reify_open_file_envelope_matches_the_reify_mcp_shape`).

**`reify_set_parameter` vs `reify_update_source` — the durability split.**
`reify_set_parameter` is the INV-GUI-3 path: it edits the user's canonical
document on disk, splicing ONLY the default literal's own span, and its
`value` is a **unit-bearing literal** (`"120mm"`, `"45deg"`) whose unit is
parsed by exactly the same dimension-aware parse the property-panel edit box
uses (#5757), with the value written back preserving the REPLACED literal's
unit. `reify_update_source` is the live-buffer edit: it recompiles in memory
and writes no disk, so nothing reconciles the editor buffer on its own —
which is why its `apply_gui_state` push carries the optional
`file: {path, content}` member (see `write_tool_frontend_payload`). That
member's `path` is the SESSION's canonical path
(`resolve_update_source_push_path`), never the caller's raw spelling: the
active-file guard below deliberately accepts non-canonical spellings, and
`canonicalizeKey` returns any non-absolute path unchanged, so echoing one back
would fork a second editor tab while the real one kept stale text (the
duplicate-tab shape of #3892).

**`reify_update_source` is ACTIVE-FILE ONLY.** `EngineSession::update_source`
deliberately ignores the caller's path once a `load_file` has set
`self.file_path` — it derives `module_name` from the session's own entry path
and commits with `FilePathUpdate::Preserve` (task 3370). That is right for the
editor, which only ever edits the active buffer, but this surface takes
`file_path` from an AI client, so on a multi-file project a
`reify_update_source(file_path = "…/lib.ri")` would overwrite the ACTIVE
buffer with lib.ri's text and still answer `success: true` — after which the
diagnostics filter matches nothing, the pushed `file` member opens a tab whose
content the engine does not hold, and a later `reify_save_file` writes that
text to the active path ON DISK. So the mismatch is **refused**, not
redirected: `update_source_target_matches_active` accepts the active path
verbatim, its stem-only `"<stem>.ri"` module key, or any on-disk spelling that
`canonicalize`s to it, and anything else returns `"reify_update_source can only
update the active file <path>"` having mutated nothing (no engine state, no
baseline advance, no disk). The guard is evaluated with **no engine lock held**
— the session's entry path is read under a short lock of its own, then
`update_source_target_matches_active` (up to two `std::fs::canonicalize`
syscalls) runs outside it, and the write seam is entered only once the target is
accepted. Holding the engine mutex across filesystem I/O is the exact pattern
#5193 forbids, and the one that keeps `open_source_into_engine_and_refresh_baseline`
out of the write seam in the first place. Editing a NON-active file is §11 out of scope:
Claude uses its own native Write/Edit tools and the FS-watcher reloads them.

**Diagnostics filtering.** `reify_update_source` returns diagnostics filtered
to the named file via `filter_diagnostics_for_file`, which matches BOTH the
caller's path spelling AND the stem-only `"<stem>.ri"` module key the engine
actually stamps on every `DiagnosticInfo.file_path`
(`EngineSession::get_diagnostics` → `resolve_source` → `module_key`) — a bare
`==` between the two matches nothing and would silently drop the whole
warning stream.
`reify_set_parameter`, by contrast, returns its diagnostics **unfiltered**,
which is exactly what `crates/reify-mcp/src/tools/write.rs` does for that tool
name; keep it that way so the two surfaces' envelopes stay in parity.

**`reify_save_file` and `reify_export` are pure I/O — but they still push.**
Neither commits new engine state, yet both route through
`write_on_engine_and_refresh_baseline` so §6.2 invariant (a) holds across the
four seam-routed tools without a per-tool exception. The seam's
`build_gui_state()` is a genuine REBUILD, not a cached snapshot: it calls
`mark_demand_pruned_pending()`, re-runs `tessellate_snapshot` and resolves
material appearance, and a rebuild is not guaranteed bit-identical. Refreshing
the baseline from a rebuild the frontend never saw would advance `last_state`
past what the frontend holds and the next normal command's delta would omit the
difference — the stale-baseline desync (bug #7) inverted. So both handlers push
the returned `GuiState` via `query_frontend("apply_gui_state", …)` exactly as
the mutating tools do; the baseline can never move past the frontend.

**`reify_save_file` never guesses its target.** `file_path` is the one
OPTIONAL write-tool param, so "absent" carries the live meaning *save the
ACTIVE file* — which makes two silent-fallback shapes reachable, and both are
refused rather than guessed:

- A **wrong-typed** `file_path` (`120`, `true`, an object) is refused with
  `"file_path must be a string"`. It goes through
  `reify_write_optional_str_param`, not a bare `as_str().map(…)`: folding
  "mistyped" into "absent" would turn an intended save-as into an overwrite of
  the user's canonical `.ri`. (The REQUIRED-param helper
  `reify_write_str_param` deliberately *does* fold the two, because there both
  arms end in the same refusal.)
- **Neither** an explicit target nor a session path — a `load_from_source`
  session, which has a buffer but no canonical path — is refused with
  `"no active file to save; supply file_path …"`. Falling back to
  `GuiState.files[0].path` there would write the stem-only `source_map` key
  (`"part.ri"`) as a RELATIVE path into whatever CWD the GUI process happens
  to have, and report `success: true`.

**`reify_save_file` refuses a buffer the engine rejected.** It persists the
SESSION's buffer, not a caller-supplied one — and `GuiState.files[0].content`
is *not* unconditionally the committed text. After a failed
`reify_update_source`, `EngineSession::record_compile_failure` has stored the
REJECTED source and `build_files_with_live_edit` splices it into that entry, so
`files[]` and `compile_diagnostics` describe the same snapshot (right for a
read-only `engine_state` read; catastrophic for a write-back). So the save
consults `EngineSession::holds_rejected_source` FIRST — ahead of
`build_gui_state`, making the refusal atomic — and returns
`"refusing to save: the in-memory buffer does not compile …"` for BOTH the
default target and an explicit "save as" target; writing non-compiling text to
a new path while answering `success: true` is the same lie, just less
destructive. The interlock is transient, not a wedge: any `reify_update_source`
that compiles clears the failure via `commit_state`, as does a native
Write/Edit the FS-watcher reloads. The human GUI save path is unaffected — it
carries the frontend's own content rather than reading it back out of the
engine.

### REST-only handlers (not advertised in tools/list)

`clear_selection` and `toggle_select` are reachable via the REST endpoint and
have TS handlers in `buildHandlers()`, but intentionally have **no** `ToolDef`
entry and therefore do **not** appear in `tools/list`.

### Automated drift guard

`gui/src/__tests__/debugParity.test.ts` enforces:
- Every frontend-mediated `tool_def` has a `buildHandlers()` entry (no runtime
  "unknown command" errors).
- Every `buildHandlers()` entry is either advertised in `tool_defs()` or listed
  in the documented `REST_ONLY_HANDLERS` allowlist.
- The two allowlists (`PURE_ENGINE_SIDE`, `REST_ONLY_HANDLERS`) are self-checked
  — each entry must actually exhibit its asymmetry, so a stale allowlist cannot
  silently mask real drift.

---

## §1 Tool-def → dispatch → handler wiring

### Defining a new tool

A new frontend-mediated tool requires three coordinated changes:

1. **`gui/src-tauri/src/debug_server.rs` — `tool_defs()`**
   Add a `ToolDef { name, description, input_schema }` entry so the tool
   appears in MCP `tools/list` responses.

2. **`gui/src-tauri/src/debug_server.rs` — `dispatch_tool()`**
   The default arm delegates every unrecognised name to
   `DebugBridge::query_frontend(name, params)`.  Purely engine-side tools
   (e.g. `engine_state`, `mesh_stats`) add a named match arm instead.

3. **`gui/src/debug/bridge.ts` — `buildHandlers()`**
   Add a `command_name: (params) => result` entry in the handler map.
   The handler receives the JSON params object and returns either a value
   or a `{error: string}` envelope (see §2).

4. **Engine-side WRITE tools take a different, wider path** (task 5097).
   A tool that MUTATES engine or document state has no `buildHandlers()`
   entry at all — it resolves in Rust — but it must touch four surfaces,
   all in the same commit or the tree is red:
   - the `ToolDef` in `tool_defs()` (literal `ToolDef { name: "..." }` form
     is mandatory: `gui/src/__tests__/toolDefNames.ts` parses the source
     text with a regex, and the `expectedNameCount` cross-check counts raw
     `ToolDef {` literals);
   - a named `dispatch_tool` arm;
   - the mutation routed through `write_on_engine_and_refresh_baseline`, so
     the delta baseline is refreshed (§0 "AI write tools" above);
   - the name added to BOTH `PURE_ENGINE_SIDE` in
     `gui/src/__tests__/debugParity.test.ts` and `KNOWN_DEBUG_TOOL_NAMES` in
     `gui/test/visual/assertions.ts` — each is mechanically enforced
     (parity case (c)/(f); assertions.test.ts case (a), task-5934), so
     omitting either reds the suite rather than silently drifting.

### Dispatch flow

```
MCP client
  → POST /mcp  { method:"tools/call", params:{name, arguments} }
      → dispatch_tool(state, name, args)
          → if engine-only arm matches: run directly in Rust
          → else (default arm): DebugBridge::query_frontend(name, args)
              → emits Tauri event "debug-request" { id, command, params }
                  → JS bridge (gui/src/debug/bridge.ts) listen handler
                      → buildHandlers()[command](params)
                      → invoke('debug_response', { id, result: JSON.stringify(result) })
              → DebugBridge::resolve(id, json) wakes the waiting oneshot
              → returns serde_json::from_str(json) : Value
  → MCP tool-result content ARRAY: [text] | [image] | [image, text]
       (the third shape is element_screenshot's pane diagnostics — see §2d)
```

The `id` is a monotonically incrementing u64 assigned by `DebugBridge::next_id`
that pairs each request with exactly one response via a per-request oneshot
channel (see `gui/src-tauri/src/debug.rs`).

---

## §2 JSON error envelope

Three distinct error shapes exist depending on which layer the error originates.

### 2a — Frontend in-band `{error: string}`

**Source:** `gui/src/debug/bridge.ts`, handler functions inside `buildHandlers()`.

**Shape:** `{ "error": "<message>", ...optional extra fields }`

**Examples:**
```jsonc
// Unknown command (bridge dispatch):
{ "error": "unknown command: pick_entity_at" }

// Missing required parameter:
{ "error": "selector is required" }

// Invalid CSS selector (try/catch):
{ "error": "Failed to execute 'querySelector' on 'Document': ':::' is not a valid selector" }

// Screenshot too large (with extra fields):
{ "error": "screenshot too large", "size": 17825792, "limit": 16777216 }

// Viewport not ready:
{ "error": "viewport not ready" }
```

A **wrong-typed** parameter is a schema violation, and gets a §2a error that
says so — never a not-found, and never an observation. `{"testId": 3}` does not
come back as `element with data-testid="3" not found`, which would send a
harness author hunting in the DOM for an element that was never asked for; and
`{"selector": ["div"]}` does not come back as `{"exists": true, …}`, which
would answer a malformed *request* with a true-looking *observation* — the
array stringifies to `div` inside `querySelector`, so only the guard stops it.
Every tool that resolves an element from a caller-supplied value rejects the
type at its own boundary before resolution — whether that value is `testId`,
`open_menu`'s `name`, the tree-node tools' `path`, or a whole-selector
`selector`. That rule is stated once as THE BOUNDARY RULE on
`RESOLVE_BY_TESTID_ERRORS` in `bridge.ts`, whose exported
`TYPE_GUARDED_RESOLVER_TOOLS` carries the canonical enumeration as a checkable
value, and is pinned **per guard copy** — the guards are independent copies, so
one row per copy is what keeps any single one from regressing — by the
`boundary guards above the escape` block in `debugBridge.test.tsx`, which also
asserts the enumeration against those rows.

Type-guarding is not escaping, and the two arms differ only on the latter: a
value *interpolated into* a selector this bridge builds (`testId`, `name`,
`path`) is additionally escaped, while a *whole* `selector` is not — its
metacharacters are the caller's own syntax. A malformed selector STRING is
therefore still the §2a `Failed to execute 'querySelector'…` above, not a
required-param error.

The Rust transport passes this object through verbatim: the JSON string
returned by the JS bridge is parsed by `DebugBridge::resolve` →
`serde_json::from_str`, so any extra fields survive intact.

**Guarded by:** `debugContract.test.ts` §error-envelope + wiring (step-3),
which asserts the exact `error` field for unknown-command and missing-param
cases, and confirms that invalid-selector produces `typeof result.error === 'string'`.

### 2b — Rust handler `Err(String)` → MCP `isError: true`

**Source:** `dispatch_tool()` in `debug_server.rs`, when a named Rust arm returns
`Err(e)` or when `DebugBridge::query_frontend` itself fails (timeout, channel drop,
JSON parse error).

**MCP wire shape (tools/call response):**
```jsonc
{
  "jsonrpc": "2.0",
  "id": <id>,
  "result": {
    "content": [{ "type": "text", "text": "Error: <e>" }],
    "isError": true
  }
}
```

The `isError: true` flag tells the MCP client that the tool invocation failed.
The error text is `"Error: "` + the Rust `String` from the `Err` variant.

**Source function/arm:** the `Err(e)` arm of the `dispatch_tool()` call inside
the `"tools/call"` branch of `handle_mcp()` in `debug_server.rs`.

### 2c — JSON-RPC method error

**Source:** `debug_server.rs`, unknown `method` field in the JSON-RPC request.

**Shape:**
```jsonc
{
  "jsonrpc": "2.0",
  "id": <id>,
  "error": { "code": -32601, "message": "method not found: <method>" }
}
```

This is a JSON-RPC 2.0 protocol error (not a tool-result error).  MCP clients
treat `response.error` as a transport-level failure, distinct from `isError:true`
inside a tool result.

**Source function/arm:** the `_ =>` (unknown-method) wildcard arm of
`match req.method` in `handle_mcp()`, `debug_server.rs`.

### Summary table

| Origin | Shape | `isError` |
|--------|-------|-----------|
| JS bridge handler | `{ "error": "…" }` inside tool-result text | ✗ (not set) |
| Rust Err(String) | `{ content:[…], isError:true }` | ✓ |
| Unknown JSON-RPC method | `{ error: { code, message } }` | n/a (protocol layer) |

### 2d — Image tool results: image block + optional trailing text

**Source:** `mcp_content_blocks()` in `gui/src-tauri/src/debug_server.rs`.

A tool result is a content **array**, and for image tools it is not always of
length 1. The two wire shapes a decoder has to handle:

```jsonc
// screenshot, screenshot_window, and a single-match element_screenshot:
{ "content": [ {"type": "image", "data": "<base64 PNG>", "mimeType": "image/png"} ] }

// element_screenshot that matched more than one element:
{ "content": [
    {"type": "image", "data": "<base64 PNG>", "mimeType": "image/png"},
    {"type": "text",  "text": "{\n  \"viewportId\": \"design-main\",\n  \"matchCount\": 2\n}"}
] }
```

Decoder-author checklist:

- The image block is **always** at `content[0]` — diagnostics are APPENDED,
  never prepended. A positional `content[0].type === "image"` test is therefore
  safe.
- `content.length` is **not** always 1, and must never be assumed to be. A
  decoder that reads only `content[0]` silently discards the pane diagnostics.
- `screenshot` and `screenshot_window` never emit the trailing block. Only
  `element_screenshot` does.

Two further facts about this envelope live with the code that enforces them:

- GATING RULE — the exact condition under which the trailing block is emitted:
  the `mcp_content_blocks` doc comment, property 2
  (`gui/src-tauri/src/debug_server.rs`).
- The trailing block never carries a top-level string `error`: the
  CROSS-LANGUAGE INVARIANT paragraph in `isInBandError`'s docblock
  (`gui/test/visual/rpcEnvelope.mjs`), stated there for BOTH languages.

The positional-vs-search split between the two JS decoders is the one §2d fact
this doc owns, under "The §2d divergence — canonical statement" below.

### JS-side decoders

`gui/test/visual/rpcEnvelope.mjs` is the single home of the JS-side decode of all
three shapes above — read it, not this table, for the exact branch order:

- **§2a** in-band `{error: string}` → `isInBandError`
- **§2b** `isError: true` → folded into the §2a shape by `normalizeRpcEnvelope`
- **§2c** JSON-RPC method error → surfaced as `transportError`, which the
  `makeDebugRpc` transport throws (so a driver's server-poll loop keeps retrying
  rather than mistaking an outage for an answer)

It is CI-covered by `gui/test/visual/rpcEnvelope.test.ts`. The six `smoke_*.mjs`
drivers all obtain their `rpc()` from `makeDebugRpc`; none decodes an envelope
itself.

`parseRpcResponse` (`gui/test/visual/rpc.ts`) is the typed harness's separate
rendering. It shares the §2a discriminator and the text-payload parse with the
module above, but deliberately keeps its own branch table — it collapses every
failure into `{ok: false, error}` where `normalizeRpcEnvelope` preserves the
in-band shape.

#### The §2d divergence — canonical statement

The two decoders diverge on §2d's image envelope as well, and again on purpose.
This section is the canonical statement of that rationale; the code sites cite it
rather than restating it.

`normalizeRpcEnvelope` SEARCHES the content array for its text block;
`parseRpcResponse` stays POSITIONAL on `content[0]`. Each is right for its own
caller: `run.ts` feeds `value.data` straight into `Buffer.from(…, "base64")`, so
the typed harness must take the IMAGE at `content[0]`; no driver reads image
data, so the normaliser searches past it and hands back the diagnostics a driver
can actually branch on.

Do NOT reconcile them. Rewriting `parseRpcResponse`'s branch 3 to search for the
TEXT block the way `normalizeRpcEnvelope` does — `content.find(c => c.type ===
"text") ?? content[0]`, where the `?? content[0]` fallback is needed here and
not in `normalizeRpcEnvelope`, to keep the image check reachable when no text
block exists — lets branch 4 win ahead of branch 3 for a multi-match
`element_screenshot`. Branch 4 JSON-parses the diagnostics text into
`{viewportId, matchCount}`, which has no `data` field, so `value.data` goes
missing: a `{path: "data", op: "exists"}` assertion (`assertions.ts`'s
`element_screenshot` scenario) fails outright, and a consumer piping
`value.data` into `Buffer.from` gets a TypeError on `undefined`. `run.ts`'s own
`Buffer.from` call reads the `screenshot` tool, whose envelope never carries a
trailing block, so it is untouched either way — which is why, before case 4b
existed, this rewrite passed the entire JS suite and surfaced only as corrupt
PNG bytes.

An IMAGE-targeted `.find` is a different matter and the suite does not object:
`content.find(c => c.type === "image") ?? content[0]` agrees with the positional
read on every envelope this contract admits, because §2d fixes the image at
`content[0]` — an ordering pinned by the Rust test
`mcp_content_blocks_appends_pane_diagnostics_beside_the_image`. Branch 3 stays
positional to keep branch PRECEDENCE explicit at the point of reading, not
because a test would catch that rewrite.

Coverage: §2d is pinned on both sides. EMISSION by `debug_server.rs`'s
`mcp_content_blocks_*` tests; DECODE by one *success* two-block envelope fed
through both decoders — case 4b of `gui/test/visual/rpc.test.ts`'s "the
documented divergence" suite, which asserts the two verdicts side by side, and
the branch-3 fall-through case in `gui/test/visual/rpcEnvelope.test.ts`. The
error-envelope divergences are pinned case-by-case in those same two files.
Under the TEXT-targeted rewrite above, case 4b is the SOLE failure.

---

## §3 Coordinate convention

### Pixel frame

Every pixel tool uses **CSS logical pixels measured from the viewport (window) top-left**.
This is the same frame as `Element.getBoundingClientRect()` and `MouseEvent.clientX/Y`.

```
(0, 0) ──────────────────────────────► x  (clientX / rect.left / rect.x)
  │
  │   viewport origin
  │
  ▼
  y  (clientY / rect.top / rect.y)
```

- `get_layout_metrics(selector)` returns `bounds: { x, y, width, height }` where
  `x = rect.left` and `y = rect.top` from `getBoundingClientRect()`.
- `get_window_state()` returns `devicePixelRatio` (a number) so callers can convert
  CSS pixels to physical device pixels when needed (e.g. for canvas pixel-level ops).

### Canonical round-trip

```
bounds = get_layout_metrics(selector).bounds
center = { x: bounds.x + bounds.width / 2, y: bounds.y + bounds.height / 2 }
click_at(center)      # dispatches synthetic PointerEvent at clientX/clientY
  → element's JS click handler fires with event.clientX === center.x
```

**Guarded by:** `debugContract.test.ts` §coordinate-convention (step-5),
which stubs `getBoundingClientRect` to `{x:100, y:50, width:80, height:40}`,
verifies `get_layout_metrics.bounds === {x:100, y:50, width:80, height:40}`,
then proves the derived center `(140, 70)` fires the element's click handler.

### Notes

- This convention is validated **arithmetically** in the unit tests.  The live
  `document.elementFromPoint(centerX, centerY)` hit-test (OS layout + compositing)
  is a synthetic-event fidelity gap: `click_at` dispatches a `PointerEvent` via
  `dispatchEvent`, which fires JS handlers but does not involve OS hit-testing
  (see §4).  Real-GUI e2e tests verify full OS compositing end-to-end.
- The canvas (viewport) coordinate frame is the same CSS-pixel frame: the NDC
  conversion in `createSelection` uses `rect = canvas.getBoundingClientRect()` as
  its origin (see §5).
- Downstream tools that introduce a new pixel frame MUST document the conversion
  and add a boundary test before landing.

---

## §4 Synthetic-event fidelity gaps

Synthetic `PointerEvent` / `MouseEvent` dispatch (via `element.dispatchEvent(...)`)
**fires JS event handlers** but has the following known gaps relative to real user
input:

| Capability | Synthetic events | Real user input |
|------------|-----------------|-----------------|
| JS event handlers | ✓ fires | ✓ fires |
| CSS `:hover` pseudo-class | ✗ NOT applied | ✓ applied |
| CSS `:active` pseudo-class | ✗ NOT applied | ✓ applied |
| Native drag-and-drop (`dragstart`, `drop`) | ✗ not triggered | ✓ triggered |
| OS / compositor hit-testing (`elementFromPoint`) | ✗ not involved | ✓ involved |
| `focus` / `blur` side-effects (click on input) | partial — only if `focus()` called explicitly | ✓ automatic |

**Practical implication:** tools that dispatch synthetic events can assert that
JS-registered handlers fire (click handlers, React `onClick`, Three.js pointer
listeners, etc.) but **cannot** assert CSS pseudo-class styling changes.  Tests
must reflect this: they assert handler invocation, not visual state.

This gap is accepted by the PRD (§0/§3 G6): the debug tools are designed for
programmatic control of application logic, not pixel-perfect CSS rendering
verification.

**Guarded by:** `debugContract.test.ts` §pick↔raycast (step-7), which drives the
real `createSelection` raycaster (a JS handler) and asserts `onSelect` was called —
not that any CSS changed.

---

## §5 pick\_entity\_at ↔ raycast convention

`pick_entity_at` answers: *which Three.js entity is under screen pixel (clientX, clientY)?*
It wraps the **same** `createSelection` + `Raycaster` path that is used for interactive
mouse selection.

### NDC formula

Given a canvas whose `getBoundingClientRect()` returns `rect`:

```
NDC.x =  ((clientX - rect.left) / rect.width)  * 2 - 1
NDC.y = -((clientY - rect.top)  / rect.height) * 2 + 1
```

Source: `gui/src/viewport/selection.ts` — `computeNDC()`.

### Pick pipeline

```
screen pixel (clientX, clientY)
  → computeNDC(event, rect)           # CSS-pixel → [-1, +1] NDC
  → raycaster.setFromCamera(ndc, camera)
  → raycaster.intersectObjects(meshes)
  → intersections[0].object.name     # entity path string, or null if empty
```

`Mesh.prototype.raycast` is patched with `three-mesh-bvh`'s `acceleratedRaycast`
(see `gui/src/viewport/selection.ts:28`).  If no BVH tree has been built for the
geometry, `acceleratedRaycast` falls back to the standard Three.js face traversal
transparently — no caller change is needed.

### Query-only guarantee

`pick_entity_at` is **query-only**: it does NOT mutate selection state, fire
`onSelect`, or trigger any side-effects.  Its return value is the entity path string
(or `null`) that the raycaster would resolve for the given screen coordinate.

### Validation

**Guarded by:** `debugContract.test.ts` §pick↔raycast (step-7), which builds a
real `PerspectiveCamera` at `(0, 0, 5)` looking toward the origin, places a
`BoxGeometry(1,1,1)` mesh named `'entity/box'` at the origin, and drives
`createSelection` with:
- canvas center `(400, 300)` → NDC `(0, 0)` → ray along `-Z` → hits box →
  `onSelect('entity/box')`
- far corner `(5, 5)` → NDC `≈ (-0.988, +0.983)` → misses box →
  `onSelect(null)`

This pins the screen→NDC→raycast convention that `pick_entity_at` is built on top of.
