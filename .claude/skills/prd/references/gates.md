# Gates — G1, G2, G3, G4, G5, G6, META

Each gate's section names:
- **What it catches** — the audit-data class of failure it prevents.
- **Application** — the exact algorithm to apply during author or decompose mode.
- **Level** — `block` / `prompt` / `prompt with heuristic`.

The audit cluster references (`C-NN`) point into `docs/architecture-audit/phase-3-files-synthesis.md` §1.

---

## G1 — Consumer named

**Level:** **block** (both modes; checked at PRD save and re-checked at decompose).

**What it catches.** Type A producer-orphan and large parts of Pattern 1 of the audit. ~19/44 mechanism clusters had producers built without a named consumer. Examples:
- C-02 (ComputeNode dispatch) — producer fully built, FEA #16 consumer task remained pending for months.
- C-10 (selector_vocabulary_v2) — 22+ functions in `reify-eval`; none in the eval dispatch table.
- C-17 (OpenVDB ingestion) — full FFI ingest module; `reify-eval` doesn't depend on the crate.
- C-25 (build_doc_model) — HTML formatter exists; CLI uses `render_html_stub`.

**Application.** For every mechanism the PRD introduces — value type, struct, fn, syntax surface, kernel hook, runtime entry, GUI affordance — the PRD must name **at least one consumer**:
1. A specific other PRD by slug (`docs/prds/.../foo.md`), AND/OR
2. A specific user-observable surface (CLI command, viewport behaviour, LSP feature, IDE diagnostic, stdlib `.ri` example).

A "mechanism" follows the audit-brief definition: if you can write a one-sentence test ("does X work end to end?"), it's a mechanism.

If no consumer can be named today, the PRD is incomplete by construction. Two valid resolutions:
- **(a)** Defer the producer work until the consumer-side PRD exists. Mark this PRD blocked-on-consumer in `Pre-conditions for activating`.
- **(b)** Author the consumer-side PRD first (or as a paired commit), then return.

Do **not** accept "future consumer in an unfiled PRD" as a named consumer. That's the failure mode the gate exists to prevent.

**Engine-integration sub-check.** If the mechanism is an in-engine seam (kernel module, dispatcher, walk, hook, runtime trampoline), the named consumer must plug into one of the 7 in-engine seams catalogued in `docs/prds/v0_3/engine-integration-norm.md` §3:

| § | Seam |
|---|---|
| §3.1 | op-execute |
| §3.2 | realization-kind dispatch |
| §3.3 | multi-kernel dispatch |
| §3.4 | ComputeNode dispatch (per `compute-node-contract.md`) |
| §3.5 | ConstraintSolver |
| §3.6 | freshness-only walk |
| §3.7 | KernelAttributeHook |

(§3.8 OptimizedImpl is deprecated; don't cite it for new work.)

If the mechanism is a NEW seam not in the catalogue, that itself is the cross-PRD design question — author a norm extension first (or fold into G4). The norm exists to prevent kernel-module-callable-in-isolation drift (cluster C-14 / GR-017): producer surfaces that ship without any engine caller. Cite the relevant §3.N as the consumer in the PRD's "Sketch of approach" or "Cross-PRD relationship" section.

**In author mode:** Conversational. Walk the PRD's introduced mechanisms one by one, ask Leo to name the consumer for each, push back on fictional / future consumers.

**In decompose mode:** Re-check by reading the saved PRD. If a mechanism appears without a named consumer, escalate before queueing tasks.

---

## G2 — User-observable leaf

**Level:** **block** (decompose only; author-mode informs the decomposition plan but the hard check is at decompose time).

**What it catches.** Pattern 3 of the audit. 15+ tasks marked done with load-bearing wiring absent. Cluster C-07 catalogues the cases. Examples:
- task 2954 (screenshot_window) — closed via a docs-only commit.
- task 2657 (Manifold MeshGL walk) — trait wiring landed, the actual walk stubbed.
- task 2967 (auto-resolve panel) — frontend ready, backend event source absent (self-documenting bridge.ts comment).
- task 2699 (topology selectors) — task `done` with `reopen_reason` listing 11 missing dispatch arms.

The audit memory `feedback_task_chain_user_observable` codifies the policy: every leaf task names a user-observable signal proving completion.

**Application.** For each task in the decomposition:
1. Classify: **leaf** (no other task in this batch depends on it) or **intermediate** (other batch tasks consume its output).
2. **Leaf tasks must declare a user-observable signal**, one of:
   - CLI output difference (`reify check ...` emits a diagnostic; `reify <subcmd>` returns specific text).
   - Viewport / GUI state change observable via debug MCP (mesh count, screenshot delta, store_state assertion).
   - LSP behaviour (hover content, completion item, diagnostic emission).
   - A stdlib `.ri` example that exercises the new path and runs in CI.
   - A user-facing diagnostic (`E_*` / `W_*` code visible to the end user).
3. **Intermediate tasks must declare which downstream prerequisites they unlock** — the consumer task ID or task title. Producer-only intermediate tasks with no named downstream consumer are not acceptable.
4. The signal becomes the task's `user_observable_signal` metadata field at filing.

If a leaf task's only "signal" is "a unit test passes against synthetic input", reject. That's the failure shape the gate exists to prevent (the C-02 example: tasks 3380/3381/3382/3385 each passed unit tests against synthetic inputs and closed cleanly; no user observed anything different).

**Escape hatch.** Foundation-style tasks that genuinely cannot demonstrate a user-observable signal in isolation (e.g. ComputeNode P3.1 alone) are acceptable IFF they are roped into a paired integration-gate task within the same batch — the integration-gate task is the leaf, the foundation tasks are intermediates that unlock it. This is the **C-as-integration-gate** pattern from `preferences_implementation_chain_portfolio`.

---

## G3 — Grammar verified

**Level:** **block** (both modes).

**What it catches.** Cluster C-06 of the audit. 13/40 PRDs assume parser features that don't exist. Examples flagged by Phase 3: `= auto` literal, `subject to`, `@shell(thickness = linear_taper(...))` Expr annotation arg, `schema = { x: Length(mm) }` block, decl-level `match`, `chain` body, `forall ... : <body>`, `sub name : Type { body }`, `for ... in ...` comprehension, `#[allow(shadowing)]` bracket form, `RegularGrid1` struct ctor.

The audit memory `feedback_prd_grammar_gate` codifies the policy.

**Application.** See `references/grammar-gate.md` for the exact procedure. Summary:
1. Enumerate every Reify-syntax fragment in the PRD prose.
2. For each fragment, extract a small `.ri` fixture and run `tree-sitter parse --quiet <fixture>` from `tree-sitter-reify/`.
3. Exit 0 = pass. Exit 1 (CST contains `(ERROR ...)` nodes) = fail. Ambiguous extraction = ask Leo.
4. Every failing fixture must be resolved before save/queue. Two valid resolutions:
   - **(a)** Rewrite the PRD prose to use existing grammar. Re-run the parse on the rewritten fragment.
   - **(b)** Queue grammar work as an explicit prerequisite task in the decomposition plan. The PRD's `Pre-conditions for activating` section names the grammar prereq.

Don't accept "the grammar will exist by the time this PRD activates" as a resolution unless the grammar work is filed and tracked as a hard prerequisite task in the DAG.

---

## G4 — Cross-PRD seam ownership

**Level:** **prompt** (both modes).

**What it catches.** §3 contested-ownership pairs from `docs/architecture-audit/phase-3-breadcrumb-map.md`. The audit identified three genuinely contested seams (each PRD claims the other owns the integration):
1. `persistent-naming-v2 ↔ multi-kernel` — Manifold MeshGL walk / `propagate_attributes` for ManifoldKernel.
2. `imported-field-source ↔ multi-kernel` — OpenVDB dispatcher/consumer boundary.
3. `topology-selectors ↔ persistent-naming-v2` — `try_eval_topology_selector` dispatch arms.

Plus mild-contradiction cases like `structural-analysis-fea ↔ structural-analysis-shells` (each notes the other landed code ahead of itself).

**Application.** For every cross-PRD reference in the PRD:
1. Identify the mechanism the seam owns (the function, event, file, or trait whose implementation crosses the boundary).
2. Ask Leo: **which PRD owns the seam?**
3. The named owner gets the integration task in its decomposition. The other PRD references the seam-owner task as a dependency.
4. **Detect reciprocal ambiguity.** If the cross-PRD-section reads "X is owned by the other PRD" while the other PRD's section reads the same back, that's the C-13/C-19-shape failure. Surface it. Leo picks an owner; the other PRD gets updated in a paired commit OR a follow-up edit task.

Bookkeeping artifact: every saved PRD has a `## Cross-PRD relationship` (or equivalent) section with a table:

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `path/to/other.md` | consumes / produces | `Foo::bar()` / `EvalEvent::baz` | this-prd OR other-prd | wired / queued / blocked |

In author mode the skill drafts this table from conversation; in decompose mode the skill re-checks it before queueing tasks.

**Conditional adoption.** G4 only fires for PRDs that touch existing PRD territory or have load-bearing cross-PRD dependencies. Standalone foundational PRDs (rare; e.g. some v0.1 grammar PRDs) may not need the section — note "no cross-PRD seams" inline.

---

## G5 — Design-first when stakes are high (approach H)

**Level:** **prompt with heuristic** (author only; informational at decompose).

**What it catches.** The audit's structural argument (`preferences_implementation_chain_portfolio` + `feedback_orchestrator_narrow_locks_favor_upfront_design`): under the dark-factory orchestrator's narrow-file-lock model, integration-step tasks that span crates get starved or never get queued. Approach B (vertical slice) is fine for architecturally-simple features; approach **B + H** (contract document + interface tests + two-way boundary tests) is required for high-stakes / architecturally-complex features. The audit found `compute-node-contract.md` had to be retrofitted as the H component for cluster C-02 after months of producer tasks closed without integration.

**Heuristic.** A PRD needs **B + H** rather than bare B when **any of**:

- **Cross-crate blast radius ≥ 3 crates touched.** Crate count from the decomposition plan's `Crates touched` notes (or a reasonable estimate at author time).
- **Mechanism count ≥ ~8.** A coarse signal that the PRD is too large to vertical-slice without a contract.
- **High stakes.** Touches a load-bearing seam (FEA, ComputeNode dispatch, persistent-naming, multi-kernel, grammar/parser).
- **Cross-PRD consumers ≥ 2.** Multiple downstream PRDs assume this one's output; per audit §2.6 those are where the integration step gets starved.

When any condition holds, prompt Leo: "This PRD looks B+H-shaped. Should we add a §contract section (signatures of the seam + invariants) and a §boundary-test sketch (cross-crate scenarios facing both producer and consumer sides) before saving? Otherwise we accept the risk that integration tasks starve at medium priority under the narrow-lock orchestrator."

Default answer is **yes for high-stakes seams**, **no for self-contained features** (e.g. a single new diagnostic, a single stdlib helper). Leo can override.

**What B + H adds to the PRD.**
1. A **contract section** — function signatures of the seam, invariants, ordering rules, error semantics. Worked example: `compute-node-contract.md` §2–§6.
2. A **boundary-test sketch** — table of scenarios with preconditions + postconditions, facing both the producer crate and the consumer crate. Worked example: `compute-node-contract.md` §7 (7.1 producer-side / 7.2 consumer-side).
3. The decomposition plan's integration-gate task names the boundary-test sketch as its observable signal — closing the loop into G2.

**Approach E interaction.** G5 says "design-first when stakes are high". Approach E (cross-PRD seam ownership) overlaps but is checked separately as G4. A high-stakes PRD typically triggers both.

---

## G6 — Premise validity

**Level:** **block** (both modes; checked at PRD save and re-checked at decompose).

**What it catches.** A failure class orthogonal to G1–G5: an observable / leaf signal whose **substantive quantitative premise** is false, unreachable, or misattributed. G1–G5 validate the *structure* of the implementation chain — a consumer exists, the signal is user-observable, the grammar parses, the seam has an owner. G6 validates the *truth* of the claim embedded in the signal. A signal can pass every other gate and still assert something impossible. Three caught-at-execution-time examples (2026-05-26/27):

- **esc-3436-210** (`multi-kernel-phase-3.md` §8 task ε) — leaf signal demanded an end-to-end "BRep→Mesh intermediate + Manifold-Boolean output" observable on ε, but ε's dependency set (δ = the `Convert` capability *descriptor* only, α = the `produced_repr` field) cannot produce it; the Manifold execute arm (ζ) and OpenVDB consumer (η) both **depend on** ε. The signal belonged on a downstream leaf.
- **esc-3453-5/6** (`buckling-eigensolver.md` §13 task δ) — RED tests baked a 5% accuracy bound and a "fixed-fixed ⇒ k=0.5" BC mapping. P1-tet cannot reach 5% at practical mesh density for slender columns (bending lock; L/r≈138 gave 9–10% on every variant), and pointwise Dirichlet BCs realize fixed-pin (k≈0.67–0.70), not fixed-fixed. The fixture's "Tuned" comment was aspirational — the tests never went green.
- **esc-3770-1** (`trajectory-input-shaping.md` §11 task β) — step-1 RED asserted a **natural** cubic spline reproduces a general cubic off-knot to 1e-12. Provably impossible: natural BC forces `M[0]=M[N]=0` ⟹ `p''(endpoints)=0` ⟹ the reproduced polynomial is degree ≤ 1.

In all three the false premise was frozen into a RED test (or end-to-end signal) and surfaced only when an agent tried — and provably couldn't — to GREEN it, costing an escalation, an architect/steward cycle, and sometimes a planner-tier amendment.

**Application.** For every observable / leaf signal in the decomposition plan, classify its assertion and apply the matching check. Most signals — "emits diagnostic `E_*`", "compile test", "screenshot delta" — assert no quantitative premise and pass trivially.

1. **Numeric bound / threshold** ("within X%", "≤ ε", "≥ N dB", "to M digits"). Cite an *achievability basis*:
   - an existing validated test / reference that already hits that accuracy on a comparable problem, OR
   - a back-of-envelope error estimate for the method at the planned resolution (element order × mesh density, iteration count, conditioning), OR
   - a reference computation.
   If none exists, the bound is a **guess** — either set it to a defensible value, or mark it provisional and file a calibration task. **Reject bare guessed thresholds.** A fixture comment claiming "Tuned" is not a basis.

2. **Closed-form exactness / reproduction** ("exact within 1e-12", "reproduces P(t) exactly", "round-trips losslessly"). State the **mathematical identity** that makes it true, then confirm the asserted **configuration** satisfies it. Exactness is almost always configuration-dependent (boundary condition, element order, end conditions, basis degree) — name the configuration that earns it. (Natural vs. clamped cubic spline is the worked example: a cubic spline reproduces a cubic only under clamped / not-a-knot end conditions, never natural.)

3. **End-to-end capability** ("produces a Mesh", "evaluates to `Value::X`", "the union renders"). Trace every capability the signal requires to the task's **dependency set**: each must be delivered by this task or one of its **prerequisites** — never by a task that **depends on** this one. If a required capability is owned by a downstream task, the signal belongs on that downstream leaf (the **C-as-integration-gate** pattern from G2), not here.

**Resolution when a premise fails** (any one):
- **(a)** Move the signal to the task that can actually produce it (fixes misattribution).
- **(b)** Weaken the assertion to what's achievable now, and file a follow-up task for the stronger property.
- **(c)** Change the asserted configuration (BC, element order, basis) so the claim becomes true.

**In author mode:** as the decomposition plan takes shape, walk each leaf's drafted signal through the trichotomy. Substantive premises are where domain intuition is most fragile (FEA numerics, spline math, multi-kernel capability availability) — exactly what the structural gates cannot see.

**In decompose mode:** re-check against the saved PRD before queueing. If a signal's premise cannot be substantiated, escalate before filing tasks (cheap) rather than letting an implementer discover it against a RED test (expensive).

---

## META — "is this PRD good?"

**Level:** **block** (author mode only; the final check before saving).

**What it catches.** The substantive quality check Leo named in the design session: a structural-headers-all-present PRD can still be incomplete if it leaves load-bearing design questions undecided.

**Application.** Before writing the PRD to disk, ask:

> If I decompose and queue this PRD without further oversight, will the architecture of what gets implemented be complete, coherent, cohesive, and **good**?

If not, identify the open **design** questions and resolve them inline with Leo. Tactical/implementation-time open questions (e.g. "should the no-trampoline policy be hard-error or body-inline in debug builds?" — `compute-node-contract.md` §9.1) are acceptable and go in `## Open questions`. Design-level open questions (e.g. "should we use nominal or structural trait conformance?", "where does cancellation live?", "what's the producer's API shape?") are not — push to resolve them in the session.

The boundary between "design" and "tactical":
- **Design** — if a downstream architect could choose differently and arrive at an architecturally inferior result.
- **Tactical** — if the choice is local, recoverable, and an architect could pick either and the system would still be coherent.

When unsure, ask Leo: "is this design-level or tactical?" Default toward design-level (resolve now).

---

## Gate-application order (author mode)

Walk in this rough order in conversation. Iterate freely as discussion surfaces new mechanisms:

1. **G1 first.** Establish who consumes this; otherwise the rest is exercise.
2. **G3 second.** If novel syntax fails to parse, drop / queue it before designing further.
3. **G4 third.** Identify cross-PRD seams; resolve ownership before writing relationship table.
4. **G5 fourth.** Decide B vs B+H; if H, draft contract + boundary-test sketch now (they shape the decomposition).
5. **G2 stays in the decomposition plan** — author-mode draft of the decomposition names observable signals per task even though the hard check is at decompose time.
6. **G6 alongside the G2 draft.** Validate each drafted leaf signal's substantive premise — numeric bound has an achievability basis, exactness names the configuration that earns it, end-to-end capability traces to the task's dependency set.
7. **META last.** Final sanity check before save.

## Gate-application order (decompose mode)

1. **G1, G3, G4 re-check** against the saved PRD (fast; mostly looking for drift between author and decompose).
2. **G2 walk** — enumerate every task in the proposed DAG, classify leaf/intermediate, attach `user_observable_signal` / `consumer_ref`.
3. **G6 re-check** — for each leaf signal, validate its substantive premise (numeric basis / exactness identity / dependency-set trace). If one can't be substantiated, escalate before filing — cheaper than an implementer discovering it against a RED test.
4. **G5 informational** — note whether the PRD declared B or B+H; if B+H, verify the integration-gate task exists in the DAG and points at the boundary-test sketch.
5. File the batch (see `references/decompose-mode.md`).
