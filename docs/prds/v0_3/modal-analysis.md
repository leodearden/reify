# Modal Analysis — Free Vibration + Transient Response

Status: contract authored 2026-05-17 in interactive `/prd` session. Sibling
to `kinematic-constraints-completion.md`, `rigid-body-dynamics.md`,
`trajectory-input-shaping.md`, `compliant-joints-flexures.md`. Pending Leo
approval before queueing tasks.

## §0 — Purpose and supersession

Reify's FEA stack (`solver-elastic` v0.3, shells v0.4, buckling v0.5) ships
static and stability analysis but no dynamics. For ultra-performant 3D
printer design, **ringing and resonance set the ceiling on commanded
acceleration and jerk** — far more than static stiffness or strength does.
This PRD adds free-vibration modal analysis (per-Part eigenmodes) and
time-domain transient response via mode superposition. These are the
foundation that `trajectory-input-shaping.md` consumes to compute
shaped-motion profiles that don't excite resonance.

Scope intentionally stays at the **per-Part** layer (single FEA mesh,
single set of supports). Full-mechanism flexible-multibody modal analysis
(Craig-Bampton component-mode synthesis assembling per-Part modes across
joint interfaces) is bookmarked as a separate future PRD; the v0.5+
printer-design use cases dominantly need per-Part modes for the frame and
the toolhead structure.

Damping scope: undamped natural modes plus optional **Rayleigh
proportional damping** (`C = αM + βK`). Complex-eigenvalue
(non-proportional / localized) damping is bookmarked as a separate
future PRD; required only for systems with localized dampers (rubber
mounts, fluid dampers).

No supersession; greenfield for the v0.3 line.

## §1 — Goal and user-observable surface

A Reify user can compute the natural frequencies and mode shapes of any
Part with boundary conditions, and then compute the time-domain
displacement of that Part under an arbitrary forcing time history via
mode-superposition transient response. Concretely:

```reify
// examples/modal/printer_gantry_modes.ri
let gantry = printer_gantry_part();
let bcs = [
    FixedSupport(at: gantry_mount_left),
    FixedSupport(at: gantry_mount_right),
];

let modes = modal_analysis(gantry, ModalOptions(
    n_modes: 10,
    boundary_conditions: bcs,
    damping: RayleighDamping(alpha: 0.0, beta: 1e-4),
));

// modes : ModalResult — List<Mode> ordered by frequency
let f1 = modes.modes[0].frequency;
report("first mode = " ++ show(f1));   // expected ~120 Hz
```

```reify
// examples/modal/transient_step_response.ri
let forcing = StepForce(
    at: gantry_toolhead_mount,
    direction: X_HAT,
    magnitude: 10N,
    start_time: 0s,
);
let response = transient_response(modes, forcing, t_range: 0s..0.1s, dt: 0.1ms);

// response : DisplacementTimeHistory — per-node displacement(t)
let tip = response.displacement_at(gantry_toolhead_mount, X_HAT);
plot(tip);   // exponential decay envelope from Rayleigh damping
```

**CI-gate signals:**

- `cargo test -p reify-eval --test modal_analysis_e2e` runs four
  fixtures and they all pass:
  - `examples/modal/cantilever_beam_modes.ri` — uniform steel cantilever
    L=200mm, b=10mm, h=2mm. First-mode frequency matches the analytic
    Euler-Bernoulli value `(1.875² / 2π) · √(EI / ρAL⁴)` within 10%
    (P1-tet bending-lock floor — this beam is L/r≈346 about the h=2mm
    weak axis and f∝√K, so it cannot reach 2% at P1 at any CI-practical
    mesh; the original 2% is aspirational, gated on the P2-tet modal
    follow-up task 4066 — see §9.1 and the 2026-05-29 achievability survey).
  - `examples/modal/simply_supported_beam_modes.ri` — same beam, both
    ends pinned. First three frequencies match `(nπ)² / 2π · √(EI / ρAL⁴)`
    within 10% (same P1-tet bending-lock floor as the cantilever, and
    higher modes lock harder; the original 2% is aspirational, gated on
    the P2-tet modal follow-up task 4066).
  - `examples/modal/transient_step_response.ri` — apply a unit step
    force to the cantilever tip. Time-history of tip displacement
    shows damped sinusoid; decay envelope `exp(-ζω_n t)` matches
    Rayleigh-derived ζ within 5%.
  - `examples/modal/printer_gantry_modes.ri` — dogfood fixture; reports
    first 5 modes of the printer-build gantry. Used as the
    resonance-budget reference downstream of motor-tuning.

**Diagnostic signals:**

- `E_ModalNoMassMatrix` — Part has no Material assigned; mass matrix
  cannot be assembled.
- `W_ModalRigidBodyMode` — eigenvalue near zero (frequency ≈ 0); Part
  has insufficient boundary conditions (rigid-body mode present).
- `W_ModalConvergence` — Lanczos failed to converge to the requested
  `n_modes`; returned only the converged subset.
- `E_TransientForcingMissing` — transient_response called with empty
  forcing time history.

## §2 — Scope

### §2.1 — Mechanism table

| # | Mechanism | State today | Owner |
|---|---|---|---|
| α | `Mode` value type (frequency, shape, participation_mass) | NEW | this PRD |
| β | `ModalResult` value type (part, modes, boundary_conditions, damping) | NEW | this PRD |
| γ | `RayleighDamping` / `NoDamping` damping descriptor | NEW | this PRD |
| δ | Mass-matrix assembly in `reify-solver-elastic` (consistent mass for tet4 elements; lumped variant in §7.3) | NEW (extends solver-elastic) | this PRD |
| ε | Shift-invert Lanczos eigensolver reuse from buckling-eigensolver (cross-PRD generic refactor) | NEW (consumes buckling Phase X) | buckling-eigensolver |
| ζ | `modal_analysis(part, opts) -> ModalResult` stdlib fn + eval dispatch | NEW | this PRD |
| η | `ForcingTimeHistory` value type (per-DOF time-series force) | NEW | this PRD |
| θ | Mode-superposition transient solver (per-mode 2nd-order ODE in modal coords; exact Duhamel integration over uniform sample grid) | NEW | this PRD |
| ι | `transient_response(modal_result, forcing, t_range, dt) -> DisplacementTimeHistory` stdlib fn + eval dispatch | NEW | this PRD |
| κ | ComputeNode trampoline registration for `modal_analysis` (eigensolver call is expensive) | NEW | this PRD |
| λ | ComputeNode trampoline registration for `transient_response` (per-mode ODE integration is moderate; benefits from caching given trajectory-input-shaping's per-iteration calls) | NEW | this PRD |

### §2.2 — Bookmark tasks

- **Full-mechanism flexible-multibody PRD slot** —
  `docs/prds/v0_4/flexible-multibody-modal.md` (unauthored). Goal:
  Craig-Bampton component-mode synthesis assembling per-Part modes across
  mechanism joint interfaces. Captures cross-body modes (gantry + toolhead
  coupling). Triggers: per-Part modal analysis dogfood reveals that
  cross-body modes are the actual bottleneck on a real printer.
- **Complex-eigenvalue / non-proportional damping PRD slot** —
  `docs/prds/v0_5/complex-modal-analysis.md` (unauthored). Goal: quadratic
  eigenvalue problem `(K + iωC - ω²M)x = 0` for systems with localized
  damping. Triggers: rubber-mount / damper isolation design need.

### §2.3 — Out of scope

- **Full-mechanism modal coupling** (bookmarked).
- **Non-proportional damping** (bookmarked).
- **Modal correlation / MAC against experimental data.** Out of scope;
  this is post-FRF analysis. Filed as future PRD slot if dogfood demands.
- **Modal optimization / topology optimization driven by modal
  frequencies.** Out of scope.
- **Acoustic radiation modes.** Out of scope.
- **Aeroelastic / fluid-coupled modes.** Out of scope (no fluid solver).
- **Geometrically nonlinear modal analysis** (large pre-stress effects on
  frequencies, e.g. spinning blades, taut strings). Buckling handles
  the small-strain pre-stress case; nonlinear modal is separate.

## §3 — Pre-conditions for activating

| Pre-condition | Owner | Status today (2026-05-17) | Gate phase |
|---|---|---|---|
| `Part`, `Material`, `Support` value types stable | solver-elastic v0.3 + structure-instance-runtime | solver-elastic landing; SIR-α in flight | hard prereq for all phases |
| Shift-invert Lanczos refactored into a generic `lanczos_shift_invert<M_op, K_op>` | `docs/prds/v0_5/buckling-eigensolver.md` (refactor task to be filed in buckling decomp) | not yet filed | hard prereq for ε |
| FEA-stack mass-matrix slot: solver-elastic exposes a mass-matrix-assembly hook on the same mesh / element basis as stiffness assembly | this PRD task δ | new (added by this PRD) | hard prereq for ε onwards |
| ComputeNode contract (GR-002) | `compute-node-contract.md` | landed | substrate for κ, λ |
| SIR-α | `structure-instance-runtime.md` task 3540 | in-flight | hard prereq for α, β, γ, η ctor lowering |

The dependency on buckling-eigensolver's Lanczos refactor is the
load-bearing cross-PRD edge: this PRD's Phase 2 (ε) blocks on the
generic-Lanczos task. Per [[preferences_cross_prd_deps_real_edges]]
this is wired as a real `add_dependency` edge at decompose time.

## §4 — Contract: `Mode`, `ModalResult`, `RayleighDamping`

### §4.1 — Mode + ModalResult

```reify
structure def Mode {
    param frequency        : Frequency      // ω / 2π in Hz
    param shape            : List<Vec3>     // mass-normalized; Φ_i^T·M·Φ_i = 1
    param participation_mass : Real         // effective modal mass along
                                            //   a reference direction (set
                                            //   at modal_analysis call time)
    param damping_ratio    : Real           // ζ_i = (αω_i² + β)/(2ω_i)
                                            //   for Rayleigh; 0 for undamped
}

structure def ModalResult {
    param part                  : Part
    param modes                 : List<Mode>    // ordered by frequency
    param boundary_conditions   : List<Support>
    param damping               : DampingDescriptor  // see §4.2
    param mass_matrix_norm      : Real          // ‖M‖_F for diagnostics
    param stiffness_matrix_norm : Real          // ‖K‖_F for diagnostics
}
```

**Mass normalization.** Mode shapes are normalized such that
`Φ^T · M · Φ = I` (identity). This is the standard form for modal
superposition; the per-mode "generalized mass" is unity and the
generalized stiffness is `ω²`.

**Participation mass.** Computed for a reference direction `d` (default:
the gravity direction or a user-supplied `Vec3`) as
`m_eff,i = (Φ_i^T · M · d)²`. Sum over modes equals total Part mass in
direction `d` — useful for spotting modes that contribute negligible mass.

### §4.2 — DampingDescriptor

```reify
trait DampingDescriptor {
}

structure def NoDamping : DampingDescriptor {
}

structure def RayleighDamping : DampingDescriptor {
    param alpha : Real    // mass-proportional coefficient (1/s)
    param beta  : Real    // stiffness-proportional coefficient (s)
}
```

Per-mode damping ratio derived from Rayleigh parameters:

```
ζ_i = (α + β·ω_i²) / (2·ω_i)
```

This preserves mode-shape orthogonality (the decoupled modal ODEs stay
1D-second-order), so transient response stays in real arithmetic.

### §4.3 — ModalOptions

```reify
structure def ModalOptions {
    param n_modes               : Int               // request count
    param boundary_conditions   : List<Support>
    param damping               : DampingDescriptor // default: NoDamping
    param sigma                 : Frequency         // shift-invert origin
                                                    //   default: 0 Hz (finds
                                                    //   lowest)
    param tol                   : Real              // Lanczos tolerance
                                                    //   default: 1e-9
    param max_iters             : Int               // default: 200
    param reference_direction   : Vec3              // for participation_mass
                                                    //   default: -Z_HAT (gravity)
}
```

Mirrors `BucklingOptions` from `buckling-eigensolver.md` §4 — same
six-knob shape, swapped semantics of `sigma` (shift-invert origin in
frequency space rather than buckling-load space). Validation runs at
ctor: `n_modes >= 1`, `tol > 0`, `max_iters >= 1`,
`reference_direction.norm() > 0`.

## §5 — Contract: Transient response

### §5.1 — ForcingTimeHistory

```reify
trait ForcingFunction {
}

// Common forcing primitives:
structure def StepForce : ForcingFunction {
    param at           : LocationId   // where on the Part
    param direction    : Vec3         // unit vector
    param magnitude    : Force        // N
    param start_time   : Time         // step from 0 to magnitude at t=start_time
}

structure def ImpulseForce : ForcingFunction {
    param at           : LocationId
    param direction    : Vec3
    param impulse      : ImpulseDim   // N·s = kg·m/s
    param time         : Time         // Dirac delta time
}

structure def HarmonicForce : ForcingFunction {
    param at           : LocationId
    param direction    : Vec3
    param amplitude    : Force
    param frequency    : Frequency
    param phase        : Angle        // default: 0
}

structure def SampledForce : ForcingFunction {
    param at           : LocationId
    param direction    : Vec3
    param time_samples : List<Time>
    param force_samples: List<Force>
}

structure def ForcingTimeHistory {
    param part      : Part
    param sources   : List<ForcingFunction>  // additive
}
```

For mechanism-driven forcing (the trajectory-input-shaping consumer),
the trajectory PRD provides a helper that converts a mechanism's
inverse-dynamics torque time history into a per-Part forcing time
history via `rigid_body_dynamics.inverse_dynamics(mech, traj)` →
joint-attachment-point reaction forces → `ForcingTimeHistory`.

### §5.2 — DisplacementTimeHistory

```reify
structure def DisplacementTimeHistory {
    param part         : Part
    param modal_result : ModalResult       // back-reference
    param t_samples    : List<Time>
    param mode_coords  : List<List<Real>>  // outer = modes, inner = time
                                           //   ξ_i(t_j); reconstruction
                                           //   uses Φ_i to expand to
                                           //   physical displacement
}

// Reconstruction accessor (lazy; doesn't materialize the full
// per-node-per-time matrix unless queried):
fn displacement_at(history: DisplacementTimeHistory,
                   location: LocationId,
                   direction: Vec3) -> List<Real>
```

The lazy reconstruction avoids materializing the (n_nodes × n_times)
matrix when only a handful of locations are queried — typical for
input-shaping iteration.

### §5.3 — Per-mode ODE solution

In modal coordinates, each mode's response is decoupled:

```
ξ̈_i + 2·ζ_i·ω_i·ξ̇_i + ω_i²·ξ_i = f_i(t) / m_i = Φ_i^T · F(t)
```

For mass-normalized modes, `m_i = 1`. For uniformly-sampled forcing,
the solution over each timestep uses the **exact Duhamel-integral
formula** for a linear second-order system with linearly-varying
forcing (analytically integrable per-timestep). This is more accurate
and more stable than Newmark-β for the typical input-shaping use case
(short trajectories, well-defined modes, uniform sampling).

For non-uniform forcing (e.g. `SampledForce` with irregular
`time_samples`), the integrator falls back to Newmark-β (γ=1/2, β=1/4,
unconditionally stable).

### §5.4 — Quasi-static initial condition

Default IC: zero displacement, zero velocity in modal coordinates.
Optional `ic_offset_displacement: List<Vec3>` and `ic_offset_velocity`
parameters supported via a `TransientOptions` struct (filed as Open
Question §12.2 — see notes there).

## §6 — Contract: ComputeNode wiring

Both `modal_analysis` and `transient_response` are expensive and
candidates for ComputeNode caching:

```rust
#[compute_node]
pub fn modal_analysis_node(
    part:  ComputeNodeInput<PartHandle>,
    opts:  ComputeNodeInput<ModalOptionsValue>,
    state: &mut OpaqueState<ModalAnalysisCache>,
    cancel: &CancellationHandle,
) -> Result<ModalResult, DiagnosticCode>;

#[compute_node]
pub fn transient_response_node(
    modal:   ComputeNodeInput<ModalResultHandle>,
    forcing: ComputeNodeInput<ForcingTimeHistoryHandle>,
    t_range: ComputeNodeInput<TimeRange>,
    dt:      ComputeNodeInput<Time>,
    state:   &mut OpaqueState<TransientCache>,
    cancel:  &CancellationHandle,
) -> Result<DisplacementTimeHistory, DiagnosticCode>;
```

`modal_analysis_node` caching: keyed on (part-content-hash, opts-hash,
mesh-element-set-hash). OpaqueState carries the previously-assembled
(K, M) matrices to amortize across multiple modal_analysis calls
differing only in `n_modes`.

`transient_response_node` caching: keyed on (modal-result-hash,
forcing-hash, t_range-hash, dt-hash). Cheap re-evaluation for input-
shaping iteration that changes `forcing` while `modal_result` stays
fixed.

Cancellation: modal_analysis honours cancellation between Lanczos
iterations. transient_response honours cancellation between timesteps
(per mode) or between modes (per timestep) — whichever is more
granular for typical workloads (likely per-timestep).

## §7 — Resolved design decisions

**(7.1) Per-Part scope, no multi-body coupling in v0.3.** Selected
over Craig-Bampton because: (a) per-Part modes capture the dominant
ringing modes of a typical printer frame and toolhead; (b) component-
mode synthesis is a substantial 3–5× scope expansion; (c) the v0.5+
flexible-multibody PRD slot is reserved for when dogfood reveals
cross-body modes are the limiting factor.

**(7.2) Free-vibration plus mode-superposition transient response in
one PRD.** Transient response with proportional damping is a natural
extension of free vibration (per-mode decoupled ODEs in real
arithmetic). Trajectory-input-shaping consumes transient_response
directly, so packaging them together avoids cross-PRD plumbing.

**(7.3) Rayleigh proportional damping; bookmark complex damping.**
Rayleigh damping preserves mode orthogonality and keeps the modal-
superposition path real-valued. Captures the dominant damping
behaviour of metal frames within engineering tolerance. Localized /
non-proportional damping requires the quadratic eigenvalue problem
and is bookmarked separately.

**(7.4) Defer eigensolver to buckling-eigensolver's Lanczos refactor.**
Selected over independent-Lanczos and refactor-in-this-PRD because:
(a) refactoring shared code in the PRD that already owns it
(buckling) is cleaner; (b) the dependency is real and load-bearing;
(c) this PRD's Phase 2 (ε) blocks on the refactor task — a clean
sequencing point that mirrors the SIR-α / KGQ Phase 4 pattern in
sibling PRDs.

**(7.5) Mass-normalized mode shapes (Φ^T·M·Φ = I).** Standard for
modal superposition. Decouples the per-mode ODE generalized mass to
unity. Mode-shape `shape` field carries the normalized shape; users
who need physical displacement units multiply by the modal coordinate
ξ_i and recover physical-space displacement.

**(7.6) Consistent mass matrix as default; lumped-mass available as
opt-in.** §7.3 — wait, this is a §9 entry. Let me adjust: consistent
mass is the default (more accurate, slightly more expensive to
assemble); a `ModalOptions.mass_lumping: Bool = false` flag opt-ins to
lumped mass when needed for very large meshes. Filed in Open
Questions §12.4.

**(7.7) Exact Duhamel per-timestep integration for uniform forcing;
Newmark-β fallback.** Selected because: (a) per-mode 2nd-order linear
ODE has a closed-form Duhamel solution over uniform timesteps; (b)
exact integration eliminates numerical-dissipation artifacts; (c)
input-shaping iteration benefits from accurate response prediction.
Newmark-β fallback handles non-uniform sampling.

**(7.8) ComputeNode wiring for both modal_analysis and
transient_response.** Both are expensive enough to warrant caching;
trampoline boilerplate cost is small. transient_response especially
benefits since input-shaping iterates with varying `forcing` while
`modal_result` is fixed.

## §8 — Cross-PRD relationship (G4)

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `docs/prds/v0_5/buckling-eigensolver.md` | consumes (cross-version) | shift-invert Lanczos generic refactor | buckling-eigensolver | refactor task to be filed in buckling decomp |
| `docs/prds/v0_3/structural-analysis-fea.md` (solver-elastic) | extends | mass-matrix assembly added in same crate alongside existing stiffness assembly | this PRD | extends solver-elastic |
| `docs/prds/v0_3/structure-instance-runtime.md` (SIR / GR-001) | consumes | `Value::StructureInstance` + ctor lowering for `Mode`, `ModalResult`, `DampingDescriptor`, `ForcingTimeHistory`, etc. | SIR | queued (task 3540 in flight) |
| `docs/prds/v0_3/compute-node-contract.md` (GR-002) | consumes | `#[compute_node]` trampoline | GR-002 | landed |
| `docs/prds/v0_3/engine-integration-norm.md` (GR-017) | references | §3.4 ComputeNode dispatch seam | GR-017 | landed |
| `docs/prds/v0_3/trajectory-input-shaping.md` (this session) | produces | `transient_response(...)` consumed by input-shaping iteration | this PRD | both authored this session |
| `docs/prds/v0_3/rigid-body-dynamics.md` (this session) | independent | none direct; both consume FEA stack at different layers | n/a | both authored this session |
| `docs/prds/v0_4/flexible-multibody-modal.md` (bookmark) | produces | per-Part modes consumed by Craig-Bampton component synthesis | this PRD | bookmark only |

No reciprocal-ownership ambiguity. The cross-version dependency on
v0.5 buckling-eigensolver is the most interesting seam: this PRD ships
in v0.3 conceptually but blocks on a buckling-eigensolver refactor
task that lives in the v0.5 PRD's decomposition. Scheduling-wise the
refactor can land first (buckling-eigensolver Phase 1 / 2 of its
decomp) without the full buckling decomp completing.

## §9 — Boundary test sketch (cross-crate; facing both ways)

### §9.1 — Producer-side

| Scenario | Preconditions | Postconditions |
|---|---|---|
| **Cantilever first-mode ground truth.** Uniform steel beam L=200mm, b=10mm, h=2mm (L/r≈346, weak-axis bending). | δ, ε, ζ wired. | `modes[0].frequency` matches `(1.875²/2π)·√(EI/ρAL⁴)` ≈ 41.0 Hz within 10% (P1-tet bending-lock floor; original 2% aspirational → P2-tet modal follow-up task 4066). |
| **Simply-supported beam first three modes.** Same beam pinned at both ends. | δ, ε, ζ wired. | First three frequencies match `(nπ)²/2π·√(EI/ρAL⁴)` within 10% (P1-tet bending-lock floor; original 2% aspirational → P2-tet modal follow-up task 4066). |
| **Rayleigh damping decay envelope.** Cantilever, β=1e-4, free vibration from initial displacement. | γ, ζ, ι wired. | Tip displacement envelope `‖x(t)‖ ≤ x₀·exp(-ζ_1·ω_1·t)` within 5% over t ∈ [0, 10/ω_1]. |
| **Step-force impulse response.** Cantilever tip force step of 10N. | η, ι, θ wired. | Tip displacement settles to the FEA static solution for t → ∞ (which itself under-predicts the analytic `F·L³/(3EI)` by the P1-tet bending-lock margin on this slender beam — compare to the static FEA result, not the closed form, unless on task 4066); ringing frequency matches mode 1; decay matches Rayleigh ζ_1. |
| **Mass-normalization invariant.** Any modal_result. | ζ wired with normalization step. | For each Mode i: `Φ_i^T · M · Φ_i = 1.0` within 1e-12; `Φ_i^T · M · Φ_j = 0.0` for i ≠ j. |
| **Modal participation mass conservation.** | ζ wired. | Σ `modes[i].participation_mass` ≈ total Part mass along reference direction, within 1% (modes capture ≥99% of mass; warning if not). |
| **Rigid-body mode detection.** Unconstrained Part. | ζ wired. | At least one mode has ω ≈ 0 (within 1e-6 of zero); `W_ModalRigidBodyMode` diagnostic emitted. |
| **ComputeNode cache hit on modal_analysis.** Two calls with identical Part + opts. | κ wired. | Second call is a cache hit. |
| **ComputeNode cache hit on transient_response varying forcing.** Same modal_result, different forcing. | λ wired. | Re-evaluation skips modal_analysis path; only re-integrates ODE. |

### §9.2 — Consumer-side

| Scenario | Preconditions | Postconditions |
|---|---|---|
| **Input-shaping algorithm reads modes.** trajectory-input-shaping reads `ModalResult.modes` for shaper-design. | This PRD's β stable. | Trajectory PRD tests compile + pass against the API surface. |
| **Input-shaping forward-pass via transient_response.** Trajectory PRD's iteration computes expected end-effector trajectory error. | This PRD's ι stable. | Trajectory PRD's iteration test pins expected residual ringing reduction (e.g. ZV-shaped step input reduces tip-vibration peak by >40dB). |
| **GUI dashboard surfaces first-mode frequency.** Editing the Part triggers ComputeNode re-evaluation; viewport reports `modes[0].frequency` live. | κ wired with cache-invalidation. | GUI shows live-updating first-mode frequency on Part edits. |

## §10 — Decomposition plan

Vertical-slice B+H. Phase 1 foundation (structure_defs + mass-matrix
assembly); Phase 2 free-vibration; Phase 3 transient response; Phase 4
ComputeNode wiring; Phase 5 dogfood + companion + bookmarks.

### Phase 1 — Foundation

- **α — `Mode`, `ModalResult`, `DampingDescriptor` family, `ModalOptions`
  `structure def`s.**
  - Crates: `reify-compiler/stdlib/modal_analysis.ri` (NEW), constraint
    validation (n_modes ≥ 1, etc.).
  - Observable signal (intermediate): compile test confirms templates;
    `ModalOptions(n_modes: 0)` emits constraint-violation diagnostic.
  - Prereqs: SIR-α task 3540.

- **δ — Consistent mass-matrix assembly in solver-elastic.**
  - Crates: `reify-solver-elastic/src/mass_matrix.rs` (NEW), exposed
    via existing FEA assembly pipeline.
  - Observable signal (intermediate): unit test confirms total mass of
    a uniform-density block equals ρV within 1e-12; mass matrix is
    symmetric PSD.
  - Prereqs: solver-elastic v0.3 stiffness assembly landed.

### Phase 2 — Free vibration

- **ε — Generic shift-invert Lanczos refactor in buckling-eigensolver.**
  - Crates: refactor `reify-solver-elastic` or move into a shared
    `reify-eigen-solver` crate (decision at impl time). Generic over
    operator pairs (K, K_g) vs (K, M).
  - Observable signal: existing buckling tests still pass; new
    smoke test in shared crate confirms eigenvalues recoverable for
    a (K, M) input pair.
  - **This task lives in buckling-eigensolver's decomposition**, not
    this PRD's batch — but is wired as a hard cross-PRD prereq.
  - Prereqs: buckling-eigensolver Phase 1 / 2 landed.

- **ζ — `modal_analysis(part, opts) -> ModalResult` stdlib fn + eval
  dispatch + ε consumer wiring.**
  - Crates: `reify-stdlib/src/modal/free_vibration.rs` (NEW),
    `reify-eval/src/modal_ops.rs` (NEW).
  - Observable signal: `examples/modal/cantilever_beam_modes.ri` and
    `examples/modal/simply_supported_beam_modes.ri` end-to-end via
    `reify eval`; first-mode frequencies within 2% of analytic.
  - Prereqs: α, δ, ε.

### Phase 3 — Transient response

- **η — `ForcingFunction` family + `ForcingTimeHistory` structure_defs.**
  - Crates: `reify-compiler/stdlib/modal_analysis.ri` (extend), constraint
    validation.
  - Observable signal (intermediate): templates compile; constructor
    constraints fire on bad input (`HarmonicForce(amplitude: -1N, ...)`
    diagnostic).
  - Prereqs: SIR-α.

- **θ — Mode-superposition transient solver (exact Duhamel uniform +
  Newmark-β fallback).**
  - Crates: `reify-stdlib/src/modal/transient.rs` (NEW), unit tests
    confirming exact-integration accuracy on analytic test problems
    (single-DOF mass-spring-damper with sinusoidal forcing).
  - Observable signal: unit tests pin Duhamel solution accuracy
    (relative error < 1e-9 vs analytic for a single-DOF system at 50
    sample points).
  - Prereqs: ζ, η.

- **ι — `transient_response(...)` stdlib fn + eval dispatch +
  DisplacementTimeHistory + lazy reconstruction.**
  - Crates: `reify-stdlib/src/modal/transient.rs` (extend),
    `reify-eval/src/modal_ops.rs` (extend).
  - Observable signal: `examples/modal/transient_step_response.ri`
    runs end-to-end; decay envelope match within 5%.
  - Prereqs: θ, ζ.

### Phase 4 — ComputeNode wiring

- **κ — ComputeNode trampoline for modal_analysis.**
  - Crates: `reify-stdlib/src/modal/trampoline.rs` (NEW), engine
    integration per `engine-integration-norm.md` §3.4.
  - Observable signal: cache-hit test passes; cancellation test
    passes; first-mode-frequency live-update smoke test against the
    GUI debug MCP.
  - Prereqs: ζ, GR-002.

- **λ — ComputeNode trampoline for transient_response.**
  - Crates: `reify-stdlib/src/modal/trampoline.rs` (extend).
  - Observable signal: cache-hit test pins skip-modal-analysis when
    forcing varies but modal_result is identical.
  - Prereqs: ι, GR-002.

### Phase 5 — Dogfood, companion, bookmarks

- **μ — Printer-gantry modal dogfood `.ri`.**
  - Files: `examples/modal/printer_gantry_modes.ri` (NEW).
  - Observable signal: example runs, prints first 5 modes.
  - Prereqs: ζ, κ.

- **ν — File bookmark tasks: flexible-multibody PRD slot + complex-
  damping PRD slot.**
  - Two deferred bookmark tasks per [[preferences_bookmark_task_pattern]],
    one each for the future PRD slots.
  - Prereqs: none.

### §10.1 — Dependency view

```
α (structure defs) ──┐
                     ├──→ ζ (modal_analysis)
δ (mass matrix) ─────┤        │
                     │        │
buckling-eig → ε ────┘        ├──→ κ (CN modal)
                              │
η (forcing types) ──→ θ ─→ ι ─┼──→ λ (CN transient)
                              │
                              └──→ μ (dogfood)

ν (bookmarks) is independent.
```

10 tasks in-batch + 2 bookmarks + 2 cross-PRD edges (buckling-
eigensolver Lanczos refactor + SIR-α 3540).

## §11 — Out of scope for this PRD

(See §2.3 for full list. Plus.)

- **Modal optimization** — using modal frequencies as a design objective.
- **Damping identification** — extracting Rayleigh α, β from
  experimental data.
- **Periodic / Floquet modal analysis** — for parametrically excited
  systems.
- **Fluid-structure coupled modes** — no fluid solver in Reify.

## §12 — Open questions (surfaced but not decided in this session)

1. **Reference direction default for participation_mass.** §4.3 picks
   `-Z_HAT` (gravity). For horizontal-print printers (rare) this is
   fine. For gantry-style FDM where the relevant direction is the
   print-bed-Z, a different default may be more useful. **Suggested
   resolution:** keep `-Z_HAT` default; allow per-call override; revisit
   if dogfood reveals confusion.

2. **Initial-condition support in transient_response.** §5.4 picks
   zero IC default. Some use cases (e.g. analyzing settling after a
   prior motion) need non-zero initial state. **Suggested
   resolution:** v0.3 ships zero IC only; add `TransientOptions.ic`
   in a follow-up if dogfood demand.

3. **Consistent vs lumped mass matrix.** §7.6 picks consistent default
   with lumped opt-in. The lumped variant trades accuracy for
   assembly speed and matrix sparsity. **Suggested resolution:** ship
   consistent only in v0.3; add `mass_lumping: Bool` flag in a
   follow-up if mesh size becomes a perf bottleneck.

4. **Eigenvalue sign convention near `sigma`.** Shift-invert Lanczos
   finds eigenvalues closest to `sigma`. For sigma=0 this gives the
   lowest frequencies (intended). For positive sigma it finds
   frequencies straddling sigma, in both directions. **Suggested
   resolution:** document the behaviour; provide convenience helpers
   `lowest_modes(part, n)` and `modes_near(part, n, freq)`.

5. **Forcing-function piecewise definition.** A user may want to
   compose forcing time histories (step + harmonic + damped sine).
   §5.1 has additive `ForcingTimeHistory.sources` but no overlap
   semantics for sources that share `at + direction`. **Suggested
   resolution:** sources sum linearly at each (t, location);
   document; defer multi-step composition syntactic sugar to a
   follow-up.

## §13 — Gap-register companion edits

Adds "modal analysis" as a new mechanism cluster in gap-register. No
existing GR row claims free-vibration or transient-response analysis.
Companion task adds the row.

End of PRD.
