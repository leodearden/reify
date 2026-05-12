# Audit Brief — input for parallel audit agents

## Why you (the audit agent) exist

Reify's PRD corpus has been authored and decomposed by many independent architects over ~3 months. Each PRD individually looks reasonable. But on 2026-05-12 a routine unblock-triage on task 3378 surfaced that the **runtime evaluation of structure constructors** (e.g. `Steel_AISI_1045()` → `Value::Map`) is silently missing — despite being assumed by at least three merged PRDs (FEA materials, multi-load-case, FEA #1-#5). The fix isn't urgent. The pattern is. We don't know how many other gaps of this shape exist.

Your job: take one PRD, enumerate every mechanism it assumes exists at runtime, classify the current state of each, and emit findings in a uniform schema so Phase 3 can synthesize across all PRDs without re-deriving them.

## What you're auditing

**One PRD.** Your supervisor will name it in your invocation. Don't follow cross-PRD references — leave breadcrumbs, don't chase.

## What you're looking for — failure-mode catalog

Every mechanism the PRD assumes falls into one of these states. Use the codes verbatim in your output.

| Code | Failure mode | Example |
|---|---|---|
| **WIRED** | Mechanism exists, fully implemented, tests cover it | Tree-sitter named-fields struct call (task 2039) |
| **PARTIAL** | Mechanism partially implemented; known gaps documented or not | `List<TraitObject>` call-site conformance (task 2227 — took 6 weeks to fully wire) |
| **TODO** | Mechanism marked with TODO/placeholder in code or PRD; not yet implemented | `Field<X,Y>` in param position (TODO(field-in-param), task #3117) |
| **FICTION** | PRD assumes mechanism exists; code provides nothing | Struct-constructor runtime evaluation (GR-001 — `Steel_AISI_1045()` → `Value::Undef`) |
| **DRIFT** | Mechanism exists but PRD describes a different shape than what landed | (none confirmed yet; flag any you find) |
| **ORPHAN** | Mechanism exists but no PRD calls for it | (informational; low priority) |

A gap (audit finding) is any mechanism that is NOT `WIRED`.

## What "mechanism" means

Anything the PRD says will exist at runtime or compile time that is not pure stdlib data. Examples:

- "`@optimized("solver::elastic_static")` on a stdlib `fn` lowers to a ComputeNode" — mechanism: @optimized annotation lowering for `fn` context
- "`solve_elastic_static(...)` evaluates as a ComputeNode" — mechanism: ComputeNode dispatch
- "Cache key derived from input hashes" — mechanism: cache-key composition
- "Warm-state attached to ComputeNode lifetime" — mechanism: OpaqueState plumbing
- "`Load` and `Support` are stdlib structs" — mechanism: structure_def declaration + runtime instantiation

Granularity rule: if you can write a one-sentence test ("does X work end to end?"), it's a mechanism. If you can't, decompose.

## Method (read this carefully)

1. **Read the PRD end-to-end.** No skimming. Note any "Sketch of approach" / "Resolved design decisions" / decomposition-task references.
2. **Enumerate mechanisms.** For every sentence describing runtime or compile-time behavior, list the mechanism. Aim for 5–25 per PRD; if you have <3, you're under-decomposing; if >30, you're over-decomposing.
3. **Classify each mechanism.** For each:
    - Search the codebase: `grep`, `find`, the existing `Explore` agent if needed
    - Search fused-memory: `mcp__fused-memory__search` with the mechanism name and adjacent terms; `include_planned=true`
    - Check tasks: `mcp__fused-memory__get_task` for any decomposition task that owns the mechanism
    - Classify against the failure-mode catalog
4. **Write findings as you go** (see Output below). Don't batch — durability matters if you crash mid-audit.
5. **Don't try to fix anything.** Don't propose architectural decisions. Don't follow cross-PRD references beyond noting them.
6. **Cite known gaps by GR-ID.** If you encounter the struct-ctor gap, write `state=FICTION evidence="GR-001"` and move on. Don't re-research it.

## Output

### Per-PRD file

Write to `docs/architecture-audit/findings/<prd-slug>.md` where `<prd-slug>` is the PRD's filename without `.md`. Use this template:

```markdown
# Audit: <PRD title>

**PRD path:** `docs/prds/.../<file>.md`
**Auditor:** audit-<prd-slug>
**Date:** 2026-05-12
**Mechanism count:** <total>
**Gap count:** <total - WIRED count>

## Top concerns

<2-4 bullet points; what would Phase 3 most want to know first about this PRD>

## Mechanisms

### M-001: <one-line mechanism name>

- **State:** WIRED | PARTIAL | TODO | FICTION | DRIFT | ORPHAN
- **Failure mode:** N/A (if WIRED) | F1..F7
- **Evidence:** file:line refs, task IDs, gap-register IDs
- **Blocks:** tasks/PRDs gated on this (if applicable)
- **Note:** one-sentence summary; longer if subtle

### M-002: ...
```

### Fused-memory writes

For every gap (state != WIRED), call `mcp__fused-memory__add_memory` with:

- `project_id`: `"reify"`
- `agent_id`: `"audit-<prd-slug>"` (your audit identity)
- `category`: `"decisions_and_rationale"` (gaps are load-bearing facts about the system, akin to decisions)
- `content`: a single line in this exact format (for grep-ability):

```
[arch-audit-gap <prd-slug>-NNN] mechanism="<name>" | prd="<slug>" | state=<STATE> | failure_mode=<F1..F7|N/A> | evidence="<refs>" | blocks="<tasks|PRDs|none>" | note="<one sentence>"
```

NNN is your local counter (`<prd-slug>-001`, `<prd-slug>-002`...). Phase 3 maps these to global GR-IDs during synthesis. Parallel agents don't collide because each has its own namespace.

Also write **one summary memory** at end:

```
[arch-audit-summary <prd-slug>] mechanisms=<n> wired=<n> partial=<n> todo=<n> fiction=<n> drift=<n> orphan=<n> top_concern="<one sentence>"
```

Both writes feed Phase 3's two synthesis paths (file-based vs memory-based) — the comparison itself is an experiment in fused-memory affordances.

## Things to take as given (do not re-research)

- **GR-001** — Structure-constructor runtime evaluation does not work. `Steel_AISI_1045()` → `Value::Undef`. Confirmed via engine_eval.rs:114-125 and tasks 3213/3240/3264. Parser side is wired (task 2039). No task tracks the eval implementation.
- **Task 3440 (done)** — Two-pass fn-signature type resolution: structure/trait names now resolve in fn signatures.
- **Task 2227 (done)** — `List<TraitObject>` / `Option<TraitObject>` / `Set<TraitObject>` / `Map<K,TraitObject>` call-site conformance checks are wired.
- Reify's trait system is **nominal, not structural** (`entity.rs:3031`). Trait conformance is via declared `: TraitName` bounds; there is no Map-shape-implements-trait mechanism.
- `Solid` resolves to `Type::Geometry` (`type_resolution.rs:513`) as a builtin alias.
- Runtime ctor naming is inconsistent: snake_case for loads (`point_load`), PascalCase for supports (`FixedSupport`).

## Boundaries

- **One PRD per agent.** If a mechanism is documented in another PRD, cite it, classify against current code state, and stop.
- **Read-only.** Don't edit code, stdlib, tests, tasks, or PRDs. Don't change task status.
- **No fixing.** Don't propose what should happen. Phase 3 does that.
- **No cross-PRD synthesis.** If you notice "this mechanism is also relevant to PRD Y", note it in a `## Cross-PRD breadcrumbs` section at the end of your file. Don't follow.
- **Skip the inventory if PRD is purely a process doc** (e.g. migration-toolchain). Note as `## Skipped: purely process` and move on.

## Termination

You're done when:
1. Every mechanism in the PRD has a row in your findings file
2. Every gap has been written to fused-memory
3. The summary memory has been written
4. You've written a 2-paragraph final message to your supervisor: gap count, top concern, anything surprising

Hard cap: 100k tokens of your own context. If you hit 80k, drop further code-spelunking and write what you have.

## Worked example

For the FEA PRD, your findings file would include:

```markdown
### M-007: `solve_elastic_static` `@optimized` registration

- **State:** FICTION
- **Failure mode:** F6 (ComputeNode infrastructure leaned on but absent)
- **Evidence:** docs/prds/v0_3/compute-node-infrastructure.md tasks P3.1-P3.6 (3379-3385); 3380/3381/3382/3385 done, 3379/3383/3384 pending; no integration with stdlib fn yet (3378 was the integration task, currently deferred); GR-001 transitively blocks (no runtime structure for inputs)
- **Blocks:** 2924 (FEA #16 engine integration)
- **Note:** ComputeNode struct + dispatch registry partly built, but stdlib `fn` → ComputeNode wiring + the input-value-to-Rust-call surface still missing.
```

And in fused-memory:

```
[arch-audit-gap structural-analysis-fea-007] mechanism="solve_elastic_static @optimized registration" | prd="structural-analysis-fea" | state=FICTION | failure_mode=F6 | evidence="tasks 3379/3383/3384 pending; GR-001 transitive" | blocks="2924" | note="ComputeNode struct partly built; stdlib fn integration absent"
```
