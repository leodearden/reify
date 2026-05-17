# Rigid-Body Dynamics — Inverse Dynamics Foundation

Status: contract authored 2026-05-17 in interactive `/prd` session. Sibling
to `kinematic-constraints-completion.md` and foundation for
`trajectory-input-shaping.md` + `compliant-joints-flexures.md`. Pending Leo
approval before queueing tasks.

## §0 — Purpose and supersession

Reify's kinematics stack (v0.1, v0.2, v0.3 completion contract) shipped
position-level mechanism modelling: forward kinematics, closed-chain
loop-closure, joint zoo, interference under motion. None of it sees force.

This PRD adds **inverse dynamics**: given a mechanism + a prescribed motion
trajectory (joint positions, velocities, accelerations as functions of time),
compute the actuator torques and forces required to produce that motion. The
output drives motor sizing, holding-torque budgets, trajectory feasibility
checks, and is the foundation that `trajectory-input-shaping.md` and
`compliant-joints-flexures.md` consume.

**Forward dynamics is explicitly out of scope** for this PRD; a deferred
follow-up task is filed for it (§13). The split mirrors how real motion-
control engineers work: inverse dynamics is the design-time primitive (sizing,
budgets), forward dynamics is the simulation-time primitive (drop test, stall
sim).

No supersession; this is greenfield for the v0.3 line.

## §1 — Goal and user-observable surface

A Reify user can compute the actuator torques required to drive a mechanism
along a prescribed motion profile and read the result back as a typed value.
Concretely:

```reify
// examples/dynamics/toolhead_motor_sizing.ri
let m = printer_toolhead_mechanism();
let traj = ramp_profile(x_axis, from: 0mm, to: 500mm, max_accel: 5000mm/s^2);
let forces = inverse_dynamics(m, traj);

// forces: List<JointForce> indexed parallel to mechanism's driving joints
let peak_x_torque = max(forces.map(f -> f.scalar_force_or_torque));
report("peak X-axis force = " ++ show(peak_x_torque));
```

**CI-gate signals:**

- `cargo test -p reify-eval --test rigid_body_dynamics_e2e` runs three
  fixtures and they all pass:
  - `examples/dynamics/pendulum_idyn.ri` — single-revolute-joint pendulum.
    `inverse_dynamics(m, motionless_traj)` returns torque exactly
    `mg * L * sin(θ)` to within 1µN·m for a 1kg, 100mm pendulum at θ=30°.
    Analytic ground truth.
  - `examples/dynamics/double_pendulum_idyn.ri` — two-revolute serial
    chain. Torques cross-validated against a published symbolic solution
    (Spong & Vidyasagar's "Robot Modeling and Control" Example 7.2).
  - `examples/dynamics/closed_4bar_idyn.ri` — four-bar planar linkage
    (closed chain). Constraint forces λ on the loop residual are
    finite and non-NaN; output torques sum to (input + grav.) work
    via virtual-work check.

- `examples/dynamics/toolhead_motor_sizing.ri` — printer-build dogfood
  fixture. Peak Y-axis torque over the planned print envelope is reported;
  used as the sizing-budget gate downstream of motor selection.

**Diagnostic signals:**

- `E_DynamicsMassPropsMissing` — body has no Solid and no explicit mass
  override; mass tensor cannot be computed.
- `W_DynamicsSingularInertia` — articulated-body inertia matrix near
  singular at this snapshot (e.g. zero-length link or degenerate joint
  axis). Output may be unreliable.
- `E_DynamicsClosedChainOverdetermined` — Lagrange multiplier system has
  more constraint equations than free DOFs (related to but distinct from
  the kinematic-completion PRD's `E_KinematicOverconstrained`).

## §2 — Scope

### §2.1 — Mechanism table

| # | Mechanism | State today | Owner |
|---|---|---|---|
| α | `MassProperties` value type (mass / com / inertia tensor / origin frame) | NEW | this PRD |
| β | `body_mass_props(body, density?)` stdlib fn — kernel-computed from `body.solid`; default density from body's `Material`, override via param | NEW (consumes KGQ Phase 4) | this PRD |
| γ | `JointForce` value type (per-joint scalar torque/force OR multi-DOF vector for multi-DOF joints) | NEW | this PRD |
| δ | `MotionTrajectory` value type — `(θ(t), θ̇(t), θ̈(t))` per driving joint over a time range | NEW | this PRD |
| ε | Featherstone articulated-body inverse-dynamics core in Rust | NEW | this PRD |
| ζ | Closed-chain Lagrange-multiplier extension reusing loop_closure_solver | NEW | this PRD |
| η | `inverse_dynamics(mechanism, trajectory) -> List<JointForce>` stdlib fn (eval-side dispatch) | NEW | this PRD |
| θ | `inverse_dynamics_at_snapshot(mechanism, snapshot, q_dot, q_ddot) -> List<JointForce>` — single-time-step variant | NEW | this PRD |
| ι | ComputeNode trampoline registration (per `compute-node-contract.md`) — inverse_dynamics is expensive enough to warrant caching + cancellation | NEW (consumes GR-002) | this PRD |

### §2.2 — Bookmark task

- **Forward dynamics PRD slot.** Submit a deferred bookmark task referencing
  `docs/prds/v0_3/forward-dynamics.md` (unauthored slot). Goal: integrate
  ODE for (θ, θ̇) given (τ, gravity, contact). Triggers: real demand from a
  printer simulation use case (drop-test, stall-sim, control-loop tuning).
  Deferred per [[preferences_bookmark_task_pattern]].

### §2.3 — Out of scope

- **Forward dynamics.** Bookmarked.
- **Contact / collision dynamics.** No contact forces, no friction at
  contact, no impact response.
- **Friction in joints.** Joint Coulomb + viscous friction is a follow-up;
  inverse dynamics here is frictionless.
- **Actuator dynamics.** Motor torque-vs-speed curves, back-EMF, current
  limits, missed-step prediction — separate domain.
- **Trajectory generation.** This PRD consumes a `MotionTrajectory`;
  generating one is `trajectory-input-shaping.md` territory.
- **Modal / vibration analysis.** Sibling PRD `modal-analysis.md`.
- **Flexible-body dynamics.** Sibling PRD `compliant-joints-flexures.md`
  for spring-damper joint extensions; flexible-body FEA coupling is even
  farther out.

## §3 — Pre-conditions for activating

| Pre-condition | Owner | Status today (2026-05-17) | Gate phase |
|---|---|---|---|
| `JointValue` enum + chain widening landed | `kinematic-constraints-completion.md` task γ | this session | hard prereq for ε, ζ |
| `Snapshot.free_values: List<JointValue>` shape stable | `kinematic-constraints-completion.md` task α-pre | this session | hard prereq for ε |
| KGQ Phase 4 — `mass`, `center_of_mass(Solid, density)`, `inertia_tensor(Solid, density)` kernel queries | `docs/prds/v0_3/kernel-geometry-queries.md` Phase 4 | pending decomp | hard prereq for β |
| SIR-α `StructureInstance` for `MassProperties`, `JointForce`, `MotionTrajectory` ctor lowering | `docs/prds/v0_3/structure-instance-runtime.md` task 3540 | in-flight | hard prereq for α, γ, δ |
| ComputeNode contract (GR-002) trampoline registration mechanism | `docs/prds/v0_3/compute-node-contract.md` | landed | substrate for ι |
| Loop-closure solver multi-DOF widening | `kinematic-constraints-completion.md` task γ | this session | hard prereq for ζ |

## §4 — Contract: `MassProperties`, `JointForce`, `MotionTrajectory`

### §4.1 — `MassProperties`

```reify
structure def MassProperties {
    param mass     : Mass            // kg
    param com      : Point3          // centre of mass in body local frame
    param inertia  : Tensor3x3       // inertia tensor about COM, body frame
    param origin   : Frame3          // body frame the above are expressed in
}
```

`Tensor3x3` is the standard symmetric 3×3 inertia tensor; six independent
components stored as a `[[Real;3];3]` Matrix at value level.

**Invariants** (statically declared in the structure_def constraint block):
- `mass >= 0` — mass is non-negative; zero allowed only for "massless"
  intermediate links (warning emitted on use in dynamics if zero).
- `inertia` is symmetric positive-semidefinite (PSD). Validation runs at
  `MassProperties(...)` ctor time: compute eigenvalues, assert all ≥ 0.
  Violation emits `E_DynamicsInertiaNotPSD`.

### §4.2 — `JointForce`

Parallels `JointValue` enum semantically — per-joint shape depends on
joint kind:

```reify
structure def JointForce {
    param joint_id : BodyId          // the joint this force acts on
    param value    : JointForceValue // shape depends on joint DOF
}

// JointForceValue is a tagged union (mirrors JointValue enum):
//   ScalarForce(Real)      — for prismatic (force in joint axis, N)
//   ScalarTorque(Real)     — for revolute (torque about joint axis, N·m)
//   ScalarTorque(Real)     — for coupling (torque on parent joint)
//   CylForce([Real;2])     — for cylindrical: [axial force N, twist torque N·m]
//   PlanarForce([Real;3])  — for planar: [Fx, Fy, Mz] N/N/N·m
//   SphereForce([Real;3])  — for spherical: [Mx, My, Mz] N·m
//   ZeroForce              — for fixed joints (no actuated force)
```

The variant tag is determined statically from the joint's kind at the
mechanism build time; output is shape-validated by the dispatcher.

### §4.3 — `MotionTrajectory`

A motion trajectory binds a driving joint to a time-parameterised function
returning `(θ(t), θ̇(t), θ̈(t))`. For inverse dynamics the input is a
**sample sequence** along a time grid (no symbolic differentiation
required):

```reify
structure def TrajectorySample {
    param t       : Time             // s
    param values  : List<JointValue> // q(t)  — per driving joint
    param vels    : List<JointValue> // q̇(t) — derivative; matching shape
    param accels  : List<JointValue> // q̈(t) — second derivative; matching shape
}

structure def MotionTrajectory {
    param mechanism : Mechanism
    param samples   : List<TrajectorySample>
}
```

For multi-DOF joints, `vels` and `accels` are shape-matched per-component
derivatives in the joint's local frame (planar: dx/dt, dy/dt, dθ/dt etc.).

A helper stdlib fn `ramp_profile(joint, from, to, max_accel)` produces
trapezoidal-velocity samples (no jerk limiting; that's
`trajectory-input-shaping.md` territory).

## §5 — Contract: Featherstone articulated-body inverse dynamics

### §5.1 — Spatial-vector core

Reify uses Featherstone's 6D spatial-vector representation
(`[ω; v]` for velocities, `[τ; f]` for forces, `[[E;0];[r̃E;E]]` for
spatial transforms). The Rust module `crates/reify-stdlib/src/dynamics/`
ships:

```rust
// reify-stdlib/src/dynamics/spatial.rs (NEW)
pub struct SpatialVector6([f64; 6]);   // [ω; v]  or  [τ; f]
pub struct SpatialTransform6([f64; 36]); // 6×6 matrix
pub struct SpatialInertia6([f64; 36]);   // 6×6 symmetric PSD

impl SpatialTransform6 {
    pub fn from_frame3(f: &Frame3) -> Self { /* ... */ }
    pub fn compose(&self, other: &Self) -> Self { /* ... */ }
    pub fn inverse(&self) -> Self { /* ... */ }
}
```

The motion-subspace matrix `S_i` for each joint kind (the columns of the
joint's allowable spatial velocity) is supplied by extending
`crates/reify-stdlib/src/joints.rs` with a `motion_subspace_columns(joint)`
function returning a 6×k matrix where k = DOF count.

### §5.2 — Recursive Newton-Euler (RNEA) inverse dynamics

```
fn inverse_dynamics_open_chain(mechanism, snapshot, q_dot, q_ddot, gravity):
    // Forward pass (outward from base; computes spatial vel + accel)
    for joint i in spanning-tree order (parent before child):
        v_i = X_p->i * v_p + S_i * q̇_i
        a_i = X_p->i * a_p + S_i * q̈_i + crossM(v_i, S_i * q̇_i)
        // crossM is the spatial-velocity cross product

    // Backward pass (inward to base; computes spatial forces)
    for joint i in reverse-tree order:
        f_i = I_i * a_i + crossF(v_i, I_i * v_i)
              + transmitted forces from children
        τ_i = S_i^T * f_i

    return [τ_1, ..., τ_n] reshaped into JointForce list
```

The implementation is a straight Featherstone "RNEA" (Recursive
Newton-Euler Algorithm). Standard references: Featherstone (2008),
RBDL, Klampt.

### §5.3 — Closed-chain extension (Lagrange multipliers)

For mechanisms with `loop_closures`, the spanning tree's RNEA yields a
"reduced" system on independent coordinates. The closed-chain residuals
add constraint forces λ ∈ ℝ^m where m is the total loop-residual DOF
(sum over loops of effective residual dim, §4.3 of completion contract):

```
RNEA produces base equation:   M(θ)·q̈ + C(θ,θ̇) + G(θ) = τ_open
Closed-chain adds:             A(θ)^T · λ = τ_closed
                              A(θ) · q̈   = -ȦA·θ̇    (acceleration-level loop constraint)

Solve coupled system for (q̈_dependent, λ); τ = τ_open + τ_closed.
```

A(θ) is the loop constraint Jacobian — the chain-Jacobian we already
compute in `loop_closure_solver.rs::chain_jacobian`. The Lagrange system
is solved via LDLᵀ on the augmented matrix (existing solver
infrastructure handles the dense factorization; mechanism sizes are
small enough that sparse representation is not needed in v0.3).

### §5.4 — Mass-property assembly

For each body, `MassProperties` is built from one of three sources:

1. **Explicit override** at body construction: `body(solid, at: world,
   mass_props: explicit_mp)`. Highest priority; no kernel query.
2. **Body's Material** if present: `body_mass_props(body)` calls into
   KGQ Phase 4's `mass(solid, density)` with `density =
   body.material.density`. Default path.
3. **Default density 1000kg/m³** (water) with `W_DynamicsDefaultDensity`
   warning if no explicit override and no Material.

The validation that `inertia` is PSD runs once per `MassProperties`
constructor invocation (eigenvalue check; computed via
the existing faer-rs dependency from the buckling-eigensolver PRD or its
predecessor `nalgebra` if not yet pulled in).

### §5.5 — Snapshot-vs-trajectory entry points

Two stdlib entry points:

- `inverse_dynamics_at_snapshot(mechanism, snapshot, q̇, q̈) → List<JointForce>` —
  single time-step variant. Caller supplies velocities + accelerations
  as List<JointValue>. Useful for instantaneous analysis at a known pose.
- `inverse_dynamics(mechanism, trajectory) → List<List<JointForce>>` —
  trajectory variant. Iterates per `TrajectorySample`, returns the
  per-sample force list. Outer list parallel to `trajectory.samples`.

Both share the RNEA core; the trajectory variant is essentially a loop
calling the snapshot variant with cached `MassProperties` (mass props are
trajectory-invariant for rigid bodies, modulo `body.solid` editing).

## §6 — Contract: ComputeNode wiring

Inverse-dynamics is a non-trivial computation (O(n) per time step, n
joints; trajectories typically have 100s–1000s of samples; design-loop
iteration repeats similar mechanism configurations). Per `compute-node-
contract.md` §4 and `engine-integration-norm.md` §3.4, register a
ComputeNode trampoline:

```rust
// reify-stdlib/src/dynamics/trampoline.rs
#[compute_node]
pub fn inverse_dynamics_node(
    mech: ComputeNodeInput<MechanismHandle>,
    traj: ComputeNodeInput<MotionTrajectoryHandle>,
    state: &mut OpaqueState<InverseDynamicsCache>,
    cancel: &CancellationHandle,
) -> Result<List<List<JointForce>>, DiagnosticCode>;
```

Caching: results keyed on (mechanism content hash, trajectory content
hash, gravity vector hash). OpaqueState carries the cached
`MassProperties` per body, refreshed only when mechanism's body-solid
hashes change. Cancellation honoured on per-sample loop iteration
boundaries.

## §7 — Resolved design decisions

**(7.1) Inverse dynamics only; forward dynamics bookmarked.** Cleaner
scope, single PRD, avoids ODE-solver dependency selection and
time-stepping stability discussion. Forward dynamics needs a separate PRD
design session including ODE-solver choice (Runge-Kutta vs implicit-Euler
for stiff systems, etc.).

**(7.2) Featherstone articulated-body algorithm.** Selected over
classical Newton-Euler and Lagrangian because: (a) spatial-vector
representation reuses the `JointValue` shape + SE(3) twist already in
loop_closure; (b) closed-chain extension via Lagrange multipliers
slots cleanly into the existing loop_closure solver; (c) industry-
standard for serial chains and trees (RBDL, Klampt, robotics curricula).

**(7.3) Kernel-computed mass properties as default; explicit
override accepted.** Selected over kernel-only or override-only:
(a) most bodies have a Solid + Material, so kernel computation is the
sensible default; (b) overrides are essential for components whose
geometry is approximate (motors, electronics, purchased parts modelled
as simple bounding boxes); (c) the dispatch chain (explicit > Material
density > default-water) is a clean priority ladder.

**(7.4) Closed-chain Lagrange-multiplier extension.** Reuses the
existing `loop_closure_solver.rs::chain_jacobian` infrastructure. No
parallel solver. The Lagrange multipliers add to the RNEA output
torques as transmission forces through constraint joints.

**(7.5) Sample-based MotionTrajectory.** No symbolic differentiation;
caller supplies (q, q̇, q̈) samples explicitly. Avoids the AST-walk
complexity that symbolic differentiation requires. Helpers like
`ramp_profile`, `s_curve_profile` (filed in
`trajectory-input-shaping.md`) materialize samples from parametric
profiles.

**(7.6) `JointForce` variant shape parallels `JointValue`.** Same
discriminant set, same per-kind shape. Keeps dynamics output
shape-symmetric with kinematics input. Future-proof for IK + dynamics.

**(7.7) ComputeNode wiring.** Inverse-dynamics-over-trajectory is
expensive enough that the cache+cancel benefit pays for the trampoline
boilerplate. Per-snapshot variant (`inverse_dynamics_at_snapshot`) does
NOT wrap in a ComputeNode — too fine-grained, would thrash the cache.

## §8 — Cross-PRD relationship (G4)

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `kinematic-constraints-completion.md` | consumes | `JointValue` enum, `Snapshot` shape, `loop_closure_solver::chain_jacobian` | kinematic-completion | both authored this session |
| `docs/prds/v0_3/structure-instance-runtime.md` (SIR / GR-001) | consumes | `Value::StructureInstance` + ctor lowering for new structure_defs | SIR | queued (task 3540 in flight) |
| `docs/prds/v0_3/kernel-geometry-queries.md` (KGQ) | consumes | KGQ Phase 4 `mass`, `center_of_mass`, `inertia_tensor` kernel queries | KGQ | queued (Phase 4 decomp pending) |
| `docs/prds/v0_3/compute-node-contract.md` (GR-002) | consumes | `#[compute_node]` trampoline registration mechanism | GR-002 | landed |
| `docs/prds/v0_3/engine-integration-norm.md` (GR-017) | references | §3.4 ComputeNode-dispatch seam | GR-017 | landed |
| `docs/prds/v0_3/trajectory-input-shaping.md` (this session) | produces | `inverse_dynamics(...)` consumed by input-shaping forward-pass | this PRD | both authored this session |
| `docs/prds/v0_3/compliant-joints-flexures.md` (this session) | produces | Joint surface API extension hook for spring-damper terms (additive to dynamics force balance) | this PRD | both authored this session |
| `docs/prds/v0_3/modal-analysis.md` (this session) | independent | none direct; both consume FEA stack at different layers | n/a | both authored this session |

No reciprocal-ownership ambiguity. This PRD owns inverse dynamics; KGQ
owns kernel-level mass-property queries; SIR owns ctor lowering.

## §9 — Boundary test sketch (cross-crate; facing both ways)

### §9.1 — Producer-side

| Scenario | Preconditions | Postconditions |
|---|---|---|
| **Pendulum analytic ground truth.** Single revolute joint, 1kg point-mass at L=100mm, θ=30°, motionless. | β, ε wired; KGQ mass query landed. | `inverse_dynamics_at_snapshot` returns τ = m·g·L·sin(θ) = 0.4905 N·m within 1µN·m tolerance. |
| **Double pendulum cross-validation.** Two-link planar serial chain. | β, ε wired. | Per-joint torques match Spong & Vidyasagar Example 7.2 within 1e-6 relative error at a sample of 10 (θ₁, θ₂) pairs. |
| **Four-bar virtual-work check.** Planar 4-bar at θ_input = 45°, ω = 2π rad/s, α = 0. | ζ wired (closed-chain extension); kinematic-completion γ landed. | `Σ τ_i · θ̇_i = ΔKE + ΔPE` (within 1µJ); Lagrange multipliers λ are finite and non-NaN. |
| **MassProperties PSD validation.** Construct MassProperties with non-PSD inertia. | α wired with eigenvalue check. | Compiler / eval emits `E_DynamicsInertiaNotPSD`; ctor returns Undef. |
| **Default-density warning.** Mechanism with bodies having no Material. | β wired with priority-ladder. | `W_DynamicsDefaultDensity` diagnostic for each such body; computation still proceeds. |
| **ComputeNode cache hit.** Run `inverse_dynamics(m, traj)` twice with identical inputs. | ι wired. | Second call is a cache hit; observable via `ComputeNodeStats::hit_count`. |
| **ComputeNode cancellation.** Long trajectory; cancel mid-flight. | ι wired with per-sample boundaries. | Computation aborts within 1 sample interval; partial results not returned. |

### §9.2 — Consumer-side

| Scenario | Preconditions | Postconditions |
|---|---|---|
| **Trajectory PRD consumes torques.** trajectory-input-shaping's input-shaping algorithm calls `inverse_dynamics` to evaluate peak torque vs trajectory. | This PRD's η wired. | Trajectory PRD's tests compile + pass against the stable API surface. |
| **Flexure PRD adds spring-force term.** compliant-joints PRD's spring-damper joint contributes -k·θ to the joint's force budget. | This PRD's RNEA backward-pass exposes a per-joint additive force hook. | Flexure PRD's spring-damper test fixture (torsional spring resisting twist) produces expected ±k·θ contribution. |
| **GUI motor-budget panel.** GUI dashboard reads `inverse_dynamics` peak across the design-time motion envelope; emits a "motor sizing" report. | η wired; GR-016 channel. | GUI panel shows peak torque per driving joint; live-updates on mechanism edit (via ComputeNode invalidation). |

## §10 — Decomposition plan

Vertical-slice B+H. Phase 1 supplies foundation (MassProperties +
spatial-vector primitives); Phase 2 ships open-chain RNEA; Phase 3 adds
closed-chain extension; Phase 4 wires ComputeNode; Phase 5 is dogfood +
companion edits.

### Phase 1 — Foundation

- **α — `MassProperties` + `JointForce` + `MotionTrajectory` `structure
  def`s in stdlib.**
  - Crates: `reify-compiler/stdlib/dynamics.ri` (NEW), constraint
    validation (PSD inertia check).
  - Observable signal: compile test asserts the three structure_defs
    resolve to expected templates; ctor of `MassProperties(mass: ..., inertia:
    non_psd_matrix, ...)` emits `E_DynamicsInertiaNotPSD`.
  - Prereqs: SIR-α task 3540.

- **β — `body_mass_props` stdlib fn + kernel-query wiring.**
  - Crates: `reify-stdlib/src/dynamics/mass_props.rs` (NEW),
    `reify-eval/src/dynamics_ops.rs` (NEW), KGQ Phase 4 kernel queries.
  - Observable signal: `body_mass_props(box_body)` returns expected
    mass + COM + inertia for a uniform-density box (analytic ground
    truth I = m/12 · diag(b²+c², a²+c², a²+b²)).
  - Prereqs: α, KGQ Phase 4 mass/inertia queries.

- **γ — Spatial-vector core (`SpatialVector6`, `SpatialTransform6`,
  `SpatialInertia6`).**
  - Crates: `reify-stdlib/src/dynamics/spatial.rs` (NEW), unit tests.
  - Observable signal: unit test confirms `SpatialTransform6::from_frame3(f).compose(SpatialTransform6::from_frame3(f).inverse())` is identity within 1e-12; round-trips for 50 random Frame3 samples.
  - Prereqs: none (uses existing `reify-geometry::Frame3`).

### Phase 2 — Open-chain RNEA

- **δ — `motion_subspace_columns(joint)` per joint kind.**
  - Crates: `reify-stdlib/src/joints.rs` (extend per-kind), tests.
  - Observable signal: unit test confirms motion-subspace dimensions
    match DOF counts (prismatic:1, revolute:1, cylindrical:2, planar:3,
    spherical:3, fixed:0); for prismatic, the column equals `[0; axis]`.
  - Prereqs: γ.

- **ε — Featherstone RNEA inverse-dynamics core (open chains).**
  - Crates: `reify-stdlib/src/dynamics/rnea.rs` (NEW).
  - Observable signal: `examples/dynamics/pendulum_idyn.ri` + the
    double-pendulum cross-validation test pass.
  - Prereqs: β, γ, δ.

### Phase 3 — Closed-chain extension

- **ζ — Lagrange-multiplier closed-chain dynamics.**
  - Crates: `reify-stdlib/src/dynamics/closed_chain.rs` (NEW). Reuses
    `loop_closure_solver::chain_jacobian` for the constraint
    Jacobian A(θ).
  - Observable signal: `examples/dynamics/closed_4bar_idyn.ri` runs;
    virtual-work check passes within 1µJ.
  - Prereqs: ε, kinematic-completion γ (multi-DOF widening).

### Phase 4 — Eval-side dispatch + ComputeNode

- **η — `inverse_dynamics(...)` + `inverse_dynamics_at_snapshot(...)`
  stdlib fns + eval dispatch.**
  - Crates: `reify-stdlib/src/lib.rs` (registrar), `reify-eval/src/dynamics_ops.rs`.
  - Observable signal: `examples/dynamics/pendulum_idyn.ri` runs
    end-to-end via `reify eval`; prints expected torque value.
  - Prereqs: ε, ζ, α, δ.

- **ι — ComputeNode trampoline registration for
  `inverse_dynamics(mechanism, trajectory)`.**
  - Crates: `reify-stdlib/src/dynamics/trampoline.rs` (NEW), engine
    integration.
  - Observable signal: cache-hit test passes; cancellation test
    passes; integration with `engine-integration-norm.md` §3.4.
  - Prereqs: η, GR-002 contract (already landed).

### Phase 5 — Dogfood + companion

- **κ — Printer-toolhead motor-sizing dogfood `.ri`.**
  - Files: `examples/dynamics/toolhead_motor_sizing.ri` (NEW).
  - Observable signal: example runs, prints peak-torque
    report for the printer-build mechanism.
  - Prereqs: η, ι.

- **λ — Forward-dynamics bookmark task.**
  - Submit a deferred bookmark task per [[preferences_bookmark_task_pattern]]:
    title "[bookmark] forward-dynamics PRD slot", planning_mode=True,
    deferred status. References `docs/prds/v0_3/forward-dynamics.md`
    (unauthored slot).
  - Prereqs: none.

### §10.1 — Dependency view

```
α (structure defs) ──┐
                     ├──→ β (mass props)
                     │
γ (spatial-vec) ─────┼──→ δ (motion-subspace)
                     │       │
                     │       ▼
                     └────→ ε (open-chain RNEA) ──→ η (stdlib fns)
                                  │                    │
ζ (closed-chain) ─────────────────┘                    │
   │                                                    │
   └─ depends on kinematic-completion γ (multi-DOF)    │
                                                        │
                                                        ▼
                                                       ι (ComputeNode)
                                                        │
                                                        ▼
                                                       κ (dogfood)
                                                        │
                                                       λ (forward-dyn bookmark)
```

11 tasks total in-batch plus 1 bookmark + 2 cross-PRD dep edges
(SIR-α task 3540 + KGQ Phase 4 task).

## §11 — Out of scope for this PRD

(See §2.3. Also.)

- **Joint friction.** Coulomb + viscous friction in joints. Adds
  per-joint friction-force term to RNEA backward-pass; small extension
  but design-level decision (Coulomb regularization choice). Filed as
  future PRD.
- **Body-fixed external forces.** Aerodynamic / magnetic / wind loads.
  Filed as future PRD.
- **Trajectory generation.** `trajectory-input-shaping.md` territory.
- **Modal / vibration analysis.** `modal-analysis.md` sibling.
- **Compliant joints.** `compliant-joints-flexures.md` sibling.

## §12 — Open questions (surfaced but not decided in this session)

1. **Gravity vector source.** Two reasonable defaults: (a) constant
   `[0, 0, -9.81 m/s²]` everywhere; (b) per-snapshot override
   via mechanism's `gravity()` builder method. **Suggested
   resolution:** default constant, with `mechanism().gravity(g_vec)`
   override hook. Decide at task ε implementation; reasonable
   default suffices for v0.3.

2. **Trajectory sample count vs adaptive timestepping.** Sample-based
   trajectory means caller controls density. For long trajectories
   with non-uniform dynamics, adaptive resampling (Richardson
   extrapolation on torque) would help — but adds complexity.
   **Suggested resolution:** uniform sample density in v0.3; document
   the limitation. Adaptive in a follow-up.

3. **Inertia tensor frame convention.** §4.1 says "body frame about
   COM". Many references express inertia about the joint frame
   instead. Translation is the parallel-axis theorem. **Suggested
   resolution:** body-frame-about-COM is the canonical representation;
   parallel-axis translation happens internally when assembling the
   spatial inertia in the RNEA forward pass. Decide at ε impl.

4. **Cylindrical joint motion-subspace column ordering.** Two
   conventions: `[translation, rotation]` or `[rotation, translation]`.
   §4.2 picks former (matches `JointValue::Cyl`). Confirm at γ impl.

5. **Faer vs nalgebra for the Lagrange system solve.** Buckling
   eigensolver PRD pulls in faer-rs. Sharing the dependency is
   cleaner. **Suggested resolution:** faer-rs if available at ζ
   impl time; fall back to nalgebra otherwise.

## §13 — Gap-register companion edits

This PRD lands "rigid-body dynamics" as a new mechanism cluster — no
existing gap-register row claims it. Companion task adds a row under
"Resolved by docs/prds/v0_3/rigid-body-dynamics.md" for future audit
sweeps.

End of PRD.
