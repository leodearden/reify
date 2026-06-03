# Capability Manifest — mechanism-completion

Per-leaf capability→evidence bindings (mechanizes G3 + G6). Any **FAIL** blocks queueing.
PRD: `docs/prds/v0_6/mechanism-completion.md`. Batch `stdlib-mechanism-2026-06-03`.

This PRD is doc-reconcile + compiler-signature + diagnostics work — **no numeric
capabilities**, so the numeric-floor binding is N/A throughout. The dominant risks are
(1) diagnostic-emission **wired-on-main** (a typed code reserved but never emitted is the
exact `MechanismDuplicateSolid` failure mode) and (2) the **anti-inversion** premise that
the FK-interference core is on main (3906), not a downstream dependency this PRD must
build.

## δ — diagnostic-emission seam + E_MECHANISM_DUPLICATE_SOLID

| Capability | Evidence | Verdict |
|---|---|---|
| Reserved code exists | `DiagnosticCode::MechanismDuplicateSolid` — `crates/reify-core/src/diagnostics.rs:789` | **PASS** |
| Detection exists | duplicate-solid scan writes `Value::Map` `error="duplicate_solid"` — `crates/reify-stdlib/src/mechanism.rs:469,357` | **PASS** |
| Field-population (anti-Undef) | δ must make `EvalResult.diagnostics` carry the typed code on the production eval path (not a `tests/` helper). Today the only surface is the Map `error` field → δ adds the real emission. | **PASS-on-build** (signal asserts `EvalResult.diagnostics` contains the code) |
| Wired-on-main (anti-orphan) | Emission boundary = the eval pipeline that owns `EvalResult.diagnostics` (the same surface `reify check` reads). §10 open Q confirms the check-vs-build boundary at impl time. | **PASS** |
| Grammar-fixture | No novel syntax (existing `mechanism()/body()` builtins). | **N/A** |

## α — L1 runtime non-driving-joint guard + E_MECHANISM_NONDRIVING_JOINT

| Capability | Evidence | Verdict |
|---|---|---|
| Discriminator substrate | `is_joint_value` single-source over `JOINT_KINDS` (task 2632) — `is_driving_joint` is the DrivingJoint-subset sibling. | **PASS** |
| New reserved code | `E_MECHANISM_NONDRIVING_JOINT` added to `reify-core/diagnostics.rs` (reserve-then-emit, mirroring `MechanismDuplicateSolid`). | **PASS-on-add** |
| Field-population | Guard returns a real diagnostic via δ's seam (not silent `Undef`); reify-eval test asserts both directions. | **PASS** (deps δ) |
| Wired-on-main | `bind`/`sweep`/`dim` are live eval builtins (`snapshot.rs:35`, `sweep.rs:35/60/129`). | **PASS** |
| Grammar-fixture | `bind(couple(prismatic(...), -1.0), 5mm)` uses existing call syntax. | **N/A** |

## β — L2 constructor signature family (full §13 vocabulary)

| Capability | Evidence | Verdict |
|---|---|---|
| Signature-family template | `is_math_typed_fn` + `math_fn_result_type` (`crates/reify-compiler/src/math_signatures.rs`); `units::is_geometry_query` family wired at `expr.rs:1570`. β is a structural copy. | **PASS** |
| Target nominal types exist | `Prismatic/Revolute/Cylindrical/Planar/Spherical/Coupling/Fixed/Mechanism/Snapshot/BodyId/SweepDim` declared in `kinematic.ri` (3845); `JointBinding`/`Twist` added by γ. | **PASS** (JointBinding/Twist via γ) |
| Field-population (type, not Undef) | β sets the builtin-call cell *type* (compile-time), not a runtime value — analog to the geometry-query arm that avoids the first-arg fallback (`expr.rs:1585`). Signal = compiler cell-type test. | **PASS** |
| Disjointness (anti-mismatch) | New family pinned disjoint from math/geometry/dynamics families via the `units.rs` `*_query_names_are_disjoint_from_other_families` test pattern. | **PASS-on-add** |
| Grammar-fixture | No novel syntax (types existing builtin calls). | **N/A** |

## γ — L2 trait Joint hierarchy + JointBinding/Twist decls + DrivingJoint-bound enforcement

| Capability | Evidence | Verdict |
|---|---|---|
| Trait-supertrait grammar | `trait DrivingJoint : Joint {}` parses + `reify check` exit 0 — fixture `/tmp/prd-gate-fixtures/mech-trait-hier.ri` (precedent `materials_electrical.ri:51`). | **PASS** (G3-confirmed) |
| `structure def X : Joint` grammar | `structure def Coupling : Joint {}` in same fixture, exit 0. | **PASS** |
| Conformance-check substrate | `satisfies_trait_bound` (`entity.rs:3732`) + `check_type_param_bounds` (`entity.rs:3639`) — reused for the builtin-arg `DrivingJoint` check (same machinery the generics PRD γ/4232 reuses). | **PASS** |
| Two-way boundary test (H) | Signal pins **both** directions: reject `bind(couple(...),…)` naming `Coupling`; accept `bind(prismatic(...),…)`. | **PASS-on-test** |
| Wired-on-main | Builtin-call typing arm in `expr.rs` (consumes β); `kinematic.ri` is registered in `stdlib_loader.rs`. | **PASS** |

## L3 — generic Coupling<P> / MotionValue<J> (DEFERRED forward-stub)

| Capability | Evidence | Verdict |
|---|---|---|
| Generic-fn grammar | `fn couple<P: DrivingJoint>(...)` + `guf-*.ri` fixtures parse 0-ERROR (generics PRD 4230). | **PASS** (grammar only) |
| Generic-fn semantics | Generic user fns unimplemented on main (`FnDef.type_params` parsed-not-read, `functions.rs:16`). Dimension-param coherence needs `Type::ScalarParam`/dimension-kinded params. | **PREREQ** — deps **4232** (generics γ), **4235** (generics ζ) |
| Status | Filed `deferred`, **not** flipped to `pending`. Forward-stub. | **DEFERRED** (by design) |

## θ — §13 doc-reconcile + completion + gap-register flip

| Capability | Evidence | Verdict |
|---|---|---|
| Anti-inversion (FK premise) | FK-interference core on main: `engine_build.rs:1998,2374` (post_process_kinematic_queries) + 3906 (done, `ApplyTransform` pre-probe) + `mechanism_interference_smoke.rs::fk_posed_cubes`. θ documents this, does not depend on building it. | **PASS** |
| Swept-example ownership | `dock_pickup.ri` swept `.map(interferes)` + e2e owned by KCC **3848** (pending); θ depends-on it. | **PASS** (dep wired) |
| Method-call-syntax fiction | `.map`/`.windows`/`.norm` appear only in comments as *non-expressible* (`modal_analysis.ri:57`); θ marks §13.6 forms non-Reify. | **PASS** |
| Same-file-lock hygiene | All §13 prose edits consolidated into this one leaf (avoids 3-way `reify-stdlib-reference.md` lock contention). | **PASS** |
| Grammar-fixture | Doc-only. | **N/A** |

## Cross-batch dependency wiring (decompose-time)

- L3 → `4232`, `4235` (generics PRD, external same-project bare deps).
- θ → `3848` (KCC-ι, same-project bare dep).
- Intra-batch: α→δ; β→γ; γ→δ,α; θ→α,β,γ,δ,3848.
- **3844 re-scope flag:** `update_task(3844, append note)` that 3906's `ApplyTransform`
  path may supersede its `try_resolve_snapshot_body` + `distance_with_transform` (3841)
  FFI approach — a curatorial note, **not** a leaf.
