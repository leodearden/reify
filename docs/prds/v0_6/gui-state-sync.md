# PRD: GUI state-sync — one choke-point for backend→frontend field sync

**Milestone:** v0_6 · **Status:** active (design-first) · **Date:** 2026-07-06
**Approach:** **B + H** (engine/IPC consumers + frontend render + two-way boundary tests) — the GUI state
bridge is hotspot **H4** of the 2026-07-05 bug survey; the derive-and-choke-point seam is architecturally
load-bearing (four parallel sync mechanisms, no seam owner).
**Program:** bug-hotspot hardening (survey `docs/notes/bug-hotspot-survey-2026-07-05.md` §H4 + latent bugs
5/6/7/13). **Establishes:** **INV-GUI-1** (every `GuiState` field has a declared sync mechanism) and
**INV-GUI-2** (every engine-mutation entry point flows through the same delta choke-point) —
`docs/invariants.md`.

---

## 0. Thesis & consumer (G1)

Backend→frontend GUI state sync has **four parallel mechanisms and no choke-point**, and coverage is
*opt-in per field*. The result is a recurring bug class — "computed backend-side but never surfaced live" —
fixed **four times in two months** by promoting one field at a time from silently-stale to bespoke-wired
(1764 → 3351 → 3386 → 4252), plus desync bugs 4251 / 4884. The survey found **six more** `GuiState` fields
sitting in the same stale state right now (§1), i.e. the next 4–5 bugs of this class are already latent.

The four mechanisms (H4 diagnosis, code-confirmed):
1. **Per-field delta events** (`diff.rs` `StateDelta`/`diff_gui_state`/`delta_to_events`) — the principled
   channel, but covers only **5 of 13** `GuiState` fields.
2. **The full-snapshot command return** — the only *universal* channel — is **discarded** on the two hottest
   paths (`App.tsx:1663` `handleSetParameter`, `App.tsx:1269` source edit both `.catch()`-only), so it
   never refreshes anything live on a param edit.
3. **Six bespoke emitters**, installed one-per-feature, fired via a 5-line "emit quintet" copy-pasted at 6
   sites in `engine.rs` — the 6th copy (`load_from_compiled`, `engine.rs:5291-5300`) has **already drifted**
   (dropped `emit_fea_diagnostics`).
4. **Phase-transition pull-refetch.**

`debug_server.rs` adds a *fifth de-facto path*: its mutation handlers share the engine `Arc` but bypass
`AppState.last_state`, so a subsequent normal command diffs against a **stale baseline** (latent bug #7;
plausible driver of `debug_server.rs`'s 67-of-69 fix-touches-since-May).

**This PRD collapses the mechanisms to one seam** — a schema-derived per-field sync channel plus a single
emission choke-point — under two uniformly-enforced invariants, and clears the six latent stale fields on
the way (stopgap now, per Leo's ratified sequencing).

**Consumers (G1):**
- **Leo, dogfooding `prj/printer_v01` in the dev GUI daily** — the direct user surface; every stale field is
  a wrong thing on his screen after a param edit.
- **Task #5023 (async-recalc Phase A, deferred bookmark)** — adds per-artifact freshness fields
  (`fresh`/`computing`/`failed` per value/constraint/mesh) that **must ride the INV-GUI-1 derive**: the
  "calculating…" indicator must never itself go stale (else it becomes the next #4884). #5023's own task
  description already names "GuiState schema-derived sync (INV-GUI-1)" as a dependency to wire — the seam is
  reciprocally ratified. See §6.
- **AI-native editing tools PRD** (`docs/prds/v0_6/ai-native-editing.md`, concurrent /prd session) — consumes
  INV-GUI-2's choke-point: AI-driven engine mutations must flow through the same delta path so edits sync like
  human edits. See §6.

---

## 1. The coverage matrix (code-confirmed, current main)

`GuiState` (`gui/src-tauri/src/types.rs:162-293`) has **13 fields**. Their sync mechanism today:

| # | Field | Mechanism today | Live on param edit? |
|---|---|---|---|
| 1 | `meshes` | **Diffed** (`mesh-update`/`mesh-removed`) | ✅ |
| 2 | `values` | **Diffed** (`value-update`/`value-removed`) | ✅ |
| 3 | `constraints` | **Diffed** (`constraint-update`/`constraint-removed`) | ✅ |
| 4 | `files` | **Emitter** (`file-changed`/`file-removed`) | ✅ |
| 5 | `tessellation_diagnostics` | **Diffed** (`tessellation-diagnostics`) | ✅ |
| 6 | `compile_diagnostics` | **Diffed** (`compile-diagnostics`) | ✅ |
| 7 | `tensegrity_wires` | **none — stale** | ❌ (bug) |
| 8 | `tensegrity_surfaces` | **none — stale** | ❌ (bug) |
| 9 | `display_panes` | **none — stale** | ❌ (bug) |
| 10 | `display_appearance` | **none — stale** | ❌ (bug) |
| 11 | `fea_diagnostics` | **Emitter** (`fea-diagnostics-changed`, #4884) | ✅ |
| 12 | `fea_convergence` | **none — stale** | ❌ (bug — structurally identical to #4884) |
| 13 | `demand_prune_measurement` | **observability-only** (read via MCP `engine_state_json`, no UI reader) | n/a |

The frontend already **holds** the six stale fields (`engineStore.ts:145` populates them from the full
snapshot) and renders them — they simply never receive a *live* update, so they go stale after every param
edit until the next full reload. Six fields, no live wire: that is the entire latent-bug surface, mechanically.

---

## 2. Sketch of approach — the two-invariant spine

### INV-GUI-1 — every field has a declared sync mechanism (`lint interim → type`)

**Rollout follows the registry's fail-closed protocol** (`docs/invariants.md`: contract → warn-mode sweep →
fix bulk producers → flip to enforce):

- **Interim — a field-coverage lint** (a Rust test, §8 L1). It enumerates every `GuiState` field (by
  serde-key reflection of a fully-populated fixture — robust to reordering, no `syn` needed) and cross-checks
  each against a **classification table** with three buckets: `Diffed` | `Emitter(<event>)` |
  `FullReloadOnly(<reason>)`. A field that is neither classified **nor** on an explicit shrinking
  known-stale allowlist → the lint **fails**. The six stale fields start on the allowlist (warn-mode: they
  are the batch-enumerated violations), each entry naming the stopgap task that clears it. *(Note:
  `scripts/check_event_inventory.sh` checks event-**name** drift only — field **coverage** is a new, distinct
  check.)*
- **Real fix — a derive macro** (`#[derive(GuiSync)]` + per-field `#[sync(...)]`, §8 L5) that generates the
  `StateDelta`/`diff`/`delta_to_events` code from the classification, making **an unclassified field a
  compile error**. This retires the interim lint (the hand-maintained table becomes per-field attributes)
  and makes drift *unrepresentable* — the survey's cross-cutting fix-shape #1 ("an extraction isn't done
  until the old shape is unrepresentable"). The derive supports two classifications — `diffed` (macro
  generates the diff + event) and `full_reload_only(reason)` (macro generates nothing; the field is declared
  to sync only via full-snapshot application or a separately-named channel, `reason` forced non-empty).

**Coarse whole-state JSON diff is REJECTED** (§5) — mesh payloads on the slider-drag path. The derive is
strictly **per-field**.

### INV-GUI-2 — one emission choke-point for every engine-mutation entry point (`test + routing`)

- **Extract `post_engine_call_telemetry()`** (§8 L4) — the 6 hand-copied emit quintets collapse to one
  helper, so a new engine entry point cannot forget an emitter (fixes the already-drifted `load_from_compiled`
  copy by construction). The team pre-scoped this as review suggestion #4 of task **3541**'s amendment pass
  (code comment `engine.rs:1493-1500`).
- **Route `debug_server` mutations through the same delta path** (§8 L6) — eliminate the parallel state path
  so `debug_server` mutations update `AppState.last_state` and a subsequent normal command diffs against a
  fresh baseline.

### Stopgap first (ratified sequencing)

Per Leo's decision #3 ("stopgap wiring now + schema-derived per-field events as the real fix"), the six
stale fields are cleared **before** the derive lands, using the existing transports as literal templates
(§8 L2/L3) — this kills the next 4–5 known-class bugs immediately. The derive (L5) then migrates the
now-complete classification into compile-time enforcement.

---

## 3. Pre-conditions / substrate (G3)

- **No novel `.ri` grammar or DSL surface.** Every mechanism is Rust/TS wiring against **existing** types
  (`GuiState`, `StateDelta`, the Tauri emitter traits, the frontend `engineStore`). The grammar gate does
  not apply; there are no `.ri` fixtures.
- **The delta channel is wired on main** (not test-only): `main.rs` calls `compute_delta(&last_state, …)` +
  `emit_delta(…)` at every command site (`main.rs:242/294/312/361/387/511/715`). New `Diffed` fields
  therefore reach the frontend on the real path.
- **The frontend already holds and renders the six stale fields** (`engineStore.ts:80-145`); the stopgap adds
  the missing *live* transport (backend diff/emitter arm **plus** the frontend listener + reducer), not new
  UI.
- **`fea_convergence` is structurally identical to the already-fixed #4884** (`fea_diagnostics`): both are a
  full-value FEA-overlay snapshot sourced on the check path; the #4884 emitter (`main.rs:150-165`) is the
  literal template.
- **The tessellation-diagnostics diff arms** (`diff.rs:150-156` and `:231-239`) are the literal template for
  the four list-field stopgap.
- **Compile-time field-classification substrate (L5) — verified absent as a *derive*, so L5 owns its
  creation or uses `macro_rules!`.** Generic-G3 check (2026-07-06): the workspace has **no first-party
  `proc-macro = true` crate** and **no `#[proc_macro_derive]`** (only the *external* `strum` derive is used);
  `syn`/`quote` appear in `Cargo.lock` only transitively via `strum_macros`. So a `#[derive(GuiSync)]` is
  **not** existing substrate. L5 must therefore pick one of two non-fictional paths, **not** assume a derive:
  (i) **`macro_rules!`** — built-in, **no new crate / no `syn`/`quote`**: a `gui_state! { <classification>
  <field>: <ty>, … }` macro that is the single definition site of `GuiState` and generates
  `StateDelta`/diff/`delta_to_events`; a field written without a classification token fails to expand →
  compile error. This is the recommended default (adds no substrate). (ii) a **new first-party proc-macro
  crate** (`#[derive(GuiSync)]` + `#[sync(...)]`) — more ergonomic, but then *creating that crate + adding
  `syn`/`quote` as direct deps is explicit L5 scope*. Either satisfies INV-GUI-1; neither may be silently
  assumed.

Anchors were re-verified against current main on 2026-07-06 (survey anchors were 2026-07-05; line numbers
above reflect today's tree).

---

## 4. Resolved design decisions

- **D1 — per-field, not coarse-diff.** The real fix is a per-field schema-derived channel. A coarse
  whole-`GuiState` JSON diff is rejected: it would ship full mesh payloads on the slider-drag path
  (Leo decision #3). (§5)
- **D2 — stopgap before derive.** Clear the six stale fields with the existing transports now; the derive
  migrates the complete classification later. (§2)
- **D3 — `fea_convergence` gets a bespoke emitter** (not a `StateDelta` arm), cloning
  `TauriFeaDiagnosticsEmitter`, so both FEA-overlay fields ride the same transport (consistency with #4884).
  The four *list* fields get `StateDelta` arms (consistency with `tessellation_diagnostics`).
- **D4 — `demand_prune_measurement` is kept, classified `FullReloadOnly(observability)`, not deleted and not
  live-wired.** It has **zero UI readers** (grep-confirmed) but **is** consumed live via the reify-debug MCP
  `engine_state_json` path (`commands.rs:262`) for the selective-demand e2e gate (task 4741). Deleting it
  breaks that observability; live-wiring it serves no UI. So it is a legitimate `FullReloadOnly` field with a
  justification string. *(This resolves the brief's "wire or delete" via code evidence; flagged for Leo's
  objection in §9.)*
- **D5 — the derive is designed so #5023's per-artifact freshness fields are a one-line classification.**
  Adding `fresh`/`computing`/`failed` fields is a single `#[sync(diffed)]` each; the staleness indicator
  therefore rides the same choke-point it reports on and cannot go stale independently. (Hard G4 constraint,
  §6.)
- **D6 — sidecar prompt honesty is a *delete*, not a gate** (Leo-ratified). The system prompt is rewritten to
  the real tool surface and the dead `reify_`-prefix interception is removed; the properly-wired version is
  re-introduced later by the AI-native editing tools PRD. (§6, §8 L7/L8)
- **D7 — L6 resolved to the fallback route: thread the shared delta baseline, keep the parallel
  `query_frontend` push.** Investigate-then-route (§8 L6) found three compelling reasons the primary route
  (retire the debug-server push in favor of `compute_delta`/`emit_delta`) is not viable: (1) the e2e
  visual-regression harness (`gui/test/visual/*`) depends on `DebugBridge::query_frontend`'s synchronous
  request/response round-trip (`debug.rs:117-162`) to know a mutation is applied **before** it screenshots —
  `emit_delta` is fire-and-forget with no synchronization point and would race; (2) the frontend handlers do
  frontend-specific work no delta event can express (`bridge.ts:1086-1120`): `open_file` opens a **new editor
  tab** (`editor.openFile`) plus a full `engine.initFromState` and `viewState.resetToDefaultView()`;
  `apply_gui_state` uses `initFromState` but **deliberately omits** the view reset so the camera stays fixed
  across per-case FEA screenshots; (3) OCCT panics inside tokio, so the debug path's `run_on_engine`
  real-OS-thread model is structurally distinct from `main.rs`'s Tauri-command-thread + fire-and-forget
  `emit_delta` model. Instead, `DebugServerState` gets a `last_state: Arc<Mutex<Option<GuiState>>>` field — the
  **same** `Arc` as `AppState.last_state` — refreshed by two thin helpers
  (`open_source_into_engine_and_refresh_baseline`, `set_fea_case_on_engine_and_refresh_baseline`) that call
  `compute_delta` (discarding the returned delta) purely to advance the baseline before the full-state push.
  **OQ4 resolution:** no shared `apply_and_emit(session, last_state, app)` helper — the two paths' threading and
  delivery models differ too much to unify; they instead share only the minimal primitive (`compute_delta`, the
  same choke-point `main.rs`'s normal command path diffs against), reused directly from `debug_server.rs`. (§8
  L6)

---

## 5. Out of scope / rejected alternatives

- **Coarse whole-state JSON diff / generic top-level patch channel — REJECTED (D1).** Ships mesh payloads on
  the high-frequency slider-drag path. The derive is per-field precisely to keep mesh-sized fields on their
  keyed per-item diff and small fields on cheap whole-value diffs.
- **Deleting `demand_prune_measurement` — REJECTED (D4).** Live MCP consumer.
- **Re-introducing wired AI-editing tools here — out of scope.** The dead `reify_` interception is *deleted*;
  the AI-native editing tools PRD owns the re-introduction with real sync (§6). This PRD's deletion is that
  PRD's clean slate.
- **New UI / new panels — out of scope.** The frontend already renders every field; this PRD only fixes the
  *transport*.
- **Retiring the remaining bespoke emitters** (`file-changed`, `fea-diagnostics-changed`, `fea-case-changed`,
  `mode-shape-frame`, `warm-pool-event`, `solver-progress`) beyond what the derive migration needs — deferred.
  Non-field emitters (mode-shape frames, solver progress, warm-pool events) are event *streams*, not `GuiState`
  fields, and are outside INV-GUI-1's field-coverage scope.
- **The other H4 proposals not in Leo's ratified set** (e.g. C1's alternative "generic diff/patch channel"
  framing) — not pursued.

---

## 6. Cross-PRD relationship + seam owners (G4)

| Seam | Owner | Consumer | Resolution |
|---|---|---|---|
| INV-GUI-1 derive (per-field sync classification) | **this PRD** (§8 L5) | **#5023 async-recalc Phase A** | #5023's freshness fields ride the derive as one-line `#[sync(diffed)]` (D5). **Reciprocally ratified**: #5023's description already lists "GuiState schema-derived sync (INV-GUI-1)" as a dependency to wire. Decompose wires `add_dependency(5023 → L5)`. |
| INV-GUI-2 choke-point (all mutation entry points → one delta path) | **this PRD** (§8 L4/L6) | **AI-native editing tools PRD** (`docs/prds/v0_6/ai-native-editing.md`, concurrent session) | AI-driven engine mutations route through the same choke-point so AI edits sync like human edits. That PRD is the consumer of the choke-point; this PRD's `reify_`-interception deletion (L8) is its clean slate. |
| Sidecar process-lifecycle (`SidecarState` + `on_sidecar_exit`) | already-unified (positive template) | — | **Do NOT touch.** It is the good template (single terminal-event choke-point). |

`display_panes` / `display_appearance` originate from the appearance/multi-pane batch (PRDs
`appearance-viewport-egress.md`, `multi-pane-viewport.md`, tasks 4765/4772) — this PRD does **not** change
their *production*, only adds their **live sync** (they are currently `FullReloadOnly` by omission).

---

## 7. Boundary-test sketch (G5 — B + H, two-way)

Each high-stakes seam gets a test facing **both** producer and consumer:

- **INV-GUI-1 derive (L5) — two-way:**
  - *Producer face:* a `trybuild`/compile-fail fixture — a `GuiState` field with no `#[sync(...)]` attribute
    **fails to compile** (the contract that makes drift unrepresentable).
  - *Consumer face:* a **parity test** — the derive-generated `diff_gui_state`/`delta_to_events` produces
    byte-identical output to the retired hand-written version over a corpus of snapshot pairs (proves the
    migration preserved behavior).
- **INV-GUI-1 interim lint (L1) — negative test:** the checker **rejects** a synthetic unclassified field
  key (dev-observable negative test; the checker is itself the rejection mechanism and the leaf proves it
  fires).
- **INV-GUI-2 telemetry choke-point (L4):** a grep-architecture test asserts **no bare emit quintet remains**
  (all entry points route through `post_engine_call_telemetry`), and a regression test asserts the
  previously-drifted `load_from_compiled` path now emits `fea-diagnostics`.
- **INV-GUI-2 debug_server routing (L6) — two-way:** a **baseline-desync regression test** — a debug-driven
  mutation (`load_fixture`/`set_fea_case`) followed by a normal `set_parameter` produces **correct deltas**
  (before: the normal command diffed against the stale pre-debug baseline).
- **Stopgap (L2/L3):** a running-GUI reify-debug MCP session asserts each newly-wired field refreshes **live**
  on a param edit with no reload.

---

## 8. Decomposition plan (one leaf per bullet; observable signal + INV-id)

Dependency DAG (real `add_dependency` edges): `L1 → {L2, L3, L5}`; `L4 → {L3, L5, L6}`; `{L2, L3, L4} → L5`;
external `L5 → #5023`. `L7`, `L8` independent (sidecar). `L4`, `L6`, `L7`, `L8` need no intra-batch prereq
beyond the noted edges.

- **L1 — coverage lint (INV-GUI-1 interim, warn-mode).** A Rust test enumerating every `GuiState` field by
  serde-key reflection of a fully-populated fixture, cross-checked against the `{Diffed | Emitter | FullReloadOnly}`
  classification table + a shrinking known-stale allowlist (the six §1 fields, each naming its clearing task).
  Includes a **negative unit test**: the checker rejects a synthetic unclassified key.
  *Signal (dev-observable):* `cargo test` — the checker **fails on a synthetic unclassified field**, and the
  warn-mode report lists the six currently-unwired fields. *INV: INV-GUI-1.*
- **L2 — stopgap: four list fields → `StateDelta`.** Add `tensegrity_wires`, `tensegrity_surfaces`,
  `display_panes`, `display_appearance` to `StateDelta`/`diff_gui_state`/`delta_to_events` (template:
  `tessellation_diagnostics` arms at `diff.rs:150-156`/`:231-239`) **plus** the frontend `bridge.ts` listeners
  + `engineStore` reducers; remove the four from L1's allowlist (now `Diffed`).
  *Signal (user-observable, reify-debug MCP):* open a `.ri` carrying tensegrity + display-pane content, edit a
  param, assert the tensegrity overlay and display panes update **live** with no reload (was stale until
  reload). *INV: INV-GUI-1. Deps: L1.*
- **L3 — stopgap: `fea-convergence-changed` emitter.** Add `TauriFeaConvergenceEmitter` cloning
  `TauriFeaDiagnosticsEmitter` (`main.rs:150-165`), emit it inside `post_engine_call_telemetry` (L4), wire the
  `bridge.ts` listener + `engineStore.feaConvergence` reducer; remove `fea_convergence` from L1's allowlist
  (now `Emitter`).
  *Signal (user-observable, reify-debug MCP):* a param edit that changes FEA convergence refreshes the
  convergence indicator **live** (was stale until reload — the next #4884). *INV: INV-GUI-1. Deps: L1, L4.*
- **L4 — extract `post_engine_call_telemetry()` (INV-GUI-2 choke-point).** Replace the 6 hand-copied emit
  quintets (`engine.rs:1369` [test], `1909`, `1963`, `2124`, `2205`, `5291`) with one helper; the drifted 6th
  copy (`load_from_compiled`, missing `emit_fea_diagnostics`) is fixed by construction. Provenance: review
  suggestion #4 of task **3541**'s amendment pass (comment `engine.rs:1493-1500`).
  *Signal (dev-observable):* a grep-architecture test asserts zero bare quintets remain; a regression test
  asserts `load_from_compiled` now emits `fea-diagnostics`. *INV: INV-GUI-2. Deps: none.*
- **L5 — schema-derived sync (INV-GUI-1, real fix).** A compile-time field-classification macro on
  `GuiState` — a `macro_rules!` single-definition macro (recommended; **no new substrate**) **or** a
  first-party proc-macro derive crate that **L5 itself creates** (none exists today — see §3). Each field
  carries a classification (`diffed` | `full_reload_only(reason)`); the macro generates
  `StateDelta`/`diff`/`delta_to_events`; a field with no classification is a **compile error**. Migrate all
  13 fields; delete the L1 interim lint (allowlist now empty). Design so #5023's freshness fields are a
  one-line `diffed` classification (D5).
  *Signal (dev-observable, two-way):* a compile-fail fixture — a field without `#[sync]` fails to compile;
  a parity test — generated delta == retired hand-written delta over a snapshot-pair corpus.
  *INV: INV-GUI-1. Deps: L1, L2, L3, L4. Consumer: #5023 (wire `add_dependency(5023 → L5)`).*
  *`metadata.files = []` (broad refactor; BRE acquires footprint incl. any new derive crate).*
- **L6 — route `debug_server` mutations through the delta path (INV-GUI-2).** Investigate-then-route: first
  verify the e2e harness's synchronous `query_frontend`-then-respond assumption (a characterization test
  pinning today's stale-baseline behavior — `debug_server.rs:1292-1399` pushes full `GuiState` via
  `debug_bridge`, bypassing `AppState.last_state`); then route `open_file`/`load_fixture`/`set_fea_case`
  through the same command impls + `compute_delta`/`emit_delta` path `main.rs` uses, eliminating the parallel
  path. **Fallback** (if the investigation finds a compelling reason to keep the parallel path): thread the
  delta baseline into `DebugServerState` instead — record the chosen route + rationale in this PRD's §4.
  *Signal (user-observable, reify-debug MCP):* a debug-driven mutation followed by a normal `set_parameter`
  produces **correct deltas** (baseline-desync regression test). *INV: INV-GUI-2. Deps: L4.*
  *Resolved to the fallback route — see D7 (§4).*
- **L7 — sidecar system-prompt honesty.** Rewrite `gui/sidecar/src/system-prompt.ts:67-83` to the **real**
  tool surface (`Write`/`Edit` + `mcp__reify-debug__*`), dropping `reify_set_parameter`/`reify_update_source`/
  etc. that `session.ts:81` `ALLOWED_TOOLS` structurally cannot grant.
  *Signal (dev-observable):* a sidecar test asserts every tool named in the system prompt is grantable
  (∈ `ALLOWED_TOOLS` or a real reify-debug MCP tool); the `reify_`-prefixed fictions are gone. *Sibling of L8;
  cites the AI-native editing tools PRD. Deps: none.*
- **L8 — delete the dead `reify_` interception.** Remove the `reify_`-prefix interception at
  `claude_bridge.rs:424-487` and its now-orphaned tests (git preserves). Cite the **AI-native editing tools
  PRD** (concurrent session) as the consumer whose clean slate this is.
  *Signal (dev-observable):* the interception is gone (compiles without it; orphaned tests removed); a sidecar
  integration test confirms a reify-debug MCP tool call still round-trips (nothing real was removed).
  Cite `docs/prds/v0_6/ai-native-editing.md` in the task so the lineage is findable. *Sibling of L7. Deps: none.*

---

## 9. Open (tactical) questions

- **OQ1 (for Leo — D4).** `demand_prune_measurement` is resolved as `FullReloadOnly(observability)` on the
  code evidence that the reify-debug MCP `engine_state_json` path consumes it live (task 4741). If you'd
  rather it be deleted outright (no UI future), say so and L1/L5 drop it instead of classifying it.
- **OQ1b (impl-time, L5) — resolved to a bounded choice by the generic-G3 check (§3).** `macro_rules!`
  (no new substrate — recommended default) vs a new first-party proc-macro crate (L5 creates it + adds
  `syn`/`quote`). Both satisfy INV-GUI-1; the derive path is *not* pre-existing substrate and must not be
  assumed. If the proc-macro path is taken, the compile-fail boundary test likely wants `trybuild` (also not
  yet a workspace dep — add as a dev-dep).
- **OQ2 (impl-time, L5).** Whether the macro **retires** the two FEA bespoke emitters (`fea-diagnostics-changed`,
  `fea-convergence-changed`) by folding those fields into `diffed`, or keeps them as `full_reload_only`
  annotations pointing at the emitter (avoids a frontend-listener rewrite). Recommendation: keep the FEA
  emitters as annotated `full_reload_only` for L5's first cut; fold later if desired. Either satisfies the
  invariant.
- **OQ3 (impl-time, L5).** Attribute spelling for keyed collections (`meshes`/`values`/`constraints`/`files`
  need a key fn) vs whole-value fields (diagnostics/lists). Suggested: `#[sync(diffed, key = "entity_path")]`
  vs bare `#[sync(diffed)]`.
- **OQ4 (impl-time, L6).** Exact route: reuse `reify_gui::commands::*` impls directly from `debug_server`, or
  factor a shared `apply_and_emit(session, last_state, app)` helper both `main.rs` and `debug_server` call.
  Decided inside L6 after the characterization test.
- **OQ5 (process).** PRD placed at `docs/prds/v0_6/` (active milestone; matches the overlay
  `<vM_N>/<slug>.md` convention and the `gui-state-sync` owner slug in `docs/invariants.md`). Relocate
  trivially if the hotspot-hardening program wants its own dir.
