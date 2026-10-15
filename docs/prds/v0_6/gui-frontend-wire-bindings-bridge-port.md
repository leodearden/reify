# PRD: GUI frontend — generated wire bindings, a typed bridge port, schema-validated debug handlers

**Milestone:** v0_6 · **Status:** active — authored 2026-10-14/15 (pass 1, `23390a9d75`); decomposed 2026-10-15 (pass 2): α #9062, β #9063, γ #9064, δ #9065, ε #9066, ζ #9067, η #9068, θ #9069, ι #9070, κ #9071, λ #9072, μ #9073, χ #9074, ν #9075, ξ #9076, ο #9077, π #9078, ρ #9079, σ #9080, τ #9081, υ #9082, ω #9083 (ω close); rulings OQ1–OQ7 (§7) · **Approach:** B + H

Authored 2026-10-14 by the S26 `/team /prd` session of program `review-all-reify-20261010`
(`plans/review-all-program-reify-20261012.md`, stream **S26 GUI frontend**, wave 3, mode spawn,
L/medium). Ruling source: the program doc §Resolved decisions → "S26 GUI frontend: generated wire
types, bridge port, schema-validated handlers" (Leo, 2026-10-12: specta + tauri-specta; wire types,
the Tauri commands and the events generated end to end into a committed `bindings.ts` guarded by a
regenerate-and-diff parity test; `bridge.ts` becomes thin adapters over the bindings and serves as
the typed bridge port App and engineStore take; closed-set fields become Rust enums serialised to
string-literal unions; one ordered side-panel entry list; viewStateStore owns tree reactivity;
debug handlers validate params once at dispatch against one declared schema per tool). This PRD
does not relitigate that ruling; §2 records where the code at main differs from the review's
premises and from the ruling's wording, and §7 records Leo's rulings on the questions this session
put to him.

**Code anchors** verified against main `1b06f93151` (2026-10-14); the review's anchors were
verified at `a030ee7528` (2026-10-10). `git diff a030ee7528..main` on every S26 path shows only
#7465's `ConstraintData.status` narrowing in `gui/src/types.ts`/`gui/src/bridge.ts`, #7816's
sidecar liveness work (`gui/sidecar/src/session.ts`, `system-prompt.ts`, new `tool-surface.ts`)
and `baad7c48f5`'s shared `readTauriSrc`. Main moves fast — cite-by-symbol; re-locate lines at
implementation time. Library facts (§2.9) were established by building a scratch crate outside the
repo against the repo's `Cargo.lock` (tauri 2.11.2) and running real exports and `tsc` (TypeScript
5.9.3), 2026-10-14.

---

## §0 — Goal and user-observable surface

The GUI frontend (`gui/src`, plus the Node sidecar `gui/sidecar/src`) talks to the Rust backend
(`gui/src-tauri/src`) over 34 Tauri commands and 42 event names. Today every byte of that contract
is restated by hand on the TypeScript side: `gui/src/types.ts` (1,191 lines) mirrors
`gui/src-tauri/src/types.rs` in prose ("Mirrors … in types.rs", 24 times), `gui/src/bridge.ts`
(1,042 lines) hand-spells every command name, argument key and event name, and seven listeners
re-validate payload shapes at runtime because nothing checks them at compile time. The tests
cannot reach the IPC layer through any seam, so eight test files replace `../bridge` wholesale with
hand-written factories, kept in step by a 940-line guard that parses test source text. The debug
MCP bridge (`gui/src/debug/bridge.ts`, one 1,521-line closure) re-validates each tool's params by
hand while the schemas sit unused in Rust. Around these sit a side-panel grid that drops two
panels' tracks, a subscription teardown that leaks on retry, a store whose queries go non-reactive
before the tree loads, a layout dimension set enumerated nine times, two regexes over Rust `Type`
Display text, a quantity grammar re-implemented in TypeScript, and a sidecar method whose catch-all
swallows its own bugs.

After this PRD lands, a user observes:

- **Approving a tool permission in the chat panel works.** Today `claudePermissionDecision` sends
  its fields flat while the `claude_permission_decision` command takes one named argument
  `decision`, so Tauri rejects every Allow/Deny with a missing-key error (§2.1). Fixed first (α),
  and made unrepresentable by the generated command signature (ι).
- **A renamed command, a changed argument or a renamed event fails `tsc`**, not the running app:
  `gui/src/bindings.ts` is generated from Rust and a gate test fails when it is stale.
- **The side panel keeps the chat pane usable when buckling frames or FEA diagnostics appear.**
  Today, with chat open, mounting `BucklingPanel` or `FeaDiagnosticsPanel` shifts every later child
  one grid track and collapses the chat into a clipped implicit row.
- **`reset_app_state` resets the problems panel too** (height and collapsed state), as its tool
  description promises; harness scenarios stop inheriting a resized problems pane.
- **The property editor accepts exactly what the engine accepts** (`5 mm` included) and shows the
  engine's own refusal text for a bare number in a dimensioned cell, because the backend validates
  the literal (θ).
- **A malformed debug-MCP call is refused at dispatch with a uniform message** (`testId must be a
  string`, `testId is required`), never answered with a true-looking DOM observation; tools named
  `constructor`/`toString` are unknown commands, not prototype methods.
- **A failure while mapping an assistant reply surfaces as an error in the chat**, instead of the
  reply silently vanishing.

The agents who change this code next see: one generated file for the IPC contract, one port object
for every IPC dependency, one fake for tests, one validator for debug params, and family modules a
few hundred lines long.

## §1 — Findings ledger

Thirteen keys; nine open structural, four already filed as mechanical tasks. Re-measured at main
(scouts, 2026-10-14). "→" names the leaf that closes the key (each key stamped on exactly one
leaf); other leaves that contribute are listed in §10.

| key | anchor | sev | re-measured at main | → |
|---|---|---|---|---|
| fk-2be83576bc8b | `gui/src/App.tsx::App` side-panel track IIFE | high | holds: tracks for AutoResolve, Mechanism, chat only; `BucklingPanel`, `FeaDiagnosticsPanel` in-flow with no track; 4 `countGridTracks` tests, none mounts either; Mechanism and chat predicates each stated twice | β |
| fk-252aa009dcdc | `gui/src/App.tsx::App` imports `./bridge` directly | high | holds with one correction (§2.2): 32 names imported; **8** (not 11) test files mock the bridge wholesale; guard = 561 lines (940 with the parser's own tests); 9 module-level `vi.mock` in App.test.tsx | ο |
| fk-559caca373a7 | `App.tsx::App` `initApp` subscriptions | medium | holds: 11 `let …Unsub`, 10 guarded blocks + the debug-bridge block; entry teardown clears 10 (omits `debugBridgeUnsub`), `onCleanup` 11; `handleRetry` → `initApp` leaks the previous debug-bridge listener | γ |
| fk-e0f49abf4b34 | `gui/src/debug/bridge.ts::buildHandlers` | high | holds: 1,521 lines, 58 handlers, BOUNDARY RULE comment 66 lines, 12 typeof-string copies (11 + 1 in `formControl.ts`), 6 `params.x as string` casts; "143 input_schema entries" is a token count — 69 `ToolDef`s, 55 with a frontend handler | σ |
| fk-b2129b18b706 | `gui/src/stores/viewStateStore.ts::createViewStateStore` | medium | holds: 3 empty-map short-circuits; `treeGeneration` on 10 App lines (incl. the setter) and 8 engineStore lines; docblock 73 lines; `DesignTree` reads the same non-reactive maps (§2.5) | δ |
| fk-7262d24da57d | `gui/src/types.ts` hand mirror | medium | changed in part: #7465 narrowed `ConstraintData.status` in TS by hand (`CONSTRAINT_STATUSES`, `RawConstraintData`, `convertRawConstraint`); the rest holds; `JointDescriptor.kind`/`.dimension` are two more closed-set strings | λ |
| fk-c8938e374057 | `gui/src/stores/layoutStore.ts::createLayoutStore` | medium | holds: 9 enumerations; `reset_app_state` restores 5 of 7 keys; 0 tests cover the problems pair after reset | ε |
| fk-a03c96e5733d | `gui/src/stores/autoViewGenerator.ts::isLetGeometryType` | medium | holds; the purpose-view path is **not** dead-by-design — #6905 ε feeds it (§2.6) | η |
| fk-39619f02ae9c | `gui/sidecar/src/session.ts::SidecarSession.invokeSdk` | medium | holds: 275 lines; the catch around all event handling (`JSON.parse` + mapping + `onOutput`) swallows everything as "unparseable" | ζ |
| fk-068d91243153 | `viewPersistence.ts::isPersistentViewState` duplicate | low | holds | filed #8543 (referenced, unchanged) |
| fk-0b62433f5807 | `types.ts::PersistentViewState` inline `import()` | low | holds; generation resolves it (the wire types' home becomes `bindings.ts`) | λ (subsumes #8562) |
| fk-b131e38ebad7 | `DiagnosticsPanel.tsx::DiagnosticEntry` cycle | low | holds | filed #8563 (referenced, unchanged) |
| fk-5067404e6b75 | `viewStateStore.test.ts` source-comments block | low | holds | filed #8544 (referenced, unchanged) |

## §2 — Premise corrections found at authoring (G6)

### §2.1 A live defect: permission decisions never reach the sidecar

`gui/src-tauri/src/main.rs::claude_permission_decision` takes `decision:
reify_gui::claude_bridge::PermissionDecisionArgs`. Tauri deserialises named command arguments by
key, so the invoke payload must be `{ decision: { request_id, behavior, … } }`.
`gui/src/bridge.ts::claudePermissionDecision` sends `{ request_id, behavior, … }` flat, and
`gui/src/__tests__/claudeBridge.test.ts` pins the flat shape against a mocked `invoke`. Every
Allow/Deny therefore fails with "missing required key decision"; the sidecar's pending
`approve_tool` call stays blocked. Introduced with task 3206 (`f39d1878fd`); no task tracks it.
This is exactly the class of drift generated command signatures make a compile error — and the
reason α lands before the generation chain rather than waiting for it.

### §2.2 Eight wholesale bridge mocks, not eleven; the guard is 940 lines

`git grep -nE "vi\.mock\(.*bridge"` finds real factories in 8 files: `App.test.tsx` (62 keys),
`contextIntegration.test.tsx` (62), `engineStore.test.ts` (22), `constraintVerdictParity.test.ts`
(21), `selectiveDemand.test.ts` (17), `observedDemand.test.ts` (16), `sidecarPersistence.test.ts`
(2), `viewport/Viewport.test.tsx` (2). `bridgeMockContract.test.ts` mentions the call only inside
string fixtures. The guard is `bridgeMockContract.ts` (332) + `bridgeMockCoverage.test.ts` (229) +
`bridgeMockContract.test.ts` (379). The ruling's "11 wholesale vi.mock factories" reads "8"
everywhere below.

### §2.3 `bridge.ts` is not the only IPC seam

Seven non-test modules call `@tauri-apps/api` directly: `stores/selectionStore.ts`
(`update_selection`), `editor/lspClient.ts`, `editor/completions.ts`, `editor/gotoDefinition.ts`,
`editor/hover.ts` (`lsp_request`), `editor/diagnostics.ts` (`listen('diagnostics')`) and
`debug/bridge.ts` (`debug-request` / `debug_response`). The port (§4.4) covers the first and the
last; the five LSP-channel modules go to S27 λ #8777 (G4: `lspClient.ts` is S27's), which this PRD
releases (§9). `bridge.ts` also carries hidden module state (`EDIT_EPOCH`, `lastEditSeq`) shared by
`setParameter`/`previewParameter`/`updateSource`, and imports `MessageContext` from
`stores/claudeStore` while the stores import the bridge (a layering inversion the split removes).

### §2.4 The Rust wire surface is wider and less regular than `types.rs`

- All 34 commands are `#[tauri::command]` fns in `gui/src-tauri/src/main.rs` (the `gui`-gated
  binary); none is in the library, so no library test can name them. They reach main.rs-private
  items: `engine_call`, `DebugEnabled`, `DebugMcpEndpoint`, `watch_file`/`create_watcher`.
- Wire types outside `types.rs`: `PublishedState` and `EditOrder` (`eval_queue.rs`),
  `KernelStatus` (`kernel_status.rs`), `MessageContext` and `PermissionDecisionArgs`
  (`claude_bridge.rs`), and five `reify_core` types behind its optional `serde` feature (six once S25 σ adds `Severity`)
  (`DiagnosticInfo`, `SourceLocationInfo`, `DimensionLadder`, `UnitOption`, `AutoScale`).
- `GuiState` and `StateDelta` are generated by the `gui_state!` macro
  (`gui/src-tauri/src/gui_state_schema.rs`); their derives live inside the macro.
- `MeshData` has a hand-written `Serialize` that omits empty/`None` optional fields and refuses
  non-finite floats; its `Deserialize` is derived.
- 20 integer fields/args of 64 bits (`u64` ×6 incl. `EditOrder.epoch`/`.seq`, `usize` ×10,
  `i64` ×3, the `debug_response` id).
- `serde_json::Value` on the wire: `PermissionDecisionArgs.updated_input`, the `mcp_tool_call`
  command (deleted by S25 ο #8951), and untyped `json!` members of the claude, LSP and
  `debug-request` events.
- Events: 42 names. The 12 delta events and `serialization-error` are emitted by dynamic name from
  `(String, serde_json::Value)` pairs built by `gui_state!`'s `delta_to_events`; the 10 `claude-*`
  events by dynamic name from `claude_bridge.rs::outbound_to_event`; `emit_typed` covers 10 more.
  `claude-ready` sends `{}` while `auto-resolve-start/complete` send `null`.
- `navigate-to-source` has exactly one production emitter, the event closure `mcp_tool_call`
  builds; S25 ο deletes `mcp_tool_call`, after which the App listener has no producer (§6 D27).
- Three registered commands have no frontend caller: `mcp_tool_call` (deleted by S25),
  `get_active_fea_case`, and `get_entity_identity_map` — the last has a filed consumer (#6905 ε's
  binding picker), so it stays.

### §2.5 Tree reactivity: `treeGeneration` has a second job, and a second reader

`createSelectiveDemandSync`'s docblock records that `treeGeneration` (bumped after every
idle-triggered tree refetch) is also what re-fires the demand sync after in-place mesh updates,
which the `Object.keys(meshes)` source cannot see. And `panels/DesignTree.tsx` reads the same
non-reactive maps (`getStalePaths`, `getEffectiveVisibility`) and stays correct only because its
`tree` prop changes at the same time. δ's design (§4.5) preserves the first and covers the second.

### §2.6 The purpose-view path is about to be fed, not deleted

#6905 (pending, high; gui-purpose-surface E3C2) replaces App's hard-coded `[]` with the live
active-purpose list, feeding `generatePurposeViews`. Its own text calls the
`manufacturing_ready` heuristic (`manufacturingReadyVisibilityFor`, `MATERIAL_TYPE_NAME_RE`) dead —
no purpose of that name exists — and leaves retiring it to "a follow-up". η is that follow-up; it
keeps `generatePurposeViews`. Separately: `GEOMETRY_TYPE_NAME_RE`'s `Surface`/`Curve` alternatives
match nothing (no `reify_core::ty::Type` Displays so; they are marker-trait names only), and the
backend already has the predicate — `gui/src-tauri/src/engine.rs::is_geometry_valued(&Type)`,
`Type::Geometry`-rooted through `List`/`Set`/`Option`/`Keyed`/`Map`, used today by the
consumed-intermediate rule.

### §2.7 The debug tool schemas are weaker than the hand checks, and 7 tools rewrite their params

At main `tool_defs()` is still in `gui/src-tauri/src/debug_server.rs` (#6964 has not landed). Of 69
`ToolDef`s, 14 resolve in Rust, 48 fall through to the frontend verbatim, and 7 have a Rust arm that
also calls the frontend with a **Rust-built** payload: `open_file` (`{path, content, guiState}`;
`published` after S25 ρ), `wait_for_idle` (`{timeout_ms}`), `wait_for`/`wait_for_selector`
(canonicalised), and the three image tools (`save_path` stripped). Three handlers have no
`ToolDef`: `apply_gui_state` (deleted by S25 ρ #8954), `clear_selection`, `toggle_select`
(REST-only). The advertised schemas use only `type`, `properties`, `required`, `description`,
`items`, `enum`, `minItems`, `maxItems`; the handlers additionally enforce non-empty strings,
`> 0`/`>= 0` bounds, integer line/col `>= 0`, `pick_entity_at`'s both-or-neither, and `wait_for`'s
predicate union. A validator driven by today's schemas would be weaker than today's handlers.
In S25 τ's planned `debug-tools.json`, `route` is not "has a frontend handler" (7 engine rows have
one), so validation keys on `frontend_handler != null` (§4.6). And `handlers[command]` is a plain-object index, so `constructor`,
`toString`, `valueOf` and `hasOwnProperty` bypass the unknown-command branch.

### §2.8 The sidecar prompt's tool table is curated prose, not a projection

`gui/sidecar/src/system-prompt.ts::ADVERTISED_DEBUG_TOOLS` lists 17 of 69 tools in three groups
with hand-written design-facing summaries; `sidecarPromptParity.test.ts` hand-classifies the other
52 (`NOT_ADVERTISED_TO_SIDECAR`, five groups). `gui/test/visual/assertions.ts::KNOWN_DEBUG_TOOL_NAMES`
has 72 names (69 + the 3 handler-only) and `FIXTURES` hand-copies Rust `fixture_relpath()`.

### §2.9 specta / tauri-specta facts the design depends on (probed)

- Every 2.x is a release candidate: specta `2.0.0-rc.25`, tauri-specta `2.0.0-rc.25`,
  specta-typescript `0.0.12`; nothing newer published; no maintainer statement on rc production
  status (tauri-specta #247). They must be `=`-pinned; tauri 2.11.2 compiles and exports with them.
- One `collect_commands![…]` list drives both `invoke_handler()` and the TS output; the Builder
  must be built where the commands are nameable (same crate). Commands must not be `pub` at a
  crate root (E0255). `State`/`AppHandle`/`Window`/`Webview` params are skipped. Argument names
  become camelCase (as Tauri already expects); invoke strings keep the Rust fn name.
- `Builder::export(Typescript::default(), path)` runs in a plain `#[test]` with no app; only
  export-to-file exists (no export-to-string for the full file); output was byte-identical across
  5 processes; types and events are emitted sorted. No formatter hook — the committed file is the
  raw output. Rust doc comments are emitted as JSDoc, so a doc edit changes `bindings.ts`.
- Default error handling wraps `Result` commands as `{status, data|error}`;
  `ErrorHandlingMode::Throw` keeps throwing call sites unchanged.
- Events: `#[derive(tauri_specta::Event)]` with `#[tauri_specta(event_name = "…")]` keeps wire names
  byte-for-byte, including from a `macro_rules!` `literal` fragment; payloads must be named types
  (newtypes fine); derived `emit`/`listen` need `builder.mount_events(app)`; a plain
  `app.emit("name", …)` stays wire-compatible with the generated listener.
- 64-bit integers are refused at export unless overridden (per field `#[specta(type =
  specta_typescript::Number)]`, or a global cast). `serde_json::Value` cannot be exported at all in
  this version; each such field needs `#[specta(type = specta_typescript::Unknown)]`.
- **Serde phases:** any `skip_serializing_if` makes the exporter emit `Foo_Serialize`,
  `Foo_Deserialize` and `Foo = Foo_Serialize | Foo_Deserialize`, spreading to every embedding type;
  event listeners are typed with the Deserialize phase. With `#[serde(default)]` alongside, the two
  phases are structurally identical (`x?: T | null`) and consumers can use the unsuffixed alias;
  without it, the listener's type is wrong for what Rust sends. `#[specta(optional)]` and
  `disable_serde_phases()` do not help.
- Floats: `f32`/`f64` render as `number | null` by default (NaN/∞ → `null` in JSON) — honest,
  since serde_json writes a non-finite float as `null`. The only global alternative,
  `semantic_types(…enable_lossless_floats())`, is not usable: it injects a deep-copy remap
  (`.then(v => ({...v, meshes: v.meshes.map(…)}))`) on every command and event carrying a float
  `Vec`/`Option`/array, its event wrappers fail `tsc` (TS7006), and a newtype event's wrapper
  indexes `v[0]` and throws (upstream specta #578). A per-field `#[specta(type =
  specta_typescript::Number)]` (also `Vec<Number>`, `Option<Number>`) emits plain `number` with no
  remap and passes `tsc`. §7 OQ4 records the policy.
- Foreign types need `specta::Type` from their owner crate (optional feature + `cfg_attr` derive);
  `#[specta(remote = …)]` from the GUI crate hits the orphan rule (E0117).
- `Builder::typ::<T>()` exports a type nothing references (verified for a renamed unit-variant
  enum → string-literal union).
- No new frontend dependency: the output imports `@tauri-apps/api` (already `^2`, 2.10.1
  installed) and passes `tsc` under the repo's options.

## §3 — Sketch of approach

Five strands, sequenced so nothing waits that need not.

1. **Fix and harden now (α–θ).** Six leaves need nothing from the generation chain: the
   permission-decision envelope (α); one side-panel entry list (β); one subscription set in
   `initApp` (γ); viewStateStore owning tree reactivity (δ); one layout dimension table with
   `reset()` (ε); the sidecar's `invokeSdk` split into pure parts (ζ). Two need only S25's
   `engine.rs` partition and change the wire surface, so they land **before** ι (which then
   generates them): the backend's `is_geometry_value` on `EntityTreeNode` (η) and a side-effect-free
   `validate_parameter_value` command plus a `requires_unit` fact so the property editor holds no
   grammar (θ).
2. **Generate the contract (ι, κ).** Commands move into a library module; one Builder lists every
   command and every event; `gui/src/bindings.ts` is exported and committed; a gate test fails
   when it is stale. ι covers commands and every type they reach; κ types all 42 events (the delta
   events generated by `gui_state!` itself).
3. **Adopt it (λ, μ, ν, χ).** The TS side imports wire types from `bindings.ts` and deletes its
   mirrors and their runtime re-validation (λ); the remaining closed-set fields become wire enums
   with one `isActivePhase` (μ, and χ for `JointDescriptor` after S6/S25 type the joint kind);
   `LspMethod` is exported for S27 (ν).
4. **One port (ξ, ο, π).** `bridge.ts` becomes four port modules over the generated bindings with
   one exported `BridgePort` type and one test fake (ξ); App, engineStore and every descendant
   that touched IPC take the port, and the eight wholesale mocks and the 940-line guard go (ο);
   App.tsx is then partitioned along its measured concerns (π).
5. **One debug contract (ρ, σ, τ, υ).** The debug tool table gains what the frontend needs (ρ);
   one dispatch-time validator replaces every hand guard (σ); `buildHandlers` splits into family
   modules over a resolve leaf (τ); the tool-name and fixture tables are derived from the exported
   table (υ).

## §4 — Contract (H)

### §4.1 The generated contract (ι)

```rust
// gui/src-tauri/src/ipc.rs  (#[cfg(feature = "gui")], new)
/// The whole IPC surface: every command and every event, listed once.
pub fn builder() -> tauri_specta::Builder<tauri::Wry>;   // commands + events + typ::<…>() extras
pub const BINDINGS_TS_PATH: &str = "gui/src/bindings.ts"; // repo-relative
pub fn export_bindings(path: &std::path::Path) -> Result<(), String>; // the one export config
// gui/src-tauri/src/tauri_commands.rs  (#[cfg(feature = "gui")], new): every registered command,
// each #[tauri::command] #[specta::specta], moved verbatim from main.rs with the main.rs-private
// helpers they reach (engine_call, DebugEnabled, DebugMcpEndpoint, watch_file/create_watcher);
// θ's validate_parameter_value included, get_active_fea_case deleted (D28).
```

- **Order against S25 (Leo OQ5).** ι does not wait for S25's AI-tool phase. If `mcp_tool_call`
  still exists at ι's base, its `Value` argument and return are wrapped in a `#[serde(transparent)]`
  newtype typed `Unknown` (wire bytes unchanged) and S25 ο #8951 later deletes the command from
  `tauri_commands.rs`; if ο landed first there is nothing to wrap. Likewise ι does not wait for
  S25 σ #8952 (which depends on S25 ν and so on #6665): if σ lands after ι, σ derives
  `specta::Type` on `Severity` and regenerates `bindings.ts` (rule 4a; note on #8952). ι's only
  S25 prerequisite is θ #8944.
- **The permission-decision IPC test (B5's Rust half)** lives here: once the commands are in the
  library, a gui-feature test drives the real `claude_permission_decision` through tauri's `test`
  mock runtime with the exact payload the bridge sends (α keeps the vitest pin and the GUI signal).

- **main.rs** keeps the app wiring: `let ipc = reify_gui::ipc::builder();`
  `.invoke_handler(ipc.invoke_handler())`, `ipc.mount_events(app)` first in `setup`.
  `tauri::generate_handler!` is gone (one list).
- **Export config (one home, `export_bindings`):** `Typescript::default()` with a header naming
  the regeneration command; `ErrorHandlingMode::Throw`; the float policy of §7 OQ4. Layout
  `FlatFile` (type names must be unique across modules; a collision is renamed at the Rust type).
- **Gate test** `gui/src-tauri/src/tests/bindings_tests.rs::bindings_ts_is_current`
  (`#[cfg(feature = "gui")]`): exports to a temp path, compares bytes with the committed file,
  and on mismatch fails naming `REIFY_UPDATE_BINDINGS=1 cargo test -p reify-gui --features gui
  bindings_ts_is_current`, which rewrites the committed file. The test module is declared in
  `tests/mod.rs` (its orphan-file guard).
- **Where it runs.** `cargo nextest run -p reify-gui --features gui` runs on the merge gate
  (`DF_VERIFY_ROLE=merge` forces `--scope all`) and on any task gate whose affected closure
  reaches `reify-gui`. A `gui/src/**`-only task diff runs no cargo step, so a hand edit of
  `bindings.ts` in such a task is caught at the merge gate, not the task gate — accepted, the same
  coverage S25 τ accepts for `debug-tools.json`.
- **Type rules** (each a gate-visible convention, ratcheted where a pattern exists — Appendix A):
  1. Every wire type derives `specta::Type`; foreign wire types get it from their owner crate via
     an optional `specta` feature (`reify-core` in ι; `reify-lsp` in ν) enabled on reify-gui's
     **unconditional** dependency line, as `serde` is today (Leo OQ6): both gate passes then share
     one reify-core build instead of the gui-feature pass recompiling the reify-core → reify-gui
     cone with a second feature set. ι records the measured compile-time delta in its task.
  2. Every 64-bit integer field or argument carries `#[specta(type = specta_typescript::Number)]`
     (20 sites at main; each documents that its values stay below 2^53). No global cast: a future
     `u64` hash would lose precision silently.
  3. Every `serde_json::Value` field carries `#[specta(type = specta_typescript::Unknown)]`
     (`Option<…>` where optional).
  4. Every wire `skip_serializing_if` is written `#[serde(default, skip_serializing_if = …)]`
     (wire bytes unchanged: `default` affects only deserialisation; every such field is an
     `Option<T>` today, so it compiles), so both serde phases are the same shape and consumers
     import the unsuffixed alias. Enforced by a gui-feature test in ι (B4): for every
     `X_Serialize`/`X_Deserialize` pair in the generated text, the two bodies are identical. (A
     Rust attribute cannot be ratcheted: `adoption_ratchet.py` skips every line whose first
     non-blank character is `#`.)
  4a. **After ι, any change by any stream to a command signature, a wire type or an event payload
     regenerates `gui/src/bindings.ts` in the same diff** — the gate test fails otherwise and names
     the command. Notes go on the pending tasks whose text instructs the hand-mirrored route
     (§8).
  5. `GuiState`/`StateDelta` get their derives inside `gui_state!`; `MeshData`'s derived
     `Deserialize` carries the `default, skip_serializing_if` attributes that mirror its
     hand-written `Serialize` omissions, pinned by a test that serialises a `MeshData` with every
     optional field empty and compares the omitted key set with the fields carrying the attribute.
  6. `PublishedState` (Serialize-only, private fields) derives `Type` as is.
  7. Floats follow §7 OQ4: the generator's default `number | null`, except a field whose Rust
     producer refuses non-finite values (e.g. `MeshData`'s validating `Serialize`), which carries
     `#[specta(type = specta_typescript::Number)]` (or `Vec<Number>`) citing that check. No
     `semantic_types` configuration.
- **Doc comments become JSDoc.** Editing a Rust doc comment on a wire type changes `bindings.ts`;
  the failing test names the regeneration command. Accepted: the doc is part of the contract.
- **Reversibility.** Derives and the one Builder module are the only specta surface; a fall-back to
  ts-rs replaces derives and attributes mechanically and loses only the generated
  commands/events layer.

### §4.2 Typed events (κ)

- Every event the backend emits is a named Rust type deriving `tauri_specta::Event` with
  `#[tauri_specta(event_name = "<today's name>")]`, listed once in `ipc::builder()`'s
  `collect_events!`. Emission goes through `Event::emit(&app)`; `event_bus::emit_typed` is deleted.
- **Delta events.** Each diffed row of `gui_state!` gains event type idents beside its wire
  names — `update=$u:literal, update_event=MeshUpdate, remove=$rm:literal, remove_event=MeshRemoved`
  (`macro_rules!` cannot mint an ident from a literal, the file's own header records why, and
  `paste` is ruled out there) — and the macro declares one newtype per ident with
  `#[tauri_specta(event_name = $u)]` (the `literal`-fragment form is probe-verified), plus the
  registration list `ipc::builder()` splices into `collect_events!`. `delta_to_events` emits
  through them. The row stays the one home of a field's sync classification, wire name and event
  type (INV-GUI-1's mechanism, extended — no second list). The `serialization-error` contract is
  kept: a payload that fails to serialise still produces `serialization-error` naming the item
  (today `push_serialized_event` pre-serialises; κ keeps that behaviour around the typed emit).
- **Claude events.** `claude_bridge.rs::outbound_to_event` returns a typed enum of the ten payload
  structs (`ClaudeTextDelta {id, content}`, …, `ClaudeReady {}` keeping `{}` on the wire);
  `tool_input`/`result` members are `Unknown`.
- **LSP and debug events.** `diagnostics` and `lsp-log` payloads are named structs whose LSP
  protocol members are `Unknown` (typing LSP is out of scope); `debug-request` is `DebugRequest
  {id: u64 → number, command: String, params: Unknown}` (in `debug.rs`, which S25 π #8953 also
  edits — file lock; whichever lands second rebases, symbol-cited).
- **S25-owned emitters.** S25 λ's `TauriSessionEvents` (main.rs) keeps its method set; κ changes
  only each method body from `emit_typed(…)` to the typed `Event::emit`.
- **Inventory doc and lint.** `docs/gui-event-channels.md` lists exactly the 42 names today (§1
  and §2 tables); a gui-feature test compares the registered names with column 1 of both tables.
  `scripts/check_event_inventory.sh` needs no edit: its reverse pass greps any quoted `"name"`
  literal, which `event_name = "name"` satisfies.
- **`navigate-to-source`** is typed like every other event: it has a producer until S25 ο #8951
  deletes `mcp_tool_call`. The diff that deletes its last producer deletes the event type, its
  registration, its adapter and App listener and its doc row (§6 D27; note on #8951; if S25 ο has
  already landed when κ runs, κ does it).

### §4.3 Closed-set wire fields (μ, χ)

S25 θ #8944 and σ #8952 type `ValueData.determinacy`/`.freshness`/`.kind`, `ConstraintData.status`,
`EntityTreeNode.freshness` and `DiagnosticInfo.severity`. S26 types the rest whose producer closes
the set, with per-variant `#[serde(rename = "…")]` (wire bytes unchanged, tokens greppable — S25
D7's convention):

| field | enum | tokens | producer | leaf |
|---|---|---|---|---|
| `EvaluationStatus.phase` | `EvalPhase` | `"idle"`, `"evaluating"` (§7 OQ3) | `From<EvalActivity>` in the observer | μ |
| `EntityTreeNode.kind` | `TreeNodeKind` | `structure`, `occurrence`, `param`, `let`, `auto`, `realization`, `sub`, `port` | `build_template_node` (fingerprint bytes unchanged) | μ |
| `DefInfo.kind` | `DefKind` | `structure`, `occurrence` | `find_parsed_decl_containing_offset` | μ |
| `WarmPoolEvent.kind` | `WarmPoolEventKind` | `evicted`, `donated` | `WarmPoolEvent::from_engine_event` | μ |
| `PermissionDecisionArgs.behavior` | `PermissionBehavior` | `allow`, `deny` | the frontend | μ |
| `ViewDefinitionData.visibility` values, `PersistentViewState.explicit` values | `VisibilityState` | `show`, `ghost`, `hidden` | the frontend (sidecar file); behaviour change, stated: a `.views.json` carrying an unknown token is refused as a whole with the serde error, as malformed JSON is today (Rust accepts any string at main and no TS guard checks values) | μ |
| `JointDescriptor.kind`, `.dimension` | `JointKindWire`, `JointDimension` | `prismatic`, `revolute`, `fixed`, `coupling`; `length`, `angle`, `dimensionless` | from S6's `JointKind` after S25 κ #8946 | χ |

Stay `String`, with the reason recorded in μ's task: `TensegrityWireData.kind`/`…SurfaceData.kind`
(open at the DSL level: `stdlib/tensegrity.ri` declares `kind` a String a user may set),
`SolverProgress.solver_kind` (display-only, one producer), `ScalarChannelTag.unit` and channel
keys (open), `PersistentViewState.version` (a format version, compared once in the loader).

**One active-phase predicate.** `gui/src/evalPhase.ts` exports `isActivePhase(phase: EvalPhase):
boolean` as an exhaustive `switch` over the generated union (a new variant fails `tsc` there);
`engineStore.setEvalStatus`, the debug `wait_for_idle` handler (which loses its `engine_phase`
error branch and its comment naming a non-existent `error` phase), App's five `phase !== 'idle'`
sites and `Viewport`'s overlay call it. `StatusBar` keeps its `data-phase` styling over the union.

### §4.4 The bridge port (ξ, ο)

```ts
// gui/src/bridge/engine.ts     engine commands + the edit-order counter (setParameter,
//                              previewParameter, updateSource share it), view sidecar,
//                              kernel status, FEA case, cancel, validateParameterValue (θ)
// gui/src/bridge/events.ts     engine, file, kernel, selection-side subscriptions (one
//                              on<Name>(cb) per event, over events.<name>.listen)
// gui/src/bridge/claude.ts     claude commands + subscriptions + MessageContext wire mapping
// gui/src/bridge/host.ts       dialogs (save/open/ask), isDebugEnabled, onDebugRequest/debugResponse,
//                              updateSelection
// gui/src/bridge/index.ts
export function createBridge(): BridgePort;          // one instance per app; the edit-order
                                                     // counter (EDIT_EPOCH, lastEditSeq) lives in
                                                     // its closure, not in module state
export type BridgePort = ReturnType<typeof createBridge>;
export const BridgeContext: Context<BridgePort>;     // provided by App
export function useBridge(): BridgePort;
// gui/src/index.tsx: render(() => <App bridge={createBridge()} />)
// gui/src/__tests__/support/fakeBridge.ts
export function createFakeBridge(overrides?: Partial<BridgePort>): FakeBridge; // BridgePort +
//   fake.emit.<event>(payload) to drive subscriptions, + call recorders
```

- **Adapters are thin.** Each wraps a generated `commands.*`/`events.*` call; logic stays only where
  it is real — the edit-order counter, `MeshData` → typed-array buffers, `MessageContext` mapping,
  dialog defaults, `readViewSidecar`'s `null` coercion. `validatePayload`, the `KEYS_*` arrays and
  the seven runtime shape guards are deleted: the payload types are generated and the gate test
  pins them. `MessageContext` and `KernelStatus` are imported from `bindings.ts`, not from a store.
- **Consumers take the port.** `App` takes `props.bridge` (default `createBridge()`) and provides
  `BridgeContext`; stores constructed in App receive the slice they use explicitly
  (`createEngineStore({…, bridge})` covering its 20 subscriptions and 3 commands;
  `createSelectionStore({updateSelection})`; `subscribeModeShapeFrames(bridge.onModeShapeFrame,…)`;
  `createSidecarPersistence({readViewSidecar, writeViewSidecar})`); components outside App's body
  (`Editor`, `Viewport`, `WarmPoolDebugPanel`) call `useBridge()`. `createDebouncedSaver`'s save
  function is injected, so the `viewPersistence` namespace import goes.
- **Tests.** One `createFakeBridge`; `tsc` enforces completeness (a missing member is a type
  error). Ratchet `bridge-wholesale-mock` at 0 forbids `vi.mock` of the bridge.

### §4.5 App (β, γ, δ, π)

- **Side panel (β).** `gui/src/app/sidePanel.ts` exports `SidePanelEntry = {id, mounted: () =>
  boolean, track: () => string, render: () => JSX.Element}` and a pure `sidePanelTracks(entries)`;
  App renders `<For each={mountedEntries()}>` and sets `grid-template-rows` from the same list, so
  a panel cannot be added to one without the other. `BucklingPanel` and `FeaDiagnosticsPanel` take
  `auto` tracks like AutoResolve and Mechanism. Out-of-flow overlays (`SolverProgressOverlay`,
  `position: fixed`) render outside the grid container, so "tracks == children" is exact. Today's
  track strings are preserved for every combination that does not mount the two newcomers
  (including "chat closed with an optional panel ⇒ no `1fr` track").
- **Subscriptions (γ).** `initApp` registers through one `SubscriptionSet` (`track(promise)`,
  `disposeAll()`), iterating a table of `[subscribe, handler, onFailure]` rows that keeps today's
  per-row failure policy (toast / warn / error); the entry teardown and `onCleanup` both call
  `disposeAll()`; the debug-bridge unlisten joins the set.
- **Tree reactivity (δ).** viewStateStore keeps its maps but owns a `treeVersion` signal; every
  map-reading query reads it before any short-circuit, so a reactive caller subscribes even before
  the tree loads. `setTree` and `regenerateAutoViews` bump it **last, inside one `batch()`** with
  the map swap and the store write, so no observer runs between the swap and the explicit-state
  prune (today App bumps `treeGeneration` *after* `regenerateAutoViews` returns for exactly this
  reason). App's `treeGeneration`, its three threadings and `createSelectiveDemandSync`'s third
  parameter (and the 73-line readiness docblock) are deleted. The demand sync's second trigger is
  preserved without App: `getAllEffective()` tracks `treeVersion`, which `regenerateAutoViews`
  bumps after every idle refetch, exactly when `treeGeneration` was bumped. The fuzzy-rebind effect
  (which today tracks only `treeGeneration` and `untrack`s `getStalePaths`) tracks `getStalePaths()`
  itself. `DesignTree`'s memos subscribe through the same queries. B8' pins one `syncDemand` call
  per regeneration (no over-firing).
- **Partition (π).** After ο, App.tsx (2,395 lines; the component 1,952) splits along the measured
  concerns into `gui/src/app/`, in a fixed import order — each module imports only modules to its
  left:
  `viewHelpers.ts` (the exported pure helpers tests already import) < `layout.ts` (splitters,
  clamps, side-panel entries; takes `initPhase` and `chatOpen` accessors) < `scene.ts` (entity
  tree, auto views, visibility, panes, demand sync; takes an `onViewportSelect` callback rather
  than calling the shell's handler) < `fileWorkspace.ts` (open/save/reload/conflict, view
  persistence; exposes `markChanged`/`removeChanged`) < `session.ts` (subscriptions and init;
  calls `fileWorkspace`'s API) < `App.tsx` (shell, dialogs, keyboard). Each module is a factory
  with its **own named deps interface** (not one shared services bag — heuristic 9's narrow
  interfaces); the few imperative handles (viewport `flyTo`/`fitToView`, the editor's live
  content) travel as one small `AppRefs` record created by the shell. No reach-back into App's
  closure (heuristic 13) and no module that is a function-bag over another's private state
  (heuristic 14's no-cheating clause). Each module stays under the h14 soft ceiling (~1,500
  lines); π records before/after sizes and the import order in its task.

### §4.6 Debug dispatch: one validator, family modules, derived tables (ρ, σ, τ, υ)

- **Validation boundary.** S25 τ's `gui/src-tauri/debug-tools.json` carries per tool `{name,
  description, inputSchema, route, frontend_handler}`. Every row with `frontend_handler != null`
  is validated once by `initDebugBridge` before its handler runs, against the row's
  `frontend_schema` when it has one and its `inputSchema` otherwise, refusing in-band (`{error}`,
  which the harness already reads as failure). `frontend_schema` (added by ρ) describes the
  payload a Rust arm sends when it differs from what the agent sent — the seven tools of §2.7:
  `open_file` (post-S25-ρ `{path, content, published}`), `wait_for_idle` (`{timeout_ms}`),
  `wait_for` (`{predicate, timeout_ms}` — Rust validates only `timeout_ms` and passes the predicate
  through unvalidated, so the `kind`-discriminated `oneOf` lives here), `wait_for_selector` (its
  allowlist), and the three image tools (`save_path` stripped). No frontend handler runs on
  unvalidated params.
- **Lookup.** `Map<string, CommandHandler>` (or `Object.hasOwn`) — inherited prototype names are
  unknown commands.
- **Schema strength (ρ, §7 OQ1).** Every frontend-routed row's schema expresses the rules its
  handler enforces today: `minLength: 1` on identifier strings, `minimum`/`exclusiveMinimum` on
  bounds and timeouts, `minimum: 0` on LSP line/col, `oneOf` with a `kind` discriminator for
  `wait_for`'s predicate, `dependentRequired` for `pick_entity_at`'s x/y, `required` on `drag`'s
  x/y. **Every frontend handler without a row at ρ's base** gains a row marked
  `advertised: false` (absent from `tools/list`, present for dispatch): `clear_selection`,
  `toggle_select`, and S25 ο's handler-only `sync_editor_buffer {path, content}` (a Rust-sent
  payload, so its schema is a `frontend_schema`); σ adds a vitest that every `buildHandlers` key
  has a row. S25 τ's exhaustive
  `EngineTool` dispatch must still reach them between τ and ρ (a fall-through for names without a
  row, or rows τ registers) — a cross-reference to the lead and a note on #8955; neither has a
  caller beyond manual REST use, so a short gap is tolerable but not designed in. The validator
  supports exactly the keywords the table uses — a row using an unsupported keyword fails a vitest
  that walks the committed JSON.
- **Validator** `gui/src/debug/validate.ts`: dependency-free, `validate(schema, params) → null |
  string`, uniform messages (`<param> is required`, `<param> must be a <type>`, `<param> must be
  one of a, b`, `<param> must not be empty`, `<param> must be > 0`). The BOUNDARY RULE's property —
  a wrong-typed `testId` never reads as "not found" — holds by construction (validation precedes
  resolution); its census comment, `TYPE_GUARDED_RESOLVER_TOOLS` and the per-copy test rows are
  replaced by one vitest over the committed table ("every frontend-routed row whose properties
  include `testId`/`selector`/`viewportId` types it `string`").
- **Family modules (τ).** `gui/src/debug/handlers/{inspect,drive,pointer,camera,layout,lsp,
  fixtures,wait,editor}.ts`, each `(ctx: DebugContext) => Record<string, CommandHandler>`, reading
  `ctx` lazily (it is mutated by `registerDebugPanel` after build), over
  `gui/src/debug/resolve.ts` (resolveByTestId, pickViewport, pollUntil, …, imported downward).
  `buildHandlers(ctx)` merges the family records into one flat record and stays exported and
  construction-free (`debugParity.test.ts` and `assertions.test.ts` call it with `{}`).
  `driveTreeNode` and the hoisted LSP client move into their family.
- **Derived tables (υ).** `KNOWN_DEBUG_TOOL_NAMES` = every row's name (advertised or not);
  `FIXTURES` = the JSON's `fixtures` map; `debugParity.test.ts`'s `REST_ONLY_HANDLERS` =
  `advertised == false`. The sidecar prompt's curated table stays prose in `system-prompt.ts`
  (§7 OQ2); its membership is checked against the JSON (every listed name is an advertised tool)
  and `NOT_ADVERTISED_TO_SIDECAR` becomes the derived complement, so adding a tool needs no
  prompt-parity edit. Every test helper that reads a Rust **source** file goes; `tauriSource.ts`
  itself goes only if no reader remains (S25 τ may repoint it to read `debug-tools.json`, which is
  not a Rust source). υ also adds the `resize_panes`-vs-layout-table pin (§4.7).

### §4.7 Layout dimensions (ε)

```ts
// gui/src/stores/layoutStore.ts
export const LAYOUT_DIMENSIONS = [
  { key: 'editorWidth',       kind: 'px',   default: 300, requiredInSaved: true },
  { key: 'sideWidth',         kind: 'px',   default: 300, requiredInSaved: true },
  { key: 'designTreeHeight',  kind: 'px',   default: 160 },
  { key: 'propertyHeight',    kind: 'px',   default: 200 },
  { key: 'constraintHeight',  kind: 'px',   default: 140 },
  { key: 'problemsHeight',    kind: 'px',   default: 160 },
  { key: 'problemsCollapsed', kind: 'flag', default: true },
] as const;
export type PanelLayout = …derived…;  export type LayoutStore = { state; set<Key>…; reset(): void };
```

`PanelLayout`, `loadPanelLayout`'s validation (the two `requiredInSaved` keys keep dropping the
whole saved layout when absent), initial state, persistence and setters derive from the table;
`reset()` restores every row. `DebugStores.layout` is `LayoutStore`; `resize_panes` iterates the
`px` rows (its advertised schema's six properties; υ, which runs after S25 τ produces
`debug-tools.json`, adds the vitest pinning that equality); `reset_app_state` calls
`layout.reset()`. The eight hand-built layout stubs in
debug tests become one shared factory over the real `createLayoutStore`.

### §4.8 Backend facts replace frontend grammar (η, θ)

- **η.** `EntityTreeNode` gains `is_geometry_value: bool`, set in the value-cell branch of
  `build_template_node` from `is_geometry_valued(&cell.cell_type)` (false elsewhere).
  `autoViewGenerator.defaultVisibilityFor` rule 2 tests `node.kind === 'let' &&
  node.is_geometry_value`; `GEOMETRY_TYPE_NAME_RE`, `MATERIAL_TYPE_NAME_RE`,
  `manufacturingReadyVisibilityFor` and its `manufacturing_ready` branch are deleted;
  `generatePurposeViews` stays for #6905 ε.
- **θ.** New command `validate_parameter_value(cell_id: String, value: String) -> Result<(),
  String>`: `parse_cell_id` → `resolve_known_cell_type` → `parse_value_string_for_cell`, with no
  edit, no solve slot and no commit. `PropertyEditor` awaits it on Enter/blur before
  `setParameter`, marks the field invalid with the returned message, and drops `isValidValue`.
  **Keyboard contract:** at most one validation per cell in flight; an Enter or blur arriving
  while one is pending is a no-op, so Enter-then-blur submits once and never validates a stale
  buffer (B12').
- **The focus-time edit seed (Leo OQ7).** `ValueData` gains `requires_unit: bool`, set by the
  backend from its own `dimension_requires_unit` over the cell's declared dimension (the
  `LADDER_COVERAGE` rule — one home). `editSeed` appends the display unit iff it is true, so an
  untouched commit stays a no-op (the #5757 contract, B12''); which unit is shown stays the
  frontend's display preference.
- `unitLadder.ts` keeps display helpers only (`formatDisplayNumber`, `convertToUnit`,
  `normalizeUnitLabel`, `ladderForDimension`); `NUMBER_RE`, `buildQuantityRe`,
  `acceptsBareNumber`, `BASE_UNIT_LABELS`, `BASE_UNIT_DIMENSIONS`, `quantityUnitAlphabet` and
  their grammar prose are deleted, as is the Rust test that mirrors the TS floor
  (`every_dimension_the_frontend_floor_gates_is_gated_here_too`, whose subject is gone). Behaviour
  changes, stated: the editor accepts every literal the backend accepts (e.g. `5 mm`), and
  validity feedback arrives after one IPC round trip.

### §4.9 Sidecar (ζ)

`gui/sidecar/src/claudeArgs.ts::buildClaudeArgs(config, sessionId, mcpConfigPath): string[]` (pure);
`gui/sidecar/src/streamEvents.ts::mapStreamEvent(event, state): {outputs: OutboundMessage[], state}`
(pure reducer over the assistant/result events and the four content-block kinds, carrying
`lastTextLen`, `lastThinkingLen`, the message-id turn boundary and the tool-use correlation);
`invokeSdk` keeps process lifecycle only (spawn, stdin, timeout, exit). The per-line `try` covers
`JSON.parse` alone: an unparseable line is skipped with a `console.warn`, and an exception from
mapping or from `onOutput` propagates, so `invokeSdk` rejects and `handleSendMessage` emits
`claude-error`. Behaviour change, stated: a throwing host callback now surfaces instead of
vanishing.

## §5 — Boundary tests (two-way)

| # | scenario | producer side | consumer side |
|---|---|---|---|
| B1 | Rust renames a command argument | `bindings_ts_is_current` fails until regenerated | regenerated `bindings.ts` makes the adapter's call a `tsc` error |
| B1' | TS adapter passes a wrong argument key | — | `tsc` fails on `commands.x({…})` |
| B2 | Rust changes an event payload field | `bindings_ts_is_current` fails | listener's use of the field fails `tsc` |
| B3 | `gui_state!` gains a diffed field | macro emits its event newtype + registration; test fails until regenerated | `events.<name>` exists; `eventChannelConsumerCoverage` names it |
| B4 | a wire field gains `skip_serializing_if` without `default` | ι's phase-identity test reds (`X_Serialize` and `X_Deserialize` bodies differ) | — |
| B5 | permission decision Allow | gui-feature mock-runtime IPC test: the bridge's exact payload deserialises into the real command (ι, once the commands are in the library) | vitest pins `invoke('claude_permission_decision', {decision: {…}})` (α) |
| B6 | side panel with every mount combination | — | `sidePanelTracks` table test: tracks == mounted entries for all 32; App test with buckling + FEA + chat mounted: chat panel in its `minmax(160px, 1fr)` track |
| B7 | `initApp` fails then Retry | — | after Retry exactly one `debug-request` listener is live (counted on the mocked `listen` in γ; on the fake port after ο) |
| B8 | first post-mount visibility toggle before the tree loads | viewStateStore bumps `treeVersion` last in `regenerateAutoViews`' batch | `syncDemand` is called with the toggled realization; DesignTree eye reflects it |
| B8' | an idle refetch regenerates the auto views | one `treeVersion` bump | exactly one `syncDemand` call; the fuzzy-rebind prompt fires once per stale path |
| B9 | `reset_app_state` after `resize_panes {problemsHeight: 400}` and a collapse toggle | `layout.reset()` | `get_local_storage('reify-panel-layout')` shows `problemsHeight: 160`, `problemsCollapsed: true` |
| B10 | debug `dom_query {testId: 7}` | validator refuses `testId must be a string` before resolution | harness sees `{error}` |
| B10' | debug tool `constructor` | Map lookup | `unknown command: constructor` |
| B11 | `phase` gains a variant in Rust | regenerated union | `isActivePhase`'s exhaustive switch fails `tsc` |
| B12 | property editor `10` in a Length cell; `5 mm` | `validate_parameter_value` refuses / accepts | inline invalid state carries the backend message / value submits |
| B12' | Enter then blur on one edit while validation is in flight | one validation per cell in flight | one `setParameter` call; the pending blur is a no-op |
| B12'' | focus a Length cell showing 120 mm | `ValueData.requires_unit = true` | the edit seed is `120mm` (an untouched commit is a no-op, the #5757 contract) |
| B13 | `onOutput` throws while mapping a text delta | `mapStreamEvent` unaffected | `invokeSdk` rejects; chat shows `claude-error` |
| B14 | a new debug tool row | `debug-tools.json` regenerated (S25 τ) | `KNOWN_DEBUG_TOOL_NAMES` and the prompt-parity complement include it with no TS edit |
| B15 | `LspMethod` gains a variant | reify-lsp test: serde token == `as_str` for every `ALL` member; bindings regenerated | S27 λ's `lspRequest(method: LspMethod)` call sites type-check |

## §6 — Resolved design decisions

- **D1 — `bindings.ts` at `gui/src/bindings.ts`, raw generator output, committed;** gate test in
  the gui-feature pass; `REIFY_UPDATE_BINDINGS=1` regenerates (naming precedent:
  `REIFY_UPDATE_GOLDEN`, S25's `REIFY_UPDATE_DEBUG_TOOLS`). Rejected: generating at `npm` build
  time (the GUI test script must run without a Rust build — ruling).
- **D2 — commands move to `tauri_commands.rs`, the Builder to `ipc.rs`, both in the library under
  `cfg(feature = "gui")`;** main.rs keeps setup. Rejected: a `#[cfg(test)]` exporter inside main.rs
  (the bin is not a library test target; the list would stay split from `generate_handler!`).
- **D3 — exact pins** (`=2.0.0-rc.25`, `=0.0.12`); any bump is a reviewed regenerate event.
- **D4 — `ErrorHandlingMode::Throw`.** Rejected: the default `Result` wrapper (rewrites every call
  site, and folds transport errors into the typed error channel, tauri-specta #135).
- **D5 — per-field `Number` overrides for 64-bit integers.** Rejected: the global cast (silent
  precision loss for a future large value); narrowing Rust types to `u32` (Rust churn for no wire
  gain).
- **D6 — `Unknown` for `serde_json::Value`.** `any` rejected.
- **D7 — `default` beside every wire `skip_serializing_if`** (§2.9 serde phases).
- **D8 — float policy:** §7 OQ4. Rejected outright: `enable_lossless_floats()` (§2.9: runtime
  deep-copy remaps, `tsc` failures, specta #578).
- **D9 — foreign `Type` derives behind owner-crate `specta` features**, enabled on reify-gui's
  unconditional dependency line (Leo OQ6). Rejected: DTO mirrors in the GUI crate (a second shape
  home that drifts); gating on reify-gui's `gui` feature (a second reify-core feature set in the
  gui-feature pass).
- **D10 — `gui_state!` generates the delta event types and their registration** (one home for a
  field's classification and event name).
- **D11 — typed events everywhere, emitted through `Event::emit`;** `emit_typed` deleted. Rejected:
  generating listeners while Rust keeps string emits (wire-compatible but the Rust half unchecked).
- **D12 — TS-only shapes leave `types.ts`** for their consumers' modules (`MeshBuffers` beside the
  mesh conversion, toast types beside the toaster, `ExportFormat` beside the dialog); `types.ts`
  is deleted when empty.
- **D13 — one `BridgePort = ReturnType<typeof createBridge>`, App prop + `BridgeContext`, stores
  take explicit slices, one fake.** The factory owns the edit-order counter (critic C14). Rejected: module-level DI (`setBridge()`; a global mutable seam).
- **D14 — `isActivePhase` as an exhaustive switch in `gui/src/evalPhase.ts`.**
- **D15 — side panel as one entry list; overlays outside the grid;** today's track strings kept for
  the existing combinations.
- **D16 — one `SubscriptionSet` with per-row failure policy.**
- **D17 — `treeVersion` inside viewStateStore** (not a reactive Map rewrite: the maps are rebuilt
  wholesale; one version signal is the minimal reactive dependency).
- **D18 — one layout table with `kind` and `requiredInSaved`.**
- **D19 — every frontend handler validates at dispatch** against `frontend_schema ?? inputSchema`.
  Rejected (critic C4): trusting Rust for the engine-routed frontend halves — `handle_wait_for`
  passes `predicate` through unvalidated.
- **D20 — nine family modules over one resolve leaf;** families follow the measured inventory (the
  review named seven; `drive` and `editor` hold the 16 handlers its list did not place).
- **D21 — derived tool tables; the prompt's curated prose stays in the prompt** (§7 OQ2).
- **D22 — `is_geometry_value` from the compiled cell type;** the dead `manufacturing_ready`
  heuristic goes, `generatePurposeViews` stays for #6905.
- **D23 — a side-effect-free `validate_parameter_value` command, and `ValueData.requires_unit`
  for the focus-time edit seed (Leo OQ7).** Rejected: submit-and-report via `set_parameter`
  (consumes an edit order and races the queue for a value that may be refused); reusing
  `preview_parameter` (side effects); a TS coverage predicate over the ladders (a second home for
  the coverage rule).
- **D24 — sidecar: pure args builder and stream reducer; narrow catch.**
- **D25 — the permission envelope is fixed first (α)**, independent of generation.
- **D26 — `LspMethod` exported with `typ::<LspMethod>()`; `lsp_request` keeps `method: String`.**
  Lane routing on the parsed method is S27's (G4: `lane_for_method`); S27 λ types the TS side.
- **D27 — `navigate-to-source` goes with its last producer** (S25 ο deletes `mcp_tool_call`; S25
  §11 leaves the eleven undelegated reify-mcp tools unreachable from the GUI): the deleting diff
  removes the event, adapter, listener and doc row; κ does it only if ο has already landed.
  Re-adding it is the job of whichever change delegates `navigate_to_source`.
- **D28 — `get_active_fea_case` deleted in ι** (no caller, no filed consumer);
  `get_entity_identity_map` kept (#6905 ε).
- **D29 — four filed mechanicals stay as filed** (#8543, #8563, #8544 unchanged; #8562 subsumed by
  λ, whose generated types are the wire types' lower home).

## §7 — Rulings

Program ruling (Leo, 2026-10-12) as quoted in the header. Session rulings (Leo, on questions this
session put):

- **OQ1 (2026-10-14) — S26 extends S25's debug tool table.** After S25 τ #8955 lands, S26 ρ
  strengthens the frontend-routed rows' schemas and adds `advertised: false` rows in
  `debug_tools.rs`; G4 row becomes "S25 τ creates; S26 extends the frontend-routed rows".
- **OQ2 (2026-10-14) — the prompt's curated prose stays in `system-prompt.ts`;** membership is
  checked against the JSON and `NOT_ADVERTISED_TO_SIDECAR` becomes the derived complement.
- **OQ3 (2026-10-14) — drop `resolving`:** `EvalPhase = {idle, evaluating}`; a cross-reference note
  goes on solver-legibility-telemetry's θ leaf.
- **OQ4 (2026-10-14) — honest default + finite exceptions:** generated `number | null`; per-field
  `Number` override only where the Rust producer refuses non-finite values.
- **OQ5 (2026-10-15) — ι decoupled from S25 ο:** `mcp_tool_call`'s `Value` wrapped in a
  transparent `Unknown` newtype; ι depends on S25 θ #8944 and σ #8952 only (plus S26 η, θ).
- **OQ6 (2026-10-15) — `reify-core/specta` enabled on reify-gui's unconditional dependency line**
  (one reify-core build shared by both gate passes; CPU over disk).
- **OQ7 (2026-10-15) — the edit seed comes from a backend `ValueData.requires_unit` flag.**

Lead decisions within those rulings (not put to Leo; recorded so a later reader can contest them):
D2, D5, D12, D13's factory, D15's preserved track strings, D20's nine families, D27, D28; the
subsumptions #8562, #6843 (λ), #8184, #7858 (τ); the fold of the critic's C1–C20 (ratchet rows one
glob each; η, θ before ι; B5's Rust half in ι; `frontend_schema`; `treeVersion` ordering; macro
event idents; π's module order).

## §8 — Pre-conditions for activating

- **S25 leaves (real edges, §10):** α #8937, ε #8941 (θ), γ #8939 (η), θ #8944 (ι), λ #8947,
  μ #8948 (κ), κ #8946 (χ), ρ #8954 (σ), τ #8955 (ρ, υ). All pending at authoring. ι waits for
  neither S25 ο #8951 nor σ #8952 (Leo OQ5; §4.1).
- **S27 κ #8760** (`LspMethod`) before ν.
- **Other tasks' delivered checks this PRD flips** are corrected at decompose (three writes each:
  task `metadata.delivered_checks`, the owning PRD's `.yaml` and `.md` twins), found by a scan of
  every task's checks for the paths S26's leaves move, restructure or delete (`main.rs`,
  `types.rs`, `claude_bridge.rs`, `event_bus.rs`; `gui/src/bridge.ts`, `App.tsx`, `types.ts`,
  `debug/bridge.ts`, `debug/types.ts`, the stores `unitLadder.ts`, `autoViewGenerator.ts`,
  `layoutStore.ts`, `viewStateStore.ts`, `engineStore.ts`, `hooks/useLayoutPersistence.ts`,
  `panels/PropertyEditor.tsx`; `gui/sidecar/src/session.ts`, `system-prompt.ts`;
  `gui/test/visual/assertions.ts`; the `gui/src`, `gui/src/debug`, `gui/src/stores` and
  `gui/src/__tests__` directories). The scan (all 8,993 tasks, 2026-10-14) found 27 non-terminal
  checks on those paths; these flip:

  | task | check | correction |
  |---|---|---|
  | #6904, #6831 | `set_purpose` present in `main.rs` | paths += `gui/src-tauri/src/tauri_commands.rs` (ι moves the commands) |
  | #6905, #6834 | `fn get_entity_identity_map` present in `main.rs` | paths += `gui/src-tauri/src/tauri_commands.rs` |
  | #6905, #6834 | `regenerateAutoViews\(entityTree\(\), \[\]` absent in `App.tsx` | paths += `gui/src/app` (π moves the call; over the old path alone the check would go green vacuously) |
  | #6905, #6833 | `set_purpose` present in `gui/test/visual/assertions.ts` | paths += `gui/src-tauri/debug-tools.json` (υ replaces the hand name list with the derived one) |
  | #8951 (S25 ο) | `fn mcp_tool_call\(` absent in `main.rs` | paths += `gui/src-tauri/src/tauri_commands.rs` (if ι lands first, the check over `main.rs` alone goes green vacuously) |

  The other 22 (`ConstraintData`, `update="constraint-update"`, `createSelectiveDemandSync(`,
  `KNOWN_DEBUG_TOOL_NAMES`, `VALUE_SCENARIOS`, `undef-reason`, and S25's own) survive S26's edits.
- **Pending tasks whose text instructs the route S26 replaces** get a dated note at decompose
  (a check scan cannot see prose): #6898 (adds a command via `generate_handler!`, a `bridge.ts`
  wrapper and a `types.ts` mirror, and an `epoch: u64` that needs rule 2's override), #6904
  (registers `set_purpose`/`clear_purpose` in `generate_handler!`), #6905 (its δ step edits
  `KNOWN_DEBUG_TOOL_NAMES` by hand), #6732 (hand-edits `KNOWN_DEBUG_TOOL_NAMES`), #7807
  (`types.ts::ExportFormat` + `bridge.ts`), #6961 (new `MeshData` fields; `unitLadder.ts`,
  `App.tsx`), #8951 (deletes `mcp_tool_call` from `tauri_commands.rs` if ι landed first, and
  with it `navigate-to-source` — D27), #8955 (the REST-only rows, §4.6), #8760 (ν adds derives to
  `LspMethod`), and solver-legibility-telemetry's θ leaf (OQ3). Each note states the post-ι route:
  register through `ipc::builder()`, regenerate `bindings.ts`, extend the port, no hand mirror.
- No grammar work: `grammar_confirmed = true` on every leaf. No `.ri` fixture is added: every
  signal is a GUI, debug-MCP, IPC or test-suite observation over existing examples
  (`examples/buckling_column_smoke.ri` for β's debug-MCP signal).

## §9 — Cross-PRD relationship and seam owners

| seam | owner | this PRD's relation |
|---|---|---|
| `gui/src-tauri/src/types.rs` as the generation source | **S26** (G4; shared with S25) | ι adds derives; S25 θ/σ land their enums first |
| `KNOWN_DEBUG_TOOL_NAMES`, `system-prompt.ts` tool tables | **S26** (G4) | υ derives them from `debug-tools.json` |
| `gui/src-tauri/debug-tools.json` (tool table export) | S25 (τ #8955) | consumed by σ, υ; ρ extends its frontend-routed rows (§7 OQ1) |
| S25 θ/σ wire enums; λ/μ event set; ο `sync_editor_buffer`; ρ handler set | S25 | consumed (edges in §10) |
| `TauriSessionEvents` (main.rs) | S25 λ | κ edits only its method bodies (`Event::emit`) |
| `engine/entity_tree.rs`, `engine/units.rs` (moved by S25 γ, α) | S25 | η, θ edit them after the move |
| `LspMethod` (S27 κ #8760); `lane_for_method`, `lspClient.ts` | S27 | ν, after κ, adds `Serialize`/`Deserialize` + `specta::Type` (behind reify-lsp's new `specta` feature) to `LspMethod` — a sequential second editor of `method.rs`, named in a note on #8760 and a cross-reference to the lead; S27 λ #8777 (held on S26) consumes the export and is released by this session |
| `gui/src-tauri/src/debug.rs` (`DebugBridge`) | S25 π #8953 also edits it | κ types `debug-request`; file lock, whichever lands second rebases (no edge) |
| `docs/debug-mcp-contract.md` | S25 (G4) | after S25 τ rewrites it, σ adds the dispatch-validation paragraph and υ the derived-tables paragraph ("Defining a new tool" + drift guard); sequential, named in a cross-reference to the lead |
| `gui-purpose-surface` #6905 | that PRD | keeps `generatePurposeViews` and `get_entity_identity_map`; its delivered checks are widened at decompose |
| #6519 (wider DSL unit registry), #8594 | follow-ups of units-length | θ removes the frontend grammar #6519's frontend half would widen; #6519 depends on θ (note appended) |
| #6843 (narrow `ConstraintData.status` in TS) | gui-on-demand follow-up | subsumed by λ. S25's 2026-10-14 note ("not subsumed" by S25 θ, "its #6898 premise stands") concerns θ, which makes no TS edit; λ is the TS edit, and with generation #6898's vocabulary widening becomes a Rust variant plus regeneration, so #6843's wait-for-the-vocabulary premise dissolves. λ's exhaustive switches deliver its "make the fallbacks loud" half |
| `solver-legibility-telemetry.md` OQ-4 (`phase = 'resolving'` for its θ chip) | that PRD | μ drops `resolving` (Leo OQ3); re-adding it is one `EvalPhase` variant there, and `isActivePhase`'s exhaustive switch points `tsc` at every consumer — a note goes on that PRD's θ leaf at decompose |
| #8184, #7858 (debug/bridge.ts h14 partition) | follow-ups | subsumed by τ |
| `scripts/check_event_inventory.sh` | GR-016 lint | unchanged: its reverse pass already sees `event_name = "…"` literals |
| INV-GUI-1 (`gui_state!` classification) | gui-state-sync | κ extends each macro row with its event type idents (`update_event`/`remove_event`), so classification, wire name and event type stay in one row; the registry row's text is unchanged |
| PLINECITE `--rename` / Pointer same-diff obligations | S24 (#8822, #8821) | every S26 leaf that moves a file under PLINECITE scope after #8822 lands runs `plinecite-baseline-gen --rename`; no `docs/invariants.md` Pointer names a gui/src symbol today |

## §10 — Decomposition plan

Labels Greek; task ids filed 2026-10-15 (pass 2); no held leaf. Every leaf carries `metadata.x_finding_key` (list),
`metadata.x_finding_run = "review-all-reify-20261010"`, `metadata.source = "review-all"`, priority
= the highest severity among its keys (medium where it carries none), `grammar_confirmed = true`.
"Rows" names the Appendix A ratchet rows the leaf adds to `tests/infra/adoption-ratchet.manifest`
at its post-extraction count, named in its done criteria (INV candidate 5). Every leaf runs
`scripts/gui-test.sh` (and the gui-feature cargo pass where it touches Rust). After ι lands, every
leaf that changes a wire type, command or event regenerates `gui/src/bindings.ts` in its diff
(rule 4a). Same-diff (S24): after #8822 lands, a moved file under PLINECITE scope runs
`plinecite-baseline-gen --rename`.

**Phase 0 — before generation**

- **α #9062 — permission decisions reach the sidecar.** `claudePermissionDecision` sends `{decision:
  {…}}`; `claudeBridge.test.ts`'s flat-shape pin is corrected. B5 (TS half). Signal: in a GUI
  session a permission prompt's Allow unblocks the pending tool call (the command returns `Ok`);
  ι adds the IPC-level Rust test. Keys: none (found at authoring, §2.1). medium.
- **β #9063 — one side-panel entry list.** §4.5. B6. Rows `side-panel-track-iife`. Signal: on
  `examples/buckling_column_smoke.ri` with chat open, after buckling frames arrive, debug MCP
  `get_layout_metrics` on the chat panel reports height ≥ 160 px and the side panel's children
  equal its tracks. Keys: **fk-2be83576bc8b** (high, gate-dep #8612).
- **γ #9064 — one subscription set in `initApp`.** §4.5. B7. Rows `app-unsub-slot`. Signal: after an
  init failure and Retry, exactly one `debug-request` listener is live (App test over the real
  `initApp`), where today the retry leaks one. Keys: **fk-559caca373a7**. medium.
- **δ #9065 — viewStateStore owns tree reactivity.** §4.5. B8, B8' (the esc-6045-1 App regression test
  kept, passing with no `treeGeneration`). Rows `tree-generation-app`, `tree-generation-stores`.
  Signal: the first post-mount visibility toggle reaches `syncDemand` and the DesignTree eye with
  no App-threaded counter. Keys: **fk-b2129b18b706**. medium.
- **ε #9066 — one layout dimension table with `reset()`.** §4.7. B9. Rows `layout-default-import-debug`.
  Signal: debug MCP `resize_panes {problemsHeight: 400}`, a collapse toggle, then
  `reset_app_state` → `get_local_storage('reify-panel-layout')` shows `problemsHeight: 160`,
  `problemsCollapsed: true`. Keys: **fk-c8938e374057**. medium.
- **ζ #9067 — sidecar `invokeSdk` split; catch narrowed.** §4.9. B13. Rows `sidecar-args-literal`.
  Signal: a stream-json event whose mapping throws reaches the chat as `claude-error` instead of a
  silently missing reply. Keys: **fk-39619f02ae9c**. medium.
- **η #9068 — `is_geometry_value` on `EntityTreeNode`; no regex over Type Display.** §4.8. Rows
  `type-name-regex`. Signal: in the GUI, a design whose `let` binds a Solid shows it hidden by
  default and a `Real` let shown (debug MCP `store_state` / DesignTree eye state);
  `engine_tests.rs`'s M5 flange consumed-intermediate pin stays green. Depends: S25 γ #8939.
  Keys: **fk-a03c96e5733d**. medium.
- **θ #9069 — the backend validates parameter literals; `requires_unit` seeds the editor.** §4.8. B12,
  B12', B12''. Rows `ts-quantity-grammar-stores`, `ts-quantity-grammar-panels`. Signal: in the GUI,
  `10` typed into a Length cell is refused inline with the backend's message, `5 mm` is accepted,
  and focusing a Length cell seeds `120mm`. Depends: S25 α #8937, ε #8941 (`build_values` moves to
  `projection.rs`). Keys: none (the stream remedy's
  second half of fk-a03c96e5733d). medium. #6519 depends on θ.

**Phase 1 — the generated contract**

- **ι #9070 — commands into the library; one Builder; `bindings.ts` for commands and their types; gate
  test.** §4.1. B1, B1', B4 (phase-identity test), B5 (Rust half). Wraps `mcp_tool_call`'s `Value`
  if S25 ο has not deleted it (OQ5); deletes `get_active_fea_case`; records the reify-core compile
  delta (OQ6) and, in its task, that S25 θ's own chain is its only S25 wait. Signal:
  `cargo test -p reify-gui --features gui bindings_ts_is_current` passes on the committed file and
  fails after a scratch rename of a command argument; `scripts/gui-test.sh` typechecks the
  generated file; the GUI runs unchanged. Depends: S25 θ #8944; η, θ. Keys: none
  (contributes to fk-7262d24da57d). medium.
- **κ #9071 — every event typed and registered once.** §4.2. B2, B3. Deletes `emit_typed` (and updates
  any identifier list that names it, e.g. the INV-GUI-2 scanner's `PRIVATE_EMIT_IDENTIFIERS` if S25
  ο has not yet deleted that scanner). If `mcp_tool_call` still exists, its `navigate-to-source`
  emit closure (built for `mcp_context.rs::TauriToolContext`, an S25 file) stays an untyped string
  emit until S25 ο deletes it: `untyped-emit`'s ceiling is 1 citing #8951, else 0. Rows
  `untyped-emit`. Signal: `bindings.ts`'s `events` lists every channel in
  `docs/gui-event-channels.md` (a gui-feature test compares the registered names with both
  tables' column 1); a payload that fails to serialise still yields `serialization-error`.
  Depends: ι, S25 λ #8947, μ #8948. medium.

**Phase 2 — adoption**

- **λ #9072 — the TS side imports wire types from `bindings.ts`.** Deletes the hand mirrors in `types.ts`
  (and its 24 `types.rs` prose mentions), `RawConstraintData`/`CONSTRAINT_STATUSES`/
  `convertRawConstraint`, the inline `import()` types; TS-only shapes move beside their consumers
  (D12) except the unions μ generates (`VisibilityState` stays put until μ replaces it, so its 76
  sites move once); exhaustive switches over `ConstraintStatus` in ConstraintPanel/StatusBar (no
  permissive `default`, including ChatPanel's verdict filter, #6843's third consumer). Subsumes
  **#8562** (fk-0b62433f5807) and **#6843** (§9; cancelled at decompose with reason naming λ).
  Rows `types-rs-prose-mirror` — removed again in the same diff if λ deletes `types.ts` (a row
  whose glob matches no file fails the ratchet). Signal: `scripts/gui-test.sh` green with `git grep -c "types.rs"
  gui/src/types.ts` 0 (or the file gone); a scratch rename of a `ValueData` field in Rust fails
  `tsc` after regeneration. Depends: ι. Keys: **fk-7262d24da57d**, **fk-0b62433f5807**. medium.
- **μ #9073 — remaining closed-set fields as wire enums; one `isActivePhase`.** §4.3. B11. Rows
  `phase-literal-app`, `-stores`, `-debug`, `-viewport`. Signal: debug MCP `wait_for_idle` during
  an evaluation waits and returns ok (no `engine_phase` error path remains); `tsc` fails on a
  scratch `phase === 'resolving'`. Depends: λ. Keys: none (contributes to fk-7262d24da57d). medium.
- **χ #9074 — `JointDescriptor.kind`/`.dimension` as wire enums.** §4.3. Signal: MechanismPanel's
  prismatic/revolute rendering unchanged on the mechanism examples; `tsc` rejects a misspelt joint
  kind. Depends: λ, S25 κ #8946. low.
- **ν #9075 — `LspMethod` in `bindings.ts`.** reify-lsp `specta` feature; per-variant serde renames equal
  `as_str`, pinned by a reify-lsp test over `ALL`; `.typ::<LspMethod>()`. B15. Releases S27 λ
  #8777. Signal: `bindings.ts` exports `LspMethod` with exactly `LspMethod::ALL`'s tokens. Depends:
  ι, S27 κ #8760. low.

**Phase 3 — the port**

- **ξ #9076 — `gui/src/bridge/` port modules over the bindings; `createBridge()`; one fake.** §4.4.
  Deletes `validatePayload`, `KEYS_*`, the seven shape guards and `gui/src/bridge.ts`;
  `gui/src/bridge/index.ts` also re-exports today's free functions bound to one default
  `createBridge()` instance, so the eight importing modules build unchanged until ο removes the
  re-exports; `bridgeMockCoverage.test.ts`'s allowlist drops the deleted names;
  `eventChannelConsumerCoverage.test.ts` reads the generated `events` instead of `bridge.ts`
  source. Signal: `scripts/gui-test.sh` green;
  no `invoke(`/`listen(` call in `gui/src/bridge/` outside the generated bindings it imports.
  Depends: κ, λ, α. Keys: none (contributes to fk-252aa009dcdc). medium.
- **ο #9077 — App, engineStore and every IPC-touching descendant take the port.** §4.4. Deletes the 8
  wholesale bridge mocks, `bridgeMockContract.ts`/`.test.ts`, `bridgeMockCoverage.test.ts`, the
  `viewPersistence` namespace import, `initApp`'s mock-defensive code. Rows `bridge-wholesale-mock`,
  `tauri-api-direct-app`, `-stores`, `-viewport`. Signal: `scripts/gui-test.sh` green with
  App.test.tsx's module-level `vi.mock` count down to the rendering stubs (viewport, editor, file
  tabs); a scratch new port member fails `tsc` in the fake until added. Depends: ξ, β, γ, δ. Keys:
  **fk-252aa009dcdc** (high, gate-dep #8612).
- **π #9078 — App.tsx partitioned into `gui/src/app/`.** §4.5 (module order, per-module deps, `AppRefs`).
  Rows `phase-literal-app-modules`, `tauri-api-direct-app-modules`, `tauri-api-direct-app`
  re-checked at 0. Signal: `scripts/gui-test.sh`
  green with no test body edited beyond import lines; every module under ~1,500 lines; the import
  order holds (recorded in the task). Depends: ο, μ. Keys: none. medium.

**Phase 4 — the debug contract**

- **ρ #9079 — frontend-facing rows of the debug tool table** (Leo OQ1): strengthened schemas on the
  frontend-routed rows, `frontend_schema` on the seven rewriting rows, `advertised: false` rows for
  the REST-only handlers (§4.6). Signal: `tools/list` shows the strengthened schemas (e.g.
  `dom_query.testId` `minLength: 1`); `debug-tools.json` regenerated; the exported table's
  `wait_for` row carries the predicate `oneOf`. Depends: S25 τ #8955. medium.
- **σ #9080 — one dispatch-time validator; hand guards and casts gone.** §4.6. B10, B10'. Retires the
  BOUNDARY RULE comment, `TYPE_GUARDED_RESOLVER_TOOLS` and its per-copy rows; `initDebugBridge`
  takes the port's host slice (`onDebugRequest`, `debugResponse` over the generated
  `events.debugRequest` / `commands.debugResponse`), so ο's App test drives it through the fake.
  Rows `debug-string-guard`, `debug-param-cast`, `tauri-api-direct-debug`. Signal: debug MCP
  `dom_query {"testId": 7}` → `{error: "testId must be a string"}`; `constructor` → unknown
  command; `wait_for {"predicate": {"kind": "nope"}}` refused before any polling; the visual
  harness green. Depends: ρ, ε, κ, ξ, S25 ρ #8954. Keys: **fk-e0f49abf4b34** (high, gate-dep #8612).
- **τ #9081 — `buildHandlers` split into family modules over `resolve.ts` (move-only).** §4.6. h14
  measurement recorded. Subsumes **#8184**, **#7858**. No new rows (σ's rows glob
  `gui/src/debug/**/*.ts`). Signal: `scripts/gui-test.sh` and the visual harness green, no test
  body edited beyond imports; `gui/src/debug/bridge.ts` holds only `buildHandlers`' merge and
  `initDebugBridge`. Depends: σ. medium.
- **υ #9082 — tool-name and fixture tables derived from `debug-tools.json`.** §4.6. B14. Deletes
  `tauriSource.ts`. Rows `rust-source-reader-src`, `rust-source-reader-visual`. Signal: adding a
  row to the Rust table and regenerating updates `KNOWN_DEBUG_TOOL_NAMES` and the prompt-parity
  complement with no TS edit (assertions.test.ts, sidecarPromptParity green). Depends: ρ, S25 τ
  #8955. medium.

**Close**

- **ω #9083 — PRD close stamp.** Depends on every other leaf (a cancelled sibling counts as satisfied).
  Deliverable: the terminal Status marker naming the landed leaf ids, the AS-AUTHORED paragraph and
  LIVE/AS-AUTHORED map here and in the capability manifest (overlay "PRD terminal status"). low.

DAG summary: α → ξ; {β, γ, δ} → ο; ε → σ; S25 γ → η → ι; S25 {α, ε} → θ → {ι, #6519}; S25 θ → ι →
{κ, λ, ν}; S25 {λ, μ} → κ; λ → {μ, χ}; S25 κ → χ; S27 κ → ν → S27 λ #8777; {κ, λ, α} → ξ → ο →
π; μ → π; S25 τ → ρ → {σ, υ}; {ρ, ε, κ, ξ, S25 ρ} → σ → τ; every leaf → ω. Gate #8612 ← β, ο, σ.

## §11 — Out of scope

- The sidecar ↔ Rust stdio protocol (`gui/sidecar/src/types.ts` vs `claude_bridge.rs`
  `InboundMessage`/`OutboundMessage`): a different wire, hand-mirrored; no finding covers it.
- Typing the LSP protocol payloads (`diagnostics`, `lsp-log` members stay `Unknown`).
- Routing `lsp_request` on a parsed `LspMethod` (S27).
- The engine-routed debug tools' Rust-side validation and the `debug-tools.json` format beyond
  the two keys ρ adds (`frontend_schema`, `advertised`) (S25).
- `gui/sidecar/src/__tests__/discover-mcp-tools.ts` (an orphaned Rust-source reader over
  reify-mcp; S25 ν/ξ own the registry) — cross-reference to the lead.
- `EvalStatusInfo.phase` in reify-mcp (`mcp_context.rs` hard-codes `"idle"`; S25 σ's MCP enums).

## §12 — Open (tactical) questions

1. β: whether `SolverProgressOverlay` moves to App's overlay layer or stays a `position: fixed`
   sibling outside the grid (either satisfies tracks == children).
2. ι: placement of the 20 `Number` overrides on fields of `reify_core` types (the overrides live
   with the owner-crate derive).
3. κ: `ClaudeReady` as an empty named struct is an unprobed specta shape — κ probes it; if it does
   not render `{}`, the event becomes a unit struct (`null`) and its listener changes in the same
   diff.
4. ξ: module names inside `gui/src/bridge/` (the four-way split is the contract; names are not).
5. π: whether `fileWorkspace.ts` and the view-persistence saver are one module or two (measure).
6. σ: the exact uniform message strings; tests pinning today's wording are updated in σ's diff.
7. τ: whether `fixtures` and `editor` families merge (4 + 5 handlers) if either stays under ~150
   lines.

## Gate walk (author mode)

- **G1** — every mechanism names its consumer: `bindings.ts` → the port modules (ξ) and every TS
  consumer (λ); the port → App/stores (ο); the validator → `initDebugBridge` (σ); `is_geometry_value`
  → `defaultVisibilityFor`; `validate_parameter_value` → `PropertyEditor`; `LspMethod` export →
  S27 λ #8777; strengthened schemas → σ's validator and `tools/list`; `treeVersion` → the demand
  sync and DesignTree. No orphan.
- **G2** — every leaf names a GUI, debug-MCP, IPC or gate observation (§10); ι, κ, ξ, π, τ are
  intermediates whose consumers are named, each with its own concrete check.
- **G3** — substrate verified: specta/tauri-specta behaviour by probe build (§2.9); tauri's `test`
  module exists in 2.11.2 (`tauri-2.11.2/src/test/`, feature `test`); `is_geometry_valued`,
  `parse_value_string_for_cell`, `resolve_known_cell_type`, `parse_cell_id` exist (engine.rs,
  moving with S25 α/γ); S25/S27 producers are filed and pending (edges).
- **G4** — seams in §9; no reciprocal "the other owns it"; the one shared-artefact extension
  (ρ on S25's table) is ruled by Leo (§7 OQ1); the sequential second edits of S27's `LspMethod`
  (ν), S25's `debug.rs` (κ) and S25's contract doc (σ, υ) are named in §9.
- **G5** — B + H: contracts §4, boundary tests §5.
- **G6** — premises corrected in §2 (mock count, guard size, purpose path, schema strength, wire
  surface, the permission defect); no numeric bound is asserted.
- **G7** — INV-SF family: no new placeholder or silent fallback is introduced (ζ removes one: the
  swallowing catch); the debug validator refuses loudly in-band.
- **META** — after §7's rulings, yes.

## Appendix A — Adoption-ratchet rows

Manifest grammar `pattern-id | ERE | path | ceiling | owner` (`tests/infra/adoption-ratchet.manifest`
header): **one glob per row** (`adoption_ratchet.py` globs `row.path_glob` once and fails a row whose
glob matches no file), unique ids, no Rust-attribute subjects (a line whose first non-blank
character is `#` never counts, so `#[serde(…)]` cannot be ratcheted — ι's B4 test covers that rule), no `\b`/`\d`/backslash-in-brackets/`(?…)` (the engine's
rejection rules). Counts are `git grep -E -c` at `1b06f93151`, comment lines included
(`adoption_ratchet.py` counts every line whose first non-blank character is not `#`, so `//`
comments count — leaves word their comments accordingly). A glob that matches nothing yet is added
by the leaf that creates its files. "(at owner)" = measured by the owner leaf at its base and
recorded in its task; the ceiling binds.

| pattern-id | ERE | path | today | ceiling | owner |
|---|---|---|---|---|---|
| side-panel-track-iife | `grid-template-rows': \(\(\) =>` | `gui/src/App.tsx` | 1 | 0 | β |
| app-unsub-slot | `let [a-zA-Z]*[uU]nsub[:;= ]` | `gui/src/App.tsx` | 11 | 0 | γ |
| tree-generation-app | `[tT]reeGeneration` | `gui/src/App.tsx` | 10 | 0 | δ |
| tree-generation-stores | `[tT]reeGeneration` | `gui/src/stores/*.ts` | 8 | 0 | δ |
| layout-default-import-debug | `DEFAULT_[A-Z_]+(WIDTH\|HEIGHT\|COLLAPSED)` | `gui/src/debug/*.ts` | (at owner) | 0 | ε |
| sidecar-args-literal | `--output-format` | `gui/sidecar/src/*.ts` | 2 (session.ts) | the claudeArgs.ts count only (named) | ζ |
| type-name-regex | `TYPE_NAME_RE` | `gui/src/stores/*.ts` | (at owner) | 0 | η |
| ts-quantity-grammar-stores | `buildQuantityRe\|NUMBER_RE\|acceptsBareNumber\|quantityUnitAlphabet` | `gui/src/stores/*.ts` | (at owner) | 0 | θ |
| ts-quantity-grammar-panels | same | `gui/src/panels/*.tsx` | (at owner) | 0 | θ |
| untyped-emit | `emit_typed\(\|\.emit\([^,)]*,\|\.emit\($` | `gui/src-tauri/src/*.rs` | (at owner) | 1 while `mcp_tool_call`'s closure exists (#8951), else 0 | κ |
| types-rs-prose-mirror | `types\.rs` | `gui/src/types.ts` | 24 | 0 (or the file deleted, row removed) | λ |
| phase-literal-app | `phase (===\|!==) '` | `gui/src/App.tsx` | (at owner) | 0 | μ |
| phase-literal-stores | same | `gui/src/stores/*.ts` | (at owner) | 0 | μ |
| phase-literal-debug | same | `gui/src/debug/**/*.ts` | (at owner) | 0 | μ |
| phase-literal-viewport | same | `gui/src/viewport/*.tsx` | (at owner) | 0 | μ |
| bridge-wholesale-mock | `vi\.mock\(['"](\.\./)+bridge['"]` | `gui/src/__tests__/**/*.ts*` | 27 (incl. the guard's string fixtures, deleted with it) | 0 | ο |
| tauri-api-direct-app | `^import \{[^}]*\} from ['"]@tauri-apps/api/(core\|event)` | `gui/src/App.tsx` | (at owner) | 0 | ο |
| tauri-api-direct-stores | same | `gui/src/stores/*.ts` | (at owner; `import type` lines do not match) | 0 | ο |
| tauri-api-direct-viewport | same | `gui/src/viewport/*.tsx` | (at owner) | 0 | ο |
| phase-literal-app-modules | `phase (===\|!==) '` | `gui/src/app/*.ts*` | — | 0 | π |
| tauri-api-direct-app-modules | `^import \{[^}]*\} from ['"]@tauri-apps/api/(core\|event)` | `gui/src/app/*.ts*` | — | 0 | π |
| tauri-api-direct-debug | same | `gui/src/debug/**/*.ts` | (at owner) | 0 | σ |
| debug-string-guard | `typeof [a-zA-Z.]+ !== 'string'` | `gui/src/debug/**/*.ts` | 23 | the validator's own count (named) | σ |
| debug-param-cast | `params\.[a-zA-Z]+ as ` | `gui/src/debug/**/*.ts` | (at owner) | 0 | σ |
| rust-source-reader-src | `readTauriSrc\|readFileSync\([^)]*\.rs` | `gui/src/__tests__/*.ts` | (at owner) | 0 | υ |
| rust-source-reader-visual | same | `gui/test/visual/*.ts` | 0 (a guard against new readers) | 0 | υ |

(`|` inside an ERE is escaped as `\|` only for this Markdown table; the manifest row carries the
bare `|`. `untyped-emit` counts single-line two-argument `.emit(` calls — the string-name Emitter
form — and misses a call split across lines; κ's registered-names test is the behavioural cover.
The LSP editor modules' direct `invoke` is S27 λ's, outside these globs.)
