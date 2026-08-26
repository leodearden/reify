# Instantiation value-flow integrity

**Status:** active — contract PRD. Authored 2026-08-26 (assembly-derivation design
session, Leo + Claude; groundwork by agent team).
**Code anchors** verified against main `041554eae524` (2026-08-26). Main moves
fast — cite-by-symbol; re-locate lines at implementation time.

## 1. Goal

A constructor argument supplied at any instantiation site drives the child's
ENTIRE subtree — value cells, sub placements, descendant constructor args, and
realized geometry — from ONE effective valuation. When it cannot (an argument
evaluates undef; geometry fails to realize), the failure is loud: a coded
Error-severity diagnostic that fails `reify check`, never a green run over
phantom or default-shaped geometry.

User-observable on landing (the #6592/#6586 acceptance, restated):

- The #6592 probe (committed fixture,
  `tests/prd-gate/fixtures/instantiation_value_flow_probe.ri`): `reify build`
  STEP output shows `m_ovr.marker` at x = 377 (= 300 + 77), an `m_ovr.leaf`
  cylinder of half-length 38.5, `m_spec.marker` at 655 — today 310 / 5 / 610.
- A `FairleadPair(side: -1)`-shaped fixture: all pair children at mirror-twin
  stations in `reify build` STEP output and GUI `mesh_stats`.
- The #6586 repro: `b_one` / `b_onear` / `b_chain` realize with lengths
  0.09 / 0.10 / 0.12 m.
- A model whose constraint's backing geometry could not realize: `reify check`
  exits nonzero with a coded error naming the sub and cause — today it prints
  "All constraints satisfied."
- printer_v01 follow-through (dogfood-side, not a task deliverable): delete
  `FairleadPairMirrored` / `CapstanUnitMirrored` (pass `side: -1`), delete the
  `lead_bk_*` continuation segments (restore `span_bu` / `span_au: 430mm`).

## 2. Background

Family history: #3814 (cross-sub `self.sub.body` ignored overrides — fixed for
the manual-lift idiom) → #4147 (overrides dropped on the `at`/auto-surface path
— fixed ONE level deep) → #5360 (chained member reads — fixed via the
instance-scope phase-1.5 dependency-ordered elaboration in
`elaborate_child_instance_nested`) → the 2026-08-25 dogfood findings #6586
(a ctor arg sourced from a sub-member read evaluates undef in the child,
silently) and #6592 (ctor args never reach sub placements or deeper ctor args
at ANY site; value cells carry them anyway, so values and geometry disagree and
`reify check` greens phantom state).

Groundwork clarifications (2026-08-26, agent team; all cite-checked):

- **The pose crux.** There is exactly one evaluation site for concrete sub
  poses: `walk_placed_realizations` → `eval_sub_pose`, evaluated against the
  single global ValueMap. Pose `CompiledExpr`s are compiled in the DECLARING
  template's scope, so their `ValueRef`s are template-scoped; the walk's
  instance `path_prefix` is used only for `entity_path` formatting. No instance
  rescoping of pose evaluation exists anywhere. The root template escapes only
  because the root's instance IS its template scope. `at auto` is analogous
  (`auto_pose_cell(template, sub)` — template-keyed).
- **The geometry-plane one-level boundary is a DOCUMENTED v0.1 scope cut**, not
  an accident: `seed_cross_sub_named_steps` rustdoc ("one level of override
  depth only"), the empty `child_named_steps` map in
  `realize_sub_override_handles`, and the pinning test
  `cross_sub_nested_sub_in_override_path_produces_compile_error`
  (`crates/reify-eval/tests/cross_sub_geometry_e2e.rs`), whose own doc says to
  re-baseline it when nested overrides land. This PRD completes that cut and
  removes its silent manifestations.
- **Task 4147's "threads through multiple nesting levels" is values-plane /
  direct-child-op-args only.** Its tests use childless leaves; the
  "two levels" test (`nested_constructor_arg_threads_through_two_levels`)
  asserts on cells with a kernel-less engine. No test covers a child WITH subs
  under an override, a sub pose reading an overridden param, depth-2 ctor args,
  or a derived let feeding an op arg under override. #6592 is not a regression;
  it is the documented boundary plus never-implemented instance-scoped pose
  evaluation, mis-summarized as complete.
- **The value domain already works.** Since #5360, phase-1.5 commits correct
  instance-scoped cells for the whole plain-sub subtree
  (`Probe.m_ovr.leaf.k = 77mm` is right in the values map). The defect is that
  the geometry/placement/constraint pipelines never read them.
- **Specialization and keyed arms share the path.** `sub x : T { p = v }`
  lowers body overrides into `SubComponentDecl.args` (task 4694); keyed per-key
  overrides ride the same args into `elaborate_child_instance`. Every fix below
  covers all three arms with no separate treatment.

The orchestrator's architect has already planned #6592 (planning spawned #6598,
which records the value-axis approach and splits the geometry-HANDLE axis out
as its own low-priority task) and #6586 carries routing. This PRD therefore
ADOPTS the live tasks rather than re-filing them, adds the leaves no task owns,
and sequences the whole set.

## 3. Consumers (G1)

- **printer_v01 dogfood** (live user surface): the forced duplicates
  `FairleadPairMirrored` / `CapstanUnitMirrored`; the silently-inert
  `RailTendons(span_bu:)` overrides (two rear strands have rendered 30mm short
  of their pulleys since rear-routing v1).
- **assembly-derivation-toolbox PRD**
  (`docs/prds/v0_6/assembly-derivation-toolbox.md`, paired commit this
  session): its Layer-1 derived-sub instantiation ("prototype re-evaluated with
  merged args") presupposes exactly this value flow; hard dependency.
- **indexed-sub-instantiation** (`docs/prds/v0_6/indexed-sub-instantiation.md`,
  #5483 γ): per-index instances with per-element args ride the same
  elaboration machinery; γ consumes Stages 1–2 below as its expansion-helper
  substrate.
- **uniform-member-access** (#5426): lazy let-instance realization reuses
  `elaborate_child_instance` semantics.
- Every existing "single source" threading chain (the task-4147 idiom): CoreXY
  `tendon_outer_z` → DriveTendons `z_a` → TendonQuad/RailTendons/CarriageIdlers
  — all currently coincidentally correct only where threaded values equal
  defaults.

## 4. Contract (B+H)

- **C1 — One effective valuation.** For every instantiation site — bare
  `sub x = T(args)`, specialization `sub x : T { p = v }`, keyed member, and
  (when #5482 lands) indexed element — the child's effective valuation is
  `template defaults ⊕ site args`, and that one valuation drives all three
  planes: (a) the instance's value cells; (b) the instance's sub placements and
  descendant constructor args, recursively; (c) realized geometry.
- **C2 — Values ≡ geometry.** Any member readable on an instance corresponds to
  the geometry actually realized for that instance. No plane may diverge
  silently.
- **C3 — Dependency-ordered elaboration.** Template-scope evaluation orders
  parent lets and sub elaborations by dependency (the phase-1.5 semantics
  ported up), so a ctor arg reading a sibling sub's member sees the committed
  value, never declaration-order undef. (#6586.)
- **C4 — Loud failure.** An undef constructor argument, and any child whose
  geometry fails to realize, emit an Error-severity coded diagnostic naming the
  sub and the argument/cause, record an `UndefCause`, and fail `reify check`
  via the severity-exit gate (the task-4458 house pattern; INV-SF-2 — no
  per-code escalation lists). Constraint verdicts keep never-false-Violated
  (INV-SF-4): affected constraints stay Indeterminate with an attributable
  cause; the RUN fails because the error was emitted.
- **C5 — Instance-scoped constraint truth.** A constraint declared in a child
  definition is checked against each instance's effective valuation, not only
  the template defaults — an override that violates a child constraint turns
  `reify check` red.

**Mechanism (resolved: Shape B — recursive instance-scope elaboration).**
Evaluated against per-instantiation template specialization ("monomorphization",
Shape A) and a hybrid (C): A is rejected as the general mechanism — args are
runtime values, so specialization would re-mint templates on every threaded
`edit_param`, converting value edits into topology changes (fingerprint churn,
dep-structure rebuilds), and template count blows up under recursion and
indexed subs; C inherits A's mint problems plus a two-authorities seam. B lands
at the exact defect sites with existing precedents for every sub-mechanism:
the phase-1.5 overlay projection (value plane, landed), a per-instance overlay
threaded through the surfacing walk into `eval_sub_pose` (placement), a
recursive generalization of `realize_sub_override_handles` reading
instance-path cells with a recursively-seeded named-steps map (geometry), and
`map_value_refs`-rescoped, runtime-materialized constraint nodes (constraints —
precedent: the forall per-element emission ledger). Value edits stay
non-structural; entity identity stays stable. **Shape A survives as the test
ORACLE:** acceptance asserts instance output ≡ a hand-specialized twin (the
mechanism the dogfood duplicates prove works end-to-end).

## 5. Boundary-test sketch

| # | Scenario | Precondition | Postcondition |
|---|---|---|---|
| B1 | `Mid(s: 77mm)` marker placement | probe fixture | STEP/mock-op shows marker at parent_x + 77, not + 10 |
| B2 | `Mid(s: 77mm)` grandchild ctor arg | probe fixture | a k = 77 cylinder exists (half-length 38.5) |
| B3 | `sub m : Mid { s = 55mm }` spec arm | probe fixture | marker at 655 — first-ever specialization-arm geometry test |
| B4 | value-cell relay (regression) | probe fixture | `ovr_probe` = 77 — value plane stays correct |
| B5 | `FairleadPair(side: -1)` | pair fixture | mirrored stations in STEP + GUI mesh_stats |
| B6 | ctor arg from sibling-sub read (#6586) | b_one/b_onear/b_chain | lengths 0.09/0.10/0.12 m; geometry realizes |
| B7 | genuinely-undef ctor arg | contrived undef source | coded Error names arg + sub; check exits nonzero; UndefCause recorded |
| B8 | unrealized backing geometry under a constraint | forced realization failure | constraint verdict PINNED Indeterminate-with-cause (never-false-Violated, INV-SF-4 — the test asserts the verdict, not just exit 1 + "error"); run fails via severity gate |
| B9 | override violates child constraint | `Bar(length: 0mm - 5mm)` vs `constraint length > 0mm` | VIOLATED (today: green — checked at template default) |
| B10 | derived let feeding an op arg under override | new repro (args-only-overlay gap) | op arg reflects the overridden derivation |
| B11 | no-args sub sharing (regression) | existing tests | template-handle fast path unchanged |
| B12 | one-level override (regression) | 4147 tests | `at_placed_constructor_override_surfaces_re_realized_geometry` stays green |
| B13 | nested-override compile-error pin | `cross_sub_geometry_e2e.rs` | re-baselined from expect-error to expect-nested-geometry |
| B14 | warm re-elaboration | collection count edit | edited N picks up per-instance valuations, no stale defaults |
| B15 | verdict-pin corpus | best_practices gate | `EXPECTED_INDETERMINATE` re-baselined for any flips |
| B16 | oracle equivalence | any override fixture | instance output ≡ hand-specialized twin (Shape-A oracle) |

## 6. Resolved design decisions

1. **Mechanism = Shape B** (§4), staged: Stage 0 ordering (#6586) → Stage 1
   placement overlay → Stage 2 geometry recursion → Stage 3 instance
   constraints. Each stage independently green and valuable (~5–9 wk total).
2. **Adopt, don't re-file**: #6586 = leaf α, #6592 = leaf β (descriptions
   updated at decompose to cite this PRD; #6592's check-truthfulness acceptance
   clause moves to leaf γ). #6598 (geometry-handle axis) and #5868 (recursion
   budget) stay independent satellites — referenced, not duplicated.
3. **Loud-failure shape** (γ): new `UndefCause` variant + new Error-severity
   `DiagnosticCode`s replacing the code-less `Diagnostic::error` on the
   per-instance re-realization failure path + `reify check` converging on the
   severity-exit house pattern. Not a per-code escalation list; not a
   verdict-semantics change.
4. **Instance-scoped constraints (δ) are IN this PRD** (not deferred to the
   derivation toolbox): the vacuity is live today (any `RailTendons(z_a: …)`
   override is checked against defaults), the machinery is precedented
   (runtime-materialized constraint nodes via the forall emission ledger;
   `map_value_refs` rescoping), and the toolbox PRD needs it for image
   constraints.
5. **The probe fixture is the contract's executable form**, committed with this
   PRD at `tests/prd-gate/fixtures/instantiation_value_flow_probe.ri`, together
   with the three D3-session baseline fixtures
   (`adv_ivf_undef_ctor_arg_check_silent.ri`,
   `adv_ivf_undef_flow_constraint_indeterminate_exit0.ri`,
   `ivf_override_violates_constraint.ri` — B7/B8/B9 pre-fix baselines, each
   header-documented with its verified today-output). Rust tests that read any
   of them register the basename in `_RUST_COUPLED_RI_FIXTURES` in the same
   diff (leaf ζ). D3 disposition note: the decompose-verify workflow
   (wf_e306782c-e1a) BLOCKED on a module-path defect in the probe fixture
   (fixed) and two adversary findings (B5 ownership moved to ζ; B8
   verdict-pin added); the corrected fixtures were then re-verified by the
   deterministic α harness (`scripts/prd-capability-check.py`, fresh debug
   binary, 2026-08-26): all five probes PASS, silent-accept baselines observed
   verbatim.
6. **`at auto` per-instance solve is a declared v1 boundary**: relate-solve
   stays template-keyed in this PRD; per-instance auto placement is the
   placement-relations-belt seam, not silently attempted here.

## 7. Cross-PRD relationships (G4)

| Other PRD / task | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| assembly-derivation-toolbox.md (paired commit) | it consumes | merged-args elaboration + loud-failure diagnostics | this PRD | its batch dep-wires onto α/β/γ/δ |
| indexed-sub-instantiation.md | it consumes | instance-scope elaboration + per-instance handle rows (its γ #5483) | this PRD owns arg-flow integrity beneath; #5482 chain owns the indexed surface | dep note in both batches |
| uniform-member-access.md (#5426) | it consumes | `elaborate_child_instance` semantics | that PRD | reference only |
| sub-placement-and-surfacing.md | corrects | the one-level scope-cut prose + rustdoc + test claims | this PRD (leaf ε) | queued |
| placement-relations-belt.md | boundary | per-instance `at auto` solve (out of scope here) | that PRD (joint seam, per its charter) | declared v1 boundary |
| #6598 / #5868 | siblings | geometry-handle axis / recursion node budget | those tasks | out of scope, wired as related context |
| solver-driver-parity.md (P1, landed 2026-08-26) | it consumes | solved-value realization at the child boundary — P1 B2's geometry half rides β #6592 + κ #6657; P1 owns whether the solve runs, this PRD owns the value flow. P1 §8 also names γ #6608 as adjacent (check exit-gate seam; #5403 ruling 2026-08-26) | this PRD (β, κ) | row added 2026-08-26; mirrors P1 §8 |

## 8. Decomposition plan (task IDs assigned at decompose, 2026-08-26)

- **α = #6586 — adopt** [high]: Stage 0 — port the phase-1.5 dependency-ordered
  overlay walk to the template-scope sub loop; pre-evaluated-literal args; the
  undef-ctor-arg coded diagnostic (with UndefCause). Signals: B6, B7.
- **β = #6592 — adopt** [high, deps α]: Stages 1–2 — per-instance overlay into
  pose evaluation at the walk's three call sites; recursive
  `realize_sub_override_handles` with full-cell instance overlay +
  recursively-seeded named-steps; widen the re-realization trigger from
  "has args" to "effective cells differ from template cells" with
  overlay-fingerprint dedup; retire the one-level boundary INCLUDING its
  rustdoc scope-cut banner (retired in the same diff). Signals: B1–B4, B10,
  B13, B16. (B5 — the pair fixture — is ζ's observable: the fixture does not
  exist until ζ builds it; D3-adversary finding, accepted.)
- **γ = #6608 — loud-failure convergence** [high, deps α, #5403]: `UndefCause` variant +
  Error-severity `DiagnosticCode`s + `reify check` severity-exit convergence;
  re-baseline the `cli_check.rs` indeterminate-exits-0 pins where the new class
  fires. Signals: B7, B8. *(Delta, 2026-08-26 ruling: the check severity-exit gate is #5403's deliverable — allowlist as bounded migration, burned to zero by #5404 — and γ now depends on #5403 (edge wired). γ delivers the UndefCause variant + coded Errors and re-baselines pins/fixtures against #5403's gate; it does not implement the gate, and its new codes must never enter CHECK_ERROR_EXIT_ALLOWLIST. C4/§6.3's "no per-code escalation list" stands as the end-state reached via #5404, not as a prohibition on #5403's waivered migration ratchet.)*
- **δ = #6609 — instance-scoped constraint checking** [high, deps β]: enumerate
  materialized instances; clone child-template constraints with rescoped
  `ValueRef`s; dispatch through the existing machinery; per-instance
  post-geometry folding for arg-carrying instances. Signals: B9, B15.
- **ε = #6610 — test-plane completion + story correction** [medium, deps β]:
  geometry-plane siblings for the values-plane #5360 suite; the first
  specialization-arm geometry test; re-baseline the nested-override
  compile-error pin; correct `seed_cross_sub_named_steps` rustdoc and the
  sub-placement-and-surfacing prose. Signals: B11–B13 green in gate.
- **ζ = #6611 — integration gate** [medium, deps β γ δ ε]: commit the pair fixture,
  register both fixtures, CLI STEP-grep tests, the Shape-A oracle harness
  (hand-specialized twin equivalence). Signals: the §1 bullets, executed —
  the canonical `reify build` STEP grep + `mesh_stats` run.
- **θ = #6612 — docs-truth bundle** [medium, deps β]: doc-chunk update (sub
  instantiation/override semantics — signatures verified against compiler
  registries); `examples/best_practices/` exemplar (per-instance sizing driving
  nested geometry, newly possible) + `INDEX.md` line; reify-design cheatsheet
  index line; intent-level discoverability acceptance.
- **ι = #6613 — warm-trace follow-up** [low, deps β]: instance param cells commit with
  an empty `DependencyTrace`, so the warm reverse-index has no edge from a
  parent cell into a ctor-arg instance cell; record traces at commit. Signal:
  GUI edit of a threaded param updates child geometry without a full rebuild
  (mesh_stats delta), B14.
- **κ = #6657 — solved-value-auto consumption in the per-instance overlay**
  [high, deps β γ; added 2026-08-26, approved by Leo (solver-integration
  session; trace memo in #6631/#6592 details)]: β's differ-trigger meets
  scoped Auto cells in BOTH their states — the compiler filters auto ctor
  args out of `compiled_args` and mints parent-scoped Auto cells
  (`extract_auto_free` filter arm; spec-body arm identical), so an auto
  sub's effective valuation always differs from template defaults, whether
  Undef (pre-solve / solver-free drivers) or solved. This leaf owns that
  interaction: Undef effective cell ⇒ coded-diagnostic skip (γ's codes,
  C4 — never a silent default realization, never an uncoded error; verdicts
  stay never-false-Violated); solved effective cell ⇒ the child re-realizes
  at the solved value through the β overlay (the dimensional solver writes
  back exactly this scoped key namespace). Signals: solver-wired
  `Engine::build` realizes the #6631 auto_ctl shape at the solved 10mm
  (STEP z-extent −10, not the 5mm default); solver-free build/check of the
  same fixture emits the coded diagnostic. Consumers: #6631 (author-surface
  composite, dep-wired on β) and the printer_v01 pin retirement; #5617
  stays the value-plane sibling.
- **η = #6614 — PRD close** [low, deps all; docs edit, normal/simple]: backfill real
  IDs into this section, terminal stamp + AS-AUTHORED freeze + LIVE map, same
  header on the capability manifest.

## 9. Out of scope

- #6598's geometry-handle axis (an overridden child's own body reading
  `self.<innersub>.body`) — architect-ruled out of #6592; its own task.
- #5868 recursion node budget.
- Per-instance `at auto` relate-solve (placement-relations-belt seam; §6.6).
- Any change to Kleene semantics or never-false-Violated.
- Realization-node freshness/caching identity for per-instance handles (the
  existing one-level path already sits outside it; growing that population is
  accepted and noted for the caching PRD when one exists).
- Pattern builtins; everything the assembly-derivation-toolbox PRD owns.

## 10. Open questions (tactical)

1. Diagnostic code names (`E_CTOR_ARG_UNDEF`, `E_SUB_GEOMETRY_UNREALIZED` or
   similar — final names at implementation; codes mandatory per INV-SF-6).
2. Whether γ's severity-exit convergence lands check-wide in one step or scoped
   to the new codes first — implementer verifies blast radius against existing
   code-less Error emissions on healthy paths (INV-SF-2's corollary: demote
   those, never exempt them).
3. Diagnostic dedup across many instances of one template (dedup by
   message+entity vs per-instance emission) — decide at γ.
4. Exact fixture split (one probe file vs probe + pair) — decide at ζ.
