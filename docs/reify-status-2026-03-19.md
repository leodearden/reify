# Reify — Project Status Report

**Author:** Leo (with Claude)
**Date:** 2026-03-19
**Project start:** 2026-03-13

---

## What is Reify?

Reify is a text-based domain-specific language (DSL) for engineering design — mechanical and mechatronic parts, assemblies, and eventually full systems. The name means "to make real": a `.ri` source file progressively refines an under-determined specification into a fully realized, manufacturable artifact.

The language covers parametric modeling, dimensional analysis (9-base-quantity SI system), constraint solving, geometry generation (B-rep via OpenCASCADE), and export (STEP/STL/3MF). The engine is incremental and demand-driven: edit a parameter, and only the affected subgraph re-evaluates.

**Think:** OpenSCAD's text-first philosophy meets SolidWorks' constraint-driven parametrics, with a real type system and incremental evaluation engine underneath.

**License:** AGPL-3.0-or-later

---

## What is currently implemented (M1–M4)

### Architecture

- **14-crate Rust workspace** with clean dependency layering
- **Custom incremental evaluation engine** (Salsa was evaluated and rejected — see `docs/research/salsa-fit-gap-analysis.md`)
- **Evaluation graph** with 7 node types: ValueCell, ConstraintNode, ResolutionNode, RealizationNode, ComputeNode, SchemaNode, SourceNode
- **Two-phase cycle:** elaboration (build graph from AST) → value evaluation (demand-driven pull)
- **Immutable snapshots** backed by persistent data structures (HAMT via `im-rs`)
- **Content-hash caching** (XXH3-128) with early cutoff — unchanged subtrees skip recomputation

### Milestones completed

| Milestone | Name | What it delivered |
|-----------|------|-------------------|
| **M1** | "Hello Bracket" | End-to-end pipeline: `.ri` source → parse → type-check → elaborate → evaluate → STEP file. Tree-sitter grammar, OCCT FFI, CLI (`reify check`, `reify build`, `reify eval`). |
| **M2** | "Real Engine" | HAMT-backed persistent snapshots, content-hash caching, dependency tracking (Adapton-style dynamic traces), dirty/demand cone computation, early cutoff, snapshot provenance. |
| **M3** | "Auto Resolution" | Constraint solving with three sub-solvers: NLopt (dimensional/nonlinear), SolveSpace libslvs (geometric), OR-Tools CP-SAT (logical). `auto` parameter resolution, `minimize`/`maximize` objectives, argmin solver. |
| **M4** | "Living Design" | Concurrent evaluation (Tokio work-stealing), warm-start pools (LRU, memory-budgeted), cooperative cancellation, priority scheduling (P0/P1/P3), LSP server (diagnostics, hover, completions, go-to-def), event journal. |

### Post-M4 bug-fix sweep

A code review identified 15 issues (8 critical, 6 medium, 1 low). All 52 tasks in the task tree were completed and verified — dimension handling, content hashing, OCCT thread safety, grammar edge cases, race conditions, and solver convergence.

### By the numbers

| Metric | Value |
|--------|-------|
| Rust source (hand-written) | ~36,000 lines across 14 crates |
| Tree-sitter grammar | 225 lines (JavaScript DSL → generated C parser) |
| Tests | 784 `#[test]` functions, all passing |
| Design documents | ~290 KB (language spec, architecture, implementation plan, stdlib reference) |
| Git commits | 835 |
| External solver integrations | 3 (NLopt, SolveSpace libslvs, OR-Tools CP-SAT) |
| Geometry kernel | OpenCASCADE via `cxx` FFI |

### Largest crates by size

| Crate | Lines | Role |
|-------|-------|------|
| reify-eval | 12,000 | Evaluation engine core |
| reify-runtime | 4,900 | Async scheduling, warm pools, cancellation |
| reify-lsp | 2,900 | Language server |
| reify-types | 2,800 | Type system, dimensional analysis |
| reify-kernel-occt | 2,700 | OpenCASCADE FFI |
| reify-constraints | 2,500 | Constraint orchestrator + 3 solvers |
| reify-compiler | 2,300 | Name resolution, elaboration |

---

## How long did that take?

| Date | Commits | Activity |
|------|---------|----------|
| Mar 13 | 7 | Project inception, initial design documents |
| Mar 14 | 2 | Design refinement |
| Mar 15 | 5 | Language spec finalization |
| Mar 16 | 48 | Implementation plan, orchestrator setup, M1 begins |
| Mar 17 | 282 | M1–M3 bulk implementation (orchestrator-driven TDD) |
| Mar 18 | 322 | M3–M4 completion, code review, bug-fix sweep begins |
| Mar 19 | 169 | Bug-fix sweep complete, all 52 tasks done |

**Total elapsed: 7 calendar days** (Mar 13–19), with the first 3 days focused on design and the last 4 on implementation. The implementation was heavily accelerated by a dark-factory orchestrator running up to 12 concurrent Claude Code agents in TDD workflows.

**Effective implementation time:** ~4 days of orchestrator-driven parallel development, producing M1 through M4 plus a full bug-fix sweep.

---

## What is planned

### Milestone 5: "Language Breadth" — Full language coverage

The remaining language constructs, in rough priority order:

1. **Traits and trait bounds** — compile-time conformance, multiple inheritance, diamond resolution
2. **Sub-structures with collections** — `sub vents : List<Vent>`, count constraints, positional indexing
3. **Guards** — `where` clauses, conditional declarations, structural presence/absence
4. **Connect/chain** — port compatibility, frame alignment, connector instantiation
5. **Occurrences** — process entities with in/out ports
6. **Enums and match** — C-style enums, exhaustive matching
7. **Fields** — `Field<D, C>` type, analytical/sampled/composed, differential operators
8. **Purposes** — activation/deactivation, scoped constraint injection
9. **Functions** — `fn` with type parameters, recursion, overloading, `@optimized` hooks
10. **More geometry** — sweeps, patterns, queries, edge/face selectors
11. **More constraint domains** — deeper SolveSpace/OR-Tools integration
12. **Multi-module** — `import`, module DAG, re-exports

This is the largest milestone — it's essentially "implement the rest of the language spec."

### Milestone 6: "Visual" — GUI for alpha testing

- 3D viewport (tessellated geometry from RealizationNodes)
- Property editor (read/write ValueCells)
- Constraint status panel
- Technology choice deferred: wgpu+egui vs Tauri+WebGL vs web-native (WASM)

---

## Projected timeline

### Rate of progress so far

- **Design phase:** 3 days for complete language spec (~50 pages), architecture doc (~50 pages), implementation plan, and stdlib reference
- **M1–M4 implementation:** 4 days for 36K lines of Rust, 784 tests, 3 solver integrations, OCCT FFI, LSP server, and a full bug-fix sweep
- This rate reflects orchestrator-driven parallel development (up to 12 concurrent agents)

### Estimates for remaining work

| Milestone | Scope relative to M1–M4 | Estimated duration | Rationale |
|-----------|-------------------------|-------------------|-----------|
| **M5** | ~1.5–2× M1–M4 in feature count, but builds on established crate architecture and patterns | **5–8 days** | 12 feature areas, but each is smaller than a full milestone. Traits, collections, and guards are structurally complex; the rest are more incremental. Orchestrator parallelism helps less here — features have sequential dependencies. |
| **M6** | New domain (GUI/rendering), technology evaluation needed | **3–5 days** | Minimal viable viewport + property editor. Heavily depends on technology choice — wgpu+egui is more work but better long-term; Tauri is faster to prototype. |

**Projected total to v0.1-alpha (M1–M6):** ~15–20 days from project start, or roughly **8–13 more days** from today.

### Key risks to the estimate

- **M5 feature interactions:** Traits + guards + collections interact in complex ways (e.g., conditional trait conformance, guarded collection elements). Integration testing may surface architectural gaps.
- **OCCT thread safety:** Already identified as requiring a dedicated kernel thread. More geometry operations in M5 may surface additional FFI pain.
- **Orchestrator diminishing returns:** M5 features have more sequential dependencies than M1–M4, reducing parallelism gains.
- **M6 technology evaluation:** The GUI technology decision hasn't been made. A wrong choice could cost a restart.

---

## Summary

Reify went from zero to a working incremental constraint-driven CAD engine in 7 days, with 4 milestones complete, 784 tests passing, and a clean 14-crate architecture. The remaining work (M5: language breadth, M6: GUI) is estimated at 8–13 more days. The hardest part of M5 is the interaction between traits, guards, and collections; the hardest part of M6 is the technology choice.
