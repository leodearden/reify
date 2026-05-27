# Phase 2 — completion summary

**Run date:** 2026-05-12
**Supervisor:** claude-interactive (single session, four sequential batches)

## Headline numbers

- **Agents dispatched:** 40 (one per PRD)
- **Findings files written:** 40 (all in `findings/`; one per PRD; full coverage)
- **Fused-memory gap memories streamed:** ~380 `[arch-audit-gap ...]` entries plus 40 `[arch-audit-summary ...]` entries (per per-agent reports; raw lines stored as Graphiti episode bodies under `agent_id="audit-<prd-slug>"`, `category="decisions_and_rationale"`)
- **Failed agents needing re-dispatch:** 0

The single agent that wrote no gap memories — `audit-migration-toolchain` — did so deliberately per the audit-brief's "purely process" carve-out, and wrote a one-line summary memory recording the skip rationale. Not a failure.

## Per-batch shape

| Batch | PRDs | Agents | Approx gap memories |
|---|---|---|---|
| A (v0.3) | 8 | 8 | ~112 |
| B (v0.4 + v0.5) | 6 | 6 | ~85 |
| C (v0.2) | 7 | 7 | ~55 |
| D1 (top-level half 1) | 10 | 10 | ~55 |
| D2 (top-level half 2) | 9 | 9 | ~73 |
| **Total** | **40** | **40** | **~380** |

Gap memory counts are per-agent self-reports; Phase 3 will get authoritative counts by reading the findings files and searching fused-memory directly.

## Top PRDs by gap count

| Rank | PRD slug | Gaps | Mechanism count |
|---|---|---|---|
| 1 | `structural-analysis-fea` | 19 | 28 |
| 2 | `a-posteriori-error-estimation` | 17 | 18 |
| 2 | `imported-field-source-hdf5-csv` | 17 | 18 |
| 2 | `reify-doc-tool` | 17 | 24 |
| 5 | `structural-analysis-shells` | 16 | 25 |
| 5 | `multi-load-case-fea` | 16 | 18 |

Three-way tie at #2. Five of the top six are FEA / FEA-adjacent PRDs in v0.3/v0.4.

## Recurring patterns surfaced

(Phase 3 will synthesize properly. The following are recurring shapes called out by ≥3 agents in their "top concern" / "surprising" sections — flagged here so Phase 3 doesn't miss them.)

- **"Scaffold without a caller" / one-sided contract** (compute-node-infrastructure, persistent-fea-cache, multi-kernel, persistent-naming-v2, node-trait-composition, warm-state-eviction, solver-hint-payloads, specialization-scope, match-block-decls, fea-gui-rendering, auto-type-param-resolution, topology-selectors). Repeating shape: producer-side infrastructure builds out beautifully with tests, but no production call site invokes it. The same failure topology as GR-001, recurring across most domain PRDs.
- **Grammar-level fictions** (multi-load-case-fea `subject to`, varying-thickness-shells `@shell(thickness = linear_taper(...))`, imported-field-source-hdf5-csv `schema = { x: Length(mm), ... }`, specialization-scope `sub name : Type { body }`, match-block-decls decl-level `match`, deep-dot-chain method-call AST). PRDs assume grammar shapes that don't exist. Worth a Phase-3 category distinct from runtime FICTION. *(Note 2026-05-27: `= auto` at the param-default position was always parseable via `auto_keyword` and has been removed from this list; broader binding-site coverage is being addressed by `docs/prds/auto-binding-site-positions.md`, α task 3802 landed, β–ε queued.)*
- **Tasks marked `done` while the load-bearing wiring is absent** (fea-gui-rendering task 2954, persistent-naming-v2 tasks 250/2652/2657/2658/2699, stdlib-trait-breadth audit-doc, node-trait-composition 2358, several auto-type-param-resolution tasks). Task-status accounting is systematically optimistic relative to runtime reachability.
- **GR-001 transitive blast radius**: confirmed by 7+ audits as a load-bearing blocker (FEA, multi-load-case, kinematic-constraints-toplevel, varying-thickness-shells, composite-laminated-shells, structural-stability-buckling, field-source-kinds). One audit explicitly noted GR-001 does NOT transitively block them (mesh-morphing) — useful negative datapoint.
- **PRD/spec/code three-way drift** on diagnostic codes and trait/type naming surfaces (freshness-4-variant `Failed`/`error`; deep-dot-chain `W_DEEP_DOT_CHAIN`; pragmas `#kernel` accepted-but-inert; geometry-traits `inferred_traits` field). Documentation-layer rot.
- **Bare-MITC3 vs MITC3+ DRIFT** (structural-analysis-shells M-005): a benchmark suite whose pass-bands were widened to span both the shipped behaviour and the PRD-promised behaviour. Worth a Phase-3 discussion of "the test exists but pins the wrong contract" as a sub-pattern.

## Non-results to feed Phase 3

- Every batch produced a findings file for every PRD in scope. No re-dispatch needed.
- Several agents flagged cross-PRD breadcrumbs they did not chase (per audit-brief boundaries). These accumulate to a meaningful cross-cutting backlog — Phase 3 should harvest the `## Cross-PRD breadcrumbs` sections at the bottom of each findings file.
- The fused-memory write path is async (Graphiti queue). Phase 3 should expect some entries to still be flushing through reconciliation when synthesis begins; if a memory search by `agent_id="audit-<slug>"` comes up short, retry after a few minutes.

## What Phase 3 inherits

- `docs/architecture-audit/findings/*.md` — 40 files, one per PRD
- Fused-memory: ~380 `[arch-audit-gap ...]` plus 40 `[arch-audit-summary ...]` memories under `agent_id="audit-<prd-slug>"`
- `docs/architecture-audit/gap-register.md` — seeded with GR-001; awaits Phase 3 promotions
- `docs/architecture-audit/audit-brief.md` — reference for failure-mode codes

Phase 3 is interactive with Leo and is explicitly out of scope for this Phase-2 supervisor session.
