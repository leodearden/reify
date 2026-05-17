# Compliant Joints + Flexures

Status: contract authored 2026-05-17 in interactive `/prd` session. Sibling
to `kinematic-constraints-completion.md`, `rigid-body-dynamics.md`,
`modal-analysis.md`, `trajectory-input-shaping.md`. Pending Leo approval
before queueing tasks.

## §0 — Purpose and supersession

The four sibling PRDs in this session ship rigid-body mechanism modelling.
Real ultra-performant 3D printers — and any precision-motion system —
depend critically on **flexures**: monolithic compliant joints that
replace bearings/pivots with deliberately-shaped thin elastic elements.
Flexures have zero friction (no Coulomb stick-slip), zero hysteresis
(infinite resolution), zero backlash, and zero particulate generation —
all attributes that bearings cannot match. They are how precision moves.

This PRD adds:

1. **Spring-damper joint extensions** — existing prismatic / revolute /
   etc. joints get optional `spring_rate` and `damping` fields. When set,
   those joints contribute restoring + dissipative forces in rigid-body-
   dynamics and additive stiffness in modal-analysis.
2. **Pseudo-rigid-body (PRB) flexure primitives** — Howell-style closed-
   form mapping from flexure geometry + material to equivalent spring-
   loaded joint. Covers ~10 standard flexure types from Howell 2001.
3. **Compound flexure stages** — parallelogram flexure (4-blade
   single-DOF stage), double-parallelogram (parasitic-error cancellation),
   cartwheel flexure (multi-blade revolute). Two more compound primitives;
   generic compound-flexure builder bookmarked separately.
4. **Stress check** at flexure-construction time — warn on yield stress
   exceedance at the declared range endpoints.

The PRD is **PRB-fidelity**, not FEA-fidelity. PRB models are accurate
for small-to-moderate deflections (≤5°) and are the industry-standard
design-time primitive. Large-deflection geometrically-nonlinear flexures
(beam theory for ≥10° deflection) are bookmarked at
`docs/prds/dynamics-fidelity-roadmap.md` §1.6.

No supersession; greenfield for the v0.3 line.

## §1 — Goal and user-observable surface

A Reify user can declare a flexure-joint as a thin-shape primitive
(beam, notch, hinge) parameterized by its geometry + material; the PRB
helper returns a spring-loaded `Revolute` or `Prismatic` joint that
plugs into the existing kinematic + dynamic stack. Concretely:

```reify
// examples/flexures/cantilever_beam_prb.ri
let steel = Steel_AISI_1045();

let pivot_flexure = prb_cantilever_beam(
    length:    20mm,
    width:     5mm,
    thickness: 0.5mm,
    material:  steel,
    pivot:     point3(0, 0, 0),
    axis:      Y_HAT,
);
// pivot_flexure : Revolute joint with:
//   - spring_rate = γ·E·I/L · neutral_zero (where γ=2.65 for cantilever)
//   - range automatically capped at yield stress
//   - max_stress accessor exposed

let m = mechanism()
    .body(frame_solid(), at: world)
    .body(rotor_solid(), at: pivot_flexure, parent: world)
    .build();

// Standard mechanism + sweep + interference all work — flexure is just
// a Revolute with spring/damping fields set.
let snap = snapshot(m, [bind(pivot_flexure, 1deg)]);
let stress_at_1deg = pivot_flexure.max_stress;
report("flexure stress at 1° = " ++ show(stress_at_1deg));
```

```reify
// examples/flexures/parallelogram_stage.ri
let stage = prb_parallelogram_flexure(
    length:          50mm,    // blade length
    width:           20mm,    // blade depth
    thickness:       1mm,
    blade_spacing:   30mm,    // distance between parallel blades
    material:        Aluminium_6061_T6(),
    motion_axis:     X_HAT,
);
// stage : Prismatic with composite spring_rate from 4 parallel blades.
// Compound flexure's parasitic-rotation error (the Roberts approximation
// error) reported in stage.parasitic_rotation_at_max_displacement.

let m = mechanism()
    .body(precision_optic(), at: world)
    .body(stage_carriage(), at: stage, parent: world)
    .build();
```

**CI-gate signals:**

- `cargo test -p reify-eval --test flexure_e2e` runs five fixtures:
  - `examples/flexures/cantilever_beam_prb.ri` — single-leaf cantilever.
    PRB equivalent k_θ matches Howell formula `γ·E·I/L` within 1% (γ=2.65,
    closed-form analytic ground truth).
  - `examples/flexures/notch_hinge_circular_prb.ri` — circular notch
    hinge. PRB k_θ matches Paros-Weisbord formula within 2%.
  - `examples/flexures/parallelogram_stage.ri` — 4-blade parallelogram
    flexure. Single-DOF linear stage; assert orthogonal-DOF stiffness
    ratio ≥ 1000:1; parasitic-rotation error < L/1000 at max displacement.
  - `examples/flexures/double_parallelogram.ri` — 2-stage doubled
    parallelogram. Assert parasitic-rotation error < L/100000 at max
    displacement (4 orders of magnitude better than single-stage due to
    symmetric cancellation).
  - `examples/flexures/yield_warning.ri` — flexure with declared range
    exceeding yield stress. `W_FlexureYielding` diagnostic emitted at
    PRB ctor time; ctor returns a valid flexure with `at_yield` flag.

- `examples/flexures/printer_z_compliant_mount.ri` — printer-build
  dogfood fixture; a real flexure-mounted Z-axis preload spring from
  the printer-build. Used as design-time validation of preload force +
  yield margin.

**Diagnostic signals:**

- `W_FlexureYielding` — max stress at declared range endpoint exceeds
  material yield. Reports actual stress + yield + safety factor.
- `W_FlexureFatigueCheckMissing` — flexure declared without a fatigue-
  cycle budget. Bookmarked fatigue PRD (`dynamics-fidelity-roadmap.md`
  §1.7) is referenced.
- `E_FlexureGeometryInvalid` — input geometry violates structural
  constraints (e.g. thickness ≥ length for a beam; aspect ratio out of
  PRB validity range).
- `W_FlexurePRBOutOfRange` — declared range exceeds the PRB
  approximation's validity bound (typically ±5° for beam flexures);
  warn that PRB accuracy degrades; suggest geometrically-nonlinear FEA
  (bookmarked).

## §2 — Scope

### §2.1 — Mechanism table

| # | Mechanism | State today | Owner |
|---|---|---|---|
| α | `Prismatic`, `Revolute`, etc. Joint structures get optional `spring_rate` and `damping` fields | NEW (extends kinematic-completion ζ) | this PRD |
| β | rigid-body-dynamics backward-pass extension: `τ_spring = -k·(θ - θ_neutral)` and `τ_damping = -c·θ̇` additive contribution | NEW (extends rigid-body-dynamics) | this PRD |
| γ | `FlexureCompliance` value type (stiffness matrix, max_stress accessor, parasitic-error metric, validity range) | NEW | this PRD |
| δ | PRB primitive ctors — 10 standard Howell 2001 primitives (see §2.2) | NEW | this PRD |
| ε | Compound flexure ctors — parallelogram, double-parallelogram, cartwheel | NEW | this PRD |
| ζ | Yield-stress check at PRB ctor time; max_stress / max_stress_at(θ) accessors; `W_FlexureYielding` emission | NEW | this PRD |
| η | modal-analysis stiffness-matrix assembly extension: spring-loaded joint contributes additive stiffness to K | NEW (extends modal-analysis) | this PRD |
| θ | Flexure-aware mass-properties for prb-constructed bodies (the flexure has its own mass distribution distinct from the rigid-body it joins) | NEW (extends rigid-body-dynamics β) | this PRD |

### §2.2 — PRB primitive set (Howell 2001 standard)

| Primitive | Stdlib fn | DOF | Reference |
|---|---|---|---|
| Cantilever beam flexure | `prb_cantilever_beam(...)` | Revolute (1) | Howell §5.2, γ=2.65 |
| Fixed-fixed beam flexure | `prb_fixed_fixed_beam(...)` | Prismatic (1, transverse) | Howell §5.3 |
| Circular notch flexure | `prb_notch_circular(...)` | Revolute (1) | Paros & Weisbord 1965 |
| Elliptical notch flexure | `prb_notch_elliptical(...)` | Revolute (1) | Smith et al. 1997 |
| Right-circular notch flexure | `prb_notch_right_circular(...)` | Revolute (1) | Paros & Weisbord 1965 (toroidal variant) |
| Living hinge (thin polymer) | `prb_living_hinge(...)` | Revolute (1) | Howell §5.7 |
| Cross-spring pivot | `prb_cross_spring_pivot(...)` | Revolute (1) | Haringx 1949 |
| Lamina-emergent torsion (LET) | `prb_let_joint(...)` | Revolute (1, multi-blade torsion) | Jacobsen et al. 2009 |
| Compliant prismatic blade | `prb_prismatic_blade(...)` | Prismatic (1) | Howell §6.2 |
| Two-axis flexure pivot | `prb_two_axis_pivot(...)` | Spherical (2) | Henein 2010 |

### §2.3 — Compound flexure primitives (this PRD)

| Compound | Stdlib fn | DOF | Composition |
|---|---|---|---|
| Parallelogram flexure | `prb_parallelogram_flexure(...)` | Prismatic (1) | 4 parallel cantilever beams |
| Double-parallelogram | `prb_double_parallelogram_flexure(...)` | Prismatic (1) | 2 parallelograms in series; parasitic-error-cancelling |
| Cartwheel flexure | `prb_cartwheel_flexure(...)` | Revolute (1) | N radial blades meeting at a pivot point |

### §2.4 — Out of scope (bookmarked)

- **Generic compound-flexure builder** (`compose_flexures([...])`).
  Bookmarked as `docs/prds/v0_4/compound-flexure-builder.md` (unauthored
  slot) per [[preferences_bookmark_task_pattern]]. Triggers: real
  printer-build dogfood reveals demand for compound flexures beyond the
  three primitives in §2.3 (butterfly flexure, Roberts approximation
  variants, multi-stage parallelogram chains, etc.).
- **Geometrically nonlinear flexure FEA** for large deflections.
  Bookmarked at `dynamics-fidelity-roadmap.md` §1.6.
- **Fatigue analysis (S-N).** Bookmarked at
  `dynamics-fidelity-roadmap.md` §1.7.
- **Buckling check for column-flexures.** Buckling-eigensolver v0.5 PRD
  already handles linear buckling; flexure-specific buckling
  (e.g. cross-axis buckling of parallelogram flexures) folds into that
  PRD's mode shapes.
- **Material-creep behaviour** in compliant joints. Out of scope; would
  fold into fatigue / wear PRD family.
- **Flexure-stress-distribution detailed FEA.** PRB is the design-time
  primitive; high-fidelity FEA validation is `solver-elastic`'s job.
  Cross-validation between PRB and FEA at decompose time confirms PRB
  accuracy; not a runtime check.

## §3 — Pre-conditions for activating

| Pre-condition | Owner | Status today | Gate phase |
|---|---|---|---|
| Joint structure_defs with `Option<...>` fields supported | kinematic-completion ζ (first-class types) | this session | hard prereq for α |
| `RotationalStiffness`, `RotationalDamping`, `TranslationalStiffness`, `TranslationalDamping` dimensioned types | reify-types or new stdlib `dimensions.ri` extension | likely needs new task | hard prereq for α |
| rigid-body-dynamics backward-pass extension hook | `rigid-body-dynamics.md` task ε (RNEA core) | this session | hard prereq for β |
| modal-analysis K assembly extension hook | `modal-analysis.md` task δ (mass-matrix assembly) | this session | hard prereq for η |
| Material structure_defs with `youngs_modulus`, `yield_stress` | shipped (materials_fea.ri) | landed | hard prereq for δ, ε |
| SIR-α `Value::StructureInstance` | structure-instance-runtime task 3540 | in-flight | hard prereq for γ, δ, ε |

## §4 — Contract: Joint surface API extension

### §4.1 — Optional spring/damping fields

```reify
structure def Revolute : DrivingJoint {
    param axis        : Vec3
    param range       : Range<Angle>
    param pivot       : Point3
    param spring_rate : Option<RotationalStiffness>  // NEW; N·m/rad
    param damping     : Option<RotationalDamping>    // NEW; N·m·s/rad
    param neutral     : Option<Angle>                // NEW; rest position; default 0
}

structure def Prismatic : DrivingJoint {
    param axis        : Vec3
    param range       : Range<Length>
    param spring_rate : Option<TranslationalStiffness>  // NEW; N/m
    param damping     : Option<TranslationalDamping>    // NEW; N·s/m
    param neutral     : Option<Length>                  // NEW; rest position; default range midpoint
}

// Cylindrical, Planar, Spherical get tuple-valued spring/damping shapes
// matching their JointValue shapes — deferred to Open Question §11.2;
// v0.3 ships spring-damper extensions for Prismatic + Revolute only.
```

When `spring_rate` is `None`, behaviour is unchanged from
kinematic-completion. When set, the joint contributes additional
forces and additive stiffness as documented in §5 and §6.

### §4.2 — `FlexureCompliance` accessor type

```reify
structure def FlexureCompliance {
    param effective_stiffness     : RotationalStiffness  // OR TranslationalStiffness
    param max_stress              : Pressure             // at range endpoint
    param max_stress_at_neutral   : Pressure             // zero unless preloaded
    param yield_margin            : Real                 // (yield - max_stress) / yield
    param parasitic_error         : Option<Length>       // for compound flexures; orthogonal-DOF parasitic motion
    param prb_validity_range      : Range<Angle>         // where PRB is accurate within ~5%
    param at_yield                : Bool                 // true if max_stress >= yield
}

// Accessor:
fn flexure_compliance(joint: Joint) -> FlexureCompliance
```

The PRB ctors return Joints with `spring_rate` and `damping` populated
AND a hidden side-channel `FlexureCompliance` record cached on the
joint. The accessor surfaces it for user inspection.

## §5 — Contract: PRB primitives

### §5.1 — Cantilever beam (canonical example)

```reify
fn prb_cantilever_beam(
    length:     Length,
    width:      Length,
    thickness:  Length,
    material:   ElasticMaterial,
    pivot:      Point3,
    axis:       Vec3,
    neutral_angle: Option<Angle>,  // default 0
) -> Revolute
```

Howell-formula computation (closed-form, no FEA call):

```
I        = width · thickness³ / 12       // second moment of area
gamma    = 2.65                          // PRB stiffness coefficient (Howell §5.2)
k_theta  = gamma · material.youngs_modulus · I / length

// Validity range from yield stress:
sigma_max_at_theta = material.youngs_modulus · (thickness/2) · theta / length
theta_yield        = material.yield_stress · length / (material.youngs_modulus · thickness/2)
// PRB accuracy degrades beyond ~5°:
theta_prb_limit    = 5deg
prb_validity       = (-min(theta_yield, theta_prb_limit), min(theta_yield, theta_prb_limit))
```

Returns a Revolute joint with `spring_rate = k_theta`, `range =
prb_validity`, the original `pivot` + `axis`, and `damping = None`
(damping is material- and shape-dependent and not part of the PRB
closed-form; users add it explicitly if needed).

The same closed-form pattern repeats for the other 9 PRB primitives.
Each has its own `gamma` coefficient + stress formula; references in
the §2.2 table.

### §5.2 — Notch flexure (Paros-Weisbord)

```reify
fn prb_notch_circular(
    notch_radius: Length,
    web_thickness: Length,   // ligament thickness at the notch root
    width:        Length,
    material:     ElasticMaterial,
    pivot:        Point3,
    axis:         Vec3,
) -> Revolute
```

Paros-Weisbord 1965 closed-form:

```
k_theta = (2 · E · b · t^(5/2)) / (9π · r^(1/2))
        = (2 · material.youngs_modulus · width · web_thickness^2.5)
          / (9 · pi · notch_radius^0.5)

sigma_max_at_theta = 4 · E · t · theta / (3π · (2r + t))
```

Variants (`elliptical`, `right-circular`) substitute different
geometry-dependent shape factors but follow the same closed-form
pattern.

### §5.3 — Stress check + diagnostic emission

At ctor time:

```
if max_stress > material.yield_stress:
    emit W_FlexureYielding(
        flexure_kind: "cantilever_beam",   // ...etc
        max_stress: max_stress,
        yield_stress: material.yield_stress,
        safety_factor: material.yield_stress / max_stress,
        range_at_yield: ...,    // suggested narrower range
    )
    // Ctor still returns a valid Joint; at_yield is set on the
    // FlexureCompliance record.
```

W_FlexureFatigueCheckMissing emits **once per session** as an
informational diagnostic (not per-flexure) the first time a PRB ctor is
called, citing the bookmarked fatigue PRD.

## §6 — Contract: Compound flexure primitives

### §6.1 — Parallelogram flexure

```reify
fn prb_parallelogram_flexure(
    length:        Length,    // blade length
    width:         Length,    // blade depth
    thickness:     Length,    // blade thickness (in motion direction)
    blade_spacing: Length,    // distance between parallel blades
    material:      ElasticMaterial,
    motion_axis:   Vec3,
    pivot:         Point3,
) -> Prismatic
```

Composition: four parallel cantilever beams; each contributes
`k_blade = γ_pp · E · b·t³/12 / L³` (with `γ_pp = 12` for fixed-guided
beam bending; differs from cantilever's 3-coefficient). Combined
stiffness `k_stage = 4 · k_blade`. Parasitic rotation (Roberts
approximation error) computed as `δ_rot = L · (1 - cos(δ/L))` where
δ is the transverse displacement.

### §6.2 — Double-parallelogram flexure

Two single-stage parallelogram flexures in mirror-symmetric series.
The mirror symmetry exactly cancels the first-order parasitic rotation
contribution, leaving a tiny higher-order term. Parasitic-rotation
error scales as `(δ/L)³` instead of `(δ/L)`, 4+ orders of magnitude
better at typical operating ranges.

### §6.3 — Cartwheel flexure

```reify
fn prb_cartwheel_flexure(
    blade_count:   Int,         // typically 3-8
    blade_length:  Length,
    blade_width:   Length,
    blade_thickness: Length,
    material:      ElasticMaterial,
    pivot:         Point3,
    axis:          Vec3,
) -> Revolute
```

N blades radiating from the pivot point; each contributes a cantilever
stiffness `k_blade`. Combined revolute stiffness `k_pivot = N · k_blade`.
Provides high transverse stiffness (resists off-axis loads) while
allowing pure rotation about the cartwheel center.

## §7 — Contract: Integration with rigid-body-dynamics and modal-analysis

### §7.1 — rigid-body-dynamics force balance

In the RNEA backward-pass at the joint i:

```
tau_total[i] = tau_actuator[i]                            // from §5.2 RNEA result
              + spring_force(joint[i], snapshot[i])       // NEW
              + damping_force(joint[i], snapshot[i], q_dot[i]) // NEW
```

where:

```
spring_force(joint, snap) =
    if joint.spring_rate is Some(k):
        -k * (snap.value - joint.neutral)
    else:
        0

damping_force(joint, snap, q_dot) =
    if joint.damping is Some(c):
        -c * q_dot
    else:
        0
```

For multi-DOF joints (cylindrical / planar / spherical), the spring +
damping shapes match the JointValue shape (deferred to Open Question
§11.2; v0.3 ships Prismatic + Revolute only).

### §7.2 — modal-analysis stiffness extension

In modal-analysis K assembly, each spring-loaded joint contributes
additive entries to the global stiffness matrix at the joint's DOF
indices:

```
K_joint[i,i] += k    // diagonal entry, joint i's DOF
```

Damping contributes to C analogously if not using Rayleigh damping
(if Rayleigh, the per-joint damping is folded into the per-mode
Rayleigh ratio).

This means a Part containing flexure-jointed bodies includes the
flexure-equivalent stiffness in its modal model. The first-mode
frequency of a flexure-suspended mass falls out naturally:
`ω = √(k_flexure / m_load)`.

### §7.3 — Mass properties for flexure-jointed bodies

PRB models the flexure as a massless joint; the body's MassProperties
covers the rigid mass attached. For accurate dynamics on systems where
the flexure mass is a significant fraction of the load mass (e.g. long-
blade parallelogram flexures with a small carriage), users can
explicitly add a virtual "flexure mass body" parented at the flexure's
midpoint with the flexure's computed mass.

A helper `flexure_self_mass(flexure_ctor_args)` returns the PRB's own
mass for this composition pattern. Not auto-injected — keeps the
mechanism kinematic structure transparent.

## §8 — Resolved design decisions

**(8.1) Howell 2001 standard primitive set (~10 primitives).** Selected
over rectangular-only because: (a) Howell's canonical reference is the
industry standard; (b) the ~10 primitives cover the dominant flexure-
type cardinality in precision-instrumentation literature; (c) each
primitive's closed-form is small, so the per-primitive code cost is
low; (d) bookmarking generic compound-flexure builder for v0.4 keeps
this PRD's scope bounded.

**(8.2) Extend existing Joint Map with optional spring_rate / damping
fields.** Selected over new SpringPrismatic / SpringRevolute kinds and
over Coupling-wrapper composition because: (a) smallest surface change;
(b) preserves existing kinematic test behaviour (default None = rigid);
(c) PRB ctors return a Revolute / Prismatic with these populated, which
plugs cleanly into the rest of the stack (no new joint kind dispatch
needed); (d) future multi-DOF spring-damping extensions are additive.

**(8.3) Warn-on-yield + expose accessor.** Selected over hard-error and
over silent because: (a) early design exploration benefits from seeing
"this almost yields" rather than being blocked; (b) the accessor is
always available for user-side strict checks; (c) the diagnostic
includes a suggested narrower-range so the user has actionable
feedback. Plus emit fatigue-check-missing once per session
(informational) referencing the bookmarked fatigue PRD.

**(8.4) Include parallelogram + double-parallelogram + cartwheel as
compound primitives.** Selected because: (a) parallelogram flexures
are the workhorse precision-linear-stage primitive; (b) double-
parallelogram is the parasitic-error-cancelling cousin used universally
in serious precision design; (c) cartwheel is the workhorse precision-
revolute pivot. Generic compound-flexure builder bookmarked separately
for users wanting bespoke compositions.

**(8.5) PRB-fidelity only in v0.3.** PRB is the design-time primitive
in real engineering practice; full geometrically-nonlinear FEA
(Cosserat rod / corotational beam) is bookmarked under
`dynamics-fidelity-roadmap.md` §1.6. PRB is accurate to ≤5° deflection
which covers the precision-positioning use case dominantly.

**(8.6) v0.3 ships Prismatic + Revolute spring-damper extension only.**
Cylindrical / planar / spherical multi-DOF spring-damping is deferred
to Open Question §11.2. Most flexures are 1-DOF; multi-DOF flexures
(e.g. spherical flexure pivot) are rare enough to bookmark.

**(8.7) `damping = None` default for PRB ctors.** Damping is material-
and shape-dependent and not part of the PRB closed-form; users add it
explicitly if needed for dynamics modelling. Avoids speculative damping
values masking real material data.

**(8.8) Flexure self-mass via opt-in helper, not auto-injected.**
Keeps the kinematic structure transparent. Users wanting accurate
flexure-mass dynamics (large-blade flexures) explicitly call
`flexure_self_mass(...)` and attach the virtual body.

## §9 — Cross-PRD relationship (G4)

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `docs/prds/v0_3/kinematic-constraints-completion.md` | extends | Joint structure_defs (adds optional spring_rate / damping fields) | this PRD | this session |
| `docs/prds/v0_3/rigid-body-dynamics.md` | extends | RNEA backward-pass force balance (additive spring + damping terms) | this PRD | this session |
| `docs/prds/v0_3/modal-analysis.md` | extends | K assembly (additive joint-stiffness contribution) | this PRD | this session |
| `docs/prds/v0_3/structure-instance-runtime.md` (SIR / GR-001) | consumes | `Value::StructureInstance` + ctor lowering for new structure_defs | SIR | queued (3540 in flight) |
| Material `Steel_AISI_1045()`, `Aluminium_6061_T6()`, etc. | consumes | `youngs_modulus`, `yield_stress` fields | structural-analysis-fea | landed |
| `docs/prds/v0_4/compound-flexure-builder.md` (bookmark) | produces | parallelogram + cartwheel composition surface; generic builder consumes | this PRD | bookmark only |
| `docs/prds/dynamics-fidelity-roadmap.md` §1.6 (nonlinear beam FEA) | produces | PRB validity bounds expose the FEA-fidelity-needed threshold | this PRD | roadmap entry only |
| `docs/prds/dynamics-fidelity-roadmap.md` §1.7 (fatigue) | produces | flexure stress + cycle counts feed S-N curve | this PRD | roadmap entry only |

This PRD **extends** three sibling PRDs in this session in additive
ways: it adds optional fields to existing structures and additive
contributions to existing force-balance / stiffness-matrix assembly
paths. No reciprocal-ownership ambiguity.

## §10 — Boundary test sketch (cross-crate; facing both ways)

### §10.1 — Producer-side

| Scenario | Preconditions | Postconditions |
|---|---|---|
| **Cantilever Howell-formula ground truth.** Steel cantilever L=20mm, b=5mm, h=0.5mm. | δ wired. | `prb_cantilever_beam(...)` returns Revolute with `spring_rate` matching analytic 2.65·E·I/L within 1%. |
| **Notch hinge Paros-Weisbord.** Circular notch r=1mm, web t=0.2mm, w=5mm steel. | δ wired. | `prb_notch_circular(...)` k_θ matches Paros-Weisbord formula within 2%. |
| **Parallelogram stiffness ratio.** Standard parallelogram, motion vs transverse stiffness. | ε wired. | Computed motion-stiffness / transverse-stiffness ratio ≥ 1000:1. |
| **Double-parallelogram parasitic cancellation.** Double-parallelogram at max displacement. | ε wired. | Parasitic-rotation error < L/100000 (versus L/1000 for single-stage). |
| **Yield warning emission.** Cantilever with thickness=0.05mm forced to 10° range. | ζ wired. | `W_FlexureYielding` emitted; ctor returns valid joint with `at_yield = true`. |
| **PRB validity range degradation.** Cantilever forced to range = ±10°. | ζ wired. | `W_FlexurePRBOutOfRange` emitted citing 5° accuracy bound; suggested nonlinear-FEA path referenced. |
| **rigid-body-dynamics spring contribution.** 1-DOF spring-loaded pendulum, inverse dynamics at θ=30°. | α, β wired. | Inverse-dynamics output includes `-k·(30° - neutral)` additive term. |
| **modal-analysis flexure-resonance.** Block of mass m suspended on a Howell cantilever, modal_analysis. | α, η wired. | First-mode frequency matches `(1/2π)·√(k/m)` within 2%. |

### §10.2 — Consumer-side

| Scenario | Preconditions | Postconditions |
|---|---|---|
| **trajectory-input-shaping with flexure-mounted toolhead.** Mechanism uses prb-cartwheel-pivot mount; input-shaping algorithm reads modal_result; computes shaped trajectory. | All phases wired; trajectory PRD consumes. | trajectory PRD's tests pass against the flexure-jointed mechanism. |
| **GUI viewport shows flexure compliance overlay.** GUI panel reads flexure_compliance() for joints in the mechanism; surfaces max_stress / yield_margin. | γ wired; GR-016 channel. | GUI shows live yield margin per flexure; turns red when at_yield. |
| **Cross-validation against `solver-elastic` FEA.** Real high-aspect-ratio cantilever; compute PRB k_θ; mesh + FEA cantilever in `solver-elastic`; compare deflection-per-applied-torque. | δ wired; solver-elastic shipped. | PRB result within 5% of FEA result for L/h ≥ 20; PRB validity bound documents this. (Soft test; not a CI gate.) |

## §11 — Decomposition plan

Vertical-slice B+H. Phase 1 foundation (Joint surface extension +
FlexureCompliance); Phase 2 single-flexure PRB primitives; Phase 3
compound flexures; Phase 4 force-balance integration; Phase 5
stress-check diagnostics; Phase 6 dogfood + bookmarks.

### Phase 1 — Foundation

- **α — Joint structure_def extensions (optional spring_rate, damping,
  neutral fields on Prismatic / Revolute) + `RotationalStiffness` /
  `TranslationalStiffness` / `RotationalDamping` / `TranslationalDamping`
  dimensioned types.**
  - Crates: `reify-types/src/dimensions.rs` (extend),
    `reify-compiler/stdlib/kinematic.ri` (extend per
    kinematic-completion ζ).
  - Observable signal (intermediate): existing kinematic tests still
    pass (None default = no behaviour change); new test confirms
    `Revolute(... spring_rate: 1 N·m/rad, ...)` ctor accepts the field.
  - Prereqs: kinematic-completion ζ (first-class types) + SIR-α 3540.

- **β — `FlexureCompliance` structure_def + accessor stdlib fn.**
  - Crates: `reify-compiler/stdlib/flexures.ri` (NEW).
  - Observable signal (intermediate): compile test on template; ctor
    constraints.
  - Prereqs: SIR-α 3540.

### Phase 2 — Single-flexure PRB primitives

- **γ — Cantilever beam + fixed-fixed beam PRB ctors.**
  - Crates: `reify-stdlib/src/flexures/beam.rs` (NEW), unit tests
    pinning analytic Howell formulas.
  - Observable signal: `examples/flexures/cantilever_beam_prb.ri`
    runs end-to-end; k_θ within 1% of analytic.
  - Prereqs: α, β.

- **δ — Notch flexures (circular + elliptical + right-circular).**
  - Crates: `reify-stdlib/src/flexures/notch.rs` (NEW).
  - Observable signal: `examples/flexures/notch_hinge_circular_prb.ri`
    runs end-to-end; k_θ within 2% of Paros-Weisbord.
  - Prereqs: α, β.

- **ε — Living hinge + cross-spring pivot + LET joint.**
  - Crates: `reify-stdlib/src/flexures/hinge.rs` (NEW).
  - Observable signal: smoke tests pin each ctor's analytic k_θ.
  - Prereqs: α, β.

- **ζ — Compliant prismatic blade + two-axis flexure pivot.**
  - Crates: `reify-stdlib/src/flexures/prismatic.rs` (NEW).
  - Observable signal: smoke tests pin each ctor's analytic
    stiffness.
  - Prereqs: α, β.

### Phase 3 — Compound flexures

- **η — Parallelogram + double-parallelogram flexures.**
  - Crates: `reify-stdlib/src/flexures/compound.rs` (NEW), parasitic-
    error computation.
  - Observable signal: `examples/flexures/parallelogram_stage.ri` and
    `examples/flexures/double_parallelogram.ri` pass acceptance.
  - Prereqs: γ.

- **θ — Cartwheel flexure.**
  - Crates: `reify-stdlib/src/flexures/compound.rs` (extend).
  - Observable signal: smoke test pins cartwheel k_θ for N=4 blades.
  - Prereqs: γ.

### Phase 4 — Force-balance integration

- **ι — rigid-body-dynamics backward-pass extension (spring + damping
  contributions).**
  - Crates: `reify-stdlib/src/dynamics/rnea.rs` (extend; should be
    additive to the existing RNEA loop).
  - Observable signal: spring-pendulum inverse-dynamics unit test
    confirms additive contribution.
  - Prereqs: α, rigid-body-dynamics ε.

- **κ — modal-analysis K-assembly extension (additive joint-stiffness).**
  - Crates: `reify-solver-elastic/src/joint_stiffness.rs` (NEW) or
    extension to existing K-assembly path.
  - Observable signal: mass-on-cantilever modal frequency matches
    `√(k/m)/(2π)` within 2%.
  - Prereqs: α, modal-analysis δ.

### Phase 5 — Stress check + diagnostics

- **λ — Stress-check at PRB ctor + `W_FlexureYielding` /
  `W_FlexurePRBOutOfRange` / `W_FlexureFatigueCheckMissing` diagnostic
  emission; `FlexureCompliance` populated.**
  - Crates: each PRB ctor in `reify-stdlib/src/flexures/*.rs` (extend);
    `reify-types/src/diagnostics.rs` (new variants).
  - Observable signal: `examples/flexures/yield_warning.ri` runs;
    diagnostic emitted; ctor returns at_yield-flagged joint.
  - Prereqs: γ, δ, ε, ζ, η, θ.

### Phase 6 — Dogfood + bookmarks

- **μ — Printer-flexure dogfood `.ri`.**
  - Files: `examples/flexures/printer_z_compliant_mount.ri` (NEW).
  - Observable signal: example runs; flexure-compliance report printed.
  - Prereqs: λ, ι, κ.

- **ν — File bookmark tasks.**
  - Generic compound-flexure builder PRD slot (v0.4); fatigue PRD
    slot; nonlinear-beam-FEA PRD slot — all per
    [[preferences_bookmark_task_pattern]].
  - Prereqs: none.

### §11.1 — Dependency view

```
α (Joint ext)
β (FlexureCompliance)
   │
   ├─→ γ (cantilever/fixed-fixed)
   ├─→ δ (notch flexures)
   ├─→ ε (living hinge / cross-spring / LET)
   ├─→ ζ (compliant prismatic / 2-axis pivot)
   │       │
   │       ▼
   ├──→ η (parallelogram / dbl-parallelogram)
   │       │
   │       ▼
   ├──→ θ (cartwheel)
   │
   ├─→ ι (RBD backward-pass extension)
   │       └─ depends on rigid-body-dynamics ε
   │
   └─→ κ (modal-analysis K extension)
           └─ depends on modal-analysis δ

λ (diagnostics) depends on γ, δ, ε, ζ, η, θ.
μ (dogfood) depends on λ, ι, κ.
ν (bookmarks) independent.
```

13 in-batch tasks + 3 bookmarks + 4 cross-PRD edges (kinematic-
completion ζ, rigid-body-dynamics ε, modal-analysis δ, SIR-α 3540).

## §12 — Out of scope (see §2.4 also)

- Multi-DOF joint spring/damping extension (cylindrical / planar /
  spherical). Bookmarked at Open Question §11.2.
- Geometrically nonlinear flexure beams (≥10° deflection).
- Fatigue / wear / creep.
- Material temperature dependence.
- Flexure-stress-distribution detailed FEA.

## §13 — Open questions (surfaced but not decided in this session)

1. **Multi-DOF joint spring/damping.** §4.1 ships Prismatic + Revolute
   spring-damper extensions only. Cylindrical / planar / spherical
   multi-DOF flexures (e.g. spherical flexure pivot) are rare in
   practice but exist. **Suggested resolution:** ship Prismatic +
   Revolute in v0.3; add multi-DOF in v0.4 if dogfood demands.
   Multi-DOF spring tensor shape matches JointValue (3×3 for spherical,
   etc.).

2. **Damping default for PRB ctors.** §5.1 picks `damping = None`.
   Material loss factor (e.g. `ζ = 0.001` for steel, `0.005` for
   aluminium, `0.05` for ABS) could populate a small Rayleigh-style
   damping by default. **Suggested resolution:** None default in v0.3;
   add a `damping_estimate(material, joint_geometry)` helper as
   optional sugar in v0.4.

3. **Neutral position semantics for Prismatic.** §4.1 sets default
   `neutral = range midpoint` for Prismatic. For some flexures (e.g.
   pre-loaded blade springs) the neutral position is offset from
   geometric centre. **Suggested resolution:** v0.3 default is range
   midpoint; users supply explicit `neutral:` when preloaded. Document
   the convention.

4. **Stress at compound flexure interfaces.** Single-flexure stress
   formulas are well-defined; compound flexures (parallelogram etc.)
   have stress concentrations at blade-to-rigid-block transitions.
   **Suggested resolution:** PRB stress is "average bending stress in
   the blade"; not stress-concentration-aware. Document; bookmark
   detailed-FEA-validation entry under `dynamics-fidelity-roadmap.md`
   §3.5.

5. **PRB validity range hard-cap.** §5.1 caps validity at 5°. Some
   sources (Howell) allow up to 10° with reduced accuracy. **Suggested
   resolution:** 5° hard-cap in v0.3 (conservative); user can extend
   via Open Question §11.5 escape hatch when needed.

## §14 — Gap-register companion edits

Adds "compliant joints + flexures" as a new mechanism cluster.
Companion task adds a row pointing at this PRD.

End of PRD.
