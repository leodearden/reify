# Design — Geometric Relations (underconstrained `at auto` geometry + solved geometric `relate`s)

**Status:** design, **/prd-candidate** — do not implement from this doc directly; hand off via `/prd` (decompose) or `/do`. The user-facing surface and ontology are resolved (decisions A–O, §14); the implementation order in §11 is the de-risked sketch, not a task list.
**Provenance:** interactive design session 2026-06-08 (Leo + Claude), backed by a 4-agent recon/design team (CAD prior-art survey, first-principles datum-algebra synthesis, Reify-substrate fit, unified-DAG feasibility) and a background adversarial stress-test of the joint-unification (decision B). Durable record: `~/.claude/projects/-home-leo-src-reify/memory/project_geometric_constraint_relations_design.md`.
**Code anchors** are hints as of writing; main moves fast — **re-locate every symbol at implementation time** (cite-by-symbol, the line is a hint).
**Owners / consumers:** the `.ri` design author (CLI `reify eval`/`check`/`explain`, the GUI). Cross-cutting: the topology-selector value type (tasks 4116–4120), the dormant SolveSpace solver (`crates/reify-constraints/src/solvespace.rs`), the unified build DAG (`docs/prds/v0_6/engine-unified-build-dag.md`), the mechanism/joint subsystem (`crates/reify-stdlib/src/{mechanism,snapshot,joints}.rs`), and `KIN-OFFSET-1` (task 4331, **deferred** — a hard prerequisite for the joint half, §8).

---

## 0. Problem

Today a `.ri` author places geometry by **computing coordinates and transforms** by hand:

```reify
sub bolt : Bolt at transform3(orient_identity(), vec3(40mm, 30mm, 6mm))
//   40,30 copied from the drill placement; 6 = half of box(...,12mm) being OCCT-centred.
//   Change the plate thickness → the bolt silently floats off the face. Intent is invisible.
```

This is fragile (numbers are duplicated and uncorrelated — a parameter change breaks downstream placements with no error), burdensome (the author does the SE(3) arithmetic, including alignment quaternions for tilted features), inflexible, verbose, and it **leaves design intent implicit**: nothing in the source says "concentric" or "flush" — only the arithmetic result.

The goal of this design is to let the author instead declare **underconstrained geometry** — placements that are wholly or partially unknown (`at auto`) — and add explicit **geometric relations** that remove degrees of freedom until the unknowns are soluble, so a solver derives the coordinates. Design intent becomes the source; coordinates disappear; the model is robust under parameter change (relations re-solve).

### Substrate reality (why this is "activate and generalize," not green-field)

- A **full SolveSpace/libslvs geometric constraint solver already exists** — `crates/reify-constraints/src/solvespace.rs` recognizes Coincident / Parallel / Perpendicular / Distance / Angle patterns and implements the `ConstraintSolver` trait — but it is **dormant**: never registered in production (the CLI/GUI install only `DimensionalSolver`, `crates/reify-cli/src/main.rs` ~`:651`), the relation vocabulary is not callable from `.ri`, and the constraint classifier already routes `std::parallel`/`std::distance`/`std::angle_between`/`std::tangent` to `ConstraintDomain::Geometric` (`crates/reify-constraints/src/classifier.rs`).
- The **typed topology-selector value type** (`Type::Selector`/`Value::Selector`, `SelectorKind`) is landed (tasks 4116/4117 done; 4118–4120 pending) — the substrate a feature→datum projection rides on.
- `Frame`/`Transform`/`Plane`/`Axis` are first-class `Value` variants with constructors (`frame3`/`transform3`/`plane_*`/`axis_*`).
- The **unified build DAG** (Option A) removes the kernel-less `check()` phase for `build()`, making geometry-backed *predicates* evaluable (§10).

The gaps are bounded and named in §11.

---

## 1. Goal & user-observable surface

The same intent as §0, expressed three ways (all the same flat-conjunction semantics):

```reify
// (a) primitives
sub bolt : Bolt at auto
relate {
    coincident(bolt.shank.axis, plate.hole.axis)    // coaxial — removes 4
    coincident(bolt.head.seat,  plate.top.plane)     // flush   — removes 3 (2 redundant)
}                                                    // residual: 1 (spin about the axis)

// (b) named compounds
sub bolt : Bolt at auto
relate { concentric(bolt.shank, plate.hole);  flush(bolt.head.seat, plate.top) }

// (c) attached to the sub
sub bolt : Bolt at auto where {
    concentric(bolt.shank.axis, plate.hole.axis)
    flush(bolt.head.seat, plate.top.plane)
}
```

The numbers `40mm, 30mm, 6mm` are gone; `concentric` and `flush` *are* the source; a change to plate thickness re-solves the bolt's pose. `reify eval`/`build` produces the placed geometry; `reify explain` and the GUI report the DOF state (§9).

---

## 2. The entity ontology — a typed datum lattice

Relations never grip "a shape"; they grip a **datum** — the minimal carrier of pose information — together with the DOF that datum's pose carries. The minimal complete set is six typed kinds:

| Datum | Pose DOF (`codim`) | Units | Realized topology that projects to it |
|---|---|---|---|
| **Direction** | 2 | dimensionless unit vector | edge tangent, face normal |
| **Point** | 3 | `Length³` | vertex, sphere/arc centre |
| **Axis** = `Point ⊕ Direction` | 4 (roll-free) | mixed | straight edge, cylinder/cone axis |
| **Plane** = `Direction ⊕ offset:Length` | 3 | `Length` offset | planar face |
| **Frame** = `Point ⊕ Rotation` | 6 | `Length³` + SO(3) | any feature, when all 6 are wanted |
| **Scalar** | — | any unit | the metric value in `distance`/`angle` |

`Direction` is a **new first-class type** (dimensionless, unit-normalized) — distinct from `Vector3<Length>` (a displacement) and from `Orientation`. This distinction is load-bearing: it makes "translate by a direction," "angle between two points," and "distance between two directions" *category errors at the source* (§3).

### 2.1 Projections (downward in the lattice)

Each datum exposes **only the projections it has**, total downward: `axis.dir : Direction`, `axis.point : Point`; `plane.normal : Direction`, `plane.point : Point`, `plane.offset : Length`; `frame.origin : Point`, `frame.x/y/z : Direction` (or `Axis`), `frame.xy_plane : Plane`. A relation defined on a small datum **lifts** to any datum that projects onto it — structural subtyping by projection (§3.2). A datum that *lacks* a projection (`Direction` has no `→Point`; `Point` has no `→Direction`) is exactly how the type system rejects nonsense.

### 2.2 Feature → datum: a trait-shaped bundle

A realized feature carries **datum-bearing traits**; each trait projects one or more datums; the feature's bundle is the **deduplicated union**. This extends the existing geometry-traits inference (`crates/reify-compiler/src/geometry_traits_inference.rs`, the per-op `Bounded/Convex/Connected` table) with a new category of marker trait that carries a projection.

| Datum trait | Projects | Provenance-independent source |
|---|---|---|
| `Planar` | `Plane` | OCCT `GeomAbs_Plane` |
| `Cylindrical` | `Axis` + `radius:Length` | `GeomAbs_Cylinder` |
| `Conical` | `Axis` + `apex:Point` + `half_angle:Angle` | `GeomAbs_Cone` |
| `Spherical` | `Point`(centre) + `radius` | `GeomAbs_Sphere` |
| `Linear` (edge) | `Axis` | `GeomAbs_Line` |
| `ArcBounded` (edge) | `Axis` + `Point`(centre) + `radius` | `GeomAbs_Circle/Ellipse` |
| `Revolute` (solid/face) | `Axis` | revolve op / symmetry |
| `Extruded` | `Direction` | prism ruling / extrude op |
| `Vertex` | `Point` | always |

**Provenance-independence** (a cylindrical hole has an `Axis` regardless of *how* it was built) is fundamentally a **resolve-time** property: it comes from the kernel's analytic surface/curve classification (`BRepAdaptor_Surface`/`Curve` → `GeomAbs_*` → `.Axis()`), which is provenance-independent for analytic geometry, **unioned** with construction-history datum-traits for the non-analytic tail (an oblique-swept-ellipse landing as a B-spline has no `GeomAbs_Cylinder`, but a `Revolute` history trait still yields its axis). More sources → more coverage; not absolute.

**Deduplication** ("even more important"): the bundle is canonicalized by the *same* geometric equivalence the relation algebra uses — coaxial axes merge, coplanar planes merge, coincident points merge (within tolerance, §2.3). A cylinder revolved from a rectangle has a side-face axis, two end-arc axes, and a `Revolute` axis — all collapse to **one**. Multi-source redundancy is *robustness*: any source yields the datum; agreement confirms it; a genuine disagreement correctly *fails* to merge (a signal the feature isn't axial). Dedup is per-projection-target: `cylinder.axis` → one; `plate.axis` → ambiguous (select a sub-feature).

### 2.3 Dedup / coincidence tolerance

The tolerance that decides "same datum" is based on the **kernel's geometric representation tolerance** — *not* the mesh/tessellation deflection (a display approximation, far too coarse) and *not* the user's GD&T/design tolerances. B-reps carry per-sub-shape local tolerances that grow under booleans/import, so the principled value is

```
tol = max( global_confusion_floor (~Precision::Confusion ≈ 0.1µm), localTol(shapeA), localTol(shapeB) )
```

which collapses to a single global value for clean models and loosens correctly for dirty/imported ones. Comparisons have a **linear** component (kernel-derived `Length`) and a **scale-aware angular** component (`ang_tol ≈ lin_tol / characteristic_length`, *not* OCCT's `Precision::Angular`).

**Coherence law (load-bearing):** the *same* tolerance governs (1) dedup, (2) as-built assertion satisfaction (§4) — they are the same geometric question — and (3) it must **dominate the solver's convergence tolerance**, or a solver-satisfied relation fails its own post-solve re-check (spurious conflict). Hierarchy: `kernel_local ≤ solver_convergence ≤ assertion/dedup`. Exposed as one knob (a `Length`), kernel-defaulted, overridable per-model/per-`relate`.

---

## 3. The relation vocabulary

### 3.1 The fundamental law: `coincident(X, X)` removes `codim(X)`

"Make two same-kind datums *the same* datum" is one relation at every kind, removing the datum's own pose-DOF:

| `coincident(X, X)` | familiar name | removes |
|---|---|---|
| `Direction` | **parallel** (same sense; `antiparallel` = opposite) | 2 |
| `Point` | **coincident** | 3 |
| `Plane` | **coplanar / flush** | 3 |
| `Axis` | **coaxial / concentric** | 4 |
| `Frame` | **fasten / rigid** | 6 |

The friendly names are the surface API; `codim` is the one DOF law (no per-relation magic numbers). The other two families:

- **`on(p: Point, host)`** — point incidence into a higher-D host. `removes = 3 − dim(host)`: `on(Point, Plane)` = 1, `on(Point, Axis)` = 2, `on(Point, Point)` = 3 (≡ coincident points).
- **metrics** — `distance(…, δ: Length)` and `angle(…, θ: Angle)`, each removing **1**.

The three semantic primitives are **incidence** (`P ∈ X`), **angle** (`∠(d₁,d₂)=θ`), and **distance** (`dist(P₁,P₂)=δ`); `coincident`/`on` and all named relations reduce to these. Incidence (the collapse, removing `codim`) is a *separate* primitive from distance (generic metric, removing 1) — `coincident(p,p)` removes 3, it is **not** `distance(p,p,0mm)`.

### 3.2 Three policing layers — how nonsense dies

- **(a) Unit** — the metric arg's dimension is part of the signature: `angle(a,b,5mm)` and `distance(a,b,30°)` are type errors. Free from the dimensional system.
- **(b) Kind / projection** — a relation requires operands that *project* to the datum type it names, and a datum exposes only the projections it has: `distance(some_direction, …)` fails ("a Direction has no location"); `angle(p1, p2, θ)` fails ("angle is between directions; got Point"). **Implicit projection is allowed iff unique** (`Axis→Direction` via `.dir`, `Plane→Direction` via `.normal` auto-lift; `Frame→Direction` is ambiguous → write `frame.z`).
- **(c) Geometric preconditions — by curation, not dependent types.** Ship only signatures that are unconditionally well-defined for their argument types; express conditional relations as compounds that bundle their precondition. The one conditional case is plane-plane distance: there is **no** bare `distance(Plane, Plane)`; instead `offset(a: Plane, b: Plane, δ: Length) = parallel(a,b) & on(a.point, b, δ)` carries its own parallelism. Most overloads are already unconditional (incl. `distance(Axis, Axis)` — skew lines have a well-defined common perpendicular).

### 3.3 Cross-kind policy

Same-kind relations lift via projection (`parallel(axis,axis)` = dirs parallel; `parallel(plane,plane)` = normals parallel). **Ambiguous cross-orientation overloads are forbidden** (`parallel(Axis, Plane)` — "line in plane" vs "line ⊥ plane"): force the explicit shared-kind projection (`parallel(axis, plane.normal)`). Cross-kind richness lives only *inside* named compounds (e.g. `tangent` projects internally).

### 3.4 The DOF table (the #1 miscount source)

| relation | removes | why |
|---|---|---|
| `parallel` / `antiparallel` (Dir) | **2** | direction pinned to a *point* on S² |
| `perpendicular` (Dir) | **1** | direction on a *great circle* |
| `angle(Dir, Dir, θ≠0)` | **1** | direction on a *small circle* (cone) |
| `distance(P, P, δ≠0)` | **1** | one scalar equation |
| `coincident` / `on` (the δ=0 *collapse*) | **codim** | the target set collapses |

### 3.5 Signatures & DOF inference

Each overload publishes a nominal ΔDOF (the tables above). A compound/mate's ΔDOF is **inferred** by summing its body (independence assumed; rank/dedup handles redundancy at solve, §9) and **surfaced** as its contract (hover/doc: `offset(Plane,Plane,Length) -> Relation removes 3`). The metric arg `δ`/`θ` is an ordinary `Scalar` expression (literal/param/`let`/scalar-`auto`); the relation drives the *pose* to match it — it does **not** solve the scalar from the pose (that is the `derive` verb, §4). Implementation home: a `relation_signatures.rs` mirroring `math_signatures.rs` / `joint_signatures.rs`.

---

## 4. The `relate` block and the three verbs

Relations live in a **mandated `relate` context**, type-enforced — not a convention. Introduce a `Relation` type distinct from `Bool`:

- the relation vocabulary returns **`Relation`** (no truth value — a DOF-removal directive for the geometric solver);
- `relate { … }` accepts **only `Relation`**; `constraint` accepts **only `Bool`**; cross-placement is a type error.

> **Rule:** if it removes degrees of freedom from a pose, it's a relation → `relate`; if it has a truth value, it's a predicate → `constraint`.

One geometric concept, three verbs distinguished by syntax and type:

```reify
let gap = distance(bolt.tip, plate.top)            // DERIVE → Length   (query/measure)
relate { at_distance(bolt.tip, plate.top, 5mm) }   // DRIVE  → Relation (removes a DOF)
constraint distance(bolt.tip, plate.top) <= 5mm    // CHECK  → Bool     (verdict after solving)
```

`is_`-prefixed names are the check form (`is_parallel`, `is_flush`) — reusing the `is_watertight`/`is_manifold` precedent. The check form is *evaluable* because the unified DAG makes geometry-backed predicates real (§10).

**As-built degradation:** a relation over operands that are *already fully determined* (no `auto` to drive) has no DOF to remove and degrades to an **assertion** (satisfied, or a conflict diagnostic) — Fusion's driving-vs-as-built distinction, for free. More generally a relation's role is **per-DOF**: it *drives* the still-free DOF it touches and *asserts* the already-removed ones. The author writes intent; the system computes role and rebalances automatically as `auto` DOF appear/disappear elsewhere. Over-constraint is an error *only when inconsistent*, never merely because there are more relations than DOF (§9).

**Why `relate` carries weight (not sugar):** it is a typed routing+scoping boundary that (1) eliminates the domain-classifier heuristic (the block *is* the geometric-solver routing signal), (2) gives the DOF analysis a lexical home + error span, (3) makes the unified-DAG Resolution-node solve scope syntactic, (4) separates the geometric vs dimensional solver problems, (5) localizes the geometry-in-the-loop decline gate (§10), (6) carries its own order-independent/idempotent elaboration.

**Homes:** `sub b : … at auto where { … }` for single-sub locality (the sub's own datums refer to its *local* frame; the relations solve the local→world pose map); structure-level `relate { … }` for multi-sub / coupled / closed-loop systems. Both desugar to one flat relation set. The solve is scoped **per structure** (hierarchical, inside-out); cross-structure-level relations are out of scope for v1.

---

## 5. The `at auto` pose binding site

`at auto` declares the sub's pose as **one unknown `Frame`** (in the parent frame), 6 DOF (3 translation + a 3-DOF rotation, internally a SolveSpace quaternion "normal"); the parameterization is hidden from the author. No-`at` stays identity (not auto) — solving is opt-in. `at` becomes a new auto binding-site (extending the auto-binding-sites framework).

A parameterized `auto(…)` form unifies scalar and pose auto:

```reify
auto                                             // fully unknown
auto(free)                                       // residual under-determination is acceptable
auto(seed = <expr>)                              // witness / initial guess
auto(x = 5mm, orientation = orient_identity())   // fix components, solve the rest
```

routed scalar→dimensional vs pose→geometric by the cell's type (the same axis as `relate` vs `constraint`). **Partial-fix philosophy:** relations are primary (intent); component value-fixes are an escape hatch (known coordinates); `orientation = identity/copy` is the one idiomatic value-fix, finer rotational control → relations; fixes and relations compose.

**Seed/witness:** `auto(seed = …)` picks the *root* for multi-root nonlinear solves and seeds convergence; default = the parent frame; it affects *output* only when the system is multi-root or under-constrained (irrelevant for well-constrained single-root); type-checked.

**Residual DOF:** an error unless `auto(free)` (anonymous, solver-seeded) *or* claimed by a joint `with` declaration (named, mechanism-owned, §7–§8) — mirroring scalar `auto`/`auto(free)`.

This binding site's solve is the **datum-domain single-shot** case (§10): a sub's own datums are realized independent of its assembly pose, so the relate-solve runs once and `ApplyTransform` places it — *not* the deferred geometry-in-the-loop cycle.

---

## 6. Grounding, reference frames & construction datums

Every relate-solve needs a fixed anchor (all solves yield concrete poses). That anchor is the **structure's own frame**, exposed as `self`:

```reify
self.origin              self.x / self.y / self.z          self.frame
self.xy_plane / .yz_plane / .zx_plane
```

reusing Reify's existing `self`. **Encapsulation (a deliberate constraint that buys composability):** within a structure you reference `self` only — there is **no global `world`** reachable inside nested structures (a part is placed relative to its container, never the absolute world — that is the parent's job). At the root structure, `self` *is* world. This is what lets a sub-assembly drop in anywhere.

**Grounding.** A sub with no `at` is implicitly grounded at `self.origin` (the fixed reference — most structures have one). An `at auto` sub must trace (transitively) to ground — a grounded sub *or* `self` datums; mating to `self` *is* grounding. An assembly whose auto subs relate only to each other, with no path to ground, is a diagnosed **global-float error** ("6 DOF — the assembly floats in `self`: ground a part"), which falls out of the DOF ledger (§9). `ground(sub)` / `fix(sub)` are sugar for `fasten(sub.frame, self.frame)`.

**Construction datums** need *no new machinery* — datums are first-class let-bindable values, so reference geometry is built with a small datum-constructor library and bound:

```reify
let mid     = midplane(case.left_wall, case.right_wall)   // : Plane
let bore_ax = axis_through(hole1.centre, hole2.centre)    // : Axis
let seat    = offset(case.top.plane, 5mm)                 // : Plane
```

Core constructors (`midplane`, `axis_through`, `plane_through(p,p,p)`, `offset(plane,δ)`, `frame_at(origin,x,z)`) are mostly kernel-free value-algebra; several exist already (`plane_xy`, `axis_x`, `frame3`). A construction datum mates like any other and can itself be `auto`. CAD's whole "datum/construction geometry" subsystem is, here, lattice + constructors + `let`.

---

## 7. The abstraction mechanism — mates & joints

The standard library is *user code*. Two definition shapes, only one needing new syntax:

**Pure mate = a function returning `Relation`** (no keyword; DOF inferred & surfaced):

```reify
fn concentric(a: Axis, b: Axis)        -> Relation = coincident(a, b)              // removes 4
fn flush(a: Plane, b: Plane)           -> Relation = coincident(a, b)              // removes 3
fn offset(a: Plane, b: Plane, δ: Length) -> Relation = parallel(a,b) & on(a.point, b, δ)   // removes 3
fn tangent(cyl: <HasAxis & HasRadius>, face: Plane) -> Relation =                  // removes 2
    parallel(cyl.axis, face.normal) & distance(cyl.axis, face, cyl.radius)
```

**Joint = a mate that exposes named residual DOF** (the only new definition syntax):

```reify
joint revolute(a: Axis, b: Axis, stop: Plane) with angle: Angle in 0deg..120deg = {
    coaxial(a, b)         // −4
    on(a.point, stop)     // axial stop, −1   → residual: 1 rotational
}
joint cylindrical(a: Axis, b: Axis) with { angle: Angle, travel: Length } = coaxial(a, b)
joint ball(c: Point, d: Point) with orientation: Orientation = coincident(c, d)
```

**The self-checking law:** a joint's declared free DOF must **match the body's geometric residual** by count *and* kind (`Angle`-typed ⇒ rotational, `Length` ⇒ translational, `Orientation` ⇒ 3 rotational), verified at definition. A mismatch is a compile error ("declared 1 rotational free DOF, but the relation leaves 1 rot + 1 trans; add a constraint or declare `travel: Length`"). Joints cannot lie about their kinematics. (Post-§8 reframe: the declared DOF is the *mechanism-owned motion variable*, not a geometric-solver residual; the self-check ensures the geometric residual equals the DOF the mechanism will drive.) `range` is dimensionally-typed and reuses the existing `validate_range` machinery.

**Kind-generics:** `coincident<D: Datum>(a: D, b: D) -> Relation removes codim(D)` is one generic definition; `parallel`/`coaxial`/`coplanar`/`fasten` are specializations. Builds on the in-progress generics work (tasks 4232/4235) + a `Datum` bound; `codim(D)` resolves at monomorphization. This is ThingLab's "constraints as first-class composable objects," realized.

**Deferred:** first-class partial application (`let upright = parallel(_, world.z)`) depends on closure/function-value support; relations are still first-class values (bindable/passable/conjoinable), just not partially-applicable in v1.

---

## 8. Joints and the mechanism system (the reframed unification)

The intent: `relate` *defines* a joint (coincidence constraints + named residual DOF); the **existing** mechanism subsystem (`crates/reify-stdlib/src/{mechanism,snapshot,joints}.rs`, `joint_signatures.rs`, FK, loop-closure Newton) *animates* it. A background stress-test confirmed this is viable **with one critical reframe and one prerequisite.**

### 8.1 Reframe — relate places the *mount*, not the *motion variable*

The architecture carries **no symbolic/parametric residual**: every solve (incl. SolveSpace) returns a *concrete* `Value` per cell (`crates/reify-ir/src/constraint.rs` `SolveResult::Solved`); `auto(free)` means "skip the uniqueness check, return `unique:false`," **not** "leave a free variable" — the cell still gets a concrete seed value. The mechanism's joint variable was never a solver residual; it is supplied *externally* per evaluation via `bind(joint, value)` / `sweep` / range-midpoint (`snapshot.rs`).

Therefore: **the relate-solve determines the joint's mounting frame / axis** (a concrete result — `coaxial(a,b)` fixes the axis); **the joint's motion variable stays the mechanism's `bind`/`sweep` value, never an auto-param.** `revolute(a,b).angle` is a *declared, mechanism-owned* joint variable, not a leftover from the geometric solve. The self-checking law (§7) guarantees the geometric residual equals exactly the DOF the mechanism will drive.

### 8.2 Prerequisite — `KIN-OFFSET-1` (task 4331, deferred)

Joints today are `{kind, axis, range}` with **no pivot/origin/offset** — a revolute rotates about the *world origin* (`joints.rs`, translation `[0,0,0]`); the `pivot: Point3` in the PRDs is fiction `make_joint` never stores. There is no link geometry, so FK/loop/dynamics cannot place a joint at a real spatial position. `KIN-OFFSET-1` (the offset field threaded through `walk_fk`/loop-residual/dynamics) is a **hard prerequisite** — and the synergy is that **`relate` is the natural front-end that produces the mount frames that field stores.** Co-design them: relate solves the offsets; `KIN-OFFSET-1` threads them.

### 8.3 Two solvers, split by topology

Keep the existing **loop-closure Newton solver** as the sole owner of *motion-time* closed-chain consistency (it re-solves free joints each snapshot, with warm-start continuity for sweeps). Confine the geometric (SolveSpace) relate-solve to **single-shot static assembly-mate placement** — *not* closed kinematic loops with mobility. A relate-placed mechanism with a closed motion loop still runs Newton at snapshot time.

### 8.4 Couplings stay on the scalar side

Gear / screw / rack-and-pinion / `couple` are *algebraic ratio* relations between two joint variables (`v_child = ratio·v_parent + offset`) — not geometric coincidence over datums; they do not lower to SE(3) relations. They belong on the **scalar `constraint` side** (a "drive" over scalar DOFs), confirming the `relate`(geometric)/`constraint`(scalar) split.

### 8.5 Assets

Ranges are already dimensionally-typed (`validate_range(ANGLE)`, `joints.rs`) — `range = 0deg..120deg` fits 1:1. The done mechanism-completion enforcement (driving-vs-non-driving, the `joint_signatures.rs` typing family) and KCC closed-chain machinery are reused, not replaced. Sequence this work after `KIN-OFFSET-1`, coordinating with pending KCC tasks (loop diagnostics; the four-bar e2e that currently can't pass without link offsets).

---

## 9. Diagnostics & DOF legibility

**Governing principle:** diagnostics speak *geometry* (the datum/relation vocabulary), never solver internals.

**Constraint-health states.** Under-constrained → error (unless `auto(free)`/joint `with`), with residual **named geometrically** (decompose the constraint-Jacobian null space at the witness config into named twists: "rotation about `<axis>`", "slide along `<direction>`", "translation in `<plane>`", "screw about `<axis>`"). Redundant-consistent → **silent by default** + opt-in lint (kills the over-constraint friction). Conflicting → **loud**: minimal conflict set (SolveSpace `failed[]` → source spans) + geometric explanation ("concentric forces 0mm; at_distance forces 5mm") + newest member as primary culprit (honest it's a set) + escapes *offered* (remove, or demote to a soft objective), never auto-resolved. Non-convergent → "try a `seed:` nearer the config." Wrong-root (`unique:false`) → "set `seed:` to choose," with a visualize+re-seed loop. Datum-projection failures and ambiguous bare projections → typed errors.

**The DOF ledger** (the signature legibility artifact) — per `at auto` sub, itemize where the DOF went:

```
bolt : pose — 6 DOF
  concentric(bolt.shank.axis, plate.hole.axis)   −4
  flush(bolt.head.seat, plate.top.plane)          −1   (+2 rot, +1 trans redundant)
  ──────────────────────────────────────────────────
  spent 5  ·  free 1  →  rotation about bolt.shank.axis
  status: under-constrained — add a relation, `auto(free)`, or make it a joint
```

The redundancy column *is* the driving-vs-redundant partition; the ledger reuses the inferred ΔDOF signatures. Surfaced in `reify explain` and the GUI badge.

**Reuse, not a new system.** Plug into undef-self-describing (the `UndefCause` tracer + `reify explain` — add `SolveFailed{under/over/diverged}` and `DatumProjectionUnavailable` causes), the determinacy states, and `W_UNDERDETERMINED` (constraint-solver-completion §3.6, extended with geometric residual naming). **Static** (sound nominal-Σρ under-count, unit/kind errors, ambiguous/missing projection — LSP-live) + **solve-time** (residual naming, redundant-vs-conflict, convergence, root multiplicity) split. One story regardless of backend.

---

## 10. Solver, lifecycle & feasibility

**Over-constraint handling.** Per-relate-block, rank-analyze the relations at the witness config → partition into a **maximal independent driving set** + a **redundant remainder**. Hand only the driving set to SolveSpace (so it sees a well-constrained or cleanly under-constrained system → OKAY + residual `dof`); verify the redundant remainder as **post-solve geometry-backed predicate assertions** (the unified-DAG predicate path). This implements the redundant-as-assertion semantics exactly, gives better conditioning, and yields precise blame — independent of libslvs's redundancy mapping. **No `relate` precedence** in the language (consistent over-constraint is order-irrelevant; conflicts get a minimal-set diagnostic; soft preferences are the objective layer).

**The unified-DAG fit.** The Option-A rework removes the kernel-less `check()` for `build()` (geometry-backed *predicates* become evaluable) and makes the under-constrained-auto-through-geometry cluster statically identifiable. The **datum-domain single-shot mate solve fits its existing `solve → realize-placed` shape**: a sub's local datums are realized before the per-scope `Resolution` node, the relate-solve runs once, `ApplyTransform` places the sub. The DAG **declines** the genuinely cyclic *geometry-in-the-loop* case (a relation whose referenced datum itself depends on the unknown pose) with `E_EVAL_UNRESOLVED` — out of scope here, a future PRD.

**Wiring.** SolveSpace must be registered into the production `SolverRegistry` (`crates/reify-constraints/src/registry.rs`) for the CLI and GUI engines — it is currently dormant (only `DimensionalSolver` is installed). The classifier already routes geometric constraints to the geometric domain.

---

## 11. Substrate reuse map & build sketch (de-risked, dependency order)

| # | Step | Reuses / extends | Novelty |
|---|---|---|---|
| 1 | Register `SolveSpaceSolver` in the production `SolverRegistry` (CLI + GUI) | `registry.rs`, `solvespace.rs`, `main.rs` | wiring |
| 2 | First-class `Direction` type; datum projections (`.dir/.normal/.origin/.z/.xy_plane`) | `ty.rs`, `value.rs`, geometry value variants | type + member-access surface (today unsupported on built-in `Frame`) |
| 3 | Feature→datum trait bundle (analytic classification ∪ history) + dedup; `feature.axis : Axis|Axis?` refinement | `geometry_traits_inference.rs`, topology-selector value type (4116–4120), OCCT `BRepAdaptor_*` FFI | **the real missing bridge** |
| 4 | Relation vocabulary (`relation_signatures.rs`): primitives + `coincident<D>` + named compounds; the three policing layers; `Relation` type | `math_signatures.rs`/`joint_signatures.rs` pattern, classifier routing, generics (4232/4235) | type + signatures |
| 5 | `relate` block + `Relation`-vs-`Bool` enforcement + 3-verb; pose-`auto` (6-DOF) + `auto(…)` form + `at` binding-site | auto-binding-sites framework, constraint dispatch | grammar + elaboration |
| 6 | Per-scope relate-solve at the `Resolution` node (driving-set partition → SolveSpace → `ApplyTransform`); redundant-assertion verification | unified DAG (`engine-unified-build-dag`), `ApplyTransform` (task 3901) | integration |
| 7 | `self` datums + grounding + construction-datum constructors | `self`, existing `frame3`/`plane_*`/`axis_*` | constructors |
| 8 | Diagnostics: DOF ledger, geometric residual naming, conflict sets | undef-self-describing, `W_UNDERDETERMINED`, determinacy | diagnostic surfaces |
| 9 | Joint half (after `KIN-OFFSET-1`/4331): `joint … with …`, self-check, hand mount→FK offset; couplings on scalar side | mechanism subsystem, `validate_range`, mechanism-completion enforcement | integration; **gated on 4331** |

---

## 12. Out of scope / deferred

- **Geometry-in-the-loop solving** (a relation whose datum depends on the very pose it constrains) — `E_EVAL_UNRESOLVED`; a future PRD that would re-introduce a bounded fixpoint scoped to the `Resolution` node.
- **Closed kinematic loops with mobility** via the geometric solver — owned by the existing loop-closure Newton solver (§8.3).
- **The joint half** until `KIN-OFFSET-1` (4331) lands (§8.2).
- **First-class partial application** of relations (§7).
- **Cross-structure-level relations** (§4) — solves are per-structure in v1.

---

## 13. Open questions / loose ends (tactical)

1. **Standard mate/joint library enumeration** — mechanical given §3/§7; pin the exact set (revolute/slider/cylindrical/planar/ball + tangent/offset/centered/symmetric) and their definitions.
2. **Interop / migration** — coexistence with the explicit `at <pose>` form and raw-selector escape hatch; a migration story for existing `.ri` files.
3. **Final keyword spelling** — `relate` / `at auto` / `joint … with` / `self` / `ground` are working names; confirm before grammar lands.
4. **Dedup tolerance knob surface** — per-model vs per-`relate`; exact default expression (§2.3).
5. **`reify explain` ledger format** — table vs `--format json` (mirror `reify doc`).

---

## 14. Decision log (locked 2026-06-08, interactive)

- **relate** mandated; `Relation`-vs-`Bool` type rule; three-verb derive/drive/check; `is_`-prefix = check (§4).
- **Datum lattice** fully typed (§2); first-class `Direction`; member-access projections; trait-bundle feature→datum; dedup by geometric equivalence; type-refined `Axis|Axis?`; dedup tolerance = kernel representation tolerance, unified with the assertion tolerance, dominating the solver (decision set §2.3).
- **`coincident(X,X) = codim(X)` is fundamental** (§3.1); curation over dependent types; cross-kind via explicit projection (§3.2–3.3).
- **`at auto`** = 6-DOF unknown; `auto(…)` parameterized form; relations primary / value-fixes escape-hatch; residual = error unless `auto(free)`/joint `with` (§5).
- **Abstraction:** pure mate = `fn → Relation`; joint = `joint … with …`; self-checking law; kind-generic `coincident<D>`; defer partial application (§7).
- **Joint unification (B), reframed:** relate places the mount, the mechanism owns the motion variable; `KIN-OFFSET-1` prerequisite; solver split by topology; couplings on the scalar side (§8).
- **Diagnostics:** geometric residual naming; redundant-consistent silent; the DOF ledger; reuse undef-self-describing (§9).
- **Grounding:** `self` as anchor/root datum source, no global `world` in nested structures; grounded-vs-auto subs; construction datums as let-bound values (§6).
- **Over-constraint:** redundancy ≠ conflict; driving-set partition + redundant-as-assertion; no relate precedence (§10).
