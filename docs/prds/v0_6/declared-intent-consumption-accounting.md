# Declared-intent consumption accounting — relate verification, solver envelope, objective accounting, indeterminate honesty

**Milestone:** v0_6 · **Status:** active (authored 2026-07-24 in a `/prd` session under G1–G7+META, from spawn brief `2026-07-24-prd-consumption-accounting`; scope decisions fixed by the 2026-07-24 silent-undef/placeholder eradication investigation, session a0d342d4 — do not re-litigate) · **Approach:** B + H (light)
**Normative substrate:** `docs/legibility/design-invariants.md` (landing via task/5395) — this PRD implements **INV-SF-3 `declared-intent-consumed-or-diagnosed`** and **INV-SF-4 `indeterminate-attributable-transient`**. Every declaration expressing design intent (a constraint, a `relate` block, an objective) is either consumed by a solve/verify pass this run or generates a diagnostic naming why not; a declaration structurally incapable of ever being consumed is a compile-time error.

---

## §0 — Purpose and scope

A designer who declares intent the engine cannot or does not consume today gets **silence** — or worse, a green check. Four probe-verified failures (all reproduced 2026-07-24 with the 2026-07-22 debug binary on main `0d70ef1d5b`; committed fixtures §2.4):

1. **`relate` without `at auto` is a total silent no-op — under a green check.** `solve_scopes` (`crates/reify-eval/src/relate_solve.rs:826`) filters relate scopes at `:836` to those with ≥1 auto unknown AND ≥1 relation; a scope with relations but zero autos is dropped before any realization. The relations are neither solved NOR verified; `reify check` prints `All constraints satisfied.` (probe: fixture `dic_relate_static_violated.ri`, whose relations are geometrically FALSE at the subs' fixed placements). The verification machinery already exists in the same function — `solve_relate_scope` step 4 verifies redundant-remainder relations against a solved placement.
2. **An auto param of a kind no registered solver can represent is silently coerced, mis-solved, and mis-reported.** No representability check exists anywhere in the intake: the classifier gives `String`/`Enum`/geometry-kinded refs no domain flag (`classifier.rs:115` — explicit no-op) → `Dimensional` default → `DimensionalSolver`, whose trial/solved values are built `Value::Scalar` unconditionally (`solver.rs:138-150`, `:110-129`; `dimension_of` maps any non-Scalar type → DIMENSIONLESS at `:99-104`). Net: a `String` auto yields the **misleading** `constraints could not be satisfied (max absolute residual: 1.00e0)`, and a "solved" non-Scalar auto would be **written back as a `Value::Scalar`, clobbering its declared kind**. Probe: `dic_string_auto.ri`.
3. **A declared objective that governs nothing is silently dropped.** Three registry sites drop it with no message: no-autos early exit (`registry.rs:152-162`), `components.is_empty()` (`registry.rs:182-192` — this is the *unconstrained* `minimize (a-3.0)²` shape: `a = undef, awaiting solve`, objective never mentioned), and objective-matches-no-component → **silently attached to component 0** (`registry.rs:208-210`). Engine-side, a scope with an objective and no autos at all never solves — `minimize k*k` over a concrete `k` produces zero output of any kind. Probes: `dic_min_no_autos.ri`, `dic_min_unread.ri`, `dic_min_unconstrained.ri`.
4. **Permanently-indeterminate constraints are structurally inert under a green check.** Ad-hoc `@`-selector `frame_align` constraints (generated at `connect.rs:587-619`) are INDETERMINATE in every possible run — the `@face`/`@edge` selector evaluates to an `Undef` placeholder (`reify-expr/src/lib.rs:1281`), the `Eq` folds Undef, and `SimpleConstraintChecker::check` bails to `Satisfaction::Indeterminate` (`reify-constraints/src/lib.rs:173-206`). Non-strict check: `No constraints violated (1 indeterminate).` exit 0. Worse, the recorded reason is **discarded**: the outcome carrier `ConstraintCheckEntry` (`reify-eval/src/lib.rs:1140-1144`) carries a bare `Satisfaction`; strict mode's `report_indeterminate_detail` (`reify-cli/src/main.rs:2340-2356`) then **guesses** a generic "inputs undefined (e.g. auto-params unresolved or geometry did not realize)" — while the true recorded reason ("operator undefined for these operand kinds", `classify_undef` `reify-constraints/src/lib.rs:68-102`) exists only as a side-channel warning string. Probe: `dic_inert_connect.ri`.

**What ships:** four accounting mechanisms — **A** relate zero-auto static verification; **B** solver capability envelope with typed refusal; **C** objective consumption accounting (compile error for structurally-inert objectives + build/solve-seam diagnostic for zero-component consumption); **D** typed indeterminate reasons (transient vs structural), inert-constraint detection, and a `reify check` consumption ledger. Conservative degradation stays correct — Indeterminate/skip/refuse never becomes a false Violated — but **always leaves an observable trace**.

### §0.1 — What this is NOT (G4 boundaries, fixed by the spawn brief + sibling PRDs)

- **NOT the exit-code gate (INV-SF-2).** `error:` with exit 0 (visible in probes 1–3) is owned by the sibling "eradicate silent undef" PRD's severity gate. This PRD emits correctly-coded, correctly-severitied diagnostics and lets that gate own process exit codes. The ONE exception: mechanism D's inert class fails `reify check` through check's **native constraint-outcome path** (`check_fails`, `main.rs:2299-2305`) — an outcome-class change, not a severity-gate change.
- **NOT the discrete-solve capability.** CpSat registration/default-ON, `DiscreteFirstFallback` routing, let-tracing, `Int`/`discrete_set` domains, and minimize-over-discrete actually *solving* are owned by `discrete-cost-minimisation.md` (PRD 2, authored 2026-07-24, task/5396). This PRD owns only the **accounting**: mechanism B derives its verdicts from the **live registry contents**, so it is truthful before AND after PRD 2 lands (§3 decision 2).
- **NOT objective routing.** The component-0 fallback (`registry.rs:208-210`) and objective-side transitive coupling into the *solve* are PRD 2 α's fold/let-tracing territory; mechanism C reads transitive reachability (via landed `build_dependent_cells`, #5188) but changes no routing (§3 decision 3).
- **NOT undef provenance (INV-SF-1) or placeholder types (INV-SF-5)** — sibling PRDs from the same investigation.
- **NOT deep relate diagnostics.** The DOF ledger, `reify explain`, and `UndefCause::SolveFailed` refinements for auto-ful relate scopes are #4388 (geometric-relations θ, pending). Mechanism A owns only the zero-auto arm.
- **NOT new solver capabilities** — e.g. objective-only component construction (making unconstrained `minimize` actually solve) is a named follow-up (§9), not built here; C makes its absence loud.

---

## §1 — Consumer (G1)

| Mechanism | Consumer |
|---|---|
| A — relate zero-auto static verification (α) | The existing relate pipeline entry `engine_build.rs:3710` → `solve_scopes`; user surface: `reify eval`/`build` diagnostics on a fixed-placement assembly (violated relation ⇒ Error; today: silence). Feeds D's ledger relate row (ζ). |
| B — solver capability envelope + typed refusal (β) | `SolverRegistry` dispatch (the catalogued **§3.5 ConstraintSolver seam**, `engine-integration-norm.md`); user surface: `reify eval`/`check` typed diagnostic replacing the misleading residual error; undef cause recorded via the landed `record_failed_autos` channel (`engine_eval.rs:4821-4857`). |
| C — objective consumption accounting (γ) | Compile half: compiler diagnostics (user-facing at `reify check`). Runtime half: the engine objective path (`engine_eval.rs:4614-4643`, `:4861-4878` — the existing #4804 objective-diagnostic site). Feeds D's ledger objective row (ζ). |
| D — typed indeterminate reasons + inert detection (δ, ε) | `reify check` per-constraint report + strict detail (`report_constraint_results` `main.rs:2370-2400`, `report_indeterminate_detail` `main.rs:2340-2356`); compile half of ε: `connect.rs` generation-time refusal. |
| D — consumption ledger (ζ) | `finish_check` (`main.rs:2719-2746`) — the end-user `reify check` summary; the integration gate for the whole PRD. |

**Engine-integration sub-check (G1).** B plugs into the catalogued §3.5 ConstraintSolver seam (registry dispatch + an additive trait method with a conservative default); A extends the existing relate pipeline inside its established `engine_build` walk; C/D extend existing diagnostic/report surfaces. **No new seam; no orphan-producible `pub fn`.**

---

## §2 — Background & substrate (verified 2026-07-24, main `0d70ef1d5b`)

### §2.1 — Landed substrate this PRD builds on

- **Relate verify machinery:** `solve_relate_scope` step 4 verifies remainder relations against a placement (`relate_solve.rs:635-797`); `collect_relate_scope` (`:91`), the shared single-sub-build realization (`realize_structures`, `solve_scopes` `:826-860`), and the geometric conflict/assertion diagnostic rendering. The zero-auto arm reuses all of it.
- **Transitive-reachability builder:** `build_dependent_cells` (`engine_eval.rs:1553`), production-wired (#5188 done). C's consumption test reads it; C never re-derives connectivity (G7 `no-lockstep-duplication`).
- **Objective diagnostic precedent (#4804 done):** `W_SOLVER_OPTIMALITY_UNPROVEN` emission at `engine_eval.rs:4867-4877` (single-scope) and `:6281-6291` (merged) — the house pattern for honest objective reporting; C's runtime diagnostic lives beside it.
- **Reason substrate:** `classify_undef` (`reify-constraints/src/lib.rs:68-102`) already distinguishes "undefined inputs: {cells}" from "operator undefined for these operand kinds: {kinds}" — as strings, discarded before the outcome carrier. `GdtConformanceResolution::Indeterminate(String)` (`engine_constraints.rs:2100`) carries free-text measurement reasons. D types this existing knowledge; it does not invent new classification.
- **Conservative-degradation philosophy:** "a sub-resolution, unmeasurable, or no-kernel result is Indeterminate and can NEVER produce a false Violated" (`engine_constraints.rs:229-231`). Kept absolutely — and mirrored: D never produces a false **Inert** (§3 decision 5).
- **Undef-cause channel (undef-self-describing, done):** `UndefCause` + `trace_undef_causes` + the CLI `note: X is undef (because: …)` loop — B records refusal causes through it.
- **Ad-hoc selector generation:** `connect.rs:587-619` generates `frame_align_{l}_{r}` as `Eq(left_frame, right_frame)`; `@face`/`@edge` placeholders at `reify-expr/src/lib.rs:1281`; companion warning `detect_unresolved_ad_hoc_selectors` (`engine_eval.rs:1076-1125`).
- **Check accounting spine:** `report_constraint_results` → `ConstraintOutcome` → `check_fails` → `finish_check` (`main.rs:2370-2400`, `:2299-2305`, `:2719-2746`). ζ extends this spine; it does not fork it.

### §2.2 — Empirical baseline (probes 2026-07-24; fixtures under `docs/prds/v0_6/fixtures/`)

| Probe | Today's behaviour (verified) |
|---|---|
| `dic_relate_static_violated.ri` — relate, no autos, relations FALSE at fixed placements | `reify check`: `All constraints satisfied.` exit 0; `reify eval`: geometry realizes, zero relate output — **false green** |
| `dic_relate_static_ok.ri` — same shape, relations TRUE at fixed placements | identical silence (no distinction from the violated case) |
| `dic_string_auto.ri` — `param s : String = auto` + equality constraint | `error: constraints could not be satisfied (max absolute residual: 1.00e0)` + `s undef (solve failed: infeasible)` — misleading (the solver received a kind it cannot represent) |
| `dic_min_no_autos.ri` — `minimize k*k`, no autos in scope | **total silence** — no solve, no diagnostic, exit 0 |
| `dic_min_unread.ri` — `a = auto(free)` + `minimize k*k` (objective reads only concrete `k`) | `a` resolves via constraints (3.06…), objective silently useless; only the generic auto(free) non-uniqueness warning |
| `dic_min_unconstrained.ri` — `a = auto(free)` + `minimize (a-3)²`, no constraints | `a = undef (awaiting solve)` — objective dropped at `registry.rs:182`, never mentioned |
| `dic_inert_connect.ri` — `connect a @ face("top") -> b @ face("bottom")` | `INDETERMINATE frame_align_a_b` + reason-string warning; non-strict: `No constraints violated (1 indeterminate).` exit 0; strict: misattributed generic reason, exit 0 |

**Grammar (G3): no novel syntax.** All fixtures parse today (`tree-sitter parse --quiet`, 0 ERROR nodes, 2026-07-24). This PRD adds diagnostics, an additive trait method, an additive outcome-entry field, and CLI summary changes — no grammar work. `grammar_confirmed = true` for every leaf.

---

## §3 — Resolved design decisions

1. **Relate zero-auto = static verification, not just a notice** (the investigation's preferred fix). A scope with ≥1 relation and 0 auto unknowns runs collect → realize (same shared sub-build) → evaluate each relation as a static assertion at the subs' fixed placements (identity/declared poses — the same witness convention the solve path uses). Satisfied ⇒ silent pass, counted in the ledger; violated ⇒ the existing geometric conflict/assertion Error (fails the build, consistent with auto-ful relate); unverifiable (datum unrealizable, no kernel on this path) ⇒ a diagnostic naming why the relate block was not consumed. Never silently skipped.
2. **Envelope verdicts derive from the live registry** — landing-order independence with PRD 2. Each registered `ConstraintSolver` (and CrossDomain strategy) declares which auto-param kinds it can represent (additive trait method; conservative default = Scalar-only so out-of-tree impls fail safe). At dispatch, a component containing an auto kind the receiving solver does not support is **refused with a typed diagnostic** (naming params, kinds, the receiving solver, and the registered envelope union) instead of run-to-garbage; refused autos get a recorded undef cause. Today that fires for `Bool`/`Int`/`Enum`/`String` autos (honest, strictly better than the misleading residual); after PRD 2 registers CpSat + `DiscreteFirstFallback`, discrete kinds route and solve, and the diagnostic remains for kinds nothing supports (`String`, geometry). The pinned leaf fixture uses `String` (never solver-representable) so its RED assertion is stable across PRD 2's landing.
3. **Objective consumption is measured transitively; routing is untouched.** C's consumption test = the objective's term ValueRefs, closed over `build_dependent_cells` reachability, intersected with the scope instance's **unresolved** auto set. (a) **Structurally inert** — the template's objective references only cells that are never auto-declarable ⇒ **compile error** (INV-SF-3's "structurally incapable" clause; fixtures `dic_min_no_autos`, `dic_min_unread`). (b) **Zero-component consumption** — transitive auto-reach nonempty but zero solver components consumed the objective (unconstrained-minimize shape; registry facts from the three drop sites) ⇒ **Error diagnostic at the objective path** naming the objective and the unconsumed autos. (c) **Vacuously moot** — every objective-reachable auto-declared cell is concretely bound this run ⇒ **quiet** (healthy path: a fully-specified instantiation of an optimizable template must not error — INV-SF-2 severity-hygiene corollary). The registry's component-0 fallback and objective-side coupling into the solve are explicitly NOT changed here (PRD 2 α owns the fold/routing); C only reads facts.
4. **Typed `IndeterminateReason`, additive.** `ConstraintCheckEntry` gains `indeterminate_reason: Option<IndeterminateReason>`; `reify_ir::Satisfaction` is untouched (frozen shape; MCP/GUI compat). Variants type the *existing* recorded knowledge: `UndefInputs{cells}`, `OperatorUndefinedForKinds{kinds}`, `MeasurementUnavailable{detail}` (GD&T), extensible. Populated at the sites that know (`SimpleConstraintChecker::check` via a typed `classify_undef`; the GD&T resolution path). Strict-mode detail and the per-constraint report render the **recorded** reason; the generic guessed text at `main.rs:2340-2356` is deleted (a recorded cause is never overwritten with a guessed one — INV-SF-1 house rule applied to constraint outcomes).
5. **Inertness is a proven property; never a false Inert.** A constraint is INERT (violates INV-SF-4) only when its indeterminacy is provably run-invariant. v1 proves exactly the probe-verified class: ad-hoc `@face`/`@edge`-selector-derived `frame_align` operands, which no evaluation path can ever resolve (static fact about the evaluator, known at generation time). ε therefore (i) refuses **generation** at compile time in `connect.rs` with an Error naming the selector operands and the unverifiable relation (the `connect_compat` half of the connect is unaffected), and (ii) adds an eval-side backstop classifying the same proof as `Structural` if it ever reaches an outcome. Unproven cases remain `Transient` with their recorded reason — mirroring never-false-Violated. ε's diff must also sweep `examples/` + `stdlib/` for shapes the new error rejects and repair them to intent (drop the dead selector half or use a supported form); the shipped-example sweep is part of ε's definition of done.
6. **The check summary becomes a consumption ledger.** `finish_check` accounts every declaration class: constraints `N: M satisfied, V violated, K indeterminate (by reason class), I inert`; objectives (declared/consumed/inert); relate blocks (verified-static / solved / not-evaluable-under-check with reason). **Inert is a failing outcome class** through check's native `check_fails` path (like Violated, independent of strict) — inert is unrepresentable as a passing state. Transient indeterminate keeps today's semantics (green non-strict, failing under `--strict`), now with attributable reasons. Ledger facts flow from the outcome entries (δ) + engine relate/objective consumption facts (α, γ); counting logic lives once, CLI renders it (G7 `no-lockstep-duplication`).
7. **Every new diagnostic carries a `DiagnosticCode`** (INV-SF-6); exact spellings follow the reify-core registry conventions (tactical, §10). Aggregation follows the #5014 collateral-observability pattern: one diagnostic per *declaration* (per relate block / objective / auto param), naming the full affected set — never per-trial or per-component spam (G7 `storm-escape-required` analogue for diagnostic volume).
8. **Determinism:** all new diagnostics and ledger counts are pure functions of the compiled module + solve outcomes — no clocks, no RNG, bit-identical across runs.

---

## §4 — Contract (B + H light)

### §4.1 — Solver capability envelope (B)

```rust
/// Additive trait method on ConstraintSolver (and the CrossDomain strategy):
/// which auto-param kinds can this solver represent as trial/solved values?
/// Conservative default: continuous Scalar only.
fn supports_auto_kind(&self, ty: &Type) -> bool { matches!(ty, Type::Scalar { .. }) }
```

- Screen point: registry dispatch (`solver_for` / `solve_inner`), after classification, before any solve. A component with any unsupported auto kind for its routed solver ⇒ no solve attempt; structured refusal `{component, offending: [(param, kind)], solver, registered_envelope}` → engine renders ONE typed Error diagnostic per component and records per-auto undef causes.
- Invariant E1 (no false refusal): a component every-auto-supported solves exactly as today, byte-identical.
- Invariant E2 (no kind clobber): a solver never writes back a value of a different kind than the auto's declared type; the envelope screen makes the `DimensionalSolver` Scalar write-back unreachable for non-Scalar autos.
- Invariant E3 (registry-derived): verdicts consult the live registered set — registering a new solver (PRD 2 γ) widens the envelope with zero changes here.

### §4.2 — Objective consumption facts (C)

- The registry's three drop sites (`:152`, `:182`, `:208`) emit structured facts (objective present + consumption result) instead of silently returning; the engine's objective path (beside `engine_eval.rs:4861-4878`) renders decision-3's diagnostics from those facts plus the transitive-reach test. Facts, not behavior: solve results are unchanged in every case that solves today.
- Invariant O1: no diagnostic on any objective that governs ≥1 solved component (fixtures: every existing objective test stays byte-identical).
- Invariant O2 (vacuous-healthy): an all-bound instantiation of an optimizable template emits nothing.
- Invariant O3: the structurally-inert compile error fires per template, at `reify check` time, without a solve.

### §4.3 — Indeterminate reason + ledger (D)

- `IndeterminateReason` enum (reify-eval outcome layer): `Transient(TransientReason)` | `Structural(StructuralReason)`; `TransientReason ∈ {UndefInputs{cells}, MeasurementUnavailable{detail}, …}`; `StructuralReason ∈ {UnresolvableOperandKinds{kinds, origin}}` (extensible).
- Invariant R1 (attributable): every Indeterminate outcome carries a reason; the CLI renders the recorded reason, never a synthesized generic.
- Invariant R2 (never-false-Inert): `Structural` only via a compile-time-provable class; anything unproven is `Transient`.
- Invariant R3 (ledger honesty): ledger counts are derived from the same outcome entries the per-constraint report prints — one source, two renderings; inert ⇒ `check_fails() == true` regardless of `--strict`.
- Invariant R4 (no regression): fixtures with only transient indeterminates keep today's non-strict green / strict-fail semantics, with reasons now named.

### §4.4 — Relate static verification (A)

- Zero-auto arm in `solve_scopes`: collect → shared realize → per-relation static residual at fixed placements → Satisfied (count) / Violated (existing geometric Error rendering) / Unverifiable (diagnostic naming relation + why).
- Invariant V1: auto-ful scopes byte-identical to today. V2: a violated static relation fails the build exactly as a violated solved-remainder does. V3: consumption facts (verified/violated/unverifiable counts per scope) surface to the ledger.

---

## §5 — Boundary-test sketch (B + H, two-way)

| # | Side | Scenario | Preconditions | Postconditions (asserted) |
|---|---|---|---|---|
| B1 | producer (α) | violated static relate caught | `dic_relate_static_violated` (relations false at fixed placements) | `reify eval`/`build` emits the geometric violation Error naming the relation; baseline today: silence + green check |
| B2 | producer (α) | satisfied static relate passes silently | `dic_relate_static_ok` | zero relate diagnostics; build succeeds; ledger row counts it verified |
| B3 | back-compat (α) | auto-ful relate unchanged | any existing relate e2e fixture | byte-identical diagnostics + poses (V1) |
| B4 | consumer (β) | unsupported-kind refusal | `dic_string_auto` | typed Error names `s`, `String`, receiving solver + envelope; NO `max absolute residual` text; `note: s is undef (because: …)` carries the refusal; baseline: misleading residual error |
| B5 | back-compat (β) | supported kinds untouched | existing continuous fixtures (e.g. `objective_set_weighted.ri`) | byte-identical (E1) |
| B6 | producer (γ) | structurally-inert objective is a compile error | `dic_min_no_autos`, `dic_min_unread` | `reify check` emits the inert-objective Error per template; baseline: total silence / silent uselessness |
| B7 | producer (γ) | zero-component consumption diagnosed | `dic_min_unconstrained` | Error names the objective + unconsumed auto `a`; `a`'s undef note unchanged; baseline: objective never mentioned |
| B8 | back-compat (γ) | governing objectives quiet | every existing objective fixture | no new diagnostics (O1); vacuous-bound instantiation quiet (O2) |
| B9 | producer (δ) | recorded reason rendered | `dic_inert_connect` under `--strict` | strict detail shows "operator undefined for these operand kinds …" (the recorded reason); the generic "inputs undefined (e.g. …)" guess is gone |
| B10 | consumer (ε) | inert connect refused at compile time | `dic_inert_connect` | `reify check` emits the generation-refusal Error naming the `@face` operands; `connect_compat_a_b` still reported OK; baseline: green + INDETERMINATE |
| B11 | consumer (ζ) | ledger accounts all classes | mixed fixture (satisfied + violated + transient-indeterminate + relate block) | summary shows per-class counts with reason breakdown; relate + objective rows present; transient-only stays non-strict green |
| B12 | consumer (ζ) | inert fails check natively | any fixture with a proven-inert constraint (pre-ε-sweep shape) | `check_fails` true without `--strict`; exit-code behavior itself deferred to the sibling severity-gate PRD |

---

## §6 — Pre-conditions for activating

**None — immediately decomposable.** All substrate is landed on main `0d70ef1d5b` (§2.1); the invariants doc lands via task/5395 (deferred landing vehicle; this PRD cites slugs, not text, so ordering is documentation-only). No grammar work. PRD 2 (task/5396) is a coordination neighbour, not a prerequisite (§3 decisions 2–3).

---

## §7 — Cross-PRD relationship (G4)

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| sibling "eradicate silent undef" (INV-SF-1/2/6, same investigation, in parallel authoring) | coordinates | Error-severity ⇒ nonzero-exit gate; undef provenance backstops; diagnostic-code registry hygiene | **sibling** owns exit codes + provenance; this PRD emits coded diagnostics + the check-native inert failure class | authoring in parallel — no task ids yet; boundaries fixed by the shared spawn briefs |
| sibling "placeholder-type ratchet" (INV-SF-5) | none | no shared mechanism | n/a | independent |
| `discrete-cost-minimisation.md` (PRD 2, task/5396) | coordinates | solver registry contents + objective coupling; B's envelope widens automatically when γ registers CpSat (E3); C reads transitive reach, never routes; file adjacency in `registry.rs`/`solver.rs` | **PRD 2** owns capability (wiring, routing, domains, discrete solve); **this PRD** owns accounting (refusal, consumption diagnostics) | PRD 2 authored, decompose pending; wire ordering deps at decompose IF its task ids exist, else the design is landing-order-independent by construction |
| #5388 (strict-auto uniqueness, continuous, blocked) | none | disjoint mechanism (uniqueness honesty vs consumption accounting) | each its own | cross-referenced |
| #4388 (geometric-relations θ: DOF ledger, `reify explain`, `W_UNDERDETERMINED`) | adjacent | relate diagnostics for **auto-ful** scopes | **#4388** owns deep relate diagnostics; **this PRD** owns the zero-auto arm + ledger counting | pending; seam note in α/ζ task text |
| #5189 (JOINT-DRIVE β, `build_trial_values` per-trial recompute) | adjacent | `solver.rs` trial-value construction | #5189 owns per-trial folds; B's screen sits **before** any solve, no shared logic | pending; file-adjacency noted |
| `engine-integration-norm.md` §3.5 | extends impl | additive trait method + registry screen inside the catalogued seam | seam catalog unchanged | no norm edit needed |

No new contested-ownership pair (checked against `phase-3-breadcrumb-map.md` §3).

---

## §8 — Decomposition plan

Bare-B+H-light shape; Greek labels, task IDs at decompose. **Test-layout note (drift-guard check):** no new `tests/infra/*.sh`, no wall-clock assertions, no new standalone `crates/*/tests/*.rs` binaries — eval-side e2e tests extend `crates/reify-eval/tests/harness_engine.rs` (harness-layout ratchet, Leo ruling 2026-07-22); CLI assertions extend the existing reify-cli test surface; solver-level tests are `#[cfg(test)]` units. Drift-guard registrations N/A by construction.

- **α — Relate zero-auto static verification arm.**
  Modules: `crates/reify-eval/src/relate_solve.rs` (zero-auto arm in `solve_scopes`; static per-relation verification reusing the remainder-verify machinery), `crates/reify-eval/src/engine_build.rs` (surface diagnostics + consumption facts), `crates/reify-core` (code).
  **LEAF signal:** B1/B2 — `reify eval` on `dic_relate_static_violated.ri` emits the geometric violation Error (baseline silence probe-verified 2026-07-24); `dic_relate_static_ok.ri` stays silent-pass; existing auto-ful relate fixtures byte-identical (B3).
  Prereqs: none. `grammar_confirmed=true`.

- **β — Solver capability envelope + typed unrepresentable-kind refusal.**
  Modules: `crates/reify-constraints/src/{lib.rs,registry.rs,solver.rs,cpsat.rs}` (trait method + conservative default; registry screen; envelope impls), `crates/reify-eval/src/engine_eval.rs` (diagnostic rendering + undef-cause recording), `crates/reify-core` (code).
  **LEAF signal:** B4/B5 — `reify eval dic_string_auto.ri` emits the typed refusal (no `max absolute residual` text; baseline misleading-residual probe-verified); continuous fixtures byte-identical. Coordination: if PRD 2's γ task exists at decompose, wire an ordering dep (this after it) to serialize `registry.rs` edits; otherwise E3 makes landing order semantically irrelevant.
  Prereqs: none (see coordination note). `grammar_confirmed=true`.

- **γ — Objective consumption accounting (compile inert-error + zero-component diagnostic).**
  Modules: `crates/reify-compiler` (template-structural inert-objective error), `crates/reify-eval/src/engine_eval.rs` (transitive-reach test via `build_dependent_cells`; diagnostic beside the #4804 site), `crates/reify-constraints/src/registry.rs` (structured facts from the three drop sites — facts only, no routing change), `crates/reify-core` (code).
  **LEAF signal:** B6/B7/B8 — `reify check dic_min_no_autos.ri` / `dic_min_unread.ri` emit the inert-objective Error (baseline silence probe-verified); `reify eval dic_min_unconstrained.ri` emits the zero-component Error naming `a`; existing objective fixtures quiet.
  Prereqs: none. `grammar_confirmed=true`.

- **δ — Typed `IndeterminateReason` on the outcome carrier + recorded-reason rendering.**
  Modules: `crates/reify-constraints/src/lib.rs` (`classify_undef` → typed), `crates/reify-eval` (entry field + population at `engine_constraints.rs` sites incl. GD&T), `crates/reify-cli/src/main.rs` (`report_indeterminate_detail` renders recorded reasons; delete the guessed generic).
  **LEAF signal:** B9 — `reify check --strict dic_inert_connect.ri` detail names "operator undefined for these operand kinds" (baseline misattribution probe-verified); transient fixtures name their real reasons.
  Prereqs: none. `grammar_confirmed=true`.

- **ε — Inert-constraint detection: compile-time generation refusal + eval backstop + example sweep.**
  Modules: `crates/reify-compiler/src/connect.rs` (refuse generating provably-unverifiable `frame_align` with a coded Error), `crates/reify-eval` (backstop `Structural` classification), `crates/reify-core` (code), `examples/` + `crates/reify-compiler/stdlib` (sweep + repair shapes the new error rejects).
  **LEAF signal:** B10 — `reify check dic_inert_connect.ri` emits the generation-refusal Error while `connect_compat` still reports OK (baseline green-with-INDETERMINATE probe-verified); the repo's own examples/stdlib check clean post-sweep.
  Prereqs: δ (reason taxonomy carrier). `grammar_confirmed=true`.

- **ζ — `reify check` consumption ledger (integration gate).**
  Modules: `crates/reify-cli/src/main.rs` (`finish_check` / `report_constraint_results` ledger rendering; inert as native failing outcome class in `check_fails`), `crates/reify-eval` (consumption-fact plumbing from α/γ; single counting source).
  **LEAF signal (integration gate):** B11/B12 — a mixed fixture's `reify check` summary accounts constraints (per-class, reasons), objectives, and relate blocks; inert fails check without `--strict`; transient-only fixtures keep today's non-strict green. This is the §5 sketch executed end-to-end.
  Prereqs: α, γ, δ, ε. `grammar_confirmed=true`.

- **η — Companion docs: spec + cross-refs.**
  Modules: `docs/reify-language-spec.md` (check-summary/ledger + new diagnostic semantics; the consumption-accounting contract in the constraints/objectives sections), `docs/legibility/confusion-codebook.yaml` seam note if applicable.
  **LEAF signal (docs):** committed prose documenting the ledger + the four accounting mechanisms; spec's check description matches ζ's output.
  Prereqs: ζ. `grammar_confirmed=true`.

**Dependency view**
```
α ─────────┐
γ ─────────┤
δ → ε ─────┼→ ζ → η
β (independent leaf; ordering-dep on PRD 2 γ iff filed at decompose time)
```

---

## §9 — Out of scope for this PRD

- Exit-code semantics for Error diagnostics (INV-SF-2) → sibling silent-undef PRD. (ζ's inert-fails-check rides the native outcome path only.)
- Discrete/mixed solve capability, CpSat wiring, let-tracing, `Int`/`discrete_set` domains → PRD 2 (task/5396).
- Objective-only component construction (making unconstrained `minimize` solve) — C diagnoses its absence; building it is a follow-up capability with no current owner (candidate future PRD; noted in η's spec prose as a named gap).
- Deep relate diagnostics for auto-ful scopes (DOF ledger, `reify explain`) → #4388.
- Uniqueness honesty (continuous #5388; discrete PRD 2 b4).
- Widening the proven-inert class beyond ad-hoc `@face`/`@edge` selectors (extensible taxonomy is the substrate; each new proof is its own small task).
- DFM "silently skipped rules with unrealized handles" (INV-SF-3 evidence list) — same family, separate surface; file when the DFM census lands (the ledger's shape is reusable).

---

## §10 — Open questions (tactical — decide at implementation time)

1. **Diagnostic code spellings** (`E_AUTO_KIND_UNSUPPORTED`, `E_OBJECTIVE_INERT`, `E_OBJECTIVE_UNCONSUMED`, `E_CONSTRAINT_INERT`, relate-unverifiable) — follow reify-core registry conventions; decide in each owning task.
2. **Registry-fact plumbing shape** — extend the `solve_inner` return tuple vs a facts struct on the result; pick whichever avoids `SolveResult` shape changes (frozen per F-result I1). Decide in γ/β.
3. **Ledger exact format** — line layout, reason-class ordering, whether relate/objective rows print when zero declarations exist (suggest: omit empty classes). Decide in ζ.
4. **`IndeterminateReason` variant granularity** — start with the three typed variants (§4.3) and extend; whether GD&T detail stays free-text inside `MeasurementUnavailable`. Decide in δ.
5. **Static-relate witness for `at <pose>` subs** — fixed placements are the declared poses; confirm the realization path exposes them uniformly (identity when absent). Decide in α.
6. **Example-sweep extent for ε** — repo grep for `@ face(`/`@ edge(` connect shapes; repair strategy per instance (drop vs supported form). Decide in ε.
