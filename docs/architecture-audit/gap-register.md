# Gap Register

Master list of architecture gaps discovered across the Reify PRD corpus. Phase 3 maintains this; Phase 2 agents write to their own per-PRD files + fused-memory, and Phase 3 promotes findings into GR-IDs here.

## How to read

- **GR-NNN** — global gap ID. Allocated in Phase 3 during synthesis (agents don't allocate to avoid collisions).
- **State** — WIRED / PARTIAL / TODO / FICTION / DRIFT / ORPHAN (see audit-brief.md for definitions).
- **Failure mode** — F1..F7 per audit-brief catalog.
- **Cited by PRDs** — which PRDs depend on this mechanism (informational; helps scope decisions).
- **Disposition** — Phase 3 decision: PRD / accept / pick-existing / investigate / fix-now.

## Schema

| GR-ID | Mechanism | State | F# | Cited by PRDs | Blocks tasks | Disposition | Notes |
|---|---|---|---|---|---|---|---|

## Entries

### GR-001 — Structure-constructor runtime evaluation

| Field | Value |
|---|---|
| Mechanism | `StructureName(field: value, ...)` evaluates at runtime to a typed structure value carrying the struct's resolved cells |
| State | **FICTION → RESOLVED (PRD-shape work scheduled)** |
| Failure mode | F1 (compile-time contract → no runtime backing) |
| Evidence | `crates/reify-eval/src/engine_eval.rs:114-125` (explicit comment); tasks 3213/3240/3264 (readiness probes, all done); task 2039 (parser side wired); no eval-side task filed |
| Cited by PRDs | `structural-analysis-fea` (Material starter lib, decomp #1 + signature in §"Sketch of approach"), `multi-load-case-fea` (`LoadCase(...)`, `MultiCaseResult(...)` ctors), `structural-analysis-shells` (transitive via FEA), composite-laminated-shells, varying-thickness-shells, structural-stability-buckling, fea-gui-rendering, persistent-naming-v2 (M-022 parallel), field-source-kinds (M-016), kinematic-constraints-toplevel (M-022), pragmas (transitive), reify-doc-tool (M-006 sibling), persistent-fea-cache (transitive). Total: 17 of 40 PRDs per phase-3-breadcrumb-map.md Cluster A |
| Blocks tasks | 3378 (deferred), 3444 (pending), 3018 (pending), 2930 (pending), 2880-2884 (deferred), 2924 (pending, transitively), Stage-2 of 3213, plus follow-up chains in C-08 / C-16 / C-29 |
| Disposition | **PRD-shape work — Option B (typed Value variant, nominal conformance).** Resolution mode confirmed by Leo 2026-05-12. Follow-up PRD: `docs/prds/v0_3/structure-instance-runtime.md` (to be authored separately; not part of this session). Cluster C-01 (phase-3-files-synthesis §1) is disposed by this entry. |
| Discovered | 2026-05-12, supervisor session during task 3378 unblock-triage |
| Notes | The runtime ctors `point_load(...)` / `FixedSupport(...)` currently produce kind-tagged Maps directly via builtin dispatch; the structure-ctor path will produce the new typed Value variant, and existing builtins will be rewritten as stdlib `.ri` structure_defs producing the same shape. Affects: nominal trait conformance pathway (unchanged — stays nominal), `Value` enum variants (one new variant added), `value_type_kind_matches` (gains exact-type_id check), persistent cache key composition (serializes the typed instance), the ComputeNode trampoline signature (handles `Value::StructureInstance` arms during dispatch). |

#### Resolution (2026-05-12)

**Selected: Option B — typed Value variant, nominal conformance everywhere.**

Add `Value::StructureInstance { type_id: StructureTypeId, fields: PersistentMap<String, Value> }`. Struct constructors (`Steel_AISI_1045(...)`, `FixedSupport(...)`, `LoadCase(...)`, `MultiCaseResult(...)`, etc.) lower to this variant. Conformance stays strictly nominal: `structure_def Foo : TraitName { ... }` declares the bound; `entity::satisfies_trait_bound` consults declared bounds; structural-shape admission is NOT introduced. Existing Rust-side builtin-dispatch entry points (`point_load`, `FixedSupport`, `PressureLoad`, etc.) are rewritten as stdlib `.ri` `structure_def` declarations producing `Value::StructureInstance` — the language describes itself, removing the snake_case/PascalCase split and consolidating on PascalCase struct names per the existing PRD-corpus convention. `Value::Map` continues to exist as the shape for genuinely-map-shaped data (e.g. `Map<String, ElasticResult>` for multi-case results, dictionary configuration data); the two shapes are linguistically distinguishable.

**Rationale.** Reify's anti-thesis is "physical/mechanical nonsense should be hard to encode." Nominal-everywhere keeps `structure_def : TraitName` declarations as the explicit locus of author intent — the place where the author states "this is meant to be an ElasticMaterial." Structural admission (option C / hybrid-2) would silently equate physically-distinct shapes with coincident cell names (`ShellStress`/`LaminateStress` is the canonical near-miss). Hybrid-1 (typed-only structural admission for `Value::StructureInstance` values) was considered as a future relaxation knob and explicitly deferred: B → hybrid-1 is an additive extension if boilerplate proves a real friction; hybrid-1 → B is a breaking change, so the conservative direction is "tightest now." Choice aligns with the existing dimension system (Pressure ≠ Force nominal at the value layer), trait-combination machinery (Architecture §13: `WARM_STARTABLE | COMMITTABLE`), and the geometry-trait set (Bounded/Closed/Manifold/Watertight nominal). Map-convergence (option A) would have permanently lost typedness at the Value layer and was rejected on those grounds.

The follow-up PRD covers: the `Value::StructureInstance` variant addition and all Value-match-site adapters; rewriting the existing builtin-dispatch callers as stdlib structure_defs; the PascalCase naming sweep; updates to `value_type_kind_matches`, `entity::satisfies_trait_bound` (no semantic change — only new arm for typed instances), persistent cache key composition, and the ComputeNode trampoline signature; an `examples/structure-instance.ri` demonstrating runtime user-observable construction. Decomposition (and the user-observable leaves) belong in that PRD's own DAG, not in this session.

#### Follow-up PRD: `structure-instance-runtime.md`

Contract document authored 2026-05-12: `docs/prds/v0_3/structure-instance-runtime.md`. Operationalizes Option B: `Value::StructureInstance { type_id: StructureTypeId (opaque per-Engine u32), fields: PersistentMap<String, Value> }` + per-Engine `StructureRegistry` side-table carrying declared bounds / version / source-loc; `@version(N)` annotation on `structure def` for cache-key versioning; compile-time-only conformance with debug-build runtime invariant; persistent cache key = `("si", name, version, sorted-field-hash)` (name-stable across Engine restarts; per-Engine `StructureTypeId` u32 is NOT in the cache key); first vertical slice rewrites three builtins (Steel_AISI_1045 + PointLoad + FixedSupport — one per cluster sub-shape) plus declares stdlib `trait Load` and `trait Support`; `examples/structure-instance.ri` demonstrates both flat and nested compositional construction. Foundation slice (task SIR-α) is one wide-lock high-priority task per `feedback_orchestrator_narrow_locks_favor_upfront_design`; remaining ctor rewrites and gap-register companion edits decompose in §8 of the PRD. Filing happens in a separate session after this PRD is committed (per `feedback_commit_prds_before_referencing_tasks`).

### GR-002 — `@optimized fn` ComputeNode dispatch chain (cluster C-02)

| Field | Value |
|---|---|
| Mechanism | `@optimized("target")` on a stdlib `fn` lowers to a ComputeNode insertion in the evaluation graph; a dispatch registry routes `target` to a Rust trampoline; the trampoline runs, populates result, manages warm-state lifecycle, surfaces cancellation and pending semantics |
| State | **FICTION (producer half) / FICTION (consumer half)** |
| Failure mode | F6 (cross-PRD load-bearing dispatch infrastructure leaned on but absent) |
| Evidence | `crates/reify-eval/src/graph.rs:522` (`insert_compute_node` has only test callers); `engine_eval.rs:eval_user_function_call` ignores `optimized_target`; no `engine_compute.rs` or `compute.rs` exists; tasks 3379/3383/3384 pending. See `findings/compute-node-infrastructure.md` M-014/M-015/M-016 (producer) and `findings/structural-analysis-fea.md` M-001/M-002 (consumer) |
| Cited by PRDs | compute-node-infrastructure (producer-owner), structural-analysis-fea, structural-analysis-shells, multi-load-case-fea, fea-gui-rendering, fea-gui-rendering-shells, persistent-fea-cache, warm-state-eviction, a-posteriori-error-estimation, structural-stability-buckling, hex-wedge-meshing (transitive), composite-laminated-shells (transitive). Total: 15 of 40 PRDs per phase-3-breadcrumb-map.md Cluster C |
| Blocks tasks | 2924 (FEA #16 engine integration, pending), 2974 (persistent-fea-cache integration, pending), 3018 (multi-load-case end-to-end, deferred), 3005 (solve_load_cases, pending), 3378 (deferred), every transitive consumer named in the citing PRDs |
| Disposition | **PRD-shape work — contract document this session.** Resolution mode confirmed by Leo 2026-05-12: option B + approach H (vertical-slice decomposition under design-first/contracts/boundary-tests discipline) per `preferences_implementation_chain_portfolio.md`. Authored interactively as the ComputeNode contract document at `docs/prds/v0_3/compute-node-contract.md` (this session); supersedes `compute-node-infrastructure.md`'s accreted open design questions. Cluster C-02 disposed by this entry. |
| Discovered | 2026-05-12 architecture audit (Phase 2 findings on compute-node-infrastructure + 13 downstream PRDs) |
| Notes | The four contract questions Q-CN1..Q-CN4 (cancellation type, pending lifecycle, dispatch-registry scope, OpaqueState transfer rules) and the cross-cutting consumer policy Q-POL (which features route through ComputeNode vs bypass) are resolved in the contract document. Producer-side foundation tasks 3380/3381/3382/3385 are done; 3379/3383/3384 pending. Existing tasks 3379/3383/3384 are likely to be **superseded** by the contract's vertical-slice DAG rather than continued in-place; final task disposition pending Leo approval of the DAG sketch in §8 of the contract doc. |

### GR-003 — OpenVDB sub-kernel dispatcher / consumer boundary

| Field | Value |
|---|---|
| Mechanism | OpenVDB ingestion path through `reify-kernel-openvdb` + dispatcher routing in `elaborate_field`'s `CompiledFieldSource::Imported` arm |
| State | **FICTION** (consumer half — `elaborate_field` returns `Value::Undef`; `reify-eval` does not depend on `reify-kernel-openvdb`) |
| Failure mode | F1 (compile-time contract → no runtime backing) |
| Evidence | `findings/imported-field-source.md` M-007..M-011/M-013, `findings/imported-field-source-hdf5-csv.md` M-001/M-004/M-005, `findings/multi-kernel.md` M-011, `findings/structural-analysis-shells.md` M-025 (voxel realization), `findings/varying-thickness-shells.md` M-006 |
| Cited by PRDs | imported-field-source, imported-field-source-hdf5-csv, multi-kernel, structural-analysis-shells, varying-thickness-shells, field-source-kinds |
| Blocks tasks | Per cluster C-17 (`phase-3-files-synthesis.md` §1) |
| Disposition | **Ownership → multi-kernel; resolution mechanism `docs/prds/v0_3/multi-kernel-phase-3.md` §8 task θ (Phase 4 — OpenVDB consumer wired).** Reciprocal contested edge (each PRD said the OTHER owns) per `phase-3-breadcrumb-map.md` §3 / §4 Cluster D. Multi-kernel hosts the kernel inventory + dispatcher abstraction, so it owns wiring `reify-eval → reify-kernel-openvdb` and the `elaborate_field` consumer arm. Confirmed by Leo 2026-05-12; folded into the multi-kernel Phase 3 PRD's DAG (§8 task θ replaces the `Value::Undef` at `engine_eval.rs:621` with a `reify-kernel-openvdb::ingest` consumer). Cluster C-17 disposed by this entry. |
| Discovered | 2026-05-12 architecture audit (Phase 2 breadcrumbs) |
| Notes | Folded into multi-kernel Phase 3 PRD's §8 task θ — not a separate PRD. HDF5/CSV (cluster C-17 sibling) extends this contract once OpenVDB lands, per `docs/prds/v0_3/imported-field-source-hdf5-csv.md`. |

### GR-004 — Manifold `propagate_attributes` / MeshGL walk

| Field | Value |
|---|---|
| Mechanism | `KernelAttributeHook::propagate_attributes` for the Manifold kernel + the MeshGL walk that produces `AttributeHistory` variants |
| State | **FICTION** (stub returns `Discarded` with `tracing::warn!(reason="task_9_pending")`) |
| Failure mode | F1 |
| Evidence | `findings/persistent-naming-v2.md` M-018, `findings/multi-kernel.md` M-018; also touches cluster C-39 (fix-now) per `phase-3-files-synthesis.md` |
| Cited by PRDs | persistent-naming-v2, multi-kernel |
| Blocks tasks | Per cluster C-39 |
| Disposition | **Ownership → persistent-naming-v2.** Reciprocal contested edge per `phase-3-breadcrumb-map.md` §3. PNv2 owns the propagation contract (`AttributeHistory` shape, propagator semantics); multi-kernel hosts the kernel binary but does not define what attributes propagate. Confirmed by Leo 2026-05-12. Cluster C-39 fix-now task should land under this owner. |
| Discovered | 2026-05-12 architecture audit |
| Notes | Related fix-now task should be filed under PNv2 ownership (cross-check `phase-3-fixnow-filing-log.md` for C-39 disposition). |

### GR-005 — `try_eval_topology_selector` missing dispatch arms (11)

| Field | Value |
|---|---|
| Mechanism | Eval-side dispatch arms in `try_eval_topology_selector` for the 11 selector v2 vocabulary names registered in `GEOMETRY_TOPOLOGY_SELECTOR_NAMES` but absent from the eval switch |
| State | **FICTION** (Rust `pub fn` definitions exist in `selector_vocabulary_v2.rs`; none in dispatch) |
| Failure mode | F1 |
| Evidence | `findings/topology-selectors.md` M-003 + task 2699 `reopen_reason`; `findings/persistent-naming-v2.md` M-013/M-019/M-022 |
| Cited by PRDs | topology-selectors, persistent-naming-v2 |
| Blocks tasks | 2699 (reopen_reason from 2026-05-09 listing 11 missing arms), plus cluster C-10 fix-now consumers |
| Disposition | **Ownership → topology-selectors.** Reciprocal contested edge (neither PRD volunteered) per `phase-3-breadcrumb-map.md` §3. Selector vocabulary is topology-selectors' native domain; PNv2 only consumes via fallback. Assigned by Leo 2026-05-12. Cluster C-10 fix-now task (register names in dispatch table) is the unblocking action under this owner. |
| Discovered | 2026-05-12 architecture audit |
| Notes | Cluster C-10 fix-now should be cross-linked when filed (see `phase-3-fixnow-filing-log.md`). Task 2699's `reopen_reason` should be resolved as part of the same work. |

### GR-006 — `Field<X,Y>` in `param` position (cluster C-03)

| Field | Value |
|---|---|
| Mechanism | Type resolver doesn't accept `Field<X,Y>` in `param` position; every kernel result reverts to `Real` placeholder |
| State | **TODO** (task #3117 deferred) |
| Failure mode | F1 |
| Evidence | `findings/structural-analysis-fea.md` M-022; `findings/a-posteriori-error-estimation.md` M-002/M-005/M-011; `findings/structural-analysis-shells.md` M-016/M-017; `findings/multi-load-case-fea.md` M-009; `findings/composite-laminated-shells.md` M-007/M-011; `findings/varying-thickness-shells.md` M-006; `findings/fea-gui-rendering-shells.md` M-004; `findings/structural-stability-buckling.md` M-003/M-005 |
| Cited by PRDs | structural-analysis-fea, a-posteriori-error-estimation, structural-analysis-shells, multi-load-case-fea, composite-laminated-shells, varying-thickness-shells, fea-gui-rendering-shells, structural-stability-buckling |
| Blocks tasks | Per cluster C-03 (`phase-3-files-synthesis.md` §1); umbrella task 3117 |
| Disposition | **fix-now → existing task #3117 adequate** (`phase-3-fixnow-filing-log.md` "Existing task adequate"). Task title already specifies user-observable outcome (tighten `ElasticResult::displacement` and `::stress` from Real → Field<X,Y>); description names probe test + resolver fix path. |
| Discovered | 2026-05-12 architecture audit |
| Notes | Single language-feature gap blocking the entire FEA-stack field-typing claim (breadcrumb cluster B). Currently `deferred` — would benefit from being flipped to pending alongside the C-24/C-25/C-41 doc-tool chain. |

### GR-007 — Library-shipped / no-DSL-consumer (selector resolution) (cluster C-04)

| Field | Value |
|---|---|
| Mechanism | Library functions exist with tests, but no surface DSL path invokes them: `resolve_unique_by_attribute`, `resolve_unique_by_tag`, ad-hoc `@face("name")` evaluator, `narrow_arms_under_guard`, `NodePolicyOverrides` config |
| State | **PARTIAL/FICTION** (Rust public surface exists; DSL surface missing) |
| Failure mode | F2 |
| Evidence | `findings/persistent-naming-v2.md` M-013/M-014/M-019/M-022; `findings/topology-selectors.md` M-003; `findings/match-block-decls.md` M-012; `findings/node-trait-composition.md` M-010; `findings/auto-type-param-resolution.md` M-009/M-016 |
| Cited by PRDs | persistent-naming-v2, topology-selectors, match-block-decls, node-trait-composition, auto-type-param-resolution |
| Blocks tasks | Per cluster C-04; intersects task 2652 ("done" library-only) |
| Disposition | **investigate-further** — symptom of a process gap (many "done" tasks discovered to be library-only). Cross-cluster with C-07 (task-marked-done pattern). Needs Leo decision on whether to retroactively widen "definition of done = user-observable" or to file targeted per-resolver follow-ups. Phase-3 scaffold-pattern critique classifies the missing-consumer half as Type A (scaffold-without-caller) — the dominant audit shape. |
| Discovered | 2026-05-12 architecture audit |
| Notes | Several of the cluster's specific Rust pubs are addressed by sibling clusters (selector_vocabulary_v2 → GR-013/C-10; resolve_unique_by_attribute → fix-now batch via 3466 in the C-10 entry). The remaining cluster surface (ad-hoc `@face("name")` evaluator, NodePolicyOverrides) is what awaits the policy decision. |

### GR-008 — Auto-resolve / type-param resolver compile-pipeline call site (cluster C-05)

| Field | Value |
|---|---|
| Mechanism | Phase A/B/C orchestrator + DFS + backjumping all wired in `auto_type_params/`; no production caller invokes them from `compile_*`; `CompiledModule.auto_type_substitution` never written |
| State | **FICTION** (consumer half — orchestrator exists, compile pipeline doesn't invoke) |
| Failure mode | F1 |
| Evidence | `findings/auto-type-param-resolution.md` M-009/M-010/M-014; `findings/auto-resolution-backtracking.md` M-002/M-014; `findings/kleene-logic.md` M-002 (sibling — `implies` operator no parser); `findings/match-block-decls.md` M-001; `findings/specialization-scope.md` M-002; `findings/shadowing-warning.md` M-015/M-016 |
| Cited by PRDs | auto-type-param-resolution, auto-resolution-backtracking, kleene-logic, match-block-decls, specialization-scope, shadowing-warning |
| Blocks tasks | Per cluster C-05; **task 3465 filed** (`phase-3-fixnow-filing-log.md`) |
| Disposition | **fix-now → task #3465 filed** ("Auto-type-param resolver: invoke Phase A/B/C orchestrator from compile pipeline; populate CompiledModule.auto_type_substitution"). Leaf observable: fixture .ri with inferable type-param compiles AND eval yields correctly-typed value (not Real placeholder); negative-path emits `E_AUTO_TYPE_PARAM_UNRESOLVED`. |
| Discovered | 2026-05-12 architecture audit |
| Notes | Phase-3 scaffold-pattern critique flags this as exemplar Type A (scaffold-without-caller) — substantial orchestrator code shipped behind tests, no production call site. Sibling grammar gaps (kleene `implies`, match-block decls, sub bodies) live in C-06 (GR-009) and are PRD-shape work rather than fix-now. |

### GR-009 — Grammar-level fictions (PRD assumes unparseable syntax) (cluster C-06)

| Field | Value |
|---|---|
| Mechanism | Surface DSL the PRD authors invented but never landed in tree-sitter grammar: `auto:` in type_arg_list, `sub name : Type { body }`, decl-level `match`, `forall ... : <body>` for sub bodies, `subject to`, `= auto` literal, `chain` body, kind-bound `auto: Nat`, `implies` operator, `schema = { x: Length(mm) }` block, `Length(mm)` typed column, `name = "..."` user-label syntax, `sum(... for ... in ...)` comprehension, `@shell(thickness = linear_taper(...))` Expr annotation arg, `#[allow(shadowing)]` Rust-bracket form, `RegularGrid1` struct ctor |
| State | **FICTION** (PRDs reference syntax tree-sitter doesn't parse) |
| Failure mode | F1 |
| Evidence | `findings/auto-resolution-backtracking.md`; `findings/auto-type-param-resolution.md`; `findings/kleene-logic.md`; `findings/match-block-decls.md`; `findings/specialization-scope.md`; `findings/multi-load-case-fea.md` M-015; `findings/money-dimension.md` M-014; `findings/varying-thickness-shells.md` M-005; `findings/field-source-kinds.md` M-016; `findings/imported-field-source-hdf5-csv.md` M-006/M-007; `findings/persistent-naming-v2.md` M-015; `findings/shadowing-warning.md` M-015; `findings/forall-statement-form.md` M-013 |
| Cited by PRDs | auto-resolution-backtracking, auto-type-param-resolution, kleene-logic, match-block-decls, specialization-scope, multi-load-case-fea, money-dimension, varying-thickness-shells, field-source-kinds, imported-field-source-hdf5-csv, persistent-naming-v2, shadowing-warning, forall-statement-form |
| Blocks tasks | Per cluster C-06 (13 PRDs affected) |
| Disposition | **process + per-PRD remediation — addressed via grammar-fiction triage sweep 2026-05-12 + `feedback_prd_grammar_gate` policy.** Phase-3 supervisor logged remediation in `phase-3-grammar-fiction-triage-log.md`; the policy gate ("PRD authors must confirm grammar+parser+lowering before signing the PRD") is the durable preventative. |
| Discovered | 2026-05-12 architecture audit |
| Notes | This is the highest-cardinality grammar drift cluster in the audit; the policy gate is meant to stop the bleeding, but the existing 13 PRDs still each need a targeted edit (some via accept-and-document, some via filing replacement-syntax follow-ups). Specific items may also surface inside other clusters (e.g. RegularGrid struct ctor folds under GR-001/structure-instance-runtime). **Annotation-args sub-cluster resolved 2026-05-12 by `docs/prds/annotation-args.md`** — covers `@shell(thickness = linear_taper(...))` Expr annotation arg AND `@allow(shadowing)` (the `#[allow(shadowing)]` Rust-bracket form was respelled `@allow(shadowing)` in the A6 triage sweep; this is the parser/lowering/consumer chain). Annotation-args PRD's §8 DAG ships flag-form in Phase 1 (consumer-only — grammar+lowering already shipped) and named-arg + Expr lowering in Phases 2-3 (foundation for v0.5 varying-thickness-shells). Other C-06 fictions (`auto:`, `sub name : Type { body }`, decl-level `match`, etc.) remain tracked per `phase-3-grammar-fiction-triage-log.md` B1-B3 chains. |

### GR-010 — Task-marked-done pattern (cluster C-07)

| Field | Value |
|---|---|
| Mechanism | Task closure flag optimistic relative to user-observable behavior. ~15+ instances: 2954 (screenshot_window docs-only), 2967 (auto-resolve panel GUI ready / backend absent), 2959/2963 (FEA scalar_channels schema only), 250 (AdHocSelector → Undef), 2699 (eval dispatch for 11 selectors absent), 2657/2658 (Manifold MeshGL stub), 2347 (audit doc stale), 215 (CompiledModule doc field never added), 3034 (shell benchmarks pass via widened bands), 2657 (compute-node-infra follow-up), 2971 (NFS detection unbuilt), 2335 (freshness propagate walk no caller), 2671 (mechanism builder), 2645 (OpenVDB ingest never wired into elaborate_field), 2491 (auto-type-param-resolution call-site), 2349/2352/2354 (stdlib trait edges shipped but audit doc stale), 2456 (cache config no consumer) |
| State | **DRIFT** (process gap, not single-mechanism) |
| Failure mode | F5 |
| Evidence | `findings/fea-gui-rendering.md`, `findings/persistent-naming-v2.md`, `findings/stdlib-trait-breadth.md`, `findings/node-trait-composition.md`, `findings/freshness-4-variant.md`, `findings/imported-field-source.md`, `findings/topology-selectors.md`, `findings/auto-type-param-resolution.md`, `findings/reify-doc-tool.md`, `findings/kinematic-constraints-v02.md`, `findings/kinematic-constraints-toplevel.md` M-007 |
| Cited by PRDs | fea-gui-rendering, persistent-naming-v2, stdlib-trait-breadth, node-trait-composition, freshness-4-variant, imported-field-source, topology-selectors, auto-type-param-resolution, reify-doc-tool, kinematic-constraints-v02, kinematic-constraints-toplevel |
| Blocks tasks | 15+ individual tasks listed above |
| Disposition | **process gap — addressed via reopen-and-amend sweep 2026-05-12 + `feedback_task_chain_user_observable` policy.** Per-task remediation log lives in `phase-3-reopen-amend-sweep-log.md`; the policy ("task DAGs must terminate in user-observable behavior at leaves") is the durable preventative encoded in the fix-now-filing log's policy line. Phase-3 also flagged "reconciler false-positives" as a five-distinct-case sub-pattern (synthesis §5e) — task 215 / 2657 / 2699 / 2358 all done via found_on_main without runtime contract. |
| Discovered | 2026-05-12 architecture audit |
| Notes | Cross-cluster with C-04 (library-shipped / no-DSL-consumer — many "done" tasks are library-only by the same mechanism). The reopen-and-amend sweep individually re-points the affected tasks; the policy gate ensures future task chains terminate in observable leaves. |

### GR-011 — Load / Support type system (kind-tagged Maps vs trait-typed structs) (cluster C-08)

| Field | Value |
|---|---|
| Mechanism | PRD prose says `List<Load>` / `List<Support>` with nominal traits; code ships builtin name-dispatched ctors producing `Value::Map` with `kind` key. snake_case (`point_load`) vs PascalCase (`FixedSupport`) inconsistency. No `trait def Load` / `trait def Support` declared anywhere |
| State | **DRIFT** (PRD nominal traits vs code structural-map dispatch) |
| Failure mode | F3 |
| Evidence | `findings/structural-analysis-fea.md` M-011/M-012; `findings/structural-analysis-shells.md` M-015; `findings/multi-load-case-fea.md` M-001; `findings/structural-stability-buckling.md` M-012; `findings/composite-laminated-shells.md` M-002; `findings/kinematic-constraints-v02.md` M-007 (multi-DOF analog); `findings/kinematic-constraints-toplevel.md` M-022 (stdlib types) |
| Cited by PRDs | structural-analysis-fea, structural-analysis-shells, multi-load-case-fea, structural-stability-buckling, composite-laminated-shells, kinematic-constraints-v02, kinematic-constraints-toplevel |
| Blocks tasks | Per cluster C-08 (7+ transitive consumers) |
| Disposition | **PRD-shape work — folded into structure-instance-runtime PRD (per GR-001 §"Resolution").** The Option B typed-Value-variant resolution makes `Load` / `Support` nominal traits authored as `trait def Load { ... }` declarations; existing `point_load(...)` / `FixedSupport(...)` Rust-side dispatch rewrites as stdlib `.ri` `structure_def` declarations producing `Value::StructureInstance` — automatically consolidating snake_case/PascalCase on PascalCase. |
| Discovered | 2026-05-12 architecture audit |
| Notes | Pure subcase of C-01 from a structural standpoint; surfaced as own cluster because the cardinality (7 PRDs leaning on Load/Support) deserves explicit tracking. Resolved on the same PRD as C-01/GR-001. **Resolution mechanism: `docs/prds/v0_3/structure-instance-runtime.md`** (authored 2026-05-12). SIR-α foundation slice declares `trait Load` + `trait Support` + first PointLoad/FixedSupport rewrites (snake_case → PascalCase consolidation); SIR-β-load / SIR-β-sup wave-2 tasks (PressureLoad rewrite, PinnedSupport rewrite, etc.) close the cluster fully. |

### GR-012 — Diagnostic-code naming DRIFT (W_X vs PascalCase) (cluster C-09)

| Field | Value |
|---|---|
| Mechanism | PRD says `W_DEEP_DOT_CHAIN` / `W_SHADOW` / `W_TOPOLOGY_TAG_STALE` / `E_GEOMETRY_UNBOUNDED` style; code uses `DeepDotChain` / `Shadowing` / `TopologyTagStale` PascalCase. PRD says `EventKind::error`; code uses `EventKind::Failed` (overlaps cluster C-23 — see GR-025) |
| State | **DRIFT** (PRD prose vs landed PascalCase) |
| Failure mode | F7 |
| Evidence | `findings/deep-dot-chain.md` M-004; `findings/shadowing-warning.md` M-013; `findings/topology-selectors.md` M-009; `findings/geometry-traits.md` M-009 (doc only); `findings/freshness-4-variant.md` M-014; `findings/specialization-scope.md` (`E_SPECIALIZATION_FORBIDDEN_DECL` — matches); `findings/pragmas.md` (multiple) |
| Cited by PRDs | deep-dot-chain, shadowing-warning, topology-selectors, geometry-traits, freshness-4-variant, specialization-scope, pragmas |
| Blocks tasks | None directly — purely cosmetic |
| Disposition | **accept-and-document** — codebase has converged on PascalCase. Update PRD prose en masse; no code change. Folded into the C-26 doc-sweep disposition (GR-029). |
| Discovered | 2026-05-12 architecture audit |
| Notes | Sub-cluster of broader naming drift (Pattern 5 in synthesis §2). GR-025 (cluster C-23) covers the related `EventKind::error` vs `Failed` variant. |

### GR-013 — Persistent-naming selector v2 vocabulary dispatch (cluster C-10)

| Field | Value |
|---|---|
| Mechanism | Selector vocabulary v2 (`intersect`/`union`/`complement`/`except`, `faces_perpendicular_to`, `extremal_by_bbox`, `faces_by_surface_kind`, etc.) all live in `reify-eval/src/selector_vocabulary_v2.rs` but none are registered in `GEOMETRY_TOPOLOGY_SELECTOR_NAMES` or in `try_eval_topology_selector` dispatch |
| State | **FICTION** (consumer-half / dispatch-arms missing — 22+ functions) |
| Failure mode | F1 |
| Evidence | `findings/persistent-naming-v2.md` M-019; `findings/topology-selectors.md` M-003 (the 11 task-2699 names not in eval dispatch) |
| Cited by PRDs | persistent-naming-v2, topology-selectors |
| Blocks tasks | Task 2699 (reopen_reason from 2026-05-09 listing 11 missing arms); per cluster C-10 |
| Disposition | **fix-now → task #3466 filed** (`phase-3-fixnow-filing-log.md`). Cross-link to **GR-005** (selector-arm ownership assigned to topology-selectors). Leaf observable: fixture .ri calling intersect/union/complement/except/faces_perpendicular_to/extremal_by_bbox/faces_by_surface_kind compiles AND evaluates to non-Undef selection sets. |
| Discovered | 2026-05-12 architecture audit |
| Notes | This cluster is the fix-now action GR-005 anticipated; that GR established ownership (topology-selectors), this GR records the actual filing. Both should resolve together when 3466 lands. |

### GR-014 — Boolean/fillet/chamfer attribute-propagation eval-side wiring (cluster C-11)

| Field | Value |
|---|---|
| Mechanism | OCCT FFI (`boolean_fuse_with_history`, `fillet_with_history`, `chamfer_with_history`) exists + propagator `propagate_attributes_via_brepalgoapi_history` exists, but `AttributeHistory::Boolean/Fillet/Chamfer` variants don't exist + `execute_with_history` falls through to `AttributeHistory::None` |
| State | **FICTION** (variants missing, dispatch falls through) |
| Failure mode | F1 |
| Evidence | `findings/persistent-naming-v2.md` M-009/M-010/M-012; transitively `findings/mesh-morphing.md` M-005, topology-selectors, structural-analysis-fea |
| Cited by PRDs | persistent-naming-v2, mesh-morphing (transitive), topology-selectors (transitive), structural-analysis-fea (transitive via topology-selectors) |
| Blocks tasks | Per cluster C-11; **tasks 2656 + 2831 (pending)** cover this via the persistent-naming-v2 task-3 contract |
| Disposition | **fix-now → existing tasks #2656 + #2831 adequate** (`phase-3-fixnow-filing-log.md` "Existing task adequate"). Each task's `metadata.files` names the `engine_build` wire site and an e2e integration test (`topology_attribute_boolean_e2e.rs`, `topology_attribute_local_features_e2e.rs`). Completion = user-visible attribute propagation through Boolean/fillet/chamfer ops. |
| Discovered | 2026-05-12 architecture audit |
| Notes | Sibling to GR-004 (Manifold propagate_attributes) — same propagation contract, different kernel. PNv2 owns the AttributeHistory shape; OCCT FFI hosts the kernel-specific dispatcher. |

### GR-015 — Cache directory / config-file ingestion (env-only or unread) (cluster C-12)

| Field | Value |
|---|---|
| Mechanism | Config-file plumbing exists in `reify-config` but no consumer reads it: `Manifest::kernel_pins`, `CacheConfig`, `NodePolicyOverrides`, `warm_state_budget_bytes`, `auto_type_params::max_depth` (5 distinct unread fields) |
| State | **PARTIAL** (types exist; consumers don't read) |
| Failure mode | F2 |
| Evidence | `findings/multi-kernel.md` M-013; `findings/persistent-fea-cache.md` M-009; `findings/node-trait-composition.md` M-010; `findings/warm-state-eviction.md` M-002; `findings/auto-type-param-resolution.md` M-004 (partial); `findings/pragmas.md` M-016 (kernel_pragma unread); `findings/deep-dot-chain.md` M-003; `findings/reify-doc-tool.md` M-020 (declared_version unread) |
| Cited by PRDs | multi-kernel, persistent-fea-cache, node-trait-composition, warm-state-eviction, auto-type-param-resolution, pragmas, deep-dot-chain, reify-doc-tool |
| Blocks tasks | Per cluster C-12 |
| Disposition | **fix-now → task #3467 filed** (`phase-3-fixnow-filing-log.md`). Leaf observable: Manifest TOML override → engine/compiler picks up each of 5 fields (verifiable via 5 distinct assertions). |
| Discovered | 2026-05-12 architecture audit |
| Notes | Per-PRD trivial; pattern is structural (Type A scaffold-without-caller for each individual field). A single fix-now task wires all 5 consumers together. |

### GR-016 — GUI → backend event channel absent (cluster C-13)

| Field | Value |
|---|---|
| Mechanism | Frontend subscribes to events with optimistic comment "backend wired later"; Tauri-side emitter is missing: auto-resolve events (start/iteration/complete), morph stats RPC, solver progress overlay, mode-shape animation, mesh-morph debug RPC, FEA case picker, GUI shell display mode |
| State | **FICTION** (consumer half — listeners shipped; emitter half — missing) |
| Failure mode | F1 |
| Evidence | `findings/fea-gui-rendering.md` M-013/M-015; `findings/fea-gui-rendering-shells.md` M-001..M-009; `findings/multi-load-case-fea.md` M-016; `findings/mesh-morphing.md` M-014; `findings/structural-stability-buckling.md` M-013; `findings/warm-state-eviction.md` M-011; `findings/persistent-naming-v2.md` M-018 (Manifold hook UI) |
| Cited by PRDs | fea-gui-rendering, fea-gui-rendering-shells, multi-load-case-fea, mesh-morphing, structural-stability-buckling, warm-state-eviction, persistent-naming-v2 |
| Blocks tasks | Per cluster C-13 (7+ PRDs converging on absent emitter surface) |
| Disposition | **PRD-shape work — needs "GUI event channel inventory" PRD that catalogs which IPC events need backends.** Not folded under GR-001 / GR-002 — this is a GUI/backend coupling boundary that is not directly the structure-instance or ComputeNode dispatch contract. Each individual event is small once cataloged; the inventory + ownership decision is the missing artifact. |
| Discovered | 2026-05-12 architecture audit |
| Notes | Highest-cardinality scaffold-without-caller cluster on the GUI side. The Phase-3 scaffold-pattern critique flags this as the GUI mirror of cluster C-02 (ComputeNode producer). |

### GR-017 — Engine wiring: kernel module callable in isolation, no engine consumer (cluster C-14)

| Field | Value |
|---|---|
| Mechanism | `reify-mesh-morph` engine wire absent; `reify-shell-extract` not depended on by `reify-solver-elastic`/`reify-eval`; `dispatcher::dispatch` not called from `execute_realization_ops`; `propagate_freshness_only` no engine caller; `dispatch_volume_mesh` no caller; `mesh_surface_to_volume_with_diagnostics` no caller |
| State | **FICTION / PARTIAL** (kernel surfaces ship without engine integration) |
| Failure mode | F4 / F6 |
| Evidence | `findings/mesh-morphing.md` M-012/M-013/M-014/M-015/M-016/M-017/M-018; `findings/structural-analysis-shells.md` M-018; `findings/hex-wedge-meshing.md` M-017/M-018; `findings/multi-kernel.md` M-004/M-014/M-015; `findings/freshness-4-variant.md` M-013; `findings/structural-analysis-fea.md` (multiple) |
| Cited by PRDs | mesh-morphing, structural-analysis-shells, hex-wedge-meshing, multi-kernel, freshness-4-variant, structural-analysis-fea |
| Blocks tasks | 2924 (FEA #16 engine integration), 2947 (mesh-morph engine wire); per cluster C-14 |
| Disposition | **PRD-shape work — propose "engine integration phase" as a standard PRD-decomposition step.** Tasks 2924/2947 sit here; the decomposition norm is what's missing. Multi-kernel dispatcher work transitively folds into GR-002 ComputeNode contract (where applicable); kernel-specific engine bridges remain per-PRD scoped. |
| Discovered | 2026-05-12 architecture audit |
| Notes | Phase-3 synthesis §5d (the inversion-of-PRD-ordering observation) is the systemic finding — code lands ahead of engine seams. The norm "every PRD decomposition includes its engine-integration phase" is the durable preventative. |

### GR-018 — Unbounded geometry primitives (half_space / extrude_infinite) (cluster C-15)

| Field | Value |
|---|---|
| Mechanism | Diagnostic infrastructure (`E_GEOMETRY_UNBOUNDED`), inference fallback, warning machinery all wired and waiting; producers of `Bounded=false` (half_space, extrude_infinite) are absent |
| State | **FICTION** (loaded-gun-no-target — consumers waiting, producers absent) |
| Failure mode | F1 |
| Evidence | `findings/geometry-traits.md` M-006/M-009; `findings/topology-selectors.md` M-016 |
| Cited by PRDs | geometry-traits, topology-selectors |
| Blocks tasks | Per cluster C-15 |
| Disposition | **investigate-further** — small surface, but conceptually coupled to several follow-ups (stdlib-trait-breadth M-013 "Solid as trait-bound-bearing value", and any future infinite-half-space ops). Needs Leo decision: do we (a) ship half_space/extrude_infinite to vindicate the diagnostic infrastructure, or (b) retire the diagnostic infrastructure as YAGNI? |
| Discovered | 2026-05-12 architecture audit |
| Notes | Phase-3 synthesis classified this under "Clusters fitting NO Phase-2 pattern" — the "loaded gun, no target" shape is genuinely novel and underargues either direction. |

### GR-019 — Material starter library (stdlib structures unevaluable) (cluster C-16)

| Field | Value |
|---|---|
| Mechanism | `Steel_AISI_1045()`, `Aluminium_6061_T6()`, etc. parse fine but → `Value::Undef`; transitively blocks every FEA-stack consumer |
| State | **FICTION** (subcase of GR-001) |
| Failure mode | F1 |
| Evidence | `findings/structural-analysis-fea.md` M-010; transitively `findings/multi-load-case-fea.md`, `findings/structural-analysis-shells.md` M-016, `findings/structural-stability-buckling.md` M-011, `findings/composite-laminated-shells.md` M-001/M-002/M-013 |
| Cited by PRDs | structural-analysis-fea, multi-load-case-fea (transitive), structural-analysis-shells, structural-stability-buckling, composite-laminated-shells |
| Blocks tasks | Per cluster C-16 |
| Disposition | **PRD-shape work — folded into structure-instance-runtime PRD (per GR-001 §"Resolution").** Pure subcase of GR-001; surfaced as own cluster because the cardinality (5 PRDs + FEA-stack centrality) deserves explicit tracking. |
| Discovered | 2026-05-12 architecture audit |
| Notes | Once GR-001 Option B lands, every starter-library `StructureName(...)` evaluates to `Value::StructureInstance` and the FEA-stack chain unblocks. The material library itself is then a stdlib `.ri` authoring task. **Resolution mechanism: `docs/prds/v0_3/structure-instance-runtime.md`** (authored 2026-05-12). SIR-α foundation slice makes `Steel_AISI_1045()` reachable through the new ctor lowering path (the existing `structure def Steel_AISI_1045 : ElasticMaterial { ... }` at `materials_fea.ri:132` becomes evaluable); SIR-β-mat wave-2 task closes the remaining three materials (`Aluminium_6061_T6`, `Titanium_Ti6Al4V`, `ABS_Plastic` — also already declared but unreachable today). |

### GR-020 — Kernel/eval ReprKind chain coverage gaps (cluster C-18)

| Field | Value |
|---|---|
| Mechanism | Convert edges absent from capability descriptors (BRep→Mesh, BRep→Voxel, Voxel→Mesh, Mesh→BRep); cache key fields incomplete (force_tet not in compute key); per-handle ReprKind tracking absent |
| State | **FICTION / PARTIAL** (dispatcher abstractions exist; conversion edges + cache-key fields missing) |
| Failure mode | F6 |
| Evidence | `findings/multi-kernel.md` M-007/M-009/M-010/M-011/M-014/M-015; `findings/hex-wedge-meshing.md` M-024; `findings/structural-analysis-shells.md` M-025 |
| Cited by PRDs | multi-kernel, hex-wedge-meshing, structural-analysis-shells |
| Blocks tasks | Per cluster C-18 |
| Disposition | **PRD-shape work — resolved 2026-05-12 via `docs/prds/v0_3/multi-kernel-phase-3.md`.** The ReprKind / dispatcher contract is multi-kernel's native domain; the PRD ships Phase 3 as B+H decomposition (§8 nine-phase DAG). Cross-PRD coordination with `compute-node-contract.md` settled at §6: separate dispatch surfaces meeting at the cache-key boundary (`RealizationCacheKey.options_hash` ⟶ `ComputeNodeData.options_hash` transitivity). Folds in **GR-034** (long-chain diagnostic wiring at §8 task ρ) and the OpenVDB consumer half of **GR-003** (§8 task θ — `engine_eval.rs:621 CompiledFieldSource::Imported` arm). |
| Discovered | 2026-05-12 architecture audit |
| Notes | The OpenVDB sub-case (GR-003) is folded in here per the 2026-05-12 contested-ownership disposition. HDF5/CSV ingest extends after OpenVDB lands (PRD §10 out-of-scope). Engine integration (GR-017) and dispatcher wiring overlap here on the "execute_realization_ops doesn't call dispatcher::dispatch" finding — PRD §8 task ε resolves it. |

### GR-021 — Mid-surface / shell-extract → engine bridge (cluster C-19)

| Field | Value |
|---|---|
| Mechanism | Mid-surface mesh + segmentation + per-vertex thickness all produced in `reify-shell-extract`; never transported through IPC to GUI; never bridged to `reify-solver-elastic`'s persistent-cache ElasticResult |
| State | **FICTION** (kernel side ships; integration absent) |
| Failure mode | F1 |
| Evidence | `findings/structural-analysis-shells.md` M-018/M-019/M-020/M-022/M-023; `findings/fea-gui-rendering-shells.md` M-002/M-004/M-014; `findings/varying-thickness-shells.md` M-001; `findings/composite-laminated-shells.md` M-005/M-006 |
| Cited by PRDs | structural-analysis-shells, fea-gui-rendering-shells, varying-thickness-shells, composite-laminated-shells |
| Blocks tasks | Per cluster C-19 |
| Disposition | **PRD-shape work — shell-extract engine integration PRD.** Specific case of the engine-integration norm (GR-017); shells is large enough to warrant its own PRD slot. Also intersects GR-016 (GUI event channel) on the IPC half. |
| Discovered | 2026-05-12 architecture audit |
| Notes | Mid-surface naming (Role::MidSurfaceEdge + FeatureId::derived_mid_surface) is wired in `reify-types`; the missing piece is plumbing through the kernel→engine→solver/GUI seams. |

### GR-022 — MITC3 vs MITC3+ DRIFT (shell accuracy benchmarks widened) (cluster C-20)

| Field | Value |
|---|---|
| Mechanism | Shipped element is bare MITC3 on flat facets; benchmarks (task 3034) widened bands to 21–2200× to pass; PRD originally claimed MITC3+ specifically to avoid the regime this hit |
| State | **DRIFT → RESOLVED 2026-05-12** |
| Failure mode | F1 |
| Evidence | `findings/structural-analysis-shells.md` M-005, M-021; transitively composite-laminated-shells, structural-stability-buckling, varying-thickness-shells |
| Cited by PRDs | structural-analysis-shells, composite-laminated-shells (transitive), structural-stability-buckling (transitive), varying-thickness-shells (transitive) |
| Blocks tasks | Per cluster C-20; task **3392** (MITC3+ curved-element work) remains queued (low priority pending) for future activation |
| Disposition | **resolved 2026-05-12 — PRD edited to document bare-MITC3 as the v0.4 contract; task 3392 (MITC3+ curved-element) remains queued for future.** The widened bands in task 3034 benchmarks now correctly encode the v0.4 contract rather than pinning a fictional MITC3+ claim. Active drift pin (new Pattern A in synthesis §3) is removed. |
| Discovered | 2026-05-12 architecture audit |
| Notes | Resolution chosen: retire the MITC3+ claim from v0.4, document MITC3 as the shipped contract, defer MITC3+ to a future activation of task 3392. The downstream shells/composite/buckling/varying-thickness PRDs inherit the v0.4 contract rather than the prior MITC3+ assumption. |

### GR-023 — Vertex-correspondence in PNv2 always-empty (cluster C-21)

| Field | Value |
|---|---|
| Mechanism | `CorrespondenceMap.vertex_to_vertex` is structurally empty; any P1 mesh with surface nodes on B-rep vertices fails morph (`ProjectionFailure::MissingCorrespondence`) |
| State | **FICTION** (data structure shipped with known-empty hole) |
| Failure mode | F1 |
| Evidence | `findings/mesh-morphing.md` M-002; transitively `findings/persistent-naming-v2.md` |
| Cited by PRDs | mesh-morphing, persistent-naming-v2 (transitive) |
| Blocks tasks | Per cluster C-21 |
| Disposition | **investigate-further** — Leo decides between (a) "restrict morph eligibility" (document vertex-coincidence as a morph precondition) and (b) "fill the bijection" (compute vertex_to_vertex during attribute propagation). Both are bounded; both are real options. |
| Discovered | 2026-05-12 architecture audit |
| Notes | Phase-3 synthesis classified this under "Clusters fitting NO Phase-2 pattern" — the "known-empty hole in shipped data structure" shape. Reciprocal mesh-morph ↔ PNv2 audit cite (synthesis §3) — both audits surface it without contradiction. |

### GR-024 — Eigenvalue solver + geometric stiffness K_g (cluster C-22)

| Field | Value |
|---|---|
| Mechanism | Lanczos/Arnoldi/shift-invert via faer-rs named as the path but no `eigensolve` / `K_g` module exists; eigenvalue solver is the largest net-new kernel surface for buckling |
| State | **FICTION** (named but unbuilt) |
| Failure mode | F6 |
| Evidence | `findings/structural-stability-buckling.md` M-006, M-007 |
| Cited by PRDs | structural-stability-buckling, (modal-analysis future PRD — non-PRD breadcrumb) |
| Blocks tasks | Per cluster C-22 |
| Disposition | **PRD-shape work — buckling-specific decomposition once foundations land.** Blocked on FEA stack (#2924 in particular) reaching engine integration; until then the eigensolver has no caller. Naturally sequences after GR-001 + GR-002 land. |
| Discovered | 2026-05-12 architecture audit |
| Notes | Modal analysis (unfiled future PRD) would share this infrastructure; worth noting in any eventual eigensolver decomposition that it should be generalized rather than buckling-specific. |

### GR-025 — Failed/error event naming (PRD vs code drift) (cluster C-23)

| Field | Value |
|---|---|
| Mechanism | PRD/spec say `EventKind::error`; code says `EventKind::Failed` |
| State | **DRIFT** (PRD prose vs landed variant name) |
| Failure mode | F7 |
| Evidence | `findings/freshness-4-variant.md` M-014; transitively `findings/node-trait-composition.md` |
| Cited by PRDs | freshness-4-variant, node-trait-composition (transitive) |
| Blocks tasks | None directly |
| Disposition | **accept-and-document** — rename PRD or code, choose one. Code-side `EventKind::Failed` is the shipped variant; PRD prose update is the lower-cost change. Folded into the C-26 doc-sweep (GR-029). |
| Discovered | 2026-05-12 architecture audit |
| Notes | Sub-cluster of broader naming drift (Pattern 5). Sibling to GR-012 (cluster C-09). |

### GR-026 — Doc-comment propagation through compilation (cluster C-24)

| Field | Value |
|---|---|
| Mechanism | PRD's preamble asserts `TopologyTemplate / CompiledFunction / TraitDef / EnumDef` "all carry `doc: Option<String>`" — none of these compiled types has the field; AST has doc; LSP reads from AST; compiler drops doc at lowering |
| State | **FICTION** (compiled types missing `doc` field) |
| Failure mode | F1 |
| Evidence | `findings/reify-doc-tool.md` M-006; `findings/pragmas.md` M-012 (cross-ref) |
| Cited by PRDs | reify-doc-tool, pragmas |
| Blocks tasks | Per cluster C-24; **task #3462 filed** |
| Disposition | **fix-now → task #3462 filed** ("Doc-tool: thread doc strings through compiler types (TopologyTemplate/CompiledFunction/TraitDef/EnumDef)"). Leaf observable: compiled module's types carry `doc: Some(…)` populated from AST (unit test in reify-compiler). Sequences before 3463 (GR-027 / C-25) and 3464 (GR-042 / C-41). |
| Discovered | 2026-05-12 architecture audit |
| Notes | Phase-3 synthesis §5b includes this as a member of the "runtime/compile-time boundary is recurrently mis-modeled" family — a GR-001-shape gap that landed independently in the doc-tool PRD. |

### GR-027 — DSL `build_doc_model` + `reify doc` CLI wiring (cluster C-25)

| Field | Value |
|---|---|
| Mechanism | `build_doc_model(&CompiledModule, &str) -> DocModel` absent; HTML formatter exists but CLI uses stub; CLI passes empty DocModel; `reify doc` produces empty output |
| State | **FICTION / PARTIAL** (formatter shipped; doc-model builder absent; CLI uses stub) |
| Failure mode | F1 / F4 |
| Evidence | `findings/reify-doc-tool.md` M-005, M-008, M-022 |
| Cited by PRDs | reify-doc-tool |
| Blocks tasks | Per cluster C-25; **task #3463 filed** |
| Disposition | **fix-now → task #3463 filed** ("reify doc: wire build_doc_model + replace render_html_stub so `reify doc` emits real HTML"). Leaf observable: `reify doc <file.ri>` produces HTML containing doc-comment text and symbol names. Depends on #3462 (GR-026) for doc-field propagation. |
| Discovered | 2026-05-12 architecture audit |
| Notes | Second link in the doc-tool fix-now chain (3462 → 3463 → 3464). |

### GR-028 — Stdlib audit / config docs / examples missing or stale (cluster C-26)

| Field | Value |
|---|---|
| Mechanism | `docs/notes/stdlib-trait-audit.md` pre-dates inheritance fixes; PRD-named example file `integration_full_v01.ri` missing `#precision(0.001m)`; example `multi_load_bracket.ri` doesn't exist; `examples/m11_annotations.ri` doesn't exercise solver_hint collections; PRD-named smoke test on `examples/m5_purpose.ri` doesn't exist; PRD-promised `stdlib-trait-breadth-audit-v01.md` deliverable absent; `docs/auto-type-param-resolution.md` completeness unverified |
| State | **DRIFT / TODO** (PRD-named artifacts stale or absent) |
| Failure mode | F5 |
| Evidence | `findings/stdlib-trait-breadth.md` M-002; `findings/pragmas.md` M-018; `findings/multi-load-case-fea.md` M-015; `findings/solver-hint-payloads.md` M-012; `findings/freshness-4-variant.md` M-018; `findings/auto-type-param-resolution.md` M-014; `findings/structural-analysis-fea.md` M-027; `findings/structural-analysis-shells.md` M-023; `findings/hex-wedge-meshing.md` M-023 (validation suite) |
| Cited by PRDs | stdlib-trait-breadth, pragmas, multi-load-case-fea, solver-hint-payloads, freshness-4-variant, auto-type-param-resolution, structural-analysis-fea, structural-analysis-shells, hex-wedge-meshing |
| Blocks tasks | Per cluster C-26 (9+ PRDs with stale artifacts) |
| Disposition | **accept-and-document** — single sweep to update or file follow-ups. Subsumes naming-drift sub-clusters GR-012 (C-09) and GR-025 (C-23). Phase-3 synthesis's new-pattern-C ("PRD names a deliverable filename that's never produced") is exactly this cluster's shape. |
| Discovered | 2026-05-12 architecture audit |
| Notes | Process-shape gap; could couple to "PRD acceptance requires a delivered-artifact checklist" policy alongside `feedback_prd_grammar_gate`. |

### GR-029 — Dimensional type aliases at stdlib level (cluster C-27)

| Field | Value |
|---|---|
| Mechanism | `DimensionVector::PRESSURE` exists but no `type Pressure = Scalar<PRESSURE>` alias at `.ri`-source level. Every stdlib trait downgrades typed params to `Real` with comment |
| State | **TODO** (task #3115 deferred) |
| Failure mode | F1 |
| Evidence | `findings/stdlib-trait-breadth.md` M-012; transitively `findings/structural-analysis-fea.md` (multiple), `findings/structural-analysis-shells.md` M-017, `findings/money-dimension.md` M-014 |
| Cited by PRDs | stdlib-trait-breadth, structural-analysis-fea, structural-analysis-shells, money-dimension |
| Blocks tasks | Per cluster C-27; umbrella task #3115 |
| Disposition | **fix-now → existing task #3115 adequate** (`phase-3-fixnow-filing-log.md` "Existing task adequate"). Acceptance includes "the 15 blocked-composite sites tightened" — that is the user-observable downstream change, modulo the v0.5 fractional-exponent caveat already documented. |
| Discovered | 2026-05-12 architecture audit |
| Notes | Currently `deferred`; same status note as #3117 (GR-006). Consider flipping both to pending alongside the C-24/C-25/C-41 doc-tool chain. |

### GR-030 — `Solid` as a usable stdlib `.ri` param type (cluster C-28)

| Field | Value |
|---|---|
| Mechanism | `Solid` resolves to `Type::Geometry` as builtin alias but stdlib traits cannot use it as a slot type; PRD-spec `Physical` shape (`geometry: Solid`) blocked |
| State | **ORPHAN** (Rust-side alias works; stdlib-trait-slot use blocked) |
| Failure mode | F1 |
| Evidence | `findings/stdlib-trait-breadth.md` M-013; transitively `findings/geometry-traits.md`, FEA Material chain |
| Cited by PRDs | stdlib-trait-breadth, geometry-traits (transitive), structural-analysis-fea (transitive Material chain) |
| Blocks tasks | Per cluster C-28 |
| Disposition | **investigate-further** — language-design question. May want a small PRD: "should builtin-alias types (`Solid`, `Real`, future `Length`) be usable in stdlib-trait slot positions, and if so, what's the cell-name + conformance semantics?" Touches GR-018 (unbounded primitives — same trait-bound concern). |
| Discovered | 2026-05-12 architecture audit |
| Notes | This is one of the rare ORPHAN-state cases (mechanism exists but no PRD calls for it as currently shipped) — Rust-side alias is unused for trait slots. |

### GR-031 — Composed / derived stress recovery for varying shapes (cluster C-29)

| Field | Value |
|---|---|
| Mechanism | `to_global(stress, frame)` helper named in stdlib but absent; `linear_combine` outputs synthesized with Undef frame; envelope helpers Real-placeholder typed |
| State | **FICTION / PARTIAL** (helpers named; absent or downgraded) |
| Failure mode | F1 |
| Evidence | `findings/structural-analysis-shells.md` M-024; `findings/multi-load-case-fea.md` M-010; `findings/composite-laminated-shells.md` M-007/M-012; `findings/fea-gui-rendering-shells.md` M-007 |
| Cited by PRDs | structural-analysis-shells, multi-load-case-fea, composite-laminated-shells, fea-gui-rendering-shells |
| Blocks tasks | Per cluster C-29; **task #3468 filed** |
| Disposition | **fix-now → task #3468 filed**, BUT functional completion depends on **GR-001** (structure-instance-runtime PRD) because the typed-envelope helpers need `Value::StructureInstance` for ShellStress/LaminateStress frames. The helpers themselves are mechanical; the type-system surface they consume is GR-001 work. |
| Discovered | 2026-05-12 architecture audit |
| Notes | Captured in synthesis §1 as "depends on C-01" — the leaf is small and fix-now, the dependency chain is structural. **Functional unblock mechanism: `docs/prds/v0_3/structure-instance-runtime.md`** (authored 2026-05-12). Task 3468 (already filed) executes against this PRD's SIR-α foundation slice — once Value::StructureInstance is live, the typed-envelope helpers consume it for ShellStress/LaminateStress frames per the existing task scope. |

### GR-032 — Local-disk NFS detection + cache GC + cache CLI surface (cluster C-30)

| Field | Value |
|---|---|
| Mechanism | `reify cache stats|clear|gc|export|import` not in `reify-cli/src/main.rs`; NFS detection (PRD §"Local-disk only") not implemented; cost-aware LRU GC not implemented |
| State | **TODO** (PRD-decomposed but blocked on upstream FEA wiring) |
| Failure mode | F4 |
| Evidence | `findings/persistent-fea-cache.md` M-010, M-013, M-014, M-015, M-016, M-017, M-018, M-019 |
| Cited by PRDs | persistent-fea-cache |
| Blocks tasks | Per cluster C-30 (~8 tasks already decomposed under persistent-fea-cache PRD) |
| Disposition | **PRD-shape work — blocked on GR-002 contract DAG.** Persistent-cache work cannot land until ComputeNode dispatch ships (its consumer surface). Once GR-002's vertical-slice DAG resolves the contract, persistent-fea-cache's pre-existing decomposition activates. |
| Discovered | 2026-05-12 architecture audit |
| Notes | Already PRD-decomposed (8 tasks); the cluster is "decomposition is fine, the gate is upstream". |

### GR-033 — Kinematic `interferes` / `min_clearance`: FK-transform application (cluster C-31)

| Field | Value |
|---|---|
| Mechanism | OCCT distance probe deliberately does NOT apply per-body `world_transform`; sweep-driven interference NOT supported; `interferes_with` / `min_clearance` share this gap |
| State | **DRIFT** (per-pair API exists; FK-transform application omitted) |
| Failure mode | F2 |
| Evidence | `findings/kinematic-constraints-toplevel.md` M-019/M-020/M-021 |
| Cited by PRDs | kinematic-constraints-toplevel |
| Blocks tasks | Per cluster C-31; **task #3469 filed** |
| Disposition | **fix-now → task #3469 filed** ("Kinematic interferes/min_clearance: apply per-body world_transform before OCCT distance probe"). Leaf observable: fixture 2-body chain that overlaps only when FK-positioned returns `interferes_with=true` and `min_clearance<0`. |
| Discovered | 2026-05-12 architecture audit |
| Notes | Well-scoped single fix; the missing piece is just applying the transform before passing geometries to OCCT distance APIs. |

### GR-034 — Long-chain diagnostic / per-stage tolerance budget unreachable (cluster C-32)

| Field | Value |
|---|---|
| Mechanism | `is_long_chain_realization` + `long_chain_diagnostic` exist but no in-tree caller; `per_stage_tolerance_for_plan` degenerates because `BUDGET_QUERY_TRIPLE_V02 = BooleanUnion (BRep, &[BRep])` fixes `n_stages = 0` |
| State | **PARTIAL** (builders exist; gated on dispatcher) |
| Failure mode | F2 |
| Evidence | `findings/per-purpose-tolerance.md` M-008, M-011; `findings/multi-kernel.md` M-017 |
| Cited by PRDs | per-purpose-tolerance, multi-kernel |
| Blocks tasks | Per cluster C-32 |
| Disposition | **folded into GR-020 — resolution mechanism `docs/prds/v0_3/multi-kernel-phase-3.md` §8 task ρ (Phase 8 — Long-chain diagnostic wiring).** Once the multi-kernel Phase 3 dispatcher fan-out lands (§8 tasks ε, ι), `is_long_chain_realization` + `long_chain_diagnostic` get called from `execute_realization_ops` with wall-time bracketing. `per_stage_tolerance_for_plan` becomes meaningful because real multi-stage chains exist. Not separately fix-now-able; rides with GR-020. |
| Discovered | 2026-05-12 architecture audit |
| Notes | Cascading downstream of GR-020; resolution ships in the same PRD (`multi-kernel-phase-3.md`) at §8 task ρ. |

### GR-035 — Cancellation handle placeholder type (cluster C-33)

| Field | Value |
|---|---|
| Mechanism | `CancellationHandle` is `struct CancellationHandle;` (unit type); real type deferred to P3.5; FEA cancellation regression test unimplementable |
| State | **TODO** (placeholder unit type; design-deferred) |
| Failure mode | F3 |
| Evidence | `findings/compute-node-infrastructure.md` M-003, M-007; `findings/structural-analysis-fea.md` M-007; transitively `findings/structural-stability-buckling.md` M-002 |
| Cited by PRDs | compute-node-infrastructure, structural-analysis-fea, structural-stability-buckling (transitive) |
| Blocks tasks | Per cluster C-33; **task #3384 pending** ("pick one of three options") |
| Disposition | **PRD-shape work — blocked on GR-002 contract DAG.** Task 3384's "Arc<AtomicBool> vs tokio_util CancellationToken vs custom" choice is one of the four Q-CN questions resolved in the ComputeNode contract document (Q-CN1). Per `phase-3-fixnow-filing-log.md` "Design-deferred", this isn't a fix-now under the policy because the leaf is binary-design-ambiguous. |
| Discovered | 2026-05-12 architecture audit |
| Notes | Task 3384 is "likely to be superseded" by the contract's vertical-slice DAG per GR-002 Notes; final disposition pending Leo approval of that DAG. |

### GR-036 — Imported-tolerance promise: extractor reads non-existent `tolerance` cell (cluster C-34)

| Field | Value |
|---|---|
| Mechanism | `extract_input_tolerance_promise` reads `ValueCellId(input_template_name, "tolerance")`; stdlib `Input` trait has no `tolerance` cell — uses `Provenance.tolerance_guarantee` indirection |
| State | **DRIFT** (extractor reads the wrong field) |
| Failure mode | F1 |
| Evidence | `findings/per-purpose-tolerance.md` M-009, M-013 |
| Cited by PRDs | per-purpose-tolerance |
| Blocks tasks | Per cluster C-34; **task #3470 filed** |
| Disposition | **fix-now → task #3470 filed** ("extract_input_tolerance_promise: read Provenance.tolerance_guarantee, not non-existent `tolerance` cell on Input"). Leaf observable: fixture with Provenance.tolerance_guarantee=0.01mm yields per-stage budget reflecting that promise (not the default). |
| Discovered | 2026-05-12 architecture audit |
| Notes | Small contained fix; the read-site is one function. |

### GR-037 — Tolerance MVP scope (bare-param-subject only) (cluster C-35)

| Field | Value |
|---|---|
| Mechanism | `RepresentationWithin(subject, tol)` recognizes only bare-param subjects; member-access subjects (`subject.head`) silently dropped |
| State | **PARTIAL** (recognizer scope clipped at MVP) |
| Failure mode | F1 |
| Evidence | `findings/per-purpose-tolerance.md` M-001; transitively `findings/structural-analysis-fea.md` (FEA bracket.fea_subject usage candidate) |
| Cited by PRDs | per-purpose-tolerance, structural-analysis-fea (transitive) |
| Blocks tasks | Per cluster C-35 |
| Disposition | **investigate-further** — design-open whether to widen the recognizer to accept member-access subjects or to document the MVP scope as the v0.1 contract. FEA's likely consumer pattern (`bracket.fea_subject`) tilts toward widening. |
| Discovered | 2026-05-12 architecture audit |
| Notes | Phase-3 synthesis classified this under "MVP scope clip silently drops downstream cases" — needs a Leo decision before any code change. |

### GR-038 — NodeTraits + scheduler dispatch never bridge (cluster C-36)

| Field | Value |
|---|---|
| Mechanism | Parallel taxonomies: `NodeTraits` bitflags + `NodeArchKind` (7 variants) in reify-types AND `NodePolicyOverrides` + `NodeKind` (5 variants) in reify-runtime. Scheduler reads NodePolicyOverrides, ignores NodeTraits |
| State | **FICTION** (NodeTraits has no scheduler consumer) |
| Failure mode | F1 |
| Evidence | `findings/node-trait-composition.md` M-002, M-003, M-005, M-008, M-009, M-011 |
| Cited by PRDs | node-trait-composition |
| Blocks tasks | Per cluster C-36 |
| Disposition | **investigate-further** — Leo decides between "merge taxonomies" (NodeTraits bitflags become scheduler input + retire NodeKind enum, or vice versa) and "rename/retire one" (acknowledge that the two taxonomies are answering different questions and disambiguate). Phase-3 synthesis flagged this as a new pattern: "two parallel taxonomies that don't compose." |
| Discovered | 2026-05-12 architecture audit |
| Notes | Synthesis §3 new-pattern-B example. The other instance (Load/Support nominal vs kind-tagged) is GR-011 / cluster C-08; that one resolves under GR-001. This one is orthogonal — needs its own decision. |

### GR-039 — Kinematic singularity surfacing (cluster C-37)

| Field | Value |
|---|---|
| Mechanism | `solve_loop_closure_with_diagnostics` wired but bypassed by `snapshot()` / `sweep()`; typed diagnostic variants reserved but never reach `EvalResult`; `is_singular` flag never on Snapshot Map |
| State | **PARTIAL / FICTION** (diagnostic infrastructure shipped; user surface bypassed) |
| Failure mode | F4 |
| Evidence | `findings/kinematic-constraints-v02.md` M-009, M-010, M-011; `findings/kinematic-constraints-toplevel.md` M-007 (closed-chain) |
| Cited by PRDs | kinematic-constraints-v02, kinematic-constraints-toplevel |
| Blocks tasks | Per cluster C-37; **task #3471 filed** |
| Disposition | **fix-now → task #3471 filed** ("Kinematic singularity: route snapshot()/sweep() through solve_loop_closure_with_diagnostics; surface is_singular + typed diagnostic"). Leaf observable: near-singular kinematic snapshot returns `Snapshot.is_singular=true` AND `EvalResult` diagnostic stream contains typed `KinematicSingular` entry. |
| Discovered | 2026-05-12 architecture audit |
| Notes | Phase-3 synthesis §5e flagged kinematic-toplevel M-007 as one of the "task done; runtime contract absent" sites — the v0.1 closed-chain contract was subsumed by v0.2 without retiring the v0.1 promise. |

### GR-040 — Method-call AST shape absent (cluster C-38)

| Field | Value |
|---|---|
| Mechanism | Reify `ExprKind::FunctionCall { name: String, args: Vec<Expr> }` takes bare name, not Expr callee. PRD acceptance assumed but vacuously satisfied; `a.b.foo()` can't be expressed |
| State | **FICTION** (PRD assumes language feature that doesn't exist) |
| Failure mode | F1 |
| Evidence | `findings/deep-dot-chain.md` M-008 |
| Cited by PRDs | deep-dot-chain |
| Blocks tasks | Per cluster C-38 |
| Disposition | **accept-and-document** — flag as language-design open. The lint passes vacuously (zero false-negatives but also zero coverage). If method-call syntax ever lands, the lint needs to be revisited with real `a.foo()` cases. Document as such; no fix-now. |
| Discovered | 2026-05-12 architecture audit |
| Notes | Sibling to GR-009 (cluster C-06 grammar fictions); but unlike C-06's many small invented syntaxes, this is one feature that several future PRDs would benefit from. Defer to a language-design decision. |

### GR-041 — Composite / buckling / varying-thickness greenfield (cluster C-40)

| Field | Value |
|---|---|
| Mechanism | Every named entity (`OrthotropicMaterial`, `Laminate`, `Ply`, `tsai_wu`/`hashin`/`max_strain`, `K_g`, eigensolver, `linear_taper`, …) is FICTION; PRDs are stubs deferred to v0.5+ |
| State | **FICTION** (v0.5+ greenfield) |
| Failure mode | F1 |
| Evidence | `findings/composite-laminated-shells.md` (all); `findings/varying-thickness-shells.md` (all); `findings/structural-stability-buckling.md` (all) |
| Cited by PRDs | composite-laminated-shells, varying-thickness-shells, structural-stability-buckling |
| Blocks tasks | None active — all deferred |
| Disposition | **accept-and-document — properly deferred v0.5+ work; not active gaps.** These PRDs are honest stubs; their FICTION mechanisms exist because the PRDs are explicitly future-work outlines, not commitments. No remediation needed. |
| Discovered | 2026-05-12 architecture audit |
| Notes | Phase-3 synthesis §5g flagged the "additive on top of fictional foundation" framing as worth Leo's awareness: composite-laminated-shells says "through-thickness becomes a sum over plies" but the current through-thickness is analytical closed-form for constant thickness, not a sum-over-anything. If these PRDs activate before v0.5, this disposition needs revisiting. |

### GR-042 — Stdlib doc generator stdlib-page surface absent (cluster C-41)

| Field | Value |
|---|---|
| Mechanism | `cmd_doc` CLI rejects stdlib-walking; no `reify doc` surface produces stdlib trait/structure pages |
| State | **FICTION** (CLI rejects the stdlib path) |
| Failure mode | F1 |
| Evidence | `findings/solver-hint-payloads.md` M-011; `findings/reify-doc-tool.md` M-022, M-023 |
| Cited by PRDs | solver-hint-payloads, reify-doc-tool |
| Blocks tasks | Per cluster C-41; **task #3464 filed** |
| Disposition | **fix-now → task #3464 filed** ("reify doc --stdlib: stdlib-page surface in `reify doc` CLI"). Leaf observable: `reify doc --stdlib --out <dir>` produces HTML pages for stdlib traits/structures with known names visible in index. Depends on #3463 (GR-027) for the doc-model + HTML formatter, which depends on #3462 (GR-026) for the doc-field propagation. |
| Discovered | 2026-05-12 architecture audit |
| Notes | Final link in the doc-tool fix-now chain (3462 → 3463 → 3464). |

### GR-043 — Cost-per-byte LRU comparator inactive (cluster C-42)

| Field | Value |
|---|---|
| Mechanism | `WarmStatePool` stores `cost_per_byte` but eviction comparator is pure LRU; test actively pins the drift (`cost_per_byte_does_not_alter_lru_eviction_order`) |
| State | **DRIFT** (cost-per-byte data stored but unused; test cements drift) |
| Failure mode | F2 |
| Evidence | `findings/warm-state-eviction.md` M-004, M-005 |
| Cited by PRDs | warm-state-eviction |
| Blocks tasks | Per cluster C-42 |
| Disposition | **investigate-further** — flip comparator + wire cold-compute timing. Phase-3 synthesis §3 new-pattern-A example: this is the canonical "active drift pin — test cements PRD violation" case. Closing the gap requires explicit test removal/replacement, not just code change. Needs Leo decision before touching the test. |
| Discovered | 2026-05-12 architecture audit |
| Notes | The test name `cost_per_byte_does_not_alter_lru_eviction_order` is a verbatim contract that v0.1 ships pure LRU. Phase-3 marked this as a representative of the "drift pin" anti-pattern. |

### GR-044 — Warm-state pool: drain_events to journal never wired (cluster C-43)

| Field | Value |
|---|---|
| Mechanism | `WarmStatePool` buffers `Evicted` / `Donated` events; `drain_events()` has zero non-test callers; no GUI surface |
| State | **FICTION / PARTIAL** (event buffer ships; drain never called) |
| Failure mode | F4 |
| Evidence | `findings/warm-state-eviction.md` M-010, M-011 |
| Cited by PRDs | warm-state-eviction |
| Blocks tasks | Per cluster C-43; **task #3473 filed** |
| Disposition | **PRD-shape work — blocked on GR-002 contract DAG.** Filed as fix-now (task #3473) per the synthesis §4 disposition, but the cluster's natural sequencing is after GR-002 lands the ComputeNode lifecycle (warm-state pool feeds and drains at ComputeNode boundaries). The task's leaf observable (engine under memory pressure surfaces Evicted+Donated entries in `EvalResult.diagnostics`) is correctly user-observable and unblocks once the dispatch boundary exists. |
| Discovered | 2026-05-12 architecture audit |
| Notes | Filed AS fix-now (#3473) but practically gated on GR-002. If GR-002 reshapes the warm-state contract, #3473's leaf may shift. |

### GR-045 — Backward-compat alias `result.stress = result.stress.mid` (cluster C-44)

| Field | Value |
|---|---|
| Mechanism | PRD-promised aliasing for shell ElasticResult, documented in stdlib comments only; not implemented |
| State | **FICTION** (alias documented; not implemented) |
| Failure mode | F1 |
| Evidence | `findings/structural-analysis-shells.md` M-016; transitively `findings/fea-gui-rendering-shells.md` |
| Cited by PRDs | structural-analysis-shells, fea-gui-rendering-shells (transitive) |
| Blocks tasks | Per cluster C-44; **task #3474 filed** |
| Disposition | **fix-now → task #3474 filed** ("Stdlib shell ElasticResult: implement `result.stress = result.stress.mid` backward-compat alias"). Leaf observable: shell-solve fixture — `result.stress` and `result.stress.mid` yield identical tensor fields. Small mechanical fix; depends on GR-001 for the underlying ShellStress runtime form. |
| Discovered | 2026-05-12 architecture audit |
| Notes | Functionally gated on GR-001 (ShellStress structure-instance runtime), but the alias itself is mechanical once GR-001 lands. |

## Pending mergers from Phase 2

All clusters C-01 through C-44 promoted to GR entries during 2026-05-12 sweep. Open dispositions: GR-001 follow-up PRD (`structure-instance-runtime.md`) and GR-002 follow-up (`compute-node-contract.md` §8 DAG) are the two umbrella efforts; other clusters are covered by fix-now task filings (`phase-3-fixnow-filing-log.md`), accept-and-document records, or specific remediation sweeps (`phase-3-grammar-fiction-triage-log.md`, `phase-3-reopen-amend-sweep-log.md`).
