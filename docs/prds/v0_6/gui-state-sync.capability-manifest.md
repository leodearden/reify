# Capability manifest — gui-state-sync

Mechanizes G3 + G6 per leaf (`docs/prds/v0_6/gui-state-sync.md`). Each leaf's asserted capabilities are
bound to on-main evidence. Any `declared-only | test-only | producer-absent | producer-downstream |
fixture-ERROR | bound≤floor | rejection-absent` binding blocks the batch. **All bindings below PASS**
(evidence re-verified against current main 2026-07-06).

## Substrate class (G3 probe vectors — all N/A)

This PRD introduces **no `.ri` grammar, no DSL semantics, no eval/IR premise**. G3's three probe vectors —
grammar (`tree-sitter parse`), semantic (`reify check`), eval-error (`reify eval`) — are each **inapplicable**:
every mechanism is Rust/TS wiring against **existing** types (`GuiState`, `StateDelta`, the Tauri emitter
traits, the frontend `engineStore`). The `scripts/prd-decompose-verify.mjs` / `prd-capability-check.py`
workflow probes only `.ri` fixtures and therefore has **no probe to run** for any leaf. Substrate was
instead verified by **direct code reading** (PRD §3 anchors), recorded as `wired-on-main` grep bindings below.

- **Grammar-fixture:** N/A (no novel `.ri` syntax).
- **Field-population (empty-value sentinel):** N/A. These are GUI DTOs already populated by `build_gui_state`
  and read by the frontend `engineStore` (confirmed, `engineStore.ts:145`) — not `reify-eval` result fields
  that could be `Value::Undef`.
- **Numeric floor:** N/A (no numeric bound or closed-form exactness premise anywhere in the PRD).

## Per-leaf bindings (wired-on-main / anti-orphan)

| Leaf | Asserted capability | Evidence (on main) | Verdict |
|---|---|---|---|
| **L1** | GuiState fields enumerable by serde-key reflection | `GuiState` derives `Serialize` (`types.rs:161`); `serde_json::to_value` yields a field-keyed object | PASS |
| **L1** | checker rejects an unclassified field (negative test) | rejection mechanism **is** the deliverable, proven by the leaf's negative unit test (rejection-backed by construction) | PASS (rejection-backed) |
| **L2** | new StateDelta field emits a per-field event on the production path | `main.rs:242/294/312/361/387/511/715` call `emit_delta(compute_delta(&last_state, …))`; `delta_to_events` is pure + unit-tested (`diff.rs:181`) | PASS (grep: `main.rs` emit_delta callers) |
| **L2** | frontend renders the 4 list fields live | `engineStore.ts:80-145` already holds `tensegrityWires`/`tensegritySurfaces`/`displayPanes`/`displayAppearance`; leaf adds `bridge.ts` listener + reducer | PASS (store fields wired) |
| **L3** | bespoke emitter refreshes `fea_convergence` live | template `TauriFeaDiagnosticsEmitter` (`main.rs:150-165`) working for structurally-identical `fea_diagnostics` (#4884, done); producer `extract_fea_convergence` (`engine.rs:1244`); store field `feaConvergence` (`engineStore.ts:145`) | PASS |
| **L4** | 6 emit quintets collapse to one helper | 5 identical live sites (`engine.rs:1909/1963/2124/2205` + test `1369`) + 1 drifted (`5291-5300`, missing `emit_fea_diagnostics`) — grep-confirmed; helper is a pure extraction (comment `engine.rs:1493-1500`) | PASS |
| **L4** | drifted `load_from_compiled` gains `emit_fea_diagnostics` | `engine.rs:5291-5300` confirmed missing the call; extraction includes it by construction | PASS |
| **L5** | proc-macro derive generates diff/delta + makes unclassified field a compile error | stock Rust `proc-macro` substrate (no external dep); hand-written `diff.rs` is the parity reference; compile-error contract proven by `trybuild` fixture (leaf's own deliverable) | PASS (rejection-backed) |
| **L5** | #5023 freshness fields are a one-line classification | design property of the `#[sync(...)]` attribute; boundary-tested (D5) | PASS |
| **L6** | routing debug_server via `compute_delta`/`emit_delta` + `last_state` fixes stale-baseline desync | working path `main.rs:242` `compute_delta(&state.last_state, …)`; bypass confirmed `debug_server.rs:1292-1399` (pushes full GuiState via `query_frontend`, no `last_state` update); baseline `AppState.last_state` exists (`commands.rs:22`) | PASS |
| **L7** | real tool surface = `Write`/`Edit` + `mcp__reify-debug__*` | `session.ts:81` `ALLOWED_TOOLS = 'Read Edit Write Bash Glob Grep mcp__reify-debug__*'`; `system-prompt.ts:67-83` advertises `reify_*` tools **not** in that set | PASS |
| **L8** | `reify_`-prefix interception is dead / removable | `ALLOWED_TOOLS` cannot grant `reify_*`-prefixed tools, so `claude_bridge.rs:436` `tool_name.starts_with("reify_")` branch is unreachable from the sidecar | PASS (dead-path confirmed) |

## G6 premise validity

No leaf asserts a number, a closed-form exactness, or a producible-from-dependency-set capability that its
own dependency set cannot yield. The two rejection premises (L1 "rejects unclassified field", L5 "unclassified
field = compile error") are **rejection-mechanism-backed**: each leaf builds the rejection mechanism and its
boundary test observes the rejection firing (PRD §7). No G6 branch fires.
