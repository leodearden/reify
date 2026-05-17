# Kinematic Constraints — v0.2 Completion Contract

Status: contract resolving the accreted `docs/prds/v0_2/kinematic-constraints.md`
(authored 2026-04-28, decomposed into tasks 2583, 2670–2678 of which 8 landed
and 3 closed partial). Authored 2026-05-17 in interactive `/prd` session.
Pending Leo approval before queueing tasks.

## §0 — Purpose and supersession

The v0.2 kinematic-constraints PRD shipped 8 of 10 decomposed tasks. Three
residual mechanism gaps (singularity diagnostics not plumbed; multi-DOF joints
excluded from closed chains; analytic Jacobians stubbed) plus three gaps
inherited from the un-retired v0.1 top-level PRD (FK-aware interference,
first-class types, GUI sliders for literal-bound joints) leave the headline
"verify mechanism range-of-motion and clearance under motion" use case
materially un-completed.

This PRD names every residual mechanism, fixes seams against the v0.3 stack
that didn't exist when the v0.2 PRD was authored (SIR-α struct-instance
runtime, KGQ kernel-geometry-queries, GR-016 GUI event channel, GR-017
engine-integration-norm), and serves as the **completion contract** for the
v0.2 PRD: when this PRD's decomposition lands, v0.2 is done and v0.1 is
formally superseded.

Supersession links:
- This PRD **supersedes** `docs/prds/kinematic-constraints.md` (v0.1 top-level,
  not formally retired). §15 names the supersession edit task.
- This PRD **completes** `docs/prds/v0_2/kinematic-constraints.md`. The v0.2
  file remains as the design source-of-truth for solver semantics; this PRD's
  §4–§8 supply the missing contract specificity.
- Audit evidence:
  - `docs/architecture-audit/findings/kinematic-constraints-v02.md` —
    mechanisms M-001 through M-018; top concerns 1–4.
  - `docs/architecture-audit/findings/kinematic-constraints-toplevel.md` —
    mechanisms M-019 through M-023; gaps inherited from v0.1.
  - `docs/architecture-audit/gap-register.md` — cluster C-37 (FK-ignoring
    interference); related rows for kinematic-singularity surfacing.

## §1 — Goal and user-observable surface

A reify user can verify mechanism range-of-motion, clearance under motion,
and numerical-condition health of any v0.2-supported joint topology
(open or closed chain; full joint zoo) without manual workarounds. Concretely:

**CI-gate signals (hard acceptance):**

- `cargo test -p reify-eval --test kinematic_examples_e2e` runs three
  end-to-end fixtures and they all pass:
  - `examples/kinematic/dock_pickup.ri` — toolchanger dock pickup-path
    clearance check under prismatic-prismatic open chain. Sweep over Y∈[0,
    800mm] and X∈[0, 500mm]; `min_clearance` returns positive values along
    the planned path, negative inside the dock collision shape, transitions
    cleanly. The `// FIXME: pre-positioned` comment block at the head of the
    example is removed.
  - `examples/kinematic/counter_mass_balance.ri` — counter-mass coupling
    verification under a closed-chain mechanism whose loop traverses one
    planar joint. Snapshot returns a concrete `Snapshot` value (today: `Undef`);
    `center_of_mass(snap)` returns a finite point; `is_singular` is false.
  - `examples/kinematic/four_bar_singular.ri` (NEW) — a four-bar linkage at
    a kinematic singularity. Snapshot returns `is_singular: true` AND
    `EvalResult.diagnostics` contains a `KinematicSingularity` warning code
    naming the loop joint pair.

**GUI dogfood signal (soft acceptance at printer-build milestone):**

- Open `examples/kinematic/dock_pickup.ri` in the GUI. The
  `MechanismPanel` exposes a slider for `x_axis` even though it is bound
  via `bind(x_axis, 100mm)` literal — not via a `param` cell. Scrubbing
  the slider updates the snapshot bodies' world transforms in the
  viewport in real time; the IPC round-trip stays within the existing
  RAF-coalesced 200ms-per-frame budget. The printer-build mechanism (filed
  as a follow-up sweep) is the canonical dogfood fixture; dock_pickup is
  the regression gate.

**Diagnostic-stream signal:**

- The `KinematicSingularity` / `KinematicOverconstrained` /
  `KinematicUnderconstrained` typed `DiagnosticCode` variants reach
  `EvalResult.diagnostics` and surface through every existing diagnostic
  consumer (LSP hover, MCP `report_diagnostics`, CLI `reify check`).

## §2 — Scope

### §2.1 — Residual mechanism table

The seven residual mechanisms this PRD owns, each linked to its audit-finding
provenance and current state:

| # | Mechanism | Audit ref | State today | Owner |
|---|---|---|---|---|
| α | `solve_loop_closure_with_diagnostics` wired into `snapshot()` and `sweep()`; typed diagnostics reach `EvalResult` | v02 M-009, M-010 | PARTIAL — wrapper exists + unit-tested; production path bypasses it | this PRD |
| β | `is_singular: true` flag on Snapshot Map (always-present, false for clean snapshots) | v02 M-011 | FICTION — PRD prose mentions; no code carries it | this PRD |
| γ | Multi-DOF joints (planar, spherical, cylindrical) participate in closed-chain loops via `JointValue` chain widening | v02 M-007 (top concern 2) | PARTIAL — joints standalone-WIRED; closed-chain participation blocked | this PRD |
| δ | Analytic Jacobians for planar and spherical (cylindrical already done) | v02 top concern 3 | DRIFT — zero-magnitude placeholder columns at `joints.rs:785,800` | this PRD |
| ε | FK-aware OCCT interference / distance / clearance under Snapshot transforms | toplevel M-019, M-020, M-021 | DRIFT — per-body `world_transform` deliberately not applied | this PRD (KGQ dependency) |
| ζ | First-class language types `Type::Mechanism / Type::Joint / Type::Snapshot / Type::BodyId / Type::SweepDim`; `trait DrivingJoint` | toplevel M-022 | FICTION — documented but only `Value::Map` "kind" discriminant exists | this PRD (SIR-α dependency) |
| η | GUI per-joint slider for literal-bound joints (backend synth-virtual-param promotion) | toplevel M-023 | DRIFT — only param-bound joints scrubbable | this PRD (GR-016 substrate) |

A companion task `θ` formally retires the v0.1 top-level PRD (cosmetic but
durable; eliminates the audit-confusion source).

### §2.2 — What this PRD does NOT add

- **Inverse kinematics.** Carried forward from v0.2 PRD §"Out of scope".
  Will be addressed in the `rigid-body-dynamics` follow-up PRD's
  cross-PRD-relationship section.
- **Dynamics (kinetics).** Out of scope; the follow-up PRD
  `docs/prds/v0_3/rigid-body-dynamics.md` (this session) addresses it.
- **Contact / collision response.** Interference produces pairs; no
  corrected pose, no contact forces. Out of scope.
- **Compliant mechanisms / flexures.** Out of scope; addressed by the
  `compliant-joints-flexures` follow-up PRD (this session).
- **Cable / belt physics, path planning, trajectory generation.** Out of
  scope; trajectory addressed by `trajectory-input-shaping` follow-up
  PRD (this session).
- **Manifold-kernel parity for FK-aware queries.** Phase 5 of KGQ adds
  Manifold parity for the kernel-level scalar queries; FK-aware wrapping
  applies generically over kernels. Phase ε in this PRD ships OCCT only;
  Manifold-side parity follows automatically when KGQ's Manifold dispatch
  arms land (no extra work needed in this PRD).

## §3 — Pre-conditions for activating

| Pre-condition | Owner | Status today (2026-05-17) | Gate phase |
|---|---|---|---|
| **SIR-α** — `Value::StructureInstance` variant + ctor lowering shipped | `docs/prds/v0_3/structure-instance-runtime.md` task 3540 | in-flight | hard prereq for ζ |
| **KGQ-α** — `distance` / `interferes_with` / `min_clearance` dispatcher landed for OCCT | `docs/prds/v0_3/kernel-geometry-queries.md` Phase 2 | pending decomp | hard prereq for ε |
| **GR-016** — GUI event channel substrate for kinematic descriptor messages | `docs/prds/v0_3/gui-event-channel-inventory.md` | partial / decomp landed | substrate for η |
| **GR-017** — engine-integration-norm cite for §3.1 op-execute seam (FK-aware distance is op-execute, not a new seam) | `docs/prds/v0_3/engine-integration-norm.md` | landed | norm reference only |
| `JointValue` Rust enum lands on a foundation-prep task before γ widens chain signatures | this PRD task γ-pre | not yet filed | sequence within batch |

Pre-conditions are encoded as **real `add_dependency` cross-PRD edges** at
decompose time per `preferences_cross_prd_deps_real_edges`. Phase ζ blocks
on SIR-α task 3540 specifically; Phase ε blocks on KGQ's `distance`
dispatcher landing; Phase η consumes the GR-016 channel without blocking.

## §4 — Contract: Snapshot Map shape + diagnostic emission

### §4.1 — Snapshot Map shape after this PRD

```
Snapshot = Map {
    kind: "snapshot",
    bodies: List<BodySnapshot>,      // unchanged
    free_values: List<JointValue>,   // CHANGED: was List<Real>; carries
                                     //   per-joint multi-DOF values
    is_singular: Bool,               // NEW: always present, false for
                                     //   open chains / converged closed chains
    convergence: Map {               // NEW: present on closed-chain snapshots
        iterations: Int,             //   only; Open chains omit this field
        residual_norm: Real,         //   absent
        terminal_reason: String,     //   "converged" | "max_iter" |
                                     //   "diverged" | "singular_jacobian"
    },
}
```

**Invariants:**

1. `is_singular` is a `Bool`, never absent. Clean snapshots set it `false`.
2. `convergence` is absent on snapshots that did not run the loop-closure
   solver (i.e. open chains with no `loop_closures`). The accessor pattern
   is `snap.convergence.iterations` guarded by checking `loop_closures` on
   the originating Mechanism — not by `convergence` field presence on
   Snapshot, since `Map` field-absence is a coarse user-facing check.

### §4.2 — Diagnostic emission contract

The `solve_loop_closure_with_diagnostics` wrapper at
`crates/reify-stdlib/src/loop_closure_solver.rs:820` becomes the **sole**
entry point from `snapshot.rs` and `sweep.rs`. The bare
`solve_loop_closure` remains available for internal/test use but is no
longer called from the public eval path.

Emission rules:

| Condition | Diagnostic emitted | Snapshot `is_singular` | Snapshot return |
|---|---|---|---|
| Open chain (no closed loops) | none | `false` | bodies populated |
| Closed chain, converged within tolerance | none | `false` | bodies populated |
| Closed chain, `free_b.len() < 6` (over-constrained pre-check, per loop) | `E_KinematicOverconstrained` (per loop) | `true` (sticky) | bodies populated using fallback (last-iterate or midpoint per PRD §"Resolved design decisions") |
| Closed chain, `free_b.len() > residuals` (under-constrained pre-check) | `W_KinematicUnderconstrained` (per loop) | `false` | bodies populated using closest-to-previous / midpoint |
| Closed chain, Newton converged but ‖J‖ near singular (LDLᵀ singular-pivot detection from existing solver) | `W_KinematicSingularity` | `true` | bodies populated; last-converged config |
| Closed chain, Newton diverged (monotonic-divergence guard fired) | `W_KinematicSingularity` (with `terminal_reason: "diverged"` in `convergence`) | `true` | bodies populated; best-iterate |

Each diagnostic carries the responsible loop's `loop_closures` index and
the joint kind pair involved. Multiple loops can each emit; diagnostics
list preserves emission order.

### §4.3 — Over-constrained pre-check refinement

The audit's M-009 evidence cites the existing `free_b.len() < 6` test as
"too strict for the simple 1-DOF fixtures" — this is the deliberate reason
`snapshot.rs:158-167` documented the bypass. Resolution:

The pre-check operates on **residual rank** rather than raw `free_b.len()`.
A closed loop whose residual twist only constrains a subset of SE(3) (e.g.
a planar 4-bar constrains 3 DOF, not 6) reports its **effective residual
dimension**. If `free_b.len() < effective_residual_dim`, emit
`E_KinematicOverconstrained` — otherwise pass through.

Effective residual dimension is computed once per loop topology at
`mechanism().body()` close-edge time and stored on the `loop_closures`
record (new field: `residual_dim: Int`). For loops whose dimension cannot
be statically inferred (mixed-DOF chains), default to 6 and rely on the
Newton solver's singularity detector.

## §5 — Contract: `JointValue` and chain widening (M-007 fix)

### §5.1 — `JointValue` Rust enum

```rust
// crates/reify-stdlib/src/loop_closure_value.rs (NEW)
#[derive(Clone, Debug)]
pub enum JointValue {
    Scalar(f64),               // prismatic, revolute, coupling, fixed (= 0.0)
    Cyl([f64; 2]),             // cylindrical: [translation, rotation]
    Planar([f64; 3]),          // planar: [x, y, theta]
    Sphere([f64; 4]),          // spherical: quaternion [w, x, y, z]
}

impl JointValue {
    pub fn dof_count(&self) -> usize { ... }   // 1 | 2 | 3 | 3
    pub fn as_f64_slice(&self) -> &[f64] { ... }
    pub fn from_slice(kind: JointKind, dofs: &[f64]) -> Result<Self> { ... }
    pub fn renormalize_quaternion(&mut self) { ... }   // no-op for non-Sphere
}

pub fn flatten_dofs(values: &[JointValue]) -> Vec<f64> { ... }
pub fn unflatten_dofs(dofs: &[f64], shapes: &[JointKind]) -> Result<Vec<JointValue>, FlattenError> { ... }
```

### §5.2 — `value_for_joint` and `joint_range_midpoint` widening

The two functions at `loop_closure.rs:142-200` and `:322-356` change
return type from `Option<f64>` to `Option<JointValue>`. Arms for
planar/spherical/cylindrical that today return `None` now return
`Some(JointValue::Planar/Sphere/Cyl(...))` populated from the joint's
declared range mid-point (planar/cylindrical: range midpoints;
spherical: identity quaternion).

`chain_transform` at `loop_closure.rs:59-80` accepts `&[JointValue]`
and dispatches per-arm: prismatic/revolute/coupling use the
`JointValue::Scalar` arm; planar/spherical/cylindrical use the
corresponding multi-DOF arm calling into the existing
`joints::transform_at` multi-DOF paths.

### §5.3 — Newton solver widening

`newton_solve` operates on a flat `Vec<f64>` internally (matrix algebra
unchanged) but accepts a `&[JointKind]` shape descriptor to convert
between `Vec<JointValue>` (caller-facing) and `Vec<f64>` (math-facing) at
the boundaries. Convergence test on residual-twist norm unchanged.

Quaternion renormalization: after each Newton step, walk the
`free` slots that are `JointValue::Sphere` and call
`renormalize_quaternion()`. This is the standard projection step for
unit-quaternion manifold descent.

### §5.4 — Analytic Jacobian columns for planar / spherical (M-007 / §2.1 δ)

Replace zero-magnitude placeholders at `joints.rs:785` (planar) and
`joints.rs:800` (spherical):

- **Planar.** Three columns of the joint Jacobian, one per DOF (∂x, ∂y, ∂θ).
  Columns are the standard 2-prismatic-plus-1-revolute composite tangents
  expressed in the joint's parent frame.
- **Spherical.** Three columns corresponding to the body angular-velocity
  basis expressed in the body's current orientation; standard quaternion-
  to-twist Jacobian (the 4×3 matrix `0.5 * E(q)` where `E` is the
  quaternion-to-omega operator).

Cylindrical analytic Jacobian already wired per audit M-013 evidence
(`joints.rs:815-823`).

## §6 — Contract: FK-aware OCCT interference (M-019/20/21 fix)

### §6.1 — Pre-compose strategy

The `world_transform`-ignoring path at `geometry_ops.rs:1333-1340`
changes to: when evaluating `distance(a, b)` / `interferes_with(a, b)` /
`min_clearance(a, b)` where both `a` and `b` are **resolvable to a body
in a Snapshot context** (i.e. the call site has access to a Snapshot —
typically because the caller is `sweep(...)`, `snapshot(...)`, or an
explicitly-passed `snapshot` argument), the dispatcher composes
`t_rel = t_b.inverse() * t_a` from the snapshot's body transforms and
passes `t_rel` to the OCCT distance query.

### §6.2 — FFI extension

```rust
// crates/reify-kernel-occt/src/queries.rs — NEW function
pub fn distance_with_transform(
    shape_a: &OcctShape,
    shape_b: &OcctShape,
    t_rel: &Transform3,   // shape_a's frame expressed in shape_b's frame
) -> Result<f64, OcctError>;

pub fn interferes_with_transform(
    shape_a: &OcctShape,
    shape_b: &OcctShape,
    t_rel: &Transform3,
) -> Result<bool, OcctError>;
```

Both wrap `BRepBuilderAPI_Transform` on a single shape (the cheaper of
the two by topology size) before calling the existing distance probe.
No new OCCT topology naming concerns: the transformed copy lives for the
single FFI call and is dropped before naming bookkeeping runs.

### §6.3 — Snapshot-context resolution

`reify-eval` adds a new helper:

```rust
fn try_resolve_snapshot_body(value: &Value, ctx: &EvalContext) -> Option<(BodyId, Snapshot)>;
```

invoked at the head of `eval_kinematic_helper` for each operand. When
both operands resolve to bodies from the **same** snapshot, the
FK-aware path runs. When either operand is a raw `Solid` (no snapshot
context), the existing FK-ignoring path runs — preserving v0.1
behaviour for non-mechanism use cases. The user-facing examples
(`dock_pickup.ri`) operate inside a `sweep(m, x_axis, ...)` block where
the snapshot context is implicit.

When operands come from different snapshots, emit
`E_KinematicSnapshotMismatch` and return `Value::Undef`.

## §7 — Contract: First-class types + `DrivingJoint` (M-022 fix)

### §7.1 — Type registration via SIR-α

Once SIR-α (`docs/prds/v0_3/structure-instance-runtime.md` task 3540)
lands, this PRD's task ζ adds stdlib type declarations under a new
`crates/reify-compiler/stdlib/kinematic.ri`. Surface syntax mirrors
the conformance-via-`structure def X : TraitName` pattern shipped in
`materials_fea.ri` (G3-confirmed against tree-sitter):

```reify
// Marker trait — Reify has no method-declaration syntax; conformance
// signals "this joint kind contributes motion variables to a sweep".
trait DrivingJoint {
}

// Per-joint-kind structures conform via the `: DrivingJoint` clause.
// The five DrivingJoint-conforming kinds:
structure def Prismatic : DrivingJoint {
    param axis : Vec3
    param range : Range<Length>
}
structure def Revolute : DrivingJoint {
    param axis : Vec3
    param range : Range<Angle>
    param pivot : Point3
}
structure def Cylindrical : DrivingJoint { /* per audit M-013 */ }
structure def Planar     : DrivingJoint { /* per audit M-014 */ }
structure def Spherical  : DrivingJoint { /* per audit M-015 */ }

// Non-conforming kinds: Coupling (derived, no independent motion var)
// and Fixed (0-DOF). Declared WITHOUT the `: DrivingJoint` clause so
// `bind(coupling, ...)` is a type error.
structure def Coupling { /* per v02 PRD §8 */ }
structure def Fixed    { /* per audit M-016 */ }

// Top-level kinematic value types.
structure def Mechanism {
    param bodies        : List<Body>
    param joint_parents : Map<BodyId, JointParent>
    param loop_closures : List<LoopClosureRecord>
}
structure def Snapshot {
    param bodies      : List<BodySnapshot>
    param free_values : List<JointValue>
    param is_singular : Bool
    param convergence : Option<ConvergenceReport>
}
structure def BodyId   { /* opaque per audit M-008 */ }
structure def SweepDim {
    param joint : Joint
    param range : Range
    param steps : Int
}
```

The existing `Value::Map`-based runtime representations remain;
SIR-α's `StructureInstance` variant wraps them with named-type
metadata, enabling:

- **Type errors at compile time:** `bind(coupling_joint, 0mm)`
  becomes `E_TypeMismatch: expected DrivingJoint, got Coupling`.
  Today this returns `Value::Undef` silently.
- **`MotionValue<J>`-typed sweep ranges:** `sweep_grid([dim(joint, range, steps)])`
  typechecks against `DrivingJoint` conformance instead of the
  hardcoded `KINEMATIC_FUNCTION_OVERRIDES` set in `units.rs:81-125`.

### §7.2 — `DrivingJoint` trait conformance

Conformance is **nominal**: declared at `structure def`-site via the
`: TraitName` clause (the only conformance mechanism Reify's grammar
supports; SIR-α extends this to runtime via the `StructureInstance`
variant). No separate `impl` block — Reify has no `impl` form. The
DrivingJoint-conforming structures are listed in §7.1.

This removes the `driving_joint_kind` hardcoded set at
`sweep.rs::driving_joint_kind` (a per-name compiler hook that
parallels the audit's M-022 evidence).

### §7.3 — Compiler hook retirement

`crates/reify-compiler/src/units.rs:81-125` `kinematic_query_result_type`
and `KINEMATIC_FUNCTION_OVERRIDES` are retired in favour of the standard
type-resolution path that SIR-α + this PRD's type declarations enable.
Companion correction task strips the per-name hook code and migrates the
three callers to the standard path.

## §8 — Contract: GUI slider promotion (M-023 fix)

### §8.1 — Engine-side: synth-virtual-param descriptor

The kinematic descriptor extractor at `engine.rs:998+, 1011+` is extended:

```rust
// gui/src-tauri/src/types.rs — extended JointDesc.binding shape
pub enum JointBinding {
    ParamBound { param_name: String, range: Range, current: Value },
    LiteralBound {
        synth_param_name: String,    // e.g. "__joint_x_axis_v"
        range: Range,                // from joint declaration
        initial_value: Value,        // the literal at the bind() site
        scrubbable: true,            // NEW: always true for literal-bound now
    },
    CouplingDerived { source_joint: String },
    FixedNoMotion,
}
```

The synth name follows pattern `__joint_<joint_id>_v`. Engine ensures
no name collision against user-defined `param` cells (rejection at
mechanism-build time if a user names a param `__joint_*`; emit
`W_KinematicReservedParamName`).

### §8.2 — Frontend: MechanismPanel scrub path

`gui/src/panels/MechanismPanel.tsx` removes the
"literal-bound" badge gate at `:186-190`. Both `ParamBound` and
`LiteralBound` joints render scrubbable sliders via the existing RAF
coalescing path. Visual distinction kept (an outline or small icon
indicates literal-bound vs param-bound) but slider is functional in
both cases.

### §8.3 — Persistence semantics

Scrubbing a literal-bound joint **does not write back to the source
file**. The slider value is session-only. Reset on file reload
restores the literal value. This is a deliberate design choice —
scrubbing is an exploration tool, not a source-editing tool. Users
wanting persistence wrap the joint binding in a `param` cell (existing
behaviour).

A future PRD may add "scrub-as-edit" if dogfood demand warrants;
filed as open question §14.1.

### §8.4 — Performance budget

The synth-virtual-param promotion adds zero IPC overhead beyond
the existing param-scrub path. The 200ms-per-frame budget cited in
the v0.2 PRD §7 remains the acceptance bar; no perf regressions
expected.

## §9 — Resolved design decisions

**(9.1) Multi-DOF joints in closed chains via `JointValue` enum.** Selected
over a flat-f64-vector-with-DOF-shape-descriptor approach because: (a)
future-proof for IK and dynamics PRDs which will reuse the per-joint
shape, (b) cleaner failure modes (unflatten_dofs returns a typed error
on shape mismatch), (c) the math-side flat-f64 representation lives
behind a thin shim and doesn't infect the surface API. Largest internal
churn but cleanest seam.

**(9.2) FK-aware OCCT via pre-compose, not gp_Trsf-per-shape.** Selected
because: (a) sweep iteration must not create one BRep per step (cost is
O(num_bodies × shape_size) per snapshot — prohibitive); (b) OCCT
distance is rigid-invariant, so pre-composing into the query is
algorithmically equivalent and shape-cache-friendly; (c) no PNv2 / topology
naming concerns since no new BRep is materialized.

**(9.3) First-class types gated hard on SIR-α (task 3540).** Selected
over a soft-block-with-fallback path because: (a) the fallback path would
ship a parallel type-registration mechanism that SIR-α would have to
retrofit later — exactly the audit's dominant failure shape; (b) SIR-α
is in flight and the dependency edge is the orchestrator's signal to
schedule ζ after 3540 lands.

**(9.4) GUI slider via backend synth-virtual-param.** Selected over
frontend AST-edit or reject-with-hint because: (a) backend descriptor
extension is the lowest-friction change (no source mutation, no MCP
edit-loop coupling); (b) keeps the slider rendering uniform across
param-bound and literal-bound joints; (c) session-only persistence
matches user intent in design-exploration contexts.

**(9.5) Over-constrained pre-check uses effective residual dimension.**
Selected over keeping the simple `free_b.len() < 6` test because: (a) the
existing test is the documented blocker for the wrapper plumb-through;
(b) effective residual dimension matches the physical reality of planar
4-bar (3-DOF residual), cylindrical-axis loops (2-DOF residual), etc.;
(c) computing it once at mechanism-build time and caching on the
`loop_closures` record is a small, local change.

**(9.6) `is_singular` is always-present `Bool`, not `Option<Bool>`.**
Open-chain snapshots set it to `false`. Consistent field shape across
all snapshot kinds; user code doesn't have to guard with field-presence
checks.

**(9.7) Diagnostic emission is per-loop, not per-mechanism.** A mechanism
with three loops, two of which are singular, emits two
`W_KinematicSingularity` diagnostics — each naming its responsible loop
index. Better debuggability; trivially aggregates at the consumer.

**(9.8) Literal-bound joint scrub is session-only (no AST writeback).**
Selected per §8.3; AST writeback deferred to a hypothetical
"scrub-as-edit" PRD if dogfood demand emerges.

## §10 — Cross-PRD relationship (G4)

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `docs/prds/v0_3/structure-instance-runtime.md` (SIR / GR-001) | consumes | `Value::StructureInstance` variant + ctor lowering for `Mechanism`, `Joint`, `Snapshot`, `Snapshot.is_singular`, `JointValue` | SIR | queued (task 3540 in flight) |
| `docs/prds/v0_3/kernel-geometry-queries.md` (KGQ) | consumes | `distance` / `interferes_with` / `min_clearance` dispatcher for OCCT | KGQ | queued (Phase 2) |
| `docs/prds/v0_3/gui-event-channel-inventory.md` (GR-016) | consumes | mechanism-descriptor channel substrate | GR-016 | decomp landed |
| `docs/prds/v0_3/engine-integration-norm.md` (GR-017) | references | `§3.1 op-execute` seam (FK-aware distance dispatch is op-execute, not a new seam) | GR-017 | landed |
| `docs/prds/v0_3/rigid-body-dynamics.md` (this session; new) | produces | `JointValue` enum, spring/damper joint extension hook | this PRD | both authored this session; rigid-body-dynamics consumes once it lands |
| `docs/prds/v0_3/modal-analysis.md` (this session; new) | independent | none direct; both consume FEA stack | n/a | both authored this session |
| `docs/prds/v0_3/trajectory-input-shaping.md` (this session; new) | produces | `sweep(m, joint, range, steps)` API extended for time-parameterised motion | this PRD | trajectory PRD consumes once it lands |
| `docs/prds/v0_3/compliant-joints-flexures.md` (this session; new) | produces | Joint surface API extended for spring_rate / damping; `JointValue` enum | this PRD | flexures PRD consumes once it lands |
| `docs/prds/kinematic-constraints.md` (v0.1 top-level) | retires | none | this PRD | task η formally adds §0 supersession line |
| `docs/prds/v0_2/kinematic-constraints.md` (v0.2 parent) | completes | none | this PRD | this PRD's decomposition landing = v0.2 done |

No reciprocal-ownership ambiguity: this PRD is the unambiguous owner of
the seven residual mechanisms. SIR-α is the unambiguous owner of
`StructureInstance` and ctor lowering. KGQ is the unambiguous owner of
the kernel-level distance scalar query.

## §11 — Boundary test sketch (cross-crate; facing both ways)

### §11.1 — Producer-side (`reify-stdlib`, `reify-eval`, `reify-kernel-occt`, GUI engine look outward at consumers)

| Scenario | Preconditions | Postconditions |
|---|---|---|
| **Singular snapshot emits typed diagnostic.** Construct a rank-deficient 4-bar at a known kinematic singularity; call `snapshot()`. | M-009 wrapper wired; M-011 flag plumbed. | `Snapshot.is_singular == true`; `EvalResult.diagnostics` contains exactly one `KinematicSingularity` entry with `loop_index = 0`. |
| **Over-constrained loop emits typed diagnostic.** Construct a 5-joint loop with `free_b.len() < effective_residual_dim`. | M-009 wrapper wired; effective-residual-dim cached. | `EvalResult.diagnostics` contains exactly one `KinematicOverconstrained`; `Snapshot.is_singular == true`. |
| **Multi-DOF closed chain converges.** Construct counter-mass-balance with a planar joint in the loop. | M-007 widened (`JointValue` enum); analytic-J for planar wired. | `Snapshot.bodies[N].world_transform` is finite; `is_singular == false`; iteration count < 50. |
| **FK-aware clearance under sweep.** Run `dock_pickup.ri` sweep over X∈[0..500mm, steps=50]. | M-019 wired (FK-aware OCCT). | `min_clearance` returns negative within the dock collision range and positive outside; transition is monotonic. |
| **Type-error on `bind(coupling, …)`.** Compile `bind(coupling_joint, 5mm)` where `coupling_joint` is a `Coupling`. | ζ first-class types wired; SIR-α landed. | Compiler emits `E_TypeMismatch: expected DrivingJoint, got Coupling`; today returns `Value::Undef`. |
| **GUI scrub of literal-bound joint.** Open `dock_pickup.ri` in GUI; scrub `x_axis` slider. | η backend descriptor extended; MechanismPanel patched. | Viewport `meshCount` stays constant; body world transforms update per slider value; debug MCP `viewport_state` reflects updated free_values. |

### §11.2 — Consumer-side (downstream PRDs and user-side `.ri` code look inward at the seam)

| Scenario | Preconditions | Postconditions |
|---|---|---|
| **Dynamics PRD reuses `JointValue`.** rigid-body-dynamics PRD's inverse-dynamics consumer reads `Snapshot.free_values: List<JointValue>` directly. | This PRD's γ widening landed. | Inverse-dynamics impl compiles against `&[JointValue]` shape with no shim. |
| **Trajectory PRD reuses sweep API.** trajectory-input-shaping PRD's `simulate_trajectory(m, profile)` calls into `sweep_grid` with time-parameterised dims. | This PRD's existing sweep API unchanged for trajectory use. | Trajectory PRD's tests pass without modification to sweep internals. |
| **Flexure PRD extends Joint surface.** compliant-joints-flexures PRD adds `Joint.spring_rate` and `Joint.damping` fields to Prismatic/Revolute. | This PRD's `JointValue` enum + Joint Map representation are stable. | Flexure PRD's Map-shape extension is purely additive; existing kinematic tests don't regress. |
| **LSP surfaces `KinematicSingularity` warning.** User's `.ri` mechanism is rank-deficient. LSP runs `reify check` in background. | This PRD's α wrapper wired. | LSP hover at the `sweep(...)` call site shows the `W_KinematicSingularity` diagnostic with loop joint pair named. |
| **MCP report_diagnostics includes kinematic types.** GUI dev-mode harness calls `report_diagnostics`. | α wrapper wired. | Response includes `KinematicSingularity` / `KinematicOverconstrained` / `KinematicUnderconstrained` entries when present. |

## §12 — Decomposition plan

Vertical-slice B+H decomposition. Phase 1 supplies the foundation (the
JointValue enum + diagnostic plumb-through); Phase 2–5 each ship one
residual mechanism; Phase 6 is the integration gate + companion edits;
Phase 7 is dogfood.

### Phase 1 — Foundation (`JointValue` enum + diagnostic plumb-through)

- **α-pre — Introduce `JointValue` enum + flatten/unflatten helpers.**
  - Crates: `reify-stdlib/src/loop_closure_value.rs` (NEW), tests in `reify-stdlib`.
  - Observable signal (intermediate): every existing `Vec<f64>`-typed chain test in `loop_closure.rs` and `loop_closure_solver.rs` switches to `Vec<JointValue>` and still passes.
  - Unlocks: α, γ.
  - Prereqs: none.

- **α — Wire `solve_loop_closure_with_diagnostics` into `snapshot()` and `sweep()`; add `is_singular` field; refine over-constrained pre-check to use effective residual dimension.**
  - Crates: `reify-stdlib/src/snapshot.rs`, `reify-stdlib/src/sweep.rs`, `reify-stdlib/src/loop_closure_solver.rs`, `reify-types/src/diagnostics.rs` (strip "not currently emitted" TODOs), `reify-eval/tests/kinematic_diagnostics_e2e.rs` (extend to assert via snapshot).
  - Observable signal: kinematic_diagnostics_e2e.rs adds a snapshot-routed assertion: `Snapshot.is_singular == true` AND `EvalResult.diagnostics` contains `KinematicSingularity` for a rank-deficient four-bar fixture. Bare `solve_loop_closure_with_diagnostics`-direct tests still pass.
  - Prereqs: α-pre.

### Phase 2 — Multi-DOF closed chains (γ widening)

- **γ — Widen `value_for_joint`, `joint_range_midpoint`, `chain_transform` to operate on `JointValue`. Wire planar/spherical/cylindrical chain participation. Add analytic-J for planar + spherical (δ folded into this task).**
  - Crates: `reify-stdlib/src/loop_closure.rs`, `reify-stdlib/src/loop_closure_solver.rs`, `reify-stdlib/src/joints.rs` (analytic-J replacements at :785, :800).
  - Observable signal: `crates/reify-eval/tests/kinematic_loop_closure_machinery.rs` adds a closed-chain planar-joint fixture (counter-mass-balance with planar joint in loop). Snapshot returns finite bodies; iteration count < 50; analytic-J path used (logged via tracing).
  - Prereqs: α-pre.

### Phase 3 — FK-aware OCCT (ε)

- **ε-occt — Add `distance_with_transform` / `interferes_with_transform` FFI in `reify-kernel-occt`.**
  - Crates: `reify-kernel-occt/src/queries.rs`, FFI bindings.
  - Observable signal (intermediate): unit test in `reify-kernel-occt/tests/` confirms distance under transform matches transform-then-distance (rigid-invariance pin).
  - Prereqs: none.

- **ε — Wire FK-aware dispatch in `reify-eval/src/geometry_ops.rs`; add `try_resolve_snapshot_body` helper.**
  - Crates: `reify-eval/src/geometry_ops.rs`, `examples/kinematic/dock_pickup.ri` (strip FIXME comment block), `reify-eval/tests/kinematic_examples_e2e.rs`.
  - Observable signal: `cargo test -p reify-eval --test kinematic_examples_e2e -- dock_pickup_clearance_sweep` runs `dock_pickup.ri` end-to-end; `min_clearance` returns expected pattern (positive outside dock, negative inside, monotonic transition). FIXME comments at the head of dock_pickup.ri removed.
  - Prereqs: ε-occt, KGQ Phase 2 `distance` dispatcher landed (cross-PRD dep).

### Phase 4 — First-class types (ζ, gated on SIR-α)

- **ζ — Add stdlib type declarations `kinematic.ri`; register `trait DrivingJoint`; retire `kinematic_query_result_type` per-name hooks.**
  - Crates: `reify-compiler/stdlib/kinematic.ri` (NEW), `reify-compiler/src/units.rs` (strip overrides), `reify-compiler/src/type_resolution.rs`, `reify-stdlib/src/sweep.rs` (remove `driving_joint_kind` hardcoded set), companion docs in `docs/reify-stdlib-reference.md`.
  - Observable signal: compiler error test `cargo test -p reify-compiler -- type_error_on_bind_coupling` passes — compiling `bind(coupling_joint, 5mm)` yields `E_TypeMismatch: expected DrivingJoint, got Coupling`. Today returns `Value::Undef`.
  - Prereqs: SIR-α (task 3540) — **cross-PRD `add_dependency` edge**.

### Phase 5 — GUI slider for literal-bound joints (η)

- **η-engine — Extend `JointBinding` enum, descriptor extractor, name-collision detection in engine.rs.**
  - Crates: `gui/src-tauri/src/types.rs`, `gui/src-tauri/src/engine.rs`, `gui/src-tauri/src/commands.rs`.
  - Observable signal (intermediate): Rust unit test confirms descriptor for `bind(j, 100mm)` produces `JointBinding::LiteralBound { scrubbable: true, ... }`.
  - Prereqs: none.

- **η — Frontend: MechanismPanel renders literal-bound sliders as scrubbable.**
  - Crates: `gui/src/panels/MechanismPanel.tsx`, `gui/src/stores/mechanismStore.ts`.
  - Observable signal: GUI debug-MCP harness opens `dock_pickup.ri`, scrubs `x_axis` slider, asserts `viewport_state.bodies` update with no `meshCount` regression. RAF coalescing budget honoured (frame time ≤ 200ms p95).
  - Prereqs: η-engine, GR-016 substrate (already landed).

### Phase 6 — Companion correction tasks + supersession

- **θ — Retire v0.1 top-level kinematic-constraints PRD (formal supersession).**
  - Files: `docs/prds/kinematic-constraints.md` — add `## §0 — Superseded` line pointing at this PRD; `docs/prds/v0_2/kinematic-constraints.md` — add completion-status section noting this PRD owns the residuals.
  - Companion gap-register edits: dispose C-37 (FK-ignoring interference) with this PRD's ε as resolution; same for the kinematic-singularity row.
  - Observable signal: `grep -rE '^Status: deferred' docs/prds/kinematic-constraints.md` returns the superseded marker; `git log -1 -- docs/prds/kinematic-constraints.md` shows the supersession commit.
  - Prereqs: α, γ, ε, ζ, η (the residuals are actually resolved before retiring v0.1).

### Phase 7 — Integration gate + dogfood

- **ι — Integration acceptance sweep.**
  - Add NEW fixture `examples/kinematic/four_bar_singular.ri` exercising α (singularity diagnostic). All three example fixtures pass `cargo test -p reify-eval --test kinematic_examples_e2e`.
  - GUI debug-MCP smoke harness asserts MechanismPanel slider scrub on dock_pickup (η).
  - Strip "v0.1 simplification" comments and FIXME blocks from `geometry_ops.rs`, `examples/kinematic/*.ri`, `mechanism.rs`, `snapshot.rs`. Search-and-confirm: `grep -rE 'v0\.1 simplification|FIXME.*kinemat' crates/ examples/` returns zero hits.
  - Observable signal: full CI green; the comment-strip grep returns empty.
  - Prereqs: all of α, γ, ε, ζ, η, θ.

### §12.1 — Dependency view

```
α-pre ──→ α ──┐
              ├──→ ι
γ ────────────┤
ε-occt ──→ ε ─┤
              │
SIR-α (3540) ─→ ζ ──┤
                    │
η-engine ──→ η ─────┤
                    │
α,γ,ε,ζ,η ──→ θ ───┘
```

8 tasks total in-batch (α-pre, α, γ, ε-occt, ε, ζ, η-engine, η, θ, ι) — 10
counting the foundation/engine splits — plus 1 cross-PRD edge to SIR-α task
3540 and 1 to KGQ Phase 2 distance-dispatcher task (resolve task id at
KGQ-decomp time).

## §13 — Out of scope for this PRD

Carried forward from v0.2 PRD plus new exclusions:

- **Inverse kinematics.** Future PRD if motivated.
- **Dynamics (kinetics).** Sibling PRD `docs/prds/v0_3/rigid-body-dynamics.md` (this session).
- **Modal analysis.** Sibling PRD `docs/prds/v0_3/modal-analysis.md` (this session).
- **Trajectory simulation / input shaping.** Sibling PRD `docs/prds/v0_3/trajectory-input-shaping.md` (this session).
- **Compliant mechanisms / flexures.** Sibling PRD `docs/prds/v0_3/compliant-joints-flexures.md` (this session).
- **Contact / collision response.** No corrected pose, no contact forces.
- **Cable / belt physics.** Tendons remain `Coupling`.
- **Path planning.** Separate concern; would belong in its own stdlib module.
- **Manifold-kernel parity for FK-aware queries.** Phase 5 of KGQ adds Manifold parity at the kernel-level scalar-query layer; FK wrapping is kernel-agnostic and will inherit Manifold-side support automatically when KGQ's Manifold dispatch arms land.
- **Scrub-as-edit for literal-bound joints.** Open question §14.1.
- **`Snapshot` referential identity.** v0.1 PRD M-024 — Value-model architectural gap, not kinematic-specific.
- **Centre-of-mass over volumetric solids.** v0.1 PRD M-014/M-015 — out of scope for this PRD; touched by KGQ Phase 4 + mass-properties follow-up.

## §14 — Open questions (surfaced but not decided in this session)

1. **Scrub-as-edit for literal-bound joints.** §8.3 keeps slider session-only.
   Real dogfood demand may surface a need to write scrubbed values back to the
   `.ri` source AST literal. **Suggested resolution:** defer to a focused
   "scrub-as-edit" PRD once dogfood data exists. Decide during printer-build
   dogfood phase.

2. **Manifold-kernel FK-aware parity.** Once KGQ Phase 5 (Manifold parity)
   lands, this PRD's ε path inherits Manifold support — but the
   `interferes_with_transform` FFI needs a Manifold sibling. **Suggested
   resolution:** ride the same KGQ Phase 5 task that adds Manifold capability
   flags. Decide during KGQ Phase 5 decomp.

3. **Spherical joint analytic-Jacobian sign convention.** §5.4 specifies
   `0.5 * E(q)` but the sign / column-order convention should match the
   existing quaternion log/exp in `crates/reify-geometry`. **Suggested
   resolution:** task γ resolves at impl time against existing test fixtures
   (`crates/reify-geometry/tests/transform_log_exp.rs`).

4. **Effective residual dimension caching invalidation.** §4.3 caches
   `residual_dim` on the `loop_closures` record at mechanism-build time.
   Mutating the mechanism (adding/removing bodies) invalidates the cache.
   **Suggested resolution:** Mechanism Map is immutable in practice (builder
   pattern returns a new Map per operation); cache invalidation is a non-issue
   for the v0.3 builder. Re-evaluate if mutability emerges.

5. **`__joint_<id>_v` synth-param name collision policy.** §8.1 emits
   `W_KinematicReservedParamName` if a user names a param `__joint_*`. Hard
   reservation might be preferable. **Suggested resolution:** keep warning-not-
   error for v0.3; promote to error if dogfood reveals confusion.

## §15 — Gap-register companion edits

To be applied as part of task θ:

- **C-37 (FK-ignoring interference).** Disposition update: "**fix-now resolved
  by `docs/prds/v0_3/kinematic-constraints-completion.md` Phase ε.**"
- **Kinematic-singularity surfacing row** (currently pointing at the
  SIGABRT-remapped task 3471): update Resolution mechanism to point at
  this PRD's α task; strip the stale 3471 cite.
- **kinematic-constraints-v02 finding mechanisms M-007, M-009, M-010, M-011,
  M-019, M-020, M-021, M-022, M-023:** mark state as RESOLVED-PENDING (or
  WIRED upon landing) with provenance pointer to this PRD's task IDs (assigned
  at decompose time).

End of PRD.
