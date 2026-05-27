# Phase 3 — Memory-Based Synthesis

**Auditor:** claude-interactive (memory-path; sibling agent does the file-path synthesis)
**Date:** 2026-05-12
**Sources:** Fused-memory only (Graphiti + Mem0); no `findings/*.md` reads.

> Read this alongside `phase-2-summary.md` and (eventually) the sibling file-based
> synthesis. The whole point is to be able to compare what each substrate
> retained.

---

## 0. Critical preamble — what the memory store actually contained

Phase 2's instructions to the 40 audit agents specified writes in this canonical
form (audit-brief, lines 92-104):

```
[arch-audit-gap <slug>-NNN] mechanism="…" | prd="…" | state=<STATE> |
   failure_mode=<F#> | evidence="…" | blocks="…" | note="…"
[arch-audit-summary <slug>] mechanisms=N wired=N partial=N todo=N
   fiction=N drift=N orphan=N top_concern="…"
```

**The memory store does not contain these lines verbatim.** Both write paths
that the agents used (`add_memory` / `add_episode`) feed Graphiti's extraction
pipeline, which dissolves each input into multiple atomic edges of the form
"X is/has/relates-to Y". So instead of finding one ledger entry that says
`mechanism="X" | state=FICTION | failure_mode=F6`, the synthesizer sees three
or four separate edges:

- "The state of arch-audit-gap hex-wedge-meshing-006 is PARTIAL."
- "The failure mode of arch-audit-gap hex-wedge-meshing-006 is F4."
- "The mechanism of arch-audit-gap hex-wedge-meshing-006 involves the
  mesh_surface_to_volume_with_diagnostics tet fall-back wiring."
- "There are zero callers in reify-eval/ for arch-audit-gap hex-wedge-meshing-006."

Some gap IDs survived as identifiable strings (the ones above). Most did not —
the extractor preferred phrases like *"selector-target validation is pending
for the structural-analysis-fea"* over *"audit-gap structural-analysis-fea-005"*.
And the **40 expected `[arch-audit-summary …]` lines** are essentially gone:
broad search for `arch-audit-summary` returns the canonical "Phase 2 is part of
the PRD-corpus architecture audit" boilerplate but **zero edges containing the
literal `mechanisms=N wired=N todo=N…` summary content**. They were either
never written (low likelihood — Phase 2 supervisor confirmed all 40 emitted
one), or Graphiti dropped/diluted them past recognition.

**Net effect on the memory path:** the synthesizer must reconstruct cluster
structure from many fragmentary edges scattered across the relevance ranking,
rather than reading a clean ledger. This is feasible (this document is the
proof) but lossy in ways §4 enumerates.

A small, very useful counter-signal: **two manually-written synthesis memories
already exist in `category="observations_and_summaries"` (mem0 store)**, both
written 2026-05-12 by the Phase-2 supervisor as it batched results. They are
load-bearing for §3 below:

- *Architecture audit (2026-05-12) confirmed GR-001 transitively blocks ≥7
  PRDs … useful negative datapoint: mesh-morphing does NOT depend on GR-001 …*
- *Architecture audit (2026-05-12) surfaced "grammar-level fictions" as a
  category distinct from runtime FICTION: ≥6 PRDs assume tree-sitter-reify
  grammar productions that do not exist …*

These were the ONLY two memories I found that read like Phase-3 synthesis
content. Everything else is fragmentary edges.

---

## 1. Cluster table

Built by grouping recovered edges by **underlying mechanism**, weighted by how
many PRDs cite the mechanism (recovered from cross-PRD edges). Where I had to
infer cluster membership from fragmentary edges, the source-memory column lists
edge counts as best-effort.

| # | Cluster | Dominant mechanism | State | F# | PRDs citing (memory-recovered) | Source memories | Candidate disposition |
|---|---|---|---|---|---|---|---|
| 1 | **GR-001 transitive blast — struct-ctor runtime eval** | `Steel_AISI_1045()` etc. evaluate to `Value::Undef` because compiled struct calls have no runtime resolver | FICTION | F1 | structural-analysis-fea, multi-load-case-fea, kinematic-constraints-toplevel, varying-thickness-shells, composite-laminated-shells, structural-stability-buckling, field-source-kinds (per synthesis memory); also implied by edges in fea-gui-rendering, compute-node-infrastructure (007 "structurally identical to GR-001"), structural-analysis-fea M-? "runtime instantiation is in FICTION state" | 8+ edges + 1 synthesis memory | **Pick existing pattern**: tracked. Investigate scope of the fix (one-shot impl vs. partition by stdlib boundary per the mesh-morphing negative datapoint). |
| 2 | **ComputeNode dispatch / `@optimized` stdlib-fn lowering** | Engine has no production ComputeNode set; `@optimized` annotation is parsed (CompiledFunction.optimized_target) but P3.4 dispatch lowering is FICTION until P4 trampoline (task 3383) lands | FICTION/PARTIAL | F6 | compute-node-infrastructure, structural-analysis-fea, multi-load-case-fea, fea-gui-rendering (PRD external dep on Phase 3), persistent-fea-cache (indirect, ComputeNode is the cache-key boundary), a-posteriori-error-estimation (ZZ recovery is a candidate ComputeNode), structural-analysis-shells (solver path) | ~15 edges including "production ComputeNode set is empty until P4 trampoline", "ComputeNode infra 3426 is transitively gated on GR-001" | **PRD-shape work**: this is the scaffold-without-caller pattern in its purest form. Decide whether Phase 4 = lower one real stdlib `fn` end-to-end as proof, or unblock task 3426 as the next vertical slice. |
| 3 | **Grammar-level fictions** (grammar shapes assumed by PRDs but not in tree-sitter-reify) | `subject to`, `@shell(thickness = linear_taper(...))`, `schema = { x: Length(mm), … }`, decl-level `match`, deep-dot-chain method-call AST, `for … in …` money comprehension | FICTION (grammar) | F4 | multi-load-case-fea (re: `subject to`; see note), varying-thickness-shells, imported-field-source-hdf5-csv, specialization-scope, match-block-decls, deep-dot-chain, money-dimension (the `for…in` form) | 1 dense synthesis memory + corroborating edges: "PRD clause 'subject to' is fictional", "Reify's tree-sitter grammar does not support anonymous struct-literal expressions", "Task 2341 requires updates to tree-sitter-reify/grammar.js because the required struct-construction syntax is not present", "specialization-scope … parser hardcodes body: None" | **Category for Phase 3 discussion**: distinct from value-eval gaps. Either ship the grammar productions or rewrite the PRD prose to use the productions that exist. **2026-05-27 update:** `= auto` at the param-default position was always parseable via `auto_keyword` (grammar.js) and has been removed from this cluster's dominant mechanism list. multi-load-case-fea remains cited because its `subject to` surface is still fictional. Broader `auto` binding-site coverage (sub-overrides, named-args, let, connect-param) is being addressed by `docs/prds/auto-binding-site-positions.md` (α task 3802 landed; β–ε queued). |
| 4 | **Scaffold without a caller** (umbrella) | Producer-side infra builds + tests; no production call site invokes it. Same topology as GR-001 | varies (FICTION at integration, WIRED at unit) | F6 | compute-node-infrastructure, persistent-fea-cache, multi-kernel, persistent-naming-v2, node-trait-composition, warm-state-eviction, solver-hint-payloads, specialization-scope, match-block-decls, fea-gui-rendering, auto-type-param-resolution, topology-selectors (≥12 PRDs per phase-2-summary) | Inferred from many "no callers", "zero callable-impl hits", "library implementation has zero call-sites in any engine/CLI path", "Visual-regression harness CI wiring is partially implemented but lacks the necessary FEA scene fixtures" | **Pattern-level discussion** for Phase 3. Possibly map each instance to (a) drop the producer, (b) ship a caller, (c) reclassify as future-PRD. |
| 5 | **Persistent-naming-v2 / FeatureId / ModEntry foundations** | Task 1 of PRD declared as foundation for ModEntry, FeatureId, Role types, TopExp_Explorer walks, BRepAlgoAPI Modified hooks | PARTIAL | F2 | persistent-naming-v2, mesh-morphing (Stage B bijection check), multi-kernel, persistent-fea-cache (mod_history part of comparison key) | ~10 edges including "Architect 2570/2573/2574/2576 escalated the gap regarding missing persistent-naming-v2 task 1" | **Investigate-further**: was decomposed into 2575-2593; some children still pending; need a single owner to drive home. |
| 6 | **Persistent-fea-cache / PersistentlyCacheable** | Trait designed for export/import CLI primitive; trait serves exactly one concrete type; warm_pool LRU not wired to cache entries; cache-as-determinism-anchor-across-sessions promise unverified | PARTIAL/FICTION | F6 | persistent-fea-cache (own), structural-analysis-fea (cache is determinism anchor per FEA PRD) | ~8 edges including "warm_pool.rs file contains an LRU mechanism, but it is not used for cache entries", "trait surface currently serves exactly one concrete type", "team-distribution story including S3/git-LFS/CI artifacts is blocked" | **Accept-and-document** for v0.3, defer team-distribution stories until persistent-naming-v2 lands. |
| 7 | **Shells T-* (MITC3+, MPC tying, mixed assembly)** | MITC3+ shipped as bare MITC3 due to flat-facet under-prediction (task 3349/3392 follow-up); test bands widened to accommodate both behaviours | PARTIAL/DRIFT | F5 (DRIFT) | structural-analysis-shells | ~6 edges + 1 mem0 task-completion bundle. "empirical result for the pinched cylinder was bit-identical to bare MITC3", "Task 3392 picks Option (a) bilinear-patch + neighbour-finding for curved-element shell formulation" | **Investigate-further** for buckling/composite/varying-thickness which all depend on shells T-* + GR-001. |
| 8 | **HDF5/CSV imported field source** | No HDF5 crate, no CSV reader, FieldSource::Imported fixed 3-field struct migrating to Vec<(String,Expr)>, compile_field unconditionally errors with FieldImportedV02 | FICTION | F1 | imported-field-source-hdf5-csv | ~10 edges | **PRD-shape work**: add HDF5/CSV crate deps and wire the loader, or split v0.2 deferral into its own milestone. |
| 9 | **Scattered-sample statistics module** | MedianInterSampleSpacing absent → blocks PRD's "2× median spacing" warning policy; no SampledField scattered-points variant | FICTION | F1 | imported-field-source-hdf5-csv | ~5 edges | **PRD-shape work**: tiny module to unblock the warning policy. |
| 10 | **Multi-load-case `LoadCase` / `MultiCaseResult` / `linear_combine`** | Tests synthesize `Value::Map` directly bypassing struct-ctor evaluation; no callable-impl hits; envelope_max referenced but missing; multi-load-case error codes absent | FICTION | F1 | multi-load-case-fea | ~10 edges, gap IDs 003/004 partially survived | **Pick existing pattern** (GR-001 blocks); revisit after GR-001 resolves. |
| 11 | **Hex/wedge meshing — tet fall-back wiring** | `mesh_surface_to_volume_with_diagnostics` is `pub fn` in kernel-gmsh/mesh_volume.rs:161 but `dispatch_volume_mesh`'s `tet_path` closure is unbound in production; zero callers in reify-eval/ | PARTIAL | F4 | hex-wedge-meshing (gap 006 survived as a recognizable ID) | ~10 edges (one of the few gap IDs that did survive) | **Investigate-further**: producer is present, consumer wiring is the one-liner. |
| 12 | **Topology selectors — stdlib registration** | Architecture chose β over α (rejected .ri stdlib fn decls); TopologySelectorHelper variant added; 11 selectors registered; Engine::post_process_topology_selectors wired generically; selector-target validation still pending for structural-analysis-fea | PARTIAL/WIRED | F6 | topology-selectors, structural-analysis-fea | ~8 edges including "Topology-selectors are pending for the structural-analysis-fea", "11 topology selectors are registered in GEOMETRY_TOPOLOGY_SELECTOR_NAMES" | **Accept-and-document** the selector half; **investigate-further** the FEA boundary. |
| 13 | **Zienkiewicz-Zhu error estimator + Dörfler marking** | ZZ estimator task done; Dörfler θ=0.5; volume-weighted recovery on P1; lazy refinement at decision time | WIRED | N/A | a-posteriori-error-estimation | ~5 edges, mostly positive | **WIRED** — exception in v0.4. |
| 14 | **Mesh-morphing — elasticity morph + two-stage classifier** | Composes FEA solver primitives directly via reify-mesh-morph; PRD assertion about per-step BVP framing is unverified; FEA warm-start preservation regression test gated on M-012; Dirichlet-BC ordering must be deterministic for warm-start survival | PARTIAL | F4 | mesh-morphing | ~8 edges, including the negative-datapoint synthesis memory ("does NOT depend on GR-001") | **Accept-and-document**: known gap; the GR-001-bypass shape suggests we can land morph-only work independent of GR-001. |
| 15 | **FEA-GUI visual-regression harness** | gui/test/visual/run.ts contains only m5_geometry_flange scenario; harness CI wiring partial, lacks FEA scene fixtures; auto-clears scalars + diagnostic overlay on solver errors (PARTIAL); screenshot_window + pixelmatch prereqs done | PARTIAL | F2 | fea-gui-rendering | ~8 edges | **Accept-and-document** for now; PRD blocks on compute-node-infrastructure Phase 3 anyway. |
| 16 | **DiagnosticCode three-way drift (PRD/spec/code)** | freshness-4-variant `Failed`/`error`; deep-dot-chain `W_DEEP_DOT_CHAIN`; pragmas `#kernel` accepted-but-inert; geometry-traits `inferred_traits` field | DRIFT | F5 | freshness-4-variant, deep-dot-chain, pragmas, geometry-traits | Indirect: phase-2-summary calls this out; memory-side I found edges about DiagnosticCode rollout being "complete across eval domains" and "wire-string format for Diagnostic.code is confirmed", but DRIFT-specific edges per PRD did not surface as a single cluster | **Investigate-further**: doc-layer rot; cheap to fix once enumerated. |
| 17 | **Task-status accounting optimism** ("done while load-bearing wiring is absent") | Specific cases: fea-gui-rendering 2954, persistent-naming-v2 250/2652/2657/2658/2699, stdlib-trait-breadth audit-doc, node-trait-composition 2358, several auto-type-param-resolution tasks | various, often FICTION at integration | F7 | spans 6+ PRDs | Phase-2-summary explicitly named this; memory side has spare edges like "Task 319 is marked done since 2026-03-25, but its deliverables are not present in the codebase" — **the pattern is named in summary memory but specific instances are scattered as separate orphan-task-status edges, not cleanly indexed by audit-gap ID** | **Investigate-further**: pick a remediation policy (re-open with `reopen_reason`, or file follow-ups). |
| 18 | **Auto-type-param resolution / specialization-scope / match-block-decls** | auto type-param feature dispatches on type bounds with 10-candidate cap; specialization-scope validation tests cover nested match-arm + forbidden kinds; match-block-decls 2376 transitioned to done | PARTIAL (auto-type-param), WIRED (match-block-decls) | F4 (where grammar) | auto-type-param-resolution, specialization-scope, match-block-decls, auto-resolution-backtracking | ~10 edges; mostly WIRED-leaning but `sub name : Type { body }` parser hardcodes body=None per synthesis memory | **Accept-and-document** with grammar-level fictions (cluster 3). |
| 19 | **Solver-hint-payloads / warm-state-eviction / multi-kernel dispatch** | Inventory-based kernel registry replaces DispatchPlanner; CapabilityDescriptor + dispatcher exists (task 2641 done); long-chain dispatcher diagnostic single-task; CgWarmState in warm_state.rs | WIRED/PARTIAL | varies | multi-kernel, solver-hint-payloads, warm-state-eviction | ~8 edges, mostly positive | **WIRED-ish**: this cluster is mostly the success story of v0.2; spot-check it before relying on it. |
| 20 | **Node-trait composition + stdlib-trait-breadth audit-doc** | Task 2354 created docs/prds/stdlib-trait-breadth.md; task 2347 done; OnlyRunOnFinalInputs gating added; task 2353 stub of NodeTraits still requires task 2350 replacement | PARTIAL | F6 | node-trait-composition, stdlib-trait-breadth | ~6 edges | **Investigate-further**: who finishes task 2350? |
| 21 | **Money-dimension grammar + tests** | `pub unit USD : Money` shipped; slot 9 audit done (task 2377); `for ... in ...` money-aggregation comprehension is fictional per synthesis memory | PARTIAL | F4 (the comprehension form) | money-dimension | ~5 edges | **Accept-and-document**; the implemented core is fine. |
| 22 | **Kinematic-constraints v0.1 wiring (Joints, Mechanism builder, loop closure)** | Mechanism builder depends on closed-chain detector; both depend on Joint stdlib types; loop_closure.rs has Newton solver + NewtonConfig + NewtonOutcome | WIRED for v0.1 surface; v0.2 closed-chain + filled joint-zoo deferred | N/A for v0.1 | kinematic-constraints-toplevel | ~6 edges, mostly WIRED with explicit v0.2 deferrals | **Accept-and-document**: v0.1 looks intact in memory; verify nothing later regressed it. |
| 23 | **Per-purpose tolerance + tolerance scope** | Resolved in v0.2 2026-04-28; engine_tolerance.rs split from engine_purposes.rs; tolerance_scope + tolerance_combine preserve cross-extractor symmetry | WIRED | N/A | per-purpose-tolerance | ~4 edges | **WIRED**. |
| 24 | **Reify-doc-tool / DocModel / ItemDoc** | DocModel types shipped (task 2342); ItemHeader compile_fail doctest pinned to E0599; commit cbbd80d9 downgraded ItemDoc accessor to crate-private but build_doc_model() not yet implemented on main as of 2026-04-27; tasks 2357 (Markdown formatter) and 2359 still need build_doc_model() | PARTIAL | F2 | reify-doc-tool | ~8 edges | **Investigate-further**: who finishes build_doc_model? |
| 25 | **Field-source-kinds round-trip + CompiledFieldSource::Composed** | FieldSourceKind round-trip task 1640 done; CompiledFieldSource::Composed builds EvalContext during evaluation; Imported variant emits v0.2-deferred diagnostic | PARTIAL | F2 | field-source-kinds | ~4 edges | **Accept-and-document** (v0.2 deferral is intentional). |
| 26 | **Migration-toolchain** | Explicit "purely process" skip; preserved as deferred for v0.2; #version(0.1) pragma value-recording with no migration tool | SKIPPED (informational) | N/A | migration-toolchain | 2 edges + Phase-2-summary's named carve-out | **Not in scope** for Phase 3. |
| 27 | **Pragma framework + grammar pragma rule** | Block-valid pragmas accept #precision at module scope; Pragma framework grammar.js pragma rule wired; SOLVER_FORM_HINT extraction (task 2507) deduplicated; `#kernel` accepted-but-inert per phase-2-summary | PARTIAL/DRIFT | F5 (drift) | pragmas | ~3 edges | **Investigate-further**: enumerate which pragmas are wired vs inert. |
| 28 | **Stdlib trait breadth (Plastic, ElasticallyDeformable, mechanical material)** | Task 2347 audit done; tasks 2349, 2410 created for Plastic/ElasticallyDeformable; task 2352 done for mechanical material | PARTIAL/WIRED | N/A | stdlib-trait-breadth | 1 dense mem0 summary memory | **WIRED-ish**: most done; the named gaps have explicit tasks. |
| 29 | **Money/per-purpose-tolerance/multi-kernel — design-resolved cluster** | Three v0.2 PRDs resolved on 2026-04-28; all have WIRED implementations or near-WIRED with explicit deferrals | WIRED | N/A | money-dimension, per-purpose-tolerance, multi-kernel | Cross-PRD edge + per-PRD verification | **WIRED group**: keep as a positive baseline. |
| 30 | **Bare-MITC3 vs MITC3+ DRIFT** (sub-pattern called out in phase-2-summary) | Test envelopes widened to span both shipped bare-MITC3 and PRD-promised MITC3+ behavior; test exists but pins the wrong contract | DRIFT | F5 | structural-analysis-shells | 1 dense mem0 memory on task 3349 + corroborating edges | **PRD-shape discussion**: should the contract change to "intentionally-wide-pending-MITC3+" or should the test fail until task 3392 lands? |

Sort key was rows-cited-desc; some ties broken by mechanism scope. Total: **30
clusters** (within the 15-35 target band). Phase-2 said "~380 gap memories"
across 40 PRDs which would suggest ~30-50 distinct mechanisms; 30 clusters is
plausible if each PRD averages 1.3 unique-mechanism contributions.

---

## 2. Memory-path coverage report

### Headline numbers

| Metric | Expected (phase-2-summary) | Recovered (memory path) | Ratio |
|---|---|---|---|
| Gap memories `[arch-audit-gap …]` (verbatim) | ~380 | ~4 IDs survived as recognizable strings; ~150-200 fragmentary Graphiti edges that derive from gap-memory writes | <5% verbatim; ~40% inferable |
| Summary memories `[arch-audit-summary …]` (verbatim) | 40 | **0** matched as a summary line; the canonical `mechanisms=N wired=N…` shape returned nothing | **0%** |
| Synthesis-grade memories | (not enumerated) | 2 manually-written 2026-05-12 by supervisor (GR-001 transitive blast + grammar-level fictions) — these are the load-bearing memory artifacts | n/a |
| Per-PRD recoverable | 40 of 40 named PRDs | ~30 of 40 had recognizable mechanism-edges; ~10 only surfaced as PRD-name mentions in cross-PRD relationship edges | 75% PRD-name coverage; per-PRD detail varies enormously |

### Per-PRD coverage (PRDs phase-2-summary names + the named "top 6")

Coverage rating: **★★★** = multiple mechanism-level edges recovered, would be
usable for synthesis; **★★** = PRD named, some specifics recovered; **★** = PRD
named only, no mechanism detail; **✗** = silent.

**Top-6 (per phase-2-summary):**

| PRD | Coverage | Notes |
|---|---|---|
| structural-analysis-fea | ★★★ | 19 expected; recovered ~12 edges including FICTION/F6 multi-mechanism, ComputeNode coupling, Support stdlib, selector-target validation, M-022 unblocks-full-fidelity claim, CgWarmState reference |
| a-posteriori-error-estimation | ★★ | 17 expected; recovered ZZ-recovery, Dörfler-θ=0.5, scattered-sample-stats absence, median_spacing block. Less detail than expected for 17 gaps |
| imported-field-source-hdf5-csv | ★★★ | 17 expected; recovered ~10 edges spanning FICTION HDF5, FICTION CSV, FieldImportProvenance shape, scattered-points variant absence, FieldSource::Imported migration |
| reify-doc-tool | ★★ | 17 expected; recovered ~8 edges around build_doc_model() absence, DocModel shipped, ItemHeader doctest; less than expected |
| structural-analysis-shells | ★★★ | 16 expected; well-covered via T-task lineage edges + bare-MITC3 DRIFT synthesis memory + MPC ties + shells DOF rejects penalty coupling |
| multi-load-case-fea | ★★★ | 16 expected; well-covered including LoadCase/MultiCaseResult absence, ComputeNode dispatch, optimized(solver::elastic_static) registration, end-to-end-example reference, biggest-scope-surprise edge |

**Other named PRDs from phase-2-summary recurring-patterns list:**

| PRD | Coverage |
|---|---|
| compute-node-infrastructure | ★★★ (gap 006 = "structurally identical to GR-001"; gap 007 = Dispatch registry; P3.4 dispatch lowering FICTION; P3.1/P3.2 scope drift) |
| persistent-fea-cache | ★★★ (gap 007 = team-distribution stories; warm_pool LRU not used for cache entries; cache-as-determinism-anchor-across-sessions unverified) |
| persistent-naming-v2 | ★★★ (task 1 escalation history; ModEntry/FeatureId/Role foundation; mod_history in comparison key) |
| multi-kernel | ★★★ (CapabilityDescriptor done; inventory-based registry replaced DispatchPlanner; long-chain dispatcher diagnostic) |
| node-trait-composition | ★★ (OnlyRunOnFinalInputs gating; task 2350 must replace 2353 stub) |
| warm-state-eviction | ★ (warm_state.rs CgWarmState referenced; few specifics) |
| solver-hint-payloads | ★ (SOLVER_FORM_HINT named-constant dedup) |
| specialization-scope | ★★ (validation tests cover nested match-arm scopes + forbidden kinds; parser hardcodes `body: None` per synthesis memory) |
| match-block-decls | ★★ (task 2376 done; compile_match_arm_decl_group needs compiled_templates; decl-level match grammar fiction in synthesis memory) |
| fea-gui-rendering | ★★★ (gap PARTIAL F2; SolidJS+Three.js+Tauri 2 stack; visual-regression harness lacks FEA scene fixtures; G7/G8 external deps) |
| auto-type-param-resolution | ★★ (type-bounds dispatch; 10-candidate cap; debug-level logging) |
| topology-selectors | ★★★ (β chosen over α; TopologySelectorHelper added; 11 registered; Engine::post_process_topology_selectors generic; FEA boundary still pending) |
| varying-thickness-shells | ★ (named in synthesis memory for `@shell(thickness = linear_taper(...))` fiction; constant-thickness explicitly the v0.4 default; otherwise quiet) |
| composite-laminated-shells | ★ (named in GR-001-transitive synthesis memory only) |
| structural-stability-buckling | ★ (named in GR-001-transitive synthesis memory only) |
| field-source-kinds | ★★ (FieldSourceKind round-trip task 1640 done; CompiledFieldSource::Composed wiring; Imported v0.2-deferred diagnostic) |
| kinematic-constraints-toplevel | ★★★ (joint stdlib types; mechanism builder; closed-chain detector; v0.2 bookmark deferral; FK evaluator + Snapshot accessors) |
| mesh-morphing | ★★★ (two-stage classifier; FEA warm-start preservation; lazy-refinement-at-decision-time composition; **does NOT depend on GR-001** — explicit negative datapoint) |
| money-dimension | ★★ (USD : Money shipped; slot 9 audit done 2377; `for…in` comprehension fiction noted) |
| per-purpose-tolerance | ★★ (resolved v0.2 2026-04-28; engine_tolerance.rs split) |
| migration-toolchain | ★ (deliberate skip; preserved-as-deferred edge present) |
| freshness-4-variant | ★ (named in DRIFT pattern only; specific gap edges did not surface) |
| deep-dot-chain | ★ (W_DEEP_DOT_CHAIN named in synthesis memory) |
| pragmas | ★★ (block-valid pragma rule; SOLVER_FORM_HINT dedup; #kernel inert per phase-2-summary) |
| geometry-traits | ★★ (per-op trait propagation table done task 2312/2315; geometry_traits_inference module; inferred_traits field DRIFT named) |
| stdlib-trait-breadth | ★★★ (audit task 2347 done; gaps in Plastic/ElasticallyDeformable filed as 2349/2410; mechanical materials 2352 done) |
| auto-resolution-backtracking | ★ (PRD path named; design doc location confirmed) |
| money-dimension (already counted) | — | |

**PRDs not surfaced at all by memory-path queries:** I could not pull
recognizable mechanism-level edges for ~10 of the named v0.2/v0.3/top-level
PRDs that phase-2-summary implies have findings files. Most are the ones I list
above as ★. Whether the gap memories were never written, or were written and
extracted into edges that don't include the PRD slug, is indistinguishable from
the search-side. **This is the single biggest gap of the memory path.**

### Malformed/anomalous memories

- **Phase-1 audit-launch memory** (provenance c96eea91): split into ~5 separate
  edges ("Phase 1 is part of …", "Phase 2 is part of …", "Phase 3 is part of …",
  "Leo launched a wide PRD-corpus architecture audit …", "Suspected pattern of
  independent-architect decision accretion …"). Useful context but not gap
  data.
- **Cross-PRD slug-collision risk**: one edge says "audit-gap hex-wedge-meshing-006
  is likely owned by FEA-PRD" — the LLM extractor hallucinated an
  ownership-relation that the original gap line did not assert. Reading the
  raw fragment, the underlying ownership is *not* "FEA-PRD owns this gap" but
  "the failure mode lives at the FEA boundary." Synthesizer needs to discount
  ownership claims from extracted edges.
- **No `[arch-audit-summary …]` content recovered.** This is the cleanest
  failure mode for the memory path: the canonical summary ledger entries that
  Phase 2 supervisor confirmed were emitted are not findable by content query.
- **`category` field is null on most graphiti edges** — only the mem0-backed
  observations_and_summaries entries carry the category. So filtering by
  `categories=["decisions_and_rationale"]` (as the audit-brief specified) on
  Graphiti would not narrow the search reliably.

### Query efficiency

| Query | Useful results |
|---|---|
| `query="arch-audit-gap"` broad sweep (limit 200) | Overflowed to disk; 200 returned, only ~4 unique gap IDs survived; ~30 useful edges |
| `query="arch-audit-summary"` (limit 100) | 0 summary lines; mostly Phase-1-launch boilerplate |
| Per-PRD-slug + mechanism keyword (e.g. `"multi-load-case-fea LoadCase MultiCaseResult linear_combine"`) | **Best ROI** — surfaced 8-20 useful edges per query |
| Mechanism-name without PRD (`"ComputeNode infrastructure caller scaffold"`) | Surfaced the synthesis memories; good for cross-PRD aggregation |
| `"Architecture audit grammar-level fictions"` | Pulled the two synthesis memories — the load-bearing artifacts |

**Total queries to saturation: ~16.** First 4 returned ~70% of unique signal;
queries 5-12 added the long tail; queries 13-16 confirmed silence on remaining
PRDs. Saturation was driven by **synthesis memories**, not by the gap-memory
ledger. Without the two manually-written 2026-05-12 summary memories, the
memory path's signal-to-noise would be roughly half what it is.

---

## 3. Recurring patterns vs Phase-2's six

Phase-2-summary named six patterns. Below: which clusters from §1 fit each, and
where memory-path evidence is thin.

### 3a. Scaffold without a caller / one-sided contract
**Clusters fitting:** 2 (ComputeNode), 4 (umbrella), 5 (persistent-naming-v2),
6 (persistent-fea-cache), 15 (FEA-GUI), 20 (node-trait-composition),
24 (reify-doc build_doc_model). Memory-path evidence is **strong** — the "zero
callers", "zero callable-impl hits", "library implementation has zero call-sites"
phrasing is frequent in extracted edges.

### 3b. Grammar-level fictions
**Clusters fitting:** 3 (the dedicated cluster), 18 (auto-type-param/specialization-scope/match-block-decls grammar interactions), 21 (money `for…in`). Memory-path evidence is **strong** via the dedicated synthesis memory + corroborating fragment edges.

### 3c. Tasks marked `done` while load-bearing wiring is absent
**Clusters fitting:** 17 (the dedicated cluster). Memory-path evidence is **weak** — the *pattern* is named in phase-2-summary itself and in a memory edge ("Task 319 is marked done since 2026-03-25, but its deliverables are not present in the codebase"), but **specific per-PRD instances** (fea-gui-rendering 2954, persistent-naming-v2 250/2652/2657/2658/2699, etc.) did not surface as audit-gap edges. The memory path *names* the pattern but doesn't *enumerate* it.

### 3d. GR-001 transitive blast radius
**Clusters fitting:** 1 (the dedicated cluster), 10 (multi-load-case), the
implicit blocks on 7/14/30. Memory-path evidence is **strongest** — anchored by
the dedicated synthesis memory naming all 7 PRDs **and** the negative datapoint
(mesh-morphing does NOT depend).

### 3e. PRD/spec/code three-way drift
**Clusters fitting:** 16 (DiagnosticCode drift), 30 (bare-MITC3 vs MITC3+), 27
(pragmas #kernel inert), 12 (topology-selectors had α-vs-β rejection
documented). Memory-path evidence is **medium** — pattern named, individual
instances surface in some PRDs (geometry-traits, freshness, deep-dot-chain) but
not all.

### 3f. Bare-MITC3 vs MITC3+ DRIFT sub-pattern
**Clusters fitting:** 30. Memory-path evidence is **strong** — one of the
clearest stories in memory thanks to task 3349 reflection memory ("BOTH
enrichment approaches empirically refuted for flat-facet triangles").

### Patterns NOT found in memory but possibly in files

- **`@kernel` accepted-but-inert** is named in phase-2-summary but I could not
  pull a recovered edge that pinned exactly which pragma sites accept it inertly.
- **`inferred_traits` field DRIFT** is named in phase-2-summary; memory shows
  per-op trait propagation table is WIRED (tasks 2312/2315), but no edge
  surfaced the specific drift between code and PRD wording.
- **Per-PRD top-concern quotes** (the `top_concern="…"` field in summary
  memories) are entirely absent from memory.

---

## 4. Honest assessment of the memory path

### What the memory representation lost

1. **The structured per-gap ledger.** Phase 2 wrote ~380 single-line entries in
   a canonical key=value format. Graphiti dissolved each into 3-6 atomic edges,
   and **the join-key (gap ID) survived in only ~4 cases out of 380**. A
   synthesizer cannot reconstruct rows 1-to-1.
2. **All 40 summary memories.** Zero recovery on the canonical
   `mechanisms=N wired=N…` shape. If the supervisor ever needed to answer
   "how many gaps total?" from memory alone, they could not.
3. **Per-PRD `top_concern="…"` quotes**, which Phase 3 would want most for
   triage prioritization.
4. **Failure-mode counts per PRD.** Even when individual `failure_mode=F#`
   edges survived, aggregating them by PRD is unreliable — the slug-to-edge
   join is fuzzy.
5. **The `evidence=` file:line references** mostly survived as natural-language
   paraphrases ("kernel-gmsh/mesh_volume.rs:161 pub fn defined") but not as
   structured fields. Auditor would have to re-search the codebase to verify.

### What the memory representation gained

1. **Cross-PRD aggregation for free.** The two manually-written synthesis
   memories (GR-001 transitive list, grammar-level fictions list) are *more*
   useful than any single per-PRD file would be — they crossed PRDs in a way
   the files cannot.
2. **Negative datapoints surfaced.** "mesh-morphing does NOT depend on GR-001"
   is exactly the kind of cross-cutting observation a per-PRD file would miss
   (the mesh-morphing auditor was scoped to *its* PRD and would not normally
   document negative cross-PRD facts).
3. **Implicit-dependency edges.** Memory captured things like "ComputeNode infra
   3426 is transitively gated on GR-001" and "fea-gui-rendering related to
   compute-node-infrastructure Phase 3 as a hard precondition" as one-line
   edges that bind clusters together. Phase 3 wants exactly these.
4. **Tolerant to write-side noise.** Even with the slug join broken, the
   underlying *facts* survived in narrative form and a synthesizer can
   reconstruct.

### Could the memory path alone have produced a usable Phase-3 register?

**Yes, with caveats.** This document is the proof: 30 clusters with candidate
dispositions, six recurring patterns mapped, 40-PRD coverage at varying detail.
But:

- The synthesizer leaned heavily on **two manually-written synthesis memories**
  (only ~2KB total). Without them, the cluster table would be ~20 entries,
  missing GR-001 transitive scope and grammar-level fiction structure entirely.
- **PRD-level totals are wrong.** I cannot report "PRD X has 16 gaps, 12 FICTION,
  4 PARTIAL" because that data shape is gone. Phase 3 cost-benefit decisions
  that require gap-density numbers (e.g. "kill PRD X, it's all fiction") need
  the findings/ files.
- **Specific evidence pins are paraphrased.** Going from "the dispatch_volume_mesh's
  tet_path closure is unbound in production" to a fix requires re-finding the
  exact file:line in code. The findings/ files have it inline.
- **Pattern naming is reliable; pattern enumeration is not.** Memory tells you
  "scaffold-without-caller is endemic"; files tell you "it lives in these 12
  specific PRDs at these specific mechanisms."

### When to use which substrate

- **Use the file path for:** auditing-and-decision sessions where someone needs
  the row-level ledger, evidence pins, and per-PRD gap-density numbers. This
  is most Phase-3 work.
- **Use the memory path for:** cross-PRD pattern discovery, dependency
  inference, "what's blocked-by-what" questions, and onboarding new sessions
  cheaply. The two synthesis memories alone replace ~30 minutes of
  file-reading.
- **The hybrid is best.** Write Phase 2's per-PRD findings to files (durable,
  structured, query-friendly), **AND** write supervisor-level synthesis to
  memory as decisions are made (cross-cutting, dependency-aware). The
  two synthesis memories that did survive are the best ROI artifacts in this
  whole audit because they encode supervisor-level wisdom that the parallel
  agents could not have produced individually.

### Implication for fused-memory affordances

The audit-brief specified canonical `[arch-audit-gap …]` lines partly **on the
hope** that they would round-trip as searchable single records. They did not.
For future audits, two changes would massively improve memory-path utility:

1. **Write summary content via `add_memory` with `category="observations_and_summaries"`**
   (mem0-routed) — this category survives intact, as shown by the two synthesis
   memories that did make it through.
2. **Don't rely on canonical multi-field key-value lines for searchability** —
   Graphiti will dissolve them. Instead, write one structured fact per
   `add_memory` call with a short, semantically rich content string.

Both are cheap changes; both would have made this exercise much easier.

---

## End-state summary for §0–§4

- **Clusters:** 30
- **Recurring patterns identified:** all 6 phase-2-named, plus the negative-
  datapoint (GR-001 bypass via reify-mesh-morph) as a 7th
- **PRDs with usable mechanism-level coverage:** ~28/40 (★★ or better)
- **PRDs surfaced by name only:** ~10/40 (★)
- **PRDs silent in memory:** ~2/40 (✗)
- **Top synthesis-grade artifacts:** the 2 manually-written 2026-05-12 memories
  — these are the load-bearing pieces.
