# Trajectory + Input Shaping

Status: contract authored 2026-05-17 in interactive `/prd` session. Sibling
to `kinematic-constraints-completion.md`, `rigid-body-dynamics.md`,
`modal-analysis.md`, `compliant-joints-flexures.md`. Pending Leo approval
before queueing tasks.

## §0 — Purpose and supersession

The three sibling PRDs (kinematics-completion, rigid-body-dynamics,
modal-analysis) ship the building blocks for analyzing mechanism motion,
forces, and structural vibration. This PRD ties them together into the
**user-facing primitive that ultra-performant 3D printer design actually
wants**: given a mechanism, modal model, and a desired motion (point-to-
point, contour, or imported G-code), compute a motion profile that
minimizes printing time subject to vibration and actuator constraints.

Concretely, the PRD owns:

- A **piecewise-polynomial motion-profile DSL** for parametric trajectory
  authoring — this is the **primary authoring surface**.
- A **shaper family**: ZV, ZVD, EI (classical impulse shapers) plus
  Time-Optimal Trajectory Shaping (TOTS) for optimization-driven motion.
- A **forward-pass simulator** that consumes modal + kinematic + dynamics
  to predict end-effector trajectory under a candidate motion profile,
  used both by the user (analysis) and internally by TOTS (iteration).
- **G-code import as an OPTIONAL bolt-on** (`gcode_import(file, dialect)`),
  for analyzing real-world slicer output. The PRD is fully usable
  without G-code; the parametric DSL is the canonical CI-gate path.
  Pure-`.ri` workflows let users prove out their mechanism + modal +
  shaper design before ingesting slicer output.

No supersession; greenfield for the v0.3 line.

## §1 — Goal and user-observable surface

A Reify user can author a printer motion profile parametrically (or
import slicer G-code), apply input shaping or time-optimal shaping
against the modal model of the mechanism, and observe the predicted
end-effector trajectory error. Concretely:

```reify
// examples/trajectory/zv_shaped_ramp.ri
let mech = printer_toolhead_mechanism();
let modal = modal_analysis(printer_gantry_part(), default_opts());

// Parametric profile: piecewise-cubic spline through waypoints.
let profile = piecewise_polynomial([
    Waypoint(t: 0s,    pose: pose_at(0mm, 0mm)),
    Waypoint(t: 0.2s,  pose: pose_at(50mm, 0mm)),
    Waypoint(t: 0.4s,  pose: pose_at(100mm, 100mm)),
], boundary: NaturalSpline);

// Apply ZV shaper at the gantry's first mode.
let shaped = input_shape(profile, ZVShaper(
    target_frequency: modal.modes[0].frequency,
    damping_ratio:    modal.modes[0].damping_ratio,
));

// Forward-pass simulation: rigid-body kinematics + modal superposition.
let track = simulate_trajectory(shaped, mech, modal);
let tip_error = track.deviation_from_nominal(at: gantry_toolhead_mount);
report("peak end-effector deviation = " ++ show(max(tip_error)));
```

```reify
// examples/trajectory/tots_optimal_ptp.ri
let baseline = piecewise_polynomial([
    Waypoint(t: 0s,   pose: pose_at(0mm,   0mm)),
    Waypoint(t: 1.0s, pose: pose_at(500mm, 200mm)),
], boundary: NaturalSpline);

let optimal = input_shape(baseline, TOTSShaper(
    modes:               modal.modes,
    actuator_limits:     printer_motor_torque_limits(),
    vibration_tolerance: 0.02,
    velocity_limit:      300mm/s,
    acceleration_limit:  5000mm/s^2,
));

report("baseline duration = " ++ show(baseline.duration));
report("TOTS-optimal duration = " ++ show(optimal.duration));
// expected: optimal.duration < baseline.duration (subject to constraints)
```

```reify
// examples/trajectory/gcode_import_smoke.ri
let gcode = read_file("test_data/calibration_cube.gcode");
let imported = gcode_import(gcode, dialect: Marlin);
// imported : List<MotionProfile>
let analyzed = simulate_trajectory(imported[0], mech, modal);
plot(analyzed.end_effector_track(toolhead_tip));
```

**CI-gate signals (pure-`.ri`; G-code-free):**

- `cargo test -p reify-eval --test trajectory_e2e` runs four
  fixtures and they all pass without touching any G-code path:
  - `examples/trajectory/zv_shaped_ramp.ri` — ZV-shaped step input
    against a single-mode oscillator. Tip-vibration peak reduced by
    ≥ 40 dB vs unshaped baseline.
  - `examples/trajectory/zvd_robustness.ri` — ZVD shaper applied with
    ±10% modal-frequency error. Vibration peak stays within design
    spec.
  - `examples/trajectory/ei_robustness.ri` — EI shaper applied with
    ±15% modal-frequency error. Vibration ≤ 5% residual (the EI
    design parameter).
  - `examples/trajectory/tots_optimal_ptp.ri` — TOTS-optimal P2P
    motion against a 1-DOF gantry + single mode. Solver converges in
    ≤ 100 iterations; resulting duration is shorter than unshaped
    baseline AND vibration constraint is satisfied (within
    numerical tolerance).

**Optional G-code bolt-on signal (separate test gate, may be opt-in):**

- `examples/trajectory/gcode_import_smoke.ri` — import a 100-line
  Marlin G-code fixture; produce a `List<MotionProfile>` and
  forward-simulate the first segment without errors. This signal is
  gated separately so the PRD's core acceptance does not depend on
  the G-code parser being feature-complete.

**Dogfood signal (uses G-code OR pure-`.ri`):**

- `examples/trajectory/printer_print_envelope.ri` — printer-build
  dogfood fixture; analyzes a full print path's end-effector error
  envelope under input-shaped and TOTS-optimal motion. Used as the
  print-quality budget reference. Default fixture is a parametric
  `.ri`-generated print path; an alternate G-code-driven variant
  exercises the bolt-on path.

**Diagnostic signals:**

- `E_TrajectoryConstraintInfeasible` — TOTS solver finds no feasible
  motion profile satisfying all constraints (e.g. impossible
  combination of vibration tolerance and actuator limits).
- `W_TrajectorySolverNonConvergence` — TOTS hit `max_iters` without
  optimality certificate; returned best feasible iterate.
- `E_GcodeParseError` — G-code import failure; carries line number
  and dialect-specific reason.
- `W_GcodeDialectUnsupported` — feature in the G-code stream not
  supported by the named dialect (e.g. M-command not in the
  recognized set); ignored with warning.

## §2 — Scope

### §2.1 — Mechanism table

| # | Mechanism | State today | Owner |
|---|---|---|---|
| α | `MotionProfile` value type — piecewise-polynomial (cubic / quintic) spline through waypoints | NEW | this PRD |
| β | `Waypoint`, `BoundaryCondition` (Natural, Clamped, Periodic) value types | NEW | this PRD |
| γ | `piecewise_polynomial(waypoints, boundary)` stdlib ctor + B-spline evaluator | NEW | this PRD |
| δ | `to_trajectory_samples(profile, dt) -> MotionTrajectory` bridge to rigid-body-dynamics | NEW | this PRD |
| ε | `Shaper` trait + `ZVShaper`, `ZVDShaper`, `EIShaper` (impulse-shaper family) | NEW | this PRD |
| ζ | `TOTSShaper` value type + optimization-based time-optimal shaping (SQP on B-spline control points) | NEW | this PRD |
| η | `input_shape(profile, shaper) -> MotionProfile` stdlib fn — dispatches across Shaper variants | NEW | this PRD |
| θ | `EndEffectorTrack` value type — time-history of pose + velocity + (optional) accel at named locations | NEW | this PRD |
| ι | `simulate_trajectory(profile, mech, modal) -> EndEffectorTrack` stdlib fn — forward-pass with modal-superposition | NEW | this PRD |
| κ | G-code parser library (Marlin + Klipper dialects in v0.3; future extensible) | NEW | this PRD (new `reify-gcode` crate) |
| λ | `gcode_import(source, dialect) -> List<MotionProfile>` stdlib helper | NEW | this PRD |
| μ | ComputeNode trampoline for `simulate_trajectory` (TOTS iteration calls this) | NEW | this PRD |
| ν | ComputeNode trampoline for `input_shape` w/ TOTSShaper (heavy optimization-driven) | NEW | this PRD |

### §2.2 — Out of scope

- **Forward dynamics simulation** (closed-loop control sim). Bookmarked
  under `rigid-body-dynamics.md`. Trajectory simulation here is
  open-loop: prescribed motion → predicted vibration response.
- **Online / real-time motion control.** Shaping is design-time;
  Reify is not Klipper/Marlin.
- **Toolpath generation / slicer logic.** G-code is consumed, not
  produced.
- **Multi-axis coordination beyond mechanism's driving joints.**
  Multi-axis profiles compose per-joint MotionProfiles; coordinated
  motion across asynchronous axes (e.g. extruder + XY) is encoded
  via independent profiles sharing a time axis.
- **Other G-code dialects** (Smoothie, RepRap, Mach3, …). v0.3 ships
  Marlin + Klipper; other dialects filed as future PRD slot.
- **Cartesian inverse-kinematics** for delta / SCARA printers. Joint
  space input only in v0.3.
- **Online TOTS** (replanning during print). Out of scope; v0.3 is
  design-time analysis.

## §3 — Pre-conditions for activating

| Pre-condition | Owner | Status today | Gate phase |
|---|---|---|---|
| `Snapshot.free_values: List<JointValue>` stable | `kinematic-constraints-completion.md` task α-pre | this session | hard prereq for ι |
| `inverse_dynamics(...)` stdlib fn | `rigid-body-dynamics.md` task η | this session | hard prereq for ι (forcing assembly) |
| `transient_response(...)` stdlib fn | `modal-analysis.md` task ι | this session | hard prereq for ι |
| `ForcingTimeHistory` value type | `modal-analysis.md` task η | this session | hard prereq for ι |
| SIR-α | `structure-instance-runtime.md` task 3540 | in-flight | hard prereq for α, β, ε, ζ, θ |
| ComputeNode contract | landed | landed | substrate for μ, ν |
| Optimization solver dependency choice (faer-rs SQP / new crate) | TBD at ζ task | open | hard prereq for ζ — Open Question §12.1 |

The forward-pass simulator ι is the load-bearing seam: it consumes
inverse_dynamics + transient_response, so its dependency edge chain
runs through three sibling PRDs in this session. Per
[[preferences_cross_prd_deps_real_edges]] all edges wire as real
`add_dependency` at decompose time.

## §4 — Contract: MotionProfile + Waypoint

### §4.1 — MotionProfile and piecewise-polynomial primitive

```reify
trait Profile {
}

structure def Waypoint {
    param t      : Time              // sample time
    param values : List<JointValue>  // per-driving-joint position
    param vels   : Option<List<JointValue>>   // optional velocity tangent
    param accels : Option<List<JointValue>>   // optional acceleration tangent
}

trait BoundaryCondition {
}

structure def NaturalSpline : BoundaryCondition {
    // zero second derivative at endpoints
}

structure def ClampedSpline : BoundaryCondition {
    param start_velocity : List<JointValue>   // q̇ at first waypoint
    param end_velocity   : List<JointValue>   // q̇ at last waypoint
}

structure def PeriodicSpline : BoundaryCondition {
    // first and last waypoint agree (closed-loop motion)
}

structure def PiecewisePolynomialProfile : Profile {
    param mechanism   : Mechanism
    param waypoints   : List<Waypoint>
    param boundary    : BoundaryCondition
    param spline_kind : SplineKind    // CubicSpline | QuinticSpline
}

// Evaluator helpers (stdlib fns; defined as compiler-known intrinsics):
fn evaluate_profile(p: Profile, t: Time) -> List<JointValue>
fn evaluate_profile_dot(p: Profile, t: Time) -> List<JointValue>
fn evaluate_profile_ddot(p: Profile, t: Time) -> List<JointValue>
fn profile_duration(p: Profile) -> Time
```

The B-spline evaluator uses **de Boor recursion** for stable
evaluation; cubic and quintic are the supported orders for v0.3.
Cubic is the default; quintic is selected when waypoints carry
explicit `vels` AND `accels` (Hermite-like constraints).

**Coordinate space.** Waypoints carry per-driving-joint values, so the
profile is in joint space (not Cartesian space). For Cartesian
trajectories users compose `inverse_kinematics(mech, cartesian_pose)
-> List<JointValue>` (filed as future PRD) with waypoint generation.
v0.3 ships joint-space waypoints only.

### §4.2 — Bridge to MotionTrajectory

`MotionProfile` is parametric (continuous in time); `MotionTrajectory`
(from `rigid-body-dynamics.md` §4.3) is sample-based (discrete time
grid). The bridge:

```reify
fn to_trajectory_samples(p: Profile, dt: Time) -> MotionTrajectory
```

samples the profile at uniform `dt` intervals over `[0, profile_duration(p)]`,
producing `TrajectorySample` records with `(q, q̇, q̈)` from the
profile's evaluator helpers.

## §5 — Contract: Input shapers

### §5.1 — Impulse-shaper family

```reify
trait Shaper {
}

structure def ZVShaper : Shaper {
    param target_frequency : Frequency
    param damping_ratio    : Real           // default: 0
}

structure def ZVDShaper : Shaper {
    param target_frequency : Frequency
    param damping_ratio    : Real
}

structure def EIShaper : Shaper {
    param target_frequency    : Frequency
    param damping_ratio       : Real
    param vibration_tolerance : Real        // residual fraction (e.g. 0.05)
}

// Multi-mode shapers: convolution of single-mode shapers.
structure def CascadedShaper : Shaper {
    param shapers : List<Shaper>            // applied in sequence
}
```

**Impulse-shaping semantics.** A shaper convolves the original
`MotionProfile` with a finite-impulse train (2 impulses for ZV; 3 for
ZVD; 4 for EI). For a profile of duration T, the shaped profile has
duration T + Δ where Δ is the shaper's "trailing time" (typically a
half-period of the target frequency).

**Impulse times and amplitudes** are standard textbook (Singer & Seering,
1990; Singhose 1996):
- ZV: t_1 = 0, t_2 = π/(ω_d); A_1 = 1/(1+K), A_2 = K/(1+K) where K = exp(-ζπ/√(1-ζ²)).
- ZVD: three-impulse extension setting d/dω(residual) = 0 at design ω.
- EI: four-impulse, parameterized by `vibration_tolerance`.

### §5.2 — TOTS shaper

```reify
structure def TOTSShaper : Shaper {
    param modes               : List<Mode>   // target ringing modes
    param actuator_limits     : List<JointLimit>  // per joint
    param velocity_limit      : Velocity     // global joint-velocity cap
    param acceleration_limit  : Acceleration // global joint-accel cap
    param vibration_tolerance : Real         // residual fraction
    param max_iters           : Int          // default: 100
    param tol                 : Real         // default: 1e-6
}

structure def JointLimit {
    param joint     : Joint
    param max_force : Force                  // or torque per kind
}
```

**TOTS algorithm.** The shaped profile parameterizes the same B-spline
basis as the input `MotionProfile`. Optimization variables: spline
control points + total duration T. Objective: minimize T.
Constraints:

1. `‖shaper_output_residual_vibration‖_∞ ≤ vibration_tolerance` —
   evaluated by calling `simulate_trajectory` and reading
   `modal_response.peak_amplitude_per_mode`.
2. Per-joint velocity: `|q̇_i(t)| ≤ velocity_limit` ∀ t.
3. Per-joint acceleration: `|q̈_i(t)| ≤ acceleration_limit` ∀ t.
4. Per-joint actuator force: `|τ_i(t)| ≤ JointLimit.max_force` ∀ t —
   evaluated by calling `inverse_dynamics(mech, samples).each.value.scalar_force_or_torque`.

The optimization is solved via **sequential quadratic programming
(SQP)** on the linearized subproblem, using faer-rs's dense
factorization for the QP solve (Open Question §12.1 — choice of
optimization solver). For v0.3, the solver is a hand-rolled SQP loop
(forward-difference Jacobians, BFGS Hessian update, line search).
External solver crates (e.g. `osqp`, `argmin`) are considered in
§12.1; the v0.3 default is the in-house implementation to avoid
adding a heavy dependency.

### §5.3 — `input_shape` dispatcher

```reify
fn input_shape(profile: Profile, shaper: Shaper) -> Profile
```

Dispatches on `shaper`'s concrete type:
- `ZVShaper` / `ZVDShaper` / `EIShaper`: impulse convolution via FFT
  on the profile's polynomial representation (closed-form for
  piecewise polynomials).
- `CascadedShaper`: fold over `shapers`, applying each in sequence.
- `TOTSShaper`: SQP loop calling simulate_trajectory + inverse_dynamics
  per iteration (expensive; see ComputeNode wiring §6).

## §6 — Contract: Forward-pass simulator

### §6.1 — `simulate_trajectory`

```reify
fn simulate_trajectory(p: Profile, mech: Mechanism, modal: ModalResult)
    -> EndEffectorTrack
```

Internal algorithm (per §1's worked example):

```
fn simulate_trajectory(profile, mech, modal):
    // (1) Sample the profile at modal-aware dt.
    let dt = min(0.5 / modal.modes[-1].frequency, profile.duration / 1000)
    let traj = to_trajectory_samples(profile, dt)

    // (2) Nominal end-effector track from forward kinematics.
    let snapshots = traj.samples.map(s -> snapshot(mech, bindings_from(s.values)))
    let nominal = snapshots.map(snap -> end_effector_pose(snap, eff_locations))

    // (3) Forcing time history from inverse dynamics.
    let forces = inverse_dynamics(mech, traj)
    let forcing = forces_to_forcing_history(forces, mech, modal.part)

    // (4) Modal superposition response.
    let modal_resp = transient_response(modal, forcing, traj.duration, dt)
    let vibration = modal_resp.displacement_at(eff_locations)

    // (5) Sum linearly.
    return EndEffectorTrack(
        t_samples: traj.times,
        nominal_pose: nominal,
        vibration_offset: vibration,
        combined_pose: nominal + vibration,
    )
```

The `+` between nominal pose (rigid kinematics) and vibration offset
(small displacement) is the **small-deformation linear superposition**
assumption — valid as long as modal-superposition amplitudes are
small compared to nominal mechanism motion. For typical 3D-printer
ringing this is satisfied (vibration amplitudes ~10–100µm vs nominal
motion ~10–500mm).

### §6.2 — `EndEffectorTrack`

```reify
structure def EndEffectorTrack {
    param mechanism        : Mechanism
    param modal_result     : ModalResult
    param t_samples        : List<Time>
    param nominal_pose     : List<List<Pose3>>   // outer: time, inner: locations
    param vibration_offset : List<List<Vec3>>    // outer: time, inner: locations
    param combined_pose    : List<List<Pose3>>   // outer: time, inner: locations
}

// Lazy accessors:
fn end_effector_track(t: EndEffectorTrack, location: LocationId) -> List<Pose3>
fn deviation_from_nominal(t: EndEffectorTrack, location: LocationId) -> List<Length>
fn peak_deviation(t: EndEffectorTrack, location: LocationId) -> Length
```

## §7 — Contract: G-code import

### §7.1 — `reify-gcode` crate

A new crate `reify-gcode` houses the parser. Two dialects in v0.3:

- **MarlinDialect.** Subset of Marlin G-code: G0/G1 (linear move),
  G2/G3 (arc move, IJK form), G92 (set position), F (feedrate),
  M104/M109 (extruder temp, ignored for trajectory), M82/M83
  (extruder mode, ignored).
- **KlipperDialect.** Same core G-codes plus Klipper's
  `SET_VELOCITY_LIMIT` and `INPUT_SHAPER` directives (the latter is
  parsed but a warning emitted if it conflicts with this PRD's
  shaper-design intent).

Both dialects share a common AST (`GcodeCommand` enum) and converge
on the same `MotionProfile`-output path.

### §7.2 — `gcode_import` stdlib fn

```reify
trait GcodeDialect {
}

structure def MarlinDialect : GcodeDialect {
}

structure def KlipperDialect : GcodeDialect {
}

fn gcode_import(source: String, dialect: GcodeDialect) -> List<MotionProfile>
```

The output is a `List<MotionProfile>` because a real G-code file is a
sequence of moves potentially interleaved with non-motion commands
(temperature, fan, extruder); each contiguous motion segment becomes
one MotionProfile.

The mechanism the profiles bind to is inferred from a per-call
`mechanism: Mechanism` parameter or from a `with_mechanism(m)`
context (precise binding TBD at impl time — Open Question §12.4).

## §8 — Resolved design decisions

**(8.1) Reify .ri DSL primary + G-code as optional bolt-on.** The
parametric DSL is the canonical authoring surface; the PRD's CI-gate
acceptance requires only pure-`.ri` fixtures. G-code import is a
bolt-on for ingesting realistic slicer output during dogfood, and
lives in its own crate (`reify-gcode`) so the parser code doesn't
accrete into the main stdlib. The PRD is fully usable without G-code:
mechanism + modal + shaper design can be exercised entirely in
parametric `.ri` before any G-code is loaded. This sequencing matters
for design-loop iteration — users can prove out the dynamics +
shaper stack before invoking the slicer-output ingestion path.

**(8.2) ZV + ZVD + EI + TOTS.** Four shaper kinds. ZV/ZVD/EI cover
the classical impulse-shaping use cases (Klipper, Marlin's input-
shaping equivalents). TOTS extends to optimization-driven motion for
the printer-design use case of "fastest possible motion subject to
vibration and actuator constraints". TOTS substantially expands
scope — owns its own optimization-loop infrastructure.

**(8.3) Piecewise polynomial (cubic / quintic) primitive.** Generic
B-spline foundation, cubic default for waypoint-only specifications,
quintic for Hermite-style (with velocity + acceleration tangents).
Future formulations (e.g. NURBS for curved tool paths) extend the
SplineKind enum.

**(8.4) Mode-superposition forward-pass simulator.** Linear sum of
rigid-body kinematics (snapshot/sweep) + modal vibration (via
modal-analysis.transient_response). Valid in the small-deformation
regime that 3D-printer ringing inhabits. Full nonlinear flexible-
multibody is bookmarked under `modal-analysis.md`.

**(8.5) Joint-space waypoints, not Cartesian.** v0.3 ships joint-
space; Cartesian + IK is a follow-up (depends on the inverse-
kinematics PRD which is itself bookmarked under
`kinematic-constraints-completion.md`).

**(8.6) TOTS via in-house SQP loop.** Avoid pulling in a heavy
optimizer dependency for v0.3. Hand-rolled forward-difference SQP
with BFGS Hessian update + Armijo line search is well-understood and
small. faer-rs handles the dense linear algebra (QP subproblem,
factorization). If TOTS performance becomes a bottleneck, swap in
`osqp` or `argmin` as a follow-up.

**(8.7) ComputeNode wiring for simulate_trajectory + input_shape
TOTS path.** Both expensive enough to warrant caching. TOTS especially
benefits since the SQP loop calls simulate_trajectory and
inverse_dynamics repeatedly with varying control points but fixed
mechanism + modal model.

**(8.8) Two dialects in v0.3: Marlin + Klipper.** Most printer-build
dogfood involves one of these. Other dialects filed as future PRD
slot.

## §9 — Cross-PRD relationship (G4)

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `kinematic-constraints-completion.md` | consumes | `Snapshot`, `Mechanism`, `sweep` | kinematic-completion | this session |
| `rigid-body-dynamics.md` | consumes | `inverse_dynamics`, `MotionTrajectory`, `JointForce` | rigid-body-dynamics | this session |
| `modal-analysis.md` | consumes | `ModalResult`, `transient_response`, `ForcingTimeHistory` | modal-analysis | this session |
| `docs/prds/v0_3/structure-instance-runtime.md` (SIR / GR-001) | consumes | `Value::StructureInstance` + ctor lowering | SIR | queued (3540 in flight) |
| `docs/prds/v0_3/compute-node-contract.md` (GR-002) | consumes | `#[compute_node]` trampoline | GR-002 | landed |
| `docs/prds/v0_3/engine-integration-norm.md` (GR-017) | references | §3.4 ComputeNode-dispatch seam | GR-017 | landed |
| `compliant-joints-flexures.md` (this session) | independent | none direct; both consume the kinematic + dynamics stack | n/a | both authored this session |

This PRD is the **terminal consumer** of the four-PRD stack. No
downstream PRD depends on this in v0.3. Bookmarked follow-ups (online
TOTS, IK, additional G-code dialects) extend its scope but live in
future PRDs.

## §10 — Boundary test sketch (cross-crate; facing both ways)

### §10.1 — Producer-side

| Scenario | Preconditions | Postconditions |
|---|---|---|
| **ZV vibration suppression.** Single-mode 1-DOF oscillator (f=10Hz, ζ=0.05), apply ZV-shaped step. | ε, η wired; simulate_trajectory ι wired. | Residual vibration peak at f=10Hz reduced by ≥ 40 dB vs unshaped baseline. |
| **ZVD ±10% frequency robustness.** ZVD shaper designed for f=10Hz, apply to a system with actual f∈[9, 11]Hz. | ε, η, ι wired. | Residual vibration ≤ 5% of unshaped peak across the range. |
| **EI ±15% frequency robustness with vibration_tolerance=0.05.** EI shaper. | ε, η, ι wired. | Residual ≤ 5% across f∈[8.5, 11.5]Hz. |
| **TOTS converges + improves over baseline.** TOTS shaper, 1-DOF gantry + single mode, 500mm P2P motion. | ζ, ι wired. | SQP converges in ≤ 100 iters; resulting duration < baseline duration; vibration constraint satisfied within solver tol. |
| **TOTS infeasibility detection.** TOTS with constraints that have no feasible region (e.g. velocity_limit = 0). | ζ wired. | Solver emits `E_TrajectoryConstraintInfeasible` within a few iterations; no garbage output. |
| **G-code Marlin parser smoke.** 100-line Marlin G-code fixture. | κ, λ wired. | Produces a non-empty List<MotionProfile>; no parser errors; first segment's `simulate_trajectory` runs without diagnostics. |
| **G-code Klipper INPUT_SHAPER directive.** Klipper fixture with INPUT_SHAPER directive. | κ wired. | Parses; `W_GcodeDialectShaperConflict` warning emitted citing that the in-PRD shaper-design supersedes the file-declared shaper. |
| **ComputeNode cache hit on simulate_trajectory under TOTS iteration.** TOTS SQP loop. | μ wired. | Second call with same (profile, mech, modal) hits cache; cache invalidation on profile-control-point change. |

### §10.2 — Consumer-side

| Scenario | Preconditions | Postconditions |
|---|---|---|
| **GUI live-print preview consumes EndEffectorTrack.** GUI dashboard reads `simulate_trajectory` output for the current mechanism + modal + imported G-code; live-plots end-effector trajectory error. | ι wired; GR-016 channel. | GUI plot updates on mechanism / modal edits via ComputeNode invalidation. |
| **Printer-build dogfood: end-to-end print-envelope analysis.** Mechanism + gantry modal + imported test print G-code → simulate_trajectory → check peak deviation. | All phases wired. | Peak end-effector deviation reported; compared to a print-quality budget. Decision: motion profile is acceptable / requires TOTS reshaping. |

## §11 — Decomposition plan

Vertical-slice B+H. Phase 1 foundation (MotionProfile + Waypoint +
spline evaluator); Phase 2 impulse shapers (ZV/ZVD/EI); Phase 3
forward-pass simulator; Phase 4 TOTS; Phase 5 G-code import; Phase 6
dogfood + companion.

### Phase 1 — Foundation

- **α — `MotionProfile`, `Waypoint`, `BoundaryCondition`, `SplineKind`
  `structure def`s.**
  - Crates: `reify-compiler/stdlib/trajectory.ri` (NEW).
  - Observable signal: compile test on template shapes; ctor
    constraints fire (e.g. empty waypoints list rejected).
  - Prereqs: SIR-α task 3540.

- **β — `piecewise_polynomial` ctor + B-spline evaluator (cubic +
  quintic).**
  - Crates: `reify-stdlib/src/trajectory/spline.rs` (NEW), unit
    tests confirming evaluator accuracy on analytic functions.
  - Observable signal: under **clamped / not-a-knot** end conditions
    (exact endpoint derivatives supplied), the evaluator reproduces a
    polynomial of matching degree to within 1e-12 off-knot (cubic↔cubic,
    quintic↔quintic) — those end conditions uniquely pin the polynomial.
    The **natural** BC test asserts only at-knot interpolation to 1e-12
    plus endpoint second-derivative = 0 by construction: a natural cubic
    spline does NOT reproduce a general cubic off-knot (natural BC forces
    `M[0]=M[N]=0` ⟹ `p''(endpoints)=0` ⟹ the reproduced polynomial is
    degree ≤ 1). (Premise corrected per esc-3770-1 / task 3816; G6 —
    exactness is end-condition-dependent.)
  - Prereqs: α.

- **γ — `to_trajectory_samples` bridge to rigid-body-dynamics
  MotionTrajectory.**
  - Crates: `reify-stdlib/src/trajectory/sampling.rs` (NEW).
  - Observable signal: round-trip test: profile → samples → resampled
    profile reproduces evaluator output within mesh-density-driven
    tolerance.
  - Prereqs: β, rigid-body-dynamics α (MotionTrajectory structure).

### Phase 2 — Impulse shapers

- **δ — `Shaper` trait + `ZVShaper`, `ZVDShaper`, `EIShaper`
  structure_defs.**
  - Crates: `reify-compiler/stdlib/trajectory.ri` (extend).
  - Observable signal: compile test; ctor constraints
    (`target_frequency > 0`, `vibration_tolerance ∈ (0, 1]`).
  - Prereqs: α.

- **ε — Impulse-shaping convolution implementation (ZV/ZVD/EI/
  cascaded).**
  - Crates: `reify-stdlib/src/trajectory/impulse_shaper.rs` (NEW).
  - Observable signal: pure-Rust residual-vibration-ratio assertion
    (`V_shaped ≤ 0.01·V_unshaped` at the design frequency); construction-only
    `examples/trajectory/zv_shaped_ramp.ri`. NOTE (esc-3866-57): the literal
    "runs end-to-end; ≥ 40 dB reduced" signal is RE-HOMED to θ — it requires
    `input_shape` (ζ, downstream of ε) + `simulate_trajectory` (θ), so it
    cannot be verified at ε's DAG position.
  - Prereqs: δ, β, modal-analysis modes available (compile-time).

- **ζ — `input_shape(profile, shaper)` stdlib dispatcher + eval
  wiring (impulse-shaper arms).**
  - Crates: `reify-stdlib/src/trajectory.rs` (registrar),
    `reify-eval/src/trajectory_ops.rs` (NEW).
  - Observable signal: ZVD + EI robustness tests pass.
  - Prereqs: ε.

### Phase 3 — Forward-pass simulator

- **η — `EndEffectorTrack` + lazy accessor structure_defs.**
  - Crates: `reify-compiler/stdlib/trajectory.ri` (extend).
  - Observable signal: compile test.
  - Prereqs: α.

- **θ — `simulate_trajectory` implementation (sample → kinematics
  → inverse_dynamics → forcing → transient_response → sum).**
  - Crates: `reify-stdlib/src/trajectory/simulate.rs` (NEW).
  - Observable signal: unit test confirms zero vibration for
    static profile (constant pose); confirms expected vibration
    for a step input on a single-mode oscillator. END-TO-END
    (re-homed from ε per esc-3866-57): shape a ramp via `input_shape`
    (ζ) → `simulate_trajectory` (θ) → assert ≥ 40 dB residual-vibration
    reduction vs unshaped (`examples/trajectory/zv_shaped_ramp.ri`).
  - Prereqs: γ, η, rigid-body-dynamics η (inverse_dynamics),
    modal-analysis ι (transient_response).

### Phase 4 — TOTS shaper

- **ι — `TOTSShaper` + `JointLimit` structure_defs.**
  - Crates: `reify-compiler/stdlib/trajectory.ri` (extend).
  - Observable signal: compile test.
  - Prereqs: α, modal-analysis α.

- **κ — SQP loop implementation for TOTS.**
  - Crates: `reify-stdlib/src/trajectory/tots.rs` (NEW), faer-rs
    dense factorization (or pre-existing dep).
  - Observable signal: TOTS converges on the 1-DOF gantry +
    single-mode test fixture; resulting duration < baseline;
    vibration constraint satisfied.
  - Prereqs: θ, ι, rigid-body-dynamics η, modal-analysis ι.

- **λ — `input_shape` dispatcher extended with TOTS arm.**
  - Crates: `reify-stdlib/src/trajectory.rs` (extend).
  - Observable signal: `examples/trajectory/tots_optimal_ptp.ri` runs
    end-to-end.
  - Prereqs: κ, ζ.

### Phase 5 — G-code import (OPTIONAL bolt-on)

This phase is separable from the rest of the PRD; the core acceptance
gate (Phases 1–4 + 6 dogfood-pure-`.ri`) does not depend on Phase 5.
Decompose-mode may schedule Phase 5 in parallel or skip it for an
early-acceptance milestone.

- **μ — `reify-gcode` crate scaffold + Marlin parser.**
  - Crates: `crates/reify-gcode/` (NEW; Cargo manifest, lib, parser
    tests with Marlin fixtures).
  - Observable signal: parser unit tests pass for the Marlin
    fixture set; AST round-trip preserves semantics.
  - Prereqs: none (independent crate).

- **ν — Klipper dialect extension.**
  - Crates: `reify-gcode/src/dialects/klipper.rs` (NEW).
  - Observable signal: parser handles Klipper extras
    (SET_VELOCITY_LIMIT, INPUT_SHAPER) without errors; emits
    `W_GcodeDialectShaperConflict` on INPUT_SHAPER directive when
    consumed via gcode_import.
  - Prereqs: μ.

- **ξ — `GcodeDialect` trait + `MarlinDialect` / `KlipperDialect`
  structure_defs.**
  - Crates: `reify-compiler/stdlib/trajectory.ri` (extend).
  - Observable signal: compile test; runtime values produced.
  - Prereqs: α.

- **ο — `gcode_import(source, dialect) -> List<MotionProfile>`
  stdlib fn + eval dispatch.**
  - Crates: `reify-stdlib/src/trajectory/gcode_import.rs` (NEW),
    `reify-eval/src/trajectory_ops.rs` (extend); bridge between
    `reify-gcode` AST and `MotionProfile`.
  - Observable signal: `examples/trajectory/gcode_import_smoke.ri`
    runs; produces List<MotionProfile> for a 100-line Marlin
    fixture.
  - Prereqs: μ, ν, ξ, β.

### Phase 6 — ComputeNode + dogfood + companion

- **π — ComputeNode trampolines for `simulate_trajectory` (μ in §2.1)
  and TOTS-path `input_shape` (ν in §2.1).**
  - Crates: `reify-stdlib/src/trajectory/trampoline.rs` (NEW).
  - Observable signal: cache-hit tests; cancellation tests.
  - Prereqs: θ, κ, GR-002.

- **ρ — Printer-build print-envelope dogfood `.ri`.**
  - Files: `examples/trajectory/printer_print_envelope.ri` (NEW),
    bundled small G-code test fixture.
  - Observable signal: example runs; print-quality budget report
    printed.
  - Prereqs: λ, ο, π.

### §11.1 — Dependency view

```
α ───┐
     ├─→ β ──→ γ
     ├─→ δ ──→ ε ──→ ζ
     ├─→ η ──┐
     │       ├─→ θ ──┐
     │       │       │
     ├─→ ι ──┴───→ κ ─→ λ
     │
     ├─→ ξ ──┐
     │       │
     │       ├─→ ο
     │       │
     └─→ μ → ν

  γ, θ depend on rigid-body-dynamics η (inverse_dynamics)
  θ depends on modal-analysis ι (transient_response)

  All of α, ε onward depend on SIR-α 3540 transitively.

  π depends on θ, κ, GR-002.
  ρ depends on λ, ο, π.
```

17 in-batch tasks (α through ρ) + 4 cross-PRD edges (SIR-α 3540 +
rigid-body-dynamics η + modal-analysis ι + GR-002). Large PRD; the
TOTS phase alone is 3 tasks and the G-code crate is 4 tasks.

## §12 — Open questions (surfaced but not decided in this session)

1. **TOTS optimization solver dependency.** §5.2 picks in-house SQP
   with faer-rs linalg. Real-world TOTS perf may demand a heavier
   solver (`osqp`, `argmin`, Ipopt). **Suggested resolution:** ship
   in-house SQP in v0.3; profile against the printer dogfood; swap
   in if perf is unsatisfactory. Decide post-dogfood.

2. **B-spline basis: B-spline vs Bezier vs Catmull-Rom.** §4.1 picks
   B-spline (de Boor). Bezier is more direct for users (control
   points are interpolated at endpoints); Catmull-Rom is C^1-smooth
   through every waypoint. **Suggested resolution:** B-spline default
   for evaluator stability; add Bezier as a SplineKind variant if
   user feedback demands.

3. **Quintic spline boundary condition syntax.** §4.1 says cubic is
   waypoint-only; quintic uses `Waypoint.vels` + `Waypoint.accels`.
   This is implicit signalling. **Suggested resolution:** add an
   explicit `quintic_polynomial(...)` ctor that requires `vels` +
   `accels` and rejects bare-waypoint input. Cubic ctor accepts
   either.

4. **G-code import mechanism binding.** §7.2 punts on whether
   gcode_import takes a `mechanism` param or relies on context.
   **Suggested resolution:** `gcode_import(source, dialect, mechanism)`
   is the explicit form; a sugar `with_mechanism(m) { gcode_import(s, d) }`
   block is a future ergonomic improvement.

5. **Multi-mechanism MotionProfile?** A single profile binds to a
   single Mechanism (§4.1). Real printers have e.g. independent
   extruder + XY mechanisms running on a shared timeline.
   **Suggested resolution:** one profile per mechanism; compose
   multi-mechanism trajectories via a `MultiProfileBundle` that
   ties profiles to a shared time axis. Filed as v0.4 follow-up if
   demand emerges.

6. **Modal frequency drift over a long trajectory.** §6.1 uses a
   single ModalResult for the whole trajectory. For mechanisms with
   significantly position-dependent inertia (long crane-arm
   examples), modes drift. **Suggested resolution:** v0.3 assumes
   modes are pose-invariant (a common, defensible approximation for
   stiff printer frames). Pose-parametric modal models are a v0.5+
   research-grade extension.

## §13 — Gap-register companion edits

Adds "trajectory shaping" as a new mechanism cluster in gap-register.
No existing GR row claims input-shaping or TOTS analysis. Companion
task adds the row.

End of PRD.
