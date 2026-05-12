# Audit: Kinematic Constraints — Forward, Open-Chain, Library-Level (v0.1)

**PRD path:** `docs/prds/kinematic-constraints.md`
**Auditor:** audit-kinematic-constraints-toplevel
**Date:** 2026-05-12
**Mechanism count:** 24
**Gap count:** 11 (mapped to 10 fused-memory entries — M-019/M-020/M-021 share the FK-transform DRIFT and are written as two gap memories covering interferes vs interferes_with/min_clearance)

## Top concerns

- **Closed-chain "build-time error" is DRIFT.** PRD task 3 demands a closed chain produces `error[E_KINEMATIC_CLOSED_CHAIN]` at `mechanism()` build time. The implementation has been migrated to v0.2 behaviour (records `loop_closures` and runs a Newton solver in `snapshot()`). `DiagnosticCode::KinematicClosedChain` is reserved but explicitly "**not currently emitted**" per `crates/reify-types/src/diagnostics.rs:703-722`. This is a deliberate, documented PRD-vs-impl split — but the v0.1 PRD has not been retired/redirected.
- **OCCT interference query is FK-ignoring (DRIFT).** The PRD §5 stipulates that `interferes(snapshot) → List<(BodyId, BodyId)>` uses "OCCT BREP intersection" of the snapshot bodies. In practice (`crates/reify-eval/src/geometry_ops.rs:1333-1340`) the dispatch deliberately does NOT apply the per-body `world_transform` to the OCCT shape — geometry must be pre-positioned at source-let level. The two worked examples (`examples/kinematic/{dock_pickup,counter_mass_balance}.ri:11-22`) acknowledge this. This makes the dock-pickup-along-a-path use-case from the PRD non-functional without manual per-snapshot pre-positioning.
- **Sweep-driven interference NOT supported.** The PRD's worked example (`snapshots.map(|s| interferes(s))`) needs `interferes` to flow through `.map()` lambdas, but the eval-time post-process only resolves top-level `interferes(snapshot_let)` cells (per `dock_pickup.ri:50-52`). The PRD's headline use case is partially fictional at the eval layer.
- **Stdlib types not first-class in the language.** `Prismatic`, `Revolute`, `Coupling`, `Mechanism`, `Snapshot`, `Joint`, `BodyId`, `JointBinding`, `SweepDim`, `MotionValue<J>` are all documented as types in `docs/reify-stdlib-reference.md:1155-1290` but exist in code only as `Value::Map` records with a `"kind"` discriminant. There is no `Type::Mechanism`, no `Type::Joint`, no trait `DrivingJoint`. Type-checking these surface types relies on per-name compiler hooks (`kinematic_query_result_type` in `units.rs:110-125`) rather than declared types. Acceptance criterion "New stdlib types … registered" reads as DRIFT.

## Mechanisms

### M-001: `prismatic(axis, range)` constructor → Joint value

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-stdlib/src/joints.rs:14-28` (validation + `make_joint`); registered via stdlib dispatch in `lib.rs:72`. Compiler-side recognition is implicit (no surface type — see M-022).
- **Blocks:** none
- **Note:** 2-arg form with axis validation, range LENGTH-dimensioned check. Raw (unnormalised) axis stored; normalised at `transform_at`.

### M-002: `revolute(axis, range)` constructor → Joint value

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-stdlib/src/joints.rs:29-43`
- **Blocks:** none
- **Note:** Mirrors prismatic; range ANGLE-dimensioned.

### M-003: `couple(other_joint, ratio, offset?)` derived joint

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-stdlib/src/joints.rs:469-518`; tests exercising sign+offset behaviour cited in `snapshot.rs::tests::center_of_mass_counter_mass_balance_stationarity`; example `examples/kinematic/counter_mass_balance.ri` exercises end-to-end.
- **Blocks:** none
- **Note:** Rejects nested coupling parents at construction (termination guarantee). Accepts prismatic and revolute parents only.

### M-004: `transform_at(joint, motion_value)` → `Transform<3>`

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-stdlib/src/joints.rs:171-468`; per-kind arms for prismatic/revolute/coupling/fixed/planar/spherical/cylindrical (the last four are v0.2 ORPHAN-relative-to-this-PRD).
- **Blocks:** none
- **Note:** 1-arg form valid only for `fixed` joints (0-DOF ergonomic form, task 2688).

### M-005: `mechanism()` empty-builder seed

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-stdlib/src/mechanism.rs:36-41, 193-214`; canonical Map shape `{ bodies, joint_parents, kind, loop_closures, next_id }`.
- **Blocks:** none

### M-006: `body(mechanism, solid, at, parent?, pose?)` chain step

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-stdlib/src/mechanism.rs:48-122, 416-561`; 3/4/5-arg dispatch with default parent=world / pose=identity.
- **Blocks:** none

### M-007: Closed-chain detection emits `E_KINEMATIC_CLOSED_CHAIN` at build time

- **State:** DRIFT
- **Failure mode:** N/A (DRIFT)
- **Evidence:** `crates/reify-types/src/diagnostics.rs:703-722` ("v0.2: not currently emitted"); `mechanism.rs:481-523` (closing edges silently recorded as `loop_closures` instead of errored); `mechanism.rs` module doc lines 9-22 explicitly describe v0.2 behaviour.
- **Blocks:** PRD task 3 acceptance criterion "closed-chain → emit `E_KINEMATIC_CLOSED_CHAIN` with both joint paths in the diagnostic" — this acceptance criterion is unimplementable as written; the v0.2 PRD (`docs/prds/v0_2/kinematic-constraints.md`) supersedes.
- **Note:** Variant is intentionally reserved for a future opt-in strict mode. The DiagnosticCode comment block (line 715-721) calls this out explicitly. Architectural shift: v0.1 PRD said "v0.1 type error"; v0.2 PRD says "v0.2 solves them"; v0.2 work landed but v0.1 PRD was not amended.

### M-008: `body_id_of(mechanism, solid)` lookup

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-stdlib/src/mechanism.rs:123-177`; used by `examples/kinematic/dock_pickup.ri:63-64`.
- **Blocks:** none
- **Note:** Uses structural Value equality, not "referential identity" as PRD prose intends — documented gap (see also M-024 duplicate-solid). Stdlib reference docs §13.2 acknowledge the gap.

### M-009: `E_MECHANISM_DUPLICATE_SOLID` diagnostic emission

- **State:** PARTIAL
- **Failure mode:** F5 (declared but not wired to diagnostic pipeline)
- **Evidence:** `crates/reify-types/src/diagnostics.rs:723-745` ("TODO: wired by the snapshot/eval-pipeline integration in the task family covering 2585+"); `crates/reify-stdlib/src/mechanism.rs:357-376` records the condition on the Mechanism Map but does not emit a `Diagnostic`.
- **Blocks:** none (silent error fields on Mechanism Map are surfaced to downstream Undef chains)
- **Note:** Construction-side detection works; user just gets an `error_message` field on the Map instead of a real diagnostic.

### M-010: `snapshot(mechanism, bindings)` FK evaluator

- **State:** WIRED (with one DRIFT — see M-019 warm-start 3-arg form is a v0.2 addition not in this PRD)
- **Failure mode:** N/A
- **Evidence:** `crates/reify-stdlib/src/snapshot.rs:57-365`; `walk_fk` at line 657; exercised by `crates/reify-eval/tests/kinematic_examples_e2e.rs`.
- **Blocks:** none

### M-011: `bind(joint, value)` binding constructor

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-stdlib/src/snapshot.rs:35-56`; lazy validation — accepts non-validating motion values (`Undef` for non-DrivingJoint, dimension mismatches) and surfaces failures at `snapshot()` time.
- **Blocks:** none

### M-012: `sweep(mechanism, joint, range, steps)` 1-D batch

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-stdlib/src/sweep.rs:59-127`; first/last endpoint matching pinned by tests in sweep.rs.
- **Blocks:** none

### M-013: `sweep_grid(mechanism, [SweepDim...])` N-D cross-product

- **State:** PARTIAL
- **Failure mode:** F2 (PRD-vs-impl signature mismatch — surface uses `dim()` constructor wrapper not tuples)
- **Evidence:** `crates/reify-stdlib/src/sweep.rs:128-219`; PRD says `[(joint, range, steps)...]` (tuple-shaped) — impl requires explicit `dim(joint, range, steps)` SweepDim Maps. Reference docs `reify-stdlib-reference.md:1278-1285` document the `dim()` form. Functionally equivalent; surface-wording mismatch.
- **Blocks:** none
- **Note:** Stdlib reference doc has been updated, PRD prose has not.

### M-014: `snapshot.bodies()` accessor

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-stdlib/src/snapshot.rs:366-397`
- **Blocks:** none
- **Note:** Returns `List<Int>` (BodyId), not the PRD prose "the placed Solids". PRD wording reconciled by the stdlib reference doc shape (§13.3 `bodies(s: Snapshot) -> List<BodyId>`).

### M-015: `snapshot.transform_of(BodyId)` accessor

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-stdlib/src/snapshot.rs:398-431`
- **Blocks:** none

### M-016: `snapshot.center_of_mass(densities?)` accessor

- **State:** PARTIAL
- **Failure mode:** F3 (point-mass approximation, not volumetric)
- **Evidence:** `crates/reify-stdlib/src/snapshot.rs:432-542`; explicit comment lines 437-441: "v0.1 semantic: density-weighted mean of per-body world-frame ORIGINS (translation of each body's `world_transform`). Point-mass approximation — real volumetric centroid needs OCCT (`BRepGProp::VolumeProperties`), scope of FFI task #2530."
- **Blocks:** Counter-mass-balance acceptance test (PRD §6) is pinned by `examples/kinematic/counter_mass_balance.ri` which uses unit-translation bodies, where point-mass and volumetric COMs coincide trivially. Realistic densities/geometry will give wrong answers.
- **Note:** Same body's `solid` is not touched by FK — only world transform origin. For a single-body mechanism with a non-origin solid this returns the FK frame origin, not the geometric centroid of the body. The PRD §6 says "counter-mass-balance acceptance test" — this passes only because the test is geometry-agnostic.

### M-017: `snapshot.bounding_box()` accessor

- **State:** PARTIAL
- **Failure mode:** F3 (point-mass approximation)
- **Evidence:** `crates/reify-stdlib/src/snapshot.rs:543-598`; comment lines 547-551: "v0.1 semantic: AABB of per-body world-frame ORIGINS … real volumetric AABB requires OCCT (`BRepBndLib::Add`), scope of FFI task #2530."
- **Blocks:** none acutely, but useless for any clearance-by-bounding-box check.

### M-018: OCCT `BRepExtrema_DistShapeShape` FFI binding

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-kernel-occt/src/ffi.rs:338, 656-675`; `crates/reify-kernel-occt/src/lib.rs:694-752, 2223+`; `GeometryQuery::Distance` dispatched.
- **Blocks:** none
- **Note:** PRD §5 also names `BRepAlgoAPI_Common` for the intersection probe; the implementation collapses both probes into `BRepExtrema_DistShapeShape` with `Distance ≤ 0` for the intersection case (geometry_ops.rs:1331). Simpler and cheaper; functionally equivalent for the v0.1 acceptance.

### M-019: `interferes(snapshot)` returning body pairs

- **State:** DRIFT
- **Failure mode:** F2 (PRD-vs-impl mismatch on FK transform application)
- **Evidence:** `crates/reify-eval/src/geometry_ops.rs:1333-1340` ("v0.1 simplification: the Snapshot's per-body `world_transform` is **not** applied to the OCCT shape before the distance probe"); `examples/kinematic/dock_pickup.ri:11-22` documents the impact for users.
- **Blocks:** PRD §1 "Worked example" use case (toolchanger dock pickup-path clearance) requires FK-positioned bodies; current implementation only queries source-let geometry. The PRD's headline acceptance — clearance verification under motion — is non-functional until either (a) sweep-driven map dispatch (M-021) or (b) FK→OCCT transform application is wired.
- **Note:** Pair iteration (i<j upper-triangular) and self-pair exclusion are correct (geometry_ops.rs:1342-1345). Result List shape is correct.

### M-020: `interferes_with(snapshot, BodyId, BodyId)` targeted scalar form

- **State:** DRIFT
- **Failure mode:** F2 (same FK-transform-application drift as M-019)
- **Evidence:** `crates/reify-eval/src/geometry_ops.rs:1472-1492`
- **Blocks:** same as M-019
- **Note:** Self-pair short-circuits to `Bool(false)` (correct per PRD §5 acceptance).

### M-021: `min_clearance(snapshot, BodyId, BodyId) → Length`

- **State:** DRIFT
- **Failure mode:** F2 (same FK-transform drift)
- **Evidence:** `crates/reify-eval/src/geometry_ops.rs:1493-1514`
- **Blocks:** same as M-019
- **Note:** Self-pair → Undef (defensive; pinned by smoke test). Returns `Value::length(d)` — dimension-correct.

### M-022: Stdlib types registered as first-class language types

- **State:** DRIFT
- **Failure mode:** F1 (PRD assumes type-system facts that don't exist in code)
- **Evidence:** No `Type::Mechanism` / `Type::Joint` / `Type::Snapshot` / `Type::BodyId` / trait `DrivingJoint` exists anywhere in `crates/reify-types/` or `crates/reify-compiler/src/type_resolution.rs`. No stdlib `.ri` file declares these types (none of `crates/reify-compiler/stdlib/*.ri` mention kinematic types). The PRD's acceptance criterion "New stdlib types `Prismatic`, `Revolute`, `Coupling`, `Mechanism`, `Snapshot` registered and documented" is half-met: documented (✓) but not registered as language types (✗). Compile-time type discipline is per-name in `units.rs:81-125` for the three kinematic-query helpers only; everything else flows through as `Type::Map(...)`.
- **Blocks:** real `MotionValue<J>`-typed sweep ranges; nominal trait conformance for `DrivingJoint` (currently a per-name hardcoded set in `sweep.rs::driving_joint_kind`); any user-facing type error like "passing a Coupling to bind() is a type error" (the docs claim this is checked but runtime returns `Value::Undef`).
- **Note:** This is a structural decision pre-dating GR-001 — Reify's `Value::Map` + `"kind"` discriminant pattern is the de-facto stdlib "struct" representation for FEA loads/supports, materials, and now mechanism types. Likely related to GR-001 (structure-constructor runtime evaluation).

### M-023: GUI per-joint slider integration

- **State:** PARTIAL
- **Failure mode:** F4 (incomplete acceptance — only param-bound joints scrubbable)
- **Evidence:** `gui/src/panels/MechanismPanel.tsx` (full slider UI); `gui/src/stores/mechanismStore.ts` (optimistic-override + refresh); `gui/src-tauri/src/commands.rs:214-228` and `engine.rs:998+, 1011+, 1705+` (descriptor-extraction backend); `gui/src-tauri/src/types.rs:305-352` (wire format). Bind path: `bind(joint, param_ref)` only — literal-bound joints (`bind(joint, 0mm)`) show a "literal-bound" badge and are NOT scrubbable (`MechanismPanel.tsx:186-190`). Coupling/fixed joints show "coupling (derived)" / "fixed (no motion)" badges (correct — they have no motion variable).
- **Blocks:** PRD §7 says "the GUI viewer exposes a slider per motion variable". Implementation only exposes a slider per *driving-param-bound* motion variable. The dock-pickup example as written in the PRD (`let snapshots = sweep(m, x_axis, 0mm .. 500mm, steps: 50);`) cannot be scrubbed in the GUI because the joint isn't param-bound — the user must restructure source to use a `param` cell.
- **Note:** No 200ms-frame-budget perf test exists for `examples/kinematic/dock_pickup.ri`. Acceptance criterion "interactive at ≤200 ms per frame for ≤30 bodies" is unpinned. RAF coalescing is in place (`MechanismPanel.tsx:129-147`); the IPC round-trip cost is structurally similar to other set-parameter paths but not measured.

### M-024: Duplicate-solid detection by "referential identity"

- **State:** DRIFT
- **Failure mode:** F2 (PRD prose vs impl semantics)
- **Evidence:** `crates/reify-stdlib/src/mechanism.rs:457-460` ("v0.1 uses structural `Value` equality — the docs §13.2 spec says 'by referential identity'"). Stdlib reference §13.2 acknowledges the gap. Tied to M-008.
- **Blocks:** none in v0.1 — two structurally-identical solid calls (e.g. two `box(10mm, 10mm, 10mm)` calls) would erroneously be rejected as duplicates instead of treated as distinct bodies. Docs hand-wave with "use distinct constructor calls" — but distinct constructor calls with identical args are structurally equal.
- **Note:** Reify's `Value` model has no referential-identity primitive available at the surface. This is a Value-model architectural gap, not a kinematic-PRD-specific bug.

## Cross-PRD breadcrumbs

- **`docs/prds/v0_2/kinematic-constraints.md`** — owns the closed-chain solver (`loop_closure_solver.rs`, `loop_closure.rs`) and the planar/spherical/cylindrical/fixed/screw/gear/rack_and_pinion joints. Substantial v0.2 work has already landed in the same files this PRD touches. M-007 (closed-chain detection) is the clearest evidence that v0.2 has subsumed v0.1 without retiring the v0.1 contract. A separate audit batch will cover the v0.2 PRD per supervisor's note.
- **GR-001 (structure-constructor runtime eval)** — the language-level "Mechanism/Snapshot/Joint are types" promise in this PRD is the same shape as GR-001's "Material/LoadCase are types" promise. Both rely on undefined `StructureName(...)` runtime ctor evaluation. The kinematic PRD sidesteps it by using `Value::Map` with a `"kind"` discriminant from snake_case builtin dispatch — but this is precisely the design forced by GR-001 absence. The v0.2 fix for GR-001 may or may not retrofit the kinematic stdlib.
- **FFI task #2530** (referenced in `snapshot.rs:471, 551`) — when wired, would lift `center_of_mass` (M-016) and `bounding_box` (M-017) from PARTIAL to WIRED. Not audited here.
- **OCCT shape-transform** infrastructure — the M-019/M-020/M-021 DRIFT is unblocked by either a `GeometryOp::ApplyTransform` op + handle bookkeeping or per-pair on-the-fly OCCT transforms. Cited at `geometry_ops.rs:1336-1340` as out of scope for PRD task 8.
