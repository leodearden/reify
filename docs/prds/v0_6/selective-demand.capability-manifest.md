# Capability Manifest — selective-demand

Mechanizes G3 + G6 per leaf for `docs/prds/v0_6/selective-demand.md`. Evidence verified by
read-the-code investigation at PRD commit `9d9bb761c8` (== `main` HEAD at decompose time). Every
anchor was re-grepped this session; **re-locate each at implementation time — the engine moves fast and
line numbers drift** (already observed: `run_unified_pass_seeded` 284→273, the 4530 structural-mutation
rebuild 2178→1538). Prefer the **function/symbol names** over the line numbers.

**Substrate-verification workflow (`scripts/prd-decompose-verify.mjs`) — N/A by premise shape, ran for
confirmation.** Its three probe vectors target `.ri` premises: grammar (`tree-sitter parse`), semantic
(`reify check`), eval/IR (`reify eval`); plus numeric-floor and negative-assertion checks. This PRD is
**engine-internals + GUI wiring (Rust/TS)** and asserts **none** of those classes: **no novel `.ri`
syntax** (grammar-fixture N/A), **no rejection/negative assertions** (rejection-mechanism N/A), and the
only numeric premise (ε's "zero kernel dispatch") is an op-count **equality/floor-of-0**, structural by
construction — not an accuracy tolerance (numeric-floor degenerate). The workflow N/A's every binding;
this hand-bound manifest pays the substrate check here.

---

## α — enforce viewport-visibility demand source + cold full-scope override + all-visible differential (intermediate — unlocks β/γ/δ)

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| `sync_observed_demand` promotion template (the 4532 side-channel α promotes to populate `self.demand`) | wired-on-main | `grep:gui/src-tauri/src/engine.rs:1873` (`pub fn sync_observed_demand`) — structurally isolated from `compute_eval_set` today; α wires it into the production registry | **PASS** |
| `build_demand_for_graph` (the degenerate-total source α replaces as the warm source) | wired-on-main (replace) | `grep:crates/reify-eval/src/engine_eval.rs:215` (`pub(crate) fn build_demand_for_graph`) | **PASS** |
| `DemandRegistry` (`add_demand`/`remove_demand`/`is_demanded`/`rebuild_cone`) | wired-on-main, unit-tested | `crates/reify-eval/src/demand.rs:17-148` | **PASS** |
| differential harness to extend (`all_visible_selective_matches_total`) | wired-on-main (extend) | `grep:crates/reify-eval/tests/common/differential.rs:1552` (`assert_edit_matches_cold`) + `:1591` (`_with_solver` variant for "both schedulers") | **PASS** |
| viewport visibility tri-state source (`show`/`ghost`/`hidden`) | wired-on-main | `gui/src/stores/viewStateStore.ts:15-32, 233-245, 307-312` | **PASS** |
| cold full-scope override (the eager-errors invariant) | producer = α (NEW, this leaf) | `producer:this-leaf` — α adds the explicit total-demand flag/param on `build()`/`check()`; cold path never consults `self.demand` (D2). No substrate gap (a scope flag, not new capability) | **PASS** |
| grammar-fixture / rejection / numeric-floor | — | N/A (no `.ri` syntax; cone-size/byte-identity comparison; no negative assertion) | **N/A** |

## β — demand-scoped warm tessellate/driver (kernel-time saving) (leaf)

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| selective `self.demand` populated from visibility | capability→producer + DAG-direction | `producer:α` — **upstream** | **PASS** |
| seeded restricted-Kahn planner `run_unified_pass_seeded(traces, seed)` | wired-on-main | `grep:crates/reify-eval/src/engine_fixpoint.rs:273` (warm/edit driver, landed 4361/4531) | **PASS** |
| demand-blind warm-build sites to switch (`run_unified_pass(&trace_map)` over whole graph) | wired-on-main (the β target) | `grep:crates/reify-eval/src/engine_build.rs:2788, 4144, 7481` — all three present (`run_unified_pass(&state.snapshot.graph, &state.trace_map)`) | **PASS** |
| `dirty ∩ demand` filter (`compute_eval_set`) — already wired, no-op under total demand | wired-on-main | `grep:crates/reify-eval/src/dirty.rs:236` (`fn compute_eval_set`) + `:243` (`.filter(|n| demand.is_demanded(n))`) | **PASS** |
| `last_eval_set` (β's exclusion signal) | wired-on-main | `grep:crates/reify-eval/src/lib.rs:464` (field) + accessor `engine_admin.rs` | **PASS** |
| `would_prune.realization >= 1` (β's prune-count signal) | wired-on-main | `grep:crates/reify-eval/src/observed_demand.rs:34` (`would_prune: WouldPruneByKind`) + `:76` (`measure_would_prune`) | **PASS** |
| per-realization "0 kernel ops" tally backing | producer | `producer:ε` (the `HashMap<RealizationNodeId, usize>` tally) — **downstream of β**, but β's own signal also satisfiable via `last_eval_set` exclusion + aggregate `last_dispatch_count` (PRD §12 Q2). The crisp per-body "0 ops" assertion is **ε's** headline; β does not strictly require the per-realization map | **PASS** (β signal independently satisfiable; the per-body refinement is ε's) |
| numeric-floor / grammar-fixture / rejection | — | N/A | **N/A** |

## γ — Pending-on-prune producer + `Engine::last_substantive_value` resolver + GUI last-substantive surfacing (leaf)

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| `Freshness::Pending { last_substantive: ResultRef }` (the **anti-fiction** binding — stub's "add it" framing was wrong) | wired-on-main (already carries payload) | `grep:crates/reify-ir/src/value.rs:3462` (`Pending { last_substantive: ResultRef }`) — the enum field **already exists** (NOT declared-only); `ResultRef` is opaque `Option<ContentHash>` | **PASS** |
| `mark_pending` (sets `last_substantive` from `result_hash`) — γ's prune producer reuses it | wired-on-main | `grep:crates/reify-eval/src/cache.rs:892` (`pub fn mark_pending`) + `:895` (`last_substantive: ResultRef::of_hash(entry.result_hash)`) | **PASS** |
| pending-on-prune **producer** (a demand-pruning writer of `Pending`) | producer = γ (NEW, this leaf) | `producer:this-leaf` — no demand-pruning writer exists today (`mark_pending` is failure-gating/in-flight only); γ adds the prune-site call | **PASS** |
| `ContentHash → Value` production accessor `Engine::last_substantive_value(node) -> Option<Value>` | producer = γ (NEW, this leaf) | `producer:this-leaf` — **field-population PASS**: the value lives in `NodeCache.result` (`grep:crates/reify-eval/src/cache.rs:239`, real `CachedResult`, non-sentinel), today reachable only via test-gated `cache_store()`; γ adds the always-public accessor (mirrors `Engine::freshness`). The value store is real, not `Undef` | **PASS** |
| wire layer that **deliberately drops** the `last_substantive` payload (γ stops dropping it for pruned cells) | wired-on-main (extend) | `grep:gui/src-tauri/src/types.rs:890` (`format_freshness`) + `:882` ("Payload fields … are deliberately [dropped]"); γ adds `ValueData.last_substantive_value: Option<String>` populated by the resolver | **PASS** |
| per-cell Freshness badge (`pending` ⚠) | wired-on-main | `gui/src/panels/PropertyEditor.tsx:242-251` (task 2337) | **PASS** |
| numeric-floor / grammar-fixture / rejection | — | N/A | **N/A** |

## δ — selective-cone incremental maintenance across structural edits + re-demand staleness gate (leaf)

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| 4530 structural-mutation rebuild of reverse_index/trace_map/demand (δ extends total→selective) | wired-on-main (extend) | `grep:crates/reify-eval/src/engine_edit.rs:1538` ("install at the end rebuilds reverse_index/trace_map/demand") + `structural_mutation` gating `:1540-1645` (task 4530) | **PASS** |
| selective demand roots to re-derive from grown instances' visibility | capability→producer + DAG-direction | `producer:α` (the selective source) / `producer:β` (the selective warm schedule) — **upstream** | **PASS** |
| **per-realization input-cone hash** (the re-demand staleness gate, D3) | capability→producer + DAG-direction | `producer:task-4728` (`selective-realization-eviction` α — "record per-realization input-cone hash on `RealizationNodeData` at execution") — **cross-PRD, UPSTREAM** (4728 is `pending`, wired as a real bare-integer `add_dependency` edge). DAG-direction PASS (prereq, not dependent) | **PASS** |
| degraded fallback if 4728 slips | producer = δ (NEW, this leaf) | `producer:this-leaf` — unconditional force-recompute on re-demand (correct, slightly wasteful; PRD §12 Q1). No substrate gap | **PASS** |
| `GeometryHandleId` equivalence (un-hide correctness comparison) | wired-on-main | `grep:crates/reify-ir/src/geometry.rs` (`GeometryHandleId`, `content_hash`) — used by the un-hide-vs-cold-build assertion | **PASS** |
| numeric-floor / grammar-fixture / rejection | — | N/A (handle-equivalence, not a tolerance; no negative assertion) | **N/A** |

## ε — reify-debug MCP demand observability + integration-gate e2e (§8 boundary sketch) (leaf — the headline integration gate)

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| `engine_state` JSON projection (the ε gap — omits the demand fields today) | wired-on-main (extend) | `grep:gui/src-tauri/src/commands.rs:190` (`pub fn engine_state_json`) — returns meshes/values/constraints/files/diagnostics/stale/reload_error; **confirmed omits** `demand_prune_measurement` + `last_dispatch_count`; ε adds them | **PASS** (the gap is real; ε fills it) |
| `DemandPruneMeasurement` DTO already on `GuiState` | wired-on-main | `gui/src-tauri/src/types.rs:227-266` | **PASS** |
| `last_dispatch_count` (per-op kernel dispatch counter; aggregate) | wired-on-main | `grep:crates/reify-eval/src/lib.rs:559` (field) + accessor `engine_admin.rs` (`#[cfg(any(test, feature="test-instrumentation"))]`; `gui/src-tauri` already links `test-instrumentation`, `Cargo.toml:55`) | **PASS** |
| `would_prune` / `measure_would_prune` (the §8 row-2 prune-count assertion) | wired-on-main | `grep:crates/reify-eval/src/observed_demand.rs:34` + `:76` | **PASS** |
| **per-realization dispatch tally** `HashMap<RealizationNodeId, usize>` (the crisp per-body "0 ops") | producer = ε (NEW, this leaf) | `producer:this-leaf` — `last_dispatch_count` is build-aggregate; ε builds the per-body map (PRD §12 Q2 resolved: build the tally) | **PASS** |
| the whole prune chain (β/γ/δ) the e2e exercises | capability→producer + DAG-direction | `producer:β,γ,δ` — all **upstream** (ε `depends_on` all three) | **PASS** |
| **numeric floor — "zero kernel dispatch attributable to the hidden body"** | numeric-floor (G6 branch-3 end-to-end) | `floor:0`, `bound:0` — **exact-by-construction**: a pruned realization never enters the eval set → `execute_realization_ops` is never called for it → 0 increments. An op-count equality, not an accuracy tolerance; the "0 ops" is structurally achievable, not a guessed bound | **PASS** |
| grammar-fixture / rejection | — | N/A | **N/A** |

## ζ — companion cross-PRD prose corrections (leaf — docs-only)

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| `selective-realization-eviction.md` exists to record the reciprocal seam | wired-on-main | `docs/prds/v0_6/selective-realization-eviction.md` (committed; task 4534 / α-ε batch 4728-4732) | **PASS** |
| arch §3.2/§3.3 demand-registry / two-cone prose to point at the realized PRD | wired-on-main | `docs/reify-implementation-architecture.md` §3.2/§3.3 | **PASS** |
| dependency on ε landing (so the prose describes a real, not aspirational, realization) | capability→producer + DAG-direction | `producer:ε` — **upstream** | **PASS** |
| numeric-floor / grammar-fixture / rejection | — | N/A (docs-only) | **N/A** |

---

**No FAIL bindings.** The three genuinely-new primitives are all `producer:this-leaf` (built where consumed,
not orphaned): α's cold full-scope override, γ's pending-on-prune producer + `last_substantive_value`
resolver, ε's per-realization dispatch tally. The one cross-PRD binding (δ's input-cone hash) is a real
**upstream** `add_dependency` edge to `selective-realization-eviction` α = task **4728** (`pending`),
wired as a bare-integer intra-project edge (same project). The anti-fiction watch-item — γ's
`Freshness::Pending { last_substantive }` — is **already on main** with a real payload field (the stub's
"add the field" framing was wrong); γ adds only the *producer* and the *resolver*, both of which read a
real non-`Undef` value from `NodeCache.result`. Queue-blocking prerequisites (4361/4531/4530/4532, and
4362 — all `done`) are landed; δ's cross-PRD prereq (4728) wired as a real edge at decompose time.
