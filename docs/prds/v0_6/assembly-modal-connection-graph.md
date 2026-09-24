# Assembly modal over the connection graph: whole-machine resonance from joints, ports and rigid bodies

**Milestone:** v0_6 · **Status:** active · **Date:** 2026-09-01 · **Approach:** B + H (contract + two-way boundary tests; FEA + ComputeNode dispatch are G5 load-bearing seams)

**Code anchors** verified against main `a153350b07` (2026-09-01). Main moves fast — cite-by-symbol; re-locate lines at implementation time.

**Provenance:** interactive design session with Leo, 2026-08-31 → 2026-09-01
(`design-reify-3830-1055491`), seeded by the Part-investigation census and the #3830 bookmark
trigger ("per-Part modal dogfood reveals cross-body modes are the actual bottleneck on a real
printer" — fired by Leo's explicit request, 2026-08-31). Every design decision in §6 was ruled by
Leo in that session and is recorded here, not re-opened. Substrate probes run at authoring time
(G3): four-agent sweep, 2026-09-01 — connect/ports (fixtures under `/tmp/prd-gate-fixtures/`,
re-derived as committed fixtures at decompose), mass-properties, realization channel,
eigensolve/assembly. Findings are folded into §2/§5 as measured claims.

**Successor context:** this is PRD 1 of a pair. `docs/prds/v0_6/flexible-assembly-modal.md` (PRD 2)
extends the same surface and result contract to real-geometry bodies (single-part, mixed-fidelity
assemblies, reified-joint continuum surrogates). This PRD owns the surface and result contracts;
PRD 2 extends value sources into them and must not fork either.

## 1. Goal

A `.ri` author models the resonance behaviour of a **whole multi-part machine** — rigid bodies
connected by compliant joints at ports — and gets one honest `ModalResult`: cross-body mode
frequencies with per-direction effective mass, per-body energy shares, and quasi-rigid
classification, consumable by design constraints.

The capability statement (ruled 2026-08-31): *modal analysis of a design at its widest — one entry
point, one result model, reading the single declared connectivity representation (the joint/port
connection graph), with per-entity fidelity as the only knob.* Craig-Bampton CMS — the #3830
bookmark's framing — is a numerical **reduction strategy under this surface**, not the capability;
it stays bookmarked (§10) until a measured scale wall or per-component-caching pull triggers it.

Consumer (G1): `prj/printer_v01/printer.ri`. Today its only cross-body answer is a hand-written
2-DOF lumped chain (belts→head→gantry, quartic solved algebraically in `GantryFea`) plus rigid-body
`f1 = sqrt(k_axis/m)/2π` cells, gated against `env.drive_f1_min` (40 Hz). After this PRD, the
printer declares its compliant-interface graph — gantry on air-bearing radial stiffness, carriages
on tendon-drive springs (`k_axis` cells already computed from rope EA), frame on its feet — and
constrains `lowest mode with effective mass_Y ≥ 5% clears drive_f1_min`. Cross-body modes the
current model cannot see (gantry rocking on bearing compliance, head-on-drive coupling in 3D with
real inertia) become visible and constrainable. The committed stand-in consumer is an
`examples/best_practices/` whole-machine exemplar (leaf θ), printer-shaped by construction.

Engine-integration seam (G1 sub-check): every runtime mechanism here rides the existing §3.4
ComputeNode dispatch of `engine-integration-norm.md` (the modal trampoline family in
`crates/reify-eval/src/modal_ops.rs`), plus the §3.2 realization-kind dispatch for the
mass-properties demand. No new seam.

## 2. Background — measured state (2026-08-31 session census)

- **`mechanism_modal` is a scalar-chain degenerate case.** `solve_mechanism_modal_trampoline`
  assembles one scalar generalized coordinate per spanning-tree body: strictly diagonal K/M,
  `K[i,i]` = the inbound joint's `spring_rate` (translational only — a rotational PRB flexure is
  rejected with `W_MechanismModalRotationalDOF` and treated rigid), `M[i,i]` = scalar body mass,
  closed chains refused with `E_MechanismModalClosedChain`. No spatial structure enters anywhere;
  no mode shapes; zero participation (the #7012 fake-value family, allowlisted in
  `result-field-vacuity-closure.md` §2.3). Leo, 2026-08-31: the closed-chain refusal and the
  printer's joint-lessness are **maturity artifacts, not design intent** — the printer is the user
  need for loops.
- **printer.ri declares zero joints.** Composition is static `sub … at transform3(…)` placement;
  the cross-body ringing answer is hand-assembled at template level. Its compliant interfaces are
  already first-class *value cells* (tendon `k_a1..k_b2 = 4·EA/loop_len`, `k_axis_min`, FEA-derived
  `k_cant`) with nothing structural to attach to.
- **The joint vocabulary exists and carries stiffness.** `crates/reify-compiler/stdlib/kinematic.ri`:
  `Revolute`/`Prismatic` carry `spring_rate : Option<…Stiffness>`, `damping`, `neutral`; the 13
  `prb_*` flexure builtins return joint values populating them. `Cylindrical`/`Planar`/`Spherical`/
  `Fixed` carry axis only. What the vocabulary lacks for structural dynamics is **compliance on the
  constrained DOFs** — an air bearing is a prismatic joint with finite radial/tilt stiffness. This
  is the standard ideal-joint-vs-bushing distinction, added as one extension (C2), not a second
  vocabulary.
- **The port substrate is landed and the connect graph is enumerable** (ports-breadth
  #4254–#4260 done; probed 2026-09-01). `Port` → `LocatedPort { frame : Frame3 }` →
  `RegionPort { region : Geometry }`; `MechanicalPort : LocatedPort`; guide-port hierarchy with
  `degrees_of_freedom == 1` constraints. A structure-typed connector with params
  (`connect p1 -> p2 : MyJoint { k = 100.0 }`) checks clean today and compiles to a **real
  synthetic sub-component** (`compile_connection` pushes `__connector_N` with the connector's
  args — an evaluable instance in the namespace), and `CompiledConnection`s are carried through
  to `EvaluationGraph.connections` at eval/build time — nothing reads them structurally yet, but
  the enumeration channel exists (leaf α wires it, does not invent it). Measured gaps that are
  leaf scope, not fictions: **nested port paths fail** (`a.b.port` → "invalid port reference";
  `resolve_port_name` in `connect.rs` matches one `MemberAccess` level only — α extends it, a
  printer-depth assembly needs it); the `sub xs[i in 0..N] = …` construction-arm indexer is a
  documented no-op owned by #5482 (the `List<T>` collection arm + indexing works and is what this
  PRD relies on).
- **Two measured silent-accept warts, adjacent — noted and filed, not depended on** (probed
  2026-09-01): port-param default expressions skip type checking entirely
  (`param region : Geometry = 5mm` inside a `port` block passes `reify check` with zero
  diagnostics; the same mismatch on a plain param at least warns), and connect directionality is
  enforced only for bare ports — every dotted sub-port connection skips direction checking
  (In→In passes silently). Both belong to the ctor/param-conformance and vacuity gate families
  (`struct-ctor-field-type-conformance.md`, `result-field-vacuity-closure.md` regime); this PRD's
  own loudness does not route through either hole (C1/C6 validate joint edges at graph-build
  time with their own diagnostics). Filed as found-during tasks at decompose.
- **Eigensolve substrate is ready; three assembly pieces are absent** (probed 2026-09-01).
  `solve_eigen_dense`/`solve_eigen_shift_invert` (`crates/reify-solver-elastic/src/eigensolve.rs`)
  take arbitrary square symmetric sparse (K, M) pairs — no mesh knowledge baked in — plus
  matrix-free `StiffnessOp`/`MetricOp` traits as a documented composition seam. Absent, and
  therefore leaf δ's honest scope: a 6-DOF frame-to-frame spring/coupling element
  (`add_joint_stiffness` in `joint_stiffness.rs` is scalar-diagonal-only), rigid-body 6×6 block
  assembly into a (K, M) pair, and constraint handling that touches M
  (`mpc.rs::apply_mpc_row_elimination` is K/f-only; ideal-rigid edges need a congruence
  reduction Tᵀ(K,M)T, which does not exist today). Scale datum: dense QZ at 864 DOF ≈ 25 s
  (`modal_benchmarks.rs`); this PRD's 6N problems (N ≲ 20 bodies → ≤ 120 DOF) sit far below that.
- **Full inertia-from-solid EXISTS in production** (probed 2026-09-01): `body_mass_props(solid,
  density?)` is a `.ri`-callable builtin (`eval_body_mass_props_core`,
  `crates/reify-eval/src/dynamics_ops.rs`) issuing real `Volume`/`CenterOfMass`/`InertiaTensor`
  kernel queries (genuine integrals in both Manifold and OCCT kernels), producing a validated
  `MassProperties {mass, com, inertia 3×3, origin}`; `derive_mechanism_mass_props` already
  auto-derives it for mechanism bodies at build time; `SpatialInertia6::from_mass_com_inertia`
  (`crates/reify-stdlib/src/dynamics/spatial.rs`) supplies the 6×6 spatial-inertia math the RNEA
  path already consumes. Density-absent is already loud (`E_DynamicsNoDensity`). The modal gap is
  narrow: `assemble_mechanism_km` reads the scalar mass and discards com/inertia.
- **The realization channel is the geometry path — N-ary already, pose-blind today** (probed
  2026-09-01). `Engine::build_compute_realization_inputs` scans **all** geometry args
  (deduped, arg-ordered, 1:1 with handles); `realization_indices_where` demands per-arg — N
  bodies demand correctly with no plumbing change. Two measured gaps become leaf scope: (i)
  consumers key by first-usable-type (`realized_solver_mesh_with_handle` `find_map`s), so N≥2
  same-type artifacts need a positional/keyed consumption contract; (ii) **placement never
  reaches realization artifacts** — `ApplyTransform`/`compose_pose_chain` run only in the
  Phase-B export walk and never write back to `realization_handles`, so every realized artifact
  is local-frame. Leaf β owns the pose seam: thread the pose chain to the consumer (or bake at
  store — tactical), so rigid-body data reaches assembly in the assembly frame. Modal
  `@optimized` targets are simply not in the `register_volume_mesh_demand` registry — wiring,
  not architecture. All four modal trampolines take `_realization_inputs` underscore-ignored
  today.
- **Known landmines routed around, not depended on:** #6583 (cross-sub geometry reads reach the
  kernel UNPOSED — value-level geometry never crosses a sub boundary in this design), #6592 (ctor
  args don't drive sub subtrees — a G6 hazard only when the analysis-bearing structure is
  instantiated with overrides; the root-evaluated `Printer` runs on defaults), the template-level
  `@optimized` dispatch constraint (printer.ri's pinned-literal relay pattern; pre-existing).

## 3. The fidelity ladder (north star, shared with PRD 2)

| Rung | Bodies | Joints | Delivered by |
|---|---|---|---|
| 0 (today) | rigid, scalar DOF | scalar spring, tree only | `mechanism_modal` as landed |
| **1 (this PRD)** | **rigid, 6 DOF, real inertia** | **6-DOF port-to-port compliance, loops, ground** | leaves α–κ |
| 2 | one flexible body, real geometry | supports (BC form) | PRD 2 phase A |
| 3 | mixed rigid/flexible | lumped joints attach at port regions; bonded groups merge | PRD 2 phase B |
| 4 | flexible | reified joint entities: continuum surrogates (bonded ties + anisotropic laws) | PRD 2 phase C |
| bookmark | any | CMS reduction under the same surface; CFD-calibrated surrogates | §10 |

The **port is the invariant across the ladder**: each joint side binds to a declared port
(`LocatedPort` frame now; `RegionPort` region consumed from rung 3). Compliance is defined
port-to-port identically at every rung; only the port's *realization* changes (rigid body: frame
transform; meshed body: patch reduction / bonded mating face). Ruled 2026-09-01.

## 4. Sketch of approach

**Declaration surface — joints as connectors on port-pair edges.** Structures declare mechanical
ports (`port bore : LinearGuidePort { frame = Frame3(…) }`-class declarations, refining the
landed LocatedPort hierarchy). The assembly declares connections whose
connector is a joint value:

```
connect carriage.bearing_a.bore -> xrail_upper.guide : <joint connector>   // spelling: §12 Q4
```

The graph is naturally loopy (connections are edges, not a spanning tree); bodies are inferred as
the port-owning sub instances; grounding is a connection to the world sentinel (C1). The
`mechanism()`/`body()` builder lowers to the same graph IR (or is deprecated — Open Q).

**Geometry channel — identity through the value graph, geometry through realization.** Value-level
references are identity-only. The modal ComputeNode demands, per rigid body, its MassProperties
through the realization pipeline and consumes them via `realization_inputs`, composed with the
body's pose chain so mass/COM/inertia reach assembly in the assembly frame (the pose seam is leaf
β's deliverable — probed: placement does not reach realization artifacts today, §2).
`point_mass(m)` remains the geometry-free escape hatch. #6583 never enters the path.

**Assembly + eigensolve.** 6 DOF per body; each connection contributes either constraint DOFs
(ideal-rigid directions — eliminated) or spring stiffness (compliant directions, including
constrained-DOF compliance), acting between the two port frames with rigid offset arms to each
body's COM frame. Grounded connections constrain/spring against world. The assembled (K, M) —
dense at 6N scale — goes to the existing eigensolve entry points unchanged.

**Result surface.** One `ModalResult` for the whole ladder. Per mode: frequency, damping ratio,
6-component effective-mass fractions, per-body strain/kinetic energy shares, quasi-rigid
classification. Per-body identity via `GeometryHandleRef` join keys (the ratified `.part`
disposition — this PRD's leaves are the live owners the vacuity PRD's allowlist entries require).
Query readers select modes by physics (`lowest mode above a floor with effective mass ≥ x in
direction d`), replacing index-based `first_frequency` idiom at assembly scale.

## 5. Pre-conditions (verified at authoring, 2026-09-01)

- ports-breadth #4254–#4260 **done**: `LocatedPort`/`RegionPort`/`Frame3`/guide-port hierarchy landed.
- kinematic surface (`kinematic.ri` joint structures + `prb_*`) landed via
  kinematic-constraints-completion (decomposition landed 2026-07-06) + flexure chain (#4271 done).
- Eigensolve entry points (`solve_eigen_dense`/`solve_eigen_shift_invert`) landed, sparse-capable.
- Realization demand/execute pattern landed (#4743) and N-ary by construction (§2); the pose
  thread and the N≥2 consumption-keying contract are leaf β/α deliverables, not assumptions.
- Mass-properties substrate landed and probed green (§2): `body_mass_props` builtin,
  kernel inertia integrals both kernels, `SpatialInertia6`, `derive_mechanism_mass_props`,
  `E_DynamicsNoDensity` loudness. Leaf β adds only pose composition + the demand wiring.
- Connect substrate probed green for the load-bearing forms (structure-typed connector with
  params; sub-port endpoints incl. `List<T>` collection indexing; `EvaluationGraph.connections`
  carried to eval/build) — §2. Nested port paths (α scope) and the two silent-accept warts are
  measured, named, and dispositioned in §2.
- NOT preconditions (explicitly): #6660 (gmsh linking — no meshing in this PRD), the selector
  migration #5312/#5313 (regions unconsumed until PRD 2 rung 3), damped-modal #6877–#6886
  (orthogonal; B4-class byte-identity asserted against its fixtures).

## 6. Resolved design decisions (Leo, 2026-08-31 → 2026-09-01)

1. **Capability ≠ implementation; CB is a reduction bookmark.** The spec'd capability is assembly
   modal over the connection graph; direct assembly is the v1 numerics; CMS slots under the same
   surface later (its interface DOFs are exactly the port attachment frames). Supersedes #3830's
   framing.
2. **One unified surface — no parallel "interface" vocabulary.** The joint/port graph is the single
   connectivity representation for kinematics AND structural dynamics. The closed-chain refusal and
   translational-only lumped model are maturity artifacts to be removed, not respected.
3. **Constrained-DOF compliance extends the joint vocabulary** (ideal-rigid default, byte-identical
   kinematic reading). One extension, not a second joint family.
4. **Ports are the attachment invariant across all fidelity rungs** (frame now, region from PRD 2
   rung 3). Joint compliance is defined port-to-port at every rung.
5. **Reified joints are the continuum end-state** (PRD 2 phase C): every lumped joint is the
   calibrated reduction of a physical entity with real geometry; at mesh level the spring element
   disappears — coupling is bonded interfaces, compliance is constitutive. Recorded here because it
   constrains this PRD: nothing in the lumped surface may preclude the same declared graph being
   consumed at continuum fidelity.
6. **Geometry channel is realization-level.** Identity flows through values; posed
   geometry/mass-properties flow through realization demands. Never value-level cross-sub geometry.
7. **One `ModalResult` across the ladder**; effective mass + per-body energy + quasi-rigid class
   are the classification axes; selection readers are part of the capability, not sugar. The
   mechanism-modal fake-value family (#7012) is superseded on the new path by honest population;
   the legacy lumped path's disposition remains #7012's ruling.
8. **`.part` is a join key on `GeometryHandleRef`** (ratified in the Part-investigation session,
   recorded in `result-field-vacuity-closure.md` §2.1). This PRD's result-surface leaf and PRD 2's
   single-body leaf are the live allowlist owners.
9. **Damping in this PRD: descriptor-based only** (`NoDamping`/`RayleighDamping` unchanged,
   byte-identical). Joint dashpot params and MSE composition arrive with PRD 2's machinery (§10;
   bookmark #6861 names the joint-damping landing path).
10. **Connector-form ergonomics are deferred tactically** (Leo, 2026-09-01): settle by working 2–3
    use cases at leaf-α time; the graph IR, not the spelling, is the contract.

## 7. Contract (H)

### C1 — graph extraction

From an evaluated assembly instance: **bodies** = the set of sub instances owning a connected
mechanical port (plus explicitly enrolled point masses); **edges** = connections whose connector is
a joint value; **ground** = edges to the world sentinel. The graph is a multigraph (parallel edges
legal — two bearings between the same pair), cycles legal. Each edge carries: the two port frames
(in their owners' local frames), the joint kind, its free-DOF axes/ranges (ignored beyond axis),
`spring_rate`/`neutral` on free DOFs, constrained-DOF compliance (C2). A connection whose
connector is not a joint value is not a graph edge (signal-flow connections coexist untouched).
Extraction mechanism (probed): the connector is already a synthetic sub-component instance
(`__connector_N`) and `EvaluationGraph.connections` carries the `CompiledConnection` list to
eval/build — α reads these, extending `resolve_port_name` to nested port paths (measured
one-level limit) and adding the world/ground endpoint form.

### C2 — joint compliance semantics

Per joint kind, DOFs partition into free and constrained. **Free DOF:** `spring_rate` present →
spring at that DOF about `neutral`; absent → genuinely free (contributes a zero-stiffness
direction; restrained only by other edges, e.g. drive springs). **Constrained DOF:** default
ideal-rigid (constraint, eliminated — today's kinematic semantics, byte-identical); with declared
compliance → 6-DOF-partitioned springs (translational + rotational per constrained direction).
Compliance acts between the two port frames; port frames rigid-arm to their bodies' COM frames.
For a joint with translational free DOFs, the domain-side interface frame is derived from the
linearization configuration (the mating port's frame projected onto the guide axis) — the lumped
twin of PRD 2's domains-vs-active-patch decision; on rigid bodies any rigid offset is
answer-equivalent, so this fixes frame bookkeeping (B9), not physics. Kinematics continues to read
the ideal semantics — the compliance fields are modal-consumer-only in this PRD.

### C3 — rigid-body data

Per body: mass, COM, 3×3 inertia **in the assembly frame at assembly time**. Producer: the landed
`body_mass_props` kernel path (`eval_body_mass_props_core` → `Volume`/`CenterOfMass`/
`InertiaTensor` queries) behind a realization demand, composed with the body's pose chain — the
pose thread is leaf β's new seam (probed: placement never reaches realization artifacts today);
whether the transform is baked at store or threaded to the consumer is tactical, the assembly-frame
guarantee is the contract. `point_mass(m)` bodies get m·I₃ + zero rotational inertia at their
attachment frame with a documented caveat. A body whose mass properties are unobtainable →
**Error** naming the body (no silent sentinel; INV-SF-6-conformant code; `E_DynamicsNoDensity`
precedent governs the density leg).

### C4 — assembly and solve

State: 6 DOF per body (COM frame). Each edge assembles its compliant directions as springs between
port frames (offset arms induce the translation–rotation coupling; symmetric contribution), its
ideal-rigid directions as constraints (eliminated via null-space/reduction — never penalty
stiffness). Ground edges constrain/spring to zero. The reduced (K, M) is symmetric; M is block
diagonal SPD (from C3); eigensolve via the existing entry points. n_modes/tol/max_iters honored
per the #7084 honored-set declarations (adopted, not re-landed). Closed-loop graphs with
all-compliant closures need no constraint handling; ideal-rigid loops go through the same
elimination (redundant constraints deduplicated, not errored).

### C5 — result model (owned here, extended by PRD 2)

`ModalResult` gains (shapes per Open Q 3): per-mode 6-component **effective-mass fractions**
(Σ over modes → 1 per direction, B4); per-mode **per-body energy shares** (strain + kinetic, each
summing to 1 per mode); per-mode **classification** (`structural` | `quasi_rigid`); per-body
identity records keyed by `GeometryHandleRef` (`.part` convergence; `.topology` declared or
degraded honestly per the vacuity contract — never the undeclared-write form). New readers:
`modes_above(result, floor)`-family selection by direction + effective-mass threshold;
`mode_energy(result, i)` per-body table. Existing fields keep semantics; existing dims-path results
byte-identical (B1).

### C6 — failure and degradation semantics

Unconnected/floating bodies: legal — produce quasi-rigid modes, classified, not fatal. No mass →
Error (C3). A graph with zero edges and one body degenerates to free-free rigid modes (all
quasi-rigid, zero frequency) — legal, classified. Every new diagnostic carries a `DiagnosticCode`
(INV-SF-6). No fake values: any unpopulatable field ships as honest degraded form per
`result-field-vacuity-closure.md` C1′/C2′ (this PRD lands inside that gate's regime).

## 8. Boundary-test sketch (two-way)

| # | Scenario | Pre | Post |
|---|---|---|---|
| B1 | Legacy byte-identity | existing `mechanism_modal` + dims-path modal fixtures | results byte-identical (legacy paths untouched by α–ζ) |
| B2 | Hand-chain reproduction | the printer 2-DOF idealization (k_drive, m_head, m_rest; 1-D graph) | both eigenfrequencies match the closed-form quartic roots to solver tolerance (same idealization ⇒ tight bound is legitimate, G6-checked) |
| B3 | Loop acceptance + symmetry | gantry-on-two-rails: one body on two identical radial-compliant prismatic edges | no closed-chain error; symmetric/antisymmetric pair matches the 2-spring closed form |
| B4 | Effective-mass completeness | any fixture | per direction, Σ effective-mass fractions over all modes = 1 within 1e-9 |
| B5 | Constrained-DOF compliance | single body on one prismatic edge with radial k | radial bounce + rocking frequencies match the offset-spring closed form; with compliance omitted, those DOFs are constrained (frequency → absent, not huge) |
| B6 | Free-DOF drive restraint | prismatic free axis + separate grounded drive-spring edge | axial mode f = sqrt(k_drive/m)/2π exactly; classified structural, not quasi-rigid |
| B7 | Energy-share attribution | asymmetric two-body fixture (10:1 masses) | per-mode kinetic shares sum to 1 and identify the dominant body correctly |
| B8 | Inertia realization | box + cylinder solids | realized inertia matches analytic tensors within kernel tolerance; `point_mass` caveat path exercised |
| B9 | Port-frame offset coupling | spring attached off-COM | translation–rotation coupled mode matches closed form; moving the port moves the answer (proves frames are consumed, not COM-defaulted) |
| B10 | Rejection actually fires | body with no mass source; a joint-connector edge whose port lacks a frame | Error with code naming the body / the connection — observed, not asserted (negative-assertion mandate); a *non-joint* connector is ignored per C1, tested as silence |
| B11 | Printer consumer | whole-printer graph exemplar | `lowest mode with effective mass_Y ≥ 5%` reader returns a mode ≠ mode 0; the constraint form evaluates; result stable under body enumeration order |

G6 notes: B2's tight bound is legal because both sides share one idealization (no discretization
floor exists in a lumped eigensolve — dense QZ at 12 DOF); B3/B5/B9 closed forms are
rigid-body-exact for the same reason. No FE accuracy floors are in play anywhere in this PRD —
that hazard family arrives with PRD 2 and is handled there.

## 9. Decomposition plan (task IDs backfilled at decompose)

Phase 1 — substrate. Phase 2 — vertical slice (B2 + B6 end-to-end through the real dispatch).
Phase 3 — full contract. Phase 4 — consumer + docs-truth + close.

| Label | Task | Modules | Observable signal | Prereqs |
|---|---|---|---|---|
| α | Graph IR + extraction (`EvaluationGraph.connections` + `__connector_N` instances) + nested `resolve_port_name` paths + world/ground | reify-compiler connect, reify-eval | a two-body `.ri` with a joint-connector edge round-trips to a dumped graph (debug surface); nested `a.b.port` endpoints resolve (today: hard error); non-joint connectors ignored | — |
| β | MassProperties realization demand + the pose seam (pose chain composed so mass/COM/inertia reach assembly in the assembly frame) + N≥2 consumption-keying contract | reify-eval engine_build, dynamics_ops | B8 green; a translated copy of a body yields translated COM (pose observably consumed) | — |
| γ | Constrained-DOF compliance surface on kinematic.ri joints | reify-compiler stdlib | fixture checks green; kinematics byte-identical (B1 slice) | — |
| δ | 6-DOF assembly: rigid blocks (via `SpatialInertia6`), frame-to-frame spring element, congruence constraint reduction on (K, M), ground | reify-solver-elastic | B3 + B5 + B9 green against closed forms | α, γ |
| ε | Vertical slice: modal_analysis graph overload end-to-end + dispatch registration + #7084 adoption | reify-eval modal_ops, stdlib | B2 + B6 green via `reify eval` on a committed example | α, β, δ |
| ζ | Result surface: effective mass, energy shares, classification, join keys | reify-eval, stdlib modal_analysis.ri | B4 + B7 green; vacuity-gate allowlist entries flip to this leaf's landed form | ε |
| η | Selection readers + constraint-consumable frequencies | stdlib, reify-eval | B11 reader half green | ζ |
| θ | Whole-machine exemplar (printer-shaped joint graph + reader constraints) | examples/best_practices/ | B11 green in CI | ε, ζ, η |
| μ | `[MILESTONE]` dogfood notification (DO NOT IMPLEMENT — #6626 convention: task_kind deterministic, execution_class decision, born-at-L2 escalation to Leo: lumped assembly-modal tranche landed, update printer_v01 with the joint graph and replace/augment the hand 2-DOF chain) | task store | escalation fires exactly when deps land | ε, ζ, η, θ, κ |
| ι | Boundary-test integration gate (B1–B11 complete, two-way) | crates/*/tests + drift-guard registrations same-diff | full table green in the merge gate | δ, ε, ζ, η |
| κ | Docs-truth: chunks + cheatsheet + discoverability + exemplar INDEX | reify-mcp chunks, skills | signatures compile as written; intent-level findability ("model machine resonance across parts") | θ |
| λ | PRD-close: terminal stamp + AS-AUTHORED freeze + manifest header | this file + manifest | committed header per overlay shape | all build leaves (μ excluded — milestone, not a deliverable) |

## 10. Out of scope

- Flexible bodies, meshes, region node-sets, mixed fidelity — PRD 2 (phases A–C there).
- CMS/Craig-Bampton reduction — bookmark leaf filed by PRD 2's decompose; trigger: measured scale
  wall or per-component caching pull.
- Joint damping → modal ζ (dashpot fields exist but are not consumed here) — #6861 carries it; the
  MSE landing path is PRD 2's.
- Kinematic loop-closure *position* solving (printer.ri O3) — modal linearizes at the declared
  pose; ranged-joint pose selection is Open Q 2.
- FRF/transmissibility (#6860), complex/QEP (#3831) — untouched bookmarks.
- Honoring `mechanism_modal` knobs (`tol`/`max_iters`) and its `element_order` not-applicable
  declaration — #7084 (param-drop ζ) lands it; leaf ε adopts.
- The legacy scalar `mechanism_modal` path's fake-field disposition — #7012's ruling.

## 11. Cross-PRD relationship (G4)

| PRD / task | Direction | Mechanism | Owner |
|---|---|---|---|
| `flexible-assembly-modal.md` (PRD 2) | successor | same surface + result contract, new value sources | this PRD owns contracts; PRD 2 extends |
| `result-field-vacuity-closure.md` (#7097–#7106) | mutual | `.part`/`.topology` allowlist entries require live owners → leaves ζ (here) + PRD 2's single-body leaf; their PVAC gate reads our task IDs | that PRD owns the gate; this PRD owns population |
| `trampoline-param-drop-closure.md` leaf ζ #7084 | this PRD defers to | mechanism_modal honored-set + tol/max_iters | #7084 lands it; leaf ε adopts, never double-lands |
| #7012 (mechanism-modal fake family) | adjacent | new path populates honestly by construction; legacy lumped path disposition | #7012 |
| `damped-modal-bonded-heterogeneous.md` | disjoint here | B1 byte-identity asserted against its fixtures; MSE seam is PRD 2's | that PRD |
| kinematic-constraints-completion (landed) | extends | constrained-DOF compliance params on kinematic.ri structures | this PRD (leaf γ) |
| `ports-breadth-expansion.md` (landed) | consumes | LocatedPort/Frame3 as joint attachment | landed substrate |
| `fea-load-support-selector-migration.md` (#5312/#5313) | none yet | regions unconsumed until PRD 2 rung 3 | declared to prevent drift |
| naming-convergence program | constrains | entry-point naming (`modal_analysis` overload vs `mechanism_modal_analysis`) | Open Q 1 records the ruling |
| #3830 | superseded-by-this-pair | bookmark rescoped as the authoring vehicle | #3830 description rewritten at decompose |

Contested-pair check: none of the three known contested seams is touched.

## 12. Open questions (tactical)

1. **Entry-point spelling.** The graph lives engine-side per template (probed), so both forms
   are mechanically available: scope-implicit (`modal_analysis(options)` inside the assembly
   structure reads the enclosing instance's graph — mirrors how `realization_indices_where`
   already static-matches per template) vs an explicit capture builtin
   (`structure_graph()`-style marker value). Decide in ε; record against the naming-convergence
   program either way (incl. whether `mechanism_modal_analysis` folds into `modal_analysis`
   overloads).
2. **Linearization pose for ranged joints.** Declared pose vs `neutral` vs `snapshot` — one-line
   ruling. Decide in α.
3. **Result-field shapes.** Effective-mass/energy tables as per-Mode lists vs ModalResult-level
   tables; body identity record shape. Decide in ζ inside the vacuity contract's declaration
   regime.
4. **Connector-form ergonomics** (deferred by Leo, 2026-09-01). Work 2–3 use cases at α; the graph
   IR is the contract, the spelling is not.
5. **Builder lowering vs deprecation** for `mechanism()`/`body()`. Decide in α; either way one
   graph IR.
