# P0 — Geometry region-reference model & kernel-topology layer boundary

**Status:** contract (design-first, **B + H**). Version-agnostic language foundation. **Keystone** of the
naming-convergence program (P0–P4). Authored 2026-06-24 in an interactive `/prd` design session
(Leo + Claude). Resolves the four central design questions in
`./P0-region-reference-layer-model.brief.md`; evidence base is `./00-findings.md` (cite it; do not
re-derive). The four load-bearing decisions in §3 were approved by Leo before queueing tasks.

> **Do NOT touch task 3523 or esc-3523-75/76.** The `/unblock 3523` session owns the
> `LeafQuery::Named` leaf-predicate substrate; P0 coordinates with it (§7) and does **not** edit the
> `Named` variant.

This document is the **contract** the rest of the program implements. P1 (structured `FeatureId`),
P2 (selector-substrate convergence), P3 (provenance query surface), and P4 (FEA-target unification)
consume the decisions and the type framing fixed here. Their *mechanics* are out of scope (§10);
their *seam ownership* is in §8.

---

## §0 — Purpose and the convergence it forces

Reify claims **representation-independence**: `docs/reify-implementation-architecture.md` §1.1
("no privileged geometric representation") and §10.2 ("if the language exposed B-rep topology, it
would be impossible to back the same type with an SDF kernel"). Yet the naming/selection corner
violates that claim four ways (`./00-findings.md` §4, §5): B-rep topology nouns are first-class in
the rep-agnostic core type system, the "kernel-agnostic" kernel trait is written in OCCT vocabulary,
the mesh kernel *fakes* B-rep faces, and there are **four disconnected naming namespaces with no
shared resolver**. The flagship `@face("top")` is non-functional on every kernel.

This PRD converges the **conceptual model** before any more naming/selection surface is built. The
decisive finding from the G3 substrate verification (2026-06-24, current `main`) is that **the
convergence is already ~80 % present in the substrate** — the work is to *declare and complete* it,
not invent a new type:

- **`Value::Selector(SelectorValue)`** (`crates/reify-ir/src/value.rs:1056`, task 4116) is already a
  representation-independent, **content-hash-stable** (the hash **excludes the ephemeral
  `kernel_handle`** — `hash_ghr`, `value.rs:724`), **deferred query spec** resolved per-kernel at
  solve time. That *is* a region reference.
- **`SelectorKind`** (`crates/reify-core/src/ty.rs:39`) already self-documents as **0/1/2/3-manifold
  dimensionality** (`Vertex=0, Edge=1, Face=2, Body=3` — `dimensionality()`, `ty.rs:53`), a
  *mathematical* property compatible with §3.3.2's opaque `Surface`/`Curve`/`Point` types — not a
  B-rep `TopAbs_*` noun.
- The **fail-closed per-representation gate** (`gate_query_capability` +
  `DiagnosticCode::QueryNotSupportedOnRepr` + `Value::Undef`, `crates/reify-eval/src/geometry_ops.rs:57-143`,
  `crates/reify-core/src/diagnostics.rs:1402`) is **live production code** — but wired for *geometry
  queries* (`curvature`, `edge_length`), **not** for topology/region resolution. Extending it to
  region resolution is the foundational slice.

So the model is **not** topology-first (ruled out by §10.2) and **not** a new parallel type. It is:
**the existing `Selector` value, declared canonical and reframed as a representation-aware region
reference whose `kind` is manifold dimensionality, resolved per-kernel, fail-closed off-capability.**

---

## §1 — Goal (what observable behaviour this program produces)

P0 itself produces a **contract** plus a **foundational vertical slice** with one user-observable
signal: **resolving a region reference over a representation that cannot answer it fails *closed and
diagnosable*** — `reify eval`/`check` emits `E_QUERY_NOT_SUPPORTED_ON_REPR` and the cell stays
`Value::Undef`, instead of today's generic kernel error or a silently-faked result. The rest of the
program's user-observable surface lands in P3 (`feature()` provenance queries) and P4 (FEA loads
targeting named regions) — both gated on this contract.

The **consumer** of P0's foundational mechanisms is named and concrete (G1): the existing selector
resolution path (`crates/reify-eval/src/topology_selectors.rs`), the fillet/chamfer/datum/`connect`
target consumers, and downstream PRDs **P2/P3/P4** (§8). P0 introduces no orphan mechanism — it
*completes* an existing in-engine seam (the query-capability gate), it does not add a new one.

---

## §2 — Background (the five hunches, condensed)

`./00-findings.md` is the authoritative analysis; the load-bearing facts, re-verified against
current `main` at authoring:

| Finding | Evidence (current `main`) |
|---|---|
| Topology nouns in rep-agnostic core (§10.2 violation) | `SelectorKind{Face,Edge,Body,Vertex}` in `reify-core/src/ty.rs:39`; reserved `FaceSelector`/`EdgeSelector`/`BodySelector`/`VertexSelector` + bare `Selector`→`AnySelector` in `reify-compiler/src/type_resolution.rs:578-589` |
| Kernel trait written in OCCT vocabulary | `GeometryKernel::extract_faces`/`extract_edges`/`extract_vertices` documented as `TopExp::MapShapes(.., TopAbs_FACE/EDGE/VERTEX)` (`reify-ir/src/geometry.rs:3194/3213/3232`); default impl `Err`s |
| 3 of 5 kernels `Err` on topology; mesh kernel fakes faces | OCCT + Manifold resolve; Fidget/OpenVDB/Gmsh use the trait-default `Err`. Manifold *synthesizes* faces by coalescing coplanar triangles (`reify-kernel-manifold/src/kernel.rs:729-761`) — a box is 6 faces on OCCT, 6 coalesced groups on Manifold but tolerance-fragile; the cross-kernel `propagate_attributes` hook **computes provenance but never writes the table** (`kernel.rs:891-954`, deferred to task 4262) |
| `@face("top")` non-functional everywhere | resolves via the `cap_kind_translation` role dict {top,bottom,start,end,side} (`geometry_ops.rs:8114`) → seeds `LeafQuery::Named`, whose resolver arm returns empty + `TopologyTagStale` (`topology_selectors.rs:1501-1517`) |
| User-labels dead | `TopologyAttribute.user_label: Option<String>` (`geometry.rs:3903`) is `None` at every production seeder; the resolver read-branch never matches; `has_user_label`/`user_label_eq` are test-only orphans (`selector_vocabulary_v2.rs:774/799`, the C-10 cluster) |
| Four disconnected namespaces, no shared resolver | (1) `@face` role keywords, (2) dead `user_label`, (3) no-op `LeafQuery::Named`, (4) FEA `target:` strings — `validate_selector_target` accepts only `Value::Map`/`Value::String`, **rejects** `Value::Selector`/`Value::Frame` (`reify-stdlib/src/helpers.rs:214`) |
| The road not taken (rep-neutral intent) | FEA `PointLoad(point:"tip")` / `FixedSupport(target:"root")` already names regions **by intent** — string-typed, FEA-only, disconnected |

`Value::Selector`/`Type::Selector`/`SelectorKind` (tasks 4116/4117) and the structured
`Value::GeometryHandle` (`value.rs:1042`, keeps the structured `RealizationNodeId`) are the in-tree
precedents this contract builds on.

---

## §3 — Resolved design decisions (the four keystone resolutions)

### D1 — The canonical region reference is the existing `Selector`, reframed (brief Q1, Q4)

There is **one** canonical "reference to a sub-region of geometry": **`Value::Selector(SelectorValue)`**,
hereby declared the **canonical region reference** and reframed in prose/doc as such (a `RegionRef`
alias makes the canonical name visible in code — §4). It is **intent/predicate-first**: a region is
named by *intent* (a predicate, a role, a provenance feature, or a coordinate), carried as a deferred
query spec, and **resolved per-kernel at solve time**. Topology selection is **one resolution
strategy**, not the model's foundation.

- **Why not topology-first?** Ruled out by architecture §10.2 — exposing B-rep topology as the
  primary model makes SDF/voxel backing impossible. A region-by-intent resolves on *any* capable
  representation.
- **Why reframe `Selector` and not build a new `Region`?** `Selector` is already a content-hash-stable,
  kernel-handle-excluded, rep-independent deferred query resolved per-kernel — i.e. already the right
  shape. A parallel `Region` type would force a migration of every consumer for no semantic gain.
  The reframe **broadens** `Selector`'s admitted intents to subsume the other three namespaces
  (role-keyword, FEA-intent, provenance) and, where a *pose* is the right input (point-load-at-frame),
  composes with `Value::Frame` (P4 decides the FEA pose-vs-set split).
- **"Top face" is an intent, not a noun.** A `+Z` direction predicate (`faces_by_normal(body, +Z)`,
  existing) resolves on OCCT *and* mesh; a *role*/provenance intent ("the cap created by this
  extrude") is construction-history-dependent and resolves only where history exists (fail-closed
  elsewhere — D3). Both are region references; they differ only in resolution strategy.

A plain `let lid = faces_by_normal(body, +Z)` already binds a **re-eval-stable name** (the selector
content-hashes its query tree, excluding the ephemeral handle). **That is the language's naming
mechanism.** No new namespace is introduced.

### D2 — Topology nouns stay in core, reframed as manifold dimensionality (brief Q2)

`SelectorKind{Face,Edge,Body,Vertex}` **stays in `reify-core`**, but its **semantics are rewritten**
from "B-rep topology noun" to "**manifold dimensionality of a sub-region**" (0/1/2/3-D — already how
`dimensionality()` defines it). This satisfies §10.2: a "2-manifold sub-region" is a *mathematical*
concept (cf. §3.3.2's opaque `Surface`/`Curve`/`Point`), **not** a B-rep `TopAbs_FACE`. The B-rep
mapping (`TopExp`/`TopAbs` enumeration) stays in the **kernel layer**, where `extract_faces` already
lives.

- **Why reframe-in-core, not banish to the kernel layer?** Banishing the kind out of `reify-core`
  is a 21-file refactor (the verified blast radius) touching every consumer + the reserved type names
  + compiler resolution, for a purity gain the dimensionality reframe already secures. The reframe is
  the minimum change that makes the layer claim true.
- **Consequence for the reserved type names (spec §8.12).** `FaceSelector`/`EdgeSelector`/`BodySelector`/`VertexSelector`
  are retained as **dimensionality-typed views** of the canonical `Selector` (face = 2-manifold
  region selector), not as B-rep type names. `Selector` (bare) → `AnySelector` is the
  kind-agnostic region reference. Prose corrected in §11 / task ε.
- **Consequence for P2.** P2 unifies the two `SelectorKind` enums (`reify_core::ty::SelectorKind`
  {Face,Edge,Body,Vertex} vs `reify_ir::expr::SelectorKind` {Face,Point,Edge}) **under this
  rep-neutral framing**: the `@`-family `Point` is a 0-D region = `Vertex` (a coordinate *frame* is a
  distinct `Value::Frame`, not a `SelectorKind`). P0 fixes the **framing + location**; P2 does the
  de-duplication mechanics (§8, §10).

### D3 — Per-kernel resolution is fail-closed, reusing the live capability gate (brief Q3)

A region reference **means** "the sub-region(s) of geometry satisfying this intent, **resolved against
whatever representation the body is realized as**." Resolution is **fail-closed**: when the realized
representation cannot answer the intent, the resolver emits the **existing**
`E_QUERY_NOT_SUPPORTED_ON_REPR` diagnostic (`DiagnosticCode::QueryNotSupportedOnRepr`) and the cell
stays `Value::Undef` — **never a silent fake, never a panic**.

This **extends a live mechanism**, not a speculative one: `gate_query_capability`
(`geometry_ops.rs:57-143`) already routes geometry *queries* per `ReprKind`, fail-closed, into
`QueryNotSupportedOnRepr`. Today topology/region resolution dispatches on `SelectorKind` → kernel
trait methods, which return a *generic* `QueryError::QueryFailed("topology extraction not supported by
this kernel")` (a kernel-internal string). The foundational slice (§9 task β) **routes region
resolution through the same capability gate**, so the failure is the structured, user-visible
fail-closed diagnostic, consistent with geometry queries. (This is the precedent the v0.3
`kernel-geometry-queries.md` §5.4 specified — now realized in code for the region path too.)

- **Predicate-intent resolves widely.** `faces_by_normal`/`faces_by_area`/`edges_at_height` are
  geometric predicates a mesh kernel *can* answer (Manifold's coplanar-coalesce is the mechanism —
  a **legitimate mesh resolution strategy**, defined here, not a fake). The tolerance-fragility of
  coalescing is a quality concern, not a layer violation; the contract names it (§5).
- **Role/provenance-intent is history-dependent.** It resolves only where a construction-history
  attribute table exists (OCCT today; Manifold once `propagate_attributes` writes the table, task
  4262); fail-closed on raw mesh/SDF/voxel with no history.
- **No representation is privileged in the model** (§10.3): dispatch reads the realized `ReprKind`
  and the kernel's registered capability; the *same* region reference may resolve on one rep and
  fail-closed on another, by design.

### D4 — User-labels are dropped (brief Q5)

User-controlled face/edge **string labels are dropped.** They are dead (`None` at every production
seeder), subsumed by `let`-bound selectors for ~90 % of cases (a content-hash-stable selector **is**
a stable, user-controlled name), and the one capability a predicate selector lacks — **stability
across a topology split** — is served by **feature-provenance `mod_history`** (`AmbiguousAfterSplit`),
**not** by strings (P3 surfaces it). This retires charter D1/D3 as a string feature.

- **User-controlled naming is retained — as a `let`-binding** (already first-class). There is **no**
  new string namespace and **no** bare-`String` label type. If a future need for user-assigned names
  *beyond* `let` is ever demonstrated, it returns as a **structured first-class reference**
  (findings alt-d), never a `String` — but that is explicitly not built here.
- **Removal ownership (§8).** P0 removes the isolated, test-only orphan helpers `has_user_label`/`user_label_eq`
  (task δ — net-zero, shrinks the C-10 cluster). The substrate-entangled removals — the
  `TopologyAttribute.user_label` field, the resolver's `user_label` query-branch, and the
  user-string `LeafQuery::Named` semantics — are **P2's** (the substrate-convergence PRD already
  touching those files), driven by this decision, coordinated with `/unblock 3523` (shared `Named`
  substrate). P0 does **not** touch the `Named` variant.

### D5 — No new surface syntax (brief Q6)

The converged surface is **existing grammar**: function-call selectors (`faces_by_normal(body, +Z)`,
`face(body, …)`, provenance `created_by_feature(…)` per P3) and the already-spec'd ad-hoc
`@region(surface, predicate)` form (spec §6.1.3). The string-key `@face("top")` and the v2 **sigil
zoo** (`+X`/`>>Y`/`%Plane`/`#X`, `docs/prds/v0_2/persistent-naming-v2.md:81-89`) are **deprecated /
not-built** — both contradict spec §1.3 #1 (Regularity) / #4 (keywords over symbols) and the
first-class-identifier convention (`grammar.js:1674`). **P0 introduces zero new productions**; the
fail-closed fixture (task β) uses `faces_by_normal(...)` / `#kernel(...)` + `faces(...)`, both
verified to parse (`tree-sitter parse --quiet`, exit 0, 2026-06-24). **G3-grammar: N/A.**

---

## §4 — Contract: the region-reference value type (B + H)

The canonical region reference is the existing `Value::Selector(SelectorValue)` /
`Type::Selector(SelectorKind)`, reframed. P0 fixes the contract; P2/P3/P4 build to it.

**Type framing (the canonical names made visible in code).**

```rust
// crates/reify-core/src/ty.rs — reframe, not redefinition.
//
// SelectorKind is the *manifold dimensionality* of a sub-region (NOT a B-rep TopAbs noun):
//   Vertex => 0-manifold, Edge => 1-manifold, Face => 2-manifold, Body => 3-manifold.
// The B-rep mapping (TopExp/TopAbs enumeration) lives in the kernel layer (GeometryKernel).
pub enum SelectorKind { Vertex, Edge, Face, Body }   // unchanged variants; reframed semantics

/// Canonical region reference (alias for clarity at the contract boundary; P0 §4 / D1).
/// A representation-aware, content-hash-stable, deferred query spec resolved per-kernel.
pub type RegionRef = /* the Selector value/type — exact alias site decided at task α */;
```

**Invariants (all MUST hold; the boundary test in §6 pins them).**

1. **Representation-independence.** A `RegionRef`'s identity (content hash) is a function of its
   **query tree only** — it excludes the ephemeral `kernel_handle` (`hash_ghr`, `value.rs:724`). The
   same `RegionRef` re-resolves stably across re-eval and across kernels.
2. **Dimensionality, not topology.** A `RegionRef` carries a `SelectorKind` = manifold dimensionality.
   No B-rep `TopAbs_*` noun appears in `reify-core` or in the language surface. (`reify-core` may not
   depend on any kernel.)
3. **Fail-closed resolution (D3).** Resolving a `RegionRef` against a representation that lacks the
   capability emits exactly one `Diagnostic::error(...).with_code(QueryNotSupportedOnRepr)` and yields
   `Value::Undef`. Never a silent empty/fake result; never a panic. (Mirrors the
   `gate_query_capability` contract: `Unsupported → None → Value::Undef`.)
4. **Resolution-strategy taxonomy.** A `RegionRef`'s intent is exactly one of: **predicate** (geometric:
   normal/area/length/height — resolves on any rep that can answer the geometric query), **role**
   (construction cap-kind — history-dependent), **provenance** (feature/split — history-dependent,
   P3), **coordinate** (`@point` → eager `Value::Frame`, kernel-free). A *pose* is `Value::Frame`,
   **not** a `RegionRef` (P4 decides where FEA accepts each).
5. **One resolver, no parallel string match.** Every namespace resolves through the *one* region
   resolution path; the FEA `target:` string namespace is collapsed onto it (P4). No consumer
   reimplements region matching.

**Layer boundary (where each concept lives).**

| Concept | Layer | Rationale |
|---|---|---|
| `RegionRef` value/type, `SelectorKind` (dimensionality) | `reify-core` / `reify-ir` (rep-agnostic) | §10.2: rep-neutral; no kernel dep |
| Resolution-strategy taxonomy (predicate/role/provenance/coordinate) | `reify-ir` query spec | rep-neutral intent |
| Per-rep capability gate (`QueryNotSupportedOnRepr`) | `reify-eval` (`gate_query_capability`) | dispatch reads realized `ReprKind` |
| B-rep `TopExp`/`TopAbs` enumeration, mesh coplanar-coalesce | kernel layer (`GeometryKernel` impls) | the only place B-rep/mesh topology vocabulary is legitimate |

---

## §5 — Per-kernel resolution contract (fail-closed; the producer-facing spec)

Each kernel's region-resolution behaviour, as the contract defines it. The boundary test (§6) pins
the producer side per row.

| Kernel | `ReprKind` | Predicate intent (normal/area/…) | Role / provenance intent | Off-capability → |
|---|---|---|---|---|
| OCCT | `BRep` | resolves (`extract_*` via `TopExp`) | resolves (history table populated) | n/a |
| Manifold | `Mesh` | resolves via **coplanar-coalesce** (defined strategy, tol-fragile — a quality note, not a fake) | fail-closed until `propagate_attributes` writes the table (task 4262) | `QueryNotSupportedOnRepr` + `Undef` |
| Fidget | `Sdf` | fail-closed | fail-closed | `QueryNotSupportedOnRepr` + `Undef` |
| OpenVDB | `Voxel` | fail-closed | fail-closed | `QueryNotSupportedOnRepr` + `Undef` |
| Gmsh | `VolumeMesh` | fail-closed | fail-closed | `QueryNotSupportedOnRepr` + `Undef` |

**Contract obligations.**
- The resolver reads the body's realized `ReprKind` and the kernel's registered capability, then
  routes through `gate_query_capability` (extended to the region path by task β). The generic
  `QueryError::QueryFailed("topology extraction not supported by this kernel")` (`geometry.rs:3194`
  default) is **replaced** on the region path by the structured fail-closed diagnostic.
- Manifold's coalesced-face resolution is a **legitimate `Mesh` predicate-resolution strategy**, not
  a B-rep emulation; the contract permits it and flags its tolerance-fragility as a documented
  quality limitation (not a correctness fork).
- A `RegionRef` that resolves on one rep and fails-closed on another is **correct by design** (D3,
  §10.3). The divergence is observable (the diagnostic), never silent.

---

## §6 — Boundary-test sketch (cross-crate; facing both ways)

Tests live in `crates/reify-eval/tests/` (engine-level) and per-module `::tests`. The seam is
between `reify-eval` (region resolution + the capability gate) and (producer side) the
kernel crates, and (consumer side) the selector / FEA-target / datum-frame / fillet-target call
sites. Both directions are pinned; this matrix is the integration-gate task's (§9 γ) observable
signal — closing G2.

### 6.1 Producer-side (reify-eval looks outward at kernels)

| Scenario | Preconditions | Postconditions |
|---|---|---|
| **BRep predicate resolves.** `let f = faces_by_normal(body, +Z)` on an OCCT-realized box. | OCCT kernel; body realized `BRep`. | `f` resolves to the +Z face set; no diagnostic. |
| **Mesh predicate resolves via coalesce.** Same selector on a Manifold-realized box. | Manifold kernel; body realized `Mesh`. | `f` resolves (coplanar-coalesce); resolution is a defined `Mesh` strategy; result non-`Undef`. |
| **SDF fail-closed.** `let f = faces(body)` on a Fidget-realized sphere (`#kernel(fidget)`). | Fidget kernel; body realized `Sdf`. | Exactly one `E_QUERY_NOT_SUPPORTED_ON_REPR` diagnostic; cell `Value::Undef`; **no panic, no silent empty**. |
| **Voxel / VolumeMesh fail-closed.** Same over OpenVDB / Gmsh realizations. | OpenVDB / Gmsh kernel. | Same fail-closed signal per rep. |
| **Role intent off-history fail-closed.** A role/cap-kind region reference resolved over a raw imported mesh (no history table). | Mesh rep, empty attribute table. | Fail-closed diagnostic + `Undef` (history-dependent intent, D3). |
| **Content-hash stability.** Re-evaluate the same `.ri` twice; compare the selector's content hash. | Deterministic inputs. | Identical hash across runs (kernel_handle excluded); the `let`-bound name is re-eval-stable. |

### 6.2 Consumer-side (call sites look inward at the region reference)

| Scenario | Preconditions | Postconditions |
|---|---|---|
| **Selector consumer.** `fillet(body, edge_ref, 2mm)` where `edge_ref` is a 1-manifold `RegionRef`. | Edge region resolves on the body's rep. | Fillet applies; a dimensionality mismatch (`Face` ref to an edge param) is a **construct-time kind error**, not a solve-time surprise. |
| **Datum / frame.** `@point(x,y,z)` consumed where a `Frame` is expected. | Coordinate intent. | Resolves eagerly to `Value::Frame` (kernel-free); a `RegionRef` (a set) is **not** accepted where a pose is required — distinct types (D1, invariant 4). |
| **FEA target (P4 seam, sketched here).** `PressureLoad(face: <a 2-manifold RegionRef>)`. | P4 bridge lands (`validate_selector_target` accepts `RegionRef`). | A 2-manifold ref is accepted; a 3-manifold (body) ref to a `face:` param is a kind error. (Boundary lives in P4; row documents the contract P4 must satisfy.) |
| **Negative — dimensionality discipline.** Pass a `Body` (3-manifold) `RegionRef` to a `face:`-typed consumer. | Typed consumer. | Construct-time kind rejection (the `FaceSelector` vs `BodySelector` distinction, reframed as 2- vs 3-manifold). |

---

## §7 — Coordination with `/unblock 3523` (the shared-substrate seam)

`LeafQuery::Named` (`reify-ir/src/value.rs:462`; no-op resolver arm `topology_selectors.rs:1501-1517`)
is the substrate the `/unblock 3523` leaf-predicate-registration work interacts with. **P0 does not
touch the `Named` variant.** P0's decision D4 determines its *fate* (the user-string raison-d'être is
gone), but the **execution** is **P2 Thread C**, which already gates on P0 and coordinates with 3523.
P0's only labels execution is the isolated orphan-helper removal (task δ), which does **not** touch
`Named`, the resolver, or any 3523-owned file. **Do NOT touch task 3523 or esc-3523-75/76.**

---

## §8 — Cross-PRD relationship (seam ownership)

P0 is the keystone: it **produces** the model + the foundational slice; P1–P4 **consume** it. Real
`add_dependency` edges to P1–P4 tasks are wired by **those PRDs' own `/prd` decompose sessions** once
P0's task IDs exist (`preferences_cross_prd_deps_real_edges`); P0 ships first.

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `./P1-structured-featureid-feature-value.brief.md` | P0 references / P1 produces | structured `FeatureId` + `Value::Feature` (the provenance value a role/provenance `RegionRef` names) | **P1** | independent (Wave 1); P0 fixes no `Feature` mechanics |
| `./P2-selector-substrate-convergence.brief.md` | P2 implements P0's decisions | `SelectorKind` unification **location + dimensionality framing** (D2); `user_label` field + resolver-branch + `LeafQuery::Named` removal (D4); `FeatureTagTable` retirement | **P2** (mechanics) / **P0** (decisions) | P2 Thread A coordinates location; Thread C gated on P0 |
| `./P3-feature-provenance-query-surface.brief.md` | P3 consumes P0 + P1 | `feature()` accessor + `created_by_feature`/`split_by_feature` provenance `RegionRef`s; the **explicit-projection** (not implicit coercion) rule is preserved | **P3** | Wave 2; gated on P0 commit + P1 type |
| `./P4-region-ref-fea-selector-unification.brief.md` | P4 consumes P0 | bridge `validate_selector_target` (`helpers.rs:214`) to accept the canonical `RegionRef` (+ `Value::Frame` where a pose is meant); collapse the FEA string namespace onto the one resolver | **P4** | Wave 2; gated on P0; P4 also owns the G4 seam with the in-flight `docs/prds/v0_6/fea-load-support-selector-migration.md` |
| `docs/prds/topology-selector-value-type.md` | P0 reframes | `Value::Selector`/`Type::Selector` (tasks 4116/4117, done) — declared the canonical region reference | **P0** | the reframe is task α |
| `/unblock 3523` (esc-3523-75/76) | coordinate, don't collide | shared `LeafQuery::Named` substrate (§7) | 3523 owns `Named` | **DO NOT TOUCH** |

No new contested-ownership pair is introduced (the three known pairs in the overlay G4 list are all
*downstream* of this convergence, which resolves them: this PRD fixes the `persistent-naming-v2 ↔
multi-kernel` and `topology-selectors ↔ persistent-naming-v2` seams by collapsing the namespaces).

---

## §9 — Decomposition plan (B + H; P0's own DAG)

Per the overlay portfolio (B vertical slice + H design-first/contracts/two-way boundary tests). Each
**leaf** names a user-observable signal; intermediates name the downstream they unlock. Greek labels
here; task IDs assigned at decompose.

### Phase 1 — Foundation reframe (intermediate)

- **Task α — Region-reference contract realized in code.** Declare `Value::Selector`/`SelectorValue`
  the canonical region reference; add the `RegionRef` alias (§4); rewrite `SelectorKind`'s doc-semantics
  to manifold-dimensionality (D2); add a breadcrumb comment at the type def citing this PRD §3/§4 and
  the deferred alternatives (banish-to-kernel-layer, new-Region-type) per
  `feedback_breadcrumb_design_alternatives_at_impl_site`.
  - **Unlocks:** β (the resolver routes the now-canonical region path), γ (boundary test asserts the
    framing). **Modules:** reify-core (ty.rs), reify-ir (value.rs).
  - **Signal (intermediate):** the `RegionRef` alias compiles and is referenced by β/γ; docstrings +
    breadcrumb present (`git grep` shows the canonical framing at the type def).

### Phase 2 — Vertical slice (leaf; the user-observable foundational signal)

- **Task β — Fail-closed region/selector resolution per representation (D3).** Route region/selector
  resolution through the live `gate_query_capability` / `QueryNotSupportedOnRepr` gate, replacing the
  generic `QueryError::QueryFailed` on the region path with the structured fail-closed diagnostic +
  `Value::Undef`.
  - **Prereqs:** α. **Modules:** reify-eval (`topology_selectors.rs`, `geometry_ops.rs`).
  - **Signal (leaf, CLI diagnostic):** an `.ri` fixture resolving a function-call selector over a
    non-BRep-realized body (`#kernel(fidget)` SDF, or OpenVDB/Gmsh) makes `reify eval`/`check` emit
    **`E_QUERY_NOT_SUPPORTED_ON_REPR`** and leaves the cell `Value::Undef` — where today it is a
    generic kernel error / silent. (G6 branch-4 rejection: the mechanism is **live** for geometry
    queries — `geometry_ops.rs:57`, `diagnostics.rs:1402` — and β extends it to the region path; γ
    pins it firing.)

### Phase 3 — Integration gate (leaf; the H two-way boundary test)

- **Task γ — Two-way region-resolution boundary test (§6).** Implement the §6 matrix facing producers
  (OCCT/Manifold resolve; Fidget/OpenVDB/Gmsh fail-closed; Manifold coalesce = defined Mesh strategy)
  and consumers (selector kind-discipline, `@point`→Frame, FEA-target contract row, dimensionality
  rejection).
  - **Prereqs:** α, β. **Modules:** reify-eval (`tests/`).
  - **Signal (leaf, CI test):** a committed boundary-test file under `crates/reify-eval/tests/` whose
    rows all pass — the integration-gate signal (G5 H / G2 escape-hatch closure).

### Phase 4 — Companion corrections

- **Task δ — Drop user-label orphan helpers (D4, isolated).** Remove `has_user_label`/`user_label_eq`
  (`selector_vocabulary_v2.rs:774/799`) + their test-only callers. Net-zero behaviour (verified dead).
  - **Prereqs:** none. **Modules:** reify-eval (`selector_vocabulary_v2.rs`).
  - **Signal (leaf, audit delta):** `reify-audit` reports the C-10 `selector_vocabulary_v2` orphan
    cluster shrunk by 2; full `cargo nextest` + `reify-audit` green. **Does NOT touch** the
    `user_label` field, the resolver, or `LeafQuery::Named` (those are P2 / §7).

- **Task ε — Spec + v0_2 PRD prose corrections (D2, D5).** Update spec §6.1.3 (canonical
  region-reference model; deprecate string-key `@face("top")`) and §8.12 (`Selector`-family reframed
  as dimensionality-typed region-reference views); mark `docs/prds/v0_2/persistent-naming-v2.md`'s
  user-label `name="..."` surface + the sigil-zoo selector-vocabulary-v2 as **superseded by this
  convergence**.
  - **Prereqs:** none. **Modules:** docs only.
  - **Signal (leaf, doc):** the edits + cross-references to this PRD are present; markdown/doc-lint
    passes.

### Dependency view

```
α ─┬─→ β ─→ γ
   └────────┘
δ  (independent)
ε  (independent)
```

P1/P2/P3/P4 wire their own `add_dependency` edges onto P0's α/β (and the §8 decisions) at their
decompose time.

---

## §10 — Out of scope (owned by sibling PRDs)

- **`FeatureId` structuring + `Value::Feature`** → **P1**.
- **`SelectorKind`-enum de-duplication mechanics, `FeatureTagTable` retirement, `user_label` field +
  resolver-branch + `LeafQuery::Named` removal** → **P2** (driven by D2/D4; Thread C coordinates with
  3523).
- **`feature()` accessor + provenance selector surface** → **P3**.
- **FEA `validate_selector_target` bridge mechanics + FEA pose-vs-set target split** → **P4** (+ its
  G4 seam with the in-flight v0.6 FEA migration PRD).
- **Manifold `propagate_attributes` table-write** (task 4262) — P0 names it as the precondition for
  Manifold role/provenance resolution; the write itself is not P0's.
- **`op_accepts_repr` production wiring** (task 4050) — adjacent; not on P0's path.
- Any new naming **surface syntax** (D5 forbids it).

---

## §11 — Open questions (tactical; not design-blocking)

1. **`RegionRef` alias site.** Whether the canonical alias lives on the `Type` side, the `Value` side,
   or both, and whether it is `pub type` or a re-export. Either is coherent; decide at task α.
2. **Fixture rep-forcing for the fail-closed signal (task β).** The exact `.ri` idiom that forces a
   body to realize as `Sdf`/`Voxel`/`VolumeMesh` (a `#kernel(fidget)` pragma vs an SDF-producing op).
   Tactical; the architect picks whatever current substrate makes the body realize off-BRep. The
   *signal* (the structured diagnostic fires) is fixed.
3. **Manifold coalesce tolerance knob.** Whether the contract should expose the coplanar-coalesce
   tolerance (`PLANE_TOL`) as a representation-tolerance input (§10.4) or leave it a kernel-internal
   default. Defer; not blocking the fail-closed model.
4. **Edge/vertex provenance result kinds (P3 concern).** Whether provenance `RegionRef`s are
   Face-only or kind-parametric — flagged in the P3 brief; P0's taxonomy (invariant 4) admits any
   dimensionality, so P0 does not constrain it.
