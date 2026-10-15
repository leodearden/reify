# Capability manifest — gui-frontend-wire-bindings-bridge-port.md

Authored 2026-10-14/15 with the PRD (stream S26 of program `review-all-reify-20261010`).
Machine-readable twin: `gui-frontend-wire-bindings-bridge-port.capability-manifest.yaml` (task ids
stamped at `commit_planning`; this table is generated from it). Binds each leaf's signal to
substrate evidence (G3 + G6 mechanized). No leaf is held: every upstream producer (S25 #8937,
#8939, #8941, #8944, #8946, #8947, #8948, #8954, #8955; S27 #8760) is a filed task wired as a real
dependency.

**Code anchors** verified against main `1b06f93151` (2026-10-14). Cite-by-symbol; re-locate lines
at implementation time.

**Substrate probes.** The specta/tauri-specta premises (§2.9 of the PRD) were established by a
scratch crate built outside the repo against the repo's `Cargo.lock` (tauri 2.11.2, specta
=2.0.0-rc.25, tauri-specta =2.0.0-rc.25, specta-typescript =0.0.12), with real exports and `tsc`
5.9.3 under the repo's options, 2026-10-14. tauri's `test` module (ι's permission IPC test) exists
in tauri-2.11.2 (`src/test/`, feature `test`). No `.ri` fixture is added; `grammar_confirmed =
true` on every leaf.

**Delivered-check polarity.** Every `expect: present` pattern matched 0 times and every
`expect: absent` pattern matched ≥ 1 at main (`git grep -E -c` over the listed paths, re-measured
mechanically from the YAML on 2026-10-15). Paths that do not exist yet (`gui/src/bridge/`,
`gui/src/app/`, `gui/src/bindings.ts`, `gui/src/debug/resolve.ts`, `gui/src-tauri/debug-tools.json`)
are created by the leaf itself or by an upstream dependency, so no check is vacuous on arrival.
`α`'s pattern accepts both `{ decision }` and `{ decision: … }` and includes `bindings.ts`, so it
survives ξ moving the call into the generated layer; `β`'s check is on the builder function, so it
survives π's partition.

**Adoption ratchets (INV candidate 5).** Rows in `tests/infra/adoption-ratchet.manifest`, each
added by its owner leaf at its post-extraction count; the EREs and ceilings are the PRD's
Appendix A (one source; this manifest does not restate them).

## Per-leaf bindings

| leaf | capability | binding | verdict | delivered check |
|---|---|---|---|---|
| α | permission-envelope | producer-self — main.rs::claude_permission_decision takes `decision: PermissionDecisionArgs`; bridge.ts sends the fields flat (PRD §2.1); 0 matches at 1b06f93151; bindings.ts included so the check survives ξ moving the call into the generated layer | PASS | grep `\{ ?decision[ :}]` present in `gui/src/bridge.ts, gui/src/bridge, gui/src/bindings.ts` |
| β | side-panel-track-builder | producer-self — pure builder in gui/src/app (PRD §4.5); 0 matches at 1b06f93151; survives π's partition wherever the module lands | PASS | grep `export function sidePanelTracks` present in `gui/src/app` |
| β | track-iife-gone | producer-self — 1 match in App.tsx at 1b06f93151 (the inline grid-template-rows IIFE) | PASS | grep `grid-template-rows': \(\(\) =>` absent in `gui/src/App.tsx` |
| γ | unsub-slots-gone | producer-self — 1 match (the slot the entry teardown omits) at 1b06f93151 | PASS | grep `let debugBridgeUnsub` absent in `gui/src/App.tsx` |
| δ | store-owned-version | producer-self — 0 matches at 1b06f93151 | PASS | grep `treeVersion` present in `gui/src/stores/viewStateStore.ts` |
| δ | demand-sync-param-gone | producer-self — 1 match (createSelectiveDemandSync's third parameter) at 1b06f93151 | PASS | grep `treeGeneration: \(\) => unknown` absent in `gui/src/stores/engineStore.ts` |
| ε | layout-table | producer-self — 0 matches at 1b06f93151 | PASS | grep `LAYOUT_DIMENSIONS` present in `gui/src/stores/layoutStore.ts` |
| ε | reset-app-state-calls-reset | producer-self — 0 matches at 1b06f93151 (reset_app_state calls five setters) | PASS | grep `layout\.reset\(\)` present in `gui/src/debug` |
| ζ | pure-stream-reducer | producer-self — 0 matches at 1b06f93151 | PASS | grep `export function mapStreamEvent` present in `gui/sidecar/src` |
| ζ | pure-args-builder | producer-self — 0 matches at 1b06f93151 | PASS | grep `export function buildClaudeArgs` present in `gui/sidecar/src` |
| η | backend-geometry-fact | producer-self — is_geometry_valued(&Type) exists in engine.rs (moves to engine/entity_tree.rs with S25 γ #8939, upstream); field absent at 1b06f93151 | PASS | grep `is_geometry_value` present in `gui/src-tauri/src/types.rs` |
| η | geometry-regex-gone | producer-self — 1 match at 1b06f93151 | PASS | grep `const GEOMETRY_TYPE_NAME_RE` absent in `gui/src/stores/autoViewGenerator.ts` |
| θ | validate-command | producer-self — parse_cell_id, resolve_known_cell_type, parse_value_string_for_cell exist (engine.rs; units.rs after S25 α #8937, upstream); command absent at 1b06f93151 | PASS | grep `fn validate_parameter_value` present in `gui/src-tauri/src` |
| θ | ts-grammar-gone | producer-self — 1 match at 1b06f93151 | PASS | grep `export function buildQuantityRe` absent in `gui/src/stores/unitLadder.ts` |
| θ | requires-unit-fact | producer-self — dimension_requires_unit exists in engine.rs (units.rs after S25 α #8937); field absent at 1b06f93151; Leo OQ7 | PASS | grep `pub requires_unit: bool` present in `gui/src-tauri/src/types.rs` |
| ι | committed-bindings | producer-self — specta/tauri-specta rc.25 export probed against tauri 2.11.2 (PRD §2.9); upstream S25 θ #8944, σ #8952 and S26 η, θ (not S25 ο: Leo OQ5) | PASS | path `gui/src/bindings.ts` present |
| ι | gate-test | producer-self — 0 matches at 1b06f93151 | PASS | grep `fn bindings_ts_is_current` present in `gui/src-tauri/src` |
| ι | one-command-list | producer-self — 1 match (tauri::generate_handler! in main.rs) at 1b06f93151 | PASS | grep `tauri::generate_handler!` absent in `gui/src-tauri/src/main.rs` |
| κ | typed-events | producer-self — event_name override and macro literal fragment probed (PRD §2.9); upstream ι, S25 λ #8947, μ #8948 | PASS | grep `tauri_specta\(event_name` present in `gui/src-tauri/src` |
| κ | string-emit-helper-gone | producer-self — 1 match at 1b06f93151 | PASS | grep `pub fn emit_typed` absent in `gui/src-tauri/src` |
| λ | hand-verdict-validator-gone | producer-self — 1 match (#7465's convertRawConstraint) at 1b06f93151 | PASS | grep `export function convertRawConstraint` absent in `gui/src` |
| λ | inline-store-import-gone | producer-self — 1 match (types.ts PersistentViewState) at 1b06f93151 | PASS | grep `import\('\./stores/autoViewGenerator'\)` absent in `gui/src/types.ts` |
| μ | one-active-predicate | producer-self — 0 matches at 1b06f93151 | PASS | grep `export function isActivePhase` present in `gui/src` |
| μ | phase-enum | producer-self — EvalActivity {Evaluating, Idle} in eval_queue.rs is the source; Leo OQ3 drops resolving | PASS | grep `pub enum EvalPhase` present in `gui/src-tauri/src` |
| χ | joint-dimension-enum | producer:S25 κ #8946 (descriptor read through S6 JointKind), upstream | PASS | grep `pub enum JointDimension` present in `gui/src-tauri/src` |
| ν | lsp-method-union | producer:S27 κ #8760 (LspMethod), upstream; Builder::typ::<T>() of an unreferenced renamed enum probed (PRD §2.9) | PASS | grep `LspMethod` present in `gui/src/bindings.ts` |
| ξ | port-type | producer-self — 0 matches at 1b06f93151 | PASS | grep `export type BridgePort` present in `gui/src` |
| ο | wholesale-mocks-gone | producer-self — 27 matching lines across the 8 files at 1b06f93151 (comments included) | PASS | grep `vi\.mock\(['"](\.\./)+bridge['"]` absent in `gui/src/__tests__` |
| ο | source-parsing-guard-gone | producer-self — 1 match at 1b06f93151 | PASS | grep `export function extractBridgeFactoryKeys` absent in `gui/src/__tests__` |
| π | scene-module | producer-self — new module (PRD §4.5) | PASS | path `gui/src/app/scene.ts` present |
| ρ | unadvertised-rows | producer:S25 τ #8955 (debug_tools.rs + committed debug-tools.json), upstream; Leo OQ1 | PASS | grep `"advertised": false` present in `gui/src-tauri` |
| ρ | strengthened-schemas | producer:S25 τ #8955, upstream; 0 minLength keywords in the tool schemas at 1b06f93151 | PASS | grep `"minLength"` present in `gui/src-tauri` |
| σ | validator-module | producer-self — 0 matches at 1b06f93151 | PASS | grep `export function validate` present in `gui/src/debug` |
| σ | guard-census-gone | producer-self — 1 match at 1b06f93151 | PASS | grep `export const TYPE_GUARDED_RESOLVER_TOOLS` absent in `gui/src/debug` |
| τ | resolve-leaf | producer-self — new module (PRD §4.6) | PASS | path `gui/src/debug/resolve.ts` present |
| υ | hand-tool-name-literal-gone | producer:S25 τ #8955 + ρ, upstream; 1 match at 1b06f93151 | PASS | grep `export const KNOWN_DEBUG_TOOL_NAMES: ReadonlySet<string> = new Set\(\[` absent in `gui/test/visual/assertions.ts` |
| υ | rust-source-reader-gone | producer-self — 1 match at 1b06f93151 (readers retired by S25 θ and τ, upstream) | PASS | grep `export function readTauriSrc` absent in `gui/src/__tests__` |
| ω | terminal-status | producer-self — the committed header | PASS | manual — the close stamp is a docs edit verified by reading the header |
