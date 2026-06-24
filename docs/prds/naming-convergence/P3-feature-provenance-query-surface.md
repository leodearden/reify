# P3 — Feature-provenance query surface (`feature()` + provenance selectors)

> **Status:** active (Wave 2 surface). Naming & Selection Convergence program, P3 of P0–P4.
> Date: 2026-06-24. Approach **B + focused-H** (vertical slice over a *verified* template +
> one two-way integration-gate; the full design-first B+H lives upstream in **P0**, whose
> contract P3 builds to). Authored from
> [`./P3-feature-provenance-query-surface.brief.md`](./P3-feature-provenance-query-surface.brief.md);
> evidence base [`./00-findings.md`](./00-findings.md) §5 (orphan selectors) / "Overlap &
> alternatives" (alt-c). Substrate re-verified against `main` HEAD `1f1f503916` at authoring.
>
> **Do NOT touch task 3523 or esc-3523-75/76** — the `/unblock 3523` session owns the
> `LeafQuery::Named` substrate. P3 adds *new* `LeafQuery` variants (`CreatedByFeature`/`SplitByFeature`)
> and does **not** edit, resolve, or depend on the `Named` variant.

This PRD delivers the **original charter D2 value** (`./00-findings.md` "Program shape"): the
construction-history attribute substrate is **live and populated** on the production path, but its
query selectors are **orphans** with no `.ri` surface. P3 surfaces them — no new namespace, no new
substrate; it *registers and resolve-wires data the engine already computes*.

---

## §0 — Purpose, consumer, and user-observable surface

Feature-provenance (`feature_id` / `role` / `local_index` / `mod_history` on `TopologyAttribute`) is
**populated on the production path** by `primitive_attribute_seed` (per-face/edge/vertex seeding) and
the OCCT-history propagation that runs on every fillet/chamfer/boolean op
(`engine_build.rs` `populate_local_feature_op` :920 / `populate_boolean_op` :896 — verified wired,
§5). The selectors that would make it user-queryable — `created_by_feature`
(`crates/reify-eval/src/selector_vocabulary_v2.rs:700`) and `split_by_feature` (`:733`) — are
**pure-Rust helpers with test-only callers** (the C-10 orphan cluster, findings §5). And there is **no
`feature()` accessor at all**: a user can construct provenance but cannot name or read it.

**The one robustness this delivers that a `let`-bound predicate selector genuinely lacks**
(findings, "Overlap & alternatives"): **topology-split stability via `mod_history`**. A predicate
selector (`faces_by_normal(body, +Z)[0]`) degenerates and silently rebinds across a topology split;
feature-provenance survives it (`split_by_feature`, backed by `mod_history` / the
`AmbiguousAfterSplit` discipline). This is alt-(c) — "surface data you already compute" — not alt-(e)
string labels (dropped by P0 D4).

**Consumer (G1).** Concrete and named:
1. **In-PRD:** the `feature()` accessor's output is consumed by the two provenance selectors (task β)
   and the committed round-trip `.ri` example (task γ).
2. **In-engine seam:** the selectors plug into the **existing topology-selector resolution path**
   (`resolve_leaf` in `topology_selectors.rs`, the same seam `LeafQuery::ByRole` uses — task 4536).
   **No new seam is introduced** (overlay G1 engine-integration sub-check: this is not one of the 7
   norm seams *added*, it *completes* an existing one).
3. **Downstream PRDs:** consumers of stable provenance references — FEA load/support targets (via
   **P4**), mesh-morph warm-start (#2952), shells mid-surface. These are why provenance, not labels,
   is the value (findings §"three-layer reframe").

**User-observable surface (G2).** Three CLI/CI-observable signals, one per leaf (§8): `reify check`
types `feature(base)` as a `Feature` (was an unresolved-function error); `reify eval` resolves
`created_by_feature(filleted, feature(filleted))` to the fillet's generated faces, **distinct from**
`created_by_feature(filleted, feature(base))`; and a committed `.ri` example runs green in CI.

---

## §1 — Goal

Make already-populated feature provenance **user-queryable** in `.ri`, via existing grammar
(function-call selectors), built to **P0's** region-reference contract and **P1's** structured
`Value::Feature` type:

1. A **`feature(geometry)` accessor** — an **explicit projection** `Geometry → Feature` (P1's
   `Value::Feature`), resolving the geometry's origin feature (whole body → its realization op;
   sub-shape → its attribute-table entry).
2. Two **provenance region-reference selectors** — `created_by_feature(solid, f)` and
   `split_by_feature(solid, f)` — registered and resolve-wired by **mirroring the `LeafQuery::ByRole`
   template** over the existing pure-Rust helper predicates. Result kind: `Selector(Face)` (D2).
3. A committed **`.ri` round-trip example** exercising `feature → created_by_feature` over a fillet,
   plus a **two-way boundary test** (the focused-H integration gate).

P3 introduces **no new surface syntax** (D5 / P0 D5 — function-call forms only) and **no new
provenance-population mechanism** (it queries what the engine already writes).

---

## §2 — Background (condensed; `./00-findings.md` is authoritative)

| Fact (re-verified against `main` `1f1f503916`) | Evidence |
|---|---|
| Provenance is **live & populated** on the production path | `primitive_attribute_seed.rs` seeds `feature_id` per face/edge/vertex; fillet/chamfer → `populate_local_feature_op` (`engine_build.rs:920`); boolean → `populate_boolean_op` (`:896`); both call the wired `propagate_attributes_via_{local_feature,brepalgoapi}_history` |
| The two provenance selectors exist but are **orphans** (C-10) | `created_by_feature` (`selector_vocabulary_v2.rs:700`), `split_by_feature` (`:733`) — pure helpers, test-only callers |
| There is **no `feature()` accessor** | workspace grep: no `feature`-named geometry accessor registered (net-new) |
| The wiring template is **verified** (task 4536, `LeafQuery::ByRole`) | enum + `required_kind` + `hash_query` (`value.rs:462/488/669`); resolve arm (`topology_selectors.rs:1518`); registration (`units.rs:201/288`); lowering (`geometry_ops.rs:4556/4752`) |
| The provenance value is structured **upstream in P1**, not here | `Value::Feature(FeatureId)` + `Type::Feature` (P1 γ, task 4808); structured `FeatureId` (P1 α, task 4806) |
| The region-reference framing + fail-closed gate is **upstream in P0** | canonical `RegionRef` + `SelectorKind`-as-dimensionality (P0 α, task 4811); fail-closed `QueryNotSupportedOnRepr` resolution (P0 β, task 4812) |

`./00-findings.md` "three-layer reframe": **feature-provenance is the value; user-labels-as-strings is
not** (labels are `None` at every seeder; dropped by P0 D4). Every *planned* consumer depends on
provenance/substrate; **none** depends on user-labels. P3 surfaces the value layer only.

---

## §3 — Resolved design decisions

### D1 — `feature()` is an **explicit projection**, not implicit coercion (brief §1; charter-ratified)

`feature(geometry) : Feature` is an explicit accessor that projects a `Geometry` to its origin
**`Value::Feature`** (P1). There is **no** implicit `Geometry → Feature` coercion: a `Geometry` is
never silently usable where a `Feature` is required. This preserves the charter's ratified rationale
(brief §1) and matches the language's first-class-projection convention. P0 does not override it
(P0 §8 P3 row explicitly preserves "explicit-projection (not implicit coercion)").

- **Resolution modes.** `feature(whole_body)` → the body's realization op feature
  (`FeatureId::Realization`, read from the `Value::GeometryHandle`'s structured `RealizationNodeId`,
  `value.rs:1042`). `feature(sub_shape)` (an element of a resolved selector set, keyed in the
  `TopologyAttributeTable`) → that entry's `feature_id`.
- **`feature()` returns a `Feature`, not a `RegionRef`.** It is the *input* to the provenance
  selectors (which **are** `RegionRef`s), not itself a region. This keeps the two concepts distinct
  (P0 invariant 4: provenance is an *intent that names a region*; the `Feature` value is the intent's
  payload, surfaced as a first-class value by P1).

### D2 — Result kind is **`Selector(Face)`** (Face-only); kind-parametric is a deferred extension (brief §"Result kind"; P0 OQ#4)

The two provenance selectors resolve to **2-manifold (Face) region references**:
`created_by_feature(solid, f) : Selector(Face)`, `split_by_feature(solid, f) : Selector(Face)`.
`LeafQuery::CreatedByFeature(FeatureId)` / `SplitByFeature(FeatureId)` carry **no kind parameter**;
`required_kind()` returns `Some(Face)` (mirroring `ByRole(MidSurfaceFace) → Face`). The resolve arm
filters the attribute table to **face-kind** entries matching the feature (the `Role` variant space
groups cleanly into face/edge/vertex — `Cap`/`Side`/`RevolvedFace`/`SweptFace`/`LoftedFace`/
`MidSurfaceFace` are faces — so the architect derives a `role_is_face` predicate, the same way the
`ByRole` arm leans on the role→kind correspondence).

- **Why Face-only and not kind-parametric now?** The charter scoped these to `Selector(Face)`; the
  canonical use (and the §6 example) is a **face set across a fillet**. P0's taxonomy admits *any*
  dimensionality (P0 invariant 4 / OQ#4) but **explicitly does not constrain P3** — so this is P3's
  call, and minimal-real-implementation discipline (`feedback_no_throwaway`) ships the chartered Face
  scope, not a speculative kind-parametric surface.
- **Deferred extension (breadcrumb required).** Edge/vertex provenance selectors
  (`created_by_feature` over a `Selector(Edge)`/`Selector(Vertex)` kind) are a candidate future
  extension. Task β **must** leave an implementation-site breadcrumb at the `LeafQuery` variant def
  citing this §3 D2 + P0 OQ#4 and the deferred kind-parametric variant
  (`feedback_breadcrumb_design_alternatives_at_impl_site`). The seeding already writes per-edge and
  per-vertex `feature_id`, so the extension is data-ready — only the surface is scoped out.

### D3 — No-/off-provenance resolution is **fail-closed**, reusing P0 β's gate (brief §"feature() … imported geometry")

Resolving a provenance intent where **no construction-history attribute is recorded** (imported
geometry with no propagated table entry; a non-history-bearing representation) is **fail-closed**: it
emits a structured diagnostic and yields `Value::Undef` — **never a silent empty set, never a panic**.
This reuses **P0 β's** mechanism (`gate_query_capability` → `QueryNotSupportedOnRepr` + `Undef`,
task 4812): provenance is the **history-dependent intent** P0 §5 names, which resolves only where a
history table exists and fails closed elsewhere.

- `feature(whole_body)` over a realized body **always** resolves (every realized body has a
  realization node = a `FeatureId::Realization`).
- `feature(sub_shape)` / `created_by_feature` / `split_by_feature` over a body with **no recorded
  provenance for the queried sub-shapes** fails closed (diagnostic + `Undef`).
- **The exact diagnostic code is tactical** (reuse `QueryNotSupportedOnRepr`, or add a
  provenance-specific `E_NO_FEATURE_PROVENANCE` if the architect judges the semantics distinct) — but
  a structured diagnostic **MUST** fire and the cell **MUST** stay `Value::Undef`. The *signal* (a
  diagnostic fires; no silent empty) is fixed; the code mnemonic is the architect's choice at β.

### D4 — No new surface syntax; existing grammar only (D5 echo; verified)

`feature(base)`, `created_by_feature(base, f)`, `split_by_feature(base, f)` are **standard
function-call forms** — the most basic grammar production, identical in shape to `faces(body)` /
`mid_surface(body)`. **Grammar-gate: PASS** — a `structure def` fixture exercising all three parses
with `tree-sitter parse --quiet` exit 0, 0 ERROR nodes (verified 2026-06-24, `main` `1f1f503916`).
**G3-grammar: N/A** (no novel productions). Honors P0 D5 (no sigil zoo, no string-key surface).

### D5 — Scope is **query only**; provenance population is untouched

P3 **reads** `feature_id` / `mod_history`; it does **not** add, change, or extend any
provenance-population path (`primitive_attribute_seed`, `populate_local_feature_op`,
`populate_boolean_op`, the kernel attribute hook). The Manifold `propagate_attributes` table-write
(task 4262) and any cross-kernel provenance are **out of scope** (§9) — on the OCCT path provenance
is already populated, which is sufficient for the chartered deliverable.

---

## §4 — Contract: the accessor + the two provenance selectors

P3 builds to **P0's** `RegionRef` contract (§4 of P0) and **P1's** `Value::Feature`. The new surface:

```text
feature(geometry : Geometry) : Feature                 // explicit projection (D1); P1 Value::Feature
created_by_feature(solid : Geometry, f : Feature) : Selector(Face)   // D2; resolves to f's created faces
split_by_feature(solid : Geometry,  f : Feature) : Selector(Face)    // D2; resolves to faces split by f
```

New IR (mirroring the `ByRole(Role)` precedent — task 4536):

```rust
// crates/reify-ir/src/value.rs — additions to `enum LeafQuery` (value.rs:462)
//   CreatedByFeature(FeatureId),   // faces whose TopologyAttribute.feature_id == this feature
//   SplitByFeature(FeatureId),     // faces whose mod_history contains a split by this feature
// required_kind() (value.rs:488): both => Some(SelectorKind::Face)            (D2)
// hash_query()    (value.rs:669): new FROZEN, append-only tag bytes (≥ 8; never renumber)
```

**Invariants (the §6 boundary test pins each).**

1. **Explicit projection (D1).** `feature` is the *only* way to obtain a `Feature` from a `Geometry`;
   no implicit coercion. Type errors are construct-time (`reify check`), not solve-time surprises.
2. **Provenance is read-only (D5).** Resolving any P3 selector mutates no provenance state; the
   `TopologyAttributeTable` is consumed, never written, on the P3 path.
3. **Face-kind discipline (D2).** Both selectors are `Selector(Face)`; resolution returns only
   face-kind handles matching the feature. A `Feature` whose op created only edges/vertices resolves
   to the empty (then `Undef`, one layer up) face set — not an error.
4. **Distinct-provenance discrimination (the value, §0).** For a body with ≥ 2 distinct features
   (e.g. a filleted box), `created_by_feature(body, f1)` and `created_by_feature(body, f2)` are
   **disjoint** face sets; neither is empty when both features created faces. (Qualitative — no
   brittle exact-count assertion; the architect pins counts to the chosen fixture.)
5. **Fail-closed off-provenance (D3).** Provenance resolution over a body lacking recorded provenance
   emits exactly one structured diagnostic and yields `Value::Undef` — reusing P0 β's gate. Never a
   silent empty, never a panic.
6. **Content-hash stability (inherited from `SelectorValue`).** A P3 selector's identity is a function
   of its query tree (the `FeatureId` + kind), excluding the ephemeral `kernel_handle` (`hash_ghr`,
   `value.rs:724`); the new `hash_query` tag bytes are frozen & append-only. Re-eval is stable.

---

## §5 — The wiring template (verified against `LeafQuery::ByRole`, task 4536)

Adding an attribute-table-backed selector touches exactly these sites (all re-verified against `main`
`1f1f503916`; the `ByRole` arm is the line-by-line model):

| Site | `file:line` | Obligation for `CreatedByFeature` / `SplitByFeature` |
|---|---|---|
| `LeafQuery` enum | `crates/reify-ir/src/value.rs:462` | add the two variants carrying a `FeatureId` |
| `required_kind()` | `value.rs:488` | both `=> Some(SelectorKind::Face)` (D2) |
| `hash_query()` | `value.rs:669` | new frozen, append-only tag bytes (≥ 8) per variant |
| `resolve_leaf` arm | `crates/reify-eval/src/topology_selectors.rs:1464` (model arm `ByRole` `:1518`) | filter the threaded `&TopologyAttributeTable` by `attr.feature_id == fid && role_is_face` (CreatedBy) / `attr.mod_history.any(== fid) && role_is_face` (SplitBy); canonical `(local_index, id)` sort; **no kernel call** |
| Compiler registration | `crates/reify-compiler/src/units.rs:201` (`GEOMETRY_TOPOLOGY_SELECTOR_NAMES`) | add `created_by_feature`, `split_by_feature` |
| Result-type | `units.rs:288` (`topology_selector_result_type`) | both → `Type::Selector(Face)` |
| Eval-time lowering | `crates/reify-eval/src/geometry_ops.rs:4556` (`TopologySelectorHelper`) / `:4752` (name→helper) | new helper variants binding the `Feature` arg → the `LeafQuery` |

**`feature()` accessor** is *not* a topology selector (it returns `Feature`, not `Selector`): it
registers as its own geometry accessor (name → `Type::Feature` result; eval path reads the handle's
`RealizationNodeId` / the attribute-table entry). Exact registration file is the architect's to
acquire (it is **not** in `GEOMETRY_TOPOLOGY_SELECTOR_NAMES`).

**Scope note carried from the `ByRole` arm** (`topology_selectors.rs:1532-1546`): the threaded table is
**build-global** and the `solid`/`target` argument is *unused* by the resolve arm. For provenance this
is **benign and more precise** than for roles: a `FeatureId` is globally unique
(`entity#realization[index]`), so `created_by_feature(body, f)` returns exactly the faces `f` created,
regardless of which `body` handle is passed. β preserves this scope note verbatim (and the
`solid`-arg is retained in the surface for readability + future per-body correlation, even though the
resolve arm does not consult it).

---

## §6 — Boundary-test sketch (two-way; the focused-H integration gate)

The seam is between **the query surface** (`feature()` + the two selectors, `reify-eval` +
`reify-compiler`) and **the provenance-population substrate** (`primitive_attribute_seed` +
`populate_local_feature_op`). Task γ implements this matrix; it is γ's observable signal (closes G2
via the C-as-integration-gate pattern). Tests live under `crates/reify-eval/tests/` + a committed
`.ri` example.

### 6.1 Producer-side (the query surface looks at the populated substrate)

| Scenario | Preconditions | Postconditions |
|---|---|---|
| **Primitive round-trip.** `let f = feature(base); let s = created_by_feature(base, f)` on `box(...)`. | OCCT; box seeded by `primitive_attribute_seed`. | `f` is a `Value::Feature` (box realization); `s` resolves to the box's face set (non-empty). |
| **Fillet discrimination (the value).** `let g = fillet(base, edge, r)`; compare `created_by_feature(g, feature(base))` vs `created_by_feature(g, feature(g))`. | OCCT; fillet propagates via `populate_local_feature_op`. | Two **disjoint, non-empty** face sets: base-origin faces vs generated fillet faces (qualitative; no exact count pinned). |
| **Split stability.** `split_by_feature(g, feature(g))` after a split-bearing op records `mod_history`. | A `mod_history` entry exists (morph/split path). | Resolves the faces whose `mod_history` contains that feature — the robustness a predicate selector lacks. |
| **Off-provenance fail-closed (D3).** `feature(sub)` / `created_by_feature(imported, f)` over geometry with no recorded provenance. | Imported / no propagated entry. | Exactly one structured diagnostic (P0 β's `QueryNotSupportedOnRepr` family) + `Value::Undef`; **no panic, no silent empty**. |

### 6.2 Consumer-side (call sites look at the new surface)

| Scenario | Preconditions | Postconditions |
|---|---|---|
| **Explicit projection (D1).** Pass a `Geometry` where a `Feature` is required (no `feature()`). | Typed call site. | Construct-time type error (`reify check`) — **no** implicit `Geometry → Feature` coercion. |
| **Kind discipline (D2).** Consume `created_by_feature(...) : Selector(Face)` where a `Selector(Face)` is expected (e.g. `fillet`-of-faces). | Face-typed consumer. | Accepted; a non-Face selector to a `face:` param is a construct-time kind error. |
| **`feature()` whole-body vs sub-shape (D1).** `feature(base)` and `feature(single(faces(base)))`. | Body realized; sub-shape table-keyed. | Both resolve to a `Value::Feature`; whole-body → realization feature, sub-shape → its entry's feature. |
| **`.ri` example runs green.** the committed round-trip example. | the example file. | `reify eval`/`check` exits 0; the example is exercised in CI (file-exists + content signal). |

---

## §7 — Cross-PRD relationship (seam ownership) [G4]

P3 is a **pure consumer** of P0 + P1. Real `add_dependency` edges are wired at this decompose
(`preferences_cross_prd_deps_real_edges`); the upstream batches already exist.

| Other PRD / task | Direction | Seam mechanism | Owner | Edge |
|---|---|---|---|---|
| **P1 α — structured `FeatureId`** (task **4806**) | P3 consumes | `FeatureId` is the payload of `LeafQuery::CreatedByFeature(FeatureId)` / `SplitByFeature(FeatureId)` | P1 delivers; P3 carries it in the variants | **β → 4806** |
| **P1 γ — `Value::Feature` + `Type::Feature`** (task **4808**) | P3 consumes | `feature()` returns `Value::Feature`; the selectors take a `Feature` arg | P1 delivers the type; **P3 owns the accessor/selector integration** (P1 §Cross-PRD P3 row) | **α → 4808**, **β → 4808** |
| **P0 α — canonical `RegionRef` + `SelectorKind`-as-dimensionality** (task **4811**) | P3 consumes | the selectors' `Selector(Face)` result is a 2-manifold `RegionRef`; D2 result-kind framing | P0 fixes the framing; P3 instantiates a provenance intent | **β → 4811** |
| **P0 β — fail-closed `QueryNotSupportedOnRepr` resolution** (task **4812**) | P3 consumes | D3 off-provenance resolution reuses P0 β's gate; P3 adds resolve arms in the **same** `topology_selectors.rs` resolve path P0 β converges | P0 owns the gate + the converged resolve path; P3 lands its arms **after** β so they are gate-consistent | **α → 4812**, **β → 4812** |
| **P0 δ — drop `has_user_label`/`user_label_eq`** (task **4814**) | file-adjacency (no mechanism seam) | shared file `selector_vocabulary_v2.rs` — **disjoint regions**: 4814 removes `:774/:799` (user-label orphans); P3 reuses `:700/:733` (provenance helpers) | independent; whoever lands second rebases the trivial region | **no edge** (documented adjacency) |
| **P0 γ — region-resolution boundary test** (task 4813) | sibling test | P3's §6 boundary test mirrors the producer/consumer shape but over the provenance path | independent test artifacts | **no edge** (P3 depends on the *mechanism* α/β, not P0's test) |
| **P2 — selector-substrate convergence** | benefits-from (not blocking) | converged `SelectorKind` / retired `FeatureTagTable` | P2 (Wave 1, mostly independent) | **no edge** (P3 does not require P2; brief §Dependencies) |
| **P4 — FEA-target unification** | P3 **produces for** | stable provenance `RegionRef`s become FEA load/support targets | P4 owns the bridge | downstream; P4 wires its own edge at P4 decompose |
| **`/unblock 3523`** (esc-3523-75/76) | coordinate, don't collide | shared `LeafQuery` enum file `value.rs` — **disjoint variants** (3523 owns `Named`; P3 adds `CreatedByFeature`/`SplitByFeature`) | 3523 owns `Named` | **DO NOT TOUCH** |

No new contested-ownership pair (overlay G4): P3 *resolves* the `topology-selectors ↔
persistent-naming-v2` seam by surfacing the provenance the convergence ratified.

---

## §8 — Decomposition plan (B + focused-H)

Greek labels; task IDs assigned at decompose. Each **leaf** names a user-observable signal; the
DAG is serial-ish because α/β/γ overlap on `units.rs` + `geometry_ops.rs` (serial avoids rebase
churn on a 3-task surface).

### α — `feature(geometry)` accessor → `Value::Feature`  *(leaf, CLI signal)*

Register and eval-wire the explicit projection `feature(geometry) : Feature` (D1). Whole body →
realization-op feature (`FeatureId::Realization` from the handle's `RealizationNodeId`); sub-shape →
its `TopologyAttributeTable` entry's `feature_id`; no recorded provenance → fail-closed (D3, P0 β
gate). Returns P1's `Value::Feature` (task 4808).
- **Prereqs:** 4806 (`FeatureId`), 4808 (`Value::Feature`/`Type::Feature`), 4812 (fail-closed gate).
- **Modules:** reify-compiler (accessor registration + result type), reify-eval (eval path). *files:*
  `[]` (registration-file footprint acquired by BRE; accessor is **not** a topology selector).
- **Signal (leaf, CLI):** `reify check` on `let f = feature(box(10mm,10mm,10mm))` types `f` as
  `Feature` (was an unresolved-function error); `reify eval` produces a non-`Undef` `Value::Feature`;
  `feature()` over no-provenance geometry emits the structured fail-closed diagnostic. **Unlocks** β, γ.

### β — `created_by_feature` / `split_by_feature` selectors (the wiring slice)  *(leaf, CLI signal)*

Add `LeafQuery::CreatedByFeature(FeatureId)` + `SplitByFeature(FeatureId)` and resolve-wire them by
**mirroring the `ByRole` template** across all §5 sites (enum + `required_kind`+`hash_query`; resolve
arm filtering the table by `feature_id` / `mod_history` to **face-kind** entries; `units.rs`
registration + `Type::Selector(Face)`; `geometry_ops.rs` lowering). Reuse the existing pure-helper
predicates (`selector_vocabulary_v2.rs:700/733`). Leave the **D2 deferred-extension breadcrumb**
(kind-parametric edge/vertex provenance; cite §3 D2 + P0 OQ#4).
- **Prereqs:** α (Feature source for the surface round-trip), 4806 (`FeatureId` in the variant),
  4808 (`Value::Feature` arg), 4811 (`Selector(Face)` = `RegionRef`), 4812 (gate-consistent resolve
  path).
- **Modules:** reify-ir (`value.rs`), reify-eval (`topology_selectors.rs`, `geometry_ops.rs`),
  reify-compiler (`units.rs`). *files:* `[]` (adding `LeafQuery` variants fans out to every exhaustive
  matcher — `value.rs`, `topology_selectors.rs`, `geometry_ops.rs`, `compiler/geometry.rs`,
  `compiler/units.rs` — BRE acquires the exact set; mirrors P1 γ).
- **Signal (leaf, CLI):** `reify check` types `created_by_feature(base, feature(base))` as
  `Selector(Face)` (was an unresolved-function error); `reify eval` over a filleted box resolves
  `created_by_feature(g, feature(g))` to the generated fillet faces, **disjoint from**
  `created_by_feature(g, feature(base))` (both non-empty). **Unlocks** γ.

### γ — Round-trip `.ri` example + two-way boundary test (the focused-H integration gate)  *(leaf, CI signal)*

Commit the charter's `FeatureProvenanceSelectorsV2` example (`feature(base)` → fillet →
`created_by_feature` → distinct face sets) and implement the §6 two-way boundary matrix (producer:
primitive round-trip, fillet discrimination, split stability, off-provenance fail-closed; consumer:
explicit-projection type error, kind discipline, whole-body-vs-sub-shape). Document the three new
functions in `docs/reify-stdlib-reference.md`.
- **Prereqs:** α, β.
- **Modules:** reify-eval (`tests/`), `examples/`, docs. *files:* `[]` (new example + new boundary-test
  file spanning the reify-eval test tree — footprint acquired by BRE; mirrors P0 γ / P1 ε).
- **Signal (leaf, CI):** a committed `examples/…/feature_provenance_selectors_v2.ri` that `reify
  eval`/`check` runs green (file-exists + content), **and** a committed boundary-test file under
  `crates/reify-eval/tests/` whose §6 rows all pass (fillet discrimination disjoint & non-empty;
  off-provenance fail-closed; explicit-projection type error).

### Dependency view

```
4806 ─┐                     4811 ─┐
4808 ─┼─► α ──► β ──► γ      4812 ─┤
4812 ─┘        ▲            (β also ┘ )
              4806,4808,4811
α,β ──────────► γ
```

P4 wires its own `add_dependency` edge onto P3's β/γ at P4 decompose time.

---

## §9 — Out of scope (owned elsewhere)

- **`FeatureId` structuring + `Value::Feature`/`Type::Feature`** → **P1** (P3 consumes; tasks 4806/4808).
- **Region-reference model, `SelectorKind`-as-dimensionality, fail-closed gate, user-label drop** →
  **P0** (P3 consumes 4811/4812; the `user_label` field + resolver + `LeafQuery::Named` removal are
  P2 / 3523).
- **Edge/vertex provenance selectors (kind-parametric variants)** — deferred extension (D2 breadcrumb);
  data-ready (seeding writes per-edge/vertex `feature_id`) but surface-scoped-out.
- **Any change to provenance population** — `primitive_attribute_seed`, `populate_local_feature_op`,
  `populate_boolean_op`, the kernel attribute hook (D5).
- **Manifold `propagate_attributes` table-write** (task 4262) — provenance on the mesh path; P3 is
  OCCT-path query only. Off-OCCT provenance fails closed (D3), by design.
- **FEA-target acceptance of provenance `RegionRef`s** → **P4**.
- **User-label selectors** (`has_user_label`/`user_label_eq`) — dropped by P0 D4 (the orphan helpers
  are removed by P0 δ / task 4814); P3 does not surface them.

---

## §10 — Open questions (tactical; not design-blocking)

1. **`feature()` registration site.** Which name-table the accessor registers in (it is *not* a
   topology selector — returns `Feature`, not `Selector`). The architect acquires it at α; the
   *signal* (`reify check` types `feature(x) : Feature`) is fixed.
2. **Off-provenance diagnostic code (D3).** Reuse `QueryNotSupportedOnRepr` vs add a provenance-specific
   `E_NO_FEATURE_PROVENANCE`. Either is coherent; a structured diagnostic + `Undef` is the fixed
   contract. Decide at α/β.
3. **`role_is_face` derivation (D2).** Whether to add a `Role::is_face()`/`dimensionality()` helper or
   inline the face-role set in the resolve arm. Cosmetic; decide at β.
4. **Fillet fixture exact counts.** The §6 fillet-discrimination row asserts *disjoint & non-empty*
   (qualitative). Whether to additionally pin exact face counts is the architect's call against the
   chosen fixture — pin only after observing the realized geometry (avoid a guessed-count RED test,
   the G6 hazard).
5. **`split_by_feature` exercise fixture.** Which production path records a `mod_history` split entry
   for the example (morph/split op). If no non-morph production split path exists yet, γ exercises
   `split_by_feature` via the morph path or scopes its example row to `created_by_feature`, noting the
   gap. Tactical; decide at γ.
