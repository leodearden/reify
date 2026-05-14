# Architecture Audit (2026-05-12)

Triggered by the discovery during task 3378 unblock-triage that **structure-constructor runtime evaluation is silently missing** despite being assumed by multiple merged PRDs (FEA materials, multi-load-case, and others). The pattern — independent architects accreting decisions that each look reasonable but don't compose at runtime — is suspected to have produced other latent gaps. (Task 3378 has since been cancelled-as-superseded by task 3426; see `phase-3-eight-dag-filing-log.md`.)

This audit inventories the gaps **across the v0.1–v0.5 PRD corpus** so Leo can make scope-shaping decisions before any further consumer work lands.

## Files

| File | Purpose |
|---|---|
| [`audit-brief.md`](audit-brief.md) | Input for audit agents: failure-mode catalog, method, output schema |
| [`gap-register.md`](gap-register.md) | Master gap list. Seeded with GR-001. Phase 3 merges per-PRD findings here |
| [`findings/`](findings/) | One file per audited PRD; agents write here |
| [`phase-2-start-prompt.md`](phase-2-start-prompt.md) | Self-contained prompt to launch Phase 2 in a fresh session |

## Three-phase shape

1. **Phase 1 — Scoping (this commit).** Audit brief authored, gap register seeded, downstream tasks deferred. Done.
2. **Phase 2 — Parallel audit.** One agent per PRD; each writes a findings file AND streams gap entries to fused-memory under `agent_id="audit-<prd-slug>"`. Phase 3 will compare the file-based and memory-based syntheses — illuminating about fused-memory affordances.
3. **Phase 3 — Synthesis + decisions.** Interactive with Leo. Merge findings into the master gap register; group by mechanism; decide per-mechanism between (a) PRD-shape work, (b) accept-and-document, (c) pick-an-existing-pattern, (d) investigate-further.

## Not in scope

- Fixing anything. The audit is read-only.
- Cross-PRD synthesis (Phase 3's job — leave breadcrumbs, don't follow).
- Proposing architectural decisions (Phase 3's job).
- Re-researching gaps already in the gap-register (cite by GR-ID).
