# PRD — P2: Selector substrate convergence

> **Program:** naming & selection convergence (P0–P4). **Charter/evidence:** `./00-findings.md`
> (§5 split-brain, §2 stringly-typed, §4 layer violation), the brief
> `./P2-selector-substrate-convergence.brief.md`, and the **keystone contract**
> `./P0-region-reference-layer-model.md` (committed + decomposed 2026-06-24 — its §3 decisions D2/D4
> directly direct this PRD). Authored 2026-06-24 via a `/prd` session (Leo + Claude). Substrate
> **G3-verified against current `main`** this session (§2).
>
> **Status:** active — all three threads are concretely specified and dependency-gated on the live
> P0 task set (§7). **Do NOT touch task 3523 or esc-3523-75/76** — the `/unblock 3523` session owns
> the `LeafQuery::Named` leaf-predicate substrate that Thread C coordinates with.
>
> Line numbers below are **snapshots at time of writing** — verify against `main` at dispatch.

## 1. Why this PRD exists (and who benefits — G1)

The selector machinery is split-brained (findings §5): two `SelectorKind` enums sharing a name, a
write-only-dead v0.1 attribute table, and a no-op named-resolution stub reachable from `.ri`. P2
converges this substrate so P3/P4 build on one coherent base, **executing P0's decisions** (D2:
`SelectorKind` framing+location; D4: drop user-labels) in the substrate-entangled code P0 carved out
for P2 (P0 §8/§10).

This PRD introduces **no new user-facing mechanism** — it *retires* dead code and *de-duplicates*
existing substrate. G1 therefore applies as a **beneficiary** check:

- **Thread B** (deletion): beneficiary is the converged attribute substrate — exactly one live
  topology-attribute table (`TopologyAttributeTable`) afterward; no mechanism introduced.
- **Thread A** (enum de-collision): beneficiary is the converged type vocabulary P0 §4 fixes — one
  canonical manifold-dimensionality `SelectorKind`, consumed by P3/P4 and the reframed reserved type
  names (spec §8.12, done in P0 4815/ε).
- **Thread C** (drop user-labels): beneficiary is the single region resolver P0 invariant 5 mandates
  — removing the dead user-string namespace collapses 1 of the 4 namespaces (findings §5), consumed
  by P3 (provenance) / P4 (FEA-target unification).

## 2. Substrate verification (G3) — verified against `main`, 2026-06-24

**No novel `.ri` syntax** (Threads A/B/C are Rust-internal refactor/deletion; any fixtures reuse
existing `@face`/`@point`/`@edge` and `faces()/edges()/vertices()` forms that parse today).
**Grammar gate N/A.** The G3 check is the semantic/structural verification of the brief's premises,
run via three parallel source-reading agents this session:

| Premise | Verdict | Evidence (snapshot) |
|---|---|---|
| Two `SelectorKind` enums exist | **TRUE** | `reify_core::ty::SelectorKind {Face,Edge,Body,Vertex}` (`crates/reify-core/src/ty.rs:30-51`, `Display→"*Selector"`, `dimensionality()` 0/1/2/3); `reify_ir::expr::SelectorKind {Face,Point,Edge}` (`crates/reify-ir/src/expr.rs:16-25`, no `Display`). Only these two `enum SelectorKind` in `crates/`. |
| The `@`-family `Point` is a coordinate frame, not a topology vertex | **TRUE** | `@point(x,y,z)` builds a `Value::Frame` from coordinates and **never reaches the kernel** (`reify-expr/src/lib.rs:1188-1236`; `from_selector_kind`→`None` for `Point`, `geometry_ops.rs:7882-7892`). Consistent with **P0 invariant 4 / §6.2** (coordinate intent → `Value::Frame`, **not** a `SelectorKind`). |
| `FeatureTagTable` is written in production but its only reader is test-only | **TRUE** | `record` `engine_build.rs:6169`, evict `:5468` (both prod); sole reader `resolve_unique_by_tag` (`topology_selectors.rs:1185`) has **all 5 callers** `#[test]` (`:2227,:2283,:2347,:2414,:2606`, inside `#[cfg(test)] mod tests :1558`). Zero prod callers. |
| `TopologyAttributeTable` supersedes it; lacks `step_kind`/`source_span` | **TRUE** | Def `geometry.rs:3938`; stores `TopologyAttribute {feature_id, role, local_index, user_label, mod_history}` — no `step_kind`/`source_span`. Live read `engine_build.rs:6462`. |
| Deleting `FeatureTagTable` is behavior-neutral | **TRUE** | No prod reader consumes any written entry; values evicted/abandoned unread. |
| `LeafQuery::Named` is a no-op, reachable from `.ri` | **TRUE** | Arm `topology_selectors.rs:1501-1517` returns `Ok(Vec::new())` + `TopologyTagStale`; does NOT call `resolve_unique_by_tag`. Built by `face()/edge()/solid_body()/vertex()` via `eval_named_leaf_selector_ctor` (`geometry_ops.rs:5287`) → `face(b,"top")` silently returns nothing. (The arm's "unreachable from .ri" comment is **stale** — breadcrumb for Thread C.) |
| `TopologyAttribute.user_label` is dead | **TRUE** (P0 §2) | `geometry.rs:3903`; `None` at every production seeder; the resolver read-branch never matches (P0 §2 / findings §5). |
| `@face("name")` resolves via a separate ROLE path (kept-but-deprecated) | **TRUE** | `cap_kind_translation` 5-keyword role dict (`geometry_ops.rs:8114`) + `resolve_unique_by_attribute` over `TopologyAttributeTable`, via `post_process_ad_hoc_selectors` (`engine_build.rs:8160`). Deprecated by P0 D5 / 4815 (prose), not removed by P2. |

Every assumed capability verified present (or, for deletions, verified absent-of-consumers). No
unverified substrate remains.

## 3. Resolved design decisions (P2 implements P0's §3 contract)

| # | Decision | Rationale / source |
|---|---|---|
| **D1** | **Thread A implements P0 D2: de-duplicate the two `SelectorKind` enums under the manifold-dimensionality framing.** The canonical `reify_core::ty::SelectorKind {Vertex,Edge,Face,Body}` **stays in `reify-core`** (P0 fixed location); P2 de-collides the duplicate `reify_ir::expr::SelectorKind {Face,Point,Edge}` against it. `@face`/`@edge` map to the canonical 2-/1-manifold kinds; **`@point` is the coordinate intent → `Value::Frame`, NOT a `SelectorKind` member** (P0 invariant 4 / §6.2). | P0 §3 D2: "P0 fixes the framing + location; **P2 does the de-duplication mechanics.**" The substrate (§2) confirms `@point` is a frame, not a vertex. |
| **D2** | **Thread B (P0-independent): delete `FeatureTagTable` outright** — `FeatureTagTable` + `FeatureTag` + `resolve_unique_by_tag` + the dead `record`(`:6169`)/`remove`(`:5468`) path + the 5 `#[test]` callers (+ test writers `:16189`/`:16295`). **Do NOT fold** `step_kind`/`source_span`. | (Leo, 2026-06-24.) Verified zero prod reader → zero behavior change. Folding the diagnostic fields into the live `TopologyAttribute` with no consumer recreates the write-only-dead antipattern this thread retires; op-level provenance, if ever wanted, is P1/P3 with a named consumer (reconstructable from `feature_id`+`Role`). |
| **D3** | **Thread C implements P0 D4: drop user-labels in the substrate-entangled code P0 assigned to P2.** Remove the `TopologyAttribute.user_label` field (`geometry.rs:3903`), the `topology_attribute_resolver` `user_label` query-branch, and the user-string `LeafQuery::Named` no-op semantics. Surviving name resolution (the **role** intent `@face("top")`/`face(b,"top")`) routes through the **one** role/attribute resolver (P0 invariant 5), **not** the dead string path. | P0 §3 D4 + §8 + §10: user-labels dropped; P0 took only the isolated orphan-helper half (4814/δ) and **assigned P2** the field + resolver-branch + `Named` removal, "coordinated with /unblock 3523." P0 §7: P0 fixes the *fate*; **execution is P2 Thread C.** |
| **D4** | **`Named` variant disposition is a coordinated mechanics call, not pre-decided here.** Whether `LeafQuery::Named` is removed entirely (re-routing the named-leaf constructors) or retained only for the role-keyword case is decided at implement time **after checking the live state of task 3523** (which owns the `Named` leaf-predicate substrate). P2 must NOT touch 3523/esc-3523-75/76 or the substrate they own; Thread C lands after 3523's work and reconciles. | Brief + P0 §7 "coordinate, don't collide." Hard-removing substrate 3523 is mid-registering would collide. |

## 4. Scope — three threads

### Thread A — De-duplicate the `@`-family `SelectorKind` *(implements P0 D2; gated on P0 4811/α)*
P0 4811/α reframes `reify_core::ty::SelectorKind` as manifold-dimensionality + adds the `RegionRef`
alias. Once that lands, P2 de-collides the duplicate `reify_ir::expr::SelectorKind`: re-express the
`@`-family so `@face`/`@edge` reference the canonical dimensionality kinds and `@point` is dispatched
as the coordinate→`Value::Frame` intent (no `SelectorKind` needed — it is not a region selection).
The name collision (two `enum SelectorKind`) is gone. The exact form (rename the `@`-discriminant to
e.g. `AdHocSelectorKind`/`AtFormKind`, or fold its Face/Edge into a view over the canonical kind) is
implement-time mechanics; **honor P0 invariant 4** (`@point` ≠ a `SelectorKind`).
*Breadcrumb:* P0 D2's prose phrase "the `@`-family `Point` is a 0-D region = `Vertex`" is superseded
by its own **invariant 4 / §6.2** (`@point → Value::Frame`); implement to the invariant.

### Thread B — Retire the write-only-dead `FeatureTagTable` *(landable now; P0-independent)*
Delete `FeatureTag`, `FeatureTagTable` (`crates/reify-ir/src/geometry.rs:3578-3634`), the production
write/evict path (`engine_build.rs:6169`/`:5468`), the sole reader `resolve_unique_by_tag`
(`topology_selectors.rs:1185`) + its 5 `#[test]` callers (+ test writers `:16189`/`:16295`). Net
production behavior change: **none** (§2). BRE acquires the full ripple (re-exports; whether
`StepKind`/`SourceSpan` become newly-dead and removable) before editing.

### Thread C — Drop user-labels: field + resolver-branch + `Named` user-string semantics *(implements P0 D4; gated on P0 4814/δ; coordinate 3523)*
Execute the substrate-entangled half of P0 D4 (P0 §8/§10): remove `TopologyAttribute.user_label`, the
resolver's `user_label` query-branch, and the dead user-string `LeafQuery::Named` semantics; collapse
surviving name resolution onto the single role/attribute resolver (P0 invariant 5). **Coordinate with
`/unblock 3523`** (shared `topology_selectors.rs` `Named` substrate); land after 3523's leaf-predicate
work; **do not touch 3523/esc-3523-75/76.** `face(b,"top")`/`@face("top")` role-keyword resolution
remains (deprecated by P0 D5, not removed here).

## 5. Out of scope

- The region-reference model, the `@`-family's *form decision*, Named's *fate decision*, and the
  canonical-enum *location* → **P0** (committed; P2 implements its slice).
- `FeatureId`/`Feature` typing + fallible codec → **P1**. Op-level provenance (`step_kind`/
  `source_span`), if resurrected, lands there/in P3 **with a consumer** — not folded here (D2).
- Provenance/`feature()` query surface → **P3**. Region-ref ⇄ FEA-target unification (incl. the FEA
  `target:` string namespace collapse) → **P4**.
- The isolated user-label orphan helpers `has_user_label`/`user_label_eq` → **P0 task δ (4814)** —
  Thread C does **not** re-do them; it owns only the field + resolver-branch + `Named` semantics.
- The role-keyword `@face("top")` surface itself → **deprecated by P0 D5 / 4815** (prose), kept
  working; P2 does not remove it.

## 6. Cross-PRD relationship (G4) — live P0 task IDs

| Other PRD / task | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `./P0-…` **task 4811 (α)** | P2 Thread A consumes | `SelectorKind` reframe + `RegionRef` alias + location (D2) | P0 produces; **P2 de-dups** | **dep:** τA `depends_on` 4811 (pending) |
| `./P0-…` **task 4814 (δ)** | P2 Thread C consumes | isolated user-label orphan-helper removal lands first | P0 (δ) / **P2 (entangled half)** | **dep:** τC `depends_on` 4814 (pending) |
| `./P0-…` **task 4812 (β)** | P2 Thread C coordinates | both edit `topology_selectors.rs` resolver region path | P0 (β) | **soft-dep:** τC after 4812 to avoid file churn (pending) |
| `./P1-structured-featureid-feature-value` | P2 hands off | `step_kind`/`source_span`, if resurrected, with a consumer | P1/P3 | n/a — P2 deletes, no fold (D2) |
| `docs/prds/topology-selector-value-type.md` (done) | P2 builds on | `SelectorKind`, `LeafQuery`, `resolve()` substrate | predecessor (done) | wired |
| tasks **3523**/**4759** (`/unblock 3523`) | Thread C coordinates | shared `topology_selectors.rs` `Named` substrate | 3523/4759 | **coordinate (soft) — land τC after; DO NOT TOUCH 3523** |
| contested pair `topology-selectors ↔ persistent-naming-v2` (overlay G4 #3) | resolved upstream | namespace collapse | **P0** (D1/D4) | resolved by the convergence |

No **new** contested-ownership pair introduced; the known one is resolved by P0's namespace collapse.

## 7. Pre-conditions for activating

All three threads are concretely specified (P0's decisions are committed) and **filed `pending`,
dependency-gated** — the scheduler holds each behind its deps (no deferred bookmarks needed):

- **Thread B (τB):** no prereq — independent, Wave 1.
- **Thread A (τA):** `depends_on` **4811** (P0α reframe + `RegionRef` alias). Scheduler dispatches τA
  once 4811 is `done`.
- **Thread C (τC):** `depends_on` **4814** (P0δ isolated half) and soft-after **4812** (P0β resolver
  region path); **soft-coordinate with 3523/4759** — at dispatch, the architect confirms 3523's live
  state before editing the `Named` substrate (D4). A hard `depends_on 3523` edge is the decompose
  session's call only if 3523 is healthy (it has open escalations — avoid starving τC behind it).

## 8. Decomposition plan (G2 signals drafted; hard check at decompose)

Approach **B** (not B+H): no new integration seam (Thread B deletes; A de-dups; C removes a dead
namespace). The H-component reduces to per-thread *no-behavior-change* equivalence checks (each
thread's own signal). Active blast radius ≤ ~3 crates per thread.

- **τB — Retire `FeatureTagTable` (Thread B).** *Leaf / pending / independent.* *Modules:* reify-ir,
  reify-eval. *User-observable signal:* full `cargo nextest` suite green post-deletion (incl. the
  realization/selector/FEA-selector tests exercising `execute_realization_ops` — proving zero
  behavior change) **and** `grep -rn 'FeatureTagTable\|\bFeatureTag\b\|resolve_unique_by_tag' crates/`
  returns 0 outside removed lines. *Consumer:* converged attribute substrate. *G6:* "zero behavior
  change" achievable **by construction** — verified no prod reader (§2). *`metadata.files`:* the 3
  verified core files (`reify-ir/src/geometry.rs`, `reify-eval/src/engine_build.rs`,
  `reify-eval/src/topology_selectors.rs`); BRE acquires re-export/`StepKind` ripple.

- **τA — De-duplicate the `@`-family `SelectorKind` (Thread A).** *Leaf / pending / `depends_on`
  4811.* *Modules:* reify-ir + reify-compiler + reify-expr + reify-eval (footprint depends on chosen
  form → `metadata.files = []`). *User-observable signal:* a committed `.ri` fixture exercising
  `@face`/`@point`/`@edge` **and** `faces()/edges()/vertices()` `reify eval`s identically to a
  pre-change baseline (`@point` still → `Value::Frame`); only one `enum SelectorKind` remains
  (`git grep "enum SelectorKind" crates/` = 1). *Consumer:* P3/P4 + reframed reserved type names.
  *G6:* identical-output is by-construction (de-collision changes no logic); honor P0 invariant 4.

- **τC — Drop user-labels: field + resolver-branch + `Named` semantics (Thread C).** *Leaf / pending
  / `depends_on` 4814 (soft-after 4812; coordinate 3523).* *Modules:* reify-ir
  (`geometry.rs` user_label field) + reify-eval (`topology_attribute_resolver`, `topology_selectors.rs`
  `Named` arm) → `metadata.files = []` (3523-coordination footprint resolved at dispatch).
  *User-observable signal:* `reify-audit` reports the user-label namespace removed (the C-10 cluster's
  field/resolver/Named members gone, complementing P0 δ's helper removal); `face(b,"top")`/`@face("top")`
  role-keyword resolution still resolves to the same handles on a committed fixture (no regression to
  the deprecated-but-live role path); full `cargo nextest` + `reify-audit` green. *Consumer:* the
  single region resolver (P0 invariant 5), P3/P4. *G6:* "no role-path regression" binds against the
  live `@face`-via-attributes path (§2); user-string removal is by-construction (verified dead, P0 §2).

DAG: τB independent · τA → after 4811 · τC → after 4814 (soft-after 4812; soft-coordinate 3523).
All three filed `pending` together; the scheduler gates each behind its deps.

## 9. Open questions (tactical — non-blocking)

1. **Thread B ripple extent.** Whether `StepKind`/`SourceSpan` (or other `FeatureTag`-only types)
   become newly-dead and removable, and which re-export sites need touching — BRE acquires at edit
   time (τB names the 3 verified core files only).
2. **Thread A `@`-discriminant name/form** (if rename) — `AdHocSelectorKind` / `AtFormKind` /
   fold-into-view. Implement-time mechanics under P0 D2's framing; must keep `@point ≠ SelectorKind`.
3. **Thread C `Named`-variant disposition** (remove entirely vs retain role-only) — decided at
   dispatch after confirming 3523's live state (D4); whether τC takes a hard `depends_on 3523` edge
   is the decompose/architect call given 3523's open escalations.
