# Discrete cost minimisation — CP-SAT deployment, let-tracing, honest discrete optimality (PRD 2)

**Milestone:** v0_6 · **Status:** active (authored 2026-07-24 in a `/prd` session under G1–G7+META; scope decisions resolved with Leo 2026-07-24) · **Approach:** B + H
**Cluster:** `cost-optimisation`. Charter inherited from `continuous-cost-minimisation.md` (PRD 1, landed) §0.1: **supplier / stock-size / count selection** — `Int`/`Bool`/`Enum` autos plus discrete-set `Real`s. Consumes `ranked-solve-result.md` (F-result, landed): PRD 2 owns the CP-SAT `solve_ranked` override (F-result §0.1/§6). Mixed discrete+continuous follows `whole-model-objective-coupling.md` (M-WHOLE) §3.1: **CP-SAT outer enumeration wrapping the continuous inner solve; MINLP rejected** — binding, do not re-litigate.

---

## §0 — Purpose and scope

A designer who writes the *natural* formulation of a discrete choice today gets a wrong or misleading answer. Four failures, all **empirically reproduced 2026-07-24** (probe fixtures §2.3; binary 2026-07-22, source anchors re-verified on main `0d70ef1d5b`):

1. **CP-SAT is never wired.** `SolverRegistry::production()` leaves both the `logical` and CrossDomain `fallback` slots `None` (`crates/reify-constraints/src/registry.rs:42`), so every discrete component lands on `DimensionalSolver`, whose trial values are structurally `Value::Scalar` only — a natural Bool-auto formulation fails with the **misleading** `constraints could not be satisfied (max absolute residual: 1.00e0)`.
2. **`let`-indirection severs constraint↔auto connectivity** in three one-hop-blind layers (§2.2). Constraints that read autos *through a `let`* are silently dropped before any solver sees them: the autos report `W_UNDERDETERMINED` + `undef`. This bug is solver-independent — it breaks **continuous** formulations too (probe `dcm_let_cont.ri`, §2.3), and the spike proved CP-SAT alone does not fix it.
3. **The discrete domain channel is missing.** `AutoParam.bounds` is *always* `None` (`crates/reify-eval/src/engine_eval.rs:1439`), so `Int` autos can never enumerate; `@solver_hint("discrete_set", collection)` is parsed and validated (`crates/reify-compiler/src/annotations.rs:279`) but has **zero solver-side readers** (audit finding M-008) — stock-size selection over a catalog is exactly its intended semantics and it is dead at the IR seam.
4. **Honesty gaps.** `CpSatSolver::solve()` hardcodes `unique: true` (`cpsat.rs:202/243`) — a determinacy lie for multi-solution problems (the discrete twin of in-flight #5388); and under an objective CP-SAT silently ignores it *and* no `W_SOLVER_OPTIMALITY_UNPROVEN` fires.

**What ships (Leo 2026-07-24: scope (b)+(d) as one PRD):** the let-tracing transitive fix; the discrete domain channel (`Int` bound-mining + `discrete_set` wiring); CP-SAT **default-ON** (no env flag — the spike's `REIFY_DEV_CPSAT` gate is scaffolding, not the landing shape) with a `DiscreteFirstFallback` CrossDomain strategy; a CP-SAT `solve_ranked` override doing **complete enumeration + argmin**, producing the codebase's **first honest `OptimalityStatus::ProvenOptimal`**; enumeration-backed `unique` honesty; and the **mixed** discrete×continuous outer loop per M-WHOLE §3.1.

### §0.1 — What this is NOT (scope boundaries, resolved 2026-07-24)

- **NOT the `solutions()` query surface (path c).** First-class queryable solution sets — purposes as named solve scopes, `solutions(P)` returning **records of the auto assignments** (Leo chose records over materialized instances for v1) — is a dep-gated follow-on: bookmark task θ (`[MILESTONE]`, design-first G5 on activation). This PRD builds the enumeration engine it needs (`solve_all`, the mixed enumerate×residual loop) but **no language surface**.
- **NOT MINLP.** SCIP/`russcip` monolithic mixed-integer-nonlinear was **rejected** in M-WHOLE §3.1 (determinism + dependency weight). Do not re-open.
- **NOT functional-enumeration ergonomics.** Recursion bug, dead `filter`/`map` lambda combinators, doc overstatement → already filed as #5393 (pending, orthogonal).
- **NOT geometry-dependent cost** (volume/waste/nesting) → M-WASTE (`material-waste-cost-minimisation.md`, milestone 4787).
- **NOT the continuous-side uniqueness fix.** #5388 (blocked, in-flight) owns the perturbation-based re-solve's basin-blindness in `solver.rs`/`registry.rs`. This PRD's b4 is its **discrete twin in `cpsat.rs`** — no file overlap; coordinate by cross-reference, don't duplicate.
- **NOT `prefer_stock` / `preferred_strategy` hint consumption.** Only `discrete_set` is wired here; the other two `SolverHintKind`s remain compile-checked no-ops (unchanged).

---

## §1 — Consumer (G1)

| Mechanism | Consumer |
|---|---|
| Let-tracing transitive closure + shared per-trial dependent-cell fold (α) | The existing `build_solver_problem` → `SolverRegistry` path (**§3.5 ConstraintSolver seam**); user surface: `reify eval` on a let-indirected fixture — continuous (α's own leaf signal) and discrete (γ/ε). |
| Discrete domain channel: `Int` bound-mining + `discrete_set` → `AutoParam.domain` (δ) | `CpSatSolver::build_variable_domain` (existing fn, `cpsat.rs:43` — today errors for unbounded `Int` and cannot see `Real` catalogs); user surface: the stock-size pick example (ε). |
| CP-SAT default-ON + `DiscreteFirstFallback` (γ) | `SolverRegistry::production()`'s two on-main call sites — `crates/reify-cli/src/main.rs:1319` and `gui/src-tauri/src/engine.rs:1981` — i.e. **both** shipped binaries. |
| `CpSatSolver::solve_all` + the `solve_ranked` override (β) | The engine objective path `engine_eval.rs:2897` → `SolverRegistry::solve_ranked` per-component dispatch (`registry.rs:280`, landed via F-result γ #4804 / M-WHOLE δ #5016). F-result §0.1 names **PRD 2** the sole owner of this override and of `ProvenOptimal`. Forward consumer: the `solutions()` surface (bookmark θ). |
| Enumeration-backed `unique` honesty (β, wired by γ) | The strict-`auto` uniqueness surface: `reify eval` "not uniquely determined" diagnostics now tell the truth for discrete components. |
| Mixed enumerate×residual outer loop (ζ) | The CrossDomain slot of the §3.5 seam (`registry.rs:90`); user surface: the mixed fixture (ζ's leaf signal). |

**Engine-integration sub-check (G1).** Every solver-side mechanism plugs into the catalogued **§3.5 ConstraintSolver** seam (`docs/prds/v0_3/engine-integration-norm.md`): γ *populates* the registry's existing `logical`/`fallback` slots; β overrides the `solve_ranked` trait method F-result already catalogued; α/ζ extend problem construction and dispatch inside the same seam. **No new seam; no orphan-producible `pub fn` in a kernel crate.**

---

## §2 — Background & substrate (verified in-tree 2026-07-24, main `0d70ef1d5b`)

### §2.1 — Landed substrate this PRD builds on

- **F-result carrier (landed):** `RankedSolveResult`/`RankedCandidate`/`OptimalityStatus` (`crates/reify-ir/src/ranked.rs`); defaulted `ConstraintSolver::solve_ranked` + `DimensionalSolver` override; invariants I1–I5. The engine objective path already calls `solve_ranked` and emits `W_SOLVER_OPTIMALITY_UNPROVEN` **iff** `optimality == BestFound` (#4804) — so an honest `ProvenOptimal` suppresses the warning with **zero engine changes**.
- **Registry per-component dispatch (landed, M-WHOLE δ #5016):** `SolverRegistry::solve_ranked` routes each decomposed component through the domain solver's own `solve_ranked` (`registry.rs:280`) — the CP-SAT override becomes reachable the moment γ populates the slots. Note: `registry.rs:244` currently passes `dependent_cells: Vec::new()` into sub-problems — one of α's three layers.
- **`dependent_cells` machinery (landed, #5188):** `ResolutionProblem.dependent_cells` carries the topo-ordered coupled non-auto cells, but it is consumed **only** by post-solve write-back (`materialize_dependent_cells`, `engine_eval.rs:1684`); `build_dependent_cells` (`engine_eval.rs:1553`) is the reusable transitive-closure builder.
- **CP-SAT solver (landed, unwired):** forward-checking backtracker with domains — `Bool` intrinsic, `Int` from `param.bounds` capped at `MAX_INT_DOMAIN = 1000`, `Enum` **constraint-mined** from variant literals (`cpsat.rs:43–115`). The Enum arm is the in-tree precedent for δ's `Int` bound-mining.
- **Classifier (landed):** a constraint mixing a Bool ref with numeric arithmetic classifies **CrossDomain, not Logical** (`classifier.rs`, `has_numeric && has_logical`) — this is *why* wiring CP-SAT into the `logical` slot alone is insufficient and `DiscreteFirstFallback` must own the CrossDomain slot.
- **`@solver_hint` compile chain (landed, dead at IR seam):** `extract_solver_hints` (`annotations.rs:264`), `ValueCellDecl.solver_hints` (`reify-compiler/src/types.rs:1125`), collection validation, stdlib catalogs (`crates/reify-compiler/stdlib/standard_stock.ri`: `standard_bolt_lengths`, `standard_sheet_thicknesses`), and the shipped example `examples/m11_annotations.ri` (BoltedPanel). Zero solver-side readers (M-008).
- **Money-objective machinery (landed, PRD 1 #4789/#4791):** robustness floor synthesised in **`DimensionalSolver` problem assembly** (`solver.rs:487`) + `cost_robustness_tradeoff`. Because the floor is a DimensionalSolver mechanism, the mixed inner residual solve inherits it **unchanged**, and the pure-discrete path never fabricates a floor (§3 decision 8).
- **Spike (proof of wiring, not landing shape):** branch `spike/discrete-solve-cpsat`, commit `75cf3b4d19` (worktree `/home/leo/src/warm-lanes/personal/discrete-solve-spike`). Proves: registry wiring (~30 lines) + `DiscreteFirstFallback` → natural 6-Bool hexagon balance solves in **0.46 s** (flag off: byte-identical baseline); `CpSatSolver::solve_all(problem, cap)` with `SolveAllResult { solutions, complete }` + 3 unit tests. **Adapt this code; strip the env flag.**

### §2.2 — The let-tracing gap: three one-hop-blind layers (α's charter)

1. `filter_constraints_reading_autos` (`engine_eval.rs:1410`) keeps a constraint only if it **directly** reads an auto id → let-indirected constraints never enter the `ResolutionProblem`.
2. `decompose_into_components` (`crates/reify-constraints/src/decompose.rs:110–150`) builds connectivity edges only from **direct** `ValueRef`s ∩ auto params → even admitted constraints can't couple through a `let`.
3. **No per-trial fold:** neither `DimensionalSolver` residual evaluation nor CP-SAT's forward-check materializes `dependent_cells` per trial assignment, and `registry.rs:244` passes `Vec::new()` into sub-problems — so a constraint over `let f = if up then 1.0 else -1.0` cannot be evaluated against a trial `up`.

Fix shape (α): transitive closure in (1) and (2) **reusing `build_dependent_cells`**; **one shared** per-trial fold helper used by both solvers (§3 decision 9); thread `dependent_cells` through registry sub-problems. The infrastructure exists; the fix is mechanical.

### §2.3 — Empirical baseline (probes 2026-07-24; fixtures under `docs/prds/v0_6/fixtures/`)

| Probe | Today's behaviour (verified) |
|---|---|
| `n1` natural inline-Bool hexagon balance | `constraints could not be satisfied (max absolute residual: 1.00e0)` — misleading |
| `n2` same, forces named via `let` (the natural authoring shape) | all autos `undef`, constraints silently dropped |
| `discrete_let_cont.ri` — **continuous** autos, constraints via `let` (`s = a+b`, `s == 10`) | `W_UNDERDETERMINED: … not touched by any constraint` + `undef` — the gap is solver-independent |
| `discrete_int_auto.ri` — strict `Int` auto with comparison bounds | `strict auto parameter resolution is not uniquely determined` + `undef` — Int enumeration impossible |
| `discrete_mixed.ri` — Bool + Real coupled, `minimize` | `constraints could not be satisfied (max absolute residual: 1.00e0)` |
| Spike n3/n3b — `minimize` over Bools (CP-SAT wired) | solves, but objective **silently ignored** (flipped objective → same config) and **no** `W_SOLVER_OPTIMALITY_UNPROVEN` |

**Grammar (G3): no novel syntax.** All fixture shapes parse today (`tree-sitter parse --quiet`, 2026-07-24): `@solver_hint("discrete_set", standard_bolt_lengths)` on an auto param (the shipped `examples/m11_annotations.ri` surface), array literals, `param n : Int = auto`, `param s : Supplier = auto` with `Supplier.Acme` variant refs, `minimize` in a mixed structure. `grammar_confirmed = true` for every leaf.

**Semantic-substrate wrinkle (probe 2026-07-24):** while an array-literal catalog *parses*, `@solver_hint("discrete_set", <local-let ident>)` is **compile-rejected today** (`error: unknown selector kind '@solver_hint'`, exit 1) — the annotation's collection argument validates only against **registered stdlib collections** (`standard_bolt_lengths`, `standard_sheet_thicknesses`; `structure def` + stdlib ident checks clean, exit 0). v1 therefore scopes `discrete_set` catalogs to the stdlib surface (§3 decision 7); user-defined local catalogs are an explicit out-of-scope follow-up (§9).

---

## §3 — Resolved design decisions

1. **Default-ON, no env flag** (Leo 2026-07-24). Today's Bool-auto behaviour is strictly a bug — a misleading diagnostic with nothing to preserve. Landing order is the only constraint: **γ (wiring) hard-depends on α (let-tracing) and β (honest CP-SAT core)**, so the day CP-SAT becomes reachable it is already honest (no `unique:true` lie, no silent objective-ignore) and the natural let-indirected formulation works. The spike's `REIFY_DEV_CPSAT` gate does not land.
2. **Scope (b)+(d) in one PRD; (c) is a bookmark.** CpSat deployment + let-tracing + minimize-over-discrete ship here; the `solutions()` surface is bookmark θ, design-first on activation.
3. **Mixed = CP-SAT outer enumeration × continuous inner residual solve** (M-WHOLE §3.1, binding): enumerate discrete sub-assignments with the CP-SAT backtracker (constraints touching continuous autos are non-prunable at enumeration time); for each leaf, fix the discrete values and run the continuous residual sub-problem through `DimensionalSolver`; a candidate = (discrete assignment + continuous witness). The backtracker takes this as a **leaf callback** (structurally small; the spike assessment confirms the shape).
4. **Optimality honesty (F-result I3 refined).** `ProvenOptimal` **iff** complete enumeration of a pure-discrete component (every leaf visited or pruned soundly, no cap hit). Mixed components are **always `BestFound`** — the continuous inner argmin is Nelder-Mead (budget-bounded, unproven), so complete discrete enumeration still cannot prove global optimality. Capped/incomplete enumeration → `BestFound { reason }` → the engine's existing warning fires. This PRD produces the codebase's **first** `ProvenOptimal`; F-result §0.1 grants that exclusively to PRD 2.
5. **`unique` honesty by enumeration (b4).** `CpSatSolver::solve()` = `solve_all(cap = 2)`: `unique = complete && solutions.len() == 1`. First-feasible selection order stays deterministic. Discrete twin of #5388 (continuous side, blocked, `solver.rs`/`registry.rs` — no file overlap with `cpsat.rs`); coordinate by cross-reference.
6. **`Int` domains are constraint-mined** (δ), consistent with the landed Enum precedent in the same function: `build_variable_domain` derives `[lo, hi]` from the component's **direct comparison constraints against compile-time constants** (`n >= k`, `n <= k`, `n == k`; conservative — mining is a fallback when `param.bounds` is `None`). No mineable finite domain → a **typed diagnostic** (never a silent skip, never a string-only `Err`): "Int auto has no derivable finite domain; add bounding constraints or `@solver_hint("discrete_set", …)`".
7. **`discrete_set` is WIRED, not deleted** (δ). Thread `ValueCellDecl.solver_hints` compiler → IR cell decl → `build_auto_param_list`, which resolves the hint's **registered stdlib collection** (the only surface the compiler validates today, §2.3 — a zero-arg stdlib fn evaluated once at problem build; empty/unresolvable ⇒ typed diagnostic) into a new `AutoParam.domain: Option<Vec<Value>>`. CP-SAT consumes `domain` for `Real`/`Length`(and any)-typed autos; `DiscreteFirstFallback` counts an auto-with-`domain` as discrete. Stock-size selection over `standard_bolt_lengths`/`standard_sheet_thicknesses` is exactly this semantics. **User-defined (local `let`) catalogs are compile-rejected today and stay out of scope** (§9) — wiring them means extending `validate_solver_hint_collections` + determined-cell resolution, a follow-up once the stdlib surface is proven end-to-end.
8. **Robustness-floor composition** (PRD 1 machinery "must compose"): the floor lives in `DimensionalSolver` problem assembly (`solver.rs:487`), so the **mixed inner residual solve inherits floor + `cost_robustness_tradeoff` unchanged**; the **pure-discrete** path has no continuous slack dial — no floor is fabricated (enumeration evaluates exact feasibility per candidate). This is composition by construction, not an exception.
9. **One fold, no lock-step twins** (G7 `no-lockstep-duplication`): α extracts a **single** `fold_dependent_cells`-style helper (trial values → materialized dependent-cell values, in `build_dependent_cells`' stored topo order) consumed by (a) `DimensionalSolver` residual eval, (b) CP-SAT forward-check, (c) the mixed leaf callback — the existing post-solve `materialize_dependent_cells` either delegates to it or is subsumed by it.
10. **Determinism.** Fixed variable order (declaration order), fixed value order (domain construction order), no RNG, no clock; two identical runs produce bit-identical resolved values and candidate ordering (extends M-WHOLE BT5 to the discrete path).

---

## §4 — Contract (B + H)

### §4.1 — Routing (`DiscreteFirstFallback`, γ/ζ)

`SolverRegistry::production()` installs `CpSatSolver` in the `logical` slot and `DiscreteFirstFallback` in the CrossDomain `fallback` slot. Per decomposed component:

| Component shape | Route |
|---|---|
| every auto is discrete (`Bool` / `Int` / `Enum` / `Real`-with-`domain`) | CP-SAT (pure enumeration) |
| mixed discrete + continuous autos | ζ's enumerate×residual loop; **before ζ lands**: fall through to `DimensionalSolver` (today's behaviour, unchanged — staged, not silent regression) |
| every auto continuous | `DimensionalSolver` (byte-identical to today) |

### §4.2 — Enumeration core (β; adapt spike `75cf3b4d19`)

```rust
pub struct SolveAllResult {
    /// Every solution found, in deterministic search order.
    pub solutions: Vec<HashMap<ValueCellId, Value>>,
    /// True iff the search space was exhausted (no cap/budget hit).
    pub complete: bool,
}
impl CpSatSolver {
    /// All-solutions enumeration sharing the forward-checking backtracker.
    pub fn solve_all(&self, problem: &ResolutionProblem, cap: usize) -> SolveAllResult;
}
```

- `solve()` (trait, unchanged signature): first feasible solution; `unique = solve_all(cap=2)`-derived (§3.5 decision 5).
- `solve_ranked()` (the F-result override this PRD owns): enumerate all solutions under the enumeration budget; evaluate the objective at each via the shared fold + `eval_objective_set`; return `Ranked { candidates (ascending score, I2; truncated to a top-N carrier cap — truncation never affects `optimality`), optimality }` with `optimality = ProvenOptimal` iff `complete` (pure-discrete), else `BestFound { reason: "enumeration budget reached (...)" }`. `objective_score` populated iff an objective governs the solve (I4). No objective → size-1 `Ranked`, `FeasibilityOnly` (I2 feasibility form).

### §4.3 — Mixed outer loop (ζ)

`DiscreteFirstFallback` for a mixed component: backtrack over the discrete subset (constraints reading any continuous auto are non-prunable during enumeration); at each discrete leaf, fold dependent cells, fix discrete values into `current_values`, build the continuous residual sub-problem, and dispatch it through the registry's continuous path (`DimensionalSolver` — floor/tradeoff machinery intact). Candidate = discrete assignment ∪ continuous witness; score = the inner solve's objective score. `solve` = first feasible pair; `solve_ranked` = ranked candidates, **always `BestFound`** (§3 decision 4). Diagnostics from infeasible inner solves are not user-surfaced per leaf (they are pruning evidence), only aggregate infeasibility is.

### §4.4 — IR additions (additive only; F-result I1 untouched)

- `AutoParam.domain: Option<Vec<Value>>` — explicit finite domain from `discrete_set` (δ). `None` = today's semantics everywhere.
- IR-side value-cell decl gains the lowered `solver_hints` (δ) — the compiler already carries them; the seam crossing is the new work.
- `SolveResult`, `ConstraintSolver::solve()`, and the F-result carrier are **frozen** (I1); everything here is a sibling/extension.

### §4.5 — Invariants

- **D1 — no-regression.** A model with no discrete autos and no let-indirected auto-reading constraints resolves **byte-identically** to today. (Let-indirected models change from `undef`/dropped to solved — that is the bug fix, asserted by α's boundary tests.)
- **D2 — optimality honesty.** `ProvenOptimal` ⇔ complete pure-discrete enumeration. Mixed or capped ⇒ `BestFound`. (Refines F-result I3; the engine warning surface then behaves correctly with zero engine changes.)
- **D3 — unique honesty.** For a CP-SAT-routed component, `unique` reflects enumerated reality (`complete && count == 1`), never a hardcoded value.
- **D4 — determinism.** Bit-identical values + candidate order across runs (no RNG/clock/map-iteration leaks).
- **D5 — never silent.** Un-enumerable domain (unbounded `Int`, undetermined/empty `discrete_set` collection, domain-product overflow) ⇒ typed diagnostic; capped enumeration ⇒ `BestFound` (⇒ warning on the objective path); mixed-degrade before ζ ⇒ existing behaviour (no new silence introduced).
- **D6 — single fold.** Exactly one dependent-cell fold helper serves DimensionalSolver residual eval, CP-SAT forward-check, mixed leaf callback, and post-solve write-back.

---

## §5 — Boundary-test sketch (B + H, two-way)

The integration-gate leaf ε names rows B1–B8 as its observable signal; ζ owns B9–B10.

| # | Side | Scenario | Preconditions | Postconditions (asserted) |
|---|---|---|---|---|
| B1 | producer (α) | let-indirected **continuous** constraints enter the solve | `dcm_let_cont` shape: `a,b : Real = auto`, `let s=a+b, d=a-b`, `s==10, d==2` | `reify eval` resolves `a=6, b=4` (solver tol); **no** `W_UNDERDETERMINED`; baseline today: `undef` (§2.3) |
| B2 | producer (α) | direct formulations unchanged | any existing fixture with only direct auto reads (e.g. `objective_set_weighted.ri`) | byte-identical resolved values + diagnostics (D1) |
| B3 | consumer (γ) | natural let-indirected Bool balance solves | `n2` hexagon fixture, `auto(free)` Bools | `reify eval` yields exact `Bool`s satisfying all three balance constraints; no residual-failure error; baseline today: dropped/`undef` |
| B4 | producer (β) | `unique` honesty | strict Bool autos, exactly 2 feasible configs | solve reports non-unique (strict-auto diagnostic fires honestly); with exactly 1 config: `unique == true` via `complete` cap-2 enumeration |
| B5 | consumer (β+γ) | argmin over discrete, proven | `n3` shape: `minimize` over Bool config | selected config is the true argmin; **flipping the objective flips the config** (baseline: same config — silent ignore); `optimality == ProvenOptimal`; **no** `W_SOLVER_OPTIMALITY_UNPROVEN` |
| B6 | producer (β) | capped enumeration stays honest | enumeration budget < solution count (test-scale cap) | `BestFound { reason ~ budget }`; warning fires on the objective path; returned best is best-of-enumerated |
| B7 | consumer (δ) | `Int` bound-mining | `n : Int = auto`, `n>=1, n<=8, 3n>=10, n*n<=17` | `reify eval` resolves `n=4`, reported determined (unique); unbounded variant emits the typed no-finite-domain diagnostic (not a bare error string) |
| B8 | consumer (δ+ε) | `discrete_set` stock pick under cost | catalog via `@solver_hint("discrete_set", standard_bolt_lengths)` (the validated stdlib surface, §2.3), `Money` objective | resolved value **is a catalog member**, is the feasible cost-argmin; `ProvenOptimal`, no warning; no robustness floor fabricated on the pure-discrete path (§3.8) |
| B9 | producer (ζ) | mixed enumerate×residual | `dcm_mixed` shape: `up: Bool = auto`, `t: Real = auto`, `t >= (if up then 3 else 5)`, `t<=10`, `minimize t` | resolves `up=true, t≈3.0` (strictly better than the `up=false` branch's 5.0); `optimality == BestFound` (never `ProvenOptimal`, D2); warning fires honestly; baseline today: residual failure |
| B10 | consumer (ζ) | mixed inner solve keeps PRD 1 machinery | mixed fixture with a `Money` objective + inequality on the continuous part | inner residual solve applies the robustness floor exactly as a standalone continuous solve does (composition §3.8) |
| B11 | back-compat (γ) | determinism | any discrete fixture, two runs | bit-identical values + candidate ordering (D4) |

---

## §6 — Pre-conditions for activating

**None — immediately decomposable.** All substrate is landed on main (§2.1): F-result carrier + engine/registry `solve_ranked` dispatch, `dependent_cells` + `build_dependent_cells` (#5188), the CP-SAT solver + classifier, the `@solver_hint` compile chain + stdlib catalogs, PRD 1's Money machinery. The spike branch is reference material, not a dependency. No grammar work (§2.3).

---

## §7 — Cross-PRD relationship (G4)

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `ranked-solve-result.md` (F-result) | consumes/produces-into | CP-SAT `solve_ranked` override + `ProvenOptimal` production (F-result §0.1/§6 assign both to PRD 2); carrier/trait frozen | **this PRD** owns the override; F-result owns carrier+default | landed substrate; η updates F-result §6's "PRD 2 unauthored" row |
| `whole-model-objective-coupling.md` (M-WHOLE) | consumes | the continuous inner solve (merged clusters, floor, determinism regime) wrapped by ζ's outer enumeration — the exact §3.1 staging M-WHOLE resolved | **this PRD** owns the outer loop; M-WHOLE owns the inner continuous solve | landed; η updates M-WHOLE §8's PRD-2 row |
| `continuous-cost-minimisation.md` (PRD 1) | consumes | Money-objective + robustness floor + `cost_robustness_tradeoff`, composing via the mixed inner solve (§3 decision 8) | PRD 1 (landed; this PRD reuses) | landed |
| `material-waste-cost-minimisation.md` (M-WASTE, 4787) | sibling | both consume F-result; no direct code seam (geometry outer loop ⊥ discrete enumeration) | n/a | independent |
| `solutions()` surface (future, path c) | produces-for | `solve_all` / mixed enumeration engine + purposes-as-solve-scopes; **records of auto assignments** v1 | future PRD via bookmark θ (design-first) | bookmark θ in this batch |
| #5388 (strict-auto uniqueness, continuous) | coordinates | none (disjoint files: `solver.rs`/`registry.rs` vs `cpsat.rs`); shared *concept* of honest uniqueness | each side owns its solver's fix | cross-referenced in both |
| `engine-integration-norm.md` §3.5 | extends impl | populates existing registry slots; overrides the already-catalogued `solve_ranked` | seam catalog unchanged (F-result δ #4805 already noted `solve_ranked`) | no norm edit needed |

No new contested-ownership pair (checked against `phase-3-breadcrumb-map.md` §3).

---

## §8 — Decomposition plan

B+H shape. Greek labels; task IDs at decompose. Spine: α → β → γ, then δ/ζ slices, ε integration gate, η companion docs, θ bookmark. **Test-layout note (drift-guard check):** no new `tests/infra/*.sh`, no wall-clock assertions, and **no new standalone `crates/*/tests/*.rs` binaries** — eval-side e2e tests extend the existing `crates/reify-eval/tests/harness_engine.rs` harness (harness-layout ratchet, Leo ruling 2026-07-22); solver-level tests are `#[cfg(test)]` units in `reify-constraints` src. Drift-guard registrations therefore N/A by construction.

- **α — Let-tracing: transitive constraint↔auto closure + shared per-trial dependent-cell fold.**
  Modules: `crates/reify-eval/src/engine_eval.rs` (transitive `filter_constraints_reading_autos` via `build_dependent_cells`; thread `dependent_cells` into per-template problems), `crates/reify-constraints/src/{decompose.rs, solver.rs, cpsat.rs, registry.rs}` (transitive decompose edges; the ONE fold helper (§3.9) used in DimensionalSolver residual eval + CP-SAT forward-check; registry sub-problems get real `dependent_cells`, replacing `registry.rs:244`'s `Vec::new()`).
  **LEAF signal (user-observable):** `reify eval` on the let-indirected continuous fixture resolves `a=6, b=4` with no `W_UNDERDETERMINED` (B1) — baseline `undef` probe-verified 2026-07-24; direct-formulation fixtures byte-identical (B2/D1). **Also unlocks β/γ/δ/ζ.**
  Prereqs: none. `grammar_confirmed=true`.

- **β — CP-SAT honest enumeration core: `solve_all` + cap-2 `unique` + the `solve_ranked` argmin override.**
  Modules: `crates/reify-constraints/src/cpsat.rs` (+ `lib.rs` export). Adapt spike `75cf3b4d19`: `solve_all(problem, cap)` / `SolveAllResult`; `solve()` honesty via cap-2 (D3); the F-result override — enumerate + fold + `eval_objective_set` + ascending rank + `ProvenOptimal`/`BestFound` (§4.2, D2).
  **Intermediate** (CP-SAT unreachable in production until γ) — consumer: γ wiring, ε gate, ζ leaf callback; behaviour pinned by B4/B5/B6 via γ/ε. Unit tests in-src.
  Prereqs: α (the fold). `grammar_confirmed=true`.

- **γ — Default-ON wiring: `production()` populates `logical` = CpSat + CrossDomain `fallback` = `DiscreteFirstFallback` (all-discrete arm).**
  Modules: `crates/reify-constraints/src/registry.rs` (+ the fallback strategy type). All-discrete components → CP-SAT; mixed → DimensionalSolver fall-through (staged until ζ); continuous → unchanged (§4.1). No env flag (§3.1).
  **LEAF signal:** `reify eval` on the natural let-indirected Bool hexagon fixture (n2 shape) yields exact `Bool`s satisfying the balance constraints, no misleading residual error (B3); strict two-solution fixture reports honestly non-unique (B4); determinism B11. Baselines probe-verified (§2.3).
  Prereqs: α, β. `grammar_confirmed=true`.

- **δ — Discrete domain channel: `Int` bound-mining + `discrete_set` → `AutoParam.domain`.**
  Modules: `crates/reify-constraints/src/cpsat.rs` (bound-mining in `build_variable_domain`, Enum-precedent style; typed no-finite-domain diagnostic), `crates/reify-compiler` (lower `solver_hints` across the IR seam), `crates/reify-ir` (cell-decl hints + `AutoParam.domain`), `crates/reify-eval/src/engine_eval.rs` (`build_auto_param_list` resolves the registered stdlib collection → `domain`; empty/unresolvable validation diagnostics), `crates/reify-core` (diagnostic codes).
  **LEAF signal:** `reify eval` resolves the bounded `Int` fixture to `n=4` uniquely (B7, baseline failure probe-verified); a `discrete_set` stdlib-catalog auto resolves to a catalog member (B8 feasibility half; baseline today: indeterminate/unsolved, probe-verified); unbounded-`Int` and empty/unresolvable-collection fixtures emit the typed diagnostics (D5).
  Prereqs: γ (observability). `grammar_confirmed=true`.

- **ζ — Mixed outer loop: enumerate discrete × continuous residual (M-WHOLE §3.1).**
  Modules: `crates/reify-constraints/src/{registry.rs, cpsat.rs}` (leaf-callback plug-in on the backtracker; residual sub-problem construction; candidate assembly; always-`BestFound`).
  **LEAF signal:** `reify eval` on the mixed fixture resolves `up=true, t≈3.0` with honest `BestFound` + warning (B9); Money-objective mixed fixture shows the inner floor composing (B10). Baseline residual-failure probe-verified.
  Prereqs: β, γ. `grammar_confirmed=true`.

- **ε — Integration gate: committed CI examples + eval tests (the B+H vertical slice).**
  Modules: `examples/` (`discrete_pulley_balance.ri` — natural let-indirected Bool balance; `discrete_stock_cost.ri` — `discrete_set` catalog + `Money` `minimize` picking the cheapest feasible stock), `crates/reify-eval/tests/harness_engine.rs` (e2e asserting B3/B5/B7/B8: values, honest `ProvenOptimal` silence, argmin flip-on-objective-flip).
  **LEAF signal (integration gate):** both examples run under `reify eval` in CI with correct values and honest diagnostics — the §5 sketch rows B1–B8 executed end-to-end.
  Prereqs: γ, δ. `grammar_confirmed=true`.

- **η — Companion docs: spec §"Deferred capabilities" + cross-PRD status rows.**
  Modules: `docs/reify-language-spec.md` (~§2285: replace "queued and not yet authored" with the landed discrete/mixed capability + honest-optimality description), `docs/prds/v0_6/ranked-solve-result.md` (§6 PRD-2 row: future → landed), `docs/prds/v0_6/whole-model-objective-coupling.md` (§8 PRD-2 row).
  **LEAF signal (docs):** committed prose; spec no longer claims PRD 2 unauthored.
  Prereqs: ε, ζ. `grammar_confirmed=true`.

- **θ — `[MILESTONE]` Bookmark: `solutions()` query surface (path c) — design-first on dispatch.**
  PENDING milestone (bookmark-tasks preference: dep-gated follow-on ⇒ PENDING `[MILESTONE]`, not deferred). ON DISPATCH: **escalate for a `/prd` design session — do NOT implement.** Charter: purposes as named solve scopes; `solutions(P)` returning **records of the auto assignments** (Leo: records over materialized instances, v1); count/first/quantifiers compose in-language; capacity semantics (enumerable iff every auto in the post-let-tracing component is finite-domain OR the continuous residual is uniquely determined per assignment; else a typed diagnostic; cap ⇒ `complete:false` warning mirroring `SolveAllResult`). Substrate delivered by this PRD: `solve_all`, the mixed enumeration engine, honest optimality/uniqueness channels.
  Prereqs: ε, ζ. Non-code on dispatch (`execution_class: decision`).

**Dependency view**
```
α → β → γ → δ → ε → η
         γ ──→ ζ ──→ η
             δ,γ → ε        ε,ζ → θ [MILESTONE]
         (ζ also ← β)
```

---

## §9 — Out of scope for this PRD

- The `solutions()` language surface, purpose-as-predicate, and any solution-set value type → bookmark θ.
- MINLP / SCIP / `russcip` (rejected, M-WHOLE §3.1).
- `prefer_stock` / `preferred_strategy` hint consumption; hint surfaces beyond `discrete_set`.
- **User-defined `discrete_set` catalogs** (local `let`-bound collections): compile-rejected today (§2.3); wiring them = `validate_solver_hint_collections` extension + determined-cell resolution — follow-up after the stdlib surface ships end-to-end.
- Continuous-side uniqueness (basin-blind perturbation re-solve) → #5388.
- Functional-enumeration ergonomics (recursion, `filter`/`map` lambdas) → #5393.
- Geometry-dependent cost → M-WASTE (4787). Multi-aspect coherence → M-UNITS (landed guard).
- Any change to `SolveResult`, `ConstraintSolver::solve()` signatures, or the F-result carrier (frozen, I1).

---

## §10 — Open questions (tactical — decide at implementation time)

1. **Cap values.** Enumeration budget (search-space leaves) and the `Ranked` top-N carrier truncation. Existing precedent `MAX_INT_DOMAIN = 1000`; a domain-product guard is also needed. **Suggested:** budget ~1e5 leaves, top-N 16; tune in β. Never affects honesty (D2/D5 make caps loud).
2. **Diagnostic code names** for un-enumerable `Int`, undetermined/empty `discrete_set` collection, domain-product overflow. Follow `reify-core` naming conventions; decide in δ.
3. **Bound-mining expression coverage.** v1 = direct comparisons against compile-time-constant expressions (mirroring the Enum arm's literal scan). Whether to fold constants through simple arithmetic (`n*3 >= 10` ⇒ `n >= 4` vs relying on enumeration-time pruning by the wider mined range) — either is correct (pruning catches what mining misses); decide in δ.
4. **`AutoParam.domain` representation** — `Vec<Value>` vs `Arc<Vec<Value>>` (clone cost on problem construction). Decide in δ.
5. **Strict-`Int` uniqueness UX** — a strict `Int` auto with a multi-member feasible domain correctly reports non-unique; examples should model with `auto(free)` + constraints (or an objective) when exploration is intended. Confirm example authoring in ε (mirrors PRD 1's `auto(free)` precedent).
6. **Spike-code reuse extent** — cherry-pick vs re-derive against moved `main`; the spike is 1 commit, small. Decide in β.
7. **Fixture naming/location** for the committed probe fixtures (`docs/prds/v0_6/fixtures/discrete_*.ri` vs `crates/reify-eval/tests/fixtures/`). Decide in ε; the PRD-level probes stay under `docs/prds/v0_6/fixtures/`.
