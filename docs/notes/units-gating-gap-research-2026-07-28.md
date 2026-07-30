# Units-gating gap research — verified fact base and ratified design (2026-07-28)

Research record for the units-gating program (five PRDs: geometry LENGTH gate completion,
`reify check` truthfulness, ANGLE convention + units-surface UX, dimensioned-value
construction semantics, dimension-checked readers/solvers). Produced by a 10-agent
verification session on 2026-07-28; phase-1 facts measured at HEAD d2651bce16 (units-relevant
sources byte-identical to 7a21980c88), phase-2 facts at HEAD 1195020471. File:line anchors
are point-in-time — re-verify against current main before building on one.

Ratified decisions (Leo, 2026-07-28): hard-REJECT bare numbers at dimensioned positions
(no warn-and-convert; bare `0` included); strict DimensionVector equality EVERYWHERE
(resolves task 5627 as its candidate 4, reinstating PRD decision D5 of
real-dimensionless-unification); eval layer = soundness, compile slots = first-class UX;
kernel tripwire as cfg(debug_assertions) assertion naming op+field, not a hard gate;
source-text-driven closure-guard probe over GEOMETRY_FUNCTION_NAMES; add `Nm` torque unit
symbol + Energy/Torque teaching diagnostics; make displayed unit strings re-parse (accept
U+00B7 as unit-multiply, normalize curated labels to `^` form, round-trip property test).


All facts below were verified this session (2026-07-28, HEAD d2651bce16; all units-relevant
sources byte-identical to the brief's 7a21980c88) by five agents: route census (opus,
boundary-probe method), guard precedents, angle census, blast-radius measurement
(hand-verified), and empirical probes against a freshly rebuilt release binary.

## Verified fact base

### The un-gated LENGTH surface (complete route map)
- **R7 — raw-Value kernel passthrough: 46 `Value`-typed GeometryOp IR fields** (one-to-one
  verified against kernel `extract_f64` call sites): 38 length-semantic (box/cylinder/sphere/
  tube/cone/wedge/torus dims, half_space px/py/pz, fillet radius, chamfer d, chamfer_asymmetric
  d1/d2, shell thickness, thicken offset, zone_slab width, offset_solid/offset_curve distance,
  extrude/extrude_symmetric distance, pipe radius, rectangle/circle/ellipse dims), 3
  dimensionless (half_space normal), 2 angle (draft.angle fully un-gated — bare 5 = 5 rad ≈ 286°;
  circular_pattern.angle semi-gated via resolve_bare_angle), 3 gated-length (linear_pattern
  spacing ×3 via required_length_value). Coerced at `reify-kernel-occt/src/lib.rs:193`
  (`extract_f64` = `.as_f64()`), and at a SECOND kernel boundary `reify-kernel-fidget/src/kernel.rs:349`
  (sphere/box subset).
- **R1 — `eval_named_arg_f64` → f64 IR fields: 22 length positions** (translate dx/dy/dz,
  rotate_around px/py/pz, revolve ox/oy/oz, line_segment ×6, arc center ×3 + radius,
  helix radius/pitch/height). This is exactly pending task 5623's charter.
- **R2 — `eval_all_args_to_f64` variadic**: interp/bezier/nurbs coords + polygon 2D pairs
  (tasks 5658/5661).
- **R3 — `point3_components`** (no dimension check) feeding `decode_plane`/`decode_axis`
  origins + NurbsSurface control points. This is the `mirror(body, plane_yz(10))` bypass —
  scalar form gated (5214), value form silent. Verified end-to-end empirically (centroid
  x=20m vs x=0.02m, zero diagnostics).
- **R11 — `make_plane`/`make_axis` stdlib producers** accept any as_f64-able offset and emit
  `Value::Point` with bare `Real` components. Verdict from git archaeology: ACCIDENTAL gap
  (predates the units doctrine by 4 months), not deliberate design; a regression test
  (`plane_xy_real_zero_produces_dimensionless_origin`) locks the dimensionless path in place.
- **R4 — isosurface.iso**: length-semantic per IR doc, silently defaults to 0.0 on
  non-numeric (silent-default antipattern).
- **R8 — `decompose_transform_to_arrays`** → apply_transform/arbitrary_pattern translation:
  rejects bare Real but ACCEPTS Scalar{DIMENSIONLESS} — a latent hole if Real→Scalar{DIMLESS}
  unification lands.
- **R12 — `affine_translate`/`affine_map`** via `decompose_xyz3`: fully open (only requires
  components to share a dimension).
- **Gated today (exact)**: linear_pattern/linear_pattern_2d spacing (eval `required_length_value`
  + compile CheckableArg slots — the ONLY compile-layer length slots), mirror/circular_pattern
  SCALAR-form origins (eval `required_length_origin3`), arbitrary_pattern offsets (eval),
  query-side z/tol helpers (`resolve_length_scalar_arg`/`resolve_point3_length_arg` — gate ZERO
  GeometryOp construction positions), 4 selector tol args require ANGLE Scalar.
- **No compile-layer check on any primitive dimension** — arity only.

### `reify check` false-green (root cause resolved)
Two independent causes: (1) default `check` path runs plain `Engine::eval`;
`compile_geometry_op` — where ALL units gates live — is called only from `engine_build.rs`,
so gates never execute. (2) Even when check builds (geometric Conforms/DFM), exit code is
computed solely from ConstraintOutcome; diagnostics NEVER gate exit (only 2 special-cased
escalations). `reify eval` by contrast fails on any Severity::Error. The asymmetry is
undocumented, not a recorded design decision. Note `compile_geometry_op` builds IR only
(no kernel realization) — running it under check would be cheap.

### Angle class (parallel defect, 3 contradictory conventions)
- degrees-warned+converted: circular_pattern only (`resolve_bare_angle`).
- radians-silent: rotate, rotate_around, revolve, arc ×2 (via eval_named_arg_f64), draft
  (via R7 kernel passthrough).
- REJECTED (require ANGLE Scalar): 4 selector tol args — Contract-C-style discipline already
  ships for angles.
- DSL fully supports deg/rad suffixes; corpus overwhelmingly uses them. Exactly ONE live
  bare-radians reliance in examples (`feature_datum_axis.ri:24`, bare 2π in revolve).

### Migration blast radius (MEASURED, hand-verified)
- Real corpus-wide blast radius of hard-rejecting bare lengths: **31 call sites in 3 Rust
  test files** (let_scope_tests.rs 17, compile_api_tests.rs 13, fn_arg_trait_conformance 1),
  all trivial integer/one-decimal literals in r#"..."# fixtures; **ZERO in examples/**/*.ri,
  zero in all .ri fixtures, zero in golden sidecars** (goldens contain no op-call syntax at
  all — they're Debug-printed outputs).
- ~23 additional mechanical hits are deliberate negative/RED tests that must STAY bare.
- box() total call sites ≈ 933; bare-affected: 0 (all 13 mechanical hits were prose in
  assert messages). The "~370" figure was the anchoring PRD, irrelevant to units.
- Codemod judgment: pure textual append-mm; zero ambiguous cases in the true-positive set.

### Existing machinery to build on
- `arg_acceptance::accept_arg` + ArgSpec/length_spec: strict DimensionVector equality,
  3-state (Accepted/Undefined/Rejected), shared rejection wording.
- Compile layer: `builtin_arg_slots` → `CheckableArg{index,name,expected}` with real
  DimensionVector per slot, arity-keyed (5652). Gradualism: `plane_yz(...)` etc. are runtime
  builtins with static Type::Error/TypeParam → compile slots skip them (5662's record).
- Guard precedents: version_id_discipline_gate.rs (token scan + brace-scoped allowlist +
  escape comment + 7 seeded anti-vacuity self-tests); arg_slot_keys_are_registered_builtin_names
  (THE vacuity lesson: probe universe must be independent of the assertion target; probes all
  BUILTIN_NAME_FAMILIES × arity 0..=14); registry-completeness test w/ VARIANT_COUNT backstop
  (only ModifyKind has it — other 5 families' ALL_* arrays can silently go stale);
  corpus_no_bare_scalar.rs (corpus regex sweep); pdssentinel.rs (/audit window-scan detector).
- Contract C is prose only; PTODO-invisible; tracking task 5623 pending/low-priority; no
  test/audit/allowlist asserts completeness anywhere (verified by search).

### In-flight tasks
- 5623 (pending): R1 sweep — exactly the eval-chokepoint pattern, charter matches census.
- 5658 (pending, deps 5623): R2 curves; charter already amended to sharpen (not drop) the
  residual note and to re-read this research's PRD if landed.
- 5661 (pending, deps 5658): R2 polygon; curator note suggests folding into 5658.
- 5662 (pending): compile-layer origin slots for mirror/circular_pattern SCALAR forms;
  its "≥6 breaking call sites" is really ~3; its stated reasoning that 5214's eval gate
  covers value forms is FALSE (Finding 2), so the value-form hole is unowned.

## Candidate designs

### Q1 Policy: REJECT (warn diagnostic + hard error), not warn-and-convert
- Warn-and-convert requires choosing a default unit; any default re-creates the silent-guess
  class (bare 20 as mm silently REINTERPRETS any existing metre-intent call — behavior change
  with no error). Reject is honest, matches shipped Contract C UX, and the measured blast
  radius (31 test literals, 0 designs) makes it nearly free.
- No two-tier policy needed (new args vs existing): measured corpus is already clean.
- Angles: also reject bare (it's genuinely ambiguous — two live conventions in the same
  file). Deprecate circular_pattern's degrees-convert (keep warning for a window, then
  error). Migrate the one bare-2π example.

### Q2 Layer: eval is the soundness layer; compile is UX; PLUS fix check
- Keep the 5652/5662 doctrine: compile slots complement, never replace, eval gates
  (dynamic/gradual types are statically invisible).
- NEW: fix `reify check` visibility — two sub-options:
  (i) make check run the geometry IR-build step (compile_geometry_op — cheap, no kernel) and
      make Severity::Error diagnostics fail check's exit code;
  (ii) rely solely on compile-layer slots for check visibility (incomplete: value forms and
      gradual types stay invisible to check).
  Recommend (i); it's a separable work item (check semantics change).

### Q3 R7 gating point — options
- **A. Eval-layer gate at Value insertion** (extend Contract C chokepoint to all 38 R7
  length slots + R3 decode origins + R8/R12): best diagnostics (spans, established wording),
  smallest diff, no IR/kernel change. Failure mode: new op can silently skip the gate →
  needs the closure guard (Q6).
- **B. Typed IR**: replace `Value`-typed length fields in GeometryOp with a dimensioned
  newtype (private constructor, only obtainable via the gate). By-construction soundness;
  new-op failure mode = compile error (best possible). Cost: touches 46 IR fields, both
  kernels, the 20k-line tests.rs, characterization tests; golden churn LOW (goldens print
  Debug of IR — dimensioned fixtures already produce Scalar; but Real→newtype changes Debug
  text everywhere goldens exist).
- **C. Kernel-boundary strictness**: extract_f64 → extract_length that REJECTS Real/Int
  (requires Scalar{LENGTH}). Covers both kernels' 46+ sites. Failure mode UX: kernel errors
  have no .ri spans; late failure. Alone: bad UX. As TRIPWIRE behind A: catches any future
  un-gated route loudly instead of silently coercing.
- **Recommendation: A + C-as-tripwire now; B as the recorded endgame** (possibly folded into
  the Real→Scalar{DIMENSIONLESS} unification, decisions_real_dimensionless_unification).

### Q4 Value-form hole (R3/R11)
- Gate at the CONSUMERS (`decode_plane`/`decode_axis`/point3_components' length-position
  callers) with Contract C wording — covers every producer including user-built point3.
- ALSO fix the producers (`make_plane`/`make_axis`): require Length offset, update the
  locking regression test (it pins the accidental behavior).
- 5662's compile-layer origin slots stay valid as complement; its arity guard reasoning
  survives; its premise sentence must be corrected.

### Q5 Migration
- Big-bang per route family; no deprecation window needed (31 trivial test literals).
- Leave the ~23 deliberate negative-test fixtures bare (they become tests of the new gates
  where wording matches, or get updated expected-messages).
- Golden churn ≈ 0 for A (fixtures already dimensioned).

### Q6 Closure guard (replaces Contract C prose)
- **Core shape: behavioral boundary probe, not source grep** (R7 leaves no as_f64
  fingerprint; greps are structurally blind to it — this is the brief's own lesson).
- A cargo test that, for every geometry builtin name (universe = compiler's own
  GEOMETRY_FUNCTION_NAMES + stdlib families, NOT a hand list — the
  arg_slot_keys_are_registered_builtin_names lesson) × probed arity (0..=MAX like the 5652
  test), synthesizes a call with bare `Value::Real` in every numeric position, runs it
  through compile_geometry_op (precedent: compile_geometry_op_characterization.rs), and
  asserts the outcome is EITHER a rejection diagnostic (Contract C wording) OR the position
  appears in an explicit allowlist of deliberately-dimensionless positions (each entry
  individually justified: unit-vector, count, weight, knot, factor, index).
- Anti-vacuity: seeded self-tests (shrunken-allowlist harness fires; known-gated position
  fires; escape-hatch suppression) mirroring version_id_discipline_gate's 7 self-tests.
- Second independent layer: the C tripwire at both kernels' extract sites (a new un-gated
  route hits a loud kernel refusal at runtime + is caught by the probe test at CI time).
- Also: VARIANT_COUNT backstops for the 5 GeometryOp kind families that lack them (a new
  enum variant can currently dodge the registry-completeness test).
- Contract C doc comment shrinks to a pointer at the guard.

### Q7 Scope split
- **PRD 1 (core): LENGTH gate completion program** — policy decision, eval chokepoint for
  R7+R3+R8+R11+R12, kernel tripwire, closure guard, migration of 31 sites, Contract C
  rewrite. Consumes/rebases: 5623 (R1) and 5658+5661 (R2) become the first leaves (their
  charters already match the chokepoint pattern — keep their per-route scoping).
- **PRD 2 (small, separable): check-layer visibility** — check runs geometry IR build;
  Severity::Error fails check. Independent value even without PRD 1.
- **PRD 3 (sibling): ANGLE convention convergence** — reject bare angles, deprecate
  degrees-convert, migrate 1 example + kernel unit tests; shares arg_acceptance machinery.
  Sequenced after PRD 1 establishes the pattern (or folded in as a phase if Leo prefers).
- 5662 survives rescoped (premise correction + call-site list correction).

## RED-TEAM RESOLUTIONS (2026-07-28, opus adversarial pass — all findings evidence-backed)

1. BLOCKER: compiler desugarings emit bare Value::Real into gated slots — cylinder_centered
   dx/dy zeros (reify-compiler/src/geometry.rs:1502-1521), rounded_rect dz (:2525), revolve_full
   TAU angle (:2064-2066). PRD 1 needs a "dimension the synthesized literals" leaf BEFORE the gate.
2. BLOCKER: check has a THIRD false-green cause — `let _ = engine.build(...)` (main.rs:621)
   discards build diagnostics wholesale on the geometric-Conforms path. PRD 2 bigger than drafted;
   exit-code escalation is ad-hoc prefix matches; don't overload --strict (INDETERMINATE-only today).
3. MAJOR: for `reify check`, compile slots are the ONLY layer with teeth today (linear_pattern
   compile slot → check exit 1; mirror eval gate → check green). Re-weight compile slots to
   first-class deliverable, eval gate as dynamic backstop.
4. MAJOR: the 31-site figure is compile()-only tests — inapplicable to an eval-layer gate. Eval-layer
   migration ≈ 24 sites (topology_selectors.rs 6, unified_dag_geometry_executors.rs 5, shell_solve.rs 5).
   let_scope_tests.rs:1728 translate_non_geometry_target_uses_fallback INVERTS under a compile slot.
   Zero-design-breakage independently confirmed.
5. BLOCKER (scope): reify-stdlib flexures/joints parallel surface — length_si (flexures/common.rs:95),
   length_input (joints.rs:1181; screw lead :587, rack pitch radius :660, prismatic :539/:1763).
   prb_cantilever_beam(20,...) vs (20mm,...) = 1e9x silent spring-rate error, zero diagnostics.
   No builtin_signatures entry, no .ri signature — invisible to BOTH proposed layers. Own PRD or named deferral.
6. MAJOR: Q6 probe as drafted (variant-keyed) cannot be built (compile_geometry_op takes name-keyed
   CompiledGeometryOp; name→kind mapping is a 6400-line match). Buildable+better: SOURCE-TEXT-driven —
   compile(parse("structure S { let x = <name>(<bare args>) }")) over GEOMETRY_FUNCTION_NAMES (66 names,
   units.rs:21-86); catches desugarings (finding 1). Targets synthesizable without kernel (characterization
   harness precedent, empty named_steps — fast). Second universe needed: plane_*/axis_*/point3 + prb_*/joints.
7. MAJOR: C as hard kernel gate breaks hundreds of kernel-side tests (Value::Real op inputs: occt 546,
   ir 242, fidget 40) and kernel errors carry no span/arg. Make C a cfg(debug_assertions) assertion
   naming op kind + field, plus release diagnostics that name op+field.
8. MAJOR: GUI parameter editor (gui/src-tauri/src/engine.rs parse_value_string) injects bare Int/Real
   via edit_check; GUI has a DIVERGENT UNIT_TABLE (mm/cm/m/deg/rad — lacks in/km/ft/thou/user units).
   PRD 1 needs a GUI leaf: input-time rejection + table reconciliation.
9. OK: arithmetic-derived lengths safe — 2*10mm, sqrt(100mm*100mm), max(10mm,20mm) all → LENGTH Scalar.
   Policy sign-off: bare 0 NOT special-cased (0 rejected too; corpus already writes 0mm).
10. OK: Undef is fail-closed at required_length_arg (distinct message, documented); R7 UX improves.
    Nit: kind label prints "linear" not "linear_pattern".
11. OK: reject policy is spec-MANDATED — reify-language-spec.md:125 "There is no 'default unit
    system.'"; compat disclaimed :2575; breaking-change obligation :2597-2601 already satisfied by
    length_spec hint. No distribution channel; 4 prior big-bang precedents (real-dimensionless-
    unification.md:64 is the landing-shape template: migrate corpus first, then land error).
12. OK/MINOR: no live GeometryOp construction bypass (no serde; latent handle.rs:887 only). Fields
    pub, no non_exhaustive → invariant is convention-only. Partial-B (newtype only the 38 length
    fields + ctor family) is materially cheaper than full B.
13. MINOR: two check paths with different cost/visibility; PRD 2 must name which it changes.
14. MINOR: PRD-1 leaf signals must be phrased against `reify eval` unless PRD 2 lands first — decide explicitly.

## Known tensions to probe
- Is A+C-tripwire actually sound, or does C need to be a hard gate for routes A can't see
  (e.g. stdlib producers constructing GeometryOp-bound Values outside geometry_ops.rs)?
- Does the behavioral probe actually reach R7 positions (compile_geometry_op inserts raw
  Values without error today — the probe must assert POST-gate behavior; pre-gate it's the
  RED suite)?
- Can the probe synthesize valid calls for ops needing GeometryHandle targets/profiles?
- Undefined-value semantics: accept_arg maps Undef→quiet degrade; is that right for R7?
- Does rejecting dimensionless in R8 conflict with Real→Scalar{DIMENSIONLESS} unification?
- Performance: does the closure-guard probe need kernel realization (slow) or is IR-build
  enough (fast)? (Census says gates live in compile_geometry_op = IR-build only — fast.)

# PHASE 2 — NON-LENGTH/ANGLE DIMENSION SWEEP (2026-07-28 evening, HEAD 1195020471, 4 agents)

VERDICT: YES — a third defect class exists and it is the DOMINANT one: ~65-70 argument
positions across 12 dimensions (PRESSURE ~20, FORCE ~12, TIME 6, MASS_DENSITY 5, FREQUENCY 5,
MASS 3, + VELOCITY/ACCELERATION/MOI/FORCE_DENSITY/TEMPERATURE/AREA). Gated: 3 strict
(Contract A/B density ×2, faces_by_area) + 1 cross-context (spring_rate_for_lumped_dof,
modal_ops.rs:1300 — DimensionVector match, BEST extension precedent) + 1 by-arithmetic.

THREE SUB-CLASSES:
1. COMPILER — struct-ctor dimensioned-field slots validate NOTHING. Root:
   conformance/mod.rs:1691-1697 `general_leaf_param_family_is_validated` returns true only for
   Bool/Int/String/dimensionless-Scalar; dimensioned Scalar → `_ => false` (family held by 5465,
   ruling owned by task 5627, still pending). Probes: Steel(youngs_modulus: 200mm),
   Steel(density: "not a number") — silent at check AND eval. Constraint `magnitude > 0N` goes
   Indeterminate-satisfied on a bare/wrong value (StepForce probe) — constraints are NOT a
   backstop. Ctor conformance is Warning-only (mod.rs:32, δ flip pending).
   5627 CONTEXT: PRD D5 (real-dimensionless-unification.md:51) ruled bare-at-dimensioned a HARD
   ERROR; task 4318 shipped the opposite; no doc records the reversal. 5627 scoped to the
   dimensionless case only — the CROSS-DIMENSION + String cases fall through the same exclusion
   and are not named in its ruling question. All 4 of its candidate resolutions would fix them.
2. STDLIB/NATIVE READERS — ~50 dim-blind reader sites on Value::as_f64 (value.rs:1635, never
   checks dimension). 13 chokepoint helpers (scalar_si/material_field_si flexures/common.rs:109/126;
   cell_f64 dynamics/eval.rs:55; read_scalar_si ×3 near-duplicates input_shape.rs:51,
   trampoline.rs:139, modal_ops.rs; jointvalue_from_bound_value loop_closure.rs:691; etc).
   Flexures read E/yield with NO dimension check → Material(E: 200mm) → prb_cantilever_beam
   spring rate EXACTLY 1e12x wrong, zero diagnostics (probe-verified). min/max silently compares
   mismatched dims (numeric.rs:46-87); asin/acos/atan/atan2 accept dimensioned input (trig.rs).
   Safe wrappers EXIST (helpers.rs validate_dimensioned_scalar:229) — just not used at these sites.
   arg_acceptance.rs imported by ZERO files in compute_targets/**.
3. SOLVER INVERSIONS + DEAD WIRING — PointLoad(force: 5000N) (the units-CORRECT spelling)
   warns + contributes ZERO force in solve_elastic_static (extract_loads elastic_static.rs:4029
   matches only Value::Real) but 5000 N in solve_buckling (buckling.rs:730) — same source,
   different physics per solver. TractionLoad/BodyForce dead-wired (extract_loads dispatches
   only PointLoad/PressureLoad/Gravity by type_name) → silent zero. Bare density → 0.0 silently
   (extract_density:3979); malformed buckling load → 1.0 N sentinel (extract_total_load:714).
   PointLoad.force/PressureLoad.magnitude declared Real in fea_multi_case.ri:315 — retyping to
   Force/Pressure MUST land WITH the reader fix (inversely coupled; either alone worsens).
   Also: shell_voxel_size read as Value::Real but encoded Option<Scalar{LENGTH}> — user override
   silently discarded (shell_extract_compute.rs:550). Declared-only fields with zero readers:
   yield_stress/shear_modulus/CTE/thermal_conductivity (declared-only = fiction per memory).
   [CORRECTION 2026-07-29 (#5814) — that four-item list is wrong on three of its items;
   re-measured against main 916cffb7bd. Genuinely declared-only, zero PRODUCTION readers
   repo-wide: shear_modulus (materials_mechanical.ri:100) and thermal_expansion
   (materials_thermal.ri:41); both owned by #5801. (a) CTE is not an identifier that exists
   anywhere in the repo — `grep -rniw cte` over crates/ returns zero hits; the real field is
   thermal_expansion : ThermalExpansion. (b) yield_stress is NOT declared-only: it has 7
   production readers, all `material_field_si(material, "yield_stress")` — beam.rs:89,
   hinge.rs:98 and :252, compound.rs:102 and :281, notch.rs:135, prismatic.rs:110 in
   reify-stdlib/src/flexures/, each above its own file's #[cfg(test)] boundary (261/347/401/
   267/312). The zero-reader claim holds only when scoped to compute_targets/**. (c)
   thermal_conductivity has a live DSL reader — structural_physical.ri:150
   `constraint thermal_conductivity > 0W/(m*K)` — so its zero-reader claim holds only for
   Rust/host readers. Register: docs/reify-stdlib-reference.md, "Declared-only material
   properties" after §6.3.]

THE TRAP: ~25 stdlib params already declared Pressure/Density/Force/etc — declarations look
like guarantees (BOTH sweep agents initially misread them as gates) but enforce nothing at
ctor slots. Dimension safety exists in EXPRESSIONS (arithmetic is strict), not at
value-construction boundaries or native-reader boundaries.

DSL SURFACE (inventory agent): 49 nameable dimension classes, all usable as param types;
compound suffixes (7850kg/m^3, 200GPa, 9.81m/s^2) parse and are corpus-common (26 files carry
kg/m^3). Gotchas: 5N*m = ENERGY not Torque (need /rad); display middle-dot · not parseable
back (only ASCII *); NAMED_DIMENSIONS doc says 34, actually 51 names (stale) — reconciles
to the 49 classes above: 50 distinct constants (IMPULSE spelled twice as Impulse/Momentum)
minus 2 same-value alias pairs (STIFFNESS/TRANSLATIONAL_STIFFNESS,
ABSORPTION_COEFF/CURVATURE) = 48, +1 for the table-excluded DIMENSIONLESS = 49;
stdlib-reference documents Material fields as Real ("pending #3111") but live code already
dimensioned (stale doc). [CORRECTION 2026-07-29 (#5814) — this was true as measured on
2026-07-28 and is now FIXED, not merely stale: §6.1/§6.2 of docs/reify-stdlib-reference.md
were rewritten to match materials_mechanical.ri verbatim (including the `: Visual` base and
the appearance param the doc had omitted entirely), and that file now carries zero #3111
references. Do not act on this finding as an open gap.]

CONSEQUENCE FOR PRD PORTFOLIO: the 4-PRD split needs a 5th program (or a reshaped PRD 4):
"dimensioned-value integrity" = (a) ctor-slot conformance promotion (coordinate/fold into
5627's ruling — extend its question to cross-dimension + String), (b) dimension-generic
accept_arg adoption at the ~13 reader chokepoints, (c) coupled load-reader + .ri retyping
fixes (PointLoad/PressureLoad/TractionLoad/BodyForce), (d) closure guard generalized beyond
LENGTH (allowlist keyed by expected DimensionVector per position, not just LENGTH).
