# Gap Register

Master list of architecture gaps discovered across the Reify PRD corpus. Phase 3 maintains this; Phase 2 agents write to their own per-PRD files + fused-memory, and Phase 3 promotes findings into GR-IDs here.


> **2026-05-14 WAL-recovery state.** The 2026-05-13 fused-memory SIGABRT discarded ~150 reify task rows (WAL uncheckpointed since 2026-05-11). Recovery splits into three classes:
>
> **Renumbered — originals lost, new IDs canonical:**
> - ComputeNode DAG `3491-3502` → `3420-3431`
> - multi-kernel-phase-3 DAG `3526-3542` → `3432-3448`
> - GR-024 buckling DAG `3576-3594` → `3449-3462`
> - structure-instance-runtime (GR-001) `3503/3504/3508/3510/3512` → `3540/3542/3544/3546/3549`
> - GR-016 gui-event-channel `3563-3574` → `3536-3552`
> - GR-007/018/037/043 ticket-replayed singletons `3575/3578-3584` → `3463-3468`
> - 41 orchestrator-spawned singletons (originals `3420-3461`) → `3476-3516`
> - 14 worktree-orphan tasks → `3522-3535` (partial recoveries from fix-now, grammar B1, reopen-amend, annotation-args, engine-integration-norm, etc.)
>
> **Shadow-paired — originals survived authoritatively:** Tasks 3413/3414 still authoritative (`done` / `pending` respectively); 3469/3470 are cancelled `audit_provenance` shadow rows from the ticket-replay process — do not cite them as renumberings. Tasks 3415-3419 are cancelled as singleton dupes.
>
> **Re-filed 2026-05-14 by agent team** (29 tasks + 3 reopens):
> - Fix-now batch (was 3462/3463/3464/3467/3468/3470/3471/3473/3474) → `3557/3562/3565/3572/3575/3578/3580/3582/3583`. Doc-tool chain 3557→3562→3565 sequenced; cross-PRD deps on SIR-α (3540) and CN-α (3420).
> - Grammar-fiction B-chains: B1 lowering+leaf (was 3477/3478) → `3558/3559` (deps on existing 3526); B2 trio (was 3480/3481/3483) → `3563/3564/3567`; B3 trio (was 3485/3486/3488) → `3569/3571/3573`.
> - Reopen-amend missing 3 (was 3484/3489/3490) → `3560/3568/3576`. Reopens re-applied: 250 → deferred (dep 3463), 2954 → deferred (dep 3527), 2699 → deferred (dep 3560). 2669 → pending (dep 3576).
> - GR-038 node-traits-unification DAG (was 3599-3607) → α=`3561`, β=`3566`, γ=`3570`, δ=`3574`, ε=`3577`, ζ=`3579` (wide_lock), η=`3581`, θ=`3584`, ι+κ=`3585`. Cross-PRD edge 3464→3574 wired.
> - AdHocSelector dedup: `3463` survives (scope from 3528 merged in); `3528` cancelled.
>
> **NOT recovered — truly unrecoverable:** task 3548 only (worktree cleaned pre-SIGABRT).
>
> Canonical translation: `dark-factory/docs/task-recovery-2026-05-13/id-map.json` (121 mappings) + sibling `singletons-map.json` + `worktree-orphans-map.json`. Forensic write-up: `dark-factory/docs/task-recovery-2026-05-13/investigation.md`.

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
| State | **DONE** (2026-05-26: SIR-α task 3540 + SIR-β-mat task 3542 merged; `Value::StructureInstance` shipped, starter-library `StructureName(...)` evaluates at runtime) |
| Failure mode | F1 (compile-time contract → no runtime backing) |
| Evidence | `crates/reify-eval/src/engine_eval.rs:114-125` (explicit comment); tasks 3213/3240/3264 (readiness probes, all done); task 2039 (parser side wired); no eval-side task filed |
| Cited by PRDs | `structural-analysis-fea` (Material starter lib, decomp #1 + signature in §"Sketch of approach"), `multi-load-case-fea` (`LoadCase(...)`, `MultiCaseResult(...)` ctors), `structural-analysis-shells` (transitive via FEA), composite-laminated-shells, varying-thickness-shells, structural-stability-buckling, fea-gui-rendering, persistent-naming-v2 (M-022 parallel), field-source-kinds (M-016), kinematic-constraints-toplevel (M-022), pragmas (transitive), reify-doc-tool (M-006 sibling), persistent-fea-cache (transitive). Total: 17 of 40 PRDs per phase-3-breadcrumb-map.md Cluster A |
| Blocks tasks | 3426 (pending), 3444 (pending), 3018 (pending), 2930 (pending), 2880-2884 (deferred), 2924 (pending, transitively), Stage-2 of 3213, plus follow-up chains in C-08 / C-16 / C-29 |
| Disposition | **PRD-shape work — Option B (typed Value variant, nominal conformance) — PRD AUTHORED + DECOMPOSED.** Resolution mode confirmed by Leo 2026-05-12. Follow-up PRD: `docs/prds/v0_3/structure-instance-runtime.md` (commit b6da30e1f8). Decomposed into 5 tasks (pending): SIR-α=**3540** (wide-lock foundation; in-progress / high priority), SIR-β-mat=**3542**, SIR-β-load=**3544**, SIR-β-sup=**3546**, SIR-β-mlcfea=**3549**; existing **3468** is SIR-γ envelope helpers (now depends on 3540). Cluster C-01 (phase-3-files-synthesis §1) is disposed by this entry. |
| Discovered | 2026-05-12, supervisor session during task 3378 unblock-triage (task 3378 cancelled-as-superseded by task 3426; see `phase-3-eight-dag-filing-log.md`) |
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
| Blocks tasks | 2924 (FEA #16 engine integration, pending), 2974 (persistent-fea-cache integration, pending), 3018 (multi-load-case end-to-end, deferred), 3005 (solve_load_cases, pending), 3426 (pending), every transitive consumer named in the citing PRDs |
| Disposition | **PRD-shape work — contract authored + decomposed.** Resolution mode confirmed by Leo 2026-05-12: option B + approach H (vertical-slice decomposition under design-first/contracts/boundary-tests discipline) per `preferences_implementation_chain_portfolio.md`. Authored as `docs/prds/v0_3/compute-node-contract.md` (commit d2cfe40980); supersedes `compute-node-infrastructure.md`'s accreted open design questions. Cluster C-02 disposed by this entry. **§8 DAG filed 2026-05-12** as 12 tasks 3420-3431 (α=3420, β=3421, γ=3422, δ=3423, ε=3424, ζ=3425, η=3426, θ=3427, ι=3428, κ=3429, μ=3430, ν=3431); 15 intra-DAG `add_dependency` edges wired; tasks flipped deferred→pending via commit_planning. ν=3431 done (found_on_main). η=3426 cross-PRD-depends on 3540 (SIR-α) + 3449. Filing log: `docs/architecture-audit/phase-3-eight-dag-filing-log.md`. |
| Discovered | 2026-05-12 architecture audit (Phase 2 findings on compute-node-infrastructure + 13 downstream PRDs) |
| Notes | The four contract questions Q-CN1..Q-CN4 (cancellation type, pending lifecycle, dispatch-registry scope, OpaqueState transfer rules) and the cross-cutting consumer policy Q-POL (which features route through ComputeNode vs bypass) are resolved in the contract document. Producer-side foundation tasks 3380/3381/3382/3385 are done. Tasks **3379/3383/3384 cancelled** (set_task_status reopen_reason citing the contract DAG): 3379 → subsumed by η=3426 vertical slice; 3383 → subsumed by γ=3422; 3384 → split across δ=3423 (pending) + ε=3424 (cancellation). Supersession provenance: 2924 ← η, 2947 ← κ, 2974 ← ι. |

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
| State | **DONE** (2026-05-26: task 3088 added the `Field<D,C>` arm to `resolve_parameterized_builtin_type`; task 3117 confirmed `Field<X,Y>` resolves in `param` position and tightened `ElasticResult.displacement`/`.stress`) |
| Failure mode | F1 |
| Evidence | `findings/structural-analysis-fea.md` M-022; `findings/a-posteriori-error-estimation.md` M-002/M-005/M-011; `findings/structural-analysis-shells.md` M-016/M-017; `findings/multi-load-case-fea.md` M-009; `findings/composite-laminated-shells.md` M-007/M-011; `findings/varying-thickness-shells.md` M-006; `findings/fea-gui-rendering-shells.md` M-004; `findings/structural-stability-buckling.md` M-003/M-005 |
| Cited by PRDs | structural-analysis-fea, a-posteriori-error-estimation, structural-analysis-shells, multi-load-case-fea, composite-laminated-shells, varying-thickness-shells, fea-gui-rendering-shells, structural-stability-buckling |
| Blocks tasks | Per cluster C-03 (`phase-3-files-synthesis.md` §1); umbrella task 3117 |
| Disposition | **fix-now → existing task #3117 adequate** (`phase-3-fixnow-filing-log.md` "Existing task adequate"). Task title already specifies user-observable outcome (tighten `ElasticResult::displacement` and `::stress` from Real → Field<X,Y>); description names probe test + resolver fix path. |
| Discovered | 2026-05-12 architecture audit |
| Notes | Single language-feature gap that blocked the entire FEA-stack field-typing claim (breadcrumb cluster B); **resolved** — tasks 3088/3117 merged. Now consumed by `anisotropic-heterogeneous-elastostatics.md` (`param material : Field<Point3<Length>, AnisotropicMaterial>`). |

### GR-007 — Library-shipped / no-DSL-consumer (selector resolution) (cluster C-04)

| Field | Value |
|---|---|
| Mechanism | Library functions exist with tests, but no surface DSL path invokes them: `resolve_unique_by_attribute`, `resolve_unique_by_tag`, ad-hoc `@face("name")` evaluator, `narrow_arms_under_guard`, `NodePolicyOverrides` config |
| State | **PARTIAL/FICTION** (Rust public surface exists; DSL surface missing) |
| Failure mode | F2 |
| Evidence | `findings/persistent-naming-v2.md` M-013/M-014/M-019/M-022; `findings/topology-selectors.md` M-003; `findings/match-block-decls.md` M-012; `findings/node-trait-composition.md` M-010; `findings/auto-type-param-resolution.md` M-009/M-016 |
| Cited by PRDs | persistent-naming-v2, topology-selectors, match-block-decls, node-trait-composition, auto-type-param-resolution |
| Blocks tasks | Per cluster C-04; intersects task 2652 ("done" library-only) |
| Disposition | **fix-now (residuals) + accept process trajectory** — 2026-05-12 investigate-further triage resolved: Leo directed "sweep all currently-done C-04 tasks; file targeted tasks for anything without a consumer and also not explicitly handled by sibling clusters." /prd's G1 consumer-named gate (preferences-implementation-chain-portfolio approach A) prevents recurrence going forward; today's residuals filed below. The blanket policy ("definition of done = user-observable") is the implicit norm now per [[feedback-task-chain-user-observable]]. |
| Discovered | 2026-05-12 architecture audit |
| Notes | Sibling-cluster handling: selector_vocabulary_v2 → GR-013/C-10; resolve_unique_by_attribute → 3466 (in C-10 entry). Residuals filed 2026-05-12 via fused-memory submit_task; **all three curated to tasks 2026-05-13**: (1) `tkt_0RNVPZERQVAE6SQFSBTB1AMSGM` → **task 3575** "Wire AdHocSelector engine-side evaluator (@face(\"name\") → handle)" (reopens phantom-done Task 250's M-022 root cause). (2) `tkt_0RNVQ0MQMVRKAA3PB6W8TP2324` → **task 3578** "Wire NodePolicyOverrides config-file ingestion (reify.toml [node_overrides])" (satisfies node-trait-composition acceptance #3; depends_on 3602/GR-038 δ). (3) `tkt_0RNVQ26ARAN08H900XE501NZG5` → **combined → existing done task 2329** (curator dedup: "Re-route Filtered Selectors through Feature-Tag Resolution" was already done). Grammar-gated holdout: `narrow_arms_under_guard` (match-block-decls M-012) call-site wiring is structurally complete but requires decl-level `match` grammar landing (cluster C-06) — tracked there. Filing log: `docs/architecture-audit/phase-3-investigate-further-triage-log.md`. |

### GR-008 — Auto-resolve / type-param resolver compile-pipeline call site (cluster C-05)

| Field | Value |
|---|---|
| Mechanism | Phase A/B/C orchestrator + DFS + backjumping all wired in `auto_type_params/`; no production caller invokes them from `compile_*`; `CompiledModule.auto_type_substitution` never written |
| State | **FICTION** (consumer half — orchestrator exists, compile pipeline doesn't invoke) |
| Failure mode | F1 |
| Evidence | `findings/auto-type-param-resolution.md` M-009/M-010/M-014; `findings/auto-resolution-backtracking.md` M-002/M-014; `findings/kleene-logic.md` M-002 (sibling — `implies` operator no parser); `findings/match-block-decls.md` M-001; `findings/specialization-scope.md` M-002; `findings/shadowing-warning.md` M-015/M-016 |
| Cited by PRDs | auto-type-param-resolution, auto-resolution-backtracking, kleene-logic, match-block-decls, specialization-scope, shadowing-warning |
| Blocks tasks | Per cluster C-05; **task 3522 filed** (originally 3465; remapped via 2026-05-13 SIGABRT recovery — see `phase-3-fixnow-filing-log.md` top banner) |
| Disposition | **fix-now → task #3522 filed** ("Auto-type-param resolver: invoke Phase A/B/C orchestrator from compile pipeline; populate CompiledModule.auto_type_substitution"). Leaf observable: fixture .ri with inferable type-param compiles AND eval yields correctly-typed value (not Real placeholder); negative-path emits `E_AUTO_TYPE_PARAM_UNRESOLVED`. |
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
| Notes | Pure subcase of C-01 from a structural standpoint; surfaced as own cluster because the cardinality (7 PRDs leaning on Load/Support) deserves explicit tracking. Resolved on the same PRD as C-01/GR-001. **Resolution mechanism: `docs/prds/v0_3/structure-instance-runtime.md`** (authored 2026-05-12, decomposed). SIR-α foundation slice (**task 3540**) declares `trait Load` + `trait Support` + first PointLoad/FixedSupport rewrites (snake_case → PascalCase consolidation); SIR-β-load (**task 3544** — PressureLoad) and SIR-β-sup (**task 3546** — PinnedSupport) wave-2 tasks (both depend on 3540) close the cluster fully. |

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
| Evidence | `findings/fea-gui-rendering.md` M-013/M-015; `findings/fea-gui-rendering-shells.md` M-001..M-009; `findings/multi-load-case-fea.md` M-016; `findings/mesh-morphing.md` M-014; `findings/structural-stability-buckling.md` M-013; `findings/warm-state-eviction.md` M-011; ~~`findings/persistent-naming-v2.md` M-018 (Manifold hook UI)~~ (struck 2026-05-14 per task 3548 / PRD task ι — see GR-004 for the actual chain) |
| Cited by PRDs | fea-gui-rendering, fea-gui-rendering-shells, multi-load-case-fea, mesh-morphing, structural-stability-buckling, warm-state-eviction, ~~persistent-naming-v2~~ |
| Blocks tasks | Per cluster C-13 (7+ PRDs converging on absent emitter surface) |
| Disposition | **PRD-shape work — resolved via `docs/prds/v0_3/gui-event-channel-inventory.md`** (authored 2026-05-12; decomposed 2026-05-13). PRD owns: inventory document, naming/payload/versioning/test convention, all currently-absent emitter wiring for true C-13 channels (auto-resolve trio as proof slice; warm-pool, solver-progress, fea-case-changed, mode-shape v0.5+ as forward slices; morph-stats debug-MCP RPC). Cross-PRD seam ownership is centralized in this PRD per Leo decision 2026-05-12; per-channel emitter tasks list upstream data-source prereqs as cross-PRD metadata deps. **Decomposed 2026-05-13** into 12 tasks **3536-3552** (α=3536 inventory doc — DONE found_on_main; through μ=3552 contributor doc); 22 dep edges wired; η/3026 seam-overlap re-handled via append+add_dep; ζ/2965 arrow reversed (2965 deps ζ). |
| Discovered | 2026-05-12 architecture audit |
| Notes | Highest-cardinality scaffold-without-caller cluster on the GUI side. The Phase-3 scaffold-pattern critique flags this as the GUI mirror of cluster C-02 (ComputeNode producer). **Citation correction (2026-05-12, PRD-authoring trace):** PRD §1 chain analysis identified that the `persistent-naming-v2 (M-018 Manifold hook UI)` evidence citation conflates a backend-only stub (Manifold MeshGL walk, already separately tracked under GR-004) with this GUI-event-channel cluster. PNv2 M-018 surfaces through existing `mesh-update` + selection state delta channels and does not need a new event channel; its actual chain breaks at GR-004 (Manifold producer) + GR-013 (selector v2 dispatch) + task 2699 (v0.1 selector dispatch arms). PRD task ι (#3548) **applied 2026-05-14** — citation struck in Evidence + Cited by PRDs rows above; effective citing-PRD count is **6** (not 7+). |

### GR-017 — Engine wiring: kernel module callable in isolation, no engine consumer (cluster C-14)

| Field | Value |
|---|---|
| Mechanism | `reify-mesh-morph` engine wire absent; `reify-shell-extract` not depended on by `reify-solver-elastic`/`reify-eval`; `dispatcher::dispatch` not called from `execute_realization_ops`; `propagate_freshness_only` no engine caller; `dispatch_volume_mesh` no caller; `mesh_surface_to_volume_with_diagnostics` no caller |
| State | **FICTION / PARTIAL** (kernel surfaces ship without engine integration) |
| Failure mode | F4 / F6 |
| Evidence | `findings/mesh-morphing.md` M-012/M-013/M-014/M-015/M-016/M-017/M-018; `findings/structural-analysis-shells.md` M-018; `findings/hex-wedge-meshing.md` M-017/M-018; `findings/multi-kernel.md` M-004/M-014/M-015; `findings/freshness-4-variant.md` M-013; `findings/structural-analysis-fea.md` (multiple) |
| Cited by PRDs | mesh-morphing, structural-analysis-shells, hex-wedge-meshing, multi-kernel, freshness-4-variant, structural-analysis-fea |
| Blocks tasks | 2924 (FEA #16 engine integration), 2947 (mesh-morph engine wire); per cluster C-14 |
| Disposition | **PRD-shape work — RESOLVED 2026-05-12 by `docs/prds/v0_3/engine-integration-norm.md`.** Norm catalogs 7 in-engine seams (§3.1 op-execute, §3.2 realization-kind dispatch, §3.3 multi-kernel dispatch, §3.4 ComputeNode dispatch, §3.5 ConstraintSolver, §3.6 freshness-only walk, §3.7 KernelAttributeHook) + 1 deprecated (§3.8 OptimizedImpl); specifies per-seam consumer policy (§4); provides G1 checklist for `/prd` (§5); declares relationship to G-tool (§6.1, normative complement) + F-infra (§6.2, future consumer) + CN-contract (§6.3, single-seam contract for §3.4) + multi-kernel-phase-3 (§6.4, owns §3.1/§3.3). Migration: grandfathered until touched (§9). Mesh-morph worked example (§7) cross-references CN-contract §8 task κ; tasks 2924/2947 sit under this norm's §3.2 + §3.4 axes. Decomposition (§12) is 4 mandatory doc-only leaves (α norm doc lands + β `/prd` G1 update + γ CN-contract cross-ref + δ mesh-morph PRD cross-ref) + 1 optional G-allow sweep on engine-seam orphans. |
| Discovered | 2026-05-12 architecture audit |
| Notes | Phase-3 synthesis §5d (the inversion-of-PRD-ordering observation) is the systemic finding — code lands ahead of engine seams. The norm "every PRD decomposition includes its engine-integration phase" is the durable preventative. **Resolution mechanism: `docs/prds/v0_3/engine-integration-norm.md`** (authored 2026-05-12). Two-sided gate: forward-facing (this PRD's §3 prescribes which seam each kernel module plugs into) + backward-facing (`scripts/audit-orphan-producers.sh` — the G-tool — detects Type-A producer-orphans). `// G-allow:` marker convention extended to cite norm §3.N entries (§6.1). Approach G code-side detector already shipped + baselined (`docs/architecture-audit/g-tool-baseline-report.md`); F-infra audit cadence queued separately and will consume §3 as input. §3.7 (KernelAttributeHook) ownership between persistent-naming-v2 and multi-kernel-phase-3 remains contested per breadcrumb-map §3 #2 — listed, not assigned. |

### GR-018 — Unbounded geometry primitives (half_space / extrude_infinite) (cluster C-15)

| Field | Value |
|---|---|
| Mechanism | Diagnostic infrastructure (`E_GEOMETRY_UNBOUNDED`), inference fallback, warning machinery all wired and waiting; producers of `Bounded=false` (half_space, extrude_infinite) are absent |
| State | **FICTION** (loaded-gun-no-target — consumers waiting, producers absent) |
| Failure mode | F1 |
| Evidence | `findings/geometry-traits.md` M-006/M-009; `findings/topology-selectors.md` M-016 |
| Cited by PRDs | geometry-traits, topology-selectors |
| Blocks tasks | Per cluster C-15 |
| Disposition | **fix-now — ship producers (vindicate the diagnostic infra).** 2026-05-12 investigate-further triage resolved: Leo chose "ship producers." Diagnostic infrastructure stays; `half_space` and `extrude_infinite` filed as sibling fix-now tasks. |
| Discovered | 2026-05-12 architecture audit |
| Notes | Phase-3 synthesis classified this under "Clusters fitting NO Phase-2 pattern" — the "loaded gun, no target" shape. Tickets filed 2026-05-12; **curated to tasks 2026-05-13** then lost in the 2026-05-13 SIGABRT and **replayed 2026-05-14** to live IDs: `tkt_0RNVQ2E62WDXJ76QX594YKQHKT` → **task 3465** (half_space, was 3579) and `tkt_0RNVQ2KTNNR3EAN7N2A7KDB0W3` → **task 3466** (extrude_infinite, was 3580). Both name the `E_GEOMETRY_UNBOUNDED` diagnostic consumer + integration-test signal. Filing log: `docs/architecture-audit/phase-3-investigate-further-triage-log.md`. |

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
| Notes | Once GR-001 Option B lands, every starter-library `StructureName(...)` evaluates to `Value::StructureInstance` and the FEA-stack chain unblocks. The material library itself is then a stdlib `.ri` authoring task. **Resolution mechanism: `docs/prds/v0_3/structure-instance-runtime.md`** (authored 2026-05-12, decomposed). SIR-α foundation slice (**task 3540**) makes `Steel_AISI_1045()` reachable through the new ctor lowering path (the existing `structure def Steel_AISI_1045 : ElasticMaterial { ... }` at `materials_fea.ri:132` becomes evaluable); SIR-β-mat (**task 3542** — depends on 3540) closes the remaining three materials (`Aluminium_6061_T6`, `Titanium_Ti6Al4V`, `ABS_Plastic` — also already declared but unreachable today). |

### GR-020 — Kernel/eval ReprKind chain coverage gaps (cluster C-18)

| Field | Value |
|---|---|
| Mechanism | Convert edges absent from capability descriptors (BRep→Mesh, BRep→Voxel, Voxel→Mesh, Mesh→BRep); cache key fields incomplete (force_tet not in compute key); per-handle ReprKind tracking absent |
| State | **FICTION / PARTIAL** (dispatcher abstractions exist; conversion edges + cache-key fields missing) |
| Failure mode | F6 |
| Evidence | `findings/multi-kernel.md` M-007/M-009/M-010/M-011/M-014/M-015; `findings/hex-wedge-meshing.md` M-024; `findings/structural-analysis-shells.md` M-025 |
| Cited by PRDs | multi-kernel, hex-wedge-meshing, structural-analysis-shells |
| Blocks tasks | Per cluster C-18 |
| Disposition | **PRD-shape work — resolved 2026-05-12 via `docs/prds/v0_3/multi-kernel-phase-3.md`** (commit e477a68d96); decomposed + queued same day. The ReprKind / dispatcher contract is multi-kernel's native domain; the PRD ships Phase 3 as B+H decomposition. Cross-PRD coordination with `compute-node-contract.md` settled at §6: separate dispatch surfaces meeting at the cache-key boundary. Folds in **GR-034** (long-chain diagnostic wiring at §8 task ρ) and the OpenVDB consumer half of **GR-003** (§8 task θ). **§8 DAG filed** as 17 tasks **3432-3448** (α=3432, β=3433, γ=3434, δ=3435, ε=3436, ζ=3437, η=3438, θ=3439, ι=3440, κ=3441, ξ=3442, ο=3443, π=3444, ρ=3445, μ=3446, ν=3447, τ=3448); 18 intra-batch deps wired; cross-PRD edge ξ(3442)→3426 (compute-node-contract η). Filing log: `docs/architecture-audit/gr020-multi-kernel-phase-3-filing-log.md`. |
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
| Disposition | **PRD-shape work — shell-extract engine integration PRD.** Specific case of the engine-integration norm (GR-017); shells is large enough to warrant its own PRD slot. Also intersects GR-016 (GUI event channel) on the IPC half. **Resolution mechanism: `docs/prds/v0_4/shell-extract-engine-bridge.md`** (authored 2026-05-12) — vertical-slice decomposition under B+H discipline; supersedes parent shells PRD T18/T19/T23 (tasks 3031/3032/3036) and completes the engine-side fold-in half of T20 (3033). Plugs into landed seams GR-001 (struct-instance runtime), GR-002 (ComputeNode contract via `shell-extract::extract` target = engine-integration-norm §3.4), GR-016 (MeshData payload extension per §2.4 delegation), GR-017 (G1-checklist conformant; §3.4-only seam coverage — no new §3.2 realization-kind dispatcher). |
| Discovered | 2026-05-12 architecture audit |
| Notes | Mid-surface naming (Role::MidSurfaceEdge + FeatureId::derived_mid_surface) is wired in `reify-types`; the missing piece is plumbing through the kernel→engine→solver/GUI seams. Producer half (mid-surface + segmentation + thickness + naming records) ships in `reify-shell-extract` with synthetic-input testing; consumer-side bridge for FEA `ElasticResult { shell_channels: Option<ShellChannels> }` + GUI `MeshData { element_kind, region_tags, vector_channels }` is the PRD's deliverable. Resolution mechanism: `docs/prds/v0_4/shell-extract-engine-bridge.md`. |

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
| Disposition | **PRD-shape — RESOLVED 2026-05-13 by `docs/prds/v0_3/mesh-morphing-phase-2.md`.** Phase-2 sibling PRD bundles GR-023 vertex_to_vertex fill with M-005 (Gmsh `NodeAttachment` producer), M-006 (OCCT `Projector` impl), and engine-wire 2947 (dep extension). Five-task DAG: α PNv2 vertex widening (cross-PRD seam, PNv2-owned), β Stage-B vertex bijection fill (deletes 6 active-drift-pin tests + named v0.2-always-empty test per Phase-3 §3 new-pattern-A), γ Gmsh `NodeAttachment` producer, δ OCCT `Projector` impl, ε engine-wire 2947 deps extended. Resolution mode: portfolio approach E (cross-PRD seam ownership) + H (design-first + two-way boundary tests). Mesh-morph half of GR-017 (cluster C-14) resolves naturally when ε lands. |
| Discovered | 2026-05-12 architecture audit |
| Notes | Phase-3 synthesis classified this under "Clusters fitting NO Phase-2 pattern" — "known-empty hole in shipped data structure." Reciprocal mesh-morph ↔ PNv2 audit cite. **Resolution mechanism: `docs/prds/v0_3/mesh-morphing-phase-2.md`** (authored 2026-05-13 via `/prd` from session-prompt at `docs/architecture-audit/gr023-mesh-morph-prd-revisit-session-prompt.md`). PNv2 widening (task α) symmetric: adds `BRepKind::Vertex`, `OcctKernel::extract_vertices`, per-op vertex-attribute seeding; PNv2 PRD remains owning_prd via task metadata + real cross-batch `add_dependency` edge per [[preferences_cross_prd_deps_real_edges]]. Engine-wire 2947 unchanged in spec; only deps extended via `update_task` in the phase-2 decompose pass. Cross-link: GR-017 / cluster C-14 engine-wire disposition (engine-integration-norm §3.2 — VolumeMesh realization-kind dispatch). |

### GR-024 — Eigenvalue solver + geometric stiffness K_g (cluster C-22)

| Field | Value |
|---|---|
| Mechanism | Lanczos/Arnoldi/shift-invert via faer-rs named as the path but no `eigensolve` / `K_g` module exists; eigenvalue solver is the largest net-new kernel surface for buckling |
| State | **FICTION** (named but unbuilt) |
| Failure mode | F6 |
| Evidence | `findings/structural-stability-buckling.md` M-006, M-007 |
| Cited by PRDs | structural-stability-buckling, (modal-analysis future PRD — non-PRD breadcrumb) |
| Blocks tasks | Per cluster C-22 |
| Disposition | **PRD-shape work — resolved via `docs/prds/v0_5/buckling-eigensolver.md`** (authored 2026-05-12 in interactive `/prd` session under G1–G5+META gates). PRD owns: `solve_buckling` stdlib entry + trampoline, shift-invert Lanczos eigensolver on faer-rs `operator::self_adjoint_eigen` (with dense gevd fallback for tiny problems), P1-tet K_g element kernel + global assembly, `BucklingResult` / `Mode` / `BucklingOptions` / `MultiCaseBucklingResult` value shapes, GUI `mode-shape-frame` channel implementation (replaces GR-016 deferred bookmark task λ). Foundation gates: GR-001 (struct-instance runtime), GR-002 (ComputeNode contract, landed), FEA stack reaching ComputeNode integration. Shell K_g is out of scope for v0.5; trampoline emits `E_BucklingShellNotImplemented` citing task 3392. Decomposition is B+H (contract + cross-crate boundary tests) per `preferences_implementation_chain_portfolio`; vertical-slice DAG in §13 of the PRD with eleven phases, each leaf naming a user-observable signal. |
| Discovered | 2026-05-12 architecture audit |
| Notes | Modal analysis (unfiled future PRD) would share this infrastructure; the eigensolver crate-location decision in the PRD (`reify-solver-elastic/src/eigensolve.rs`) is deliberate so that mass-matrix `(K − ω²M)φ = 0` modal generalization is a sibling registration, not a re-architecture. PRD §5 notes the generalization-friendliness explicitly. |

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
| Mechanism | **`docs/notes/stdlib-trait-audit.md` pre-dates inheritance fixes — RESOLVED 2026-05-14 (task 3529): renamed to `stdlib-trait-breadth-audit-v01.md` and refreshed; PRD-promised `stdlib-trait-breadth-audit-v01.md` deliverable now present at the named path**; PRD-named example file `integration_full_v01.ri` missing `#precision(0.001m)` (resolved by commit `7f01d82e9c`); example `multi_load_bracket.ri` doesn't exist; `examples/m11_annotations.ri` doesn't exercise solver_hint collections; PRD-named smoke test on `examples/m5_purpose.ri` doesn't exist; `docs/auto-type-param-resolution.md` completeness unverified |
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
| Notes | Captured in synthesis §1 as "depends on C-01" — the leaf is small and fix-now, the dependency chain is structural. **Functional unblock mechanism: `docs/prds/v0_3/structure-instance-runtime.md`** (authored 2026-05-12, decomposed). Task 3468 executes against this PRD's SIR-α foundation slice — once Value::StructureInstance is live, the typed-envelope helpers consume it for ShellStress/LaminateStress frames per the existing task scope. **Real dep edge wired**: 3468 depends_on **3540** (SIR-α). |

### GR-032 — Local-disk NFS detection + cache GC + cache CLI surface (cluster C-30)

| Field | Value |
|---|---|
| Mechanism | `reify cache stats|clear|gc|export|import` not in `reify-cli/src/main.rs`; NFS detection (PRD §"Local-disk only") not implemented; cost-aware LRU GC not implemented |
| State | **TODO** (PRD-decomposed but blocked on upstream FEA wiring) |
| Failure mode | F4 |
| Evidence | `findings/persistent-fea-cache.md` M-010, M-013, M-014, M-015, M-016, M-017, M-018, M-019 |
| Cited by PRDs | persistent-fea-cache |
| Blocks tasks | Per cluster C-30 (~8 tasks already decomposed under persistent-fea-cache PRD) |
| Disposition | **PRD-shape work — gated on GR-002 contract DAG (now filed).** Persistent-cache work cannot land until ComputeNode dispatch ships (its consumer surface). GR-002's vertical-slice DAG was filed 2026-05-12; once task η=**3426** (solve_elastic_static end-to-end vertical slice, pending) lands, persistent-fea-cache's pre-existing decomposition activates. Per the compute-node-contract DAG filing log: `2974 ← ι=3428` (persistent-cache integration supersedes 2974); cross-PRD ι=3428 supersession recorded in contract metadata. |
| Discovered | 2026-05-12 architecture audit |
| Notes | Already PRD-decomposed: 13 tasks **2969-2981** filed 2026-05-04 (commit f85fbbad00) — trait foundation, storage layer, integration, lifecycle, CLI, validation. Task 2974 (ComputeNode wiring integration) is superseded by GR-002 task ι=3428. |

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
| Disposition | **folded into GR-020 — resolution mechanism `docs/prds/v0_3/multi-kernel-phase-3.md` §8 task ρ (Phase 8 — Long-chain diagnostic wiring) FILED as task 3445.** Once the multi-kernel Phase 3 dispatcher fan-out lands (§8 tasks ε=3436, ι=3440 — 3445's real dep edges), `is_long_chain_realization` + `long_chain_diagnostic` get called from `execute_realization_ops` with wall-time bracketing. `per_stage_tolerance_for_plan` becomes meaningful because real multi-stage chains exist. Not separately fix-now-able; rides with GR-020. |
| Discovered | 2026-05-12 architecture audit |
| Notes | Cascading downstream of GR-020; resolution ships in the same PRD (`multi-kernel-phase-3.md`) at §8 task ρ. 2026-05-12 investigate-further triage confirmed: GR-020 was decomposed and queued by Leo earlier same day; task ρ's observable signal (synthetic 3-stage chain fixture; 1-stage chain at same wall time does NOT emit) satisfies the user-observable-leaf gate. PRD §9 open question #6 (wall-time vs CPU-time) is decided in-task. |

### GR-035 — Cancellation handle placeholder type (cluster C-33)

| Field | Value |
|---|---|
| Mechanism | `CancellationHandle` is `struct CancellationHandle;` (unit type); real type deferred to P3.5; FEA cancellation regression test unimplementable |
| State | **TODO** (placeholder unit type; design-deferred) |
| Failure mode | F3 |
| Evidence | `findings/compute-node-infrastructure.md` M-003, M-007; `findings/structural-analysis-fea.md` M-007; transitively `findings/structural-stability-buckling.md` M-002 |
| Cited by PRDs | compute-node-infrastructure, structural-analysis-fea, structural-stability-buckling (transitive) |
| Blocks tasks | Per cluster C-33; **task #3384 pending** ("pick one of three options") |
| Disposition | **PRD-shape work — RESOLVED via GR-002 contract DAG.** Task 3384 cancelled 2026-05-12 (reopen_reason cites contract DAG); supersession provenance: split into δ=**3423** (pending lifecycle) + ε=**3424** (cancellation). Q-CN1 resolution in the contract document: `CancellationHandle` wraps `Arc<AtomicBool>` with 100ms poll budget. |
| Discovered | 2026-05-12 architecture audit |
| Notes | Task 3384 cancelled (verified via get_task: status=cancelled, dependencies=[3380,3383], reopen_reason="Superseded by ComputeNode contract DAG... split across δ + ε"); see GR-002 entry for full supersession map. |

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
| Disposition | **fix-now — widen recognizer.** 2026-05-12 investigate-further triage resolved: Leo chose to widen `RepresentationWithin(subject, tol)` to accept member-access subjects (`subject.head`-style) with a named FEA consumer fixture (`bracket.fea_subject` pattern). Ticket filed below. |
| Discovered | 2026-05-12 architecture audit |
| Notes | Phase-3 synthesis classified this under "MVP scope clip silently drops downstream cases." Ticket filed 2026-05-12; **curated 2026-05-13**: `tkt_0RNVQ2W0KGY8ZFBR9794XN4406` → **task 3581** "Widen RepresentationWithin recognizer to accept member-access subjects" — widen recognizer + symmetric input-promise widening; FEA bracket fixture is the named consumer. Filing log: `docs/architecture-audit/phase-3-investigate-further-triage-log.md`. |

### GR-038 — NodeTraits + scheduler dispatch never bridge (cluster C-36)

| Field | Value |
|---|---|
| Mechanism | Parallel taxonomies: `NodeTraits` bitflags + `NodeArchKind` (7 variants) in reify-types AND `NodePolicyOverrides` + `NodeKind` (5 variants) in reify-runtime. Scheduler reads NodePolicyOverrides, ignores NodeTraits |
| State | **FICTION** (NodeTraits has no scheduler consumer) |
| Failure mode | F1 |
| Evidence | `findings/node-trait-composition.md` M-002, M-003, M-005, M-008, M-009, M-011 |
| Cited by PRDs | node-trait-composition |
| Blocks tasks | Per cluster C-36 |
| Disposition | **PRD authored + decomposed — `docs/prds/v0_3/node-traits-unification.md`** (2026-05-13, commit 39f687bcae). Direction: **C′ refined bridge** — retire `NodeArchKind` only (collapse into single canonical `NodeKind` mirroring `NodeId`'s 5 variants); keep `NodeTraits` and `NodePolicyOverrides` as orthogonal surfaces because they answer different architectural questions (§7.6 static affordances vs §7.3 per-instance policy); build five named bridges (per-`NodeId` trait map B1; `traits_to_priority` B2; `default_overrides(NodeKind, NodeTraits)` B3; IMMEDIATE→never-cancelled scheduler guard B4; `WARM_STARTABLE`↔`WarmStartable` registry coextension assert B5; `PROGRESSIVE` invariant cache-write guard B6). Supersedes `docs/prds/node-trait-composition.md` acceptance criteria #1, #3, #5. **Decomposed 2026-05-13** into 9 tasks **3599-3607** (α=3599, β=3600, γ=3601, δ=3602, ε=3603, ζ=3604, η=3605, θ=3606, ι+κ=3607 combined); 8 intra-batch deps + 1 cross-PRD dep wired (ticket 3578 depends_on δ=3602). Integration gate: ε/3603 (`reify dev inspect-node` CLI). |
| Discovered | 2026-05-12 architecture audit |
| Notes | Synthesis §3 new-pattern-B canonical instance. The other new-pattern-B occurrence (Load/Support nominal vs kind-tagged, GR-011 / cluster C-08) resolves under GR-001 with a different shape because GR-011's two surfaces ARE answering the same question; GR-038's two surfaces (static traits vs per-instance policy) are NOT — hence C′ rather than enum-collapse. Session prompt: `docs/architecture-audit/gr038-nodetraits-unification-session-prompt.md`. Resolution mode per portfolio: H (design-first + interface contracts + two-way boundary tests; §9 of the PRD has 7 boundary scenarios facing both reify-types and reify-runtime sides). Cross-PRD seam: GR-007 ticket `tkt_0RNVQ0MQMVRKAA3PB6W8TP2324` (now **task 3578**) **NOT superseded** — its `[node_overrides]` reify.toml schema sits at precedence level 3 (between type-overrides and kind-derived defaults); the PRD's δ task (**3602**) wires the precedence slot, the ticket fills it. Real cross-PRD dep edge: 3578 depends_on 3602 (verified). **Retired on landing:** `NodeArchKind` enum, `default_traits(NodeArchKind)` (replaced by `NodeKind::default_traits`). |

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
| Disposition | **accept-and-document — APPLIED 2026-05-14** in `docs/prds/deep-dot-chain.md` (new section "Note on AC #3 (mixed call+access) — GR-040, 2026-05-14"). Flagged as language-design open: lint passes vacuously on `a.b.foo().c.d` because the syntax doesn't parse. UFCS sugar identified as the minimal-cost upgrade path if/when method-call syntax becomes valuable. No fix-now. |
| Discovered | 2026-05-12 architecture audit |
| Notes | Sibling to GR-009 (cluster C-06 grammar fictions); but unlike C-06's many small invented syntaxes, this is one feature that several future PRDs would benefit from (fluent transform chains, structure-instance methods, domain idiom `material.young_modulus()`). Reify's anti-thesis ("physical/mechanical nonsense should be hard to encode") doesn't lean OO, so method-call syntax is largely cosmetic. Disposition closed-out 2026-05-14; revisit if/when a PRD blocks on the syntax. |

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
| Notes | Phase-3 synthesis §5g flagged the "additive on top of fictional foundation" framing as worth Leo's awareness: composite-laminated-shells says "through-thickness becomes a sum over plies" but the current through-thickness is analytical closed-form for constant thickness, not a sum-over-anything. If these PRDs activate before v0.5, this disposition needs revisiting — **see Partial activation (2026-05-26) below.** |

#### Partial activation (2026-05-26) — constitutive-law portion promoted to an owned foundation

The anticipated revisit happened. A new program (`docs/prds/v0_5/fdm-as-printed-fea.md`, FEA on the as-printed FDM structure) needs an anisotropic + spatially-varying constitutive law, which is the *same* "Orthotropic constitutive law / MaterialConstitutiveLaw" surface this cluster parked as deferred FICTION. Rather than re-derive it inside the FDM program (and again inside composite-shells), the **3D-solid constitutive core** is factored into a shared upstream foundation: `docs/prds/v0_5/anisotropic-heterogeneous-elastostatics.md` (authored 2026-05-26 via `/prd`, B+H, decompose-ready).

Scope split (resolves the G4 ownership cleanly):
- **Foundation owns** the `ConstitutiveLaw` trait (the audit's "MaterialConstitutiveLaw abstraction"), `OrthotropicMaterial` / `TransverseIsotropicMaterial`, the 6×6 Voigt frame rotation, per-element spatially-varying assembly, and the generalised `solve_elastic_static.material` argument (`ConstitutiveLaw | Field<Point3, AnisotropicMaterial>`, scalar auto-lift).
- **`composite-laminated-shells.md` retains** its plane-stress reduction + ply-stack through-thickness integration + composite failure criteria (Tsai-Wu/Hashin) — it now *consumes* the foundation's constitutive surface rather than building orthotropy itself. A companion task (foundation PRD task η) edits the composite-shells "Sketch of approach" accordingly and wires a real cross-PRD dep edge.
- **`fdm-as-printed-fea.md` consumes** the foundation's `Field<Point3, AnisotropicMaterial>` solver argument.

So this entry's disposition is now **partially active**: the orthotropic/transverse-isotropic *stiffness* surface is owned and tasked (via the foundation PRD's decomposition). The rest of GR-041 — `Laminate`/`Ply`, composite *failure criteria*, `K_g`/buckling eigensolver (GR-024-tracked separately), `linear_taper`/varying-thickness — remains deferred v0.5+ FICTION as before. Foundation prerequisites GR-001 (struct-ctor runtime) and GR-006 (`Field<X,Y>` in param) are both **DONE** (tasks 3540/3542 and 3088/3117 respectively; this register's GR-001/GR-006 State columns are stale and lag the merged tasks).

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
| Disposition | **fix-now — flip comparator + replace pinning test.** 2026-05-12 investigate-further triage resolved: Leo chose "flip comparator + replace test." Ticket filed below; deliberate contract change. |
| Discovered | 2026-05-12 architecture audit |
| Notes | Phase-3 synthesis §3 new-pattern-A canonical instance ("active drift pin — test cements PRD violation"). Ticket filed 2026-05-12; **curated 2026-05-13**: `tkt_0RNVQ35E9X59VSEA90DQH5VZ7X` → **task 3584** "Flip WarmStatePool cost_per_byte LRU comparator + replace pinning test" — wires cost_per_byte into the comparator, DELETES `cost_per_byte_does_not_alter_lru_eviction_order` pinning test, ADDS `cost_per_byte_alters_lru_eviction_order` replacement, instruments ComputeNode cold-compute timing so the field is populated in production. Filing log: `docs/architecture-audit/phase-3-investigate-further-triage-log.md`. |

### GR-044 — Warm-state pool: drain_events to journal never wired (cluster C-43)

| Field | Value |
|---|---|
| Mechanism | `WarmStatePool` buffers `Evicted` / `Donated` events; `drain_events()` has zero non-test callers; no GUI surface |
| State | **FICTION / PARTIAL** (event buffer ships; drain never called) |
| Failure mode | F4 |
| Evidence | `findings/warm-state-eviction.md` M-010, M-011 |
| Cited by PRDs | warm-state-eviction |
| Blocks tasks | Per cluster C-43; **task #3473 filed** |
| Disposition | **PRD-shape work — gated on GR-002 contract DAG (now filed).** Filed as fix-now (task #3473) per the synthesis §4 disposition, but the cluster's natural sequencing is after GR-002 lands the ComputeNode lifecycle (warm-state pool feeds and drains at ComputeNode boundaries). The task's leaf observable (engine under memory pressure surfaces Evicted+Donated entries in `EvalResult.diagnostics`) is correctly user-observable and unblocks once the dispatch boundary exists. Real dep edge wired 2026-05-13: 3473 depends_on **3420** (CN-α foundation slice). |
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
| Disposition | **fix-now → task #3474 filed** ("Stdlib shell ElasticResult: implement `result.stress = result.stress.mid` backward-compat alias"). Leaf observable: shell-solve fixture — `result.stress` and `result.stress.mid` yield identical tensor fields. Small mechanical fix; depends on GR-001 for the underlying ShellStress runtime form. Real dep edge wired 2026-05-13: 3474 depends_on **3540** (SIR-α foundation slice). |
| Discovered | 2026-05-12 architecture audit |
| Notes | Functionally gated on GR-001 (ShellStress structure-instance runtime), but the alias itself is mechanical once GR-001 lands. |

## Pending mergers from Phase 2

All clusters C-01 through C-44 promoted to GR entries during 2026-05-12 sweep.

**Umbrella PRD decomposition status (2026-05-13 reconciliation):**

- **GR-001** `structure-instance-runtime.md` — decomposed, 5 tasks 3540/3542/3544/3546/3549 + existing 3468.
- **GR-002** `compute-node-contract.md` §8 DAG — decomposed, 12 tasks 3420-3431 (ν=3431 done found_on_main; 3379/3383/3384 cancelled as superseded).
- **GR-016** `gui-event-channel-inventory.md` — decomposed, 12 tasks 3536-3552 (α=3536 done).
- **GR-020** `multi-kernel-phase-3.md` — decomposed, 17 tasks 3432-3448 (folds GR-034 task ρ=3445 + OpenVDB half of GR-003 task θ=3439).
- **GR-038** `node-traits-unification.md` — decomposed, 9 tasks 3599-3607 (ι+κ combined).

**Investigate-further tickets resolved (2026-05-13 curator pass):**

- GR-007 → tasks 3575, 3578, plus combined to existing done task 2329.
- GR-018 → tasks 3465, 3466 (originally 3579/3580; replayed 2026-05-14 post-SIGABRT).
- GR-037 → task 3581.
- GR-043 → task 3584.

Other clusters covered by fix-now task filings (`phase-3-fixnow-filing-log.md`), accept-and-document records, or specific remediation sweeps (`phase-3-grammar-fiction-triage-log.md`, `phase-3-reopen-amend-sweep-log.md`).
