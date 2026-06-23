# PRD: Selective demand — make the two-cone scheduling model real

**Milestone:** v0_6 · **Status:** ACTIVE (preconditions landed; expanded to full B+H) · **Date:** 2026-06-23
**Supersedes:** the 2026-06-11 forward-stub of this file (DEFERRED milestone tracker, task **4533**).
**Parent intent:** `docs/reify-implementation-architecture.md` §3.2/§3.3 (demand registry + two-cone scheduling).
**Approach:** **B + H** (contract section §7 + two-way boundary-test sketch §8) — load-bearing core-engine
scheduler seam, blast radius reify-eval + reify-ir + gui/src-tauri + gui frontend + reify-debug MCP.

---

## 1. Goal — what a user observes

In the GUI interactive edit loop, **hidden bodies stop costing kernel time on every slider drag**. Dragging
a parameter that drives a viewport-hidden body re-realizes only the *visible* bodies; the hidden body's
geometry kernel is not invoked. The property panel still shows the hidden body's last-known values, badged
`Pending` (⚠), never a silently-stale number. Un-hiding the body re-realizes it to the correct
current-parameter geometry. The spec §3.3 **P1-slow tier becomes scheduler-real**.

**Secondary consumer:** LSP/editor warm paths (`eval_cached`) skip cones feeding nothing displayed.

This is the production realization of the **two-cone model**: work is scheduled only where `dirty ∩ demand`
is non-empty, with `demand` finally selective instead of degenerate-total.

---

## 2. Activation status — why this was deferred, and what unblocked it

Demand was **degenerate-total**: `build_demand_for_graph` (`engine_eval.rs:215-228`) marks every
value/constraint/realization node always-demanded, so the `dirty ∩ demand` intersection
(`compute_eval_set`, `dirty.rs:236-248`) is a code-live **no-op** — it prunes nothing because `is_demanded`
is universally true. The registry, the intersection, and the seeded planner were all built and unit-tested,
but never driven selectively on the production path. Three prerequisites — now **all landed** — gated
activation:

1. **A demand-consulting warm scheduler.** Geometry work is caller-invoked whole-pass; demand cannot gate
   work that is not scheduled. The unified driver (`run_unified_pass_seeded`, `engine_fixpoint.rs:284`)
   arrived with the warm stage (θ **4361**, landed) and the edit-path re-homing (θ2 **4531**, landed).
2. **Staleness → wrong-pruning conversion.** `edit_param` did not rebuild
   `reverse_index`/`trace_map`/`demand` after structural re-elaboration (collection grow / forall
   re-emission). Under total demand this is *missed-propagation*; under selective demand the same staleness
   becomes **silent wrong-pruning**. Fixed by task **4530** (landed; `engine_edit.rs:2178-2199` full rebuild
   on `structural_mutation`).
3. **G6 premise proven.** The win is real and the granularity is settled — measurement artifact
   `docs/design/selective-demand-measurement.md` (task **4532**, landed): Scenario A (bracket, body hidden)
   = **100 %** of dependent compute pruneable; Scenario B (2-body, one visible) = **50 %**; on an N-body
   model with k visible the realization-node pruning rate is **(N−k)/N**. Recommendation: **coarse
   per-realization** as the primary source captures the dominant kernel saving.

---

## 3. Substrate ground-truth (verified on main, 2026-06-23)

Everything the decomposition leans on, with wired-vs-declared status:

| Capability | Location | Status |
|---|---|---|
| `DemandRegistry` (`add_demand`/`remove_demand`/`is_demanded`/`rebuild_cone`, backward closure) | `crates/reify-eval/src/demand.rs:17-148` | wired, unit-tested |
| `dirty ∩ demand` filter | `crates/reify-eval/src/dirty.rs:236-248` (`compute_eval_set`) | wired into all edit paths; **no-op under total demand** |
| `build_demand_for_graph` (degenerate-total) | `crates/reify-eval/src/engine_eval.rs:215-228` | wired (the thing α replaces as the warm source) |
| 4532 measurement side-channel (`observed_demand`, `sync_observed_demand`, `measure_would_prune`) | `crates/reify-eval/src/observed_demand.rs`; `gui/src-tauri/src/engine.rs:1873-1914`; `gui/src/App.tsx:523-533` | wired GUI→engine, **structurally isolated** from `compute_eval_set` — the α promotion template |
| Seeded restricted-Kahn planner `run_unified_pass_seeded(traces, seed)` | `crates/reify-eval/src/engine_fixpoint.rs:284-355` | wired (warm/edit driver) |
| Warm tessellate/build sites — **demand-blind** `run_unified_pass(&trace_map)` over whole graph | `crates/reify-eval/src/engine_build.rs:2788, 4144, 7481` | wired but **demand-total** — the β target |
| 4530 structural-mutation rebuild of reverse_index/trace_map/demand | `crates/reify-eval/src/engine_edit.rs:2178-2199` | wired (rebuilds **total** cone; δ extends to selective) |
| `Freshness::Pending { last_substantive: ResultRef }` | `crates/reify-ir/src/value.rs:3445-3467` | **already carries the payload** (stub's "add it" framing was wrong); `ResultRef` is opaque `Option<ContentHash>` |
| `mark_pending` (sets `last_substantive` from `result_hash`) | `crates/reify-eval/src/cache.rs:892` | wired (failure-gating / in-flight only; **no prune producer**) |
| Per-cell Freshness badge (task 2337) | `gui/src-tauri/src/types.rs:890`; `gui/src/panels/PropertyEditor.tsx:242-251` | wired, but the wire layer **deliberately drops** the `last_substantive` payload |
| `last_eval_set` (non-gated accessor) | `crates/reify-eval/src/lib.rs:464`; `engine_admin.rs:1447` | wired |
| `last_dispatch_count` (per-op kernel dispatch counter; **aggregate, not per-body**) | `crates/reify-eval/src/lib.rs:559`; increment `engine_build.rs:5344` | wired; accessor test-gated but `gui/src-tauri` already links `test-instrumentation` (`Cargo.toml:55`) |
| `DemandPruneMeasurement` DTO already on `GuiState` | `gui/src-tauri/src/types.rs:227-266` | wired |
| reify-debug MCP `engine_state` JSON projection | `gui/src-tauri/src/commands.rs:190-218` | wired but **omits** `demand_prune_measurement` + `last_dispatch_count` — the ε gap |
| Viewport visibility tri-state (`show`/`ghost`/`hidden`) + mutation hooks | `gui/src/stores/viewStateStore.ts:15-32, 233-245, 307-312` | wired (α's enforcement-ready source) |

**Substrate that does NOT exist yet** (built in-leaf or queued as a prerequisite):

- **`ContentHash → Value` production accessor.** The cached value lives in `NodeCache.result`
  (`cache.rs:239`) but is reachable only via the test-gated `cache_store()`. γ builds a new always-public
  `Engine::last_substantive_value(node) -> Option<Value>`.
- **Pending-on-prune producer.** No demand-pruning writer of `Freshness::Pending` exists. β/γ adds one
  (reusing `mark_pending`).
- **Per-realization dispatch tally.** `last_dispatch_count` is build-aggregate; a per-body assertion needs a
  small `HashMap<RealizationNodeId, usize>`. ε builds it.
- **Per-realization input-cone hash.** Owned by the sibling **`selective-realization-eviction.md`** PRD
  (its task α). δ consumes it for re-demand staleness (see §6, §9). **Cross-PRD prerequisite.**

No novel `.ri` syntax is introduced — **the grammar gate is N/A** (G3 reduces to the wired-vs-declared
table above).

---

## 4. Resolved design decisions

**D1 — Granularity: coarse per-realization only (this PRD).**
The enforced demand source is **viewport visibility → `Realization` demand roots**; the backward closure
already pulls in the parameter cells that drive each visible body (measurement §2). Fine per-cell demand
(registering displayed property/constraint cells as roots to keep them *fresh* rather than `Pending`) does
**not** change the dominant realization-kernel saving and would require building "displayed-cell subset"
state the GUI does not have (panels render *all* cells today). Deferred to a forward-stub:
`docs/prds/v0_6/selective-demand-fine-per-cell.md` (stays-deferred bookmark; activate only if a future
feature or perf profile exposes a benefit). **Honesty for displayed-but-pruned cells is provided by γ's
`Pending` + last-substantive surfacing, not by keeping them demanded.**

**D2 — `self.demand` is the single selective source of truth; cold paths take a synchronous full-scope
override.**
α populates `self.demand` selectively from visibility; the warm tessellate/driver consults it; the four
existing edit-path `dirty ∩ self.demand` sites go selective *for free and correctly* (a value feeding both a
visible and a hidden body stays in the visible body's backward cone; only values feeding *exclusively*
hidden bodies prune). Cold `build()`/`check()`/CI request **full scope** (total demand) via an explicit
override, so **eager-over-reachable error surfacing is unchanged, deterministic, and synchronous** —
declared-but-hidden geometry errors still surface on the cold path.
*Rejected — per-call `Option<&DemandRegistry>` arg:* keeps two cones to reconcile and loses the value-loop
unification. *Rejected — pure "selective-first + deferred full pass for errors":* the deferred full pass
cannot be relied on synchronously (CI/`check` would still need a sync full path), must be cancelled on every
drag tick (so it surfaces errors only at idle), and pushes a background realization lane + cancellation onto
the core seam. Its genuine wins — **instant-unhide** (pre-warmed caches) and **idle proactive
error-warming** — are separable and layer onto this foundation with no re-architecting; captured in the
forward-stub `docs/prds/v0_6/warm-deferred-full-realization-pass.md` (stays-deferred bookmark).

**D3 — Re-demand staleness is gated by the eviction PRD's input-cone hash.**
A body pruned while hidden, then un-hidden, must not serve stale cached geometry (the `RealizationCache` key
does not move on a value-driven change — `selective-realization-eviction.md` §"Substrate gap"). On
re-demand, δ recomputes the body **iff** its per-realization input-cone hash differs from its
last-executed hash (reuse otherwise). This is a **hard cross-PRD dependency** on
`selective-realization-eviction.md` task α (input-cone hash recorded at execution) — accepted in exchange
for precision (no wasteful unconditional recompute). *Degraded-but-correct fallback if eviction slips:*
unconditional force-recompute on re-demand (see §10 open question).

**D4 — Cold-parity is conditional on full demand; pruning correctness is verified separately.**
The α differential ("all sources registered everything-visible ⇒ byte-identical to total demand") is the
byte-identity spine, and it holds **only** when every realization is demanded. Under genuine pruning a
hidden body is intentionally *not* recomputed, so it is *not* byte-identical to a cold eval — its
correctness is verified by (a) `Pending` + last-substantive display honesty (γ), (b) correct re-realization
on un-hide (δ), and (c) the zero-kernel-work e2e (ε), **not** by the differential.

**D5 — `Freshness::Pending` is load-bearing for correctness, not cosmetic.**
Pending is the marker that a node was *pruned while stale*. A pruned realization is set `Pending` (not left
`Final`), which (a) lets the property panel show last-substantive honestly and (b) signals the re-demand
path that the cached geometry must be re-validated against the input-cone hash. The enum already carries
`last_substantive`; the missing pieces are the **producer** (on prune) and the **`ContentHash → Value`
resolver** for display.

---

## 5. Sketch of approach

1. **α — enforce the demand source + cold override + differential spine.** Promote the 4532
   `sync_observed_demand` path to populate the production `self.demand` selectively (coarse per-realization
   from viewport visibility), firing on visibility change (not only at idle). Add the cold full-scope
   override so `build()`/`check()` stay eager. Land the "all-visible ⇒ byte-identical to total" differential
   (extends `assert_edit_matches_cold`).
2. **β — demand-scoped warm tessellate (the kernel saving).** Switch the demand-blind warm-build sites
   (`engine_build.rs:2788/4144/7481`) from `run_unified_pass(&trace_map)` to a demand-seeded
   `run_unified_pass_seeded` over the backward closure of demanded realizations; cold paths pass full scope.
3. **γ — Pending-on-prune + last-substantive surfacing.** Mark pruned displayed cells `Pending`; add
   `Engine::last_substantive_value`; carry the resolved value + tag through the wire layer, `ValueData`, and
   `PropertyEditor` so a pruned cell shows its prior value + a stale badge.
4. **δ — selective-cone incremental maintenance + re-demand staleness.** Keep the *selective* cone coherent
   across structural edits (inherits 4530's rebuild invariant; re-derive selective roots from grown
   instances' visibility). Gate re-demand recompute on the eviction input-cone hash.
5. **ε — debug-MCP observability + the integration-gate e2e.** Surface `demand_prune_measurement` +
   `last_dispatch_count` + a per-realization dispatch tally through `engine_state`; the e2e drives a
   hidden-body slider session asserting **zero kernel dispatch attributable to the hidden body**, then
   un-hides and asserts correct (non-stale) re-realization.
6. **ζ — companion cross-PRD prose corrections.**

---

## 6. Pre-conditions for activating

- θ **4361** + θ2 **4531** landed (demand-scoped seeded driver on warm + edit paths). ✓
- **4530** landed (dep-structure rebuild invariant). ✓
- **4532** measurement artifact committed; win + coarse-granularity confirmed (G6). ✓
- **`selective-realization-eviction.md` task α (per-realization input-cone hash) available upstream** —
  required by δ for re-demand staleness (D3). The eviction PRD must be expanded + decomposed at least
  through its α before selective-demand's δ leaf can be wired. **This is the gating sequencing item for
  decompose** (see §9 and the hand-back).
- Cutover ι **4362** desirable but not strictly required (can develop behind the unified-dag flag).

---

## 7. Contract (H)

Two seams. An architect reading this section can implement the producer side without further discussion.

### 7.1 Demand-scoped tessellation seam

- **Single source of truth.** `Engine.demand: DemandRegistry` holds the **selective** cone in warm/GUI
  operation. It is populated from viewport visibility (coarse per-realization): each viewport-visible body
  contributes `add_demand(NodeId::Realization(...))`; `rebuild_cone` pulls the backward closure (the
  parameter/value cells those realizations read). Visibility *removal* calls `remove_demand` +
  `rebuild_cone`. Population reuses the 4532 source→NodeId parsers (`engine.rs:4675-4709`).
- **Warm schedule.** The warm tessellate/build sites compute
  `seed = compute_eval_set(dirty, &self.demand, &trace_map)` (or, where they currently bypass demand
  entirely, the backward closure of demanded realizations) and drive `run_unified_pass_seeded(&trace_map,
  &seed)`. A realization absent from the seed is **not executed**.
- **Cold full-scope override (invariant: eager errors).** `build()`/`check()`/CI request total demand via an
  explicit parameter/flag (NOT by mutating `self.demand`); cold scheduling is byte-identical to today and
  **surfaces every declared-but-reachable geometry error synchronously**. The selective `self.demand` is
  never consulted on the cold path.
- **Cold-parity invariant.** When every realization is demanded, the warm seed == the cold full schedule
  and the resulting value map is byte-identical (canonical `content_hash`), under both `BuildScheduler`
  variants.
- **Prune-safety invariant.** A pruned realization's cached result is **never served as `Final`**; it is
  marked `Freshness::Pending { last_substantive }`.
- **Re-demand rule.** A realization re-entering demand is recomputed **iff** its input-cone hash (from
  `selective-realization-eviction`) differs from its last-executed hash; otherwise the cached geometry is
  reused. Absent the eviction hash (degraded fallback), recompute unconditionally.

### 7.2 Pending-on-prune freshness seam

- **Producer.** On prune of a displayed cell/realization, set `Freshness::Pending { last_substantive }` via
  `mark_pending` (already captures `last_substantive = ResultRef::of_hash(entry.result_hash)`). Spec §7.2
  (`docs/reify-implementation-architecture.md:748`): Pending "retains the most recent substantive result …
  but does NOT trigger downstream re-evaluation" — pruning naturally quiets the downstream subtree.
- **Resolver (new).** `Engine::last_substantive_value(node: &NodeId) -> Option<Value>` — always-public,
  mirrors `Engine::freshness` (`engine_admin.rs:1643`); reads the node's cached `CachedResult::Value` when
  freshness is `Pending`. This is the genuinely missing primitive (`ResultRef` is identity-only; the value
  store is test-gated today).
- **GUI surfacing.** The wire layer stops dropping the payload for pruned cells: `ValueData` gains
  `last_substantive_value: Option<String>` (`types.rs:585-603` + mirror `gui/src/types.ts`); `build_values`
  (`engine.rs:3223-3283`) populates it via the resolver when freshness is `pending`; `PropertyEditor`
  renders the prior value + the existing `⚠` Pending badge. **Never a silently-stale number.**
- **Lifecycle.** prune → `Pending` → GUI reads `last_substantive_value`; re-demand → recompute (per §7.1
  re-demand rule) → `Final`.

---

## 8. Boundary-test sketch (H) — the ε integration-gate signal

Each row faces **both** the producer (engine scheduler) and consumer (GUI / debug-MCP) sides.

| # | Scenario | Preconditions | Postconditions asserted |
|---|---|---|---|
| 1 | **All-visible == total (cold-parity)** | every realization demanded | warm `last_eval_set` == cold eval set; value map byte-identical (`assert_edit_matches_cold` + `_with_solver` extension) under both schedulers |
| 2 | **Hidden body pruned (kernel saving)** | 2-body model, `body_b` viewport-hidden; slider session on shared `drive` | `body_b`'s `NodeId::Realization` absent from `last_eval_set` each edit; `would_prune.realization >= 1`; **per-realization dispatch tally for `body_b` == 0** across the session |
| 3 | **Displayed-but-pruned cell honesty** | `body_b` hidden, one of its property cells displayed | cell shows `last_substantive_value` + `pending` badge; `GuiState.values[cell].freshness == "pending"`; the displayed number equals the last good value, not the current (un-recomputed) one |
| 4 | **Un-hide refresh correctness** | `body_b` pruned across N param edits, then un-hidden | `body_b` re-realizes; input-cone-hash gate fires recompute (or reuse iff unchanged); resulting geometry == cold-build of current params (no stale handle) |
| 5 | **Collection-grow coherence** | a `forall` grows instances mid-edit; some grown instances hidden, some visible | grown hidden instance pruned; grown visible instance realized correctly; selective cone rebuilt from selective roots — **no silent wrong-prune of a visible grown body** (the 4530-staleness-becomes-wrong-pruning hazard) |
| 6 | **Cold eager errors preserved** | a hidden body whose realization would error | warm loop prunes it (no error mid-drag); `check()`/cold `build()` still surfaces the error synchronously |

ε's observable signal **is** this table, exercised via the reify-debug MCP (zero-kernel-work, rows 2/4)
plus engine differential tests (rows 1/5) and GUI store-state assertions (rows 3/6).

---

## 9. Decomposition plan

Greek labels; task IDs assigned at decompose time. **DAG:** α → β → {γ, δ}; {β, γ, δ} → ε; ζ after ε.
Cross-PRD: **δ → selective-realization-eviction α** (external input-cone hash).

| Label | Title | Modules | Observable signal | Prereqs |
|---|---|---|---|---|
| **α** | Enforce viewport-visibility demand source + cold full-scope override + all-visible differential | reify-eval (`demand` population, cold override), gui/src-tauri (`sync_observed_demand`→`self.demand`), gui frontend (fire on visibility change) | Differential test `all_visible_selective_matches_total` GREEN under both schedulers; with a body hidden, `self.demand` is selective (`cone_size < total`) while cold `check()` stays full | — (4361/4531/4530/4532 landed) |
| **β** | Demand-scoped warm tessellate/driver (kernel-time saving) | reify-eval (`engine_build.rs:2788/4144/7481` + seeded driver) | Hidden body's `Realization` excluded from `last_eval_set`; `would_prune.realization >= 1`; hidden body executes **0** kernel ops (per-realization tally) on a slider session | α |
| **γ** | Pending-on-prune producer + `Engine::last_substantive_value` resolver + GUI last-substantive surfacing | reify-eval (prune producer, resolver), gui/src-tauri (wire + `ValueData`), gui frontend (`PropertyEditor`) | Property-panel cell of a hidden/pruned body shows its prior value + `pending` ⚠ badge (`GuiState.values[cell].freshness=="pending"`), never a silently-stale number | β |
| **δ** | Selective-cone incremental maintenance across structural edits + re-demand staleness gate | reify-eval (`engine_edit.rs` structural-mutation rebuild → selective roots; re-demand hash gate) | Collection-grow differential: grown hidden instance pruned, grown visible instance correct; un-hide of a previously-pruned body yields correct current-param geometry (input-cone-hash-gated recompute) | β; **selective-realization-eviction α (cross-PRD)** |
| **ε** | reify-debug MCP demand observability (engine_state + per-realization tally) + integration-gate e2e (§8 boundary sketch) | gui/src-tauri (`engine_state` JSON + new debug command), reify-eval (per-realization dispatch tally), tests | The §8 table GREEN: a hidden-body slider session reports **zero kernel dispatch attributable to the hidden body** via debug-MCP, and un-hide re-realizes correctly | β, γ, δ |
| **ζ** | Companion cross-PRD prose corrections | docs | `selective-realization-eviction.md` records the reciprocal "selective-demand consumes the input-cone hash" seam; arch §3.2/§3.3 points at the realized PRD | ε |

**G2 classification:** α is foundation-with-its-own-differential-signal (intermediate, unlocks β/γ/δ);
β/γ/δ are leaves with engine/GUI-observable signals; **ε is the integration-gate leaf** naming the §8
boundary sketch; ζ is the companion-correction leaf. No leaf's signal is "a unit test passes against
synthetic input."

---

## 10. Cross-PRD relationship (G4)

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `docs/prds/v0_6/selective-realization-eviction.md` (task 4534) | **consumes** | per-realization **input-cone hash** on re-demand (δ) | **selective-realization-eviction** (its task α) | **queued** — hard prereq for δ; eviction must be decomposed ≥α first |
| `docs/prds/v0_6/engine-unified-build-dag.md` (θ/θ2/ι) | consumes | `run_unified_pass_seeded` warm/edit driver | engine-unified-build-dag | wired (4361/4531 landed; 4362 optional) |
| `docs/prds/freshness-4-variant.md` / task 2337 | consumes | `Freshness::Pending { last_substantive }` enum + per-cell badge | that PRD (done) | wired |
| `docs/prds/v0_6/selective-demand-fine-per-cell.md` (new stub) | produces-for | future fine per-cell demand roots | fine-per-cell stub | deferred bookmark |
| `docs/prds/v0_6/warm-deferred-full-realization-pass.md` (new stub) | produces-for | low-priority deferred full pass (instant-unhide, idle error-warming) | deferred-full-pass stub | deferred bookmark |

**No reciprocal-ownership ambiguity.** The two siblings are complementary and the boundary is one-directional:
*demand prunes invisible work; eviction prunes unaffected work.* Eviction owns the input-cone hash;
selective-demand consumes it. ζ records the reciprocal note in the eviction stub.

---

## 11. Out of scope

- **Fine per-cell demand** (property/constraint-panel cells as demand roots) → `selective-demand-fine-per-cell.md` (deferred).
- **Low-priority deferred full realization pass** (instant-unhide cache pre-warming + idle proactive error surfacing) → `warm-deferred-full-realization-pass.md` (deferred).
- **Cold `build()` demand-gating** — declined by design (eager error surfacing); selective demand is warm/GUI-only.
- **Selective realization *eviction*** (pruning *unaffected* work) — sibling PRD `selective-realization-eviction.md`. This PRD only *consumes* its hash.
- **Changing `RealizationCache` tolerance partial-order semantics.**

---

## 12. Open questions (tactical — surfaced but not decided)

1. **Eviction-α slip mitigation.** If `selective-realization-eviction` α slips, δ ships the degraded
   fallback (unconditional force-recompute on re-demand — correct, slightly wasteful) and a follow-up swaps
   in the hash gate. **Suggested resolution:** wire the cross-PRD dep; only fall back if the eviction batch
   is not decomposed by the time δ is dispatched. Decide at δ dispatch.
2. **Per-realization tally vs eval-set membership for ε.** A `HashMap<RealizationNodeId, usize>` dispatch
   tally gives a crisp per-body "0 ops" assertion; `last_eval_set` membership + aggregate `last_dispatch_count`
   is cheaper but only proves "not scheduled," not "0 ops." **Suggested resolution:** build the small tally
   (the assertion is the headline signal). Decide during ε.
3. **Visibility-change demand-update trigger.** Today `sync_observed_demand` fires only at
   `phase === 'idle'` (`App.tsx:523`). Enforced demand should update on the visibility toggle itself.
   **Suggested resolution:** fire `add_demand`/`remove_demand` + `rebuild_cone` synchronously on
   `setVisibility`/`cycleCascading`, debounced. Decide during α.
4. **`ghost` visibility tri-state.** `viewStateStore` has `show`/`ghost`/`hidden`; is `ghost` (translucent)
   demanded? **Suggested resolution:** treat `ghost` as demanded (it is rendered) — only `hidden` prunes.
   Decide during α.
