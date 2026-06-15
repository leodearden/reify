# Reify PRD overlay

Project specialization for the generic `/prd` skill (`~/.claude/skills/prd/`, → `dark-factory/skills/prd/`). The generic skill reads this file at Step 0 and applies it as authoritative extensions/overrides to its gates. **This directory has no `SKILL.md` by design** — see `README.md`.

## Identity & paths

- **project_id:** `reify`
- **project_root:** `/home/leo/src/reify`
- **PRD path:** `docs/prds/<vM_N>/<slug>.md`, where `<vM_N>` is the milestone dir (`v0_3`, `v0_4`, `v0_5`); root-level `docs/prds/` for version-agnostic foundations.
- **Substrate-confirmed metadata field:** `grammar_confirmed` (bool): true iff the task's mechanism uses existing grammar, false if it queues grammar work.

## Provenance & portfolio

This skill operationalizes the **2026-05-12 architecture audit**: ~19/44 mechanism clusters fit the **incomplete/ill-formed implementation chain** pattern (memory `preferences_implementation_chain_naming`). The dominant prevention is discipline at PRD-authoring and decomposition time, before any task reaches the orchestrator.

Portfolio approaches baked in: **A** (consumer-first → G1), **D** (user-observable leaf → G2), **E** (cross-PRD seam ownership → G4), **H** (design-first / contracts / two-way boundary tests → G5), plus the grammar gate (→ G3). **C-as-integration-gate** is the task-DAG-shape the decompose mode produces (G2 escape hatch). See `preferences_implementation_chain_portfolio`. **F** (audit cadence + tracking infra) and **G** (corpus-level reviewer lint) are out of scope here.

Audit docs the skill may cite at G4 / META time:
- `docs/architecture-audit/README.md` — three-phase shape, motivation.
- `docs/architecture-audit/audit-brief.md` — failure-mode catalog (F1–F7); the "mechanism" definition (one-sentence end-to-end test).
- `docs/architecture-audit/phase-3-files-synthesis.md` — cluster table (`C-NN`); §2 Pattern 1, §5 surprises.
- `docs/architecture-audit/phase-3-scaffold-pattern-critique.md` — Type A/B/C decomposition + the seven approaches.
- `docs/architecture-audit/phase-3-breadcrumb-map.md` — §3 contested-ownership pairs.
- `docs/architecture-audit/gap-register.md` — GR-IDs cited at G4 / META.

## G1 — integration-seam catalogue + examples

**Engine-integration sub-check.** If a mechanism is an in-engine seam (kernel module, dispatcher, walk, hook, runtime trampoline), its named consumer must plug into one of the 7 in-engine seams in `docs/prds/v0_3/engine-integration-norm.md` §3:

| § | Seam |
|---|---|
| §3.1 | op-execute |
| §3.2 | realization-kind dispatch |
| §3.3 | multi-kernel dispatch |
| §3.4 | ComputeNode dispatch (per `compute-node-contract.md`) |
| §3.5 | ConstraintSolver |
| §3.6 | freshness-only walk |
| §3.7 | KernelAttributeHook |

(§3.8 OptimizedImpl is deprecated; don't cite it for new work.) A NEW seam not in the catalogue is itself a cross-PRD design question — author a norm extension first (or fold into G4). The norm prevents kernel-module-callable-in-isolation drift (cluster C-14 / GR-017). Cite the relevant §3.N as the consumer in "Sketch of approach" or "Cross-PRD relationship".

**Audit examples of the producer-orphan failure:** C-02 (ComputeNode dispatch — producer built, FEA #16 consumer pending for months), C-10 (selector_vocabulary_v2 — 22+ fns in `reify-eval`, none in the eval dispatch table), C-17 (OpenVDB ingestion — full FFI module, `reify-eval` doesn't depend on the crate), C-25 (build_doc_model — HTML formatter exists, CLI uses `render_html_stub`).

## G2 — signal vocabulary + examples

Reify user-observable signal types (extend the generic menu):
- CLI output difference (`reify check ...` emits a diagnostic; `reify <subcmd>` returns specific text).
- Viewport / GUI state change observable via debug MCP (mesh count, screenshot delta, store_state assertion).
- LSP behaviour (hover content, completion item, diagnostic emission).
- A stdlib `.ri` example that exercises the new path and runs in CI.
- A user-facing diagnostic (`E_*` / `W_*` code visible to the end user).

Policy source: `feedback_task_chain_user_observable`. **Reject** "a unit test passes against synthetic input" as a leaf signal — the C-02 example (tasks 3380/3381/3382/3385 each passed unit tests against synthetic inputs and closed cleanly; no user observed anything different). Audit examples of fake-done leaves (cluster C-07): task 2954 (screenshot_window — closed via docs-only commit), 2657 (Manifold MeshGL walk — trait wiring landed, the walk stubbed), 2967 (auto-resolve panel — frontend ready, backend event source absent), 2699 (topology selectors — `done` with `reopen_reason` listing 11 missing dispatch arms).

## G3 — substrate verifier (grammar AND semantic/behavioral)

Reify's substrate verifier has three probe vectors, all empirically grounded in `docs/prds/prd-gate-executable-substrate-verification.md §3`:

1. **Grammar premises — `tree-sitter parse --quiet <fixture.ri>`** (the grammar gate). Full mechanics, fixture-extraction heuristics, the exact command, "what counts as novel syntax", and the documented C-06 grammar-fiction precedents are in **`references/grammar-gate.md`** (`feedback_prd_grammar_gate`). Run at author Stage 2 (fail-fast); re-run at decompose Step 1.

2. **Semantic/behavioral premises — `reify check <fixture.ri>`**. Observes arg-vs-param rejection, type-name resolution, and member-access lowering. **Negative-assertion sentinel:** `reify check` exits 0 + `All constraints satisfied.` + no diagnostic where a rejection was asserted = silent-accept = FAIL (example: `revolute("not-an-axis", …)` — task 4575).

3. **Eval/IR probe (eval-error-signature)** — where `check` is insufficient. `CompiledExprKind::CrossSubGeometryRef` emission in `crates/reify-compiler/src/expr.rs` panics in `eval_expr`; authoring the scenario and running `reify eval` reveals the real IR shape via the panic signature (example: task 4358 — assumed IndexAccess, real shape betrayed by CrossSubGeometryRef panic).

**Four semantic-substrate worked examples (PRD §3/§10):**
- **4575 — arg-vs-param rejection (silent-accept):** `reify check` on `revolute("not-an-axis", …)` exits 0 + `All constraints satisfied.` + no rejection diagnostic. The negative-assertion sentinel fires — the compiler does **no** nominal arg-vs-param rejection for concrete params.
- **4577 — resolve_type_name:** `param t : Transform3` → `reify check` exits 1, diagnostic `error: unresolved type: Transform3`.
- **4437 — member-access lowering-to-ValueRef:** member access on a TypeParam-typed param → poison literal (not ValueRef); surfaces as a diagnostic at `reify check` time.
- **4358 — constraint-IR shape via eval-error proxy:** NOT reachable via `check`; `reify eval` surfaces the `CompiledExprKind::CrossSubGeometryRef` panic in `eval_expr` (`crates/reify-compiler/src/expr.rs`), betraying the real IR shape vs the assumed IndexAccess.

## Decompose mode — run the substrate-verification workflow

At decompose time, invoke the D3 verification workflow **before finalising the leaf batch**:

```
Workflow({scriptPath: "scripts/prd-decompose-verify.mjs"})
```

Per leaf the workflow runs three roles: **Enumerator** → **Prover ‖ Adversary** → **Synthesize**. The Enumerator extracts every premise the leaf signal asserts and enforces the negative-assertion mandate (every "X is rejected" must become a probe that observes the rejection actually fires). Prover and Adversary run in parallel: the Prover authors a probe per premise and runs it through α (`scripts/prd-capability-check.py`); the Adversary independently hunts unlisted premises and falsifications. Synthesize aggregates results. The deterministic harness is `scripts/prd-decompose-verify.py`.

**Blocks the batch** on any `FAIL`/`UNPROVABLE`/`HARNESS_ERROR` with captured command output attached — instead of tabulating an unexecuted promise. (`UNPROVABLE` blocks the same as `FAIL`: "no probe vector can currently observe this" is as dangerous as "the premise is false".)

The script is at `scripts/prd-decompose-verify.mjs` (committed to git — **not** `.claude/workflows/`, which is `.gitignored`), so the path is stable and D4 can re-run it at dispatch time.

## G4 — known contested-ownership pairs

From `docs/architecture-audit/phase-3-breadcrumb-map.md` §3 — three genuinely contested seams (don't introduce a fourth without resolving ownership):
1. `persistent-naming-v2 ↔ multi-kernel` — Manifold MeshGL walk / `propagate_attributes` for ManifoldKernel.
2. `imported-field-source ↔ multi-kernel` — OpenVDB dispatcher/consumer boundary.
3. `topology-selectors ↔ persistent-naming-v2` — `try_eval_topology_selector` dispatch arms.

Plus mild-contradiction: `structural-analysis-fea ↔ structural-analysis-shells` (each notes the other landed code ahead of itself). GR-IDs may be cited from `gap-register.md`.

## G5 — load-bearing seams

High-stakes seams that trigger the B+H prompt (any one is sufficient): **FEA, ComputeNode dispatch, persistent-naming, multi-kernel, grammar/parser**. Worked precedent: `compute-node-contract.md` had to be retrofitted as the H component for cluster C-02 after months of producer tasks closed without integration (`feedback_orchestrator_narrow_locks_favor_upfront_design`). Default **yes** for these seams, **no** for self-contained features (a single new diagnostic, a single stdlib helper). Approach E (G4) overlaps and is checked separately; a high-stakes PRD typically triggers both. Generic thresholds (blast radius ≥ 3 crates, mechanism count ≥ ~8, cross-PRD consumers ≥ 2) apply unchanged.

## G6 — domain: numerical

Reify is numerically heavy; G6 branches 1 (numeric bound) and 2 (closed-form exactness) fire often. Domain hazards where domain intuition is fragile: FEA numerics (P1-tet **bending lock** — slender columns can't reach tight accuracy at practical mesh density), boundary-condition mapping (pointwise Dirichlet realizes fixed-pin `k≈0.67–0.70`, not fixed-fixed `k=0.5`), spline **end-conditions** (a cubic spline reproduces a cubic only under clamped / not-a-knot, never **natural** — natural forces `M[0]=M[N]=0`), eigensolver conditioning. Worked cautionary examples (all surfaced at execution time, 2026-05-26/27, because a false premise was frozen into a RED test):
- **esc-3436-210** (`multi-kernel-phase-3.md` §8 task ε) — end-to-end capability misattributed: ε's signal demanded output its dependency set couldn't produce; the capability lived in tasks that **depend on** ε. (Branch 3.)
- **esc-3453-5/6** (`buckling-eigensolver.md` §13 task δ) — guessed 5% accuracy bound (bending lock gave 9–10%) + wrong BC mapping. "Tuned" fixture comment was aspirational. (Branches 1+2.)
- **esc-3770-1** (`trajectory-input-shaping.md` §11 task β) — asserted a natural cubic spline reproduces a general cubic to 1e-12; provably impossible. (Branch 2.)

## Capability Manifest — reify evidence forms

Mechanizes `gates.md` → *Capability Manifest — mechanizing G3 + G6 per leaf* for reify. **Manifest path:** `docs/prds/<vM_N>/<slug>.capability-manifest.md` (commit beside the PRD).

- **Empty-value sentinel (field-population check).** Reify's failure sentinel is `Value::Undef` (also `None` option-defaults and trivial constructor placeholders like the `{ ElasticResult() }` contract body). A result-field capability PASSES only if grep shows the **producer** writes a real `Value::Field{source: Sampled, …}` / non-`Undef` value on the production path (`crates/reify-eval/src/compute_targets/*.rs`, `crates/reify-eval/src/modal_ops.rs`). It FAILS (`declared-only`) if the only sampleable construction lives in a `tests/` module or a `significance_filter.rs` unit-test helper.
- **Wired-on-main evidence (anti-orphan).** Production entry paths to grep: the reify-eval dispatch tables + `engine_eval.rs` / `engine_build.rs` walks, the `@optimized`/ComputeNode registry (`compute_targets/mod.rs`), and the GUI `gui/src-tauri/src/engine.rs` `MeshData.scalar_channels`/`displaced_positions` path. A symbol present only under `tests/`, or declared but absent from the dispatch table, FAILS (`test-only`/`declared-only`) — precedents C-10 `selector_vocabulary_v2` (22+ fns, none in the eval dispatch table) and C-02 ComputeNode (producer built, consumer pending months).
- **Grammar-fixture (anti-mismatch).** Reuse the G3 grammar gate (`references/grammar-gate.md`): each novel syntax fragment is a committed `.ri` fixture that `tree-sitter parse --quiet` accepts with 0 ERROR nodes, OR names an upstream grammar-producer task (e.g. DCE `3936`). Cite the fixture path as manifest evidence.
- **Numeric floor.** The G6 domain hazards (P1-tet bending lock, Dirichlet `k≈0.67–0.70`, spline end-conditions, Duhamel `O((ΩΔt)²)`, eigensolver conditioning) are the floors; assert `bound > floor`.

**Worked precedent corpus** (the manifest's cautionary set — 2026-05-30 premise-review, report at `.orchestrator-scratch/v0_6-premise-review-report-2026-05-30.md`). Each is a binding the manifest would have FAILED *before* dispatch:
- `field-population`: esc-2962-33 (`ElasticResult.{stress,displacement}` = `Undef`), §3-C / task 3823 (`ModalResult.shape` Φ = `Undef`), task 3015 (superposition `linear_combine` over `Undef` fields).
- `producer-absent` / wrong-layer: esc-3005-32 (cache-reuse capability lives in reify-eval, not the task's reify-expr/reify-stdlib scope), esc-2929-40 (per-Support source-span provenance absent from value model + ComputeFn signature).
- `declared-only` / `test-only`: esc-3845-77 (bind/couple/prismatic are bare `eval_builtin`s, no compiler signature), esc-3607-59 (no on-disk geometry persistence; RealizationCache is in-memory per-Engine).
- grammar / substrate: esc-2998-47 (ConvergenceStatus payload enum — resolved by **gating on the DCE cluster `3946`**, which adds named-field payload variants, rather than a C-style re-spec), the C-06 grammar-fiction precedents.
- `bound≤floor`: esc-3821-44 (Duhamel `1e-9` ≪ `O((ΩΔt)²)≈2e-3` floor), esc-3453 buckling (`5%` < `9–10%` bending lock → 4066).

## Author-mode Stage 2 — Reify mechanism patterns to surface

- **GR-001 family.** If the PRD assumes struct-ctor runtime evaluation (`Material(...)`, `LoadCase(...)`), confirm it gates on `gap-register.md` GR-001 (resolution: `docs/prds/v0_3/structure-instance-runtime.md` once authored).
- **ComputeNode dispatch.** Mechanisms routing through `@optimized` or `Engine::insert_compute_node` consume `compute-node-contract.md` §4 / §5 (shipped; PRDs after 2026-05-12 can rely on it).
- **`Field<X,Y>` in param position.** Tracked by task #3117 — does not parse in param context as of 2026-05-12. PRDs assuming it work should reference the task as a prerequisite.

## Exemplars

- `docs/prds/v0_3/compute-node-contract.md` — **gold standard, B+H full shape**: §0 supersession + cross-PRD ref, §1 GR-001 link, §2–§6 contract sections (CancellationHandle, Dispatch registry, OpaqueState transfer, Consumer policy), §7 boundary-test sketch facing both ways, §8 vertical-slice DAG with per-leaf observable signals, §9 open (tactical) questions. New PRDs match it conceptually, not by literal numbering.
- `docs/prds/v0_3/structural-analysis-fea.md` — **bare B, large decomposition**.
- `docs/prds/v0_3/mesh-morphing.md` — **bare B, smaller; strong "Relationship to other PRDs"** (G4 exemplar).

## Anti-triggers (Reify-specific)

- Authoring `.ri` design files (parametric parts/assemblies) → `/reify-design`, not `/prd`.

## Memory namespace

`project_id="reify"`. Relevant slugs:
- `preferences_implementation_chain_portfolio` — the 8-approach portfolio.
- `preferences_implementation_chain_naming` — terminology.
- `feedback_task_chain_user_observable` — G2 source.
- `feedback_prd_grammar_gate` — G3 source.
- `feedback_orchestrator_narrow_locks_favor_upfront_design` — why G5 tilts toward H.
- `feedback_commit_prds_before_referencing_tasks` — author commits before decompose references.
- `feedback_planning_mode_scope` — why decompose uses planning_mode=True.
- `procedural_fused_memory_two_phase_writes` — submit_task + resolve_ticket (planning_mode=False only).
- `preferences_bookmark_task_pattern` — bookmark/deferred-batch lifecycle.
- `preferences_cross_prd_deps_real_edges` — all deps are real `add_dependency` edges.
- `procedural_set_task_status_semantics` — comma-separated bulk IDs.
- `feedback_blocked_vs_pending_semantics` — scheduler handles unmet-deps tasks.
- `feedback_trickle_ticket_submissions` — don't switch off planning_mode to paper over a closed gate.
- `project_phantom_done_metadata_files_strip_may09` — the "metadata.files missing" decompose edge case.
