# Capability Manifest — P2: Selector substrate convergence

Mechanizes G3 + G6 per leaf for `./P2-selector-substrate-convergence.md`. Bindings verified against
`main` 2026-06-24 (three parallel source-reading agents — PRD §2 — and the committed P0 contract
`./P0-region-reference-layer-model.md`). Any `FAIL`-class binding blocks queueing. All three leaves
are concretely specified and dependency-gated on the live P0 task set (4811/α, 4814/δ, 4812/β).

---

## τB — Retire write-only-dead `FeatureTagTable` *(leaf; independent; queued)*

Signal: full `cargo nextest` green post-deletion + `grep` for the retired symbols returns 0. A
**deletion** leaf — premises are *absence-of-consumer* facts; the anti-orphan check runs in reverse
(PASS = "confirmed unwired in production, safe to remove").

| Capability the signal asserts | Check | Evidence | Verdict |
|---|---|---|---|
| `resolve_unique_by_tag` (sole reader) has no production caller | anti-orphan (inverted) | def `topology_selectors.rs:1185`; all 5 callers `#[test]` (`:2227,:2283,:2347,:2414,:2606`, `#[cfg(test)] mod tests :1558`) | **PASS** |
| No other reader of `FeatureTagTable` | anti-orphan (inverted) | grep: zero non-test `.lookup()`/field reads; `@face` uses `TopologyAttributeTable`, not this table | **PASS** |
| Removing the prod `record`(`:6169`)/`remove`(`:5468`) pair is behavior-neutral | premise (G6 b2/3) | written entries evicted/abandoned, never read; by-construction | **PASS** |
| `step_kind`/`source_span` carry no surviving prod diagnostic | premise | consumed only by the test-only reader; absent from live `TopologyAttribute` (D2 — no fold) | **PASS** |

Grammar-fixture N/A · numeric-floor N/A · field-population N/A. **All PASS — τB does not block.**

---

## τA — De-duplicate the `@`-family `SelectorKind` *(leaf; `depends_on` 4811; queued)*

Signal: `.ri` fixture — `@face`/`@point`/`@edge` + `faces()/edges()/vertices()` `reify eval`
identically pre/post (`@point` still → `Value::Frame`); `git grep "enum SelectorKind" crates/` = 1.

| Capability the signal asserts | Check | Evidence | Verdict |
|---|---|---|---|
| The canonical `SelectorKind` (reframed, in reify-core) is upstream | anti-orphan / DAG-direction | producer **P0 task 4811/α** (reframe + `RegionRef` alias, manifold-dimensionality, stays in core); τA `depends_on` 4811 (upstream) | **PASS** (`producer:4811 upstream`) |
| `@point` resolves to `Value::Frame`, not a `SelectorKind` member | premise (G6 b3, capability-extent) | `@point(x,y,z)`→`Value::Frame`, never reaches kernel (`reify-expr/src/lib.rs:1188-1236`; `from_selector_kind`→`None`, `geometry_ops.rs:7882-7892`); matches P0 invariant 4 / §6.2 | **PASS** |
| Identical eval output across the rename (no behavior change) | premise (G6 b2) | de-collision changes no logic; identical-output by construction; existing `@`-selector + selector tests pin it | **PASS** |
| `@face`/`@edge` map to canonical 2-/1-manifold kinds | grammar-fixture (existing syntax) | `@face`/`@edge`/`faces()/edges()` parse today (no novel `.ri`) | **PASS** (grammar N/A) |

**All PASS** (gated on 4811 landing). Numeric-floor N/A.

---

## τC — Drop user-labels: field + resolver-branch + `Named` semantics *(leaf; `depends_on` 4814; queued)*

Signal: user-label namespace removed (field/resolver/`Named` user-string gone) **and** no regression
to the deprecated-but-live role path (`face(b,"top")`/`@face("top")` still resolve the same handles);
`cargo nextest` + `reify-audit` green.

| Capability the signal asserts | Check | Evidence | Verdict |
|---|---|---|---|
| The drop-user-labels decision (D4) is fixed upstream | anti-orphan / DAG-direction | producer **P0 D4** (committed contract) + **task 4814/δ** (isolated half upstream); τC `depends_on` 4814 | **PASS** (`producer:P0-D4/4814 upstream`) |
| `TopologyAttribute.user_label` is dead (safe to remove) | anti-orphan (inverted) | `geometry.rs:3903`; `None` at every prod seeder; resolver read-branch never matches (P0 §2 / findings §5) | **PASS** |
| The user-string `LeafQuery::Named` semantics are dead (safe to remove) | anti-orphan (inverted) | no-op arm returns `[]`+`TopologyTagStale` (`topology_selectors.rs:1501-1517`); §2 | **PASS** |
| The role path survives removal (no regression) | premise (G6 b2/3 — field-population twin, inverted: live path stays live) | `@face("top")` role resolution = `cap_kind_translation` + `resolve_unique_by_attribute` over `TopologyAttributeTable` (`geometry_ops.rs:8114`, `engine_build.rs:8160`) — a **separate live path** not removed by Thread C | **PASS** |
| `Named` substrate not collided with 3523 | rejection/coordination (process) | D4: confirm 3523's live state at dispatch; land τC after; **DO NOT TOUCH** 3523/esc-3523-75/76 | **PASS** (coordination gate, not code) |

**All PASS** (gated on 4814 landing + 3523 coordination). Grammar N/A · numeric-floor N/A.
