# PRD: `debug_server` gating boundary — un-gate the non-Tauri half of reify-gui's debug-MCP module

**Status:** committed 2026-08-19. Version-agnostic build-hygiene infrastructure (root `docs/prds/`).
**Approach: bare B** + an explicit §5 gating contract (the contract exists because §8 β mechanizes it; it is not a full B+H apparatus — see §7 D6).
**Base commit for every measurement below:** `2a8598c679` (main, 2026-08-19). Every line number, count and classification in this PRD was re-derived first-hand against that tree.

**One-line goal:** move the ~50 `debug_server` tests and the 16 production items they exercise out from behind `#[cfg(feature = "gui")]` into an ungated sibling module, so they run on the ordinary `cargo nextest --workspace` pass and are reachable from a plain `cargo nextest run -p reify-gui` — leaving `debug_server.rs` holding only the axum + `DebugBridge` half that genuinely needs tauri.

---

## 1. Read this first — the premise is COST and DESIGN HYGIENE, not a coverage hole

There is a stale belief in this project's escalation history that `debug_server::tests` are "compiled but never executed by any merge gate." **It is false and was closed on 2026-08-03.** Do not build on it, do not re-file it.

Verified first-hand 2026-08-19:

- `scripts/verify.sh` emits `cargo nextest run -p reify-gui --features gui --config-file …` with **no `-E` filterset** (`verify.sh:2223`, emitted at `:2227`), and the `SCOPE=all` arm is unconditional **by contract** (`verify.sh:2183-2185`, task 6030).
- `data/verify-logs/6200/attempt-1.test-20260819T035137Z.log` shows that exact invocation running all 50 `debug_server::tests::*` green inside `Summary [21.153s] 911 tests run: 911 passed, 0 skipped`.
- `review/briefing.yaml:561` records the same: 911 with the feature vs 850 without.

Coverage is fine. What is *not* fine is that a single `cfg` line — `gui/src-tauri/src/lib.rs:15-16` — makes 50 tests, of which **45 have no gui dependency whatsoever**, reachable only through a tauri + webkit2gtk + OCCT feature-unification link measured at **~20m42s cold / ~137s warm** (tasks 5076/6030).

### 1.1 What this PRD does NOT buy — stated up front so no leaf asserts it

**The merge gate does not get cheaper. Not in any variant of this design.** Two independent reasons, both re-derived from `verify.sh`:

1. The gui pass runs `-p reify-gui --features gui` **with no `-E`** (§4 G4-1 (b8)). It therefore compiles and links the whole crate regardless of which modules are gated. Moving code between modules of the same crate changes nothing it pays for. (The 45 moved tests will run *twice* on `--scope all` — once in the workspace debug pass, once in the gui pass. Both are sub-second; see §6.4.)
2. `affected_crates()` is a **reverse**-dependency closure (`verify.sh:2090-2101`, `scripts/affected-crates-lib.sh:86`). `gui/src-tauri/**` maps to `reify-gui`, so any edit to either module still puts `reify-gui` in the closure and still emits the gui pass under arm 3. A separate crate would not help either — reify-gui would depend on it, so it stays in the reverse closure.

Any acceptance criterion promising merge-gate wall-clock savings would be a false premise. None below does.

### 1.2 What it does buy

| Payoff | Nature | Evidence |
|---|---|---|
| **Developer inner loop.** `cargo nextest run -p reify-gui` currently enumerates 850 tests and **zero** debug_server tests. After this, it enumerates all 50. Exercising them today costs the `--features gui` link (~137s warm best case, ~20m42s cold) *plus* an environment barrier the ungated path does not have: webkit2gtk dev headers, OCCT libs, and `scripts/ensure-gui-sidecar-placeholder.sh` having run. Both gui-feature passes carry that prefix (`verify.sh:2227`, `:2471`); the workspace pass carries none. | Repeatable, per-iteration | `verify.sh:2029` vs `:2227`; `gui/src-tauri/Cargo.toml` `[[bin]] required-features = ["gui"]` |
| **Free clippy coverage, today.** `cargo clippy --workspace --all-targets -D warnings` runs *without* `--features gui` (`verify.sh:~2451`), so debug_server.rs is type-checked but never lint-checked (task **5841**). Un-gating puts the moved half under the existing pass immediately, without waiting on 5841. | One-off, immediate | §3.3 |
| **Caps future growth of the gated surface.** `docs/prds/v0_6/reify-debug-mcp-expansion.md` (Draft) names `debug_server.rs` tool-defs + dispatch as its substrate and adds ~30 tools — each a `tool_defs()` entry plus a schema test. Without a boundary those ~30 tests land gated. | Forward-looking; the strongest argument | §6 seam table |
| **Restores the crate's dominant test convention.** reify-gui keeps its tests in `src/tests/<module>_tests.rs` — 20 such files. Five other modules also hold small inline `#[cfg(test)]` blocks (`types.rs` 9, `engine.rs` 7, `claude_bridge.rs` 2, `display_units.rs` 1, `lsp_bridge.rs` 1 = 20 tests combined); `debug_server.rs`'s 50 is more than all five together. | Hygiene | `gui/src-tauri/src/tests/mod.rs` |

---

## 2. Consumer + user-observable surface (G1)

**Named consumer of the extracted module: the ordinary workspace debug test pass** — `emit_nextest_pass "--workspace"` (`verify.sh:2029`), which today enumerates 850 reify-gui tests and zero debug_server tests. After this PRD it picks up the moved tests with **no new plan line**, no `--features gui` link, and no verify.sh change.

Named in-crate consumers whose call sites this PRD repoints (all real code, re-derived by AST-free scan over `gui/src-tauri/src/**.rs`):

| Call site | Symbol | Note |
|---|---|---|
| `gui/src-tauri/src/claude_bridge.rs:766` | `debug_server::resolve_debug_port` | inside a `#[cfg(feature = "gui")]` block — the cfg there is *behavioural* (only inject `REIFY_DEBUG_PORT` when a debug server can exist). It currently does double duty as symbol availability; after the move it is purely behavioural. **Behaviour must not change.** |
| `gui/src-tauri/src/claude_bridge.rs:1189` | `debug_server::resolve_debug_port` | test-side |
| `gui/src-tauri/src/main.rs:945-946` | `debug_server::{debug_endpoint_url, resolve_debug_port}` | `main.rs` is `required-features = ["gui"]`, so this is gated regardless |
| `large_stack.rs:14,187` · `path_key.rs:30,35` · `commands.rs:176,311,403,543` | intra-doc `[crate::debug_server::…]` links | four of these name items that move (`run_on_engine`, `open_source_into_engine_and_refresh_baseline`); repoint them so rustdoc links resolve |

`main.rs:932` (`spawn_debug_server`) and the `commands.rs` links to `handle_engine_state` / `handle_mesh_stats` / `handle_demand_dispatch` name items that **stay** — untouched.

**User-observable signal (the acceptance oracle).** The gui-only test set, computed as a set difference over two `nextest list` runs:

```bash
comm -13 <(cargo nextest list -p reify-gui                 | sort) \
         <(cargo nextest list -p reify-gui --features gui  | sort)
```

- **Before:** 61 lines, of which **50** are `reify_gui::debug_server::tests::*`.
- **After:** that set shrinks by **exactly 50**, and contains **zero** `debug_server::` and **zero** `debug_protocol::` entries.

This is deliberately a **delta plus a zero-membership property**, not an absolute count. reify-gui gains tests weekly; freezing "885" or "900" into a leaf signal is precisely the brittle-numeric-premise failure G6 branch 1 exists to catch. The 850/911 figures are cited as the 2026-08-19 baseline (`review/briefing.yaml:561`, verify-log 6200), not as an acceptance target.

**Residual after the move.** The gui-only set retains only genuinely tauri/OCCT-bound tests. Structurally located, whole-test granularity: `tests/event_bus_tests.rs` (1), `tests/kernel_status_tests.rs::gui_tests` (3, uses `reify_kernel_occt::OCCT_AVAILABLE`), `tests/engine_tests.rs::gui_feature_tests` (1, the `MorphRegistration` cfg arm), `claude_bridge.rs:1171` (1) = **6**. The 2026-08-19 `911 − 850 = 61` arithmetic implies ~11, i.e. ~5 I did not locate by static scan (`claude_bridge_tests.rs:3087,3098` are cfg'd *assertions* inside otherwise-ungated tests, not whole tests, and do not account for them). **This discrepancy is not load-bearing**: the acceptance criterion is a delta and a zero-membership property, neither of which depends on the residual's size. Re-derive the residual at implementation time and record it; do not assert a number for it in a leaf signal.

---

## 3. Substrate verification (G3) — all verified against `2a8598c679`, do not re-verify blind

No novel *language* substrate: this PRD introduces no `.ri` syntax, no builtin, no diagnostic. The grammar gate is **N/A**. What it does assume is Rust/Cargo/verify-pipeline substrate, each checked:

### 3.1 The gate is exactly one line

```rust
// gui/src-tauri/src/lib.rs:15-16
#[cfg(feature = "gui")]
pub mod debug_server;
```

`debug_server.rs` is 4090 lines and contains **no `tauri::` reference at all**. `#[cfg(test)] mod tests` sits at `:1868`, flat, **50** `#[test]`/`#[tokio::test]` fns, zero `#[ignore]`.

### 3.2 The only two gui-only symbols it touches

| Symbol | Sites | Why gui-only |
|---|---|---|
| `crate::debug::DebugBridge` | `:23`, `:1066`, `:1836` | `debug.rs:96-97` gates the struct itself; it holds a `tauri::AppHandle<R>` and uses `tauri::{Emitter, Runtime, Wry}` (`debug.rs:82-87`) |
| `reify_mesh_morph::{stats,diagnostics}` | `:1278`, `:1343`, `:1347` (production) | `reify-mesh-morph` is `optional = true` and listed in the `gui` feature (`gui/src-tauri/Cargo.toml:19,24`) |

**No test in the module references either one through `DebugServerState`.** Verified by a comment-and-string-stripped identifier scan over each test's body: the 50 tests reference exactly 16 production items, and `DebugServerState`, `DebugBridge`, `handle_mcp`, `handle_rest`, `spawn_debug_server`, `dispatch_tool` and every `handle_*(state: &DebugServerState)` appear **only in doc comments**, never in code. The full derivation is §5.1.

### 3.3 The clippy interaction is safe — verified, not assumed

Task 5841 measured `cargo clippy -p reify-gui --features gui --all-targets` first-hand on 2026-08-19: **6 warnings**, of which 4 are in `debug_server.rs`:

| Warning | Line | Lands in |
|---|---|---|
| `collapsible_if` | `:1788` | `handle_wait_for_selector` — **stays gated** |
| `await_holding_lock` ×3 | `:2143`, `:2213`, `:2235` | the three morph tests that correctly take `TEST_LOCK` — **moves** |

So the 35 pure items and the 10 engine-helper tests are **already clippy-clean** — un-gating them cannot turn main red. The five morph tests are a different matter and §7 D4 handles it explicitly.

### 3.4 Promoting `reify-mesh-morph` costs one crate

`reify-mesh-morph` depends on `reify-core`, `reify-ir`, `reify-eval`, `reify-solver-elastic`, `serde`, `tracing`. **`reify-solver-elastic` is already in reify-gui's non-gui build graph** (via `reify-eval`, `crates/reify-eval/Cargo.toml:31`), and **`reify-mesh-morph` is already an unconditional `[dev-dependencies]` entry** of reify-gui (`gui/src-tauri/Cargo.toml`, `features = ["testing"]`). Promoting it from `optional` to a normal dependency therefore adds exactly one small crate to the ungated *lib* build, and **nothing at all** to the ungated *test* build, which already links it.

### 3.5 Verify-pipeline substrate this PRD must respect

- reify-gui is a full workspace member (root `Cargo.toml:38`); there is no `default-members`. The `gui` feature is not default and no workspace crate depends on reify-gui, so feature unification never enables it.
- `.config/nextest.toml` has no `default-filter` and no `package(reify-gui)` group or override; nothing excludes reify-gui.
- `scripts/release-sensitive-crates.txt` lists reify-gui, but its release-sensitivity comes from `tests/engine_tests.rs`'s `#[cfg(not(debug_assertions))]` sites (mechanism B), **not** from `debug_server.rs`. Its lone `#[cfg(debug_assertions)]` at `:1421` sits inside `open_path_into_engine`, which stays, and is not one of the three declared mechanisms. **No `release-sensitive-crates.txt` change.**
- `bash scripts/verify-pipeline-guard.sh requires-full-gate <files>` is the oracle for whether a change forces `--scope all`. Consult it in-leaf; note that every merge-queue landing is `--scope all` anyway (`DF_VERIFY_ROLE=merge` → contract C2, `verify.sh:778`), so the guard's verdict is informational here.

---

## 4. Cross-PRD relationship + seam ownership (G4)

| Other PRD / task | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `docs/prds/v0_6/reify-debug-mcp-expansion.md` (Draft) | consumes | `tool_defs()` + the `dispatch_tool` arm table — ~30 new tools, each a tool-def entry plus a schema test | **this PRD** | Ownership taken deliberately (§7 D5). This PRD relocates `tool_defs()` and lands the §8 β drift guard so the expansion's ~30 schema tests cannot silently re-grow the gated surface. The expansion PRD has no filed tasks, so there is nothing to block on. |
| `docs/prds/merge-gate-compile-cost.md` (committed) | adjacent | test-binary count / compile-unit shape | that PRD | **No overlap.** Its W1 consolidates `crates/*/tests/*.rs` compile units; this PRD moves `src/`-internal unit tests inside one crate and adds **zero** compile units. Its ratified "within-crate only, never merge across crates" constraint is respected by §7 D2 (no new crate). |
| `docs/prds/godfile-test-eviction.md` (active) | precedent | evicting an embedded `#[cfg(test)] mod tests` into a sibling file, exact test-name/count parity, deliberately wide lock | that PRD | Structural exemplar only. Different files, no shared task. |
| task **6338** (pending, low) | prerequisite | adds `TEST_LOCK` to the 2 morph tests at `:2085`/`:2120` — tests that this PRD moves | 6338 | **Hard `add_dependency` edge, 6338 → α.** Landing α first would relocate two tests that then need a second visit to a contended file, and would leave 6338's line anchors pointing into a file that no longer holds them. |
| task **5841** (pending, medium; already `depends_on` 6338) | downstream | the `--features gui` clippy pass + the `await_holding_lock` idiom | 5841 | **Hard `add_dependency` edge, α → 5841.** 5841's prose anchors (`debug_server.rs:2143/2213/2235`) and its instruction to "establish ONE deliberate idiom … apply uniformly to all five" both assume the pre-move layout. α establishes that idiom for the five morph tests (it must — see §7 D4) and **updates 5841's description** to its post-move scope: `main.rs:734/787`, `debug_server.rs:1788`, and adding the gui-feature clippy pass. |

**Seam constraint that must survive — verbatim from `verify.sh:2138-2141`, asserted by `tests/infra/test_compute_trampoline_registration_wired.sh` (b8):**

> NO `-E` filterset. Running the whole `-p reify-gui --features gui` suite is what makes ALL gui-gated code execute; an enumerated name filter would silently drift away from that set as modules are added.

**This PRD does not violate (b8) and must not be allowed to become a de-facto filter.** The distinction is load-bearing: (b8) forbids *narrowing which of the gui-gated tests run*. This PRD *re-classifies which code is gui-gated at all*, and the gui pass keeps running 100 % of whatever remains gated, still with no `-E`. §5 C4 states this as a contract clause and §8 β asserts it mechanically.

Other guards in `test_compute_trampoline_registration_wired.sh`, all unaffected: (b1) still a `nextest run`; (b2) the `if test -f gui/src-tauri/Cargo.toml` guard + sidecar-placeholder prefix unchanged; (b7) still inside the semaphore bracket with the trailing ` 9<&-`; (b9) still absent from lint-only and `DF_VERIFY_ROLE=offline` plans; (b10) still survives `--scope branch` for a `gui/src-tauri/` change.

**`tests/infra/test_verify_throughput.sh`'s exact-equality `THROUGHPUT-COUNTS` sentinel needs NO bump.** It counts verify.sh **plan lines**, not tests. Neither α nor β adds or removes one: α touches no `add` site in verify.sh (only a stale comment at `:2036-2041`), and β adds a `tests/infra/test_*.sh`, which `run_all.sh` discovers behind the single existing `--include-infra` plan line (`verify.sh:2493`). State this explicitly in each leaf so an implementer does not bump it defensively.

---

## 5. The gating contract

The boundary needs a definition because §8 β mechanizes it. Four clauses:

- **C1 — Placement.** `gui/src-tauri/src/debug_protocol.rs` holds every debug-MCP item with no `DebugServerState`, `DebugBridge`, `tauri`, or axum-router contact. It is declared **ungated** in `lib.rs`. Its tests live at `gui/src-tauri/src/tests/debug_protocol_tests.rs`, matching the crate's dominant `src/tests/<module>_tests.rs` convention (20 files).
- **C2 — Residue.** `gui/src-tauri/src/debug_server.rs` stays `#[cfg(feature = "gui")]` and holds only the axum + `DebugBridge` half. It contains **zero** `#[test]` / `#[tokio::test]` attributes and **no** `#[cfg(test)] mod tests`. A future test that genuinely needs `DebugServerState` goes in a new gated `src/tests/debug_server_tests.rs`, never back inline.
- **C3 — Purity.** `debug_protocol.rs` and `debug_protocol_tests.rs` contain no `cfg(feature = "gui")`, no `tauri`, no `DebugBridge`, no `DebugServerState`, no `axum::routing`. Three of these the ungated compile enforces on its own; the guard makes the failure loud, immediate, and greppable rather than a link error 20 minutes into a feature build.
- **C4 — No filter.** The gui-feature pass keeps **no `-E` filterset**. This PRD narrows the *gated set*, never the *pass*.

### 5.1 The move set — derived, not eyeballed

Every one of the 50 tests was scanned (comments and string literals stripped) for references to `debug_server.rs`'s production items. The 50 tests reference exactly **16** items; those 16 plus their transitive requirements are the move set.

**Moves to `debug_protocol.rs` (ungated):**

| Item | Line | Tests | Transitively pulls |
|---|---|---|---|
| `tool_defs()` | `:35` | 22 tests | `struct ToolDef` (`:29`) |
| `is_image_tool()` | `:1075` | 5 | — |
| `mcp_content_blocks()` | `:1113` | 36–39 | — |
| `dispatch_stateless_tool()` | `:1159` | 10, 12 | the two morph handlers below |
| `run_on_engine()` | `:1198` | 1 | `crate::large_stack` (ungated) |
| `demand_dispatch_on_engine()` | `:1241` | 16 | `crate::commands` (ungated) |
| `handle_morph_stats()` | `:1277` | 9, 10 | `reify_mesh_morph::stats` — see D3 |
| `handle_mesh_morph_stats()` | `:1340` | 11–13 | `reify_mesh_morph::diagnostics`, `SESSION_START_UNIX_MS` (`:1288`), `session_start_unix_ms()` (`:1297`), `reset_session_start()` (`:1315`, sole caller is `:1344`) |
| `fixture_relpath()` | `:1360` | 33 | — (a literal `match`, no filesystem) |
| `open_source_into_engine_and_refresh_baseline()` | `:1486` | 45–49 | `crate::{commands,diff,types,engine_lock}` (all ungated) |
| `fea_case_frontend_payload()` | `:1528` | 43 | `crate::types::GuiState` (ungated) |
| `set_fea_case_on_engine()` | `:1541` | 42 | — |
| `set_fea_case_on_engine_and_refresh_baseline()` | `:1552` | 44 | `crate::diff` |
| `canonical_wait_for_selector_params()` | `:1765` | 22 | — |
| `parse_debug_port()` | `:1816` | 24, 25 | `DEFAULT_DEBUG_PORT` (`:1811`), and `resolve_debug_port()` (`:1823`) moves with it so the `claude_bridge`/`main` consumers repoint once |
| `debug_endpoint_url()` | `:1827` | 26 | — |
| `static TEST_LOCK` | `:1871` | — | test-only; moves with the tests |

**Stays in `debug_server.rs` (gui-gated), untouched:** `struct DebugServerState` (`:1062`), `JsonRpcRequest`/`JsonRpcResponse`/`JsonRpcError` + `impl` (`:1015`–`:1057`), `dispatch_tool` (`:1168`), `handle_engine_state` (`:1226`), `handle_demand_dispatch` (`:1250`), `handle_mesh_stats` (`:1254`), `open_path_into_engine` (`:1375`), `handle_open_file` (`:1503`), `handle_load_fixture` (`:1510`), `handle_set_fea_case` (`:1581`), `handle_mcp` (`:1598`), `handle_rest` (`:1676`), `handle_wait_for_idle` (`:1687`), `handle_wait_for` (`:1721`), `WAIT_FOR_SELECTOR_DEFAULT_TIMEOUT_MS` (`:1746`), `handle_wait_for_selector` (`:1782`), `spawn_debug_server` (`:1833`).

The JSON-RPC types are pure serde and *could* move, but no test touches them and `handle_mcp` is their only consumer. **Minimal move set** is the rule (D1): move what the tests need plus what is transitively required, nothing else.

Visibility: items that are private today (`tool_defs`, `ToolDef`, `is_image_tool`, `mcp_content_blocks`, `dispatch_stateless_tool`, `run_on_engine`, `demand_dispatch_on_engine`, `handle_morph_stats`, `handle_mesh_morph_stats`, `fixture_relpath`, `canonical_wait_for_selector_params`, `session_start_unix_ms`, `reset_session_start`) become `pub(crate)`. Items already `pub` keep their visibility.

### 5.2 Test classification

| Bucket | Count | Tests | Substrate |
|---|---|---|---|
| Pure logic over static data | 35 | 2–8, 14, 15, 17–41, 43 | none — `tool_defs()` schema assertions (~22), MCP envelope shaping, port parsing, `fixture_relpath`, `fea_case_frontend_payload` |
| Engine-facing helpers | 10 | 1, 16, 42, 44–50 | `EngineSession` over `MockGeometryKernel` via `crate::tests::make_test_engine()`; 7 also use `tempfile::tempdir()`. **All ungated substrate.** |
| Global morph counters | 5 | 9–13 | `reify_mesh_morph::{stats,diagnostics}` — the only genuine feature dependency, resolved by D3 |

Zero of the 50 need a TCP port, `REIFY_DEBUG_PORT`, a Tauri runtime or `AppHandle`, a webview, X11/Wayland, a headless browser, or a child process.

---

## 6. Sketch of approach

1. Create `gui/src-tauri/src/debug_protocol.rs`; declare `pub mod debug_protocol;` **ungated** in `lib.rs` beside the existing gated `debug_server`.
2. Move the §5.1 items verbatim — no semantic change, no rename, no reformat beyond what the move requires. Adjust visibility to `pub(crate)` where §5.1 says so.
3. Move all 50 tests to `gui/src-tauri/src/tests/debug_protocol_tests.rs`; add `mod debug_protocol_tests;` to `src/tests/mod.rs`. Delete `debug_server.rs`'s `#[cfg(test)] mod tests` entirely (C2).
4. `gui/src-tauri/Cargo.toml`: move `reify-mesh-morph` from `optional = true` to a normal dependency and drop it from the `gui` feature list.
5. Repoint the §2 call sites and intra-doc links.
6. Apply the D4 `await_holding_lock` idiom to the five morph tests.
7. Correct the stale prose: `verify.sh:2036-2041` ("the wholly gui-gated debug_server / event_bus modules"), `review/briefing.yaml:561` (same phrase + the 911/850 split).
8. Land the C1–C4 drift guard (§8 β).

**Accepted cost.** The 45 non-morph moved tests will run twice on a `--scope all` gate (workspace pass + gui pass). All 50 run inside the 21.153 s that the whole 911-test gui suite takes today; 35 are pure-function assertions and 10 build a `MockGeometryKernel` `EngineSession`, the same shape as 850 existing reify-gui tests. This is bounded and accepted, not measured further. It adds **no** compile unit and **no** plan line.

---

## 7. Resolved design decisions

- **D1 — Minimal move set, derived mechanically.** Move exactly the 16 items the 50 tests reference plus their transitive requirements (§5.1). *Rejected:* "move everything without gui contact," which would sweep the JSON-RPC types and `WAIT_FOR_SELECTOR_DEFAULT_TIMEOUT_MS` across the boundary for no test benefit and widen the diff on a contended file.

- **D2 — An ungated sibling module in reify-gui, not a new crate.** `gui/src-tauri/src/debug_protocol.rs`. The exact precedent is already in this crate: `debug.rs` splits ungated `DebugTransport` from gui-gated `DebugBridge`, with its tests in the ungated `src/tests/debug_boundary_tests.rs` — the same move, already made, one layer down. *Rejected:* a `crates/reify-debug-protocol` crate. It buys a harder boundary the ungated compile already enforces, and costs a new workspace member plus interactions with `scripts/affected-crates-lib.sh`, `scripts/release-scope-lib.sh`, `.config/nextest.toml` package groups, and `merge-gate-compile-cost.md`'s ratified "crate granularity matters for `-p` scoping" constraint — for **zero** additional gate saving (§1.1 reason 2 applies to a new crate exactly as it does to a new module).

- **D3 — All 50, by promoting `reify-mesh-morph` to an unconditional dependency.** *Rejected:* stopping at 45 and leaving the five morph tests gated. Promotion adds one small crate to the ungated lib build and nothing to the ungated test build (§3.4); leaving five tests behind would keep a `--features gui` dependency alive in `debug_server.rs`'s test surface for a crate the test build already links, and would keep 6338 and 5841 pointed at a file that is otherwise test-free. Going to 50 makes C2's ratchet **zero**, which is a far stronger invariant than "≤ 5".

- **D4 — α owns the `await_holding_lock` idiom for all five morph tests; it cannot defer to 5841.** Once un-gated, those tests become visible to the ordinary `cargo clippy --workspace --all-targets -D warnings` pass, which is `-D warnings`. **Landing the move without the idiom turns main RED.** The lock is required for correctness — it guards shared `AtomicU64` morph counters and three of the five already take it deliberately — so "remove the lock" is not an option. α establishes one idiom (a scoped `#[allow(clippy::await_holding_lock)]` on each affected test citing the `TEST_LOCK` contract, **or** restructuring so the guard drops before the await) and applies it uniformly to all five. α depends on 6338 precisely so all five are visible at once.

- **D5 — This PRD owns the `reify-debug-mcp-expansion` seam and lands a drift guard.** That PRD adds ~30 tools; without a mechanical boundary its ~30 schema tests land gated and the surface re-grows silently. *Rejected:* "note the seam, no guard" (relies on prose; the compile only catches a gui-only *import*, never a pure test added back into `debug_server.rs`) and "sequence behind the expansion PRD" (it is Draft with no filed tasks — an open-ended block).

- **D6 — Bare B, plus §5's contract.** G5 heuristic: blast radius 1 crate + `tests/infra` (well under 3); mechanism count 1 (a module boundary); no load-bearing seam from the overlay's list (FEA, ComputeNode dispatch, persistent-naming, multi-kernel, grammar/parser); cross-PRD consumers 1. The §5 contract exists solely because β must check something written down — it is not a B+H apparatus and there is no boundary-test sketch.

- **D7 — The end state is SHRINK, never DELETE the gui-feature test pass.** `event_bus` is a wholly gui-gated module, and `engine_tests::gui_feature_tests` / `kernel_status_tests::gui_tests` / `claude_bridge.rs:1171` test genuinely tauri- and OCCT-bound behaviour. The pass must keep running them wholesale with no `-E` (C4). Any leaf signal implying the pass can go away would be false.

- **D8 — Acceptance is a delta plus a zero-membership property, never an absolute test count.** §2. reify-gui gains tests weekly; an absolute count is a self-invalidating premise.

- **D9 — No `THROUGHPUT-COUNTS` bump, no `run-all-classification.manifest` row for α.** §4. α adds no verify.sh plan line and no new `tests/infra/test_*.sh`. β adds one and carries its own manifest row **in the same diff** (overlay gate-test drift-guard rule). Neither adds a `crates/*/tests/*.rs` compile unit, a wall-clock assertion, or a nextest heavy/smoke partition entry, so `test_no_new_wallclock_upper_bounds.sh` and `.config/nextest.toml` need no edit either.

- **D10 — G7 walk: clean, no waivers.** Reify's normative list is `docs/legibility/design-invariants.md` — INV-SF-1..7 (silent failure) and INV-AD-1..4 (angle/dimension). A verbatim code move introduces no `Undef`, no severity path, no declared intent, no placeholder, no new diagnostic, and no angular quantity. Neither leaf hits an invariant.

---

## 8. Decomposition plan

Two leaves. The move is atomic **by necessity**, not by preference: any intermediate state where the tests are un-gated but the D4 idiom is absent is a red merge gate, and `debug_server.rs` is a contention hotspot (task 5035 records 67 of 69 fix-touches since May), so two visits to it would be strictly worse than one. β touches a disjoint file set.

### α — Move the non-Tauri half of `debug_server` into an ungated `debug_protocol` module

- **Depends on:** task **6338** (hard edge).
- **Unlocks:** β; task **5841** (hard edge).
- **Modules touched:** `gui/src-tauri/src/{lib.rs,debug_protocol.rs,debug_server.rs,claude_bridge.rs,main.rs,large_stack.rs,path_key.rs,commands.rs}`, `gui/src-tauri/src/tests/{mod.rs,debug_protocol_tests.rs}`, `gui/src-tauri/Cargo.toml`, `scripts/verify.sh` (comment only, `:2036-2041`), `review/briefing.yaml` (`:561`).
- **Work:** §6 steps 1–7, to the §5 contract and the §5.1 move set. Verbatim move — no rename, no signature change, no behaviour change. `claude_bridge.rs`'s `#[cfg(feature = "gui")]` `REIFY_DEBUG_PORT` injection keeps its current behaviour. Update task 5841's description to its post-move scope (§4).
- **User-observable signal:** `comm -13` over the two `cargo nextest list -p reify-gui` runs (§2) returns a set **exactly 50 smaller** than on the base commit, containing **zero** `debug_server::` and **zero** `debug_protocol::` entries; and `cargo nextest run -p reify-gui` (no `--features gui`, no tauri/webkit2gtk/OCCT link) executes all 50 formerly-gated tests green, naming them.
- **G6:** branch 3 (end-to-end capability). Every capability the signal needs is delivered by α itself or already exists on main: `EngineSession`+`MockGeometryKernel`+`make_test_engine` (`src/tests/mod.rs:34`, ungated), `crate::{commands,diff,types,engine_lock,large_stack,path_key}` (all ungated in `lib.rs`), `reify-mesh-morph` (already an unconditional dev-dep). No branch-1 numeric bound (D8), no branch-2 exactness claim, no branch-4 rejection assertion. **Nothing is owed by a downstream task.**
- **Not required (state it, don't defensively do it):** no `THROUGHPUT-COUNTS` bump, no `release-sensitive-crates.txt` edit, no `.config/nextest.toml` edit, no new `run-all-classification.manifest` row, no `-E` filterset anywhere (D9, C4).

### β — Land the gating-boundary drift guard

- **Depends on:** α (hard edge). β asserts the post-move state; landing it earlier would be red, and writing it to skip when `debug_protocol.rs` is absent would make it a vacuous pass — the failure shape `feedback_graceful_skip_helper_is_a_vacuity_vector` names.
- **Modules touched:** `tests/infra/test_debug_server_gating_boundary.sh` (new), `tests/infra/run-all-classification.manifest` (bucket row, **same diff**).
- **Work:** a cheap grep-only guard — no cargo invocation, so it classifies `pool` — asserting C1–C4:
  1. `debug_protocol.rs` and `tests/debug_protocol_tests.rs` exist and contain no `cfg(feature = "gui")`, no `tauri`, no `DebugBridge`, no `DebugServerState`, no `axum::routing` (C1, C3).
  2. `lib.rs` declares `pub mod debug_protocol;` with **no** preceding `#[cfg(feature = "gui")]`, and still gates `debug_server` (C1, C2).
  3. `debug_server.rs` contains **zero** `#[test]` / `#[tokio::test]` attributes and no `#[cfg(test)] mod tests` (C2 — the ratchet is zero, not a count).
  4. `tool_defs`, `mcp_content_blocks`, `parse_debug_port` are defined in `debug_protocol.rs` and **not** in `debug_server.rs` (C1, the anti-regrowth anchor for the expansion PRD's ~30 tools).
  5. The emitted gui-feature pass still carries no `-E` (C4) — assert it, do not duplicate `test_compute_trampoline_registration_wired.sh`'s (b8) logic; cite it and check the one property.
  Every assertion names the offending file and clause on failure (project norm; `merge-gate-guard-diagnosability.md`).
- **User-observable signal:** **a seeded violation is caught.** Adding a `#[test] fn` with no gui dependency to `debug_server.rs` makes `bash tests/infra/test_debug_server_gating_boundary.sh` exit non-zero naming that file and clause C2; reverting returns exit 0. Likewise seeding `use tauri::AppHandle;` into `debug_protocol.rs` trips C3 by name. Both demonstrated in the task record — **not** "a unit test passes against synthetic input."
- **G6:** branch 4 (negative assertion). The signal asserts a rejection, so it binds the rejection mechanism by *observing it fire* on a seeded violation and then observing it stop firing on revert — both directions, per the overlay's negative-assertion mandate.
- **Registration:** the `run-all-classification.manifest` row lands in β's own diff (overlay rule; the esc-4914-162 failure this exists to prevent). Bucket `pool` — hermetic, grep-only, no cargo, no host resources.

**Out-of-batch dependency wiring at decompose:** `6338 → α`, `α → 5841`. Both are real `add_dependency` edges, not prose ordering.

---

## 9. Out of scope

- **The `--features gui` clippy pass.** Task **5841** owns it (already filed, pending, `depends_on` 6338). α gives the moved half lint coverage for free via the *existing* ungated pass, and hands 5841 a residual scope of `main.rs:734/787` + `debug_server.rs:1788` + wiring the new pass. α does **not** add a clippy pass.
- **The `TEST_LOCK` inconsistency itself.** Task **6338** owns it and is α's prerequisite. α must not duplicate or absorb it.
- **The residual ~6–11 gui-only tests.** `event_bus_tests` (1), `kernel_status_tests::gui_tests` (3, OCCT), `engine_tests::gui_feature_tests` (1, the `MorphRegistration` cfg arm), `claude_bridge.rs:1171` (1) are all genuinely tauri/OCCT-bound, plus the ~5 unlocated in §2. **File a follow-up only if** α's re-derived residual turns up a test with no tauri/OCCT/webview dependency. Do not file speculatively.
- **Deleting the gui-feature test pass** — impossible while genuinely gui-gated code exists (D7).
- **Any change to the debug-MCP surface itself.** Tool names, schemas, dispatch behaviour and the `:3939/mcp` transport are byte-identical after α. This PRD makes **no** language-surface or user-facing-tool change, so the overlay's docs-truth gate (doc-chunk / exemplar-corpus / cheatsheet / discoverability leaves) does **not** fire. The only prose obligations are the two stale-fact corrections in α's own diff (`verify.sh:2036-2041`, `review/briefing.yaml:561`).
- **`event_bus.rs`**, the other wholly gui-gated module. It has 0 tests of its own and 1 gated test in `tests/event_bus_tests.rs`; there is nothing to un-gate.

---

## 10. Open questions (tactical — decide in-leaf, not design-level)

1. **Where does test 50 land?** `resolve_leaves_stem_only_path_when_canonicalize_fails` (`:4056`) references **no** `debug_server` production item at all — it exercises `crate::commands::load_file_into_engine` + `GuiState::resolve`. It arguably belongs in `tests/commands_tests.rs`, not `debug_protocol_tests.rs`. **Suggested resolution:** move it with the others in α (keeping α a pure relocation), and leave a `// TODO(#NNNN)` only if a follow-up task is actually filed — otherwise just place it in `commands_tests.rs` and say so in the task record. Decide in α.
2. **Which `await_holding_lock` idiom?** Scoped `#[allow]` citing the `TEST_LOCK` contract, or restructure so the guard drops before the `.await`. **Suggested resolution:** whichever keeps the five tests' assertions unchanged; prefer the restructure if it is mechanical, since it removes the lint rather than muting it. Decide in α (D4 requires only that ONE idiom is chosen and applied to all five).
3. **Does `debug_protocol.rs` want submodules?** It lands at roughly 900 production lines (`tool_defs()` alone is ~980 lines of `json!` literals). A `debug_protocol/{mod,tool_defs,envelope,port,engine_helpers}.rs` split may read better. **Suggested resolution:** land flat in α to keep the diff a pure move; split later only if the expansion PRD's ~30 tools make it unwieldy. Do not pre-split.
4. **Should β also assert the `src/tests/<module>_tests.rs` convention crate-wide?** Tempting, but it widens β beyond this PRD's seam and would fail on any future deliberate exception. **Suggested resolution:** scope β to C1–C4 only.
