# PRD Brief — P3: Feature-provenance query surface (`feature()` + provenance selectors)

> **Brief for a `/prd` author session** (not a finished PRD). Read `./00-findings.md` FIRST.
> This is the **original charter D2 value** — the highest-value *missing* piece: the construction
> history is already populated but has no `.ri` query surface. **Wave 2** — do NOT author until
> **P0 is committed** (its surface form depends on P0's region-reference model) and rely on **P1's**
> `Value::Feature` type.
>
> **Do NOT touch task 3523 or esc-3523-75/76.** Today is 2026-06-24. Line numbers accurate at time
> of writing — G3-verify against current `main`.

## Why this PRD exists

Feature-provenance (`feature_id`/`role`/`local_index`/`mod_history`) is **live and populated** on
the production path (primitive seeding + OCCT-history propagation), but the selectors that would
make it user-queryable are **orphans** with no surface registration (findings §5). Surfacing them
delivers the one robustness a `let`-bound predicate selector genuinely lacks: **topology-split
stability via `mod_history`** (`AmbiguousAfterSplit`). This is alt (c) in the findings — "no new
namespace, surface data you already compute."

## Scope / deliverables

1. **`feature()` accessor** — returns the `Feature` that created a geometry (whole body → its
   construction op; sub-shape → the feature in its attribute-table entry). Uses **P1's structured
   `Value::Feature`**. Surface form per **P0** (explicit projection vs whatever P0's model dictates;
   the charter's ratified rationale was *explicit* `feature()` projection, NOT implicit
   Geometry→Feature coercion — preserve that unless P0 overrides).
2. **`created_by_feature(solid, f)` / `split_by_feature(solid, f)` selectors** — register +
   resolve-wire the existing pure-Rust helpers (`crates/reify-eval/src/selector_vocabulary_v2.rs:700`,
   `:733`) following the `LeafQuery::ByRole` precedent (the full template is in
   `./00-findings.md` and the wiring map below).
3. **An `.ri` example** exercising the round-trip (a leaf task with a file-exists + content signal),
   e.g. the charter's `FeatureProvenanceSelectorsV2` sketch (`feature(base)` → `created_by_feature` →
   distinct face sets across a fillet).

## The wiring template (verified against `LeafQuery::ByRole`, task 4536)

Add a new attribute-table-backed selector by touching:
- `LeafQuery` enum + `required_kind()` + `hash_query` (new frozen tag bytes ≥ 8):
  `crates/reify-ir/src/value.rs:462` / `:487` / `:669`.
- `resolve_leaf` arm (reads the threaded `&TopologyAttributeTable`, no kernel call — mirror the
  `ByRole` arm): `crates/reify-eval/src/topology_selectors.rs:1464` (ByRole arm ~`:1547`).
- Compiler registration: `GEOMETRY_TOPOLOGY_SELECTOR_NAMES`
  (`crates/reify-compiler/src/units.rs:201`) + `topology_selector_result_type` (`:288`).
- Eval-time lowering: `TopologySelectorHelper` + name→helper dispatch
  (`crates/reify-eval/src/geometry_ops.rs:4555` / `:4750`).

New `LeafQuery` variants (charter): `CreatedByFeature(FeatureId)`, `SplitByFeature(FeatureId)`.
These are **not** `Role`-based — they read `feature_id`/`mod_history` directly; no new `Role`
variant needed.

## Design questions to resolve

- **Result kind.** Charter scoped these to `Selector(Face)`. A feature creates faces *and* edges —
  is Face-only right, or does P0's model want kind-parametric provenance selectors? (Edge/vertex
  provenance selectors are a candidate future extension; decide in light of P0.)
- **`feature()` on a whole body vs a sub-shape** — confirm both resolve (body → realization op;
  sub-shape → attribute entry) and define behavior on imported geometry (no history →
  fallback/diagnostic).
- Whether `feature()` composes with P0's region-reference model or stays a distinct provenance
  projection.

## Out of scope

- The region-reference model / labels → **P0**. `Feature` type internals → **P1**. The
  `LeafQuery::Named` / namespace cleanup → **P2**.
- User-label selectors (`has_user_label`/`user_label_eq`) — only if **P0** retains labels; if so
  they are a *separate* follow-up in whatever form P0 chooses, NOT bare strings.

## Dependencies

- **Upstream:** **P0** (surface model) + **P1** (`Value::Feature`); benefits from **P2**'s
  converged substrate. Wire real `add_dependency` edges to the P0/P1 PRD tasks
  (`preferences_cross_prd_deps_real_edges`).
- **Downstream:** consumers of stable provenance refs (FEA targets via P4, mesh-morph, shells).

## SOP reminders

- Commit the PRD before tasks. Gate every surface-syntax fragment through the grammar gate
  (function-call forms like `feature(base)` / `created_by_feature(...)` already parse — re-verify).
  Cite `./00-findings.md`. Every named deliverable (`.ri` example, smoke test) = a leaf task with a
  file-exists + content signal.
