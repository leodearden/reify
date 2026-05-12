# Author mode — conversational flow

A design session that ends with a committed PRD on disk. The PRD is the **output** of the conversation, not a template to fill in. The skill drives discussion through the gates; Leo brings the goal and the engineering judgment.

## Termination condition

The session is done when this question can be answered "yes":

> If I decompose and queue this PRD without further oversight, will the architecture of what gets implemented be complete, coherent, cohesive, and **good**?

If not, keep designing. No open **design** questions remain at PRD-save time. Tactical questions are fine in a `## Open questions` section. See `references/gates.md` § META for the design/tactical boundary.

## Conversational style

- Terse, technical. No preamble.
- Surface design choices as 2–4 way `AskUserQuestion` menus when the choice is genuinely independent of other context; otherwise ask inline.
- Do **not** recommend a single answer unless analysis genuinely converges; when you do recommend, label it `(Recommended)` and put it first.
- Push back if Leo's framing has an unstated assumption. Default toward asking the question rather than answering it for him.
- Keep the conversation moving — don't recite the gates as a checklist, weave them into substantive discussion.

## Flow

### Stage 0 — Frame the work

Open by establishing:
- **Goal.** What does the user want? What problem does this PRD solve? 1–3 sentences.
- **Milestone.** v0.3 / v0.3.x / v0.4 / v0.5+ / version-agnostic foundation. Drives the path: `docs/prds/<vM_N>/<slug>.md`.
- **Slug.** kebab-case filename. If unclear, propose 2–3 candidates and let Leo pick.
- **Type.** Greenfield PRD / contract resolving an existing accreted PRD (like `compute-node-contract.md` superseded `compute-node-infrastructure.md`) / extension of a shipped PRD.

If a relevant memory exists (e.g. similar past PRD authored, related decisions in fused-memory), surface it. `search(query="<topic> design decisions", project_id="reify")`.

### Stage 1 — Goal + motivating signal (drives G1)

Have Leo describe what a user observes if this PRD lands. Push for **specifics**:
- What command? What `.ri` file? What viewport state? What LSP behaviour?
- "A user can …" sentences with concrete artifacts.

This conversation seeds G1 (the consumer) and the decomposition plan's user-observable signals.

### Stage 2 — Enumerate mechanisms (drives G1, G3)

For every mechanism the PRD will introduce, capture in conversation:
1. **What it is** — value type, struct, fn, syntax surface, kernel hook, runtime entry, GUI affordance.
2. **Consumer** — which PRD or user surface consumes it. Push back on "future consumer in an unfiled PRD" — that's the failure mode G1 exists to prevent.
3. **Grammar reality check** — if the mechanism includes any novel Reify syntax in PRD prose, schedule a G3 parse run on a fixture during this stage. See `references/grammar-gate.md`. Fail-fast: rewrite the prose or queue grammar work *now*, before sinking design effort into something that won't parse.

Reify-specific patterns to surface:
- **GR-001 family.** If the PRD assumes struct-ctor runtime evaluation (`Material(...)`, `LoadCase(...)`), confirm it gates on `docs/architecture-audit/gap-register.md` GR-001 (resolution: `docs/prds/v0_3/structure-instance-runtime.md` once authored).
- **ComputeNode dispatch.** If the PRD's mechanisms route through `@optimized` or `Engine::insert_compute_node`, they consume `compute-node-contract.md` § 4 / § 5. The contract has shipped; PRDs after 2026-05-12 can rely on it.
- **`Field<X,Y>` in param position.** Tracked by task #3117. PRDs that assume this works should reference the task as a prerequisite.

### Stage 3 — Cross-PRD seams (drives G4)

Identify every other PRD this one touches. For each:
- **Direction.** Does this PRD produce something the other consumes, or consume something the other produces?
- **Mechanism.** Specific function, event, file, or trait whose implementation crosses the boundary.
- **Owner.** Which PRD's decomposition holds the integration task? Push to resolve any reciprocal "the other owns it" ambiguity *now*.

Build the `## Cross-PRD relationship` table inline. Check `docs/architecture-audit/phase-3-breadcrumb-map.md` § 3 for the three known contested-ownership pairs (PNv2↔multi-kernel, imported-field-source↔multi-kernel, topology-selectors↔PNv2); confirm this PRD doesn't introduce a fourth.

### Stage 4 — Approach choice (drives G5)

Apply the G5 heuristic from `references/gates.md`. If any of:
- Cross-crate blast radius ≥ 3
- Mechanism count ≥ ~8
- Touches load-bearing seam (FEA, ComputeNode dispatch, persistent-naming, multi-kernel, grammar/parser)
- Cross-PRD consumers ≥ 2

then prompt Leo with the B-vs-B+H choice. If B+H, the next two stages produce the contract section and boundary-test sketch — they shape the decomposition.

### Stage 5 — Contract section (if B+H)

Draft signatures + invariants for the seam. Worked exemplar: `compute-node-contract.md` § 2 (CancellationHandle), § 4 (Dispatch registry shape), § 5 (OpaqueState transfer rules), § 6 (Consumer policy).

Goal: an architect reading this section can implement the producer side correctly without further discussion. Function signatures, lifecycle rules, error semantics, ordering invariants.

### Stage 6 — Boundary-test sketch (if B+H)

Draft a table of test scenarios facing **both** sides of the seam. Worked exemplar: `compute-node-contract.md` § 7.1 (producer-side) + § 7.2 (consumer-side).

Each row:
- Scenario — one-sentence description.
- Preconditions — what state must hold.
- Postconditions — what the test asserts.

The boundary-test sketch is the integration-gate task's observable signal at decompose time (closing G2's loop).

### Stage 7 — Decomposition plan

Draft the decomposition. For every task:
1. **Title** — verb + noun, ≤ ~70 chars.
2. **Crates touched** — which workspace crates this task modifies.
3. **Observable signal** — see G2 in `references/gates.md`. Leaf tasks name a user-observable signal. Intermediate tasks name the downstream prerequisite they unlock.
4. **Prereqs** — task IDs (within this batch by letter, e.g. "α, β, γ") and out-of-batch dependencies (PRD names, existing task IDs).

Use Greek-letter or numeric labels in the PRD (α, β, γ, …; or 1, 2, 3, …); the actual task IDs are assigned at decompose time.

If B+H was chosen, the decomposition includes:
- Phase 1 — foundation supplements (small tasks that unblock the rest).
- Phase 2 — vertical slice (minimum-viable end-to-end producing the named consumer signal).
- Phase 3+ — incremental phases each adding one slice of capability.
- A **companion correction tasks phase** at the end for cross-PRD prose updates this PRD's resolution requires.

If bare B, the decomposition is a simpler linear or shallow DAG of vertical slices.

### Stage 8 — Open questions

A `## Open questions` section catches **tactical** questions explicitly deferred. Format:

```markdown
## Open questions (surfaced but not decided in this session)

1. **<question>**. <context>. **Suggested resolution:** <default if any>. Decide during <task α / impl phase / etc.>.
```

The section is allowed to be empty if no tactical questions remain.

### Stage 9 — META check

Before writing the file, **run the termination question aloud**:

> If I decompose and queue this PRD without further oversight, will the architecture of what gets implemented be complete, coherent, cohesive, and good?

If yes → save + commit.
If no → identify the design-level open questions and resolve them in conversation. Then re-ask.

The skill is allowed to fail at this stage. "Not yet good enough" is a valid outcome; close the session with an unsaved draft + a hand-off note naming what design questions remain.

### Stage 10 — Save + commit

Path: `docs/prds/<vM_N>/<slug>.md`.

Write the file. Then commit in the same skill turn:

```bash
git add docs/prds/<vM_N>/<slug>.md
git commit -m "$(cat <<'EOF'
docs(prd): <one-line goal summary>

<2–3 sentence summary of what this PRD covers + the load-bearing design decisions resolved>

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

The commit happens **before** any decompose-mode work (per `feedback_commit_prds_before_referencing_tasks` — task agents run in worktrees branched from main and need the PRD on disk).

### Stage 11 — Transition to decompose?

After committing, ask: "session has room — should I continue into decompose mode now, or stop and let you trigger decompose later (possibly in a fresh session)?"

If continuing → switch to `references/decompose-mode.md`.
If stopping → write a hand-off note summarizing what was authored and what's pending. Suggested location: paste into the session output, optionally append to a planning doc Leo names.

## PRD section template (content-matched, not literal)

A "good" PRD has these sections (names may vary; content is what matters):

1. **Title + status line** — milestone, "deferred" / "active" / "contract resolving …", date.
2. **Goal** — what user-observable behaviour ships.
3. **Background** — why this exists; architecture-doc references; prior work.
4. **Why deferred** *or* **Activation status** — what gates this on.
5. **Sketch of approach** — surface syntax + mechanism overview. (Often where novel syntax appears; G3 watches here.)
6. **Resolved design decisions** — the choices made in this session.
7. **Pre-conditions for activating** — list of upstream PRDs / tasks / grammar productions.
8. **Cross-PRD relationship** — the G4 seam-owner table.
9. **Decomposition plan** — the task DAG with observable signals.
10. **Out of scope for this PRD** — explicit exclusions; future-PRD pointers.
11. **Open questions** — tactical-only.
12. **(B+H only)** **Contract section** + **Boundary-test sketch**.

Worked exemplars:
- `docs/prds/v0_3/compute-node-contract.md` (B+H, full shape)
- `docs/prds/v0_3/structural-analysis-fea.md` (bare B, large decomposition)
- `docs/prds/v0_3/mesh-morphing.md` (bare B, smaller decomposition, strong "Relationship to other PRDs" section — G4 exemplar)
