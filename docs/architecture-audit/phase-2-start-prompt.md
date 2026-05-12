# Phase 2 — start prompt

Copy the content below into a fresh Claude Code session in this repo. It is self-contained: the supervisor reads the audit brief, then dispatches one parallel audit agent per PRD in batches.

---

## Prompt to paste

```
I'm starting Phase 2 of an architecture audit. Phase 1 deliverables are in
docs/architecture-audit/. Read these first:

  1. docs/architecture-audit/README.md           (3-phase shape, why)
  2. docs/architecture-audit/audit-brief.md      (method, output format, failure-mode catalog)
  3. docs/architecture-audit/gap-register.md     (seeded with GR-001)

Then dispatch parallel audit agents — one per PRD — in the batches below.
Use the Agent tool with subagent_type="general-purpose" and the per-agent
prompt template at the bottom. Send each batch as a single message with
multiple parallel Agent tool calls. Wait for each batch to finish before
starting the next.

## Batches

### Batch A — v0.3 (8 PRDs, highest priority)

  docs/prds/v0_3/compute-node-infrastructure.md
  docs/prds/v0_3/structural-analysis-fea.md
  docs/prds/v0_3/multi-load-case-fea.md
  docs/prds/v0_3/persistent-fea-cache.md
  docs/prds/v0_3/mesh-morphing.md
  docs/prds/v0_3/hex-wedge-meshing.md
  docs/prds/v0_3/fea-gui-rendering.md
  docs/prds/v0_3/imported-field-source-hdf5-csv.md

### Batch B — v0.4 + v0.5 (6 PRDs, adjacent)

  docs/prds/v0_4/structural-analysis-shells.md
  docs/prds/v0_4/fea-gui-rendering-shells.md
  docs/prds/v0_4/a-posteriori-error-estimation.md
  docs/prds/v0_5/composite-laminated-shells.md
  docs/prds/v0_5/varying-thickness-shells.md
  docs/prds/v0_5/structural-stability-buckling.md

### Batch C — v0.2 (7 PRDs, shipped; spot-check for similar accretions)

  docs/prds/v0_2/multi-kernel.md
  docs/prds/v0_2/persistent-naming-v2.md
  docs/prds/v0_2/per-purpose-tolerance.md
  docs/prds/v0_2/imported-field-source.md
  docs/prds/v0_2/kinematic-constraints.md
  docs/prds/v0_2/auto-resolution-backtracking.md
  docs/prds/v0_2/migration-toolchain.md

### Batch D — top-level cross-cutting (19 PRDs)

  docs/prds/auto-type-param-resolution.md
  docs/prds/deep-dot-chain.md
  docs/prds/field-source-kinds.md
  docs/prds/forall-statement-form.md
  docs/prds/freshness-4-variant.md
  docs/prds/geometry-traits.md
  docs/prds/kinematic-constraints.md
  docs/prds/kleene-logic.md
  docs/prds/match-block-decls.md
  docs/prds/money-dimension.md
  docs/prds/node-trait-composition.md
  docs/prds/pragmas.md
  docs/prds/reify-doc-tool.md
  docs/prds/shadowing-warning.md
  docs/prds/solver-hint-payloads.md
  docs/prds/specialization-scope.md
  docs/prds/stdlib-trait-breadth.md
  docs/prds/topology-selectors.md
  docs/prds/warm-state-eviction.md

  (Batch D may be split into D1 + D2 of ~10 each if your tool harness
  prefers ≤10 parallel agents per message.)

## After all batches

Write a one-paragraph "Phase 2 complete" note in
docs/architecture-audit/phase-2-summary.md with:

  - Total agents dispatched
  - Total findings files written (count of files in findings/)
  - Total fused-memory gap entries (search `[arch-audit-gap`)
  - Top 3 PRDs with highest gap counts
  - Any agent that failed to produce a findings file (will need re-dispatch)

DO NOT attempt synthesis. Phase 3 is interactive with Leo.
Stop after the summary note is written.

## Per-agent prompt template

For each PRD <prd-path>, dispatch one Agent with this prompt
(substitute <prd-path> and <prd-slug> per agent):

  ─── BEGIN PER-AGENT PROMPT ─────────────────────────────────────────

  You are an architecture audit agent. Read these three files in order
  before starting:

    1. docs/architecture-audit/README.md
    2. docs/architecture-audit/audit-brief.md
    3. docs/architecture-audit/gap-register.md

  Then audit exactly one PRD:

    PRD path: <prd-path>
    Your audit identity: audit-<prd-slug>

  Follow the audit-brief Method section verbatim. Write findings to:

    docs/architecture-audit/findings/<prd-slug>.md

  And stream gap entries to fused-memory using add_memory with
  agent_id="audit-<prd-slug>". Format per audit-brief §"Fused-memory
  writes".

  Hard cap 100k tokens. Stay strictly within one PRD. Don't fix
  anything. Don't propose decisions. End with a 2-paragraph final
  message to your supervisor: mechanism count, gap count, top concern,
  anything surprising.

  ─── END PER-AGENT PROMPT ───────────────────────────────────────────
```

---

## Notes for the supervisor running Phase 2

- **Batch size:** Most Claude Code harnesses comfortably parallel-dispatch 6–10 Agent calls per message. If your harness rate-limits or context-pressures, reduce batch size and dispatch sequentially within a batch.
- **Failure recovery:** If an agent crashes or returns no findings file, re-dispatch with the same per-agent prompt. The audit brief is designed for restart-safety: agents write durable artifacts as they go.
- **Cost:** Each agent does substantial code-spelunking + memory writes; expect 30–80k tokens per audit. ~40 PRDs × ~50k average = 2M+ tokens total. Leo has explicitly accepted this cost ("worth >>10x the tokens spent fixing mis-architected tasks").
- **Don't combine with Phase 3.** When you finish Phase 2, stop. Phase 3 is interactive.

## After Phase 2 completes — what Phase 3 will need

- The 40-ish findings files in `docs/architecture-audit/findings/`
- The fused-memory gap stream (search `[arch-audit-gap`)
- The Phase 2 summary note (`phase-2-summary.md`)
- The gap register (`gap-register.md`) — Phase 3 will append to it
- This audit brief (read-only, for reference)

Phase 3 will compare the file-derived synthesis against the memory-derived synthesis — an explicit experiment on fused-memory's affordances for cross-agent knowledge consolidation.
