# P3 capability manifest — feature-provenance query surface

> Mechanizes G3 + G6 for [`./P3-feature-provenance-query-surface.md`](./P3-feature-provenance-query-surface.md).
> One block per task; each asserted capability bound to evidence. Substrate verified against `main`
> HEAD `1f1f503916` at authoring. **Grammar-gate: PASS** (one fixture, §α) — function-call forms only,
> no novel productions. Any `FAIL` binding blocks the batch.

**Evidence legend.** `grep:<file>:<line>` = wired on main (verified present). `producer:task-X
upstream` = capability delivered by an upstream task in the dependency closure (P1 4806/4808, P0
4811/4812). `grammar:PASS` = `tree-sitter parse --quiet` exit 0, 0 ERROR. `rejection-check` = G6
branch 4 (author X, observe the diagnostic fires). `field-pop` = a producer writes a non-`Undef`
provenance value on the production path. `qualitative` = G6 branch-1 guard: a *distinctness* claim,
not a brittle exact-count pin.

DAG: `α ─► β ─► γ`  (all gated on P1 4806/4808 + P0 4811/4812).

---

## α — `feature(geometry)` accessor → `Value::Feature`  *(leaf)*

| Capability asserted | Evidence | Verdict |
|---|---|---|
| `feature(base)` parses (no novel syntax) | `grammar:PASS` — `structure def` fixture w/ `feature`/`created_by_feature`/`split_by_feature`, exit 0, 0 ERROR (2026-06-24) | PASS |
| `feature()` is net-new (no name collision) | workspace grep: no `feature`-named geometry accessor registered today | PASS |
| `Value::Feature` / `Type::Feature` exist to return | `producer:task-4808 upstream` (P1 γ; DAG edge α→4808) | PASS |
| Structured `FeatureId` exists (the payload) | `producer:task-4806 upstream` (P1 α) | PASS |
| Whole-body origin is recoverable | `Value::GeometryHandle` keeps the structured `RealizationNodeId` → `FeatureId::Realization` `grep:crates/reify-ir/src/value.rs:1042` | PASS |
| Sub-shape origin is recoverable | `TopologyAttribute.feature_id` is keyed in the table `grep:crates/reify-eval/src/topology_attribute_resolver.rs:76`; helpers read it `grep:selector_vocabulary_v2.rs:708` | PASS |
| **Off-provenance → diagnostic + `Undef`** (D3, rejection) | rejection-check: author `feature(imported)`/no-entry, observe structured diagnostic; mechanism = `producer:task-4812 upstream` (P0 β `gate_query_capability` → `QueryNotSupportedOnRepr`) wired into the region path | PASS (rejection bound upstream) |

## β — `created_by_feature` / `split_by_feature` selectors (wiring slice)  *(leaf)*

| Capability asserted | Evidence | Verdict |
|---|---|---|
| The `ByRole` wiring template is present & verified (task 4536) | `grep:value.rs:462/488/669` (enum/`required_kind`/`hash_query`) · `topology_selectors.rs:1518` (ByRole resolve arm) · `units.rs:201/288` · `geometry_ops.rs:4556/4752` | PASS |
| The two pure-helper predicates exist to reuse | `grep:selector_vocabulary_v2.rs:700` (`created_by_feature`) · `:733` (`split_by_feature`) | PASS |
| `FeatureId` exists to carry in the new variants | `producer:task-4806 upstream` | PASS |
| `Value::Feature` exists as the selector arg | `producer:task-4808 upstream` | PASS |
| Result kind `Selector(Face)` = a 2-manifold `RegionRef` (D2 framing) | `producer:task-4811 upstream` (P0 α canonical region ref + `SelectorKind`-as-dimensionality); `grep:crates/reify-core/src/ty.rs:60` (`dimensionality()`) | PASS |
| `role_is_face` is derivable (Face-only filter, D2) | `Role` variants group into face/edge/vertex (`grep:crates/reify-ir/src/geometry.rs:3770` enum; `MidSurfaceFace`→Face precedent in `required_kind` `value.rs:499`) | PASS |
| New `hash_query` tag bytes are frozen/append-only (≥ 8) | `grep:value.rs:669` (existing tag scheme: `Named`=0, `ByRole`=…); β appends, never renumbers | PASS |
| Exhaustive `LeafQuery` matchers are bounded & known | `grep:LeafQuery::` hits in `value.rs`, `topology_selectors.rs`, `geometry_ops.rs`, `compiler/geometry.rs`, `compiler/units.rs`, `eval/lib.rs` (construction sites) — `files:[]`, BRE acquires | PASS (extent covered) |
| **Resolve arm is gate-consistent** (lands after P0 β) | `producer:task-4812 upstream`; β adds arms in the *same* `topology_selectors.rs` resolve path P0 β converges (edge β→4812) | PASS (sequencing bound) |
| Provenance is read-only (inv. 2) | resolve arm consumes `&TopologyAttributeTable` (shared ref) — `grep:topology_selectors.rs:1469` (`table: &TopologyAttributeTable`) | PASS |

## γ — Round-trip `.ri` example + two-way boundary test (focused-H gate)  *(leaf)*

| Capability asserted | Evidence | Verdict |
|---|---|---|
| All exercised capabilities delivered upstream (no inversion, G6 branch 3) | `producer:tasks α,β upstream` (DAG α→β→γ); P1 4806/4808 + P0 4811/4812 in closure | PASS |
| Provenance is **populated on the production path** (the round-trip has a real source) | `field-pop`: `primitive_attribute_seed` writes `feature_id` per face/edge/vertex (`grep:crates/reify-eval/src/primitive_attribute_seed.rs:329/367/395/510`); seeded `mod_history` empty (`:333`) | PASS |
| **Fillet stamps generated faces with the fillet feature** (the "distinct face sets" premise) | `grep:engine_build.rs:920` (`GeometryOp::Fillet/Chamfer` → `populate_local_feature_op`) → `:1056` (`propagate_attributes_via_local_feature_history`, `face_generated←edges`) — **wired on the live op-execute dispatch** | PASS (production-backed) |
| `created_by_feature(g, f1)` vs `…(g, f2)` are **disjoint & non-empty** for a filleted box | `qualitative` (G6 branch-1 guard): distinctness asserted, **not** an exact count; the architect pins counts to the realized fixture only after observing it (OQ#4) | PASS (no guessed bound) |
| **Off-provenance fail-closed** fires (D3, rejection) | rejection-check bound via α/P0 β (`QueryNotSupportedOnRepr` family); γ observes the diagnostic + `Undef`, never silent empty | PASS |
| `split_by_feature` has a `mod_history`-bearing exercise path | `grep:morph_stage_b.rs:732/740` (`ModEntry.splitting_feature_id` written on the morph/split path) — OQ#5: γ exercises via this path or scopes the row, noting the gap | PASS (path exists; fixture choice tactical) |
| Explicit-projection type error fires (D1, consumer rejection) | rejection-check: pass a `Geometry` to a `Feature`-typed param → construct-time `reify check` error (no implicit coercion); mechanism = the dedicated `Type::Feature` `producer:task-4808 upstream` | PASS |

---

**Result:** 0 FAIL bindings. The batch clears the G3 + G6 manifest gate.

- **G3** — all substrate (the `ByRole` template, the two pure helpers, the populated provenance path)
  is present on `main`; the only *assumed* substrate (`Value::Feature`, structured `FeatureId`, the
  region-ref framing, the fail-closed gate) is queued as **explicit prerequisite edges** to P1
  4806/4808 + P0 4811/4812. Grammar is a verified PASS (no novel productions).
- **G6** — the load-bearing premise (a fillet yields *distinct* queryable face sets) is **production-
  backed** (`populate_local_feature_op` is wired on the live op-execute dispatch), and asserted
  **qualitatively** (disjoint & non-empty, no guessed exact count). Every rejection signal
  (off-provenance fail-closed; explicit-projection type error) is backed by an upstream mechanism
  (P0 β's gate; P1's dedicated `Type::Feature`), not a hoped-for one.
