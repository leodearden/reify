# PRD Brief — P2: Selector substrate convergence (unify `SelectorKind`, retire `FeatureTagTable`, resolve `LeafQuery::Named`)

> **Brief for a `/prd` author session** (not a finished PRD). Read `./00-findings.md` FIRST.
> A **foundation cleanup** PRD. Two of its three threads are independent (Wave 1, concurrent with
> P0/P1); the **third (`LeafQuery::Named` fate) is gated on P0's region-reference decision** — keep
> it as a dep-wired sub-thread, do not pre-decide it.
>
> **Do NOT touch task 3523 or esc-3523-75/76** — `LeafQuery::Named` is the substrate the `/unblock
> 3523` leaf-predicate work interacts with; coordinate, don't collide. Today is 2026-06-24.
> Line numbers accurate at time of writing — G3-verify against current `main`.

## Why this PRD exists

The selector machinery is split-brained (findings §5): two `SelectorKind` enums, a write-only-dead
v0.1 attribute table, and a no-op named-resolution stub. This debt makes every future selector
change touch divergent paths. Converge the substrate so P3/P4 build on one coherent base.

## Scope / deliverables (three threads)

### Thread A — Unify the two `SelectorKind` enums *(independent)*
- `reify_core::ty::SelectorKind` = {Face,Edge,Body,Vertex} (`crates/reify-core/src/ty.rs:39`) vs
  `reify_ir::expr::SelectorKind` = {Face,Point,Edge} (`crates/reify-ir/src/expr.rs:16`). Same name,
  different membership, different consumers (function-call family vs `@` family).
- Reconcile into one enum (or one canonical + an explicitly-derived view). Resolve the
  `Point`-vs-`Vertex` and `Body` membership mismatch. NB: P0 may move topology nouns behind a
  kernel-capability boundary — coordinate the *location* with P0, but the de-duplication itself is
  independent.

### Thread B — Retire the write-only-dead `FeatureTagTable` *(independent)*
- v0.1 `FeatureTagTable` (`crates/reify-ir/src/geometry.rs:3578`/`3593`) is **written** in
  production (`crates/reify-eval/src/engine_build.rs:6169`) but its only reader
  `resolve_unique_by_tag` has **zero production callers** (test-only). v0.2 `TopologyAttributeTable`
  (`geometry.rs:3938`) supersedes it (docstring "mirrors the FeatureTagTable shape", `:3934`).
- Delete it, OR fold its still-useful per-op `step_kind`/`source_span` (diagnostics) into
  `TopologyAttribute`. Remove the dead write path + the test-only reader. Net production behavior
  change: none (it's dead) — confirm via the dependents analysis.

### Thread C — Resolve `LeafQuery::Named` *(gated on P0)*
- `LeafQuery::Named` (`crates/reify-ir/src/value.rs:464`) resolves to empty + a `TopologyTagStale`
  warning in the first-class resolver (`crates/reify-eval/src/topology_selectors.rs:1501-1517`), so
  `face(b,"name")` as a first-class selector silently returns nothing. Meanwhile `@face("name")`
  resolves via a *separate* post-process path. **Two divergent name paths, one a no-op.**
- Per **P0's** decision: either (i) wire `Named` to attribute lookup (unifying it with the `@face`
  path), or (ii) remove it (if the converged model expresses names differently). Do not pre-decide;
  dep-wire this thread on P0.

## Design questions to resolve

- Thread A: which crate owns the unified `SelectorKind` (turns on P0's layer decision)? Is the
  `@`-family `Point` a `Vertex`, or a distinct coordinate-frame concept that shouldn't be a
  `SelectorKind` at all?
- Thread B: delete vs fold — is `step_kind`/`source_span` worth migrating, or reconstructable?
- Thread C: deferred to P0.

## Key code pointers (verify against current main)

- Two `SelectorKind`s: `crates/reify-core/src/ty.rs:39`; `crates/reify-ir/src/expr.rs:16`.
- `FeatureTagTable`: `crates/reify-ir/src/geometry.rs:3578`/`3593`; written `engine_build.rs:6169`;
  reader `resolve_unique_by_tag` (test-only) `crates/reify-eval/src/topology_selectors.rs:2227+`.
- `TopologyAttributeTable`: `crates/reify-ir/src/geometry.rs:3938` (`:3934` docstring).
- `LeafQuery::Named`: `crates/reify-ir/src/value.rs:464`; no-op resolver arm
  `crates/reify-eval/src/topology_selectors.rs:1501-1517`.

## Out of scope

- The region-reference model / Named's *semantic* fate → decided by **P0** (this PRD implements it).
- `FeatureId`/`Feature` typing → **P1**. Provenance/`feature()` surface → **P3**.

## Dependencies

- **Upstream:** P0 — Thread C only.
- **Downstream:** P3/P4 build on the converged substrate.
- Threads A & B can land before P0; Thread C waits on P0.

## SOP reminders

- Commit the PRD before tasks. Tight `metadata.files` or `[]` (never a directory). Cite
  `./00-findings.md`. Coordinate Thread C timing with the `/unblock 3523` session (shared
  `LeafQuery::Named` substrate).
