# Kinematic Constraints — Closed Chains + Filled Joint Zoo

Status: deferred to v0.2 per 2026-04-28 decision (sibling to
`docs/prds/kinematic-constraints.md`).
Design resolved 2026-04-28 — see "Resolved design decisions" below.

## Goal

Lift the v0.1 restrictions on kinematic-mechanism modelling. v0.1 ships
forward kinematics over **open chains** with **prismatic / revolute /
coupling** joints only. v0.2 adds:

- **Closed kinematic chains** — parallel mechanisms, four-bar linkages, and
  the printer's own CoreXY at the joint level. v0.1 detects closed chains
  as `E_KINEMATIC_CLOSED_CHAIN`; v0.2 solves them.
- **A filled-out joint zoo** — cylindrical (2-DOF), planar (3-DOF),
  spherical (3-DOF), fixed (0-DOF, useful for sub-assembly grouping), screw
  (coupled rotation+translation by lead pitch), gear (rotation:rotation
  coupling by ratio), rack-and-pinion (rotation→translation by pitch
  radius).

## Why deferred

- Closed-chain solving requires an iterative numerical solver (Newton-style
  on the loop-closure residual) — meaningfully bigger machinery than the
  v0.1 forward-kinematics tree walk.
- The v0.1 joint set (prismatic + revolute + coupling) covers a very large
  fraction of real mechanism modelling, and the rest can usually be
  expressed as combinations: a cylindrical joint is prismatic ⊕ revolute
  on the same axis; a screw joint is a coupling between a revolute and a
  prismatic with `ratio = lead / 2π`.
- The printer build's most kinematically interesting closed chain (CoreXY)
  is workaroundable in v0.1 by treating the head as having free X/Y DOFs
  and the tendon couplings as visual annotations — clearance and
  range-of-motion verification still works.
- Inverse kinematics has overlapping solver tech with closed-chain
  loop-closure, but is a separate feature concern (different surface API,
  different use cases). IK is intentionally out of scope for v0.2 and
  would be its own PRD if motivated.

## Sketch of approach

**Closed chains.** The mechanism builder no longer rejects bodies reachable
via two joint paths. Instead, the duplicate path is recorded as a
**loop-closure constraint**: the two paths must produce the same world
transform for the shared body. The snapshot evaluator then becomes a
two-stage process:

1. Forward-traverse the mechanism's spanning tree (chosen deterministically,
   e.g. shortest path from world to each body) with the bound motion
   variables.
2. For each loop-closure constraint, compute the residual transform
   between the two paths and Newton-iterate the *unbound* motion variables
   in the loop until residuals are below a configurable tolerance (default:
   1µm and 1µrad).

If a closed chain is over-constrained (more loop residuals than free motion
variables), the snapshot returns `error[E_KINEMATIC_OVERCONSTRAINED]`. If
under-constrained (fewer residuals than free vars), the residual minimum is
non-unique; the solver returns the closest configuration to the previous
snapshot (for sweep continuity) or to the joint-range midpoint (for fresh
snapshots), with a `W_KINEMATIC_UNDERCONSTRAINED` warning suggesting an
explicit binding.

**Joint zoo.** Each new joint type is a stdlib value with its own
`transform_at(values)` accessor. Multi-DOF joints take a tuple of motion
variables (cylindrical: `(translation, rotation)`; planar: `(x, y, θ)`;
spherical: `(α, β, γ)` — Euler or quaternion, TBD by the PRD authoring
turn). Fixed joints are 0-DOF and exist purely to group sub-assemblies for
naming and clearance-pair filtering. Screw, gear, and rack-and-pinion are
specialisations of `Coupling` over multi-DOF or cross-DOF pairs and may
share implementation.

**Numerical robustness.** The loop-closure solver uses analytic Jacobians
where available (prismatic, revolute, coupling) and finite-difference
fallback for new joint types. Singularities (e.g. a parallel mechanism at
a kinematic singularity) emit `W_KINEMATIC_SINGULARITY` and return the
last-converged configuration; the snapshot accessor reports
`is_singular: true`.

**Sweep over closed chains.** The sweep API works unchanged from a user
perspective: `sweep(m, joint, range, steps)` produces snapshots. Each step
re-runs the loop-closure solver, warm-started from the previous snapshot's
free-variable values for fast convergence over a continuous sweep.

## Pre-conditions for activating

- v0.1 (forward, open-chain) has shipped and been used in anger on the
  printer build and ideally one other mechanism design. Real failure cases
  are documented.
- `Transform3` / `Frame3` stdlib expansion (task 1 in the decomposition
  below) is treated as a hard prerequisite, not a soft one — the
  loop-closure solver needs `compose`, `inverse`, `log` (residual
  extraction), `exp` (delta application), Jacobian helpers, and quaternion
  operations. If the v0.1 stdlib lacks any of these, expansion lands first.

## Resolved design decisions (2026-04-28)

**Spherical joint uses unit quaternions internally; Euler / axis-angle exposed at the snapshot facade.** Reasoning: Euler angles have gimbal-lock singularities that are *parameterisation artefacts*, not physical joint singularities — bad for a Newton solver because residuals near gimbal lock don't reflect real geometry. Rotation vectors avoid gimbal lock but the magnitude wraps at 2π and the Jacobian is ill-conditioned near identity. Unit quaternions give smooth Jacobians everywhere except the antipodal double-cover (a discrete identification, not a continuous singularity), well-defined log/exp for Newton residuals, and are the standard choice in robotics, animation, and aerospace for exactly these reasons. The user-facing snapshot API can read/write Euler or axis-angle for human readability; quaternion is the internal representation the solver works in.

**Closed-chain infrastructure ships in v0.2 regardless of immediate printer-build need.** The PRD originally hedged that this might "stay deferred indefinitely" if the printer build's open-chain workaround proved acceptable. Reframed: closed-chain machinery (loop-closure residuals, Newton iteration, warm-starting across sweeps, analytic Jacobians + finite-difference fallback) is foundational for richer kinematics including future IK and parallel mechanisms. Building it once is the gate to those features. Activation timing is driven by v0.1-alpha experience surfacing real demand, not by whether the printer build alone needs it.

**Joint-zoo implementation strategy.** Three of the seven new joints reduce cleanly to v0.1's `Coupling` primitive — implement them as thin parameterisations sharing infrastructure:
- Screw: `Coupling(rotation, translation, ratio = lead / 2π)`
- Gear: `Coupling(rotation_a, rotation_b, ratio = -teeth_b / teeth_a)` (negative for external mesh)
- Rack-and-pinion: `Coupling(rotation, translation, ratio = pitch_radius)`

The remaining four are new primitives:
- **Cylindrical** (2-DOF): composite of prismatic ⊕ revolute on shared axis. Could be implemented as a composite of v0.1 primitives.
- **Planar** (3-DOF): composite of two prismatic + one revolute, all in-plane.
- **Spherical** (3-DOF): quaternion-internal, full new implementation.
- **Fixed** (0-DOF): trivial identity transform; exists for sub-assembly grouping (naming, clearance-pair filtering).

**Loop-closure solver.** Newton iteration on stacked-transform residual. Default convergence tolerances: 1µm position, 1µrad rotation (configurable). Warm-start from previous snapshot's free-variable values for sweep continuity; joint-range midpoint for fresh snapshots. Analytic Jacobians for prismatic, revolute, coupling (and the three Coupling-specialisations); finite-difference fallback for spherical, cylindrical, planar until analytic forms are derived.

**Singularity, over/under-constraint diagnostics.** Per the PRD sketch, retained: `W_KINEMATIC_SINGULARITY` with last-converged config returned and `is_singular: true` flag; `E_KINEMATIC_OVERCONSTRAINED` when residuals exceed free DOFs; `W_KINEMATIC_UNDERCONSTRAINED` with closest-to-previous config returned. This matches the right CAD-context behaviour — silent failure or no-output would be worse.

## Decomposition plan (10 tasks)

1. **`Transform3` / `Frame3` stdlib expansion.** Hard prerequisite. `log` / `exp`, Jacobian helpers, quaternion ops (compose, inverse, log, exp, slerp where useful, conversions to/from Euler and axis-angle).
2. **Loop-closure residual machinery.** Stack-of-transforms residual, Newton iteration loop, warm-start scaffolding, convergence-tolerance plumbing.
3. **Mechanism builder accepts closed chains.** Replace `E_KINEMATIC_CLOSED_CHAIN` rejection with loop-constraint recording; choose spanning tree deterministically (shortest-path-from-world default).
4. **Spherical joint.** Quaternion-internal solver, Euler/axis-angle facade for snapshots.
5. **Cylindrical joint.** Prismatic ⊕ revolute on shared axis composite.
6. **Planar joint.** Two-prismatic-plus-revolute in-plane composite.
7. **Fixed joint.** 0-DOF, group-only.
8. **Coupling specialisations: screw, gear, rack-and-pinion.** Single task; thin parameterisations sharing the v0.1 `Coupling` infrastructure.
9. **Singularity & over/under-constraint diagnostics.** `W_KINEMATIC_SINGULARITY`, `E_KINEMATIC_OVERCONSTRAINED`, `W_KINEMATIC_UNDERCONSTRAINED` with the snapshot fallback semantics described above.
10. **Sweep API integration.** Verify `sweep(m, joint, range, steps)` over a closed-chain mechanism works with warm-start across steps.

Mostly serial: 1 → 2 → 3, then 4–7 parallel, then 8–10. Task 8 only depends on 1+2 (it's all v0.1 `Coupling`). Tasks 9+10 depend on the new joints landing.

## Out of scope for this PRD

- **Inverse kinematics** — given desired end-effector pose, solve for joint
  values. Separate feature with overlapping solver tech.
- **Dynamics** — masses, forces, torques, time-domain simulation,
  Lagrangian / Newton-Euler dynamics. Different mathematical machinery
  entirely.
- **Contact / collision response** — interference produces a list of
  pairs; no corrected pose, no contact forces.
- **Compliant mechanisms** — flexure-hinge approximations, beam elements.
  Tendons remain visual splines.
- **Cable / belt physics** — even though the printer uses Vectran tendons
  and capstan drives, their kinematic role is expressed as `Coupling`,
  not as a physical rope element.
- **Path planning / trajectory generation** — separate concern; if added,
  belongs in its own stdlib module.
