# Kinematic Constraints — Forward, Open-Chain, Library-Level

## Goal

Add stdlib-level kinematic-mechanism modelling to reify so that mechanism
range-of-motion, clearance, and interference can be verified at design time.
v0.1 ships **forward kinematics over open chains** with **prismatic, revolute,
and position-coupling** joints, plus a **batch-sweep** API and an
**interference query**. Closed chains and the wider joint zoo are explicitly
deferred to v0.2 (see `v0_2/kinematic-constraints.md`).

## Background

The language spec classifies kinematic joints as a **library-level concern**
(`reify-language-spec.md:36`): "Domain complexity (GD&T, DFM rules, kinematic
joints, material databases) belongs in community-driven libraries." This PRD
honours that — no new core syntax, no new IR. Joints, mechanisms, and motion
variables are stdlib types; sweeps and queries are stdlib functions.

The connection graph already supports cycles (`reify-language-spec.md:1192`),
so closed kinematic chains are a *solver* problem deferred to v0.2, not a
language-surface problem.

What's new is the *evaluation pattern*: kinematic constraints introduce a
motion variable that has a **value range** and a **current value** that can
be swept. Existing reify constraints are static — solved once at build time.
A mechanism's snapshot is a function of the assigned motion variables, and
the same mechanism can be queried at many configurations within one
realization.

The driving use case is the printer build (`docs/projects/printer_v01.md`):
counter-mass coupling verification, toolchanger dock pickup-path clearance,
4-corner Z-tilt range, and tendon-routing sanity-checks under motion. The
feature is general — any reify mechanism design benefits — but the printer is
the dogfood validator.

## Worked example

A toolchanger head approaching a parked tool, with the head riding on a
gantry that itself rides on a Y-rail:

```reify
fn toolchanger_dock_check() -> Bool {
    // Two prismatic joints, declared as stdlib values.
    let y_axis = prismatic(axis: Y_HAT, range: 0mm .. 800mm);
    let x_axis = prismatic(axis: X_HAT, range: 0mm .. 500mm);

    // Mechanism assembly: bodies bound to joint frames.
    let m = mechanism()
        .body(frame_solid(), at: world)
        .body(gantry_solid(), at: y_axis)
        .body(toolhead_solid(), at: x_axis, parent: y_axis)
        .body(parked_tool_solid(), at: world, pose: dock_pose);

    // Sweep the head over its dock-approach path.
    let snapshots = sweep(m, x_axis, 0mm .. 500mm, steps: 50);

    // Interference query — toolhead must not collide with parked tool
    // anywhere along the path except at the final dock pose.
    let collisions = snapshots.map(|s| interferes(s));
    forall i in 0..50 - 1: collisions[i].is_empty()
}
```

The position-coupling case (counter-mass linked to head, opposite sign):

```reify
fn counter_mass_balance() -> Bool {
    let x_axis = prismatic(axis: X_HAT, range: 0mm .. 500mm);
    // Counter-mass tracks -1× the head along the same X.
    let cm_axis = couple(x_axis, ratio: -1.0);

    let m = mechanism()
        .body(toolhead_solid(), at: x_axis)
        .body(counter_mass_solid(), at: cm_axis);

    // At every position along the sweep, the system COM must stay fixed.
    let snapshots = sweep(m, x_axis, 0mm .. 500mm, steps: 11);
    let coms = snapshots.map(|s| s.center_of_mass());
    forall pair in coms.windows(2): (pair[1] - pair[0]).norm() < 0.1mm
}
```

## Scope

1. **Joint primitives** as stdlib types:
   - `Prismatic` — 1-DOF translation along a fixed axis, with motion-range bounds.
   - `Revolute` — 1-DOF rotation about a fixed axis, with angle-range bounds.
   - `Coupling` — derived joint whose motion variable is `ratio * other.value + offset`.
   These are *values* in the surface language; they expose `transform_at(v)`
   internally, returning a `Transform3` for the given motion-variable value.

2. **Mechanism assembly** as a stdlib builder:
   - `mechanism()` returns an empty mechanism.
   - `.body(solid, at: joint, parent: parent_joint?)` attaches a solid to a
     joint frame, optionally chaining off a parent joint (for serial chains).
   - `parent: world` (or omitted) attaches to the world frame.

3. **Forward-kinematics evaluator**:
   - `snapshot(mechanism, [(joint, value)...]) -> Snapshot` produces a
     concrete configuration: each body's world-frame `Transform3`. The
     snapshot value carries the placed `Solid`s and a small accessor surface.
   - Open chains only — every joint has exactly one parent path back to
     world. Closed chains (a body reachable via two distinct joint paths) are
     a v0.1 type error: `error[E_KINEMATIC_CLOSED_CHAIN]` referencing the
     two paths. Spec'd cyclic connection topology is unaffected; this is a
     mechanism-graph constraint.

4. **Sweep API**:
   - `sweep(mechanism, joint, range, steps) -> List<Snapshot>` produces N
     snapshots evenly spaced over `range`. Other joints take their range
     midpoint unless explicitly bound.
   - `sweep_grid(mechanism, [(joint, range, steps)...]) -> List<Snapshot>`
     produces the cross-product (lexicographic order) for 2-D and higher
     parametric sweeps.

5. **Interference query**:
   - `interferes(snapshot) -> List<(BodyId, BodyId)>` returns body pairs whose
     OCCT BREP intersection is non-empty, modulo a configurable tolerance.
     Excluded by default: pairs on the same chain segment (a body and its
     immediate joint frame parent — they share an edge by construction).
   - `interferes_with(snapshot, BodyId, BodyId) -> Bool` is the targeted
     scalar form.
   - Exposes `min_clearance(snapshot, BodyId, BodyId) -> Length` for non-zero
     clearance distance computation (BRep distance via OCCT).

6. **Snapshot accessors** — `snapshot.bodies()`, `snapshot.transform_of(BodyId)`,
   `snapshot.center_of_mass(densities?)`, `snapshot.bounding_box()`. The COM
   accessor is what powers the counter-mass-balance acceptance test.

7. **GUI integration** — when a mechanism value is in scope, the GUI viewer
   exposes a slider per motion variable, scrubbing through snapshots in real
   time at the granularity reify can re-evaluate at (acceptable: 100 ms per
   frame for 50-body assemblies; better is a stretch goal).

## Out of scope (covered by v0_2/kinematic-constraints.md)

- **Closed kinematic chains** (parallel mechanisms, four-bar linkages, the
  printer's own CoreXY at the joint level). v0.1 detects them as an error.
- **Joint zoo**: cylindrical (2-DOF), planar (3-DOF), spherical (3-DOF),
  fixed (0-DOF, useful for sub-assembly grouping), screw (coupled
  rotation+translation by lead pitch), gear (rotation:rotation coupling
  by ratio), rack-and-pinion (rotation→translation by pitch radius).
- **Inverse kinematics** (solve for joint values given end-effector pose).
- **Dynamics** — masses, forces, torques, time-domain simulation.
- **Contact response** — interference produces a list, not a corrected pose.
- **Soft / flexible elements** — tendons modelled as splines remain visual
  only; their kinematic coupling to capstans is expressed as `Coupling`, not
  as physical rope.

## Acceptance criteria

- New stdlib types `Prismatic`, `Revolute`, `Coupling`, `Mechanism`,
  `Snapshot` registered and documented in
  `docs/reify-stdlib-reference.md` under a new "§ Mechanism modelling"
  section.
- `cargo test -p reify-stdlib -- mechanism` exercises:
  - Single-prismatic mechanism: snapshot at three positions, transforms
    correct to floating-point precision.
  - Single-revolute mechanism: snapshot at 0°, 90°, 180°, transforms
    correct.
  - Two-link serial chain (revolute + prismatic): forward kinematics
    against analytic closed-form.
  - Coupling: `cm_axis = couple(x_axis, ratio: -1.0)`; at each x value,
    counter-mass transform is the additive inverse of the head transform
    along the coupled axis.
  - Sweep: `sweep(m, axis, 0..1, 11)` produces 11 evenly-spaced
    snapshots; first and last match `snapshot(m, [(axis, 0)])` and
    `snapshot(m, [(axis, 1)])` respectively.
  - Closed-chain detection: a mechanism with a body reachable from two
    distinct joint paths returns `error[E_KINEMATIC_CLOSED_CHAIN]` at
    `mechanism()` build time, with the diagnostic naming both paths.
- `cargo test -p reify-kernel-occt -- mechanism_interference` exercises:
  - Two non-overlapping cubes: `interferes` returns empty.
  - Two overlapping cubes: `interferes` returns the pair.
  - `min_clearance` between two non-overlapping cubes returns the
    expected gap to OCCT precision.
  - Self-pair exclusion: a single body's interference with itself is
    not reported.
- `examples/kinematic/dock_pickup.ri` and
  `examples/kinematic/counter_mass_balance.ri` ship as worked examples,
  exercised by the eval test harness.
- GUI integration: with a mechanism value in scope, the viewer renders a
  slider per joint and updates the assembly in under 200 ms per frame for
  the dock_pickup example (≤30 bodies).

## Dependencies

- **Existing**: stdlib registration pattern (analogous to topology-selectors),
  OCCT BREP machinery for the interference query (intersection +
  `BRepExtrema_DistShapeShape` for clearance), `Transform3` and `Frame3`
  types from the existing math stdlib, GUI viewer.
- **No new core-language work** — joints and mechanisms are surface-level
  stdlib values constructed from existing primitives.
- **Spec touchpoint**: line 36 ("kinematic joints belong in libraries") —
  this PRD is the realisation. No spec edit needed.

## Task breakdown (queueing aim: 9-11 tasks)

1. **Joint stdlib types** (`Prismatic`, `Revolute`) with `transform_at`
   accessor and motion-range metadata. Stdlib registration. Unit tests for
   `transform_at` against analytic transforms (translation, axis-angle).

2. **`Coupling` joint** — `couple(other_joint, ratio, offset)` deriving its
   motion-variable value from another joint's. Stdlib registration. Unit
   tests for sign and offset behaviour.

3. **`Mechanism` builder + closed-chain detector** — `.body(solid, at,
   parent?)` chaining; build-time DAG validation; closed-chain → emit
   `E_KINEMATIC_CLOSED_CHAIN` with both joint paths in the diagnostic.

4. **Forward-kinematics evaluator** — `snapshot(mechanism, bindings)`
   returning a `Snapshot` value with per-body world transforms. Unit tests
   against analytic two-link chain.

5. **Sweep API** — `sweep` (1-D) and `sweep_grid` (N-D) returning lists of
   snapshots; lexicographic ordering for grid. Unit tests for evenly-spaced
   sweep, end-point matching `snapshot(...)`, and small grid case.

6. **Snapshot accessors** — `bodies()`, `transform_of()`,
   `center_of_mass(densities?)`, `bounding_box()`. Unit tests including the
   counter-mass-balance COM-stationarity check.

7. **OCCT FFI: interference + clearance** — bind
   `BRepAlgoAPI_Common`-based intersection probe and
   `BRepExtrema_DistShapeShape` for `min_clearance`. Per-FFI happy-path
   tests on box fixtures.

8. **`interferes` / `interferes_with` / `min_clearance` stdlib bindings**
   over the FFI from task 7. Self-pair exclusion. Tests on adjacent and
   overlapping cubes.

9. **GUI viewer integration** — detect `Mechanism` values in scope; render
   per-joint sliders; re-snapshot on slider change; viewer redraw.
   Acceptance: dock_pickup example interactive at ≤200 ms per frame.

10. **Worked examples** — `examples/kinematic/dock_pickup.ri` (clearance
    sweep with assertions) and `examples/kinematic/counter_mass_balance.ri`
    (COM-stationarity with assertions). Exercised by eval test harness.

11. **Stdlib reference docs** — new "§ Mechanism modelling" section in
    `docs/reify-stdlib-reference.md` covering joint constructors, mechanism
    builder, sweep/snapshot API, interference/clearance API. Worked
    example snippet from this PRD reproduced in stdlib reference.
