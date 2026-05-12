# Phase 3 — Critique of the "scaffold-without-a-caller" pattern

**Date:** 2026-05-12
**Author:** Phase-3 critique sub-agent
**Status:** investigative menu. **No recommendations.** Leo selects.
**Inputs:** `phase-3-files-synthesis.md` §1 cluster table, §2 Pattern 1, §5a; `phase-2-summary.md`; representative findings (`compute-node-infrastructure.md`, `persistent-naming-v2.md`, `reify-doc-tool.md`, `fea-gui-rendering.md`, `structural-analysis-fea.md`).

---

## 1. The pattern, restated rigorously

### 1.1 What it is

Across 19 of 44 mechanism clusters, the audit found a single repeated topology:

1. A producer module exists with first-class data structures, propagation rules, tests, and named exports.
2. A consumer site is named (in PRD prose, in another PRD, in a CLI subcommand body) but **does not call** the producer.
3. The producer's task closes on producer-internal acceptance (unit tests pass).
4. The consumer either (a) was never decomposed into a task, or (b) ships its UI half against a placeholder, or (c) routes deliberately around the producer (e.g. `render_html_stub`, `minimal_doc_model_from_compiled`, `Value::Undef` arm in `AdHocSelector`).

Concrete shape, factored:

```
  ┌──────────────┐                        ┌──────────────┐
  │  Producer    │   …no production…      │  Consumer    │
  │  (M+tests)   │ ─ ── ── ── ── ── ── ── │  (named only)│
  └──────────────┘     call edge          └──────────────┘
        │                                         ▲
        └────► unit tests (synthetic input) ──────┘
```

The unit-tests-on-the-producer arrow is real and load-bearing; the production-call arrow is the empty set. Tests verify that the producer behaves correctly *given inputs that no production path constructs*.

### 1.2 Is "scaffold without a caller" the right name?

The label has two virtues: it is vivid, and "scaffold" connotes temporary support that something else will eventually replace. But it has costs:

- **It implies the producer is in a placeholder state.** It usually isn't. `selector_vocabulary_v2.rs` (22+ functions, full tests), `propagate_attributes_via_brepalgoapi_history` (30+ test cases), `significance_filter.rs`, `KernelAttributeHook` trait surface — these are first-class. The producer side is *over-built*, not under-built.
- **It implies the missing piece is incidental wiring.** It often isn't. In C-02 the missing piece is `@optimized fn` lowering plus a dispatch registry plus a trampoline plus a cache-key/result loop plus a cancellation/pending design — five sub-mechanisms. The "wiring" framing trivializes the gap.
- **It conflates two distinct shapes** the audit revealed (see §1.3): producer-orphan and consumer-with-stub.

Candidate alternative names, each with a different emphasis:

| Candidate | Emphasis |
|---|---|
| "One-sided contract" | Both sides exist conceptually; only one was implemented. (Already in Phase 2 summary.) |
| "Stranded producer" / "stranded consumer" | Whichever side is "real" is unreachable from user code. |
| "Premature done" | The closing decision happened before the work that delivers user value was done. |
| "Missing seam owner" | No task or PRD owns the integration point where producer meets consumer. |
| "Demi-feature" | A feature with one of its two required halves. |

The audit data prefers "missing seam owner" or "demi-feature." Most clusters are not lacking a caller — they are lacking an *owner of the join*. C-25 (build_doc_model + render_html stub) is the cleanest example: producer-of-html (`render_html`) exists, consumer-of-doc-model (CLI) exists, the `build_doc_model` lowering that connects them does not — and no task owns it. The trail of TODO comments (`TODO(post-2361)…`) marks the seam, but no task ID picks it up.

### 1.3 What it is NOT

- **Not "incomplete implementation."** The producer is usually complete by every internal acceptance criterion. The completeness assertion is *scoped to the producer's contract surface*.
- **Not "tech debt."** Tech debt is code that works but is awkward. This pattern is code that doesn't work, masked by tests that show it could work in principle.
- **Not a uniform pattern.** §1.2 sub-shapes:
  - **Type A: producer-orphan.** Producer's only callers are tests. (C-02 ComputeNode, C-10 selector v2, C-43 drain_events, C-17 OpenVDB, C-19 mid-surface, C-32 long-chain diagnostic, C-39 Manifold MeshGL hook, C-29 to_global.)
  - **Type B: consumer-with-stub.** Consumer exists and is invoked by users; it uses a deliberate placeholder rather than the real producer. (C-25 build_doc_model + render_html_stub; C-22 stress alias in shell ElasticResult; C-44 `result.stress` alias; the AdHocSelector `Value::Undef` arm; `engine.rs:949-950` always-empty `scalar_channels`/`displaced_positions`.)
  - **Type C: producer-and-consumer-both-built, never bridged.** (C-13 GUI events: frontend subscribes, no Tauri emitter; C-14 mesh-morph/shell-extract: engine never depends on them; C-36 NodeTraits vs NodePolicyOverrides: two taxonomies coexist.)

Type A is "we built the engine; no car." Type B is "we built the car; under the hood is a wooden engine." Type C is "we built the engine and the car; nobody bolted them together." Different mitigations for each.

### 1.4 The structural seam

Phase 2 called the pattern "one-sided contract." The contract in question is almost always between two PRDs (or between a PRD and the user surface). The seam where production calls fail to happen is the seam where **PRD authorship boundaries cross.** Section 4 returns to this.

---

## 2. Critique from multiple angles

### 2.1 Decomposition incentives

Producer-first decomposition is the path of least resistance for at least five reasons:

1. **Producers have local acceptance criteria.** "Unit tests on the propagation function pass" is a stable contract you can verify in one worktree, with one CI run, without standing up the rest of the system.
2. **Producers don't need other PRDs to land.** Persistent-naming-v2 could decompose the attribute table + propagation + resolver library without waiting for the surface DSL grammar (M-022 AdHocSelector) or the kernels (M-009/M-010 Booleans/fillet/chamfer) to be ready. Consumer-first decomposition would have stalled in week 1.
3. **Producers are easier to write specs for.** "Takes inputs, returns outputs" is well-defined. "Where the user observes this" requires answering: what does the user type? what do they see? where in the stack does that signal land? — questions that span the codebase.
4. **The orchestrator DAG topology rewards completable leaves.** A task whose closure depends on another not-yet-existing task can't be planned, can't TDD, can't merge. Producer tasks are leaves; consumer-side wiring tasks tend to be fan-in nodes.
5. **The dark-factory TDD loop rewards green tests.** A producer with green unit tests passes the gate. A consumer-side integration test that *would* fail because the producer isn't dispatched cannot exist as a green TDD step. So agents naturally write the producer side and a synthetic-input test, then close.

C-02 (ComputeNode) is the cleanest example: tasks 3380/3381/3382/3385 each delivered a load-bearing primitive and each closed cleanly. 3383/3379/3384 — the actual dispatch + lifecycle — are the tasks that need a *consumer-shaped* answer and they remain pending. The producer-first ordering was correct; the gap is that nothing forced 3383/3379/3384 to land before 2924 (FEA #16) was treated as a future-pending dependency rather than a *blocker*.

### 2.2 Test culture

Two distinct cost gradients here:

**(a) Unit tests on the producer are cheaper than integration tests across the seam.**
- A unit test of `propagate_attributes_via_brepalgoapi_history` can construct the input data inline. An integration test of "user writes `@face("top")` in `.ri`; result resolves correctly" requires the parser, type checker, evaluator, kernel, attribute table, resolver, and dispatch arm — most of which are in different crates.
- Crate boundaries amplify this: many seams are between `reify-eval` and `reify-kernel-occt`/`reify-kernel-manifold`/`reify-shell-extract`, which have circular or near-circular constraints. Integration tests live in the workspace `tests/` directory, not per-crate, and don't benefit from per-crate ownership.

**(b) Tests have a strong selection bias toward what was easy to write.**
- See `cost_per_byte_does_not_alter_lru_eviction_order` (C-42): a test deliberately *pins* the drift. Closing the gap means deleting a test. This is rare but illustrative.
- See the MITC3 vs MITC3+ band-widening (C-20): a benchmark passes by widening the acceptance band by 21–2200× to span both shipped and PRD-promised behavior. The test exists; it pins the wrong contract.
- See `gui/src/__tests__/feaModeStore.test.ts`: the feaModeStore is fully tested *under synthetic scalar_channels inputs* because `engine.rs:949` always emits an empty map. The frontend toolbar is covered; nothing tests "real FEA result lights up the toolbar."

The "new Pattern A" from §3 of the file synthesis — active drift pin — is the test-culture failure mode in its strongest form: the test *contractualizes* the gap.

### 2.3 PRD shape

Sampled PRD-vs-code prose mismatches:

- **reify-doc-tool.md** (24 mechanisms, 17 gaps): the PRD describes the data model and rendering rules in detail (DocModel schema, ItemDoc/PortDoc/ParamDoc field lists, alphabetical meta ordering, annotation rendering behavior). The consumer — `reify doc <file>` — is described in two lines (`reify doc examples/integration_full_v01.ri --format markdown produces useful output`). The detailed-producer/sketchy-consumer asymmetry is *visible in the word count*.
- **persistent-naming-v2.md**: extensive prose on the data model (TopologyAttribute, Role enum, FeatureId derivation, ModEntry threading, propagation algorithm). The user-facing payoff (`@face("top")` → handle resolution) gets one paragraph. The PRD specifies the producer in full; the consumer is "selector resolution becomes attribute lookup," handwaving past the AdHocSelector engine evaluator (M-022) that is the *actual user surface*.
- **compute-node-infrastructure.md**: precisely specifies P3.1–P3.6 producer phases. The consumer (`@optimized fn` lowering in `engine_eval.rs`) is referenced as "the trampoline" in P3.4 and lumped together with the dispatch registry in one task.
- **structural-analysis-fea.md**: 28 mechanisms. The signature `solve_elastic_static(...)` appears in the §"Sketch of approach" as a literal call site. There is no separate mechanism for "this fn declaration exists in stdlib." It is presupposed by the PRD's *prose* but never decomposed into a task. M-001 records this as FICTION — the user-visible surface is *assumed* by the audit, *named* by the PRD prose, and *unbuilt* in the stdlib.

PRD acceptance criteria are predominantly written against producer-side observable state (data shape, propagation correctness, cache-key composition) rather than user-observable state (running `reify` on a file produces X). When user-observable criteria do appear ("produces useful output," "renders in a browser"), they are written loosely enough that a stub satisfies them.

### 2.4 Risk and reward asymmetry

- **At producer-task close:** tests pass, CI green, reviewer signs off, value captured. The task closes with locally-correct work.
- **At consumer-side absence:** no signal. The work that would prove the integration broken hasn't been written. Audits surface the gap weeks later.
- **Who bears each side?** The producer-side agent / reviewer / user. The consumer-side gap is borne by the next architect who writes a downstream PRD assuming the producer is usable, and discovers (via audit, via a confused new contributor, via an end-user bug) that it isn't.

This is the **temporal asymmetry**: value capture is immediate; cost manifestation is deferred. The cost compounds with each downstream PRD that assumes the producer works.

A specific consequence: PRDs cite other PRDs' producers as "this exists" — the FEA PRD cites ComputeNode infrastructure; the multi-load-case PRD cites FEA; the buckling PRD cites shells. The "cycle" §5c surfaced is a chain of consumers each assuming an upstream producer is usable. If any link is a stranded producer, every downstream PRD plans against fiction.

### 2.5 Agent / human attention boundaries

The worktree-bounded TDD loop has structural properties that shape this failure mode:

- **The worktree boundary is per-task.** An agent in worktree `task/3382` sees the world from inside `reify-eval`. Building dependency edges + freshness walk + dirty propagation in isolation is natural. *Verifying that some future ComputeNode producer will exercise this path correctly* requires looking outside the worktree, which the agent has no ergonomic affordance for.
- **The `metadata.files` list is the unit of edit.** An agent works within its declared file scope. C-25's gap (CLI uses stub) is invisible to an agent working in `reify-doc/src/fmt_html.rs` because `reify-cli/src/main.rs` is not in its `files` list. The bridging file change *is the seam* and is owned by neither side.
- **Worktree branches rebase against `main`, not against other in-flight task branches.** Two tasks working on the producer-and-consumer pair would race; the orchestrator serializes via merge queue, which makes serialized completion easier than coupled-pair completion.
- **The post-task review cycle is local to the task.** Reviewers examine the worktree's diff against `main`. They do not run a global "are there callers of this new function?" check. The follow-up triage step (escalation watcher / unblock skill) is reactive, not preventative.

There is no orchestrator-level invariant that says "this PR introduces a new public function with no production caller — flag it." The closest existing facility — clippy `dead_code` — only fires when the symbol has *no* callers, not when it has *only test* callers. The pattern is invisible to lint.

### 2.6 Architectural "two-side" PRDs — the structural enabler

Almost every cluster in Pattern 1 has the producer and consumer in **different PRDs**:

- C-02: producer = compute-node-infrastructure PRD; consumer = structural-analysis-fea (task 2924) + persistent-fea-cache + warm-state-eviction + multi-load-case-fea + …
- C-10: producer = persistent-naming-v2 (selector_vocabulary_v2.rs); consumer would be a DSL/stdlib PRD that doesn't exist.
- C-17: producer = imported-field-source-hdf5-csv (OpenVDB FFI); consumer = imported-field-source (elaborate_field arm).
- C-25: producer = reify-doc-tool (HTML formatter); consumer = same PRD (CLI), but the *bridging lowering function* — `build_doc_model` — is itself unowned.

The decomposition-across-PRDs is itself the structural enabler. When a single PRD owns both halves, the seam tends to be addressed (e.g. parts of compute-node-infrastructure: P3.1 producer + P3.2 consumer-of-cache-key both landed). When the seam crosses PRDs, neither PRD's "definition of done" requires the seam to be live.

This is sharpened by the dependency ordering. Many PRDs were drafted before their producers were specified. The FEA PRD predates compute-node-infrastructure as a separate PRD; the shells PRD predates FEA-integration; the v0.5 composite/buckling/varying-thickness PRDs all *cite* unfinished v0.4 work as "additive on." So the consumer-side PRDs encode dependencies on producer-side PRDs at *authorship time*, but neither side has a hook to detect "the producer never finished its consumer-facing seam."

### 2.7 Definition of done

The single most informative quote from the audit:

> "task 2652 done — but 'done' here means the library function exists, not that it's wired into surface evaluation." (persistent-naming-v2 M-013)

And the recurrent variation in different PRDs:

> "task 2657 done because the *trait wiring* is done; the actual MeshGL/originalID walk is not implemented" (persistent-naming-v2 M-018)
> "task 2954 marked done via a docs-only commit" (fea-gui-rendering M-001)
> "task 2967 done; bridge.ts:580 self-documents: 'The backend event source is wired in a later task'" (fea-gui-rendering M-015)
> "task 2699 carries reopen_reason listing 11 missing dispatch arms — task is done in metadata anyway" (persistent-naming-v2 M-020)
> "task #215 (propagation to compiled output) marked status=done in fused-memory yet the field doesn't exist" (reify-doc-tool M-006)

The pattern is: **the task's scope is what was implemented, not what was assumed.** Done-ness is a property of the worktree's diff, not of user-observable behavior.

Five of these are noted as reconciler-driven false positives (Phase 3 §5e) — `found_on_main` flips, sibling-merge convergence — but at least as many are honest closures by agents who completed their declared scope. The "done" semantics are *correct relative to the task's metadata.files-bounded contract*; they are *wrong relative to the PRD's user-visible payoff*.

### 2.8 Time-decay

The 15+ "tasks marked done while wiring absent" cluster (C-07) is partly a sample-size artifact. Phase 2 inventoried 40 PRDs and surfaced ~15 specific cases. Two relevant time-decay effects:

- **Re-opening week-old tasks is harder than re-opening day-old tasks.** Memory recall, worktree availability (the orchestrator's auto-reaper culls done branches), reviewer freshness, and the spawning of dependent tasks all make later re-open more expensive. The longer a stranded producer sits, the more downstream tasks accumulate against it, the harder unwinding becomes.
- **Accumulation rate matters.** If five new C-07-shape closures land per week, and the audit catches them with two-week lag, the deficit grows. The audit is a one-shot snapshot; absent ongoing audit cadence, the gap re-accumulates.

A scheduled cross-PRD audit (proposed in §3) is a *response* to time-decay, not a fix for it. Slowing the accumulation rate requires changes at the closure decision.

---

## 3. Menu of approaches

Six approaches, ordered roughly by where in the pipeline they intervene (PRD-authoring time → task-closing time → after-the-fact audit).

### 3.1 Approach A: Consumer-first PRD section

**Mechanism.** PRD-authoring template adds a required `## Consumer / user-observable surface` section *before* the §"Sketch of approach" section. Section must name: (i) the file path of the named consumer call site, (ii) the user-observable signal (CLI output, viewport contents, IDE behavior), (iii) a smoke-test path that demonstrates the producer working end-to-end. If the consumer doesn't exist yet, the PRD is *blocked* on the PRD that owns the consumer.

**Cost.** Light template change; ongoing tax is the discipline at PRD authoring. PRD-decomposition meetings get longer because the consumer-side conversation happens.

**Risk.** PRDs become harder to draft when the consumer is genuinely unknown (research-shaped PRDs). PRDs may degenerate to fictional consumers ("the CLI will eventually call this") if the authoring discipline is weak. Doesn't catch consumers that exist-but-stub (Type B).

**Where it bites first.** PRDs like compute-node-infrastructure (no consumer named at authoring time), reify-doc-tool (CLI named, but `build_doc_model` not in the consumer path). Doesn't help GUI-event-channel-style PRDs where the consumer is a frontend subscriber that doesn't share repo authorship.

### 3.2 Approach B: Vertical-slice decomposition (first-task = end-to-end thinnest possible)

**Mechanism.** PRD-decomposition rule: the *first* task in any PRD's task tree must demonstrate one real user-observable signal flowing through one real production code path, even if narrow and unoptimized. No other tasks may close until task #1 has a green smoke test. The producer-side build-out happens in subsequent tasks that *replace* the thin first-task implementation, not in tasks that build alongside it.

**Cost.** High. Requires re-thinking the decomposition habit. Some PRDs (compute-node-infrastructure, persistent-naming-v2) genuinely have foundational producer work that doesn't fit a vertical slice; the first slice would be necessarily synthetic / minimal. Probably extends planning time substantially.

**Risk.** Forces premature design decisions to get a slice through. If the slice is brittle, all subsequent tasks dance around it. Can pressure-test the *wrong* assumption (slice succeeds for one degenerate case, fails for the general one). May force PRDs to bundle work that should be separate.

**Where it bites first.** All the engine-wiring-absent PRDs (C-14). All the producer-orphan clusters. Hardest to apply to compiler-internals PRDs (where "user-observable" means "this DSL syntax now does X"). The smoke-test-for-DSL pattern is the cheapest variant: each PRD ships an `examples/<prd-slug>.ri` that exercises the new mechanism, and CI runs it.

### 3.3 Approach C: Producer-consumer pair (neither closes alone)

**Mechanism.** When a task creates a new public function or trait, a sibling task in the same PRD or a named other PRD must consume it from production code. Both tasks share a dependency relationship: neither closes until both pass their tests. The orchestrator's merge gate refuses to land either alone.

**Cost.** Substantial orchestrator change — current merge gate is per-task. Coordinating two worktrees adds operational complexity. The "sibling task" annotation has to exist on every producer task — that's labor at PRD time.

**Risk.** Forces serialization that the current per-task model doesn't have. May deadlock when the consumer's PRD is "blocked on producer" and the producer's PRD is "blocked on consumer" — the audit's §5c cluster cycle illustrates the real shape of this. Doesn't address cases where producer and consumer cross repository boundaries (Tauri-side emitter vs. SolidJS subscriber).

**Where it bites first.** Tightly-coupled pairs like compute-node-dispatch (M-014/M-015 in compute-node-infrastructure) + first ComputeNode user (`@optimized fn solve_elastic_static` in FEA). Doesn't help Type-B (consumer-with-stub) because the consumer already closed.

### 3.4 Approach D: Definition-of-done-as-observably-true (Leo's 2026-05-12 policy)

**Mechanism.** Every leaf task names, at decomposition time, a *user-observable signal* that proves completion. The task cannot close until the signal is demonstrable. The signal is one of: a CLI output difference, a viewport screenshot delta, an LSP hover content change, an editor diagnostic emission, a stdlib `.ri` example that exercises the new path and that runs in CI.

**Cost.** Per-task overhead at decomposition time. Some signal-naming will be hard (compiler-internal mechanisms: "the audit doc says X" is a weak signal). Repository CI needs an "exemplar example" runner that runs `.ri` files end-to-end against changes — this exists in part but isn't comprehensive.

**Risk.** Forces synthetic signals where none meaningfully exist ("a unit test passes" devolves to gameable). May still close producer-only when the named signal is producer-internal ("the propagator's unit test passes" reframed as user-observable). The signal-naming discipline only works if reviewers refuse to accept producer-only signals.

**Pressure-test against the audit data:** if this policy had been in force at the time of:
- task 2657 closure (persistent-naming-v2 Manifold MeshGL stub): would have needed a user-observable Manifold-path attribute preservation signal → would have surfaced the stub.
- task 2954 closure (screenshot_window): would have needed a real screenshot output → docs-only commit would not have satisfied.
- task 215 closure (CompiledModule.doc field): would have needed `reify doc` to render a doc string from compiled output → would have failed.
- task 2699 closure (topology-selectors): would have needed at least one of the 11 selectors to evaluate end-to-end → would have failed.
- task 3380/3381/3382/3385 closure (ComputeNode P3.1-P3.6): each task's user-observable signal is *the next phase's existence*. The unit-test signal is genuinely the only signal until P3.4 lands. *This policy alone does not catch this case* — it requires Approach A or B as a complement.

**Where it bites first.** Type B (consumer-with-stub) cleanly. Type A (producer-orphan) only when the signal is forced to be a real consumer; less when synthetic signals are accepted.

### 3.5 Approach E: Cross-PRD seam ownership

**Mechanism.** At PRD-resolve time (the meeting where a PRD is approved for decomposition), every PRD declares: (i) which producers it consumes (with PRD-ID references), (ii) which consumers it expects (with PRD-ID references or names). When PRD A says "consumed by PRD B" and PRD B says "produces via PRD A," the seam — the specific function/event/file — gets an *explicit owner task*. Bookkeeping artifact: a `seams.md` registry that lists every cross-PRD seam, its owning task, its current state.

**Cost.** Bookkeeping. Requires that PRD-resolve become aware of producer/consumer pairs, which means PRDs must be drafted with cross-PRD awareness — adding work to PRD-authoring. The seam registry needs to stay synced with task state.

**Risk.** Doesn't catch first-time PRDs where the consumer is genuinely unknown. Doesn't catch the C-04 / C-25 case where the consumer is *within the same PRD* but the bridging function is unowned. Bookkeeping rot: if the registry decays, it adds friction without value.

**Where it bites first.** All "producer in PRD X, consumer in PRD Y" pairs (C-02 with FEA, C-17 with elaborate_field, C-14 with engine wiring). Doesn't help the C-10 / C-25 within-PRD cases.

### 3.6 Approach F: Scheduled audit cadence

**Mechanism.** Run the Phase-2 audit shape every N weeks (e.g. every release, every quarter, every Nth-merge). One agent per PRD; deltas against last audit go into a rolling gap register. Tasks for newly-detected gaps get filed as part of the audit closing.

**Cost.** Each audit pass is the cost of a Phase 2 run — non-trivial agent-hours, supervisor attention, sometimes a Phase-3-style synthesis. The 2026-05-12 run took multiple sessions across 40 agents.

**Risk.** Reactive. Catches accreted gaps but doesn't prevent them. False sense of safety between audits. Audit results need acting on or the register becomes noise.

**Where it bites first.** Catches *new* C-07-shape closures (tasks marked done while wiring absent) within a bounded time window. Catches time-decay (§2.8). Doesn't reduce the rate of new gaps.

### 3.7 Optional: Approach G — Lint-level "production caller" gate

**Mechanism.** A workspace-level CI check that, for every new public function introduced in a PR, verifies that at least one *non-test* caller exists in the same workspace or has a tracked-in-issues consumer task with a planned merge date. Functions added without a planned consumer fail the check.

**Cost.** Build out the static analysis. Symbols introduced as part of foundation work would need explicit annotation (`#[allow(no_production_caller, reason = "Phase 3 of compute-node-infrastructure")]`). Annotation discipline.

**Risk.** Heavy false positives (every new utility function flagged). Easy to game with annotations. Doesn't catch the trait-method-with-stub-impl shape (C-39 Manifold MeshGL hook). Doesn't catch Type B at all (the consumer exists; it just uses a stub).

**Where it bites first.** Type A (producer-orphan). Doesn't help Type B / C.

### 3.8 Approach matrix (which approaches help which sub-shapes)

| Approach | Type A: producer-orphan | Type B: consumer-with-stub | Type C: both-built-not-bridged | Grammar fictions (C-06) | Active drift pin (§3 New Pattern A) |
|---|---|---|---|---|---|
| A (Consumer-first PRD section) | strong | medium | strong | strong | weak |
| B (Vertical-slice decomposition) | strong | strong | strong | strong | medium |
| C (Producer-consumer pair) | strong | weak | medium | medium | weak |
| D (Done = observably true) | medium | strong | strong | strong | strong (forces test removal/replacement) |
| E (Cross-PRD seam ownership) | strong (for cross-PRD only) | weak | strong | medium | weak |
| F (Scheduled audit cadence) | reactive | reactive | reactive | reactive | reactive |
| G (Lint production-caller) | medium | none | none | none | none |

The diagonal pattern says: no single approach covers all sub-shapes. A combination is required.

---

## 4. Interaction with the dark-factory orchestrator topology

### 4.1 Where does the failure mode originate?

The orchestrator runs TDD per task in a worktree. Tasks land sequentially through the merge queue. Each worktree is a complete checkout that branches from `main`.

The orchestrator is *neutral* on the scaffold-without-a-caller pattern in two ways:

- **It doesn't introduce it.** Tasks closed cleanly relative to their declared scope. Producers exist with real tests. The orchestrator did its job.
- **It doesn't prevent it.** Per-task acceptance ≠ system-level acceptance. There is no orchestrator invariant for "this PR introduces a public function that has no production caller" — the task closure gate is per-task-scope.

But the orchestrator *amplifies* the pattern in three subtle ways:

1. **Per-task worktree → per-task attention.** The TDD agent sees one task. The "is the producer being called?" question requires looking outside the worktree, which is not a TDD-loop affordance.
2. **`metadata.files` constrains scope.** An agent doesn't add a caller to a file outside their declared file list. The bridging change crosses files; nobody owns the bridging change.
3. **Done-flag closure is a one-shot decision.** Once a task is `done`, it leaves the active queue. Re-opening it (to fix a stranded-producer gap) requires the unblock skill / orchestrator manual intervention. The 2026-05-12 audit found four reconciler-flip false positives (Phase 3 §5e); these are cases where the closure-decision was wrong but stuck.

### 4.2 Would orchestrator-level changes help?

The orchestrator's task-acceptance criteria are well-tuned for *per-task correctness* and *merge-time integration risk*. They are not designed for *cross-task architectural coherence*. Hooking the latter at the orchestrator level would require:

- A "consumer named in this task's metadata exists and is invoked" check — requires PRD-time annotation of the consumer.
- A "no symbols introduced without a production caller" check — would conflict with foundation-style tasks (compute-node P3.1 deliberately ships ComputeNodeData with only test callers).
- A "definition of done references a user-observable signal" check — requires Approach D's policy *before* the orchestrator sees the task.

In all three cases, **the orchestrator's check depends on annotations that must be present at task-creation time.** Which means **the intervention point is PRD decomposition, not task acceptance.**

The orchestrator can be the *enforcer* of a PRD-decomposition policy. It cannot author the policy.

### 4.3 Specifically: would Approach D land at orchestrator or PRD time?

Approach D names a user-observable signal per task. The naming happens at task-decomposition (PRD time). The verification happens at task-close (orchestrator time, in the reviewer step). So D is a *coupled* PRD+orchestrator change: PRD decomposition gains a required field; orchestrator's reviewer agent gains a check ("does the named signal hold?").

If the signal field is missing → orchestrator rejects the task plan.
If the signal-hold check is missing → reviewer flags it.

The PRD-time change is necessary; the orchestrator-time change is enforcement.

### 4.4 What about pure orchestrator changes (no PRD-time policy shift)?

A pure orchestrator change that might help:

- **Steward post-close audit:** for every closed task, run a quick check that "all public functions introduced have at least one non-test caller in the workspace." Failures become escalations rather than blockers. This is Approach G implemented at steward time rather than CI time.

The cost is moderate; the value is bounded by the false-positive rate of "no production caller" — see §3.7's risk note.

---

## 5. Open questions for Leo

Five questions where the menu can't substitute for Leo's judgment:

1. **What is the intended scope of "done"?** Today, "done" means the worktree's diff satisfies the task's declared metadata. The audit data argues for at least one of two alternatives: (a) "done means a named user-observable signal is demonstrable," (b) "done means all introduced symbols have production callers." These are *different*. (a) is approach D; (b) is approach G. They have very different ergonomics for foundation-style tasks. Which scope of "done" is the right contract for Reify, given the orchestrator's per-task-worktree topology and the trade-off between premature decomposition and post-hoc audit cost?

2. **Should the orchestrator refuse to close producer-only tasks, or should the PRD-time decomposition prevent them from existing?** The choice between "fix at the gate" (orchestrator) and "fix at the source" (PRD) determines whether the bookkeeping cost is borne by the orchestrator's reviewer step or by the PRD-resolve discussion. The audit data argues that the producer-only tasks were *correctly decomposed for their declared scope* — the gap is in the PRD's declared scope. So PRD-time may be cheaper. But PRD-time interventions require human attention at authoring; orchestrator-time interventions can be partly automated.

3. **What about the cross-PRD-cycle case (§5c)?** When PRD A names PRD B as its consumer and PRD B names PRD A as its producer, neither side can land "first." The audit found this for the FEA stack: every shells/buckling/multi-load-case/morphing PRD names FEA #16 (2924) as a dependency; 2924 names compute-node-infrastructure P3.4 (3383); 3383 names ComputeNode consumers as the rationale for its design. Some choice is needed about whether to ship a "thin" first slice that breaks the cycle, or to designate a lead PRD that ships first with explicit deferred-consumer status. This is an architectural decision about cycle-breaking, not a policy choice.

4. **Time-budget for retroactive cleanup vs. preventative discipline.** The audit surfaced 19 clusters with stranded-producer-or-consumer. Each needs disposition. The menu (Approaches A–G) addresses *future* prevention; the *existing* clusters need disposition decisions per Phase 3's `Disposition Candidates` table. If the budget for retroactive cleanup is constrained, several clusters (C-15, C-21, C-22, C-30, C-33, C-36) are in the "investigate-further" bucket — meaning they need a Leo decision before any task can be filed. Which gets prioritized: closing the 19 existing gaps, or preventing the next 19?

5. **Should "scaffold without a caller" be reframed?** The label has rallied attention but smuggles a stance (the producer is the problem). The audit data is more ambiguous — half the cases (Type B / C) are not "producer is the problem" but "the bridging function or event-emitter is unowned." A reframing toward "missing seam owner" or "unowned bridging integration" might change which approaches feel natural. Is the name worth changing?

---

## End of critique

(Supervisor message follows in the chat reply, not in this file.)
