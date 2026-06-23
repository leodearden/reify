# Capability Manifest — selective-realization-eviction

Mechanizes G3 + G6 per leaf for `docs/prds/v0_6/selective-realization-eviction.md`. Evidence verified
by read-the-code investigation at `main 3f0304ead1` / PRD commit `a5d62c0716` (re-locate every anchor
at implementation time — the engine moves fast).

**Substrate-verification workflow (`scripts/prd-decompose-verify.mjs`) — N/A by premise shape.** Its
three probe vectors target `.ri` premises: grammar (`tree-sitter parse`), semantic (`reify check`),
and eval/IR (`reify eval`); plus numeric-floor and negative-assertion checks. This PRD is **pure
engine-internals (Rust)** and asserts **none** of those classes: **no novel `.ri` syntax**
(grammar-fixture N/A), **no numeric bounds** (the ε signal is an op-count *equality*, not a tolerance
— numeric-floor N/A), and **no rejection/negative assertions** (rejection-mechanism N/A). Every
premise is a Rust symbol-existence/wiring fact, verified directly with `file:line` anchors below
(three independent read-the-code passes). The workflow would N/A every binding; the hand-bound
manifest pays the substrate check here.

## α — record per-realization input-cone hash on `RealizationNodeData` at execution (intermediate)

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| `RealizationNodeData` exists to extend with the hash field | wired-on-main (extend) | `grep:crates/reify-eval/src/graph.rs:46-78` (no result-hash field today; α adds it) | **PASS** (producer of the field = α) |
| `ComputeNodeData.result_content_hash` precedent shape | wired-on-main | `grep:crates/reify-eval/src/graph.rs:164` (+ clone `:194`) | **PASS** |
| `compute_realization_upstream_values_hash` reuse (the GHR-β input-cone fold) | wired-on-main | `grep:crates/reify-eval/src/engine_build.rs:~8076` — folds `eval_expr(arg,ctx).content_hash()` per op, skips `CrossSubGeometryRef`, excludes ephemeral `kernel_handle` | **PASS** |
| population site = `execute_realization_ops` per-realization slot | wired-on-main | `grep:crates/reify-eval/src/engine_build.rs:~4748` (per-op dispatch region) | **PASS** |
| **field-population (the result-field twin)** | producer writes a non-`None` hash on the **production** path | α populates in `execute_realization_ops` (production), not a `tests/` helper — verify at impl the write is on the production path, not test-only | **PASS** (producer = α, production path by design) |

## β — recompute-then-compare seeding + first production caller of `compute_dirty_cone_with_realizations` (intermediate)

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| stored input-cone hash on the realization | capability→producer + DAG-direction | `producer:α` — **upstream** | **PASS** |
| `compute_dirty_cone_with_realizations` (gets its first caller) | wired-on-main (function) + producer (caller) | `grep:crates/reify-eval/src/dirty.rs:95` — `pub`, caller-less, staged (`dirty.rs:84-94`); β IS the first production caller (anti-orphan resolution) | **PASS** (β supplies the missing consumer wiring) |
| `diff_realizations` / old-new-graph diff site (`edit_source` compare) | wired-on-main (extend) | `grep:crates/reify-eval/src/engine_edit.rs:569` (def) + `:2365` (the one production call) — β compares the **input-cone** hash, not the static `content_hash` | **PASS** |
| edit entries (`edit_param` / `edit_source`) | wired-on-main | `grep:crates/reify-eval/src/engine_edit.rs:901` / `:2292` | **PASS** |
| direct `realization_inputs` edge on ComputeNodes (why the realization-seeded cone is needed) | wired-on-main | `grep:crates/reify-eval/src/graph.rs:159` (`realization_inputs: Vec<RealizationNodeId>`) | **PASS** |

## γ — keyed eviction replaces wholesale flush + task-2874 contract-lock supersession (leaf)

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| `clear_realization_cache` to replace (both edit entries) | wired-on-main (replace) | `grep:crates/reify-eval/src/engine_admin.rs:644` (def) + call sites `engine_edit.rs:901`/`:2292` | **PASS** |
| `RealizationCache` keyed-evict primitive | wired-on-main (extend) | `grep:crates/reify-eval/src/realization_cache.rs` (struct + insert/lookup; a `remove`/`clear` primitive may be partially present — task 2764); γ adds per-`(entity_id,*)`-family eviction composing with `tolerance_bucket.rs:61-114` | **PASS** (producer of the API addition = γ) |
| `changed_realizations` set | capability→producer + DAG-direction | `producer:β` — **upstream** | **PASS** |
| `BuildScheduler` flag toggle (Legacy=wholesale / Unified=selective) so δ can run both | wired-on-main | `grep:crates/reify-eval/src/engine_fixpoint.rs:21-33` (`BuildScheduler::{LegacyMultiPass,UnifiedDag}`, landed 4361) | **PASS** |
| task-2874 contract-lock pins to supersede | wired-on-main (supersede, not delete — D5) | `grep:crates/reify-eval/tests/tolerance_wiring_e2e.rs:830` (step-17) / `:959` (step-19) / `:1069` (step-21/22) | **PASS** |

## δ — staleness differential corpus / H boundary test (leaf)

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| both regimes runnable (wholesale baseline ‖ selective) | capability→producer + DAG-direction | `producer:γ` (the Legacy/Unified flag split of the flush) — **upstream** | **PASS** |
| corpus harness | wired-on-main | `crates/reify-eval/tests/` (new differential file; `metadata.files=[]` → BRE acquires footprint) | **PASS** |
| `GeometryHandleId` equivalence comparison | wired-on-main | `grep:crates/reify-ir/src/geometry.rs:8-30` (`GeometryHandleId`, `content_hash`) | **PASS** |
| collection-grow rebuild invariant (a corpus scenario) | capability→producer + DAG-direction | `producer:task-4530` (reverse_index/trace_map/demand rebuild) — `done`, **upstream** | **PASS** |
| grammar-fixture / numeric-floor / rejection | — | N/A (no `.ri` syntax; handle-equivalence, not a tolerance; no negative assertion) | **N/A** |

## ε — e2e slider-drag dispatch-count gate (leaf — the headline integration gate)

| Capability | Check | Evidence | Verdict |
|---|---|---|---|
| `Engine::last_dispatch_count()` (the §2 signal's backing — **G6 branch-3 end-to-end**) | wired-on-main | `grep:crates/reify-eval/src/engine_admin.rs:~1570` (accessor, `#[cfg(any(test, feature="test-instrumentation"))]`) + increment `engine_build.rs:~4748` + reset per build entry | **PASS** |
| selective eviction mechanism (the whole chain) | capability→producer + DAG-direction | `producer:γ` (and α/β transitively) — **upstream** | **PASS** |
| multi-body isolating fixture under the unified flag | producer | `producer:ε` — the fixture isolates which body, so the **global** counter is unambiguous (no per-realization counter built — PRD §10) | **PASS** |
| numeric-floor | — | N/A — op-count **equality** (`== ops(A)`, `== 0`), not an accuracy tolerance | **N/A** |
| grammar-fixture | — | N/A — no novel `.ri` syntax | **N/A** |

**No FAIL bindings.** The one anti-orphan binding worth flagging — `compute_dirty_cone_with_realizations`
is dead code on main — is **resolved** by β being its first production caller (the function exists and
was staged precisely for this; β is the consumer wiring the PRD exists to add). Queue-blocking
prerequisites (4361, 4531) are `done`; wired as real `add_dependency` edges on α at decompose time.
