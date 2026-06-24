# PRD Brief — P0: Geometry region-reference model & kernel-topology layer boundary

> **Brief for a `/prd` author session** (not a finished PRD). Read `./00-findings.md` FIRST —
> it is the evidence base; do not re-derive it. This is the **keystone** of the naming-convergence
> program: P3 and P4 depend on the decisions you make here. Design-first (B+H) — this is the
> highest-stakes seam in the program (multi-kernel + core type system + grammar/parser).
>
> **Do NOT touch task 3523 or esc-3523-75/76.** Convert any relative dates to absolute (today is
> 2026-06-24). Line numbers below are accurate at time of writing — your G3 substrate check
> verifies them against current `main`.

## Why this PRD exists

Reify claims representation-independence (`docs/reify-implementation-architecture.md` §1.1/§10.2)
but exposes B-rep topology nouns in the core type system and surface language, and has **four
disconnected naming namespaces** with no shared resolver (findings §4, §5). The flagship
`@face("top")` is non-functional on every kernel. Before any more naming/selection surface is
built, the **conceptual model** must converge. This PRD produces the **contract** the rest of the
program implements.

## The central design questions to resolve (this is mostly a design PRD)

1. **What is the canonical "reference to a sub-region of geometry"?** A typed, representation-aware
   value? Today there are 4 incompatible answers (`@face` role-keywords, dead `user_label`, no-op
   `LeafQuery::Named`, FEA `target:` strings). Pick ONE model and define how the others collapse
   into it.
2. **Where do topology nouns belong?** `SelectorKind {Face,Edge,Body,Vertex}` currently lives in
   `crates/reify-core/src/ty.rs:39` (the rep-agnostic layer) and reserved type names in
   `crates/reify-compiler/src/type_resolution.rs:578-582`. Decide: does topology stay in core, move
   behind a kernel-capability boundary, or get routed through the existing neutral
   `ReprKind {BRep,Mesh,Sdf,Voxel,VolumeMesh}` (`crates/reify-ir/src/geometry.rs:190`)?
3. **Per-representation resolution semantics.** What does a region reference *mean* for a kernel
   with no B-rep topology (Manifold mesh fakes faces, `crates/reify-kernel-manifold/src/kernel.rs:729`;
   Fidget SDF / OpenVDB voxel have no faces)? The v0.3 PRD `kernel-geometry-queries.md` §5.4
   specifies fail-closed (`QueryNotSupportedOnRepr` + `Value::Undef`). Confirm/extend: is
   "select the top face" definable as intent (a half-space / direction predicate resolved
   per-rep) rather than a B-rep noun?
4. **Intent-named regions vs topology selection — one model or two layers?** FEA's
   `PointLoad(point:"tip")` / `FixedSupport(target:"root")` is the rep-neutral "name a region by
   intent" road-not-taken (findings §4). Decide whether the converged model is intent-region-first
   (topology as one resolution strategy) or topology-first (intent as sugar).
5. **The fate/form of user-labels.** Given user-labels are dead, subsumable by `let`-bindings for
   ~90% of cases, and only genuinely needed for topology-split stability (served by `mod_history`):
   do we keep user-controlled naming at all? If yes, in what form — a **structured first-class
   reference** (findings alt (d)), NOT a bare `String`? This decision determines whether the
   original charter's D1/D3 ever get built (and as what). **A "drop user-labels" outcome is a
   valid, expected result of this PRD.**
6. **Syntax.** If any new surface is needed, it must honor spec §1.3 principles #1 (Regularity) /
   #4 (keywords over symbols) and the language's first-class-identifier convention
   (`grammar.js:1674`) — NOT the `@`-overload + string-literal-key pattern, and NOT the v2 sigil
   zoo (`docs/prds/v0_2/persistent-naming-v2.md:81-89`). **Gate every surface-syntax fragment
   through the grammar gate** (`.claude/skills/prd/references/grammar-gate.md`).

## Scope / deliverables (the PRD should produce)

- A **contract section** (B+H): the region-reference value type's signature + invariants, the
  layer boundary (what lives in `reify-core` vs behind a kernel capability), and the per-kernel
  resolution contract (incl. fail-closed semantics).
- A **boundary-test sketch** facing both producers (kernels) and consumers (selectors, FEA,
  datums/frames, fillet/chamfer targets).
- A migration/coexistence plan for the 4 namespaces (which collapse, which are deprecated).
- An explicit **labels decision** (keep-as-structured-ref / drop / defer-with-trigger).

## Key code pointers (verify against current main)

- Layer claim: `docs/reify-implementation-architecture.md` §1.1, §10.2; spec §3.3.2, §6.1.3,
  §8.12, §1.3, §12.2 (`#kernel` pragma).
- `SelectorKind` (core): `crates/reify-core/src/ty.rs:39`. Reserved type names:
  `crates/reify-compiler/src/type_resolution.rs:578-582`.
- Neutral classifier: `ReprKind` `crates/reify-ir/src/geometry.rs:190`; `BRepKind` `:136`.
- `GeometryKernel` trait (OCCT-vocab leak): `crates/reify-ir/src/geometry.rs:3194-3239`.
- Manifold face-faking: `crates/reify-kernel-manifold/src/kernel.rs:729-761`; no-op hook `:891-953`.
- `LeafQuery::Named` no-op: `crates/reify-eval/src/topology_selectors.rs:1501-1517`.
- `@face` role resolution: `crates/reify-eval/src/geometry_ops.rs:8114` (`cap_kind_translation`).
- FEA intent regions: `crates/reify-stdlib/src/helpers.rs:214` (`validate_selector_target`).
- Fail-closed precedent: `docs/prds/v0_3/kernel-geometry-queries.md` §5.4;
  `docs/prds/v0_3/multi-kernel-phase-3.md` `op_accepts_repr` table.

## Out of scope (owned by sibling briefs)

- `FeatureId` structuring + `Feature` value → **P1**.
- `SelectorKind`-enum unification mechanics + `FeatureTagTable` retirement → **P2** (but P2's
  `LeafQuery::Named` *fate* follows YOUR decision here).
- `feature()`/provenance selector surface → **P3** (consumes your model).
- FEA `validate_selector_target` bridge mechanics → **P4** (consumes your model).

## Dependencies

- **Upstream:** none (keystone).
- **Downstream:** P3, P4 (and P2's `Named`-fate). Wave-2 briefs should not be authored until this
  PRD is committed.

## SOP reminders

- Commit the PRD doc before creating any tasks (`feedback_commit_prds_before_referencing_tasks`).
- Every PRD-named deliverable (example `.ri`, smoke test) is a leaf task with a file-exists +
  content signal (`feedback_prd_deliverable_checklist`).
- This brief is the charter; cite `./00-findings.md` for evidence.
