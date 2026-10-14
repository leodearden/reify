# Flexible assembly modal: real geometry in the modal ladder — single bodies, mixed-fidelity assemblies, reified joints

**Milestone:** v0_6 · **Status:** active · **Date:** 2026-09-01 · **Approach:** B + H (FEA + ComputeNode dispatch, G5 load-bearing)

**Code anchors** verified against main `a153350b07` (2026-09-01). Main moves fast — cite-by-symbol; re-locate lines at implementation time.

**Decomposed:** 2026-09-01 into 19 filed leaves (§9 carries their task IDs) plus two bookmarks. Capability manifest: `flexible-assembly-modal.capability-manifest.md` + its `.yaml` sidecar (normative for `delivered_check` descriptors). Four things the gates changed at decompose are recorded in the manifest's opening section: τ's registration ownership was pushed up into every test-adding leaf (esc-4914-162), A3 adopts #7097 rather than duplicating it, υ was discharged at decompose because a task-state write is unavailable to a reify agent, and contract C7 — which §9 left unassigned — was split between A1 and B2.

**Provenance:** PRD 2 of the pair authored in the whole-printer-modal design session with Leo
(2026-08-31 → 2026-09-01, `design-reify-3830-1055491`). PRD 1 —
`docs/prds/v0_6/assembly-modal-connection-graph.md` — owns the declaration surface (joint/port
connection graph) and the `ModalResult` contract; **this PRD extends value sources into those
contracts and must not fork either.** §6 decisions were ruled by Leo in that session.

## 1. Goal

Modal analysis consumes **real geometry**, in three rungs on PRD 1's ladder:

- **Rung 2 — single flexible body.** `modal_analysis(material, body : Solid, supports, options)`
  — the twin of the landed elastic body-arg chain (#4870→#4091→#5008). A `.ri` author gets the
  real modes of the real part: printer.ri's gantry tube and AFrame wall stop being
  `h_eq = sqrt(sqrt(12·ixx))` equivalent boxes with retuned ρ_eq.
- **Rung 3 — mixed-fidelity assembly.** Bodies in the PRD-1 connection graph may be flexible:
  their meshes enter the assembled eigenproblem; lumped joints attach at **port regions** (patch →
  6-DOF frame reduction); `Fixed`/bonded groups merge into one continuous body with piecewise
  material (the `damped-modal-bonded-heterogeneous.md` decision-9 shape). The whole printer —
  flexible gantry tubes and frame walls, rigid hardware, compliant bearings and drive springs —
  solves as one honest eigenproblem.
- **Rung 4 — reified joints.** A joint is modelled as the physical entity it is: a geometric
  surrogate (e.g. the air-bearing's "elastic slippery collar") bonded between the mating bodies,
  carrying a calibrated anisotropic constitutive law. At this rung the spring element disappears —
  coupling is bonded interfaces, compliance is constitutive. Joint tilt stiffness emerges from
  patch extent instead of being declared; the lumped `spring_rate` becomes the calibrated
  *reduction* of this model, checkable in-design (cross-level consistency constraints, the
  printer.ri pin-and-band idiom generalized to joints).

Consumer (G1): `prj/printer_v01/printer.ri` at each rung (real-tube modal replacing the
equivalent-beam block; the whole-printer graph upgraded body-by-body; the air-bearing collar), with
committed `examples/best_practices/` exemplars as the CI-resident stand-ins. Engine-integration
seam: §3.4 ComputeNode dispatch + §3.2 realization-kind dispatch of `engine-integration-norm.md` —
the same two seams PRD 1 rides; no new seam.

## 2. Background — measured state

- All four modal trampolines ignore `_realization_inputs` (`solve_modal_analysis_trampoline`,
  `solve_mechanism_modal_trampoline`, `solve_transient_response_trampoline`,
  `displacement_at_trampoline`, `crates/reify-eval/src/modal_ops.rs`); every modal number any
  design produces today comes from an internally synthesized box mesh (`build_beam_mesh`).
- The elastic playbook is landed but **author-unreachable**: #6660 (verified 2026-08-31) — gmsh is
  a dev-dependency of reify-eval only; no author binary links it; a kernel-absent body solve
  returns a hollow `Converged` result. That task's acceptance (link + register in CLI/GUI; refuse
  loudly when kernel-absent) gates every rung of this PRD.
- BC targeting: `FixedSupport.target : String` placeholder; the consume half
  (`target_node_set`) and produce half (`bc_resolve::build_face_anchors`) exist; the middle —
  `String → Selector` flips + build-time resolution wired into the extractors — is owned by
  `fea-load-support-selector-migration.md` Bmig2 (#5312) / Bmig2-consume (#5313), both pending.
  This PRD consumes that seam, never re-lands it.
- Heterogeneous modal substrate is in flight, undelivered: damped-modal chain #6877–#6886 all
  pending (Field classification γ #6879, declarative constructors δ #6880, heterogeneous P2 modal
  assembly ζ #6882, MSE η #6883). Its B4 pin: homogeneous modal byte-identity. Rung 3's bonded
  groups and rung 4's surrogates consume γ/δ/ζ; MSE composition (lossy collar → modal ζ) consumes
  η.
- #6887 (`[MILESTONE]` assembly-derived material fields) is parked DO-NOT-IMPLEMENT pending a
  human PRD decision. **This design session is that decision** (Leo, 2026-09-01): the
  bonded-group producer lands under this PRD's rung-3 leaf (**#7149**); #6887 was rescoped at
  decompose to point there, with #6626 dropped from its dep set.
- Known accuracy/cost hazards (G6 floor family): P1-tet bending lock (9–11% on slender members —
  P2 mandatory on the modal path, the #6882 posture); pointwise-Dirichlet BC realization
  (`k≈0.67–0.70`); two-FixedSupport pin-collapse #6663 (in-progress; inherited, not owned);
  synthetic-mesh cost blowups measured at 550 s for one dims eval (2026-08-27 probe note) and gmsh
  fill-fraction defects (#6200 class) — whence the mandatory *measurement* leaves (§9 π/ρ) before
  any cost- or accuracy-bound is asserted.
- `.part` convergence: single-body modal is where `ModalResult.part` becomes the
  `CarriedTopology.part : GeometryHandleRef` join key (`result-field-vacuity-closure.md` §2.1
  ratified disposition); this PRD's rung-2 leaf is that allowlist entry's live owner alongside
  PRD 1's result-surface leaf.

## 3. Sketch of approach

**Rung 2** twins the elastic chain: a `modal_analysis` body-arg overload dispatches to the
free-vibration trampoline; the realized posed `VolumeMesh` (P2-promoted) arrives via
`realization_inputs` exactly as `realized_solver_mesh_with_handle` consumes it in
`compute_targets/elastic_static.rs`; supports resolve through the selector→node-set seam (#5313);
assembly and eigensolve are the landed homogeneous paths. Kernel-absent → loud refusal (the #6660
acceptance posture; never hollow `Converged`).

**Rung 3** generalizes PRD-1's assembly: a body declared flexible contributes its FE block to
(K, M) instead of a 6-DOF block; a lumped joint on a flexible body attaches through its
`RegionPort.region` — resolved to a node set, reduced to a 6-DOF port frame (reduction rule: Open
Q 2), springs/constraints then act port-to-port exactly as in PRD 1 C4. `Fixed`/bonded subgraphs
collapse *before* meshing into one continuous body with a piecewise material field (union of the
members' solids + per-member laws — the #6879/#6880 surface), meshed once; their internal
interfaces cease to exist as joints. The mixed sparse (K, M) goes to `solve_eigen_shift_invert`.

**Rung 4** replaces a lumped joint edge with a **surrogate body**: a joint-entity structure whose
geometry is model geometry (a thickened film, not the literal 10 μm gap) and whose law is a named
anisotropic family (e.g. `bearing_film(k_per_area_normal, slip_axes, frame)` — near-zero shear on
slip directions, calibrated normal stiffness, near-massless), bonded at both mating regions via
the rung-3 machinery. Free sliding appears as near-zero-frequency quasi-rigid modes restrained by
drive springs — the correct physics, classified by PRD 1's result axes. Calibration is dataflow:
datasheet + provenance, analytic closed forms, or a one-time higher-fidelity run feeding the
lumped `spring_rate` cell, with cross-level consistency constraints in-design.

## 4. Pre-conditions

- PRD 1 leaves α–ζ (surface, graph IR, realization channel, result contract) — same-pair
  dependency, wired as real edges at decompose.
- #6660 (gmsh linking + loud kernel-absent refusal) — every rung.
- #5312/#5313 (+ BT gate #4371) — selector→node-set truly consumed — rungs 2/3 BC + region path.
- damped-modal #6879 (classify any Field), #6880 (declarative constructors), #6882 (heterogeneous
  P2 modal assembly + per-element-density mass) — rung 3 bonded groups; #6883 (MSE) — rung 4
  damping composition only.
- Landed and verified: VolumeMesh realization demand/execute (#4743), `promote_beam_mesh_to_p2`,
  sparse shift-invert eigensolve over arbitrary symmetric sparse (K, M) with matrix-free
  `StiffnessOp`/`MetricOp` seams (probed 2026-09-01 — mesh-agnostic, substrate-ready),
  CarriedTopology (#4654), FaceSelector modal-surface precedent (#4122/#4655). The 6-DOF
  spring element and the congruence constraint reduction on (K, M) are PRD 1 leaf δ deliverables
  this PRD's B1/B3 reuse (today `add_joint_stiffness` is scalar-diagonal-only and
  `mpc.rs` eliminates on K/f only — measured absences, owned by PRD 1). Scale datum shaping ρ/π:
  dense QZ at 864 DOF ≈ 25 s (`modal_benchmarks.rs`) — the sparse path is mandatory at every
  rung of this PRD. Realization-channel N-arity, demand registration, boundary-association
  packaging and the pose gap are probed and folded into C2 as measured claims.
- #6663 (BC pin-collapse, in-progress) is inherited fidelity, not a gate: rungs land with whatever
  BC fidelity exists, and the `analytic ×4` workaround retirement stays #6729's.

## 5. Resolved design decisions (Leo, 2026-08-31 → 2026-09-01)

1. **Same surface, same result type, new value sources.** No new entry-point family, no
   `AssemblyModalResult`. PRD 1's contracts are closed against extension-by-fork.
2. **Bonded ≡ merged.** A `Fixed`/bonded subgraph is one continuous body with piecewise material —
   never tied MPC pairs across nonconforming meshes in v1 (mesh-tying is confined to Open Q 4 as a
   measured fallback if union-meshing quality fails at printer scale). Ratifies and consumes
   damped-modal decision 9; answers #6887.
3. **Ports are where joints grab meshes.** The `RegionPort.region` declared once (PRD 1 decision 4)
   is realized on a meshed body as node-set + reduction to a 6-DOF frame. Explicit regions; no
   proximity-to-pivot magic. Reduction rule is measured, not assumed (Open Q 2).
4. **Reified joints are surrogates, loudly.** Surrogate geometry + law are jointly calibrated
   model artifacts; docs and diagnostics say so (G6 honesty — a collar is not literal fidelity).
   Near-massless by default; mass opt-in.
5. **Measure before bounding.** Scale (mesh + eigensolve cost at whole-printer size) and
   conditioning (thin anisotropic layers at extreme stiffness ratios) get dedicated
   measurement leaves whose signals are *recorded numbers*, and later accuracy/cost assertions
   cite those measurements — the esc-3453-class prevention, applied prospectively.
6. **CMS stays a bookmark under the same surface**, filed at decompose with trigger = measured
   scale wall (π's numbers) or per-component caching pull. Its interface DOFs are the port frames
   this pair builds; no surface change when triggered.
7. **Damping**: rung ≤3 inherits descriptors unchanged; rung 4 composes joint loss through MSE
   (#6883) — the #6861 landing path; bolted micro-slip and amplitude dependence stay out.
8. **Declared port regions are contact *domains*; the joint derives the *active* patch at the
   linearization configuration** (Leo's air-bearing question, ruled 2026-09-01). A guide rail's
   port declares the full guide surface — static and owner-local, because a template never
   references its mate; the sleeve's port declares its bore footprint. For a joint with
   translational free DOFs, the domain-side active region is derived at analysis time: the mating
   port's footprint, posed at the linearization configuration, projected onto the domain region.
   The rejected alternative — a port region value-depending on the mate's position — inverts
   template encapsulation and routes through cross-sub value reads / instantiation overrides,
   i.e. directly into #6583/#6592. Embraced consequence: assembly modal is
   **configuration-parametric** — the spectrum with the carriage at mid-span differs from
   carriage-at-end, physically and in the model; sweeping configurations is legitimate dogfood.

## 6. Contract (H) — extensions only; PRD 1 C1–C6 govern everywhere they reach

**C1 — body-arg overload.** `modal_analysis(material : ElasticMaterial, body : Solid, supports :
List<Support>, options : ModalOptions) -> ModalResult` (+ the Field-material variant once #6882
lands, same dispatch). Kernel-absent or unrealizable mesh → Error diagnostic; `convergence_status`
never claims what didn't happen. `ModalResult.part` = the body's `GeometryHandleRef`;
`.topology` = its `CarriedTopology` (the vacuity gate's populated form).

**C2 — posed multi-body realization.** Realization demands deliver, per flexible body, its P2
`VolumeMesh` with its `BoundaryAssociation` riding inside it (probed: `VolumeMesh.boundary`, not a
sibling channel), and per cited region, its node set — with the assembly-frame guarantee holding
at solve time via the pose seam PRD 1 leaf β lands (probed 2026-09-01: realization artifacts are
local-frame; placement never writes back to `realization_handles`; the pose chain must be baked
or threaded — β's tactical call, reused here unchanged). Mechanism facts consumed as landed:
demand via `register_volume_mesh_demand` (modal targets currently unregistered — A1 registers
them); N-ary input build is arg-ordered and 1:1 (`build_compute_realization_inputs`); N≥2
same-type artifacts are keyed by the positional/keyed contract PRD 1 α/β establish (today's
`find_map`-by-type breaks at N≥2 — measured).

**C3 — region reduction.** A lumped joint side on a flexible body: region node set → one 6-DOF
interface frame by the ruled reduction (Open Q 2), contributing its constraint/spring rows against
that frame. Invariants: reduction is rigid-limit-exact (a rigid body modelled flexibly with E→∞
reproduces PRD 1's answer within tolerance — B14); moving the region moves the answer. Region
*typing* is in-scope leaf work, G3-measured 2026-09-01: `face(…)` resolves to
`Type::Selector(Face)`, not `Type::Geometry`, so `RegionPort.region : Geometry` cannot hold a
face selector type-correctly — it only "works" today through the port-param default-expression
type-check hole (any value passes silently). B1 fixes the region typing (selector-typed or
dual-accepting, following the #5312 kind-agnostic precedent) rather than shipping through the
hole; the hole itself is filed as a found-during task (ctor/param-conformance family), not owned
here. Active-patch derivation (decision 8): for a joint with translational free DOFs the
domain-side node set is the declared region's nodes filtered to the mating port's posed footprint
(projection along the joint normal; tolerance follows mesh pitch — tactical, decide in B1); joints
with no translational free DOF consume the declared region directly. Invariant: the active patch
follows the configuration (B23).

**C4 — bonded-group collapse.** Maximal `Fixed`-connected subgraphs merge pre-mesh: union solid +
piecewise material field over member laws; the merged body's identity record carries the member
`GeometryHandleRef`s (energy shares stay per-member via element→member attribution). Interfaces
inside a merged group produce no joint rows.

**C5 — mixed assembly.** (K, M) blocks: 6-DOF rigid (PRD 1 C4) + FE flexible (per-element P2,
`_with_field` path for piecewise fields) + joint rows between port/interface frames. Symmetric,
SPD M; `solve_eigen_shift_invert`; n_modes/tol/max_iters honored per the adopted declarations.

**C6 — surrogate laws.** A named constructor family produces anisotropic laws with slip/normal
partition in a joint-local frame; conforms to the #6879 Field classification (any evaluable
source); loss factor optional (feeds #6883 MSE). A surrogate declared where its blocked
substrate is missing → loud Error naming the gate, never silent isotropic fallback.

**C7 — result extensions.** Full per-node shapes are opt-in (`ModalOptions` flag; default:
per-body reduced records — port-frame motion + energy shares). Effective-mass, energy-share,
classification and reader semantics are PRD 1 C5's, unchanged; per-member attribution inside
merged groups per C4.

## 7. Boundary-test sketch (two-way)

| # | Scenario | Pre | Post |
|---|---|---|---|
| B12 | Twin equivalence | same box geometry: dims overload vs body-arg overload | first bending family agrees within a band **derived from π's measured mesh deltas** (different meshers — never byte-identity; G6) |
| B13 | Pose invariance + consumption | same body at two placements; support selector riding along | identical spectrum (invariance); mode shapes transform by the placement (proves posed consumption) |
| B14 | Rigid limit | one flexible body with E scaled ×10⁶ in the PRD-1 graph fixture | matches PRD 1's rigid answer within tolerance; proves C3 reduction exactness |
| B15 | Bonded ≡ merged | two-box bonded pair via `Fixed` edge vs hand-authored union+field of the same | same spectrum within solver tolerance (same mesh path by construction) |
| B16 | Surrogate degenerate | collar with slip shear → normal stiffness (isotropized, stiffened) | approaches the bonded (B15) answer monotonically; proves the surrogate reduces to bond in the stiff limit |
| B17 | Slippery collar frees the DOF | bearing_film collar, axial slip | axial quasi-rigid mode restrained only by the drive spring (PRD 1 B6 twin at continuum fidelity) |
| B18 | Cross-level consistency | lumped bearing k vs collar-model emergent k on one geometry | in-design constraint band holds (the calibration idiom exemplar, band from ρ's measurements) |
| B19 | Kernel-absent refusal | author binary without gmsh (pre-#6660 posture simulated) | Error diagnostic observed; no hollow Converged (negative-assertion mandate; coordinates with #6660's own acceptance) |
| B20 | Vacuity flip | rung-2 result | `.part`/`.topology` populated (join key + CarriedTopology); PVAC gate's modal allowlist entries retire |
| B21 | Rung-2 exemplar consumer | gantry-tube-shaped real-geometry modal exemplar | the equivalent-beam idiom is demonstrably replaceable: the exemplar solves the real solid and its analytic-band constraint form evaluates (the printer.ri update itself is Leo's, triggered by milestone M-A) |
| B22 | MSE composition (rung 4, deps #6883) | lossy collar + steel bodies | modal ζ matches the MSE energy-split closed composition on the fixture within the #6883-established band |
| B23 | Active patch follows configuration | rail+sleeve fixture at two carriage positions | resolved domain-side node sets differ and track the posed sleeve footprint; the spectra differ in the direction the closed form predicts |

## 8. Measurement leaves (numbers first, bounds second)

- **π — scale probe.** Mesh + P2 promote + assemble + shift-invert on a frame+rails+gantry-class
  assembly at 2–3 mesh densities; record DOF counts, wall times, memory; record dims-vs-gmsh
  spectral deltas for B12's band. Signal: committed numbers table (a `docs/measurements/` note),
  no pass/fail bound.
- **ρ — conditioning probe.** Thin near-massless anisotropic layer between stiff bodies at
  stiffness ratios 10³–10⁹: eigensolve health (Lanczos iterations, spurious-mode census,
  factorization failures) vs ratio and layer thickness. Signal: committed envelope note that C6
  constructors cite as their validity range.

## 9. Decomposition plan (task IDs backfilled at decompose)

Phase A — rung 2 vertical slice. Phase B — rung 3. Phase C — rung 4. Bookmark filings at close.

| Label | Task | Modules | Observable signal | Prereqs |
|---|---|---|---|---|
| A1 | Body-arg overload + posed VolumeMesh consumption + loud kernel-absent refusal — **#7142** · also owns C7's `ModalOptions` full-shape opt-in flag + storage format (Open Q 5) | stdlib modal_analysis fns, reify-eval modal_ops | B13 + B19 green; a committed example solves a real solid end-to-end via `reify eval` | #6660, PRD 1 β, #4743-landed substrate
| A2 | Selector BCs on the modal path — **#7143** | reify-eval | supports via typed selectors drive the modal node sets (twin of #5313's elastic wiring) | A1, #5313 |
| A3 | `.part`/`.topology` population (join key + CarriedTopology) — **#7097** (**adopted** `result-field-vacuity-closure.md`'s existing live PVAC owner rather than filing a duplicate — that task explicitly invited adoption, and a fresh leaf would have left it deferred forever as a phantom owner) | reify-eval | B20 green; vacuity allowlist entries flip | A1, PRD 1 ζ
| A4 | Rung-2 exemplar (real-geometry part modal, gantry-tube-shaped) — **#7144** | examples/best_practices/ | B21 green in CI | A1–A3 |
| M-A | `[MILESTONE]` dogfood notification, rung 2 (DO NOT IMPLEMENT — #6626 convention: task_kind deterministic, execution_class decision, born-at-L2 escalation to Leo: real-geometry part modal landed, update printer_v01) — **#7146** | task store | escalation fires exactly when deps land | A1–A4 |
| π | Scale measurement — **#7145** | scratch + docs/measurements/ | numbers table committed | A1 |
| B1 | Region→frame reduction + region typing fix + active-patch derivation (C3, decision 8) — **#7152** | reify-solver-elastic, reify-eval, stdlib ports | B14 + B23 green | A1, PRD 1 δ/ε |
| B2 | Bonded-group collapse → union + piecewise field (C4; answers #6887) — **#7149** · also owns C7's per-member energy-share attribution inside merged groups | reify-eval, assembly-derivation seam | B15 green | A1, #6879, #6880, #6882 |
| B3 | Mixed assembly + sparse eigensolve integration (C5) — **#7153** | reify-solver-elastic, reify-eval | a 3-body mixed fixture solves; B14/B15 compose | B1, B2, PRD 1 ε
| B4 | Rung-3 exemplar (mixed-fidelity machine graph, printer-shaped) — **#7154** | examples/best_practices/ | a mixed rigid/flexible machine graph solves; reader constraints evaluate | B3, PRD 1 θ |
| M-B | `[MILESTONE]` dogfood notification, rung 3 (DO NOT IMPLEMENT — same convention: mixed-fidelity assembly modal landed, update printer_v01) — **#7147** | task store | escalation fires exactly when deps land | B1–B4 |
| ρ | Conditioning measurement — **#7150** | scratch + docs/measurements/ | envelope note committed | B3 |
| C1 | Surrogate law family + constructors (C6) — **#7155** | stdlib, reify-solver-elastic material seam | B16 + B17 green within ρ's envelope | B3, ρ |
| C2 | Calibration idiom + cross-level constraint exemplar — **#7156** | examples/best_practices/ | B18 green | C1 |
| M-C | `[MILESTONE]` dogfood notification, rung 4 (DO NOT IMPLEMENT — same convention: reified-joint surrogates landed, update printer_v01 with the air-bearing collar) — **#7148** | task store | escalation fires exactly when deps land | C1–C3 |
| C3 | MSE joint-damping composition — **#7157** | reify-eval | B22 green | C1, #6883 |
| σ | Docs-truth (chunks + cheatsheet + discoverability, all rungs) — **#7158** | reify-mcp, skills | signatures compile; intent-level findability | A4, B4, C2, PRD 1 κ
| τ | Boundary-test integration gate (B12–B23 complete, two-way) + **verification** that each landed test's drift-guard registrations are present — **G7/esc-4914-162 scope correction made at decompose:** registration OWNERSHIP was pushed up into every test-adding leaf (A1, A2, A3, B1, B2, B3, B4, C1, C2, C3), because a registration task downstream of the tests it registers is exactly what turned main RED in esc-4914-162 — **#7159** | crates/*/tests, tests/infra | B12–B22 in the merge gate | A1, A2, A3, A4, π, B1, B2, B3, C1, C3, ρ
| υ | **Discharged at decompose (2026-09-01), not filed as a leaf.** Its deliverable is a task-state WRITE, and reify has no task-write path (`crates/reify-audit/src/fused_memory_client.rs` is read-only; the sandboxed write-set never grants `.taskmaster/`), so a reify agent could not execute it. Both bookmarks were filed by the decompose session instead: **#3830** rescoped from "author the flexible-multibody-modal PRD" to *CMS as a reduction strategy under this surface* (trigger: π's measured scale wall, or a per-component caching pull), and **#7161** filed for CFD/Reynolds-calibrated surrogates (trigger: a design decision turning on film stiffness a datasheet does not cover, or C2's band unmeetable from datasheet/closed-form values). Both stay `deferred`, excluded from the batch flip. | task store | both bookmarks exist with external triggers naming π's and ρ's committed artifacts | — (discharged) |
| φ | PRD-close: terminal stamp + freeze + manifest header — **#7160** | this file + manifest | committed header per overlay shape | all build leaves (M-A/M-B/M-C excluded — milestones, not deliverables, per the damped-modal κ precedent) |

## 10. Out of scope

- CFD/Reynolds film kernels (the calibration *slot* exists; the physics kernel is the bookmarked
  future); measuring the physical EG/film properties.
- Shell elements for panels — `composite-laminated-shells.md`'s future scope; panels mesh as
  solids here with the cost that implies (π records it).
- Contact, preload stress-stiffening, gyroscopic terms, nonlinear or amplitude-dependent joints.
- Complex/QEP eigensolve (#3831), FRF (#6860) — untouched bookmarks.
- The selector-migration mechanics (#5312/#5313) and gmsh linking (#6660) — consumed, never
  re-landed.
- Kinematic loop-position solving; `at auto` mate-derived joints (a possible future producer of
  the same graph).

## 11. Cross-PRD relationship (G4)

| PRD / task | Direction | Mechanism | Owner |
|---|---|---|---|
| `assembly-modal-connection-graph.md` (PRD 1) | extends | surface + result contracts | PRD 1 owns contracts |
| #6660 | consumes | kernel linking + refusal posture (B19 coordinates wording) | #6660 |
| `fea-load-support-selector-migration.md` #5312/#5313/#4371 | consumes | selector→node-set on modal path (A2 twin-wires, never re-lands) | that PRD |
| `damped-modal-bonded-heterogeneous.md` #6879/#6880/#6882/#6883 | consumes | Field classification, constructors, heterogeneous P2 modal, MSE | that PRD; its B4 byte-identity pin binds B-phase work |
| #6887 (parked milestone) | answered-here | bonded-group producer lands as leaf B2 (**#7149**); #6887 **rescoped 2026-09-01** — its "run a /prd session" deliverable is discharged (Leo made that decision in this design session), its remaining deliverable narrowed to the printer_v01 whole-frame bonded dogfood escalation, and its dep set corrected: **#6626 removed**, **#7149 added** | this PRD |
| `result-field-vacuity-closure.md` | mutual | A3 is the `.part`/`.topology` live owner; PVAC reads its task id | that PRD owns the gate |
| #6663 / #6729 | inherited | BC fidelity + workaround retirement | those tasks |
| `composite-laminated-shells.md` (stub) | disjoint | solid-path vs shell-path, revisit at its activation | no shared mechanism |
| assembly-derivation-toolbox / #6626 | adjacent | derived subs are more bodies to this PRD; no shared mechanism v1; #6626 leaves this PRD's dep paths | that program |

Contested-pair check: none of the three known contested seams is touched.

## 12. Open questions (tactical)

1. **Field-vs-material overload spelling at rung 3** (merged groups need the Field form; single
   bodies the plain form) — follow #6882's landed dispatch. Decide in B2.
2. **Region→frame reduction rule** — rigid-spider vs load-averaging; measure both on B14's
   fixture, pick per rigid-limit exactness + conditioning. Decide in B1.
3. **Mesh-budget knobs** — per-body mesh size on the graph vs global `ModalOptions`; π's numbers
   drive the shape. Decide in B3.
4. **Union-mesh quality fallback** — if printer-scale union meshing fails (fill-fraction #6200
   class), the measured fallback is conforming-interface tying; confined here deliberately.
   Decide in B2 on π + B15 evidence.
5. **Full-shape storage format** (opt-in C7) — realization-side artifact vs in-value arrays; GUI
   contour view is the eventual consumer. Decide in A1.
