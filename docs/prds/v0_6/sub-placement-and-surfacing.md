# Sub Placement & Surfacing

> Hierarchical composition of structures is a core affordance of Reify. Today it carries
> needless ceremony: a parent only renders/exports geometry it *itself* lifts into a
> top-level `let` (e.g. `let placed = translate(self.bolt.body, 30mm, 0mm, 0mm)`), so a
> containment-only structure (just `sub`s + constraints) surfaces **nothing**, and an
> assembly is only visible if some structure manually re-expresses every descendant body
> in one coordinate frame. This PRD makes hierarchical placement *declarative and
> load-bearing* — the language already commits to the semantic model; we implement it.

---

## §0 — Purpose and supersession

**Purpose.** Add a declarative placement pose to sub-component declarations and make
placed child geometry auto-surface, transformed into the parent's coordinate frame, at
the **geometry level** — so every consumer (viewport, STEP export, FEA volume mesh,
interference/distance queries, mass properties) sees the same placed solids. Introduce an
`aux` modifier so structure-local construction geometry can be excluded from those
external effects while remaining inspectable in the GUI.

**Supersedes (as the recommended idiom, not by deletion).** The manual
`let placed = translate(self.sub.body, …)` lift pattern documented in spec §8.3 "Cross-sub
geometry composition." That mechanism remains valid for *boolean composition* (cutting a
child body into a parent solid), but is no longer the *only* way to get child geometry on
screen. After this PRD, placement is expressed once at the `sub` site; the parent need not
re-state it.

**Cross-PRD lineage.**
- Builds on `v0_3/structure-instance-runtime.md` (sub instances), `v0_3/geometry-handle-runtime.md`
  (realization handles), and the cross-sub geometry access of spec §8.3.
- Builds on `v0_3/kernel-geometry-queries.md` — the transform-aware query path
  (`distance_with_transform` / `interferes_with_transform`) already applies a `Transform3`
  to an OCCT shape; this PRD promotes that capability into a *persistent geometry op*.
- **Consumes** the FK `world_transform` produced by `v0_3/rigid-body-dynamics.md` /
  `v0_3/kinematic-constraints-completion.md` and **owns its application to geometry**
  (see §6, G4 seam resolution).

---

## §1 — Spec grounding: the model is already committed

This is the decisive framing. `docs/reify-language-spec.md` §3.3.1 *already declares* the
semantic model, normatively, and v0.1 simply never implemented the syntax or the kernel
application:

> **§3.3.1, lines 367–371 (verbatim):**
> - `Transform` is always rigid (rotation + translation). Non-rigid maps … are a separate type.
> - **Sub-structure placement is a `Transform` from child frame to parent frame.**
> - Global position is computed by composing Transforms up the containment tree.
> - No implicit global frame. All coordinates relative to parent.

The type machinery exists already: `Frame { origin, basis }` and
`Transform { rotation, translation }` (`reify-ir/src/value.rs:527,531`), the FK system
produces `Value::Transform` via `transform_compose` (`reify-stdlib/src/snapshot.rs:700`),
and the OCCT kernel applies a `Transform3 { qw,qx,qy,qz, tx,ty,tz }` via
`BRepBuilderAPI_Transform` (`reify-kernel-occt/src/ffi.rs:680`). **What is missing is (a)
syntax at the `sub` site to express the placement Transform, (b) a persistent kernel op
that applies a Transform to a realization handle, and (c) a tessellation path that
surfaces placed children.** This PRD is the implementation of an already-specified model,
not a new semantic invention.

The grammar gate (G3) therefore resolves to **"queue grammar work as an explicit
prerequisite"**: `at` and `aux` are *not* reserved today and the `sub_declaration` rule has
no placement clause (`tree-sitter-reify/grammar.js:499`, spec §15 line 2401), so Task 1
below is the grammar/parser delta and downstream tasks depend on it.

---

## §2 — Surface syntax contract

### 2.1 Placement clause: `at`

A new reserved keyword `at` introduces an optional trailing placement expression on a
`sub` declaration. The expression evaluates to a **`Transform`** (child→parent) **or** a
**`Frame`** (interpreted as "map the child's local origin frame onto this frame in the
parent's coordinates"; lowered to the equivalent Transform). Absent `at`, the placement is
the **identity** Transform (child authored directly in the parent frame).

```reify
sub bolt  : Bolt at transform3(orient_identity(), vec3(30mm, 0mm, 0mm))
sub gear  : Gear { teeth: 24 } at mount_frame      // Frame target
sub plate : Plate                                  // no clause → identity
```

Grammar delta (spec §15 / `grammar.js`), all three `sub_declaration` arms gain an optional
trailing `seq('at', field('pose', $._expression))`. AST `SubDecl` gains
`pose_expr: Option<Expr>` (`reify-ast/src/decl.rs:221`); compiler `SubComponentDecl` gains
`pose: Option<CompiledExpr>` (`reify-compiler/src/types.rs:693`).

The pose expression is an ordinary expression — `transform3(…)`, `frame3(…)`,
`frame_to_frame(a,b)`, a `let`-bound frame, or a port frame — so no new expression grammar
is required beyond the `at` token. Named args remain colon-form; no method-call syntax is
introduced (GR-040 preserved).

### 2.2 Surfacing modifier: `aux`

A new reserved keyword `aux` is an optional prefix on `let` and `sub` declarations. It
marks **structure-local geometry that has no effects outside its structure**: it is *not*
auto-surfaced as product geometry, *not* STEP-exported, *not* an FEA part, and *not*
counted in assembly mass properties — but it **is** still tessellated and shipped to the
GUI, where it appears **hidden-by-default in the outline and is toggleable** like any other
entity.

```reify
aux let blank = cylinder(8mm, 40mm)        // construction input, not product
let drilled  = cut(body, self.blank)        // blank still usable internally
aux sub jig  : Jig at fixture_pose          // internal fixture: hidden, not exported
```

Grammar delta: `let_declaration` already carries `optional('pub')`; add an independent
`optional('aux')`. Add `optional('aux')` to all `sub_declaration` arms. AST `LetDecl` and
`SubDecl` gain `is_aux: bool`. `aux` and `pub` are orthogonal axes (export vs. external
geometric effect); both may appear, though `pub aux` is unusual.

**Keyword rationale (recorded so it is not re-litigated):** `aux` was chosen over
`internal`, `scaffold`/`scaf`, and `hidden`/`invis`. `hidden`/`invis` describe only
visibility — wrong axis, and they would collide with the GUI's existing transient
per-entity visibility toggle. `internal` is the most literal match to "no effects outside
the structure" but risks future confusion with a real `pub`/private *access-control* axis.
`aux` is a 3-letter abbreviation idiomatic to Reify (`sub`, `fn`, `bidi`), connotes
"auxiliary / supporting geometry," and does not collide with any access-control concept.

---

## §3 — Surfacing semantics (the visibility axis)

Surfacing is its **own axis**, deliberately decoupled from *placement* (§4) and from
*boolean consumption*. The three coupling failure modes that motivated this decoupling:
opt-in-by-pose conflates two orthogonal concerns; suppress-on-consume means removing the
last consumer of a construction body silently surfaces it, and `mirror`-to-duplicate would
make the *original* feature vanish. An explicit, consumption-independent flag avoids all
three.

Rules:

1. **Default-surface.** Every **geometry-typed** `let` binding and every `sub` surfaces by
   default. Non-geometry bindings (`let x = 5mm`) never surface — there is nothing to
   render; the rule is a no-op for them.
2. **`aux` opts out of external effects.** An `aux let`/`aux sub` body is realized and
   tessellated, but is excluded from product surfacing, export, FEA, and mass properties.
   Its mesh is shipped to the GUI flagged **default-hidden**; toggling it on in the outline
   renders it (normal visibility control).
3. **Consumption is independent.** Whether a body is referenced by a parent geometry op
   (`union(self.a.body, …)`) does **not** affect its surfacing. To avoid a body appearing
   both standalone *and* inside a composed result, mark the operand `aux`. This is the
   canonical idiom for boolean inputs:

   ```reify
   aux let raw   = box(20mm, 20mm, 20mm)
   aux let tool  = cylinder(5mm, 30mm)
   let part = cut(self.raw, self.tool)   // only `part` surfaces
   ```
4. **Migration note.** Default-surface is a behavior change: existing multi-structure
   designs whose child geometry was previously invisible will begin to render. The remedy
   is annotating genuine construction geometry `aux`. Task 10 ships an example and the spec
   update documents the idiom; a corpus lint to flag likely-construction bodies is a
   tactical follow-up (§11).

### 3.1 Data-model implication

`TessellateResult.meshes` is today `Vec<(String, Mesh)>` (`reify-eval/src/lib.rs:809`). It
must carry per-mesh surface metadata — at minimum `default_visible: bool` (false for
`aux`). Proposed shape: `Vec<MeshSurface>` where
`MeshSurface { entity_path: String, mesh: Mesh, default_visible: bool }`. The GUI's outline
honors `default_visible`; the mesh payload is always present so a toggle can reveal it
without a rebuild.

---

## §4 — Placement & composition semantics

1. **Pose type.** A `sub`'s placement is a rigid `Transform` from the child's local frame
   to the parent's frame (spec §3.3.1). `at <Transform>` is used directly; `at <Frame>`
   lowers to the Transform mapping the child-local origin frame onto that target frame
   (target expressed in parent coordinates).
2. **Composition up the full containment tree.** A descendant surfaces at the **composition
   of every placement transform on the path from the displayed root to that descendant**:
   `world = pose_root ∘ pose_child ∘ … ∘ pose_descendant`. A grandchild of a parent placed
   at `A`, whose intermediate child is placed at `M`, whose own sub is placed at `S`,
   surfaces at `A ∘ M ∘ S`. This is the spec's "compose Transforms up the containment tree"
   made real, at arbitrary depth (no one-level limit).
3. **Surfacing recursion vs. dot-access.** Auto-surfacing walks the containment tree
   directly and composes transforms; it does **not** require the nested dot-access syntax
   `self.outer.inner.body` that spec §8.3 currently defers. Lifting that authoring
   limitation is *not* in scope here (§10); auto-surfacing does not depend on it.
4. **Geometry-level application.** The composed transform is applied to produce *real
   transformed geometry* (a new OCCT shape via §5), not a view-only transform. This is the
   reason option "C" (a GUI-only per-mesh transform) was rejected: it would render
   correctly but leave STEP export, FEA, interference, and mass properties operating on
   un-placed geometry.

---

## §5 — Kernel `ApplyTransform` primitive

This PRD introduces the geometry op that `crates/reify-eval/src/geometry_ops.rs:1440`
explicitly anticipates:

> **(verbatim):** "v0.1 simplification: the Snapshot's per-body `world_transform` is **not**
> applied to the OCCT shape before the distance probe; geometry must be pre-positioned at
> the source-let level … FK-driven OCCT placement requires either a new
> `GeometryOp::ApplyTransform` op + handle bookkeeping or per-pair on-the-fly OCCT
> transforms — both expand scope beyond the PRD task-8 acceptance."

Contract:

1. **OCCT wrapper.** Add `apply_transform_to_handle(handle: GeometryHandleId, t: &Transform3)
   -> Result<GeometryHandleId>` to `reify-kernel-occt`, wrapping the existing
   `transform(shape, Transform3Props)` (`BRepBuilderAPI_Transform`, `ffi.rs:680`). It
   produces a **new** handle for the transformed shape (handle bookkeeping), leaving the
   source handle intact so the same child can be placed in multiple frames.
2. **Compiled op.** Add `CompiledGeometryOp::ApplyTransform { target: GeomRef, transform:
   <Transform value> }` (`reify-compiler/src/types.rs:898`). Unlike the existing
   `Transform { kind: TransformKind, args }` op (which takes source-level scalar args for
   `translate`/`rotate`), `ApplyTransform` takes a fully-evaluated rigid `Transform` and is
   applied *post-realization* to a cached handle.
3. **Exactness.** Application is rigid and exact (OCCT `gp_Trsf`); tessellating a
   transformed handle yields vertices equal to the source vertices mapped by the transform,
   within tessellation tolerance.

This single primitive serves **both** static `at` placement (§4) and FK `world_transform`
application (§6) — one transform-application path, no parallel implementations.

---

## §6 — FK unification (G4 seam owner)

The mechanism/FK system computes a per-body `world_transform` (`Value::Transform`) in the
snapshot (`reify-stdlib/src/snapshot.rs:700`) but deliberately does not apply it to
geometry (the §5 deferral). **This PRD owns the geometry-application seam** and closes that
deferral: mechanism snapshot bodies are posed by feeding their `world_transform` into the
§5 `ApplyTransform` primitive, exactly as static `at` poses are.

Ownership table:

| Concern | Owner | Note |
|---|---|---|
| FK `world_transform` *computation* | rigid-body-dynamics / kinematic-constraints (v0_3) | unchanged; produces `Value::Transform` |
| Static `at` pose *evaluation* | **this PRD** | §4 |
| `ApplyTransform` geometry primitive | **this PRD** | §5; the shared application path |
| Applying FK `world_transform` to geometry | **this PRD** | §6; resolves geometry_ops.rs:1440 |
| Nested dot-access `self.outer.inner.body` | deferred (spec §8.3) | out of scope (§10) |

Consequence: after this PRD, a mechanism at a non-identity joint configuration *renders and
exports* its bodies at their FK-posed positions (previously only the source-let positions),
and interference/distance queries operate on posed geometry by default — matching the
behavior the existing `distance_with_transform` test path already validates per-query.

---

## §7 — Consumer policy ("no external effects" contract)

| Consumer | Surface today | Sees placed product geometry? | Sees `aux` geometry? |
|---|---|---|---|
| **Viewport (GUI)** | `reify gui` / reify-debug | yes, at composed world pose | yes, but **hidden-by-default**, toggleable |
| **STEP / geometry export** | `reify build <f> -o <out>` | yes, placed | **no** |
| **Interference / distance** | kernel queries (`distance`, `interferes_with`) | yes, placed | no (not a product body) |
| **FEA volume mesh** | reify-kernel-gmsh / mesh-morph | yes, placed | no |
| **Mass properties** | `center_of_mass` / `bounding_box` (coarse today) | yes, placed | no |

STEP export and interference/distance are the **rigorous** verification consumers (exact,
already runnable). FEA and mass properties corroborate; mass properties are a point-mass
approximation today (geometry_ops.rs:1440) and improving their fidelity is explicitly *not*
a deliverable here (§10).

---

## §8 — Boundary-test sketch (facing both ways)

Four seams, each tested from both sides (the H component for a high-stakes PRD):

### 8.1 eval ↔ kernel (the `ApplyTransform` seam)
- **Producer side (reify-eval looks outward):** given an evaluated `Transform`, the
  emitted `ApplyTransform` op against a known box handle yields, after tessellation, an
  AABB translated by the transform's translation and oriented by its rotation.
- **Consumer side (occt kernel looks inward):** round-trip — applying `T` then `T⁻¹` to a
  handle recovers the source AABB within tolerance; applying identity is a no-op.

### 8.2 surfacing ↔ GUI
- **Producer:** tessellation emits one `MeshSurface` per surfaced body with correct
  `entity_path` and `default_visible` (false iff `aux`).
- **Consumer:** the GUI outline lists `aux` entities hidden; `viewport_state.meshCount`
  excludes them until toggled, then includes them; placed children appear at world pose.

### 8.3 surfacing ↔ export
- **Producer:** the export body set = product (non-`aux`) realizations with composed
  transforms baked in.
- **Consumer:** the STEP writer emits exactly those solids at world coordinates; `aux`
  bodies are absent from the written file.

### 8.4 FK ↔ geometry
- **Producer:** the snapshot exposes `world_transform` per body.
- **Consumer:** `ApplyTransform`-posed geometry's distance query matches the existing
  `distance_with_transform` result for the same transform (parity check against the
  already-validated query path).

---

## §9 — Integration DAG (proposed; not yet filed)

Each leaf names its **user-observable signal** (G2). The minimum end-to-end vertical slice
(C-as-integration-gate spine) is **T1 → T2 → T4 → T5 → T7**: parse → compile → evaluate →
surface+place → STEP-export, proving geometry-level placement reaches a real consumer.

### Phase 1 — Frontend foundation
- **T1 — Grammar + parser: `aux` modifier and `at` pose clause.** *grammar_confirmed=false
  (this is the grammar work).* Signal: `tree-sitter parse --quiet` exits 0 on fixtures
  `aux let x = …`, `aux sub a : T`, `sub b : T at frame3(…)`, `sub c : T { … } at p`
  (CST has no ERROR nodes); hand-parser unit test yields `SubDecl.pose_expr = Some`,
  `SubDecl.is_aux`, `LetDecl.is_aux`.
- **T2 — Compiler lowering.** Depends T1. Signal: a `.ri` using `at`/`aux` compiles with no
  diagnostics; a compiler unit/snapshot test shows `SubComponentDecl.pose` present and the
  `is_aux` surface flag on the lowered realization.

### Phase 2 — Kernel primitive
- **T3 — OCCT `apply_transform_to_handle` + `CompiledGeometryOp::ApplyTransform`.**
  Independent (kernel-only). Signal: unit test transforms a box handle by a known
  `(quat, translation)`, tessellates, and asserts the AABB is translated and oriented as
  expected (vertex-exact within tolerance); identity is a no-op; `T∘T⁻¹` round-trips.

### Phase 3 — Placement + surfacing (vertical slice)
- **T4 — Pose evaluation.** Depends T2. Signal: eval unit test — `at transform3(q,v)` and
  `at frame3(o,b)` each produce the expected `Value::Transform` (numeric), with the
  documented Frame→Transform convention.
- **T5 — Surfacing + full-tree composition.** Depends T3, T4. Signal: `reify build`
  mesh-dump of a 2-level *containment-only* assembly emits a mesh per surfaced descendant at
  its **composed** world coordinates (golden AABB per child); an `aux` body is present in
  the output but flagged `default_visible=false`; zero manual lift transforms in the `.ri`.

### Phase 4 — Consumers
- **T6 — GUI: default-hidden + placement.** Depends T5. Signal: reify-debug screenshot +
  `viewport_state` — the containment-only assembly shows children at world pose; the `aux`
  body is excluded from `meshCount` until toggled on in the outline, then renders.
- **T7 — Export + queries honor surfacing.** Depends T5. Signal: `reify build -o out.step`
  of a placed 2-level assembly writes child solids at world coordinates and omits the `aux`
  body; a `distance` query between two placed children equals the composed-transform
  expected value.

### Phase 5 — FK unification
- **T8 — FK `world_transform` → `ApplyTransform`.** Depends T5 (and T3). Signal: a mechanism
  `.ri` at a non-identity joint configuration renders and exports its bodies at FK-posed
  positions (vs. source-let positions previously); an interference query uses the posed
  geometry; resolves the geometry_ops.rs:1440 TODO.

### Phase 6 — Spec + integration gate
- **T9 — Spec update.** Depends T5. Signal: `docs/reify-language-spec.md` updated — §3.3.1
  (model marked realized), §4.7 `sub` (document `at` + `aux`), §8.3 (auto-surfacing as the
  recommended idiom; manual lift retained for boolean composition), §15 grammar
  (`sub_decl`/`let_decl` deltas); the spec's example `.ri` parses.
- **T10 — Integration example + gate.** Depends T6, T7, T8, T9. Signal: a multi-level
  assembly example (e.g. arm → motor → shaft, plus an `aux` fixture) committed under
  `examples/`, verified end-to-end — GUI shows placement at every depth, STEP export is
  correct, the `aux` fixture is hidden/excluded, and the file contains **no** manual lift
  transforms (the ceremony is gone).

### Dependency view
```
T1 → T2 → T4 ┐
T3 ──────────┼→ T5 → {T6, T7, T8, T9} → T10
             ┘
```

---

## §10 — Out of scope

- **Collection-sub per-element placement** (`sub items : List<T>` with per-element poses).
  Requires per-instance realization handles, which spec §8.3 defers. Single (non-collection)
  subs only in v1.
- **Nested dot-access syntax** `self.outer.inner.body` (spec §8.3 deferred). Auto-surfacing
  walks the tree directly and does not need it.
- **Non-rigid placement** (scaling/shearing; `AffineMap`, spec §18 item 16). `at` is rigid
  only.
- **Connection-driven placement** — deriving a sub's pose automatically from a `connect`
  port-mate. `connect` frame-alignment already exists separately; unifying it with `at` is
  future work.
- **Mass-property fidelity** — improving the current point-mass/world-origin approximation.
  Mass props are a corroborating signal here, not a deliverable.
- **Construction-geometry lint** — a corpus pass to suggest `aux` on likely construction
  bodies during migration (§3 rule 4).

---

## §11 — Open (tactical) questions

1. **Frame→Transform convention.** Confirm the exact mapping when `at` receives a `Frame`:
   the Transform that carries the child-local origin frame onto the target frame, with the
   target expressed in the parent's coordinates. (Pin in T4 with a numeric test.)
2. **`entity_path` scheme for surfaced descendants.** Proposed `parent.sub#realization[i]`
   composed down the path (e.g. `arm.motor.shaft#body`). Must be stable across rebuilds for
   GUI selection/visibility persistence and round-trip with the existing design tree.
3. **`pub aux` interaction.** Orthogonal axes; allowed but unusual. Confirm no surprising
   interaction when an `aux` body lives in a `pub`-exported structure.
4. **Re-realization cost.** Full-tree surfacing re-realizes child geometry per placement
   path. For deep/wide assemblies this multiplies kernel work; the existing
   `seed_cross_sub_named_steps` per-instance override path (engine_build.rs:164) is the hook,
   but caching of identical (child, transform) realizations may be needed. Tactical, profile
   in T5.
5. **Double-application guard.** If an author both places a sub with `at` *and* manually
   lifts `self.sub.body` via a parent op, both surface unless the operand is `aux` (§3
   rule 3). Confirm this is acceptable and documented as the canonical idiom rather than a
   compiler error.

---

*Decompose note:* under decompose-mode, each task above files with `planning_mode=True`,
carries `user_observable_signal` / `consumer_ref` / `grammar_confirmed` metadata, wires the
§9 dependency edges, and the batch flips `deferred → pending` together. The orchestrator
does not yet read those metadata fields (F-infra follow-up substrate).
