# Dynamic-Modelling Fidelity Expansion — Roadmap

Status: stub roadmap, authored 2026-05-17. NOT a full PRD; serves as a
durable index of fidelity-expansion possibilities surfaced during the
2026-05-17 four-PRD design session (kinematic-constraints-completion +
rigid-body-dynamics + modal-analysis + trajectory-input-shaping +
compliant-joints-flexures). Each entry below is a candidate future PRD
slot; authoring requires its own design session under `/prd`.

## §0 — Purpose

This document records the **fidelity-expansion landscape beyond the v0.3
dynamic-modelling stack**. The stack as designed in 2026-05-17 (the
five PRDs cited above) covers:

- Kinematics: forward + closed-chain + multi-DOF + diagnostics
- Rigid-body dynamics: inverse dynamics
- Free vibration + Rayleigh-damped transient response
- Time-optimal + impulse-shaped motion profiles
- Compliant joints + PRB flexure models

Real-world ultra-performant 3D printer design uses more than this. The
"more" splits into clean fidelity axes; for each axis there is a
plausible v0.4+ PRD slot that extends the v0.3 stack. This roadmap names
each axis, notes its foundation, and sketches its scope so that future
`/prd` sessions can pick them up without re-discovering the landscape.

**This is a stub document.** Each entry below is a placeholder, not a
contract. Authoring a real PRD against any entry means running the full
`/prd` author-mode session for that scope.

## §1 — Foundation-axis expansions

These extend the v0.3 stack along its principal axes.

### §1.1 — Forward dynamics (closed-loop simulation)

- **What.** Given torque/force time history → integrate ODE for
  `(θ(t), θ̇(t))`. Required for: drop-test simulation, motor-stall
  sim, control-loop tuning.
- **Foundation.** rigid-body-dynamics (inverse dynamics already
  ships); ODE solver dependency choice (Runge-Kutta family vs implicit
  Euler for stiff systems).
- **Complexity.** Mid. ODE-solver-choice debate + time-step stability.
- **Cross-refs.** Bookmarked in
  `docs/prds/v0_3/rigid-body-dynamics.md` §2.2 (Phase λ filed a deferred
  task at decompose time).

### §1.2 — Joint friction (Coulomb + viscous)

- **What.** Add per-joint friction-force term to inverse-dynamics
  backward-pass: `τ_friction = -μ_v·θ̇ - μ_c·sign(θ̇)`. Coulomb
  needs regularization (smooth `sign` near zero).
- **Foundation.** rigid-body-dynamics; per-joint friction parameter on
  the Joint structure_def.
- **Complexity.** Low-mid. Regularization choice + parameter
  identification.
- **Cross-refs.** Bookmarked in
  `docs/prds/v0_3/rigid-body-dynamics.md` §11.

### §1.3 — Inverse kinematics

- **What.** Given desired end-effector pose → joint values that
  produce it. Required for Cartesian-space motion authoring (the
  natural way for printer toolpaths).
- **Foundation.** kinematic-constraints-completion (closed-chain
  solver reusable for redundant chains).
- **Complexity.** Mid. Newton-Raphson on FK residual; singularity
  handling; redundancy resolution for >6-DOF chains.
- **Cross-refs.** Out-of-scope of
  `docs/prds/v0_2/kinematic-constraints.md`; named in
  `docs/prds/v0_3/kinematic-constraints-completion.md` §13.

### §1.4 — Complex-eigenvalue / non-proportional damping

- **What.** Solve `(K + iωC - ω²M)x = 0` quadratic eigenvalue problem.
  Required for systems with localized damping (rubber mounts, fluid
  dampers, viscoelastic-bonded joints).
- **Foundation.** modal-analysis (extends from real to complex
  eigensolver path); complex-arithmetic Lanczos or QZ algorithm.
- **Complexity.** High. Quadratic-eigenvalue-problem solvers are
  more delicate than symmetric Lanczos.
- **Cross-refs.** Bookmarked in
  `docs/prds/v0_3/modal-analysis.md` §2.2.

### §1.5 — Full-mechanism flexible multibody (Craig-Bampton)

- **What.** Combine per-Part modes across mechanism joint interfaces
  via component-mode synthesis (Craig-Bampton: fixed-interface modes +
  constraint modes → reduced-order assembled model). Captures cross-
  body modes (gantry + toolhead coupling).
- **Foundation.** modal-analysis (per-Part modes); mechanism joint-
  interface DOF identification.
- **Complexity.** High. Substructure assembly, interface-DOF reduction,
  super-element formulation. 3–5× scope of per-Part modal alone.
- **Cross-refs.** Bookmarked in
  `docs/prds/v0_3/modal-analysis.md` §2.2.

### §1.6 — Geometrically nonlinear beam FEA

- **What.** Large-deflection beam theory (corotational beam / Cosserat
  rod). Required for serious flexure design (real flexures operate at
  5–30° deflection, beyond small-strain linear).
- **Foundation.** `reify-solver-elastic` (small-strain linear elastic);
  new corotational-update kinematics in element assembly.
- **Complexity.** High. Substantial FEA-stack extension; iterative
  Newton-Raphson at global level.
- **Cross-refs.** Bookmarked in
  `docs/prds/v0_3/compliant-joints-flexures.md` (forthcoming).

### §1.7 — Fatigue analysis (S-N for flexures)

- **What.** Given a flexure's cyclic loading and material S-N curve →
  cycles-to-failure prediction. Required for flexures (S-N-cycle-
  limited) and cyclically-loaded structural members.
- **Foundation.** FEA static analysis (stress under deflection); new
  S-N material trait (`FatigueLimit`).
- **Complexity.** Mid. Material data ingestion + Miner's-rule
  accumulation; not yet a current Reify need.
- **Cross-refs.** Will be bookmarked in
  `docs/prds/v0_3/compliant-joints-flexures.md`.

### §1.8 — Pose-parametric modal models

- **What.** Modes vary with mechanism pose (position-dependent
  inertia). modal_analysis becomes a function of snapshot, not just
  Part. Required for serious crane-arm / long-cantilever printer
  topologies.
- **Foundation.** modal-analysis (current per-Part snapshot-
  invariant); mechanism-state-dependent K, M assembly.
- **Complexity.** High. Mode-tracking across continuous parameter
  variation; interpolation in mode space.
- **Cross-refs.** Bookmarked in
  `docs/prds/v0_3/trajectory-input-shaping.md` §12.6.

## §2 — Subsystem expansions

These add new dynamic-modelling subsystems orthogonal to the v0.3 stack.

### §2.1 — Actuator dynamics

- **What.** Stepper motor torque-vs-speed curve, back-EMF, current
  limit, thermal limit, missed-step prediction. Required for realistic
  acceleration ceilings.
- **Foundation.** rigid-body-dynamics (provides required torque
  trajectory); motor-model crate / stdlib.
- **Complexity.** Mid. Stepper-model literature is mature.

### §2.2 — Stepper-motor electromechanical model

- **What.** Phase currents → torque (full electromechanical model with
  cogging, microstepping, current chopping). One layer below §2.1's
  curve-based model.
- **Foundation.** §2.1; numerical-integration ODE solver.
- **Complexity.** High. Substantial domain-specific modelling.

### §2.3 — Closed-loop control simulation

- **What.** Simulate encoder feedback + PID / observer-based control
  + notch filters. Required for closed-loop steppers and servos.
- **Foundation.** §2.1 / §2.2 (motor model); forward dynamics from §1.1.
- **Complexity.** High. Controller-tuning surface + simulation harness.

### §2.4 — Belt + capstan-drive dynamics

- **What.** Belt elasticity (Young's modulus + cross-section + length),
  capstan friction, slip. Tendon-routing dynamics for cable-driven
  mechanisms. Currently `Coupling` joints assume rigid linear coupling.
- **Foundation.** kinematic-constraints (Coupling joint extension);
  rigid-body-dynamics force balance.
- **Complexity.** Mid. Standard belt/cable-drive theory (Kabuto, …).

### §2.5 — Thermal-mechanical coupling

- **What.** Frame thermal expansion under heatbed thermal load.
  Affects geometric accuracy on long prints. Couples thermal solver
  (not yet in Reify) with FEA-stack.
- **Foundation.** New thermal solver (heat-conduction PDE); FEA-stack
  coupling via thermal-strain term in stiffness.
- **Complexity.** High. New solver domain entirely; requires its own
  PRD family.

### §2.6 — Print-head extrusion dynamics

- **What.** Melt rheology, pressure advance, oozing, retraction
  dynamics. Determines achievable line-width accuracy at speed.
- **Foundation.** New fluid solver (low-Reynolds non-Newtonian flow);
  cross-cuts trajectory-input-shaping.
- **Complexity.** Very high. Fluid simulation is a substantial
  domain extension.

### §2.7 — Print-firmware lookahead / junction-deviation simulation

- **What.** Simulate Klipper/Marlin trajectory-planner behaviour
  (junction deviation, S-curve LookAhead, pressure advance). Required
  for accurate prediction of actual printer behaviour vs commanded.
- **Foundation.** trajectory-input-shaping (motion profile surface);
  firmware-model crate or stdlib.
- **Complexity.** Mid. Each firmware's algorithm is documented.

### §2.8 — Online TOTS (real-time replan)

- **What.** Time-optimal trajectory shaping that replans during
  motion. Bridges design-time analysis to runtime control.
- **Foundation.** trajectory-input-shaping TOTS (design-time);
  real-time optimization-loop infrastructure.
- **Complexity.** High. Embedded-soft-real-time, predictability
  constraints; out of scope for design-time Reify but interesting
  long-term.

### §2.9 — Wear / friction degradation

- **What.** Friction parameters drift over operating cycles; bearings
  wear; belts stretch. Long-term machine-health prediction.
- **Foundation.** §1.2 (joint friction); operational-history tracking.
- **Complexity.** Mid; primarily a parameter-identification + drift-
  model story.

## §3 — Numerical / infrastructure expansions

These improve solution quality / performance of the v0.3 stack without
changing the model.

### §3.1 — Adaptive timestepping (modal-analysis transient_response, dynamics integration)

- **What.** Vary `dt` per local-error estimate (Richardson extrapolation
  on torque error or modal-response amplitude). Trades wall-clock for
  accuracy.
- **Foundation.** modal-analysis transient solver; ODE-integrator
  abstraction.
- **Complexity.** Low-mid. Standard PI-controller-style step
  controllers.

### §3.2 — Higher-order modal-superposition integrators

- **What.** Move from exact-Duhamel-per-step + Newmark-β to higher-
  order schemes (HHT-α, generalized-α) for better damping
  representation.
- **Foundation.** modal-analysis transient_response.
- **Complexity.** Low. Replace inner integrator function.

### §3.3 — Multi-body sparse-Jacobian dynamics

- **What.** Replace dense Lagrange-multiplier system with sparse
  block-tridiagonal solver. Required for mechanisms with many
  closed loops (multi-loop linkages, parallel mechanisms with
  many limbs).
- **Foundation.** rigid-body-dynamics; sparse-linear-algebra
  dependency.
- **Complexity.** Mid. Sparse-linalg dependency choice.

### §3.4 — TOTS heavy-solver backend (osqp / ipopt / argmin)

- **What.** Replace in-house SQP with an industrial-grade
  optimization solver. Required if printer-build TOTS perf falls
  short.
- **Foundation.** trajectory-input-shaping TOTS.
- **Complexity.** Low; primarily a dependency-swap PRD.

### §3.5 — Modal-correlation / MAC against experimental data

- **What.** Compare computed modes to experimental modal-analysis
  results (FRF-derived modes from a real printer). Modal Assurance
  Criterion (MAC) is the standard metric.
- **Foundation.** modal-analysis (computed modes); ingestion path
  for experimental FRF / mode data.
- **Complexity.** Mid. Data-ingestion + comparison logic; not yet
  a Reify need.

### §3.6 — Damping identification from experimental data

- **What.** Reverse-engineer Rayleigh α, β from experimental FRF
  data; or full per-mode damping coefficients for non-proportional
  systems.
- **Foundation.** modal-analysis; §3.5 ingestion.
- **Complexity.** Mid. Curve-fitting infrastructure.

## §4 — Authoring-ergonomics expansions

These reduce friction in dynamics-stack authoring.

### §4.1 — Cartesian inverse-kinematics + waypoint authoring

- **What.** Cartesian-space waypoints with IK to joint space. The
  natural authoring mode for printer toolpaths.
- **Foundation.** §1.3 (inverse kinematics);
  trajectory-input-shaping (joint-space-only today).
- **Complexity.** Mid.

### §4.2 — Multi-mechanism MotionProfile bundling

- **What.** Profiles tied to a shared time axis (extruder + XY +
  Z + multi-tool changers). v0.3 binds one profile to one mechanism.
- **Foundation.** trajectory-input-shaping.
- **Complexity.** Low. Mostly bookkeeping.

### §4.3 — Scrub-as-edit for literal-bound joints

- **What.** GUI slider for a literal-bound joint writes back to the
  `.ri` source AST. v0.3 ships session-only scrub.
- **Foundation.** kinematics-completion η; MCP edit-loop infra.
- **Complexity.** Low-mid; edit-loop coupling is the main risk.

### §4.4 — Additional G-code dialects

- **What.** Smoothie, RepRap, Mach3, GRBL, …. v0.3 ships Marlin +
  Klipper.
- **Foundation.** trajectory-input-shaping G-code module.
- **Complexity.** Per-dialect. Reuses parser harness; only the
  dialect-specific extensions vary.

### §4.5 — Bezier / Catmull-Rom spline alternatives

- **What.** Alternate basis for piecewise-polynomial profiles.
  Bezier control points at endpoints; Catmull-Rom C¹-smooth through
  waypoints.
- **Foundation.** trajectory-input-shaping spline evaluator.
- **Complexity.** Low.

### §4.6 — Multi-flexure assemblies (compound flexure stages)

- **What.** Roberts parallelogram, butterfly flexure, etc. Compound
  flexures combine multiple flexure elements for performance
  characteristics no single flexure delivers. Currently PRB models
  one flexure as one Joint; compound stages need assembly
  primitives.
- **Foundation.** compliant-joints-flexures (forthcoming).
- **Complexity.** Mid. Library of compound-flexure primitives.

## §5 — Domain expansions

Possibilities further afield from printer design.

### §5.1 — Acoustic radiation modes

- Coupling FEA modes to radiation-impedance models. Relevant for
  noise-design (mostly outside Reify's current focus).

### §5.2 — Aeroelastic / fluid-coupled modes

- Modal analysis with fluid coupling (flutter, divergence,
  fan-blade dynamics). Domain-specific.

### §5.3 — Periodic / Floquet modal analysis

- Parametrically excited systems (cyclic loading parameters).
  Rotating machinery. Out of Reify's current focus.

### §5.4 — Modal optimization / topology optimization

- Use modal frequencies as design objectives (e.g. "find topology
  maximizing first-mode frequency subject to mass budget").
  Crosses into topology-optimization land — large PRD family.

### §5.5 — Body-fixed external forces (aero / magnetic / wind)

- Per-body force/torque inputs to inverse dynamics beyond gravity.
  Aerodynamic drag for fast-moving toolheads. Magnetic forces for
  e-motor / Halbach-array applications.
- **Foundation.** rigid-body-dynamics force balance.
- **Complexity.** Low; mostly bookkeeping + force-source library.

## §6 — How to use this document

When a future `/prd` session would benefit:

1. Skim sections that match the user's stated motivation (e.g. "I
   want to model belt stretch" → §2.4; "I want accurate flexure
   design" → §1.6, §1.7, §4.6).
2. Cite the relevant entry's foundation list as the pre-condition
   gate.
3. Run the full `/prd` author-mode session for that entry; this
   roadmap is not a substitute for design conversation.

Maintenance: append new entries as new fidelity axes surface; keep
the entries terse (1 paragraph per axis maximum).

End of roadmap.
