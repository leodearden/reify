# PRD — Selective realization eviction (input-cone-hash incremental geometry)

**Milestone:** v0_6 · **Status:** ACTIVE (expanded from forward-stub 2026-06-22; preconditions
4361 + 4531 landed) · **Approach B + H** (correctness-critical core-engine cache/edit seam —
contract + two-way boundary tests below) · **Date:** 2026-06-22
**Parent:** `docs/prds/v0_6/engine-unified-build-dag.md` D8 + §9 "Out of scope"
("Selective realization eviction … a follow-up after `RealizationNodeData` result hashing exists").
**Design source:** `docs/design/engine-unified-build-dag-option-a.md` §5.2 (the dead-code
incremental machinery + the "4317-class stale" warning) and §3.6 (the **input-hashes-not-output**
rule this PRD realizes).
**Tracker:** `[MILESTONE]` task **4534** (active-intervention; this expansion discharges it).
**Code anchors** are as of current `main` (`3f0304ead1`); the engine moves fast — **re-locate every
symbol at implementation time**.

---

## 1. Goal — a slider drag re-realizes only the bodies it affects

Every edit conservatively flushes the **entire** `RealizationCache`
(`Engine::clear_realization_cache`, `engine_admin.rs:644`, called unconditionally at
`edit_param` / `edit_source` entry — `engine_edit.rs:901` / `:2292`). The engine cannot prove which
cached entries survive a given edit without per-realization input-cone analysis it does not maintain,
so a one-param slider drag cold-misses **every** realization on the next build surface and re-runs
kernel ops for **all** bodies, affected or not.

Make the cache **input-cone-aware**: record a per-realization input-cone hash, and on edit evict only
the realizations whose inputs actually moved. Unaffected bodies stay cached and skip kernel
re-execution. This is the warm/edit-loop latency win the unified-DAG red-team deliberately scoped out
of θ (D8) — deferred until "`RealizationNodeData` result hashing exists." It exists now in seed form
(GHR-β's `upstream_values_hash`); this PRD records it on the realization and wires it to eviction.

## 2. User-observable surface (what proves it landed)

- **Slider-drag dispatch reduction (the headline).** On a multi-body fixture where the edited param
  feeds **only body A**, after `edit_param` + `build_snapshot()` the test seam
  `Engine::last_dispatch_count()` (`engine_admin.rs:~1570`, `#[cfg(any(test, feature =
  "test-instrumentation"))]`) reports **`== ops(A)`**, strictly less than the all-bodies count the
  wholesale flush produces today. (`slider_drag_reexecutes_only_affected_body_e2e`.)
- **Unaffected-edit zero-dispatch.** Dragging a param that feeds **no** realization (e.g. a
  display-only annotation) yields `last_dispatch_count() == 0` on the next build (every realization
  cache-hits) — today it is the full all-bodies count.
- **No stale geometry.** Across the staleness corpus (param edit, guard flip, collection grow,
  source edit) the selective path serves a `GeometryHandleId` **byte-equivalent** to what the
  wholesale-flush baseline would have produced — never a handle the flush would have evicted. The
  superseding contract-lock tests (replacing the task-2874 "cache is empty after edit" pins) assert
  *affected realizations re-execute and unaffected ones hit*, not *the cache is empty*.
- **Downstream re-eval correctness.** A `@optimized` ComputeNode with a direct `realization_inputs`
  edge (#10) re-evaluates **iff** its input realization's input-cone moved — observable through the
  node's output value cell freshness, with no false-fresh stale result.

## 3. Background — why deferred, and the verified substrate gap

The unified-DAG red-team scoped this out of θ (D8) because the incremental machinery that exists is
**not usable as-is**, and a naive wiring would re-introduce the 4317-class stale (task 4317, `done`):

- `compute_dirty_cone_with_realizations` (`dirty.rs:95`) has **no production caller** — it is `pub`
  only to escape the dead-code lint, exercised solely by `dirty.rs` tests, staged for "P3.4+". Its
  seed-discrimination contract is already documented (`dirty.rs:76-94`): the caller must supply only
  realizations whose result content-hash **actually differs**.
- `diff_realizations` (`engine_edit.rs:569`, one production caller at `:2365` inside `edit_source`)
  keys on the **static IR `content_hash`** — `RealizationNodeData.content_hash` is
  `id_hash.combine(ops_hash)` (`graph.rs:381`), IR identity + compiled ops only. It **never moves on
  a value-driven geometry change** (slide a `width` param: the `Primitive{Box, args:[("width",
  ValueRef(width))]}` op IR is identical, so the hash is identical). Wiring propagation on it would
  silently no-op — "a guaranteed future 4317-class stale" (design §5.2).

**The substrate that DOES exist (verified, current `main`):**

- **The input-cone hash already exists as the GHR-β `upstream_values_hash`.**
  `compute_realization_upstream_values_hash(realization, ctx)` (`engine_build.rs:~8076`) folds, for
  every geometry op, each **argument expression evaluated against the current `EvalContext`**
  (`eval_expr(arg, ctx).content_hash()`), skipping `CrossSubGeometryRef` args. It is recorded on the
  `Value::GeometryHandle { realization_ref, upstream_values_hash, kernel_handle }` value
  (`reify-ir/src/value.rs:417`) at hydration. This is **an input hash, not an output hash** — exactly
  the spec §3.6 shape — and it is computable **before** kernel execution (it reads arg expressions,
  not kernel output).
- **Output hashing is impossible by design.** `kernel_handle` is ephemeral and **excluded** from
  `GeometryHandleRef` identity (`value.rs:425`, GHR-β §DD) and from `content_hash` — re-realizing the
  same geometry in a new session mints a fresh handle while semantic identity is unchanged. So the
  parent D8's loose phrase "executed-result hash" can only mean an **input-cone** hash; there is no
  sound output hash to take. (This PRD resolves that terminology — see D1.)
- **The precedent storage pattern exists.** `ComputeNodeData` already carries
  `result_content_hash: Option<ContentHash>` (`graph.rs:164`, cloned at `:194`) for the staged
  ComputeNode early-cutoff pipeline. `RealizationNodeData` (`graph.rs:46-78`) carries **no** such
  field — adding one mirrors an established struct shape.
- **The cache key and partial order.** `RealizationCache` keys on `(entity_id, repr_kind,
  options_hash, tol)` with the tighter-satisfies-looser tolerance partial order
  (`realization_cache.rs`, `tolerance_bucket.rs:61-114`: a cached `tol ≤ requested` satisfies; lookup
  returns the loosest satisfying entry; `SOFT_CAPACITY=5`). Eviction must **compose** with this
  partial order, not bypass it.
- **The dispatch-count seam exists.** `last_dispatch_count` is incremented per kernel op in
  `execute_realization_ops` (`engine_build.rs:~4748`), reset at each build entry, read via
  `Engine::last_dispatch_count()` — sufficient for the §2 headline with an isolating fixture.
- **The wholesale flush is pinned by contract-lock tests.** `tests/tolerance_wiring_e2e.rs` (task
  2874): step-17 `edit_param_clears_realization_cache_…` (`:830`), step-19
  `edit_source_clears_realization_cache_…` (`:959`), step-21/22 `clear_realization_cache_public_api_…`
  (`:1069`) each assert `lookup(...).is_none()` (whole cache cleared). Activation must **consciously
  supersede** these, not break them silently.

## 4. Sketch of approach

1. **α — record the input-cone hash on the realization.** Add a nullable hash field to
   `RealizationNodeData` (mirroring `ComputeNodeData.result_content_hash`), populated at execution
   time in `execute_realization_ops` via the existing `compute_realization_upstream_values_hash`
   fold. One hash per realization (input values are independent of output repr/tol, so it does **not**
   belong on the per-`(repr,options,tol)` cache entry).
2. **β — recompute-then-compare → `changed_realizations`, and give
   `compute_dirty_cone_with_realizations` its first production caller.** At edit entry, after the
   value cone re-evaluates against the new param, recompute each realization's input-cone hash from
   the updated context and compare to the stored hash (on the persisting graph for `edit_param`; via
   the existing old-graph/new-graph diff for `edit_source`). The moved set is `changed_realizations`;
   seed it into `compute_dirty_cone_with_realizations` so downstream ComputeNodes with direct
   `realization_inputs` edges re-evaluate. (Value-cell consumers already propagate via the existing
   value-cell dirty cone, since the GeometryHandle value's `content_hash` moves with
   `upstream_values_hash`.)
3. **γ — keyed eviction replaces the wholesale flush + contract-lock supersession.** Replace the
   unconditional `clear_realization_cache()` in both edit entries with a per-realization keyed
   eviction of `changed_realizations`' entries (an input change invalidates **all** tolerance/repr
   variants of that realization, so evict the whole `(entity_id, *)` family for each changed
   realization; surviving realizations' entries stay cached). Supersede the three task-2874
   contract-lock tests with equivalent "stale entry never survives / unaffected entry hits" pins on
   the selective path.
4. **δ — staleness differential corpus.** Selective eviction must never serve a handle the wholesale
   flush would have evicted, across param edit, guard flip, collection grow, and source edit. The
   corpus runs both regimes and asserts handle-equivalence — the H boundary-test (§6).
5. **ε — e2e dispatch-count gate.** Multi-body isolating fixture; `last_dispatch_count() == ops(A)`
   after an A-only edit; `== 0` after a no-realization edit.

## 5. Resolved design decisions

- **D1 — The eviction identity is an INPUT-cone hash, sourced from the existing GHR-β fold; the
  parent's "executed-result hash" is resolved to mean this.** Output hashing is impossible
  (`kernel_handle` is ephemeral, excluded from identity by GHR-β §DD), and spec §3.6 mandates "input
  hashes, not output hashes." `compute_realization_upstream_values_hash` is reused verbatim as the
  hash source — **do not invent a second fold**; a divergent hash would silently mis-classify the
  identity that GHR-β, the value-cell early-cutoff, and the in-memory geometry cache-key
  (`cache.rs:146-161`, tag-28) all already key on.
- **D2 — Eager recompute-then-compare at edit time, not lazy self-validation at build time.** A
  build-time self-validating lookup would handle cache staleness but could **not** seed downstream
  re-eval through the existing `dirty ∩ demand` model: ComputeNodes hold **direct** `realization_inputs`
  edges (`graph.rs:159`), so their re-evaluation requires the `changed_realizations` set **before**
  the build computes its eval set. Recompute-then-compare at edit time is the architecturally-fitting
  choice (and the parent D8 framing). The recompute cost is expression eval over realization args —
  bounded, and `≪` the kernel ops it saves.
- **D3 — Store one input-cone hash per realization on `RealizationNodeData`, not on the cache
  entry.** Input values are independent of output repr/tol, so a per-`(entity,repr,options,tol)`
  cache-entry field would duplicate it across variants. The `ComputeNodeData.result_content_hash`
  field (`graph.rs:164`) is the precedent shape; clone-propagation must mirror `:194`.
- **D4 — Keyed eviction is per-realization (whole `(entity_id, *)` family), composing with — not
  bypassing — the tolerance partial order.** An input change invalidates every tolerance/repr variant
  of that realization. Surviving realizations keep the tighter-satisfies-looser lookup semantics
  unchanged.
- **D5 — Contract-lock supersession, not deletion.** The task-2874 invariant ("a subsequent build
  cannot serve a stale handle") **stays**; only its *expression* changes from "the cache is empty
  after edit" to "the changed cone is evicted, the unaffected cone survives, and no stale handle is
  served." Each superseded test names task 2874 + this PRD's γ so the lineage is traceable.
- **D6 — Develops behind the `UnifiedDag` flag; ι cutover (#4362) is not a hard prerequisite.**
  Eviction rides the warm/edit driver paths already landed by 4361/4531. Like the parent's θ, it is
  built and gated under `feature = "unified-dag"` / `REIFY_BUILD_SCHEDULER=unified`; ι (flip default +
  delete legacy) is independent.
- **D7 — `edit_source` value-path driver-homing (#4713, blocked) is not a hard prerequisite.**
  Eviction targets the shared **flush seam** (`clear_realization_cache` at both edit entries) and the
  realization-cache lookup, not the value-eval ordering. `edit_source` still iterates `eval_set`
  directly today; the eviction logic sits at the same entry as the flush it replaces and is
  order-independent. β/γ must cover **both** edit paths regardless of 4713's status.

## 6. Contract + two-way boundary tests (H component)

**Contract — input-cone-aware realization cache.**

- **Identity.** A realization's cache validity is `(entity_id, repr_kind, options_hash, tol)` AND
  `input_cone_hash == compute_realization_upstream_values_hash(realization, current_ctx)`. The hash
  is the GHR-β fold (D1); `kernel_handle` is never part of identity.
- **Eviction invariant (the load-bearing one).** After an edit, for every realization R:
  `R ∈ changed_realizations ⇔ R.input_cone_hash moved ⇔ R's cache entries are evicted`. No realization
  outside `changed_realizations` is evicted; no realization inside it survives. Equivalently: **the
  selective path serves a handle iff the wholesale-flush baseline would serve the same handle** (a
  stale handle is never served; a fresh handle is never spuriously discarded).
- **Propagation contract.** `changed_realizations` seeds `compute_dirty_cone_with_realizations`
  exactly once per edit; downstream ComputeNodes with `realization_inputs ∩ changed_realizations ≠ ∅`
  re-evaluate; others retain their cached result.
- **Partial-order composition.** Eviction removes whole `(entity_id, *)` families; surviving entries'
  tighter-satisfies-looser lookup and `SOFT_CAPACITY` behavior are byte-unchanged.

**Two-way boundary-test sketch** (faces both the producer = the eviction mechanism, and the consumer
= `build_snapshot` serving handles + downstream ComputeNodes):

| Scenario | Preconditions | Postcondition (asserted) |
|---|---|---|
| Param edit feeds body A only (producer + consumer) | multi-body model; A's realization reads the edited param, B's does not | B's cache entry survives (hit, 0 dispatch); A's evicted + re-executed; both handles == wholesale-flush baseline |
| No-realization edit | edited param feeds only a display/annotation cell | `changed_realizations == ∅`; zero eviction; `last_dispatch_count() == 0` next build |
| Guard flip (`if cond { … }`) | edit toggles a guard activating/deactivating a realization | newly-active realization executes; deactivated one not served; no stale handle from the prior branch |
| Collection grow (`forall`/count change) | edit raises a collection count, re-emitting realizations | new members execute; pre-existing members with unchanged inputs hit; structural re-elaboration leaves no orphaned stale entry (composes with 4530's rebuild invariant) |
| `edit_source` recompile | source edit changes one body's op, leaves another's identical | changed body evicted; identical body's entry survives (via old-graph/new-graph input-cone diff); covers D7 (no 4713 dependency) |
| Tolerance interplay | unaffected realization cached at `tol=1e-6`; affected one shares `entity`-adjacent buckets | partial-order lookup on survivors unchanged; only changed `(entity_id,*)` family removed |
| Differential equivalence (the gate) | run the full staleness corpus under wholesale-flush AND selective eviction | every served `GeometryHandleId` byte-equivalent across regimes |

The δ task names the differential corpus + these boundary cases; the γ task names the superseded
contract-lock tests; ε names the two dispatch-count e2es — closing G2's loop.

## 7. Pre-conditions for activating (all satisfied)

- **θ 4361 landed** (`done`, `c5a2cca397`) — warm surfaces on the unified driver
  (`BuildScheduler::UnifiedDag`, `run_unified_pass`, `build_snapshot`/`tessellate_from_values` routed
  through it, positional terminal-handle export). Eviction rides these warm paths.
- **θ2 4531 landed** (`done`, `736affc831`) — `edit_param` on the driver
  (`run_unified_pass_seeded`). The eviction modifies edit-path flush behavior; building against the
  legacy `edit_param` loop would be rework.
- **Substrate confirmed present:** `compute_realization_upstream_values_hash`
  (`engine_build.rs:~8076`), `compute_dirty_cone_with_realizations` (`dirty.rs:95`, gets its first
  caller), `RealizationCache` + tolerance partial order, `clear_realization_cache`
  (`engine_admin.rs:644`), `last_dispatch_count` (`engine_admin.rs:~1570`),
  `ComputeNodeData.result_content_hash` precedent (`graph.rs:164`).
- **No grammar change — G3 grammar-gate N/A.** This is pure engine internals; no novel `.ri` syntax.
- **Not hard-gated on:** ι 4362 (D6) or #4713 `edit_source` driver-homing (D7).

## 8. Cross-PRD relationship

| Other PRD / task | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `engine-unified-build-dag.md` (D8 / §9) | consumes | the warm/edit unified driver + the deferral this PRD discharges | parent (driver) / **this PRD** (eviction) | hard prereq landed (4361/4531) |
| `selective-demand.md` (sibling stub, task 4533) | sibling | both ride the unified driver warm paths; **demand prunes invisible work, eviction prunes unaffected work** — disjoint filters, no shared seam to contest | independent | non-blocking |
| task 4530 (dep-structure rebuild invariant) | consumes | `reverse_index`/`trace_map`/`demand` rebuild after structural re-elaboration | task 4530 | `done` — collection-grow boundary case (§6) relies on it |
| task 4713 (`edit_source` driver-homing) | soft | the `edit_source` value-eval ordering; eviction sits at the shared flush seam, order-independent | task 4713 | `blocked`; **not** a prereq (D7) — β/γ cover `edit_source` regardless |
| task 4362 (ι cutover) | soft | default-scheduler flip + legacy delete | task 4362 | develops behind the flag (D6) |

Seam ownership is unambiguous: the parent D8 explicitly defers eviction to "a follow-up after
`RealizationNodeData` result hashing exists" = **this PRD**. No reciprocal ambiguity with the sibling
(the two filters are orthogonal by construction).

## 9. Decomposition plan

DAG: **α → β → γ → {δ, ε}**. Out-of-batch hard prereqs (4361, 4531) are `done`; wire them as real
dependency edges at decompose time per `preferences_cross_prd_deps_real_edges`.

- **α — record per-realization input-cone hash on `RealizationNodeData` at execution time.** Add the
  nullable hash field (mirror `ComputeNodeData.result_content_hash`, `graph.rs:164`/`:194`); populate
  in `execute_realization_ops` via `compute_realization_upstream_values_hash`. *Modules:*
  `crates/reify-eval/src/graph.rs`, `crates/reify-eval/src/engine_build.rs`. *Signal:*
  **intermediate** — unlocks β; directly, a test asserts a built realization's stored hash equals
  `compute_realization_upstream_values_hash` for its inputs and **moves** when an input param changes
  (and is stable when an unrelated param changes). *grammar_confirmed: true.*
- **β — recompute-then-compare seeding + first production caller of
  `compute_dirty_cone_with_realizations`.** At both edit entries compute `changed_realizations` from
  stored-vs-recomputed input-cone hashes (persisting-graph compare for `edit_param`; old/new-graph
  diff for `edit_source`); seed the realization dirty cone. *Modules:*
  `crates/reify-eval/src/engine_edit.rs`, `crates/reify-eval/src/dirty.rs`. *Signal:*
  **intermediate** — unlocks γ; directly, a `@optimized` ComputeNode with a `realization_inputs` edge
  re-evaluates iff its input realization's inputs moved (observable via its output cell freshness).
  *grammar_confirmed: true.*
- **γ — keyed eviction replaces the wholesale flush + task-2874 contract-lock supersession.** Replace
  `clear_realization_cache()` at `engine_edit.rs:901`/`:2292` with per-realization keyed eviction of
  `changed_realizations`; add the keyed-evict API on `RealizationCache` (compose with the tolerance
  partial order). Supersede the three `tolerance_wiring_e2e.rs` pins (step-17/19/22) with
  "stale-never-survives / unaffected-hits" equivalents naming task 2874 + this γ. *Modules:*
  `crates/reify-eval/src/engine_edit.rs`, `crates/reify-eval/src/realization_cache.rs`,
  `crates/reify-eval/tests/tolerance_wiring_e2e.rs`. *Signal:* **leaf** — the superseding contract
  tests pass: an affected realization re-executes, an unaffected one hits, no stale handle served.
  *grammar_confirmed: true.*
- **δ — staleness differential corpus (the H boundary test).** The §6 corpus (param edit, guard
  flip, collection grow, `edit_source` recompile, tolerance interplay) under both
  wholesale-flush and selective regimes; assert every served `GeometryHandleId` byte-equivalent.
  *Modules:* `crates/reify-eval/tests/`. *Signal:* **leaf** — corpus green; selective ≡ wholesale on
  served handles. *grammar_confirmed: true.*
- **ε — e2e: slider drag re-executes kernel ops only for the affected body.** Multi-body isolating
  fixture; `edit_param` feeding body A → `last_dispatch_count() == ops(A)` (`< all-bodies`);
  no-realization edit → `== 0`. `#[cfg_attr(not(feature="unified-dag"), ignore)]`. *Modules:*
  `crates/reify-eval/tests/`. *Signal:* **leaf** (the headline integration gate). *grammar_confirmed:
  true.*

A **capability manifest** (`selective-realization-eviction.capability-manifest.md`) is committed
beside this PRD at decompose time, binding each leaf's asserted capabilities to evidence: α/β's reuse
of `compute_realization_upstream_values_hash` (wired-on-main grep), β's `compute_dirty_cone_with_realizations`
(producer = β itself, the first caller), ε's `last_dispatch_count` (wired-on-main grep, the §2
signal's backing — G6 branch-3 end-to-end check), and the **field-population** check that α writes a
non-`None` hash on the production path (not a test-only helper). **No numeric-floor** binding (the ε
assertions are op-count equalities, not tolerances) and **no grammar-fixture** binding (no novel
syntax).

## 10. Out of scope

- **Changing the `RealizationCache` tolerance partial-order semantics** (eviction composes with it).
- **Cold-build eagerness** — unchanged; eager-over-reachable cold `build()` stays (D2 of the parent).
  Eviction is a warm/edit-path concern.
- **Warm-state (`OpaqueState`) pool policy** — separate machinery (`warm_pool.rs`, distinct from
  `RealizationCache`); LRU/memory-budget eviction of opaque state is unrelated.
- **Selective demand** (sibling `selective-demand.md`) — pruning invisible work is orthogonal.
- **A per-realization dispatch counter** — the global `last_dispatch_count()` + an isolating fixture
  suffices for ε (resolved 2026-06-22); a per-realization breakdown is a possible future refinement,
  not built here.

## 11. Open questions (tactical; not blocking)

1. **Hash field type on `RealizationNodeData`.** `Option<[u8; 32]>` (matching the GHR-β
   `upstream_values_hash` width) vs `Option<ContentHash>` (matching the `ComputeNodeData` precedent).
   **Suggested:** mirror the fold's native `[u8; 32]` to avoid a lossy re-pack. Decide during α.
2. **Realization with no hydrated output value cell (demand-pruned/unconsumed).** Such a realization
   has no value-cell `upstream_values_hash` to cross-check, but α stores the hash on the node
   directly, so the compare still works. **Suggested:** rely on the node-stored hash (D3); if a
   realization was never executed (no stored hash), treat as changed (conservative re-execute).
   Decide during β.
3. **`edit_source` old/new-graph hash diff vs `edit_param` in-place compare.** Two compare sites share
   the fold but differ in where the prior hash lives (old graph vs persisting graph). **Suggested:**
   one helper taking `(prior_hash, realization, ctx)`; both call sites supply the prior. Decide during
   β.
4. **Eviction granularity if a single entity hosts many realizations.** Per-`(entity_id, *)` family
   eviction is correct but coarse if one entity's realizations have independent input cones.
   **Suggested:** start at the entity family (D4); narrow to per-`RealizationNodeId` only if the
   differential corpus shows over-eviction. Decide during γ.
