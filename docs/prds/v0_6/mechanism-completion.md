# std.mechanism §13 Completion — accessor naming, joint-type enforcement, diagnostics, doc-reconcile

Status: authored 2026-06-03 in interactive `/prd` session. Pending Leo approval before queueing tasks.
Closes the **P19 mechanism-completion** row-group of
`docs/architecture-audit/stdlib-reference-gap-register-2026-06-01.md` (all 10 rows).

## §0 — Purpose and scope correction

The 2026-06-01 13-agent survey flagged §13 `std.mechanism` with 10 gaps and framed
the cluster as a "downstream implement PRD (depends on geometry transforms/queries)."
Ground-truthing the runtime substrate **corrects two load-bearing premises** the survey
froze in:

1. **FK-aware interference is already implemented on main.** The value-level
   `interferes`/`interferes_with`/`min_clearance` builtins return `Value::Undef`
   (`crates/reify-stdlib/src/snapshot.rs:645-676`), but that cell is **overwritten**
   by `Engine::post_process_kinematic_queries` (`crates/reify-eval/src/engine_build.rs:1998,2374`,
   task 2531), and task **3906 (done/merged `8cb0bdf79e`)** applies each body's FK
   `world_transform` via `GeometryOp::ApplyTransform` **before** the OCCT probe. The
   `fk_posed_cubes` case in `crates/reify-eval/tests/mechanism_interference_smoke.rs`
   proves a prismatic-bound cube is probed at its FK-posed position. The survey grepped
   the stub and never traced the post-process. → FK interference is **not** an implement
   target here.

2. **The "type-name fictions" are a deliberately-accepted Value::Map runtime, not an
   accident.** Task **3845 (done)** declared the nominal `.ri` joint/mechanism/snapshot
   types and was **relaxed twice** (esc-3845-77, esc-3845-91) to leave the runtime as
   `Value::Map`; the gap-register's own Bucket-B classifies this as doc-reconcile. The
   surviving real gaps are (a) the type tags are **unenforced** (no constructor returns
   them; no trait bound is checked) and (b) the **generic** spellings `Coupling<P>` /
   `MotionValue<J>` are blocked on generic user fns.

The net result: this is **predominantly a doc-reconciliation PRD plus two clean code
deltas** (a diagnostic-emission seam and a joint-type enforcement chain), with the
swept-interference worked examples owned by a **live sibling PRD** (KCC), and the
generic-typing tail owned by the **generics PRD**.

### Supersession / ownership

- This PRD **does not** re-file FK-aware interference — that capability is on main
  (3906) and the swept `.map(interferes)` worked-example closure + `dock_pickup.ri`
  e2e is owned by `docs/prds/v0_3/kinematic-constraints-completion.md` (KCC) tasks
  **3844 (KCC-ε)** and **3848 (KCC-ι)**, both pending. §13.6 doc-reconcile here
  **depends-on** 3848.
- Per the G4 decision, this PRD also **flags 3844 for re-scope**: 3844's planned
  `try_resolve_snapshot_body` + `distance_with_transform` FFI dispatch (3841, done)
  may be redundant now that 3906 landed the alternative `ApplyTransform`-the-shape
  path. Resolved as a `update_task` note on 3844 at decompose time (not a leaf).
- The **generic** tail (`Coupling<P>`, `MotionValue<J>`) is filed as a **deferred
  forward-stub** depending on the generics PRD (`docs/prds/v0_6/generic-user-functions.md`,
  tasks 4232 γ + 4235 ζ).

## §1 — Goal and user-observable surface

> A Reify user modelling a mechanism (3D-printer motion systems, machine-tool axes,
> tool-changers, counter-mass balancers) gets **honest, enforced** joint semantics:
> the documented accessor names resolve; misusing a derived-motion `Coupling` where a
> driving joint is required is **rejected** (at compile time, with an eval-time
> backstop); a duplicate solid surfaces the documented typed diagnostic; and §13 of the
> stdlib reference describes what the runtime actually does.

User-observable surfaces (G2 vocabulary):
- **Diagnostics:** `E_MECHANISM_DUPLICATE_SOLID` and a new `E_MECHANISM_NONDRIVING_JOINT`
  emitted through `EvalResult.diagnostics` / `reify check`, visible in the GUI
  diagnostics panel.
- **Type-checking:** `reify check` rejects `bind(couple(...), v)` naming `Coupling`;
  `prismatic(...)` types as `Prismatic` (LSP hover, compiler cell type).
- **Docs:** `docs/reify-stdlib-reference.md` §13 reconciled; the P19 gap-register rows
  flipped; no fictional type/accessor names presented as live.

## §2 — Sketch of approach

Coupling is **already a real, load-bearing runtime primitive** — `couple(parent, ratio,
offset)` (`joints.rs:471`) validates its parent is a driving joint, `gear`/`screw`/
`rack_and_pinion` are coupling variants, FK derives a coupling's motion from its parent,
`joint_jacobian` recurses through it (`ratio · parent`), and `counter_mass_balance.ri`
drives a ratio=−1 counter-mass through it. What is missing is **enforcement** (nothing
stops `bind(coupling, …)`) and the **generic spelling**. The graded enforcement chosen
(Leo, 2026-06-03):

- **L1 — runtime guard.** Add `is_driving_joint()` (sibling to the single-source
  `is_joint_value()`, task 2632). `bind`/`sweep`/`dim` reject `coupling`/`fixed` →
  emit `E_MECHANISM_NONDRIVING_JOINT`. Caught at eval, including dynamically-typed call
  sites where the static type is only `Joint`.
- **L2 — compile-time enforcement.** Add a joint-constructor **signature family**
  `is_joint_typed_fn` + `joint_ctor_result_type` (a new `crates/reify-compiler/src/joint_signatures.rs`,
  mirroring `math_signatures.rs` / `units::is_geometry_query`, expr.rs:1570), typing
  `prismatic()`→`Prismatic`, `couple()`→`Coupling`, etc. Declare `trait Joint {}` /
  `trait DrivingJoint : Joint {}` and make `Coupling`/`Fixed` conform to `Joint` (not
  `DrivingJoint`). Give `bind`/`sweep`/`dim` `DrivingJoint`-bounded signatures so the
  compiler rejects a `Coupling` arg via the existing `satisfies_trait_bound`
  (entity.rs:3732). **No generics required** — nominal conformance suffices.
- **L3 — generic dimensional typing (deferred forward-stub).** `Coupling<P: DrivingJoint>`
  and `MotionValue<Coupling<P>> = MotionValue<P>` (static Length-vs-Angle coherence of
  the motion variable) is *static-safety sugar only* — the runtime primitive is fully
  functional without it — and is genuinely blocked on generic user fns. Filed depending
  on the generics PRD.

The duplicate-solid path establishes the **diagnostic-emission seam** the runtime guard
reuses: the detector at `mechanism.rs:469` already writes a `Value::Map` `error` field
("duplicate_solid"); the reserved code `DiagnosticCode::MechanismDuplicateSolid`
(`crates/reify-core/src/diagnostics.rs:789`) already exists; the missing piece is the
translation of the errored Map → typed `EvalResult.diagnostics` at the eval boundary.

Everything else is **doc-reconcile** of §13 against the runtime: accessor names to the
`joint_*` prefix (Q1); `world` as the `world()` builtin (no top-level `let world` —
Reify has no top-level const, same reason `g`/`c` are functions); `E_KINEMATIC_CLOSED_CHAIN`
noted reserved-but-dead (task 2671 v0.2 loop-closure recording); `center_of_mass`/
`bounding_box` point-mass v0.1-approximation caveat; `Axis` noted as owned by the
geometry-transforms cluster (currently `Vec3` at runtime); `joint_jacobian`/`Twist`
live as `Map{angular,linear}` (the doc undersells it as "v0.2"); `.map`/`.windows`/`.norm`
method-call forms in §13.6 marked non-Reify (free-function forms only).

## §3 — Pre-conditions for activating

- **On main, confirmed:** `DiagnosticCode::MechanismDuplicateSolid` + `KinematicClosedChain`
  reserved (`reify-core/diagnostics.rs:766,789`); `joint_axis/joint_range/joint_ratio/joint_offset`
  registered (`joints.rs:638-681`); duplicate-solid detector (`mechanism.rs:469`);
  `is_joint_value` single-source discriminator (task 2632); FK post-process
  (`engine_build.rs:1998,2374`, 3906 done); `math_signatures.rs` signature-family
  template; `satisfies_trait_bound`/`check_type_param_bounds` (entity.rs:3732,3639);
  trait-supertrait + `structure def X : Trait` grammar (G3-confirmed — see §6).
- **External deps wired at decompose time:** L3 → 4232, 4235 (generics PRD); DOC → 3848
  (KCC-ι).
- **No grammar prerequisite.** The only novel `.ri` fragment is the `trait Joint` /
  `DrivingJoint : Joint` / `Coupling : Joint` hierarchy, which both parses and passes
  `reify check` (§6).

## §4 — Resolved design decisions

| # | Decision | Rationale |
|---|----------|-----------|
| D1 | **Accessors: doc-reconcile to `joint_*` prefix** (Q1) — keep `joint_axis/joint_range/joint_ratio/joint_offset` canonical; rewrite §13.1. **No bare aliases.** | `range`/`axis` are collision-risky names in the flat global builtin namespace; the prefixed form is collision-safe and self-documenting. Zero code change. |
| D2 | **Enforcement = L1 + L2** (Leo). Runtime guard *and* compile-time signature family + `DrivingJoint`-bound. | Compile-time rejection is the primary signal; the runtime guard is defense-in-depth for call sites where the static type is only `Joint` (e.g. a `List<Joint>` element bound in a loop). |
| D3 | **`trait Joint {}` + `trait DrivingJoint : Joint {}` declared; `Coupling`/`Fixed` conform to `Joint` only.** | Matches the §13.1 doc hierarchy exactly; nominal conformance is the only mechanism Reify supports and is what L2's bound-check reads. The supertrait is *enforced* (L2 reads it), not decorative. |
| D4 | **Generic `Coupling<P>` / `MotionValue<J>` deferred (L3 forward-stub), depends on generics PRD 4232+4235.** | Blocked on generic user fns (unimplemented); buys only static motion-variable dimensional coherence; the runtime primitive works without it. |
| D5 | **FK-interference NOT re-filed; §13.6 swept worked-examples owned by KCC (3844/3848).** DOC depends-on 3848; 3844 flagged for re-scope. | FK interference is on main (3906). Re-filing would duplicate the live KCC completion contract (G4). |
| D6 | **`E_MECHANISM_NONDRIVING_JOINT` is a new reserved diagnostic code** (reify-core), emitted by both L1 (eval) and L2 (compile). | Same reserve-then-emit pattern as `MechanismDuplicateSolid`; one code, two emission sites. |
| D7 | **`world` stays `world()`; `Axis` stays runtime `Value::Map`, doc-reconciled.** | No top-level const in Reify; `Axis` is owned by geometry-transforms (G-C). Static-sugar, not load-bearing. |
| D8 | **Full §13 value vocabulary typed, not joints only** (Leo). β/γ also declare marker `structure def JointBinding {}` + `structure def Twist {}` and type `bind()`→`JointBinding`, `joint_jacobian()`→`Twist`, `dim()`→`SweepDim`, `body()`→`Mechanism`, `body_id_of()`→`BodyId` (the latter four structs already declared by 3845). | A marker tag + a typed constructor = an *enforced* return type (not the "unenforced decoration" the gap-register criticizes). Makes `snapshot`'s `bindings: List<JointBinding>` enforceable. The **payload-sum** richness of `JointBinding` (one variant per driving kind) and the **field** richness of `Twist` (`angular`/`linear`) stay deferred — payload-sum → DCE (esc-2998), generic coherence → L3. |

## §5 — Out of scope

- FK-aware interference core (on main, 3906) and the swept `.map(interferes)` worked
  examples + `dock_pickup.ri` e2e (KCC 3844/3848).
- Generic `Coupling<P>` / `MotionValue<J>` dimensional typing (L3 stub → generics PRD).
- Volumetric `center_of_mass`/`bounding_box` (doc gets a v0.1-approximation caveat; the
  upgrade is a separate geometry-mass-props task, not filed here).
- `Axis`/`axis_*` constructors (owned by the geometry-transforms / G-C cluster).
- Any new core syntax or IR — §13 honours spec line 36 ("domain complexity belongs in
  community-driven libraries"); this PRD adds only stdlib-level types + compiler
  signatures + diagnostics.

## §6 — Grammar gate (G3)

No grammar prerequisite. Novel fragment confirmed against the real `reify` binary:

```reify
trait Joint { }
trait DrivingJoint : Joint { }
structure def Prismatic : DrivingJoint { param axis : Vec3 }
structure def Coupling : Joint { }
structure def Fixed : Joint { }
```
`target/debug/reify check` → exit 0 ("All constraints satisfied", module-decl warning
only). Trait-supertrait precedent: `materials_electrical.ri:51` (`trait Conductive :
ElectricallyCharacterized`). Generic-fn grammar for the L3 stub already parses (generics
PRD 4230: `guf-*.ri` fixtures parse 0-ERROR) — its *semantics* are the generics-PRD
blocker, which is why L3 is deferred, not a grammar gap.

## §7 — Cross-PRD relationship + seam-owner table (G4)

| Seam | Owner | This PRD's relationship |
|------|-------|-------------------------|
| FK-aware interference (core) | `v0_6/sub-placement-and-surfacing.md` (3906, **done**) | Consumes on main; doc-corrects the survey premise. |
| Swept `.map(interferes)` worked examples + `dock_pickup.ri` e2e | `v0_3/kinematic-constraints-completion.md` (KCC, **3844/3848 pending**) | DOC **depends-on 3848**; **flags 3844** for re-scope (3906 may supersede its FFI approach). No re-file. |
| Generic `Coupling<P>` / `MotionValue<J>` | `v0_6/generic-user-functions.md` (**4232 γ, 4235 ζ pending**) | L3 stub **depends-on 4232 + 4235**. |
| `Axis` / `axis_*` constructors | geometry-transforms / G-C cluster (`docs/prds/geometry-transforms-frames-projection.md`) | DOC notes `Axis` owned there; `revolute` runs on `Vec3` today (no hard dep). |
| Joint-ctor compiler signatures | **This PRD (β)** | New `is_joint_typed_fn` family, pinned disjoint from math/geometry/dynamics families (units.rs disjointness test). |

No new contested-ownership pair is introduced. The compiler signature family follows the
established math/geometry/dynamics-query pattern; the diagnostic emission follows the
reserve-then-emit pattern.

## §8 — G5 design posture

**Approach B (contracts) + a targeted two-way boundary test on the one genuinely-tricky
seam** (the L2 enforcement, γ). This PRD touches none of the overlay's high-stakes seams
(FEA / ComputeNode / persistent-naming / multi-kernel / grammar). Blast radius is additive
and pattern-following across reify-stdlib + reify-compiler + reify-core + docs. The
enforcement leaf γ carries the boundary test in **both** directions and at **both** layers:
`bind(couple(...), v)` is rejected (compile via γ, eval via α) **and** `bind(prismatic(...),
v)` is accepted — the H-flavoured pin on the seam where correctness matters for Leo's
driving use-case.

## §9 — Decomposition plan (one bullet per task, with observable signal)

Greek-letter batch `stdlib-mechanism-2026-06-03`. All filed `planning_mode=True`.

- **δ (delta) — diagnostic-emission seam + `E_MECHANISM_DUPLICATE_SOLID`** *(code; foundational)*
  Translate the mechanism/snapshot eval `Value::Map` `error` field → typed
  `EvalResult.diagnostics`, emitting `DiagnosticCode::MechanismDuplicateSolid` for the
  existing duplicate-solid detector. Establishes the seam α reuses.
  *Signal:* a `.ri` inserting the same `solid` twice → `reify check`/eval surfaces a
  diagnostic carrying `E_MECHANISM_DUPLICATE_SOLID` (reify-eval test asserts
  `EvalResult.diagnostics` contains the code; the Map `error` field is no longer the
  only surface). *Consumer:* end user (mechanism authoring) + GUI diagnostics panel.
  *Deps:* none.

- **α (alpha) — L1 runtime non-driving-joint guard + `E_MECHANISM_NONDRIVING_JOINT`** *(code)*
  Add `is_driving_joint()` (sibling to `is_joint_value`, JOINT_KINDS single source);
  `bind`/`sweep`/`dim` reject `coupling`/`fixed` and emit `E_MECHANISM_NONDRIVING_JOINT`
  via δ's seam. Reserve the new code in `reify-core/diagnostics.rs`.
  *Signal:* `.ri` `bind(couple(prismatic(...), -1.0), 5mm)` → eval surfaces
  `E_MECHANISM_NONDRIVING_JOINT`; `bind(prismatic(...), 5mm)` is unaffected
  (reify-eval test, both directions). *Consumer:* end user. *Deps:* δ.

- **β (beta) — L2 constructor signature family over the full §13 value vocabulary** *(code)*
  New `crates/reify-compiler/src/joint_signatures.rs`: `is_joint_typed_fn` +
  `joint_ctor_result_type` typing the §13 constructors → their nominal types:
  `prismatic/revolute/cylindrical/planar/spherical`→ their kind types,
  `couple/gear/screw/rack_and_pinion`→`Coupling`, `fixed`→`Fixed`,
  `mechanism/body`→`Mechanism`, `snapshot`→`Snapshot`, `body_id_of`→`BodyId`,
  `dim`→`SweepDim`, `bind`→`JointBinding`, `joint_jacobian`→`Twist` (D8). Wired at the
  expr.rs builtin-typing site alongside the geometry/math/dynamics families; pinned
  disjoint (units.rs test).
  *Signal:* `reify check` types `let j = prismatic(vec3(1,0,0), 0mm..1m)` as `Prismatic`
  and `let b = bind(j, 5mm)` as `JointBinding` (compiler cell-type test + LSP hover).
  *Consumer:* γ + LSP hover. *Deps:* γ (the `JointBinding`/`Twist` decls must exist to
  type into).

- **γ (gamma) — L2 `trait Joint` hierarchy + `JointBinding`/`Twist` decls + compile-time `DrivingJoint`-bound enforcement** *(code; the enforcement payoff)*
  In `kinematic.ri`: add `trait Joint {}`, `trait DrivingJoint : Joint {}`, `: Joint`
  clauses on `Coupling`/`Fixed`, and marker `structure def JointBinding {}` +
  `structure def Twist {}` (D8). Give `bind`/`sweep`/`dim` compile-time
  `DrivingJoint`-conformance checks on their joint arg (via `satisfies_trait_bound`),
  emitting `E_MECHANISM_NONDRIVING_JOINT` (naming the offending type) when given a
  `Coupling`/`Fixed`. Tighten `snapshot`'s `bindings` param toward `List<JointBinding>`.
  *Signal (two-way boundary test):* `reify check` **rejects** `bind(couple(...), v)` with
  `E_MECHANISM_NONDRIVING_JOINT` naming `Coupling`, and **accepts** `bind(prismatic(...),
  v)`. *Consumer:* end user (`reify check` / GUI). *Deps:* δ, α (shares the diagnostic
  code). *Note:* β depends on γ for the `JointBinding`/`Twist` decls; γ does **not**
  depend on β — they co-land but the `.ri` decls precede the signature wiring.

- **L3 (deferred forward-stub) — generic `Coupling<P>` / `MotionValue<J>` dimensional typing** *(code; deferred)*
  Make `couple<P: DrivingJoint>` generic; declare `Coupling<P>` and the `MotionValue<J>`
  type family with `MotionValue<Coupling<P>> = MotionValue<P>` (Length/Angle coherence).
  *Signal:* a generic coupling fn typechecks the motion-variable dimension across a
  Prismatic vs Revolute parent. *Deps:* **4232** (generics γ: trait-bounded generic fns),
  **4235** (generics ζ: dimension-param inference). Filed `deferred`, not flipped to
  `pending`.

- **θ (theta) — §13 doc-reconcile + completion + gap-register flip** *(doc; integration gate)*
  Single §13 reconcile (one task to avoid same-file lock contention): accessors→`joint_*`
  (D1); `world`→`world()` (D7); `E_KINEMATIC_CLOSED_CHAIN` reserved-but-dead; type surface
  (Joint/DrivingJoint/per-kind/Coupling/Fixed now enforced per D3; `MotionValue<J>`/
  `JointBinding`/generic `Coupling<P>`/`Twist` runtime-`Map` with the typed/generic
  spelling linked to the L3 stub; `Axis`→G-C); `center_of_mass`/`bounding_box` point-mass
  v0.1 caveat; `joint_jacobian`/`Twist` live now; §13.6 (FK-interference works in the
  build path; swept `.map(interferes)` + `dock_pickup.ri` owned by KCC 3848; `.map`/
  `.windows`/`.norm` method-call forms are non-Reify → free-function forms). Flip the 10
  P19 gap-register rows.
  *Signal:* §13 reconciled; gap-register P19 rows annotated resolved/owned; grep shows no
  `fn axis(`/`fn range(` and no `.map(|s|`/`.windows(`/`.norm()` presented as live Reify.
  *Consumer:* end user (stdlib reference + LSP/MCP autocomplete). *Deps:* α, β, γ, δ, **3848**.

## §10 — Open (tactical) questions

- δ: does the diagnostic surface at `reify check` (constraint-eval, no kernel —
  `reference_reify_eval_cli_no_solver`) or only at `reify build`? The duplicate-solid
  detection is at the mechanism-builder eval (no kernel needed), so `reify check` should
  surface it; confirm the eval boundary that owns `EvalResult.diagnostics` for the
  check path at implementation time.
- γ: the cleanest site to hang the builtin-arg `DrivingJoint`-conformance check (a
  per-builtin arg-bound table vs inline in the bind/sweep/dim typing arm). Both reach
  `satisfies_trait_bound`; impl picks.
- α/γ: whether `sweep`/`dim` should *also* reject a driving joint that contributes no
  free DOF in a degenerate range (out of scope — range validation is a separate concern).

## §11 — Adjacent unenforced-tag landscape (β/γ as a reusable pattern)

β establishes the **third** stdlib constructor-signature family (after math and
geometry/dynamics-query). It generalises: *any* "constructor builtin returns a bare
`Value::Map`/scalar that should be a nominal type" gap is enforceable by a sibling
family + nominal conformance — **no new substrate.** The discriminator is **where the
value comes from**:

| Source of the value | Enforcement mechanism | Examples | Status |
|---------------------|-----------------------|----------|--------|
| **A. A constructor builtin** | **This PRD's β/γ pattern** — `is_X_typed_fn` + `X_ctor_result_type` + nominal `: Trait` conformance | §13 joint/mechanism/binding/twist vocabulary | **In this PRD** |
| **B. A constructor builtin, other cluster** | A sibling signature family (cheap copy of β) | Tolerancing: `symmetric_tolerance()/limit_tolerance()/fit()`→`DimensionalTolerance`/`Fit` (P13); Geometry: `plane_*()/axis_*()`→`Plane`/`Axis` (G-C, once the constructors exist) | **/prd spawn** |
| **C. Inspecting BREP geometry** | Runtime kernel conformance query / InferredTraits inference | `Closed`/`Convex`/`Connected`/`Bounded` marker traits (§3.10) | **/prd spawn** (G-C) |
| **C. Parametric over a type/dimension** | **Generics** (`Type::TypeParam` threading) | `Field<D,C>`, `Scalar<Q>`, **`Coupling<P>`/`MotionValue<J>` (this PRD's L3)** | **Owned** — `v0_6/generic-user-functions.md` (4230–4235) |
| **C. A tagged union with payloads** | **Data-carrying enums** | `JointBinding` *sum* (per-kind motion value), `OutputFormat` dispatch | **Owned** — `v0_6/generic-data-carrying-enums.md` |
| **C. A trait-typed structure param with thin breadth** | Already enforced via `structure def X : Trait` (task 1874); gap is *missing params*, not tags | Ports `Bore`/`Shaft`/`MechanicalPort` (P11); material/process category traits (P12/P14) | **/prd spawn** (param-breadth, per cluster) |

**Follow-up plan (Leo, 2026-06-03).** After this PRD is decomposed, parallel `/spawn
/prd` sessions are launched — one per *authoring-needed* B/C row above:
- **B1** tolerancing constructor-return signature family (P13).
- **B2** geometry `Plane`/`Axis` signature family (G-C; gated on the `plane_*`/`axis_*`
  constructors).
- **C1** geometry marker-trait runtime conformance queries (`Closed`/`Convex`/… , §3.10).
- **C4** port / material / process trait-param breadth (P11/P12/P14).

The two remaining C rows — **parametric** (generics) and **payload-sum** (DCE) — already
have decomposed PRDs and are **not** re-spawned; this PRD's L3 stub and the deferred
`JointBinding`-sum note wire into them.
