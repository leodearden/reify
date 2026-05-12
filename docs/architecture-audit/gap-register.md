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

## Pending mergers from Phase 2

(Phase 2 agents wrote to `findings/<prd-slug>.md` + fused-memory under `agent_id="audit-<prd-slug>"`. Phase 3 promotes each gap entry here, dedup'ing where the same mechanism surfaces from multiple PRDs. C-01 → GR-001 resolved. C-02 → GR-002 resolved. Remaining clusters C-03..C-44 await separate Phase-3-register sessions; see `phase-3-files-synthesis.md` §1 for the cluster table and §4 for candidate dispositions.)
