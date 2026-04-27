# Kinematic Constraints — Closed Chains + Filled Joint Zoo

Status: deferred to v0.2 per 2026-04-28 decision (sibling to
`docs/prds/kinematic-constraints.md`).

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
- The printer-build experience identifies whether closed-chain solving is
  actually load-bearing for the user, or whether the open-chain workaround
  is acceptable. If the latter, this PRD may stay deferred indefinitely.
- The `Transform3` / `Frame3` math stdlib has accumulated enough operator
  surface (composition, inverse, log/exp for residual computation) to
  express loop-closure cleanly.
- Someone has decided on the spherical-joint parameterisation (Euler angles
  vs. unit quaternion vs. rotation vector) — has implications for
  Jacobians and singularity behaviour.

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
