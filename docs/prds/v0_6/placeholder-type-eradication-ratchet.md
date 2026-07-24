# Placeholder-type eradication ratchet — non-matchable marker types, owned retargets, PTYPE detector

**Milestone:** v0_6 · **Status:** active · **Date:** 2026-07-24 · **Approach: B + H**
(contract + two-way boundary tests; blast radius spans reify-core / reify-compiler /
reify-audit / reify-expr / stdlib `.ri` / tests-infra, and the type layer is a
load-bearing seam).

Implements **INV-SF-5 `placeholders-owned-and-loud`** (`docs/legibility/design-invariants.md`,
landing as task/5395). Authored from the 2026-07-24 silent-undef/placeholder eradication
investigation (language-review session a0d342d4). Decision record (Leo, 2026-07-24, final):
**invest in a small non-matchable marker/opaque type now** — "This is a busy codebase and
we're in this mess because of placeholders." The full typed-joints migration
(joints → typed StructureInstance, rejected by compliant-joints-flexures PRD §7.1) stays
rejected; the marker is a *static-layer* discriminator over the existing runtime
`Value::Map` representation.

## 1. Goal

After this PRD lands:

1. `reify check` on `flexure_compliance(5mm)` **exits nonzero** with a coded
   no-matching-overload diagnostic — where today it exits 0, prints
   `All constraints satisfied.`, and yields a sentinel-default record (probe re-verified
   2026-07-24; only the eval-time `W_FLEXURE_NON_JOINT_ARG` Warning stopgap fires).
2. `reify check` on `PiecewisePolynomialProfile(mechanism: 1.0)` emits the struct-ctor
   field-type conformance diagnostic at Error severity — where today it exits 0 with **no
   diagnostic at all** (probe re-verified 2026-07-24; conformance chokepoint = task 5306).
3. The **blanket type-placeholder escape**
   (`// ptodo:allow type-placeholder …` — 28 lines across kinematic.ri, trajectory.ri,
   dynamics.ri, flexures.ri, units.ri, trajectory_fns.ri, fdm.ri at authoring time) is
   **banned and extinct**: every surviving type-placeholder marker cites a live,
   non-terminal task in canonical `#NNNN` form, and a new `reify-audit --pattern PTYPE`
   detector hard-fails verify on any blanket escape or uncited type-placeholder marker.
   "No task yet owns the retarget" becomes impossible by construction.
4. `docs/notes/stdlib-real-placeholder-audit.md` (task 3090's six-bucket census) is
   refreshed: the never-owned bucket is closed, task-D's stale "pending" row corrected
   (superseded by #4229/#4485/later work), and the census records this PRD as the
   owner-of-record for the formerly phantom "kinematic-completion PRD" family.

## 2. Background

**The debt's shape.** The 3090 census closed every bucket that had a task owner
(#3111 12 sites, #3112, #3113, #3115 11 sites, #3116 24 sites, #3117/#3641). The
stdlib-surface-type-substrate PRD (task 4548 hub) closed the *blocked-surface-type*
bucket (#4574 stdlib DAG foundation, #4577 Pose3/LocationId, #4578 Part, #4579
ModalResult/records, #4580 dimensional sweep — all done). Its decision 7 explicitly
routed the remaining `joint-type` / `mechanism-type` / `body-id-type` / `body-type` /
`joint-value-type` family to "the kinematic-completion PRD" — **a PRD that was never
authored** (cited as future owner by three v0_3 PRDs; no file exists). The family's
markers were then escaped from the PTODO gate with the blanket
`ptodo:allow type-placeholder awaiting future type-system PRD` reason — the exact
laundering pattern INV-SF-5 bans. The escape reason also rotted onto non-type-placeholder
lines (units.ri:94 dead-alias cleanup, fdm.ri:105 normalization note), confirming that
freeform escape reasons decay.

**Why the placeholders are harmful (the anchor).** `flexures.ri` joint-type block
(~:211–238): `pub fn flexure_compliance(joint: Length)` is typed `Length` purely so the
accessor overload-matches a `prb_*` ctor result — the `prb_*` builtins have **no
compiler-side signature**, so their calls fall to the first-arg fallback
(`expr.rs:3540`) and are statically `Length`. Consequence: ANY `Length` (a bare `5mm`)
silently overload-matches and yields the sentinel-default record. Task 4547's eval-time
`W_FLEXURE_NON_JOINT_ARG` classifier is the stopgap; full static enforcement awaited the
phantom owner.

**Substrate that has landed since the placeholders were written** (what makes the
retargets cheap now — all verified on main 2026-07-24):

- **Nominal non-matchability is free.** Overload resolution for concrete non-generic
  params is exact `Type` equality (`resolve_function_overload`,
  `crates/reify-compiler/src/type_compat.rs:1176-1183`): `Scalar{LENGTH} ≠
  StructureRef("X")` → `NoMatch` → poisoning error. No new `Type` variant, no new
  grammar.
- **Builtin nominal return-typing has a template.** Task 4311 (mechanism β) landed
  `crates/reify-compiler/src/joint_signatures.rs`: `is_joint_typed_fn` +
  `joint_ctor_result_type` type the kinematic ctors (`prismatic`/`revolute`/… → kind
  StructureRefs, `mechanism`/`body` → `Mechanism`, `body_id_of` → `BodyId`), wired in the
  builtin-typing ladder at `expr.rs:3417`. Its module doc records why the static
  StructureRef / runtime `Value::Map` divergence is safe.
- **Marker structure defs already exist** for the kinematic family: `structure def
  Mechanism`/`BodyId` (kinematic.ri:30/:237, task 3845), reachable cross-module since the
  stdlib DAG foundation (#4574).
- **The struct-ctor conformance chokepoint exists** (struct-ctor-field-type-conformance
  PRD): named-arg ctor binding is not type-gated at bind time; the conformance checker
  (α #5302, in-progress) validates it separately, and δ #5306 (pending) flips it
  Warning→Error. Structure-field retargets get their loudness there.
- **Trait-typed params are wildcards** (`type_carries_trait_object` short-circuits the
  overload filter; `ty.rs:161-163` records call-site conformance as deferred) — so marker
  params must be **StructureRef**, never TraitObject, where rejection is the point.

## 3. Resolved design decisions

1. **Marker = StructureRef nominal checking; no new `Type` variant, no new syntax.**
   A marker type is an (empty) `structure def` whose name resolves via
   `resolve_type_with_aliases` → `Type::StructureRef(name)`. Non-matchability comes from
   the existing exact-equality overload filter; non-fabricability-from-literal comes from
   literals only ever typing as `Bool`/`Int`/`Scalar`/`String`. Rejected alternatives:
   new opaque `Type` leaf (Feature-style — strongest, but pays exhaustive-match churn
   across the workspace for no additional rejection power here); `DatumRef`-style alias
   onto `Type::Geometry` (would accept every geometry expression — too broad).
2. **Flexure joints get `structure def FlexureJoint : DrivingJoint {}`** (flexures.ri),
   and all 14 `prb_*` ctors (list: `flexures/diagnostics.rs:163-175`) get a compiler
   signature arm returning `StructureRef("FlexureJoint")`, structurally mirroring
   `joint_signatures.rs` (its own doc: the pattern is copied from `math_signatures.rs`).
   `flexure_compliance(joint: FlexureJoint)` then rejects every bare literal via
   NoMatch while `flexure_compliance(prb_*(…))` keeps matching. Declaring
   `: DrivingJoint` keeps the 4310 compile-time DrivingJoint-bound checks on
   `bind`/`sweep`/`dim` green for flexure joints ("it is just a Revolute" — the
   mechanism/body/snapshot surface stays uniform). Per-kind flexure markers
   (revolute-like vs prismatic-like) are a future refinement, out of scope.
3. **The NoMatch diagnostic gets a code.** The no-matching-overload poisoning error
   (`expr.rs:3017-3036`) is uncoded today; task α mints `DiagnosticCode`
   `E_NO_MATCHING_OVERLOAD` and tags the NoMatch base diagnostic. This serves INV-SF-6
   (`diagnostics-carry-codes`) and gives the negative fixtures a stable assertion target.
   α's fixture also asserts the Error exits nonzero at `reify check` (INV-SF-2); if the
   compile-phase exit gate misses this diagnostic class, fixing that seam is in α's
   scope.
4. **Structure-field retargets ride the ctor-conformance chokepoint.** Retargeting
   `mechanism : Real` → `Mechanism`, `joint_id : Real` → `BodyId`, `bodies :
   List<Real>` → `List<BodyId>`, `SweepDim.joint : Real` → its honest joint type makes
   the declaration nominal, but bind-time is unchecked — loudness arrives via #5306
   (Warning→Error). Task β therefore carries a **real dep edge on task 5306**; its
   negative fixtures assert the Error-severity conformance diagnostic + nonzero exit.
5. **Per-site honest typing, not blanket rename.** Known wrinkles the retarget task must
   respect: `ramp_profile` stores the *driving joint* (not a mechanism) in profile
   `mechanism` fields (accepted v1 semantic, `crates/reify-stdlib/src/dynamics/eval.rs:203-213`)
   — those sites get the type they actually receive; sites whose only expressible static
   type is a TraitObject (call-site-unenforced) must keep an eval-time misuse diagnostic
   (4309's `E_MECHANISM_NONDRIVING_JOINT` posture) so INV-SF-5's "loud" arm holds;
   examples passing dummy literals (`mechanism: 1.0`, tots_optimal_ptp.ri:58) migrate to
   real producers (`mechanism()`/joint ctors) and must stay eval-green.
6. **Marker fabrication is tolerated because it is dynamically loud.** A user *can*
   write `FlexureJoint()` (bare ctor) and statically satisfy the param; the runtime value
   is an empty map and the 4547 `W_FLEXURE_NON_JOINT_ARG` eval classifier (kept as
   defense-in-depth, not removed) surfaces the misuse. Statically-silent AND
   dynamically-silent is what INV-SF-5 bans; fabrication is dynamically loud. Making the
   marker non-pub would also hide the type name from the pub signature's callers —
   rejected.
7. **PTYPE lands zero-baseline, after the purge.** No baseline-ratchet machinery
   (PTODO's `ptodo-baseline.txt` pattern): the ordering α,β → γ (purge) → δ (detector)
   lands the detector against an already-clean tree, so any PTYPE finding is High from
   day one. Rationale: baseline plumbing is real complexity, and INV-SF-5 wants
   "impossible", not "absorbed". The cost — δ waits on the 5306 chain via β — is
   accepted (that chain is active, high priority).
8. **PTYPE grammar** (contract in §7.2): recognizes type-placeholder markers by the
   established in-file vocabulary — a `TODO(`/`FIXME(` label **ending in `-type`**
   (`joint-type`, `mechanism-type`, `body-id-type`, `body-type`, `joint-value-type`,
   `vec3-type`, …) or the literal token `type-placeholder` in a comment. Violations:
   `blanket-type-escape` (a line carrying both `ptodo:allow` and a type-placeholder
   marker/token — the escape does not exempt PTYPE), `uncited-type-placeholder` (marker
   with no `#NNNN` cite on the marker line), plus task-DB liveness (orphaned/unknown)
   **reusing PTODO's liveness lane via an extracted shared helper** — no copy-paste of
   the DB-resolution code. All High (hard gate), same fail-soft DB-degradation contract
   as PTODO §6.7.
9. **String-where-selector continuations are routed, not absorbed.** The FEA String
   fields (`TractionLoad.face`, `BodyForce.body`, …) are owned by
   `fea-load-support-selector-migration.md` residual leaves #5312/#5313 — referenced
   read-only, no task here; `direction : String` stays (mode, not topology — that PRD's
   Open-Q 5, reaffirmed). This PRD owns only the un-owned enum-shaped stragglers:
   `BucklingOptions.mode : String` (solver_buckling.ri:84) and
   `TensegrityWire/TensegritySurface.kind : String` (tensegrity.ri:143/:178) → task ε,
   dep on #5306 for conformance loudness.
10. **Wrong-value combinator bodies get a drift guard, not a rewrite.**
    option_recovery.ri / result.ri bodies stay typecheck-only placeholders (the
    reify-expr intercept is the production path). Coverage today is asymmetric: only
    Option-path `unwrap_or`/`get_or` have discriminating stdlib-compiled e2e tests; the
    Result family, `map_or`, `map_err`, and absent-key `get_or` would NOT catch an
    intercept regression (verified 2026-07-24: `result_combinator_eval_tests.rs` uses
    `EvalContext::simple` exclusively, so the `.ri` bodies are never even registered).
    Task ζ adds a discriminating stdlib-compiled e2e test per intercepted combinator —
    each asserting a value the placeholder body cannot produce.

## 4. Pre-conditions (G3 — all verified on main 2026-07-24 unless noted)

- Exact-equality overload filter + NoMatch path: `type_compat.rs:1136-1288`,
  `expr.rs:2958-3037`. ✓
- Builtin signature-arm template: `joint_signatures.rs` wired at `expr.rs:3417` (#4311
  done). ✓
- `structure def Mechanism`/`BodyId` + `DrivingJoint` trait hierarchy: kinematic.ri
  (#3845, #4310 done). ✓
- Cross-module stdlib type reachability: #4574 (done). ✓
- Same-module skeleton pre-pass makes a new `structure def` visible to same-file fn
  bodies (task 3895; flexures.ri:196-198 records it). ✓
- reify-audit per-pattern structure + PTODO hard-gate wiring: `Pattern` enum
  (`lib.rs:88`), bin allowlist (`bin/reify-audit.rs:264`), verify.sh pre-build
  (`verify.sh:1994-2114`), `tests/infra/test_reify_audit_ptodo.sh`, fixtures under
  `crates/reify-audit/tests/fixtures/ptodo/`. ✓
- Census doc exists: `docs/notes/stdlib-real-placeholder-audit.md`. ✓
- Struct-ctor conformance Error chokepoint: **#5306 pending** (α #5302 in-progress) —
  consumed via real dep edges from β and ε, not assumed landed.
- `docs/legibility/design-invariants.md`: on branch task/5395 at authoring time
  (merge-queued); normative for G7. Not a code dependency.
- No novel grammar anywhere: markers are `structure def` + type names in existing
  positions; fixtures use existing call syntax (`grammar_confirmed: true` for the whole
  batch).

## 5. Cross-PRD relationship (G4)

| Other PRD / doc | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `struct-ctor-field-type-conformance.md` | consumes | ctor field-type Error chokepoint (#5306) | that PRD | β, ε dep edges → 5306 |
| `docs/prds/v0_6/fea-load-support-selector-migration.md` | references (read-only) | FEA `String` field migration (#5312/#5313) + disallow-string | that PRD | no task here; §3.9 |
| `docs/prds/v0_6/stdlib-surface-type-substrate.md` (done) | succeeds | decision 7 routed the joint/mechanism/body-id/joint-value family to the phantom "kinematic-completion PRD" | **this PRD** assumes ownership | β, γ, η |
| `docs/prds/v0_6/mechanism-completion.md` (done) | consumes | `joint_signatures.rs` ladder arm pattern (#4311); `E_MECHANISM_NONDRIVING_JOINT` eval-guard posture (#4309/#4310) | that PRD (landed) | substrate |
| `docs/prds/v0_3/compliant-joints-flexures.md` | amends posture | §7.1 keeps `Value::Map` joints; this PRD adds the static marker WITHOUT reopening §7.1 | this PRD | α |
| sibling PRDs "eradicate silent undef" (INV-SF-1/2/6) and "consumption accounting" (INV-SF-3/4) | coordinates | if both add reify-audit detectors (diag-code detector may land there), each is a **separate detector module** — no shared-file collision; the shared PTODO liveness helper (§3.8) is this PRD's δ extraction, siblings may reuse | each its own | not yet authored at this PRD's authoring time |
| `docs/legibility/design-invariants.md` (task/5395) | implements | INV-SF-5 | that doc is normative | landing |
| `docs/prds/reify-audit-ptodo-detector.md` | consumes + extends | PTODO §8 grammar, §6.7 degradation, §6.8 escape semantics; PTYPE narrows the escape | PTODO PRD owns PTODO; **this PRD owns PTYPE** | δ |

## 6. Out of scope

- The typed-joints `Value::Map` → StructureInstance migration (stays rejected,
  compliant-joints-flexures §7.1). The marker is static-only.
- Per-kind flexure markers, `Union`-typed params, `TraitObject` call-site conformance
  enforcement (recorded deferred at `ty.rs:161-163`).
- FEA `String` selector fields + `direction : String` (owned/declined by
  fea-load-support-selector-migration).
- The `joint-value-type` retarget *execution* (`pub type JointValue = Real`,
  trajectory.ri:77) — its target type is genuinely undecided (per-joint scalar kind vs
  record); task η owns the decision as a [MILESTONE] gate. Under this PRD it merely
  becomes **owned and cited**.
- The diag-code (INV-SF-6 corpus-wide) detector — sibling PRD territory; α codes only
  the NoMatch diagnostic it touches.
- PTODO grammar changes (PTYPE is additive; `ptodo:allow` keeps its legitimate
  pattern-string use outside type-placeholder lines).

## 7. Contract (H)

### 7.1 Marker-type static semantics

- **Declaration:** an empty `structure def <Marker> [: <Trait>] {}` in the owning stdlib
  module. Resolution: `resolve_type_with_aliases` → `Type::StructureRef("<Marker>")`.
- **Producer typing:** every builtin that produces marker-valued runtime maps gets an
  `is_<family>_typed_fn` / `<family>_ctor_result_type` compiler arm in the builtin-typing
  ladder (before the first-arg fallback), returning the marker StructureRef. Runtime
  representation unchanged (`Value::Map`).
- **Rejection rule:** a concrete param typed `StructureRef(M)` matches an argument iff
  the argument's static type is exactly `StructureRef(M)`. Bare literals
  (`Scalar`/`Int`/`Bool`/`String`) can never match → `OverloadResolution::NoMatch` →
  poisoning **Error** carrying `E_NO_MATCHING_OVERLOAD`, message shape unchanged
  (`no matching overload for f(<ArgTypes>), candidates: …`), nonzero `reify check` exit.
- **Struct-field rule:** a `param f : StructureRef(M)` field is enforced at ctor time by
  the conformance chokepoint (#5306, Error) — this PRD does not add a parallel checker.
- **Fabrication:** bare marker ctor calls are statically legal; the producing family's
  eval-time misuse diagnostic (e.g. `W_FLEXURE_NON_JOINT_ARG`) must remain in place
  wherever fabrication would otherwise be silent.

### 7.2 PTYPE detector contract (normative; extends, never edits, PTODO §8)

Marker recognition (swept files = PTODO's set, §6.8; same precedence pass shape):
- **type-placeholder marker** ≙ a comment line containing `TODO(`/`FIXME(` whose label
  ends in `-type` (chars `[a-z0-9-]` before the suffix), OR containing the literal token
  `type-placeholder`.

| Kind | Trigger | Severity |
|---|---|---|
| `blanket-type-escape` | a line carrying `ptodo:allow` AND a type-placeholder marker/token — the inline escape does **not** exempt PTYPE | High |
| `uncited-type-placeholder` | a type-placeholder marker line with no `#NNNN` cite (PTODO §8.2 cite syntax) | High |
| `orphaned` / `unknown-id` | cited task terminal / absent — via the **shared** liveness helper extracted from `ptodo.rs` (single code path, both detectors) | High / Medium |

Exit semantics: High findings count into the existing exit-code mechanics (exit = High
count); the tests/infra gate hard-fails verify. Degradation: identical to PTODO §6.7
(missing/unreadable task DB → skip liveness with one stderr breadcrumb; structural lane
still runs). Registration: `Pattern::PType` variant + `"PTYPE"` bin token + default-sweep
dispatch (`is_none_or`, like PTODO) + no jcodemunch dependency.

### 7.3 Boundary-test sketch (both faces of each seam)

| # | Scenario | Precondition | Postcondition |
|---|---|---|---|
| BT1 | `flexure_compliance(5mm)` | α landed | `reify check` exit ≠ 0; diagnostic carries `E_NO_MATCHING_OVERLOAD`; message names candidates |
| BT2 | `flexure_compliance(prb_notch_circular(…))` | α landed | check exit 0; no NoMatch; compliance record populated (existing e2e stays green) |
| BT3 | full flexures example corpus (`examples/flexures/*.ri`) + mechanism/body/bind usage of a prb joint | α landed | all compile + eval green (4310 DrivingJoint bounds satisfied by `FlexureJoint : DrivingJoint`) |
| BT4 | `PiecewisePolynomialProfile(mechanism: 1.0)` | β + #5306 landed | conformance diagnostic at Error, exit ≠ 0 |
| BT5 | `mechanism: mechanism()` (real handle) in a profile ctor | β landed | conformance-silent; examples migrated off dummy literals stay eval-green |
| BT6 | a `// TODO(x-type): … // ptodo:allow type-placeholder …` fixture line | δ landed | PTYPE emits `blanket-type-escape` High; reify-audit exits nonzero |
| BT7 | a `// TODO(x-type, #NNNN): …` line citing a live task | δ landed | PTYPE emits nothing; PTODO liveness also passes (shared helper agreement) |
| BT8 | same line citing a done task | δ landed | `orphaned` High from PTYPE (and PTODO) — single shared liveness verdict |
| BT9 | tracked tree at δ's landing | γ landed first | zero PTYPE findings (zero-baseline invariant) |
| BT10 | each intercepted combinator (Option + Result families, `map_or`, `map_err`, absent-key `get_or`) with stdlib compiled | ζ landed | e2e asserts the intercept value; the `.ri` placeholder body's value would fail the assertion (discriminating by construction) |

## 8. Decomposition plan

Labels α–η; filed 2026-07-24 (planning_mode batch): α=#5408, β=#5409, γ=#5413,
δ=#5414, ε=#5411, ζ=#5410, η=#5412. All `task_kind="normal"`, `grammar_confirmed:
true`. Ordering (real `add_dependency` edges): α ∥ ζ ∥ (β after #5306); γ after
{α, β}; δ after γ; ε after #5306; η after β.

- **α — FlexureJoint marker vertical slice** (headline leaf; consumer: every flexures
  author + BT1–BT3). `structure def FlexureJoint : DrivingJoint {}`; flexure ctor
  signature arm (14 `prb_*` → `StructureRef("FlexureJoint")`, joint_signatures.rs
  pattern); retarget `flexure_compliance(joint: FlexureJoint)` (+ the module's related
  `Length`-placeholder accessor params, same block); mint + tag `E_NO_MATCHING_OVERLOAD`;
  ensure check-time nonzero exit for the coded Error; committed negative fixture
  (BT1) + positive corpus green (BT2/BT3); keep `W_FLEXURE_NON_JOINT_ARG`.
  Signal: BT1 + BT2 fixtures in CI; `reify check` on the negative fixture exits ≠ 0
  today→0 flip proven in-diff. Modules: reify-core (diag code), reify-compiler
  (signature arm + flexures.ri), fixtures. Deps: none.
- **β — kinematic-family struct-field retargets** (leaf; consumer: trajectory/dynamics
  authors + BT4/BT5). Retarget trajectory.ri / dynamics.ri / kinematic.ri placeholder
  fields to their honest nominal types (`mechanism : Mechanism` where a mechanism is
  actually stored; the profile-field driving-joint sites per §3.5; `joint_id : BodyId`;
  `bodies : List<BodyId>`; `SweepDim.joint` honest type); migrate example dummy literals
  to real producers; negative fixture BT4 (Error + exit ≠ 0), positive BT5; examples
  eval-green. Signal: BT4/BT5 fixtures in CI + examples green. Deps: **#5306** (real
  edge). Files: crates/reify-compiler/stdlib/{trajectory,dynamics,kinematic}.ri (+
  examples cascade).
- **γ — blanket-escape purge + census refresh** (leaf; consumer: δ + census readers).
  Remove every `ptodo:allow type-placeholder` escape; re-cite each surviving
  type-placeholder marker in canonical `TODO(<label>, #NNNN)` form to a live owner
  (joint-value-type → η's id; ids injected into this task's description at decompose);
  delete the dead `Velocity` alias (units.ri:94 — its own comment records removal-safe;
  verify by repo grep) and drop that TODO; reword fdm.ri:105's mislabeled β-phase note to
  non-marker prose; refresh `docs/notes/stdlib-real-placeholder-audit.md` (task-D
  supersession, never-owned-bucket closure, owner-of-record note). Signal: `git grep
  'ptodo:allow type-placeholder'` returns empty; census doc diff; PTODO gate stays green.
  Deps: α, β.
- **δ — PTYPE detector + hard gate** (leaf; consumer: verify pipeline + every future
  author). `crates/reify-audit/src/ptype.rs` per §7.2; extract the PTODO liveness lane
  into a shared helper consumed by both detectors (no copy); `Pattern::PType` + bin
  token + default-sweep dispatch; detector fixtures (BT6–BT8 shapes) under
  `tests/fixtures/ptype/`; tests-infra gate coverage — if a new `tests/infra/test_*.sh`
  is added, its `run-all-classification.manifest` bucket row lands in the **same diff**
  (gate-test drift-guard rule); zero findings on the real tree (BT9). Signal:
  `reify-audit --pattern PTYPE` exits nonzero on a blanket-escape fixture and zero on
  the tree; verify runs it as a hard gate. Deps: γ.
- **ε — mode/kind String→enum retargets** (leaf; consumer: buckling/tensegrity
  authors). `BucklingOptions.mode : String` → enum (values today: `"shift_invert"` etc.);
  `TensegrityWire.kind` / `TensegritySurface.kind` → enum (internal records — the task
  may keep String where a field is builtin-populated-only and record why, per-site
  honesty over blanket conversion); negative fixture asserting the conformance Error on
  a wrong-typed ctor arg. Signal: fixture in CI + corpus green. Deps: **#5306** (real
  edge). Files: crates/reify-compiler/stdlib/{solver_buckling,tensegrity}.ri.
- **ζ — combinator-intercept drift guards** (leaf; consumer: reify-expr maintainers /
  BT10). Stdlib-compiled discriminating e2e per intercepted combinator (Result
  `unwrap_or`/`or_else`/`fallback`/`is_ok`/`is_err`/`ok_or`, `map_or`, `map_err`,
  absent-key `get_or`) in crates/reify-expr/tests — each choosing inputs where the
  placeholder body's value ≠ the intercept's value. Signal: new tests in the merge gate;
  each demonstrably RED under intercept-removal (construction argument in test doc
  comments). Deps: none.
- **η — [MILESTONE] joint-value-type owner** (gate task; consumer: γ's cite +
  the future kinematic-value design). Decide `JointValue`'s target type (per-joint
  scalar kind vs record vs union — needs the typed mechanism/joint surface from β as
  substrate), then retarget `pub type JointValue = Real` + its `List<JointValue>`
  consumers. Until executed it is the **live cited owner** of the joint-value markers.
  Deps: β. Priority low; milestone gate — curator routing, not urgency.

**G7 walk (advisory, author-time):** α codes its diagnostic (SF-6 ✓) and asserts
nonzero exit (SF-2 ✓); β/ε route loudness through the 5306 Error gate and keep eval-time
diagnostics where static enforcement can't reach (SF-5 ✓); γ removes escapes without
adding silent defaults and rewords rather than launders (SF-5 ✓); δ degrades fail-soft
with a stderr trace (SF-1-adjacent observable trace ✓) and extracts the shared liveness
helper rather than duplicating it; ζ converts coincidentally-green coverage into
discriminating coverage (SF-5's drift-guard arm ✓); η prevents a cite-less placeholder
from surviving (SF-5 ✓). No waivers needed.

## 9. Open questions (tactical)

1. **Exact per-site types in β** (profile `mechanism` fields: `Mechanism` vs the
   driving-joint type per §3.5's v1 quirk; `SweepDim.joint`'s honest type given the
   TraitObject-wildcard caveat). Decide in-task from what each site actually receives;
   keep eval guards where static typing can't bite.
2. **ε's internal-record fields** — whether `TensegrityWire.kind` (builtin-populated,
   never user-constructed) warrants the enum or a recorded String-is-fine rationale.
   Decide in-task.
3. **δ's infra-test home** — extend `test_reify_audit_ptodo.sh` vs a new
   `test_reify_audit_ptype.sh` (+ same-diff manifest row). Either is acceptable.
4. **`FlexureJoint` name** — vs `CompliantJoint`/`PrbJoint`. α may rename at
   implementation if flexures.ri conventions argue for it; the contract only requires
   one nominal marker.
