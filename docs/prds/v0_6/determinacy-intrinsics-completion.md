# Determinacy Intrinsics Completion (`std.determinacy` §12 utility constraints)

- **Milestone:** v0.6 (spec/stdlib-reference gap-fill; the `§12` version label is an unconditional promise)
- **Status:** contract — authored 2026-06-02 in an interactive `/prd` session under G1–G6 + META. Decompose batch `stdlib-determinacy-2026-06-02`, cluster `determinacy-intrinsics-completion`.
- **Approach:** **B + H** (contracts + two-way boundary tests). Blast radius ≥ 4 crates (`reify-compiler`, `reify-eval`, `reify-kernel-occt`, `reify-ir`, plus `reify-cli` surfaces = 5); touches two load-bearing seams (the geometry-kernel realization path and the constraint-eval/report path).
- **Source gap:** `docs/architecture-audit/stdlib-reference-gap-register-2026-06-01.md` → **P18 determinacy-intrinsics** (5 gaps, 4 HIGH). Survey doc `docs/reify-stdlib-reference.md` §12.

## §0 — Purpose and scope

`docs/reify-stdlib-reference.md` §12 (`std.determinacy`) documents four prelude **predicates** (`determined`/`constrained`/`undetermined`/`partially_determined` — all exist, resolved at `expr.rs:1513-1549`) and three **utility constraints** that are fiction today:

```
constraint def AllParamsDetermined { ... }      // Compiler intrinsic -- walks all params
constraint def AllGeometryDetermined { ... }     // Compiler intrinsic -- walks all Geometry-typed params
constraint def RepresentationWithin { ... }      // Asserts geometry realizations within tolerance
```

The survey flagged five gaps. This PRD closes the **two that are genuinely this cluster's** and **coordinates the other three** (already owned elsewhere):

| Gap | Status going in | This PRD |
|---|---|---|
| `AllParamsDetermined` does not exist | HIGH; doc "compiler intrinsic" never built | **builds** (task α) — compiler-sugar intrinsic |
| `AllGeometryDetermined` does not exist | HIGH; ditto | **builds** (task α) |
| `RepresentationWithin` only extracts a bound, never asserts | MEDIUM; extractor-only | **builds** (tasks β, γ) — true post-realization assertion |
| purpose `design_review` not in stdlib | HIGH | **owned by** `purposes-completion.md` task ζ (#4016) — coordinate (§6) |
| purpose `simulation_ready` is a tautological placeholder | HIGH | **owned by** `purposes-completion.md` task ζ (#4016) — coordinate (§6) |

**The reframing finding.** The two stdlib purposes (`design_review`, `simulation_ready`) are **already authored + decomposed** in the deferred sibling PRD `docs/prds/v0_6/purposes-completion.md` (task ζ = #4016), which *deliberately* expresses them with the reflective `forall p in subject.params: …` idiom rather than calling named intrinsics. This PRD does **not** re-file them. It supplies the named intrinsics §12 promises (so the doc stops being fiction and users get reusable named symbols), and makes `RepresentationWithin` the assertion the doc claims it is.

**Design decisions (resolved this session, see §4).**
- **Intrinsics = compiler sugar, not user `constraint def`s.** A `constraint def` cannot take an entity-reference (`Structure`) parameter and reflective queries (`subject.params`) run **only in purpose bodies** (verified §3). So `AllParamsDetermined`/`AllGeometryDetermined` are *compiler-recognized constraint names* that desugar to the existing reflective `forall` form — faithful to the doc's "compiler intrinsic" wording, reusing merged machinery with **no new eval primitive**.
- **`RepresentationWithin` = a true measured assertion.** The realizer exposes an **achieved representation tolerance** (a *measured facet-chord deviation*, not the configured deflection — see the G6 reality check, §8.3), and the constraint compares `achieved ≤ asserted_bound`, three-valued.

**User-observable end state:** an engineer writes `constraint AllParamsDetermined(subject)` / `AllGeometryDetermined(subject)` in a purpose body and it participates in the `reify check` satisfied/violated/indeterminate report exactly as the hand-written reflective form; and a `RepresentationWithin(subject, tol)` on an output occurrence is no longer just a budget knob — a coarsely-realized curved subject **reports Violated** (non-zero exit) and a finely-realized one **reports Satisfied**.

## §1 — Spec / doc grounding

- **`docs/reify-stdlib-reference.md` §12** — the three utility constraints above; the example purposes call them: `purpose design_review(subject) { constraint AllParamsDetermined(subject) }`, `purpose simulation_ready(subject) { constraint AllGeometryDetermined(subject); constraint determined(subject.material) }`.
- **`docs/reify-language-spec.md` §9.4** — `determined`/`constrained`/… are compiler intrinsics composing with `forall`/`exists`/`and`/`or` (verified `expr.rs:1497-1549`).
- **Determinacy predicate semantics (verified):** `determined(p)` ⇔ state `Determined`; `constrained(p)` ⇔ `Auto`/`Provisional` (solver variable). The intrinsics below build on `determined`.

## §2 — Pre-conditions for activating

No upstream substrate blocker — all merged on main:

- **Reflective aggregation works (single-param).** `forall p in subject.params: constrained(p)` / `subject.geometric_params: determined(p)` compile to `Quantifier{collection: PurposeReflectiveAggregation, body: DeterminacyPredicate}` (`expr.rs:2330-2348`) and materialize at activation (`engine_purposes.rs:809-970`). Pinned by passing tests `purpose_compile_tests.rs:623` and `purpose_activation.rs:1422` (tasks 2289/2544/4137/4138 merged).
- **Determinacy predicates resolved** (`expr.rs:1513-1549`).
- **Grammar gate PASSED** (G3, §7).
- **Tolerance budget already drives the mesher** end-to-end: `compute_demanded_tols` → `compute_tessellation_budgets` → `per_stage_tolerance` (×0.8 safety + N-stage split) → `kernel.tessellate(handle, per_stage_tol)` (`engine_build.rs:3104/3150/4135`).
- **OCCT point-to-shape distance exists**: `BRepExtrema_DistShapeShape` is bound and used by `min_clearance` (`ffi.rs:776`, `lib.rs:863`) — the deviation metric reuses it.

## §3 — Consumer (G1)

Every mechanism this PRD introduces, and its named consumer:

| Mechanism | Consumer |
|---|---|
| `AllParamsDetermined` / `AllGeometryDetermined` compiler-sugar intrinsics (`reify-compiler`) | **`reify check --purpose <p>=<entity>`** activating a CI example purpose that calls them (`examples/determinacy_intrinsics.ri`, in this batch — task α); end users via §12 doc; `purposes-completion` #4016 as an optional downstream adopter (§6, no hard dep) |
| Realizer **achieved-representation-tolerance** metric (`reify-kernel-occt` FFI + the `engine_build.rs:4135` tessellate site) | the `RepresentationWithin` assertion eval (task γ, in this batch) |
| `RepresentationWithin` **assertion** evaluation → `Satisfaction` (`reify-eval`) | **`reify check`** report + exit code on a CI example with an output occurrence (`examples/representation_within.ri`, task γ) |
| §12 doc reconciliation | end users / doc readers |

No mechanism is a **new** in-engine seam: the intrinsics ride the existing purpose-body constraint-compile path; the assertion rides the existing constraint-eval/`report_eval_output` path; the deviation metric plugs into the existing tessellate call site. No `engine-integration-norm.md §3` seam is added.

**Anti-orphan note.** The intrinsics' immediate, in-batch consumer is the CI example + `reify check` (a user-observable signal), so they are not a producer-orphan even if `purposes-completion` is never activated.

## §4 — Contracts (the core design)

### 4.1 Compiler-sugar determinacy intrinsics (task α)

Two compiler-recognized constraint names, valid **only inside a purpose body** (reflective queries are purpose-only). Desugar at the purpose-body member-compile step **before** member compilation, by AST rewrite, then run the existing `forall` + reflective-aggregation + `DeterminacyPredicate` path:

```
constraint AllParamsDetermined(X)     ⇒  constraint forall __p in X.params:           determined(__p)
constraint AllGeometryDetermined(X)   ⇒  constraint forall __p in X.geometric_params:  determined(__p)
```

- `X` must be a **purpose param** (entity reference). The query root keys on `X`'s param name, so the desugar works identically for any purpose param (single- or, once `purposes-completion` lands multi-ref, multi-param).
- **Semantics are name-faithful:** both walk with `determined` (the doc names them "*Determined*"). (`purposes-completion`'s `design_review` chose a *weaker* `constrained` inline body — a tunable "example purpose" divergence, §6; the intrinsic is the stricter, name-faithful building block.)
- **Diagnostics:** used outside a purpose body → `E_DETERMINACY_INTRINSIC_SCOPE`; wrong arity / non-entity-ref arg → `E_DETERMINACY_INTRINSIC_ARG`. No silent degradation to an undefined identifier call.
- **No new eval primitive** — the desugared form is byte-identical to a hand-written `forall … : determined(__p)`, so it inherits Kleene three-valued semantics and the activation/deactivation bookkeeping for free.

| # | Invariant |
|---|-----------|
| A1 | `constraint AllParamsDetermined(X)` produces a `CompiledPurpose.constraints` entry **identical** to the hand-written `forall __p in X.params: determined(__p)` (golden equivalence). |
| A2 | `AllGeometryDetermined` likewise over `.geometric_params`. |
| A3 | Used outside a purpose body → compile diagnostic, never a fall-through to an unknown user fn. |
| A4 | Activating a purpose that uses the intrinsics on a fully-determined structure ⇒ **Satisfied**; with an `undef`/auto param ⇒ **Violated/Indeterminate** per the predicate's Kleene result, same as the reflective form. |

### 4.2 Realizer achieved-representation-tolerance metric (task β)

The realization result records a **measured achieved representation tolerance** per realized subject.

- **New FFI** `measure_mesh_deviation(shape: &OcctShape, mesh: &TessResult) -> f64`: for each mesh facet, sample **interior** points (triangle centroid + the three edge midpoints — points that depart from the exact surface; mesh *vertices* lie on the surface by construction and are useless, see §8.3), project each onto `shape` via `BRepExtrema_DistShapeShape`, return the **max** distance (SI metres). Reuses the existing distance primitive (`ffi.rs:776`).
- **Recorded at the tessellate site** (`engine_build.rs:4135`): immediately after `src.tessellate(pid, per_stage_tol)`, compute `achieved = measure_mesh_deviation(shape, &mesh)` and store it keyed by the realized occurrence (an engine-side `achieved_repr_tol: HashMap<occurrence_name, f64>`, or a `Option<f64>` field on the realization result struct — decompose-time tactical, §11).
- **Honest metric, not the configured deflection.** Echoing `per_stage_tol` would be *circular* (the `RepresentationWithin` bound feeds the budget that sets the deflection, so `achieved ≤ bound` would be tautologically true). The measured facet-chord deviation can **exceed** the requested deflection when OCCT clamps (MinSize / angular-deflection domination / mesh failure), which is exactly the condition a real assertion must catch.

| # | Invariant |
|---|-----------|
| B1 | A planar-only subject (e.g. `box`) has measured deviation ≈ 0 at any deflection. |
| B2 | A curved subject (sphere/cylinder) realized at a **coarse** deflection has a strictly larger measured deviation than the same subject at a **fine** deflection (monotone). |
| B3 | The metric is a finite non-negative `f64`; an unrealized / failed-mesh subject contributes **no** value (→ `None`). |

### 4.3 `RepresentationWithin` assertion evaluation (task γ)

`RepresentationWithin(subject, bound)` becomes a real assertion **without breaking the existing budget extractor** (`tolerance_combine::extract_output_tolerance_bound` stays — it still drives the deflection).

- At constraint-eval time, recognize the `UserFunctionCall("RepresentationWithin", [subject_ref, bound_lit])` (same recognition gates as the extractor: arg0 `ValueRef:StructureRef`, arg1 `Literal Scalar LENGTH finite≥0`).
- Resolve `subject` → occurrence name → the achieved-repr-tol recorded in §4.2.
- Evaluate to `Satisfaction`: `achieved ≤ bound` ⇒ **Satisfied**; `achieved > bound` ⇒ **Violated**; subject not realized / no metric available ⇒ **Indeterminate** (never a false Violated). Flows through `dispatch_constraints` → `ConstraintResult{satisfaction}` → `report_eval_output` like any constraint.

| # | Invariant |
|---|-----------|
| C1 | **Ordering:** the achieved-tol for `subject` is recorded (realization run) **before** the `RepresentationWithin` constraint is evaluated; if realization has not run for `subject`, the constraint is **Indeterminate**, not Violated. |
| C2 | The budget-extractor path (`extract_output_tolerance_bound` → deflection) is **unchanged** — the same constraint both *drives* the budget and *asserts* the result (regression-locked against the existing `tolerance_scope` / `tolerance_combine` tests). |
| C3 | Coarse-realization curved subject ⇒ Violated (non-zero `reify check` exit); fine-realization ⇒ Satisfied (zero exit). |
| C4 | A `bound = 0.0` ("exact") on a curved subject ⇒ Violated (no triangulation is exact); on a planar subject ⇒ Satisfied. |

## §5 — Approach (G5) and boundary-test sketch (the H component)

**B + H.** Two load-bearing seams: (1) the geometry-kernel realization path (β feeds γ across `reify-kernel-occt` → `reify-eval`), and (2) the constraint-eval/report path (γ). The boundary tests below are the integration-gate signal (task δ); they face both producer and consumer sides.

| # | Scenario | Preconditions | Postconditions (assert) |
|---|---|---|---|
| BT1 | Intrinsic golden-equivalence | purpose with `constraint AllParamsDetermined(subject)` vs hand-written `forall __p in subject.params: determined(__p)` | identical `CompiledPurpose.constraints` (A1) |
| BT2 | Geometry intrinsic | `constraint AllGeometryDetermined(subject)` | desugars over `.geometric_params` with `determined` (A2) |
| BT3 | Intrinsic scope diagnostic | `AllParamsDetermined(x)` outside a purpose | `E_DETERMINACY_INTRINSIC_SCOPE` (A3) |
| BT4 | Intrinsic end-to-end | `reify check --purpose pr=<determined structure>` then `<undef structure>` | Satisfied then Violated/Indeterminate (A4) |
| BT5 | Deviation monotonicity (producer side) | sphere realized at coarse vs fine deflection | coarse deviation > fine deviation; planar box ≈ 0 (B1/B2) |
| BT6 | RepresentationWithin Violated | `reify check` on output occ with `RepresentationWithin(curved, tight)` at coarse realization | report Violated + non-zero exit (C3) |
| BT7 | RepresentationWithin Satisfied | same with a sufficient bound / fine realization | report Satisfied + zero exit (C3) |
| BT8 | RepresentationWithin Indeterminate | subject not realized | Indeterminate, not Violated (C1) |
| BT9 | Budget-extractor regression | existing `tolerance_scope` / `tolerance_combine` suites | green — the assertion did not break budget extraction (C2) |

## §6 — Cross-PRD relationship (G4 seam ownership)

| Other PRD / cluster | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `purposes-completion.md` (#4016 ζ, deferred) | consumes (the purposes *may* call the intrinsics) | `design_review`/`simulation_ready` bodies | **this PRD owns the intrinsics; #4016 owns the purpose bodies** | coordinate — **no hard dep** |
| `purposes-completion.md` (#4018 η, pending) | both touch `reify-stdlib-reference.md` §12 | §12 doc text | **this PRD (δ) owns the entire §12 `std.determinacy` stdlib-reference reconciliation** (utility constraints + RepresentationWithin + the example-purposes block); #4018 retains the **spec §9.5** (`reify-language-spec.md`) update only | resolved — surface to user to trim #4018's stdlib-reference scope |
| `constraint-solver-completion.md` (#4019 κ, pending) | adjacent | reads determinacy state; `W_UNDERDETERMINED` is free-param reporting, not the §12 intrinsics | n/a | no seam |

**The intrinsics/purposes seam (the one real coordination point).** `purposes-completion` #4016 ζ inlines `forall p in subject.params: constrained(p)`. By construction (A1) the intrinsic `AllParamsDetermined(subject)` desugars to the *same* compiled form (modulo `determined` vs the `constrained` predicate #4016 chose). So #4016 **may** adopt the intrinsics for DRY but is **not required to**, and this PRD does **not** add a dependency edge into the deferred purposes-completion batch (avoids entangling it). The intrinsics have a self-contained in-batch consumer (§3), so G1 holds regardless. The doc (§12, owned by δ) shows the example purposes *using* the intrinsics, matching the spec.

**Doc-ownership resolution (avoids the §12 file-lock fight).** Both δ (here) and #4018 would otherwise edit `reify-stdlib-reference.md` §12. Resolution: **δ owns all of `reify-stdlib-reference.md` §12**; #4018 is narrowed to the `reify-language-spec.md` §9.5 example. (This narrowing is a *coordination note* — this PRD does not edit #4018's task row; surfaced in the hand-back for the user to trim.)

## §7 — Grammar gate (G3) — PASSED

`tree-sitter parse --quiet` (from `tree-sitter-reify/`), exit 0, 0 ERROR nodes:

| Fixture | Shape | Result |
|---|---|---|
| `det_intrinsic.ri` | `purpose design_review(subject : Structure) { constraint AllParamsDetermined(subject) }` + a `simulation_ready` calling `AllGeometryDetermined(subject)` with a `where exists … { constraint forall … }` guard | exit 0 |

`RepresentationWithin` introduces **no new syntax** — it is an existing `UserFunctionCall` recognition; the change is purely *semantic* (extractor → assertion). The intrinsics are recognized in the compile path, not declared in `.ri`, so they add no grammar. **`grammar_confirmed = true` for every task.**

## §8 — Substrate notes

### 8.1 Why the intrinsics are compiler sugar, not `constraint def`s
`constraint def` params are value-only (`defs_phase.rs:89-101`, `types.rs:1367-1373`); a `constraint def X(subject : Structure)` fails type-resolution. Reflective expansion (`expand_purpose_reflective_placeholders`) runs only for purpose bodies (`engine_purposes.rs:233-239`); constraint-def predicates are AST-substituted in the caller's scope and never reach it. So a doc-literal `constraint def AllParamsDetermined` is architecturally impossible today — the faithful realization is the compiler-recognized desugar.

### 8.2 What `RepresentationWithin` is today
Recognized as `UserFunctionCall("RepresentationWithin",[ValueRef:StructureRef, Literal:LENGTH])` and **only** mined for its tolerance bound (`tolerance_combine.rs:129-211`, `engine_purposes.rs:625-705`, `tolerance_promise.rs`). The bound drives the realization deflection (`engine_build.rs:3104/3150/4135`). No path compares realized geometry to the bound; the realization result (`graph.rs:46 RealizationNodeData`, `geometry.rs:1487 Mesh`) carries **no** achieved-tolerance field, and the tessellate FFI (`ffi.rs:1028`) returns only vertices/indices/normals (no `IsDone`/achieved readback). β fills this gap.

### 8.3 G6 reality check — the achieved-deviation metric (numerical hazard)
- **Basis:** `BRepExtrema_DistShapeShape` gives the *exact* minimum distance from a point to the B-rep (`ffi.rs:776`). Sampling facet **interior** points (centroid + edge midpoints) and taking the max distance is a real chord-deviation metric matching OCCT's linear-deflection definition (max facet-to-surface distance).
- **Floor / honest claim:** the metric is a **sampled max → a lower bound on the true Hausdorff deviation** (the true max can fall between samples). The assertion's signal is therefore "**max sampled facet deviation ≤ bound**", *not* "provably within tolerance everywhere". This is honest and still discriminating: a coarse sphere has sampled deviation ≫ a fine sphere (BT5), and a coarse-vs-tight pairing flips Satisfied↔Violated (BT6/BT7) — a demonstrable, non-tautological signal.
- **Two explicitly-rejected cheaper designs** (Open Question 3): (a) echoing the configured `per_stage_tol` — *circular* (bound drives deflection); (b) sampling mesh **vertices** — tautologically ≈ 0 (vertices lie on the exact surface). Both produce a useless assertion; rejected.
- **Deferred refinement:** densified / curvature-adaptive sampling, or an OCCT-side achieved-deflection readback, for a conservative *upper-bound* metric — out of scope, noted as a follow-up.

## §9 — Decomposition plan (the DAG)

Greek labels; real IDs assigned at decompose. Modules: `reify-compiler`, `reify-kernel-occt`, `reify-eval`, `reify-ir`, `reify-cli`, `examples`, `docs`.

- **α — Compiler-sugar determinacy intrinsics + example consumer.** Recognize/desugar `AllParamsDetermined`/`AllGeometryDetermined` in the purpose-body compile path (§4.1); scope/arity diagnostics; golden-equivalence vs the reflective form; commit `examples/determinacy_intrinsics.ri` exercised by a `reify check --purpose` test.
  - *Signal (leaf):* `reify check --purpose design_review=<structure> examples/determinacy_intrinsics.ri` — a purpose calling `AllParamsDetermined(subject)` activates and its constraint appears in the satisfied/violated/indeterminate report + exit code; BT1–BT4 green.
  - *Modules:* reify-compiler, reify-eval/reify-cli (tests), examples. *Prereqs:* — (reflective substrate merged). *grammar_confirmed:* true.
- **β — Realizer achieved-representation-tolerance metric.** FFI `measure_mesh_deviation` (facet-interior sampling + `BRepExtrema_DistShapeShape`); record achieved-tol at the tessellate site (§4.2).
  - *Signal (intermediate → unlocks γ):* a `reify-kernel-occt`/`reify-eval` integration test on **real** OCCT geometry — a sphere at coarse vs fine deflection yields monotonically smaller measured deviation, a planar box ≈ 0 (BT5, B1–B3). (Real-geometry measurement, not synthetic input.)
  - *Modules:* reify-kernel-occt, reify-eval. *Prereqs:* —. *grammar_confirmed:* true.
- **γ — `RepresentationWithin` assertion eval + report.** Eval-time recognition → resolve subject → read achieved-tol (β) → `Satisfaction`; keep the budget extractor intact (§4.3); commit `examples/representation_within.ri`.
  - *Signal (leaf):* `reify check examples/representation_within.ri` — a coarsely-realized curved subject with `RepresentationWithin(subject, tight)` reports **Violated** (non-zero exit + diagnostic); a sufficient bound reports **Satisfied** (zero exit); unrealized subject ⇒ Indeterminate; BT6–BT9 green.
  - *Modules:* reify-eval, reify-cli (+ reify-ir if a result field is added). *Prereqs:* β. *grammar_confirmed:* true.
- **δ — §12 doc reconciliation + B+H integration gate.** Update `reify-stdlib-reference.md` §12 to match shipped reality: `AllParamsDetermined`/`AllGeometryDetermined` as compiler-sugar over the reflective form (purpose-body-only); `RepresentationWithin` as a real *sampled-deviation* assertion (with the §8.3 caveat). This is the **integration-gate** task — its signal is the full §5 boundary-test suite (BT1–BT9) plus the doc.
  - *Signal (leaf):* BT1–BT9 green; `reify-stdlib-reference.md` §12 lists the intrinsics + RepresentationWithin with semantics matching the shipped code.
  - *Modules:* docs, reify-eval/reify-cli (boundary tests). *Prereqs:* α, γ. *grammar_confirmed:* true.

### Dependency view
```
α (intrinsics + example) ─────────────┐
β (realizer metric) ─► γ (assertion) ─┴─► δ (doc + integration gate / BT1–BT9)
```
(`α` and `β` are independent roots; `γ` follows `β`; `δ` joins `α` and `γ`. No edge into the deferred `purposes-completion` batch — §6.)

## §10 — Out of scope

- **The stdlib purposes `design_review` / `simulation_ready`** — owned by `purposes-completion.md` #4016 ζ. This PRD supplies the intrinsics they document; it does not author the purposes.
- **A conservative *upper-bound* deviation metric** (curvature-adaptive / densified sampling, OCCT achieved-deflection FFI readback). The sampled lower-bound metric is what ships; a stronger guarantee is a follow-up (§8.3).
- **`RepresentationWithin` on imported (`Input`) geometry** — the import promise (`tolerance_promise.rs`) is unverifiable for arbitrary STEP/STL; the assertion targets *output/realized* subjects only.
- **GUI surfacing** of the assertion / intrinsics — CLI is the sufficient G1 consumer.
- **Multi-ref purpose params for the intrinsics** — the desugar is param-name-keyed and multi-ref-ready, but multi-ref *activation* lands in `purposes-completion`; this batch ships the single-subject path (the only one reachable today).

## §11 — Open (tactical) questions

1. **Achieved-tol storage location** — an engine-side `HashMap<occurrence_name, f64>` vs an `Option<f64>` field on the realization result struct (`RealizationNodeData` / `Mesh`). Default: engine-side map keyed by occurrence (smallest blast radius, no IR type change). **Decide during β.**
2. **Facet sampling density** — centroid + 3 edge midpoints (4 samples/triangle) is the default; denser sampling trades cost for a tighter lower bound. **Decide during β**; densification is a deferred refinement (§10), not a re-architecture.
3. **Configured-deflection vs measured-deviation** — measured wins (configured is circular, §8.3). Recorded as resolved; listed here only to pin the rejected alternative.
4. **`E_*` diagnostic code spellings** (`E_DETERMINACY_INTRINSIC_SCOPE` / `_ARG`) — final codes per the `reify-core` diagnostic registry convention. **Decide during α.**
