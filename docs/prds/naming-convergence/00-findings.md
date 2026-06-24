# Naming & Selection Convergence — Design Findings (2026-06-24)

> Shared evidence base for the naming-convergence PRD program (P0–P4). Distilled from a
> five-way parallel deep-dive (syntax, type-model, layer-violation, composition/overlap,
> dependents/removal) run during a `/prd` design-exploration session (Leo + Claude,
> 2026-06-24). Every brief in this directory cites this doc so no downstream `/prd` session
> re-derives the analysis. Line numbers are accurate at time of writing — verify against
> current `main` (each PRD's G3 substrate check does this).

## Origin

Spawned from a `/prd` session for the `persistent-naming-v2` user-label + feature-provenance
+ user-label-selectors charter (itself from `/unblock 3523`, esc-3523-75/76). Substrate
verification PASSED — but Leo's hunch that "this whole corner isn't as well thought-out as the
rest of the language" was investigated and **confirmed across all five axes**. Rather than
build more onto an unconverged island, the decision (Leo, 2026-06-24) was **convergence-first**:
treat naming/selection as a pre-1.0 design-convergence program, sequence the design (P0) ahead
of the surfaces (P3/P4), and land the foundation refactors (P1/P2) in parallel.

**Do NOT touch task 3523 or esc-3523-75/76** — the `/unblock 3523` session owns them. Relevant
hand-off for that session: `LeafQuery::Named` (`crates/reify-eval/src/topology_selectors.rs:1501`)
is a no-op stub; that is the substrate its leaf-predicate-registration work interacts with.

## The three-layer reframe (the load-bearing distinction)

The charter bundled three things as one feature. They are at three different maturity levels;
keeping them distinct is what makes the design tractable:

| Layer | What it is | Production status | Verdict |
|---|---|---|---|
| **Feature-provenance** (`feature_id`, `role`, `local_index`, `mod_history`) | the construction-history attribute substrate | **LIVE & populated** with real values | Sound, load-bearing — but its query selectors are orphaned (no `.ri` surface) |
| **User-labels** (`user_label: Option<String>`) | user-supplied face/edge names | **DEAD** — `None` at every production seeder; the read branch never matches | Speculative; removable in ~150–200 LOC with **zero behavior change**; overlaps existing features |
| **Attribute-table substrate** (`TopologyAttributeTable` + propagation) | the storage + OCCT-history propagation | LIVE but messy (two tables, one job) | Load-bearing; needs consolidation |

Key consequence: **feature-provenance is the value; user-labels-as-strings is not.** `@face("top")`
works *today* via the **role** branch (`cap_kind_translation`, `crates/reify-eval/src/geometry_ops.rs:8114`,
the fixed dictionary top/bottom/start/end/side), **not** via user-labels. Every *planned*
consumer (#4637 cross-kernel attrs, #2952 mesh-morph warm-start, shells mid-surface, FEA
selector migration) depends on provenance/substrate; **none** depends on user-labels.

## The five hunches, scored

### 1. Syntax non-uniformity — **Serious**
- `@` is **triple-overloaded**: prefix annotation (`@test`/`@optimized`, `tree-sitter-reify/grammar.js:1703`),
  infix ad-hoc selector (`body @ face("top")`, `grammar.js:1563`), ad-hoc port in `connect`
  (spec §6.1.3). Parses unambiguously but reads as one glyph meaning unrelated things.
- **Names-as-strings is non-uniform with the whole language.** Every other structural name is a
  first-class scoped `identifier` (`grammar.js:1674`, uniform `field('name', $.identifier)` for
  let/param/fn/struct/trait/type/enum/unit/port). Only selection uses **string literals as
  resolvable keys**, with none of an identifier's scope/resolution guarantees.
- The **write side does not exist**: there is no surface syntax to *give* a face a name;
  `face("top")` looks up a magic string in a hardcoded English dictionary.
- Both contradict the spec's own §1.3 priority principles **#1 Regularity** and **#4 keywords
  over symbols**. The planned v2 "selector vocabulary" (`docs/prds/v0_2/persistent-naming-v2.md:81-89`)
  doubles down with a sigil zoo (`+X`, `>>Y`, `%Plane`, `#X`).

### 2. Stringly-typed model — **Real**
- Worst offender: **`FeatureId(String)`** (`crates/reify-ir/src/geometry.rs:3653`). It lossily
  flattens structured `RealizationNodeId { entity: String, index: u32 }`
  (`crates/reify-core/src/identity.rs:163`) via `.to_string()` (`:3682`). No `entity()`/`index()`
  accessors, no `FromStr`, nothing parses it back. Derived ids are raw `format!` concat
  (`derived_mid_surface`, `:3672`).
- **Smoking gun** — the on-disk codec (`crates/reify-shell-extract/src/result.rs:337-356`):
  `role` gets a pinned, *fallible* `u8` codec (`role_from_u8` rejects unknown discriminants,
  `:504`) while `feature_id` is opaque *string passthrough* (`:539`/`:556`, no validation). Same
  function, two concepts, opposite rigor.
- The right move is already in-tree: **`Value::GeometryHandle` keeps the structured
  `RealizationNodeId`** (`crates/reify-ir/src/value.rs:1042`). A new `Value::Feature` should
  mirror that, not promote the flattened string.
- `Role`/`SelectorKind` are *correctly* closed enums — proof the team has the discipline and
  selectively skipped it for `FeatureId`. `user_label`'s equality policy ("exact, case-sensitive,
  no-trim") lives in a comment, not a `Label` newtype.

### 3. Unique-vs-set semantics — **Real / ill-defined**
- Labels are storable non-uniquely, but `@face` read demands uniqueness
  (`AttributeResolution::AmbiguousAfterSplit`, `crates/reify-eval/src/topology_attribute_resolver.rs:114`)
  while `user_label_eq` returns a set (`Vec<GeometryHandleId>`). No single defined semantics.

### 4. Layer violation (B-rep coupling) — **Serious (the deep one)**
- Reify **explicitly claims** representation-independence: `docs/reify-implementation-architecture.md`
  §1.1 ("no privileged geometric representation") and §10.2 ("if the language exposed B-rep
  topology, it would be impossible to back the same type with an SDF kernel").
- Yet topology nouns are first-class in the **core type system**: `SelectorKind {Face,Edge,Body,Vertex}`
  in `crates/reify-core/src/ty.rs:39`; reserved type names `FaceSelector`/`EdgeSelector`/`BodySelector`
  (`crates/reify-compiler/src/type_resolution.rs:578-582`, spec §8.12).
- **The abstraction runs backwards in code:** the "kernel-agnostic" `GeometryKernel` trait is
  written in OCCT vocabulary — `extract_faces`/`extract_edges`/`extract_vertices` documented as
  `TopExp::MapShapes(.., TopAbs_FACE)` (`crates/reify-ir/src/geometry.rs:3194-3239`). A neutral
  `ReprKind {BRep,Mesh,Sdf,Voxel,VolumeMesh}` exists (`geometry.rs:190`) but topology is **not**
  routed through it.
- **The mesh kernel fakes B-rep:** Manifold synthesizes "faces" by coalescing coplanar triangles
  (`crates/reify-kernel-manifold/src/kernel.rs:729-761`) — a box is 6 faces on OCCT, 12 on
  Manifold (`docs/reify-stdlib-reference.md` §3.9). 3 of 4 kernels `Err` on selectors. The
  cross-kernel `KernelAttributeHook::propagate_attributes` is a no-op (never writes the table,
  `kernel.rs:891-953`). Persistent naming is **effectively OCCT-only**.
- **`@face("top")` is non-functional on every kernel** (even OCCT): `LeafQuery::Named` returns
  empty + a `TopologyTagStale` warning (`crates/reify-eval/src/topology_selectors.rs:1501-1517`).
- **The road not taken:** FEA already uses *intent-named regions* — `PointLoad(point:"tip")`,
  `FixedSupport(target:"root")` — a representation-neutral "designate a region by intent" model.
  But it's string-typed, FEA-only, and disconnected from selectors (`validate_selector_target`
  rejects `Value::Selector`/`Value::Frame`, `crates/reify-stdlib/src/helpers.rs:214`).

### 5. Implementation cleanliness — **Poor / split-brain**
- **Two `SelectorKind` enums:** `reify_core::ty::SelectorKind` {Face,Edge,Body,Vertex}
  (`ty.rs:39`, function-call family) vs `reify_ir::expr::SelectorKind` {Face,Point,Edge}
  (`crates/reify-ir/src/expr.rs:16`, `@` family). Same name, different membership.
- **Two return types for "select a face":** `face(body,"top")` → `Value::Selector(Face)` (a set);
  `body @ face("top")` → `Value::Frame` (a pose). Same word, opposite semantics. (These are
  arguably complementary by design — but the collision of names is confusing.)
- **Two `@` eval pipelines:** `@point(x,y,z)` resolves eagerly/kernel-free
  (`crates/reify-expr/src/lib.rs:1194`); `@face`/`@edge` emit `Value::Undef` placeholders patched
  by an engine post-process (`post_process_ad_hoc_selectors`, `engine_build.rs:7764`) — an
  order-dependent hazard.
- **Two attribute tables, one job:** `FeatureTagTable` (v0.1, `geometry.rs:3578`/`3593`) is
  **write-only dead** in production — written at `engine_build.rs:6169` but its reader
  `resolve_unique_by_tag` has zero prod callers (test-only). `TopologyAttributeTable` (v0.2,
  `geometry.rs:3938`) supersedes it (docstring "mirrors the FeatureTagTable shape", `:3934`) but
  the v0.1 path was never removed.
- **Four disconnected naming namespaces:** (1) `@face` canonical-role keywords, (2) dead
  `user_label`, (3) no-op `LeafQuery::Named`, (4) FEA `target:` strings. No shared resolver.
- Orphan helpers in `crates/reify-eval/src/selector_vocabulary_v2.rs` (19 fns total, ~11 wired):
  `created_by_feature` (`:700`), `split_by_feature` (`:733`), `has_user_label` (`:774`),
  `user_label_eq` (`:799`) — all test-only callers (the C-10 orphan cluster).

## Overlap & alternatives (composition dive)

- A plain `let lid = faces_by_normal(body,+Z)` already binds a **re-eval-stable** name: the
  `SelectorValue` content-hashes its query tree and excludes the ephemeral `kernel_handle`
  (`value.rs:407`), so it re-resolves every eval. The **only** thing it can't do is survive a
  topology **split** (predicate degenerates; `[0]` silently rebinds) — and that gap is served by
  feature-provenance's `mod_history` (`AmbiguousAfterSplit`), **not** by string labels.
- Ranked alternatives to string user-labels: **(a)** `let` + predicate selectors [exists, best
  for ~90%]; **(c)** feature-provenance with a query surface [highest-value missing piece];
  **(d)** a structured first-class reference value [right shape if labels are wanted]; **(b)**
  predicates only [minimalist]; **(e)** string labels as originally chartered [not recommended].

## Program shape (P0–P4)

- **P0** (keystone, design-first B+H): region-reference & kernel-topology layer model. Resolves
  the layer boundary, per-kernel resolution, the 4-namespace unification, and the **fate/form of
  user-labels**. Everything user-facing depends on it.
- **P1** (foundation, independent): structured `FeatureId` + first-class `Feature` value +
  fallible codec.
- **P2** (foundation, mostly independent): unify the two `SelectorKind`s; retire `FeatureTagTable`;
  resolve the `LeafQuery::Named` stub (the Named-fate part is gated on P0).
- **P3** (surface; deps P0+P1): feature-provenance query surface (`feature()` +
  `created_by_feature`/`split_by_feature`) — the original charter D2 value.
- **P4** (surface; deps P0): unify region references across selectors & FEA targets.

Waves: **Wave 1** = P0, P1, P2 (concurrent); **Wave 2** = P3, P4 (after P0 lands).
