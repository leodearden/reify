# Solution-set completeness — multimodality honesty and discrete×continuous composition (P3)

**Milestone:** v0_6 · **Status:** active (authored 2026-08-26 in a `/prd` session under G1–G7+META; design decisions D1–D4 resolved with Leo 2026-08-26) · **Approach:** B + H

**Code anchors** verified against main `2128c3692c` (2026-08-26). Main moves fast — cite-by-symbol; re-locate lines at implementation time.

**Cluster:** `solver-integration` (P3 of the 2026-08-26 solver programme: P1 driver-parity, P2 `geometry-algebra-solver-unification.md`, P3 this, P4 solver legibility). Consumes `ranked-solve-result.md` (F-result, landed) and task **#6655**'s interval propagator. Coordinates with — and does **not** duplicate — `discrete-cost-minimisation.md` (PRD 2).

---

## §0 — Purpose and scope

### §0.1 — The one-sentence problem

**`SolveResult::Solved.unique: bool` is the wrong type**, and every solver-honesty defect in this area is that type error surfacing somewhere.

A boolean cannot distinguish five genuinely different states: *proved there is exactly one*; *proved there are two*; *found one and proved nothing*; *found two*; *proved there are none*. Reify collapses all five onto one bit, so the bit has to be guessed — and the guesses are wrong in four different places at once.

### §0.2 — The four wrongs, as measured

All four verified at the anchor commit by direct probe (`reify eval`; see §2.1 on why not `reify check`). The committed evidence fixtures and their measured behaviour **today**:

| Fixture (`tests/prd-gate/fixtures/`) | Shape | Measured today |
|---|---|---|
| `ssc_two_roots_free.ri` | `auto(free)`, `abs(L − 12mm) == 2mm` → roots 10mm, 14mm | exit 0 · `length = 0.01 m` · `` warning: Parameter `length` resolved via auto(free) -- result is not uniquely determined. `` |
| `ssc_single_root_free.ri` | `auto(free)`, `L == 10mm` → **one** root | exit 0 · `length = 0.01 m` · **the identical warning** |
| `ssc_two_roots_strict.ri` | strict `auto`, same two roots | exit 1 · `length = undef` · `error: strict auto parameter resolution is not uniquely determined — consider using auto(free) for exploration` · `note: … is undef (because: solve failed: infeasible)` |
| `ssc_refuted_pair.ri` | `L == 10mm` **and** `L == 20mm` | exit 1 · `error: constraints could not be satisfied (max absolute residual: 5.00e-3)` |
| `ssc_ineq_bracketed_strict.ri` | strict `auto`, `8mm ≤ L ≤ 40mm` | exit 0 · `length = 0.024 m` · **no diagnostic at all** |

**The first two rows are the whole argument in two lines.** One model has two answers, the other has one, and reify emits **byte-identical output** for both. A signal that cannot tell those two cases apart is not a weak signal; it is not a signal.

1. **`auto(free)` with two genuine roots commits to one, silently.** `finalise_uniqueness` skips verification entirely for all-free problems and reports `unique: false` unconditionally, which the engine renders as `` "Parameter `x` resolved via auto(free) -- result is not uniquely determined." `` — a message emitted from **five byte-identical sites** (`push_merged_cluster_nonunique_warnings`, the cold `Engine::eval` arm, the `eval_cached` arm, and two arms in `engine_edit.rs`), carrying **no `DiagnosticCode`** (a live INV-SF-6 violation), firing whether or not multiplicity exists — and therefore carrying no information — and, for a merged cluster, fanned out from one cluster-wide flag to *every* free auto in the cluster, so it can name a parameter that is not the culprit. No root is ever named.

2. **Strict `auto` with two genuine roots is reported as infeasibility.** `finalise_uniqueness` demotes the whole result to `SolveResult::Infeasible` with `DiagnosticCode::ConstraintNonUnique` and the text *"strict auto parameter resolution is not uniquely determined — consider using auto(free) for exploration"*; the cell goes `undef` with `UndefCause::SolveFailed { detail: "infeasible" }`. A model with two answers is told it has none. Measured on a two-root fixture (`(L − 130mm)² == (30mm)²`, roots 100mm and 160mm): both roots exist, neither is named, `eval` exits 1.

3. **When the uniqueness probe fails, the code assumes uniqueness.** `verify_uniqueness`'s non-convergence arm returns `true` — *"cannot prove non-uniqueness — conservatively assume unique"* — which is conservative in the wrong direction: it converts *no evidence* into *a claim*. And it is not a rare path: `build_auto_param_list` sets `bounds: None` **unconditionally**, so `effective_bounds` always degrades to `default_bounds_for`, the reflected anchor always lands outside any real bracket, and the re-solve fails. This is task **#5711** (in-progress), whose own measurement records that wiring the derived box in flips six previously-`Solved` fixtures to `ConstraintNonUnique` — at least one of them falsely, because it is really evidence of suboptimality, not multiplicity.

4. **Two production drivers assert completeness they never computed.** `SolveSpaceSolver::solve` hardcodes `unique: true` on every libslvs `Ok` arm; `relate_solve::solve_frame` sets `unique = fully_pinned && !unknown.free`, where `fully_pinned` is **local Jacobian rank** — local isolation reported as global uniqueness. Neither ever reaches `verify_uniqueness`, which is private to `solver.rs`.

And, underneath all four: **the alternatives are already computed and then thrown on the floor.** `DimensionalSolver::solve_ranked_impl` runs `K = 2·(dim+1)` starts and returns them ranked; both engine consumption seams do `candidates.swap_remove(0)` and discard `candidates[1..]`. `RankedSolveResult` / `RankedCandidate` appear in no crate outside `reify-ir` / `reify-constraints` / `reify-eval` — no CLI, GUI, or MCP surface has ever seen an alternative optimum. The multimodal information is paid for at 2(dim+1)× the cost and deleted. The candidates are also **not deduplicated**, so "K candidates" is not "K solutions" — for a single-basin objective most of them are near-identical convergences of one optimum, and nothing in the carrier says which.

### §0.3 — The unifying frame

**A basin is a discrete variable the author didn't declare.**

That is not a slogan; it is the reason one carrier serves both halves of this PRD. Multiplicity arising from a *declared* `Int`/`Enum` domain and multiplicity arising from an *undeclared* nonlinearity differ only in which engine enumerates them. What the user needs to be told — *how many are there, and did you prove it* — is identical, and so is the arithmetic for composing the answer across a model that has both.

Reify already has one honest instance of this, on the discrete side: `SolveAllResult { solutions, complete }` (`cpsat.rs`, landed by PRD 2 β **#5468**), whose doc states the doctrine exactly right —

> `{ solutions: [], complete: true }` is a PROOF of unsatisfiability, while `{ solutions: [], complete: false }` proves nothing at all — and every honesty claim built on this carrier (`unique`, `ProvenOptimal`) is conjoined with this flag rather than asserted.

**This PRD promotes that discipline out of `cpsat.rs`, generalises it to the continuous side, and makes the conjunction the law rather than one solver's local good manners.**

### §0.4 — The three rungs

| Rung | Multiplicity source | Engine | Verdict reachable | Owner |
|---|---|---|---|---|
| 1 | **Declared** discrete (`Int` / `Enum` / `discrete_set` domains) | CP-SAT backtracker | `Exhaustive` when enumeration completes | **PRD 2** (`discrete-cost-minimisation.md`) — β #5468 landed, γ #5469, δ #5470, ζ #5472 pending. **Adopted, not duplicated.** |
| 2 | **Undeclared**, interval-representable, bounded, small | box branch-and-bound over #6655's HC4 propagator | `Exhaustive` / `Refuted` | **this PRD** |
| 3 | **Undeclared**, beyond rung 2's envelope | the same subdivision, truncated: surviving boxes seed the existing Nelder-Mead multistart | `Partial{reason}` only | **this PRD** |

Rungs 2 and 3 are **one mechanism, not two.** The subdivision either terminates with every box resolved (→ `Exhaustive`) or hits its budget and hands the surviving boxes to the solver as a deterministic start set (→ `Partial`). There is no separate heuristic engine to build, tune, or keep in sync — and the start set is derived, not guessed, which is what lets rung 3 stay inside the house determinism constraint (§3, D5).

### §0.5 — What this is NOT

- **NOT the `solutions()` language surface.** That is PRD 2 bookmark **θ #5474** (`[MILESTONE]`, design-first on dispatch), and it stays there per Leo's 2026-08-26 ruling. This PRD ships the carrier and the verdict that surface will render; it does not charter the surface. §7 binds the seam.
- **NOT the mixed discrete×continuous outer loop.** That is PRD 2 **ζ #5472** — CP-SAT outer enumeration wrapping the continuous inner solve, MINLP rejected, *binding, do not re-litigate*. This PRD supplies ζ the composition law (§3.5) that lets it report its verdict honestly; it does not build ζ's loop.
- **NOT the interval propagator.** That is **#6655** (pending, no deps), which builds HC4 box-consistency over `CompiledExpr` and the whole-box empty ⇒ proven-infeasible arm, and whose text explicitly says *"the box is the substrate for later branch-and-bound basin enumeration (multimodality PRD) — design the propagator so it can run on a sub-box, but build no enumeration here."* This PRD is that enumeration. **#6655 is a hard prerequisite.**
- **NOT a stochastic global optimiser.** `whole-model-objective-coupling.md` §3 decision 2 defers CMA-ES / PSO / SA to a future PRD and declines to take on the seed + RNG-pin + single-thread-reduction determinism invariant. This PRD does not take it on either — see D5.
- **NOT new selection syntax.** Resolved with Leo (D2): the idiom for pinning one of N solutions stays "add a discriminating constraint". No grammar change.
- **NOT a change to `SolveResult` or `ConstraintSolver::solve()`.** Both stay frozen (F-result invariant I1). Everything additive.
- **NOT Pareto-front exploration** (spec §10.4 calls it a tooling concern; out of scope in both `constraint-solver-completion.md` and PRD 2, and it stays out here).
- **NOT integrality for `Int` autos.** A probe found `param n : Int = auto(free)` resolves to `3.3` with the `Int` annotation erased. Real, adjacent, and **not this PRD** — see §10 for the follow-up.

---

## §1 — Consumer (G1)

| Mechanism | Consumer |
|---|---|
| `Completeness` / `SolutionSet` carrier (`reify-ir`) | **On-main user surface:** `reify eval` / `reify run` diagnostics (§3.4 verdict policy) and `reify explain`'s provenance table (ζ). **Forward consumer, by name:** PRD 2 bookmark **θ #5474** `solutions(P)` renders this carrier; PRD 2 **ζ #5472** reports through the §3.5 composition law. |
| Box branch-and-bound enumerator (`reify-constraints`) | The verdict policy (δ) and the refutation diagnostic (ε), both of which are `reify eval` output differences. Also the deterministic start-set producer for the existing `multistart_points` path. |
| Three-way verdict policy + diagnostic codes | **End user** of `reify eval`: an author whose model has two answers is told *both*, instead of being told it has none (strict) or one (free). |
| Refutation diagnostic (`Refuted{narrowing}`) | **End user**: an infeasible topology is refuted by naming the narrowing constraint at zero solver iterations, instead of `"constraints could not be satisfied (max absolute residual: …)"` after a 5000-iteration burn. |
| Basin identity (the containing verified box) | The candidate dedup key in `solve_ranked_impl` (which has none today), the `reify explain` solution-set rendering, and the warm-re-solve stability check (η). |
| Composition law at the registry | `SolverRegistry::solve_inner`'s existing per-component `unique` conjunction; the named forward consumer is PRD 2 ζ. |

**Engine-integration sub-check (G1).** Every solver-side mechanism plugs into the catalogued **§3.5 ConstraintSolver** seam (`docs/prds/v0_3/engine-integration-norm.md`), whose extension note already covers the `solve_ranked` sibling-carrier pattern this PRD follows. **No new seam.** No orphan-producible `pub fn` in a `kernel-*` crate.

**The grounding design consumer.** `prj/printer_v01/printer.ri` (branch `dogfood/printer-20260823`), the rear tendon-routing web. Its evidence is *absence*, and it is worth stating precisely, because the earlier framing of this programme overstated it:

- The file has **zero `auto` params, no objective, and no discrete scheme variable.** `DriveTendons` is 148 `let` cells of hand-derived closed-form tangency algebra with 73 constraints used as **post-hoc checkers**. The author is the solver.
- The one discrete-choice-shaped construct is `param side : Real = 1.0` with `constraint side * side > 0.999` / `< 1.001` — a `Real` abused as a ±1 sign flip, i.e. exactly the mirror-pair shape of **#5388**, in a live design. It is accompanied by `FairleadPairMirrored` and `CapstanUnitMirrored`, whole structures duplicated because a constructor argument never reaches a child's sub placements (**#6592**).
- The 64-configuration static-balance enumeration was **run by hand, offline, in Python** and one winner baked into a param default.
- The two free design variables of the v3.2 fused feed (`fuse_a1_z2`, `fa1_v1y`) are **hand-bisected literals**, annotated in-file as "DESIGN choices bounded by constraints" with the bisection recorded in prose.
- The infeasibility theorem (all-four-down-turns infeasible under A1+A2+A3) was **proved by hand** and re-checked by an out-of-band Python sweep (`prj/printer_v01/tools/v2_check.py`).
- Four open questions are parked in the header, unexplored because trying them means another manual re-derivation: **Q1** is a discrete topology flip ("does unflipping fairlead B open a fused diagonal for b2?"), **Q2/Q3** are continuous relaxations bounded by already-written constraints.

So the consumer does not need *better* multimodal solving; it demonstrates that a competent author, given no way to say "there are two of these" or "find the one that minimises rope", will do the search by hand across five destructive rewrites. That is the demand signal.

**The in-corpus consumer.** `examples/best_practices/discrete_choice.ri` — committed, compile-gated, indexed — already documents this gap in its own prose, and gets the diagnosis wrong in a way this PRD must fix:

> BE HONEST ABOUT WHAT THIS DOES: the idiom ENCODES the discrete choice, it does not ENUMERATE it. […] Which root you get is not something to rely on. […] to explore both, you currently need two runs. Real branch-and-bound over discrete variables is what CP-SAT will bring.

The last sentence is **false for that file**: `side` is a continuous `Real` auto with no finite domain, so PRD 2's CP-SAT path can never reach it. Rung 2, not rung 1, is what serves this exemplar. Rewriting that caveat is a deliverable (κ).

---

## §2 — Background & substrate (verified in-tree at the anchor commit)

### §2.1 — `reify check` does not solve, and that is load-bearing here

`reify check`, `reify build`, and `reify test` wire **no constraint solver at all** (`Engine::new(None)`; `build_test_engine` cannot even name `reify-constraints`, a dev-only dep). Every auto-driven constraint reports `INDETERMINATE … undefined inputs`, `check` prints `No constraints violated (N indeterminate).` and **exits 0** — including on a provably infeasible model. `--strict` promotes indeterminate to failure, but its output is byte-identical for a uniquely-solvable model, a genuinely bimodal one, an infeasible one and a fully determined one, because it never solves: the "indeterminate" it flags is `check`'s own refusal to run, not a property of the design.

Consequences for this PRD, both deliberate:

- **Every leaf signal in §8 is stated against `reify eval` / `reify run`**, which is where the solver actually runs (plus `report` and `explain`, which share `configured_eval_engine`, and the GUI).
- The solver-bearing posture of `check`/`build` is **P1 driver-parity's** deliverable (it executes Leo's `esc-4458-87` ruling), tracked separately as **#6631**. When it lands, this PRD's diagnostics reach `check` with no further work, because they ride the existing eval diagnostic channel. §7 binds that seam. This PRD does **not** flip the posture itself.

### §2.2 — What exists to build on

- **`OptimalityStatus`** (`reify-ir::ranked`) = `ProvenOptimal | BestFound { reason: BestFoundReason } | FeasibilityOnly`; `BestFoundReason` = `IterationLimit | ConvergedWithinBudget | Unreported` — **`Unreported` is removed by P2 μ #6680**, which replaces it with `FirstOrderStationary`; treat the variant list here as as-of the anchor commit and see §7.1. Invariant I3 already reserves `ProvenOptimal` for a real proof. **`FeasibilityOnly` says nothing about how many feasible points exist** — that is the hole this PRD fills, and it is orthogonal to optimality, which is why it needs its own field rather than new `OptimalityStatus` variants.
- **`RankedSolveResult`** = `Ranked { candidates, optimality } | Infeasible { diagnostics } | NoProgress { reason }`; `RankedCandidate { values, objective_score, unique }`. Frozen shape extended additively (§3.1).
- **`SolveAllResult { solutions, complete } | NotEnumerable { reason }`** (`cpsat.rs`) — the discrete-side precedent, and the thing being generalised. CP-SAT is **not registered in `SolverRegistry::production()`** (`logical: None`, `fallback: None`), so nothing it computes reaches a user today; PRD 2 γ **#5469** fixes that and is dispatchable now.
- **`DerivedInterval` / `derive_param_intervals` / `compose_interval` / `resolve_bounds`** (`solver.rs`) — a **one-pass syntactic bound miner, not interval arithmetic**. It recognises exactly four linear-in-one-auto shapes with a constant far side; `Eq`, `Ne`, `Or`, and every nonlinear or multi-auto shape are silently skipped; no propagation, no fixpoint, no soundness claim. **#6655** replaces it with real HC4 box consistency. **There is no interval arithmetic, contractor, box-consistency or branch-and-bound substrate in the workspace today.**
- **`multistart_points`** (`solver.rs`) — `K = 2·(dim+1)` starts, gated on `objective.is_some() && auto_params.len() >= 2 && cost_robustness_lambda.is_none()`. Since **#5618** the box is the *constraint-derived* one, not the default box (the default-box version made best-of-K degenerate to best-of-one). Start #0 is the historical seed; #1 the all-midpoint; #2..K−1 per-axis low/high corners. **Deterministic; no RNG, no clock.**
- **`eval_objective_set`** (`solver.rs`, `pub(crate)`) — the single objective fold, shared by the Nelder-Mead cost surface, both `solve_ranked` scoring paths, and `CpSatSolver::solve_ranked_with_budget`. **This PRD preserves the single-fold property**; it adds no second scoring path. (The one existing non-shared twin, `registry.rs`'s `eval_rank_cost` for lexicographic ε-bands, is out of scope.)
- **`ObjectiveProvenance` + `reify explain`** (`constraint-solver-completion.md` θ #4015 / ι #4017, landed) — the per-cell legibility surface. It carries no optimality, uniqueness, solver identity or completeness token whatsoever, and is the natural, already-shipped home for the solution set. **This PRD extends it rather than inventing a surface.**
- **`UndefCause`** = `Unbound | AwaitingSolve | SolveFailed { detail } | OpContractFailed | UserUndef` — the existing typed-cause precedent (INV-SF-1) that the non-uniqueness cause must join.
- **`auto_type_param.rs`**'s `"auto(free) type parameter has multiple feasible candidates for bound '…': …; selected lexicographically-first '…'"` — the repo's **only** existing "here are the alternatives and here is why I picked one" user string, and the structural precedent for §3.4's message shape.

### §2.3 — Grammar (G3)

**No novel `.ri` syntax. G3 grammar gate N/A.** Every fixture shape in §8 parses today and was probe-confirmed at the anchor commit: `param x : T = auto`, `param x : T = auto(free)`, `sub s = S(arg: auto)`, `constraint <expr> == <expr>`, `constraint <expr> >= <expr>`, `minimize <expr>`, `abs`/`sqrt`/`min`/`max`/`if-then-else` inside comparison operands. D2 (no new selection syntax) keeps it that way.

Three substrate facts the design deliberately does **not** rely on, each probe-confirmed absent:

- `@solver_hint("discrete_set", …)` parses and kind-validates but has **zero solve-time readers** (audit finding M-008; probe: a hint-annotated auto minimised against a stock set resolves to `13.2mm`, not a catalogue member). Wiring it is PRD 2 δ **#5470**.
- `param s : SomeEnum = auto` parses and compiles, then reaches the continuous solver and fails with `solve failed: infeasible` at residual `1.00e0`. Enum domains are PRD 2's.
- `minimize … where …` parses; the guard is **silently discarded** — a constant-false guard changes nothing, with no error and no warning. That is **#6647**, already filed; this PRD does not touch objectives' guard semantics.

---

## §3 — Contract (B + H)

The seam is a Rust carrier plus a verdict policy. An architect implementing the producer side should need no further discussion.

### §3.1 — Carrier (new, in `reify-ir`; additive only)

A sibling module to `ranked.rs`, following the same pattern F-result established for exactly this reason (`SolveResult` and `ConstraintSolver::solve()` stay frozen — invariant I1; the production `Solved { values, unique }` matches destructure **without** `..`, so adding a field would not compile).

```rust
/// How much of the solution set the producer actually established.
/// Orthogonal to `OptimalityStatus`: that says how good the best one is,
/// this says how many there are and whether that count was proven.
pub enum Completeness {
    /// Every solution inside the searched domain is present in the set.
    /// The ONLY verdict from which `unique` or `ProvenOptimal` may be derived.
    Exhaustive,
    /// More solutions may exist; how many is unknown. Never a count claim.
    Partial { reason: PartialReason },
    /// Proven that NO solution exists in the searched domain, and which
    /// constraint drove the domain empty.
    Refuted { narrowing: ConstraintNodeId },
}

pub enum PartialReason {
    /// The subdivision hit its node budget with boxes still unresolved.
    BoxBudgetExhausted,
    /// The component's dimension exceeds the enumeration envelope (λ).
    DimensionAboveEnvelope { dims: usize },
    /// An auto's domain is unbounded, so no finite box could be searched.
    DomainUnbounded { param: ValueCellId },
    /// A constraint has no sound interval form; the box cannot be trusted.
    NotIntervalRepresentable { constraint: ConstraintNodeId },
    /// A discrete leaf's continuous sub-problem was not enumerated
    /// (the PRD 2 zeta case; see §3.5).
    InnerSolveUnproven,
    /// Only the legacy perturbation probe ran — one re-solve from one
    /// reflected anchor. Establishes nothing about the set.
    ProbeOnly,
    /// No completeness reasoning was attempted by this driver.
    NotAttempted,
}
```

`SolutionSet { solutions: Vec<RankedCandidate>, completeness: Completeness }`, and `RankedSolveResult::Ranked` gains a `completeness: Completeness` field. Every existing producer that does not reason about the set sets `Partial { NotAttempted }`, so the change is behaviour-preserving until a producer opts in.

### §3.2 — Invariants

Numbered so leaves and boundary tests can cite them.

- **C1 — `unique` is derived, never asserted.** `unique == (completeness == Exhaustive && solutions.len() == 1)`. No producer may write `unique: true` from any other reasoning. This is what retires `SolveSpaceSolver`'s hardcoded `true` and `solve_frame`'s Jacobian-rank claim (θ).
- **C2 — `ProvenOptimal` requires `Exhaustive`.** Conjunction, not assertion — the discipline `SolveAllResult.complete`'s doc already states, now enforced at the carrier. (A driver holding an independent optimality certificate — an exact MILP duality gap — may still claim it; it must then also justify `Exhaustive` or carry the certificate explicitly.)
- **C3 — `Refuted` is a proof, `Partial` with an empty set is not.** `{ solutions: [], Refuted }` means no solution exists. `{ solutions: [], Partial{..} }` means the search found none and proved nothing. These must never render as the same diagnostic — that collapse is precisely what `SolveAllResult`'s `NotEnumerable` variant exists to prevent on the discrete side, and it is the distinction §0.2 wrong #2 currently gets backwards.
- **C4 — no unproven count.** A `Partial` verdict may report the solutions it found; it may never report *how many exist*, and no user-facing string derived from it may imply a total.
- **C5 — basin identity is the containing verified box.** Two converged points are the same solution iff they lie in the same sub-box that passed the existence-and-uniqueness test (D3). Outside a box search there is no identity relation and candidates are not deduplicated — which must be stated in the verdict, not papered over with a distance tolerance.
- **C6 — determinism.** The subdivision order, the start set derived from it, and the resulting solution ordering are pure functions of the problem. No RNG, no clock, no seed to maintain (D5). Two runs produce identical sets in identical order.
  **C6 is conditional and the condition is not met today.** `SubProblem.auto_params` is a `HashSet<ValueCellId>` collected into the `Vec` that becomes the solver's **axis order**, and `im::HashMap`/`HashSet` over `RandomState` iterate differently **per instance**, not merely per process — so any subdivision keyed on axis order is currently irreproducible. P1's leaf γ (invariant I4, determinism tier T1) closes it, and is a **hard prerequisite** of this PRD's **ζ** — the first leaf whose fixture carries more than one `auto` (§7.1). The single-`auto` leaves γ, δ and ε are unaffected: a one-auto problem has no axis order to permute, which is why their fixtures are single-`auto` by design rather than by accident. Stating C6 without that edge would make it a false premise; §3.3's "ties by `auto_params` declaration order" is only well-defined once P1 γ lands. Note also that this PRD *reduces* the exposure independently: today the multistart tie-break is start index, which is the axis-order map, so two equal-scoring starts in different basins hand the win to whichever got the lower index (P1 §2.4); a box-derived start set makes the tie-break positional instead.
- **C7 — budgets are counted in nodes, not seconds.** The enumeration budget is a box/node count, never a wall-clock bound. This keeps C6 true on a loaded machine, and it deliberately avoids adding a wall-clock assertion that would need registering in `tests/infra/test_no_new_wallclock_upper_bounds.sh`.
- **C8 — soundness is one-directional.** Interval arithmetic over-approximates. A box that contracts to empty is *proof* of local infeasibility; a non-empty box is *not* evidence of satisfiability. Every verdict must be derivable in the sound direction only — the same caution `solve_core_with_sd_tolerance` already documents for the existing bound miner.

### §3.3 — The enumerator

One engine, three verdicts. Operating on the component `ResolutionProblem` after `decompose_into_components_with_reads`, so dimension is per-component, not whole-model.

1. Seed with #6655's derived box for the component. If any auto's box is unbounded → `Partial { DomainUnbounded }`, stop. If any constraint has no sound interval form → `Partial { NotIntervalRepresentable }`, stop (C8).
2. If component dimension exceeds the envelope cap → `Partial { DimensionAboveEnvelope }`, stop; the undivided box still seeds the existing solve.
3. Otherwise subdivide: pop a box, HC4-contract it (#6655's propagator, run on the sub-box — the capability that task was asked to preserve).
   - contracted to empty → discard, recording the narrowing constraint;
   - passed the existence-and-uniqueness test → record exactly one solution, do not split further;
   - otherwise → bisect on the widest axis (ties by `auto_params` declaration order) and push both halves.
4. Every box discarded and none recorded → `Refuted { narrowing }`, where `narrowing` is the constraint that emptied the *root* box if #6655's whole-box arm fired, else the constraint most frequently responsible across discarded boxes (deterministic tie-break by `ConstraintNodeId` order).
5. All boxes resolved within budget → `Exhaustive`, solutions ordered by box position (C6).
6. Budget exhausted → `Partial { BoxBudgetExhausted }`, and the surviving boxes are exported as the deterministic multistart start set. This is rung 3: the existing `multistart_points` corner/midpoint construction is replaced *within a surviving box* rather than over the whole derived box, which is a strict improvement on the same determinism footing.

**Angle-typed autos are subdivided over one canonical period, not over their default box.** `default_bounds_for` gives `ANGLE → (−τ, τ)`, which **double-covers the circle**: a naive bisection of that box would find every rotational solution twice and report `Exhaustive` with a doubled count — a false completeness claim produced by the completeness machinery itself. Angle boxes are therefore canonicalised to a single period before subdivision, and two roots separated by exactly one period are the same solution. This is INV-AD-2 `quotient-pure-derivative-algebra` applied to the domain rather than the derivative, and it is a **correctness precondition of `Exhaustive`**, not an optimisation. (Surfaced by the G7 walk, §8.1.)

The existence-and-uniqueness test is an interval Newton / Krawczyk step. It needs a Jacobian; whether that comes from P2's autodiff direction or from a local finite-difference fallback is a **tactical** choice recorded in §11 Q1, not a design fork — the test is sound either way, because a failed test simply keeps splitting.

### §3.4 — Verdict policy (D1: the existing `auto` / `auto(free)` declaration carries it)

Resolved with Leo 2026-08-26. Reify already has an author-side declaration of intent: strict `auto` means *"this must be uniquely determined"*, `auto(free)` means *"any feasible point will do"*. No new expectation syntax is needed; the fix is to make the verdicts honest **under the semantics that declaration already carries**.

| Declaration | Verdict | Behaviour |
|---|---|---|
| `auto(free)` | `Exhaustive`, n > 1 | **Warning**, coded, naming n and each solution's discriminating values; pick index 0 in canonical box order. |
| `auto(free)` | `Exhaustive`, n = 1 | **Silent.** Today's unconditional warning goes away — the first time that message becomes informative is the first time it stops firing when it shouldn't. |
| `auto(free)` | `Partial{reason}` | Warning, coded, carrying the reason. Says the set was not established; does **not** claim non-uniqueness (C4). *(Delta, ruled 2026-08-27, F2 = a2: for `Partial{DomainUnbounded}` — the permanent one-sided case — the RETURNED VALUE is anchored to the user's stated bound plus a margin (the existing seed-nudge/robustness-floor shape, deterministic, within the feasible region), NEVER the dimension-default box edge; the 10 m artifact must not survive into realized geometry behind a mere warning. δ owns this value rule; P1-ε #6692's refusal covers strict `auto` only and must not fire here.)* |
| strict `auto` | `Exhaustive`, n > 1 | **Error**, naming the solutions. Replaces *"not uniquely determined — consider using auto(free)"*. The undef cause becomes a typed non-uniqueness cause, **not** `SolveFailed { detail: "infeasible" }` (INV-SF-1, INV-SF-4). |
| strict `auto` | `Exhaustive`, n = 1 | `Determined`. Honest for the first time. |
| strict `auto` | `Partial{reason}` | **Warning**, coded: *locally unique; global uniqueness unverified (\<reason\>)*, and the determinacy surface reflects it. Never a silent `unique: true`. This is **#5388** acceptance option (b), verbatim, and it removes wrong #3's false-negative. |
| any | `Refuted{narrowing}` | **Error**: proven infeasible, naming the narrowing constraint, at zero solver iterations. Distinct from `"constraints could not be satisfied (max absolute residual: …)"`, which is a search failure, not a proof (C3). |

Severity rationale (INV-SF-2 corollary — *a diagnostic expected on a healthy path is by definition not Error-severity*): a `Partial` verdict is expected on a healthy model whose component is simply above the envelope, so it is a Warning. A strict `auto` with two proven answers is a declaration the model contradicts, so it is an Error — the same severity the code assigns today, now for the true reason and with the roots named.

Every new diagnostic carries a `DiagnosticCode` (INV-SF-6). The five existing code-less free-auto sites are given one in the same work (β).

**Emission is per component, not per parameter.** A `Partial` verdict is a property of the component's solution set, so it is emitted once per component naming the autos it covers. This bounds the noise from above-envelope components — the honest-downgrade rows would otherwise fire on every strict auto in a large model — and it incidentally fixes the existing defect where one cluster-wide `unique` flag is fanned out to every free auto in a merged cluster and can name a parameter that is not the culprit (§10 item 5).

### §3.5 — The composition law (the cross-domain contract)

For a model with both declared discrete choices and continuous autos:

> `Completeness(whole) = Exhaustive` **iff** the discrete enumeration is complete **and** every discrete leaf's continuous set is `Exhaustive`. Otherwise the meet: any `Partial` dominates; `Refuted` only if every branch is `Refuted`.

`SolverRegistry::solve_inner` already conjoins per-component `unique` (`all_unique` / `other_unique`); that conjunction becomes this meet. Two things follow, and both are honest consequences worth stating rather than hiding:

- **This PRD does not by itself flip PRD 2 ζ's verdict.** ζ #5472 is chartered *always* `BestFound`, correctly, because its inner continuous argmin is budget-bounded Nelder-Mead — i.e. `Partial { InnerSolveUnproven }`. The law says exactly that. What the law *adds* is that when a mixed model's continuous sub-problems fall inside rung 2's envelope, the inner set becomes `Exhaustive` and the composed verdict can be too. That is the cross-domain payoff, and it is conditional; the PRD claims it as a reachable state, not a delivered one.
- **The law is what makes rung 1 and rungs 2/3 one system** rather than two solvers that happen to be in the same binary.

### §3.6 — What each existing producer must declare

| Producer | Today | After |
|---|---|---|
| `DimensionalSolver` (enumerated component) | `unique` from a one-shot perturbation probe | the enumerator's verdict |
| `DimensionalSolver` (component above envelope / not representable) | same probe, `true` on non-convergence | `Partial { DimensionAboveEnvelope \| NotIntervalRepresentable \| ProbeOnly }` |
| `CpSatSolver` | `unique = complete && len == 1` (already honest) | unchanged semantics, expressed as `Exhaustive` / `Partial { BoxBudgetExhausted }` |
| `SolveSpaceSolver` | hardcoded `unique: true` | `Partial { ProbeOnly }` |
| `relate_solve::solve_frame` | local Jacobian rank as global uniqueness | `Partial { ProbeOnly }` |
| default trait lift | `Unreported` | `Partial { NotAttempted }` |

---

## §4 — Boundary-test sketch (B + H)

Facing both the producer side (does the enumerator establish the verdict?) and the consumer side (does a user see the right thing?).

| # | Faces | Scenario | Preconditions | Postconditions |
|---|---|---|---|---|
| BT1 | producer | Two-root system is enumerated exhaustively | 1 auto, `abs(L − 12mm) == 2mm`, bounded `Length` domain | `Exhaustive`, exactly 2 solutions (10mm, 14mm), in canonical box order; identical across two runs (C6) |
| BT2 | consumer (`auto(free)`) | Both roots are named to the user | BT1's fixture with `auto(free)` | `reify eval` emits a coded warning naming **both** values and which was selected; exit 0 |
| BT3 | consumer (strict `auto`) | Multiplicity is not reported as infeasibility | BT1's fixture with strict `auto` | Error names both roots; undef cause is the typed non-uniqueness cause, **not** `SolveFailed{"infeasible"}`; the string "consider using auto(free)" is gone |
| BT4 | consumer (silence) | The informative message stops crying wolf | `auto(free)`, `L == 10mm` (one root) | **No** non-uniqueness warning at all (today: fires unconditionally) |
| BT5 | producer + consumer | Refutation is a proof, and costs nothing | `L == 10mm` **and** `L == 20mm` | `Refuted`, naming the narrowing constraint, with **zero** Nelder-Mead iterations; message distinct from the residual-based "could not be satisfied" |
| BT6 | producer (soundness, C8) | A non-empty box is not evidence | a satisfiable-looking system whose interval hull is non-empty but which has no solution in it | verdict is `Partial`, never `Exhaustive` with a fabricated solution |
| BT7 | producer (envelope) | Above-envelope degrades, never hangs | a component with dimension above the cap | `Partial { DimensionAboveEnvelope }` within the node budget; the solve still proceeds from the underived box; no wall-clock dependence (C7) |
| BT8 | consumer (honesty floor) | An unproven set never claims uniqueness | strict `auto`, inequality-bracketed, the #5711 shape | `Partial { ProbeOnly }` + "locally unique; global uniqueness unverified"; **not** `unique: true` |
| BT9 | consumer (drivers) | Geometric drivers stop over-claiming | a `relate`/SolveSpace fixture that lands in one of several configurations | `Partial { ProbeOnly }`; no `unique: true` anywhere in the result |
| BT10 | consumer (set exposure) | The alternatives stop being discarded | dim ≥ 2 with an objective, multi-basin | `reify explain` prints the deduplicated solution set with its completeness; the set is **not** K near-identical convergences (C5 dedup) |
| BT11 | consumer (stability) | A warm re-solve does not silently basin-hop | edit a param on a two-basin model via `edit_param` | either the incumbent basin is retained, or a coded `W_BASIN_CHANGED` names the move; cold and warm agree on the *set* |
| BT12 | producer (composition) | The meet is honest across components | a model with one `Exhaustive` component and one `Partial` | composed verdict is `Partial`, and the reason names the component responsible (§3.5) |
| BT13 | back-compat | Nothing existing changes shape | any single-solution model | `SolveResult` / `ConstraintSolver::solve()` byte-identical; producers that do not opt in report `Partial { NotAttempted }` and behave exactly as today |

---

## §5 — Resolved design decisions

**D1 — the existing `auto` / `auto(free)` declaration carries the severity policy.** (Leo, 2026-08-26.) No new expectation syntax. Strict `auto` already means "must be unique"; `auto(free)` already means "any feasible point". §3.4 is the mapping. Rejected: an `expect unique` / `expect solutions == N` declaration (new grammar, and it duplicates intent the existing keyword already carries); making plain `check` fail on any unproven multiplicity (INV-SF-4's doctrine would support it, but it reds every existing `auto(free)` model with an unbounded box until touched, and `check` does not solve today anyway — §2.1).

**D2 — no new selection surface; a discriminating constraint is the idiom.** (Leo, 2026-08-26.) When N > 1 and the author wants one, they add a constraint that cuts the others; the new verdict tells them a cut is needed and names what to cut between. Grammar stays frozen. Rejected for now: a `select` / `prefer` clause, and a `@solver_hint("near", …)` basin hint. Revisit only if fixtures show the constraint idiom is insufficient — recorded in §11 Q3, not designed here.

**D3 — basin identity is the containing verified box.** (Leo, 2026-08-26.) Two converged points are the same solution iff they lie in the same box that passed the existence-and-uniqueness test. This falls out of the enumeration, is deterministic, and has **no tolerance constant** — which matters, because a distance tolerance would be a knob whose value silently changes a reported solution *count*. Rejected: parameter-space distance clustering (introduces that knob); objective-value equality (collapses the ±1 mirror pair, which has identical objective value and is exactly two distinct solutions — it would erase the motivating case). Outside a box search there is no identity relation, and C5 requires saying so rather than substituting one.

**D4 — basin stability across warm re-solves lives here; the parity half stays P1's.** (Leo, 2026-08-26.) This PRD owns basin *identity* and the continuation-plus-`W_BASIN_CHANGED` behaviour (η). P1 owns the cold-vs-warm determinism invariant; it gains from this PRD the vocabulary to express it. §7 binds the seam.

**D5 — deterministic subdivision, no stochastic global optimiser.** The house constraint is explicit (`whole-model-objective-coupling.md` §3 decision 2: fixed start set, no RNG, no clock, no seed) and this PRD keeps it. Rung 3's start set is *derived from the subdivision*, so it is both better-targeted than today's whole-box corners and free of any new determinism obligation (C6).

**D6 — the completeness axis is a new field, not new `OptimalityStatus` variants.** *How good is the best* and *how many are there* are independent: a `Partial` set can contain a `ProvenOptimal` member (an exact certificate on the branch that was searched), and an `Exhaustive` set can be `FeasibilityOnly` (no objective at all). Folding them into one enum would make illegal states representable and legal ones inexpressible.

**D7 — budgets in nodes, never wall-clock.** (C7.) Preserves determinism under load and avoids introducing a wall-clock upper bound that would need drift-guard registration.

**D8 — `#5388` is adopted as this PRD's honesty-floor leaf, not duplicated.** It is `pending`, unclaimed, has no PRD owner, and its acceptance option (b) is verbatim what D1 chose for the strict-`auto`-`Partial` cell. Its named artifacts (`crates/reify-constraints/tests/uniqueness_basin_awareness.rs`, `crates/reify-eval/tests/solver_uniqueness_unproven.rs`, `examples/solver_uniqueness_unproven.ri`) do not exist yet and become β's deliverables. Its stale `metadata.files` entry naming a non-existent `crates/reify-constraints/src/affine.rs` is corrected at decompose time.

**D9 — `#6655` is a hard prerequisite, not a parallel effort.** Building a second interval layer here would duplicate the propagator that task exists to deliver and was explicitly asked to make sub-box-capable. γ depends on it by a real `add_dependency` edge.

---

## §6 — Pre-conditions for activating

| Prerequisite | Status at authoring | Relationship |
|---|---|---|
| **#6655** — HC4 interval propagation, sub-box-capable; whole-box empty ⇒ proven-infeasible | pending, medium, **no deps — dispatchable now** | **hard dependency** of γ (D9) |
| **#5388** — strict-auto basin blindness | pending, unclaimed | **adopted as β** (D8) |
| **#5711** — `verify_uniqueness` near-inert for inequality-bracketed autos | in-progress | β must land *after* it or absorb its outcome; the `_ =>` "assume unique" arm is the exact line β retypes. Coordinate — do not race. |
| **#6653** — toleranced Scalar equality verdicts | pending, high | Adjacent and load-bearing: today a correctly-solved root is re-checked at exact `f64` and reported `violated` (probe-confirmed on five fixtures). Without it, several §8 fixtures cannot be green end-to-end. **Hard dependency** of δ. |
| **#6654** — DimensionalSolver budget hygiene | pending | Coordinates: the enumerator changes which seeds the solver gets. No hard edge. |
| **#6659** — registry dispatch de-trap, typed per-constraint refusal | **in-progress with a live claimant** editing `registry.rs` / `solvespace.rs` | **Collision hazard.** ι and θ touch the same files. Sequence after it lands. |
| **P2** leaf **ε #6672** (forward-mode AD) | decomposed `961228d217` | **Soft, deliberately unwired** — supplies the Jacobian for §3.3's uniqueness test; finite differences are the sound fallback (§11 Q1). A hard edge would block the enumerator on a whole sibling PRD for an accuracy gain it does not need. |
| **P2** leaf **ι #6677** (unified problem / relate-solve through the registry) and **ξ #6682** (SolveSpace retirement) | as above | **Hard** for **θ #6713** — edge WIRED. Both of θ's original targets are P2's to move (§7.1) |
| **P1** leaf **γ = #6691** (deterministic ordering, I4 / T1) | committed `108d1d9226`, decomposed | **Hard** for **ζ #6711** — edge WIRED. The first multi-`auto` leaf, and the first that can assert C6 in full (§3.2) |
| **P1** leaf **κ = #6699** (warm and cold choose the same solve entry point) | as above | **Hard** for **η #6712** — edge WIRED. A basin change is indistinguishable from an entry-point artefact without it (§7.1) |
| PRD 2 γ **#5469** / δ **#5470** / ζ **#5472** / θ **#5474** | pending | adopted, not absorbed; §7 |

**Cross-PRD dependency wiring — CLOSED.** All three hard edges are real `add_dependency` edges: ζ #6711 → P1 γ #6691, η #6712 → P1 κ #6699, θ #6713 → P2 ι #6677. The fourth, γ #6708 ← P2 ε #6672, stays deliberately unwired because it is **soft** — wiring it would block the enumerator on a sibling PRD for an accuracy improvement that a finite-difference fallback already covers soundly. No steward obligation remains outstanding on this axis. A leaf whose cross-PRD prerequisite is unwired must not be dispatched on the strength of its intra-batch edges alone.

---

## §7 — Cross-PRD relationship (G4)

| Other PRD / task | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `docs/prds/v0_6/discrete-cost-minimisation.md` ζ **#5472** | produces-for | the §3.5 composition law: ζ's mixed loop reports `Partial { InnerSolveUnproven }` per discrete leaf and the registry meets them | **this PRD** owns the law and the carrier; **PRD 2** owns ζ's loop | queued (ζ pending) |
| `docs/prds/v0_6/discrete-cost-minimisation.md` θ **#5474** (`solutions()`) | produces-for | the `SolutionSet` + `Completeness` carrier that `solutions(P)` renders, and `E_SOLUTION_SET_NOT_ENUMERABLE`'s condition, which is exactly `Partial { NotIntervalRepresentable \| DomainUnbounded }` | **PRD 2** owns the language surface; **this PRD** owns the carrier it renders | bookmark, design-first on dispatch |
| `docs/prds/v0_6/discrete-cost-minimisation.md` β **#5468** (landed) | consumes | `SolveAllResult { solutions, complete }` — generalised into `Completeness`; CP-SAT's semantics are preserved exactly, re-expressed | **this PRD** owns the generalisation; PRD 2 owns `cpsat.rs` | landed substrate |
| `docs/prds/v0_6/ranked-solve-result.md` (F-result, landed) | consumes / extends | `RankedSolveResult::Ranked` gains `completeness`; `RankedCandidate` unchanged; `SolveResult` + `ConstraintSolver::solve()` stay frozen (I1) | F-result owns the carrier shape; **this PRD** owns the additive field | landed |
| `docs/prds/v0_6/constraint-solver-completion.md` (landed) | consumes / extends | `ObjectiveProvenance` + `reify explain` gain the solution set + completeness (ζ) | that PRD owns the provenance record; **this PRD** owns the added field and its rendering | landed |
| `docs/prds/v0_6/whole-model-objective-coupling.md` δ **#5016** (landed) | consumes | `multistart_points`' start set becomes box-derived (§3.3 step 6); the determinism regime (D5) is inherited unchanged | M-WHOLE owns the multistart back-end; **this PRD** owns the start-set derivation | landed |
| `docs/prds/v0_3/engine-integration-norm.md` §3.5 | extends impl | additive field on the `solve_ranked` carrier already catalogued by F-result's extension note | seam catalog unchanged — **no norm edit needed** | no seam change |
| **#5388**, **#5711**, **#6653** | adopts / coordinates | §6 | see §6 and D8 | — |

**No new contested-ownership pair** — checked against `docs/architecture-audit/phase-3-breadcrumb-map.md` §3's three known pairs (`persistent-naming-v2 ↔ multi-kernel`, `imported-field-source ↔ multi-kernel`, `topology-selectors ↔ persistent-naming-v2`); this PRD touches none of them.

### §7.1 — P2 and P1 seams

This PRD was authored under a binding sequencing constraint: bind no cross-PRD seam until P2 is committed. P2 landed as `2ea72cc5e8` and **decomposed as `961228d217` into 19 leaves #6668–#6687**; the rows below are bound against that leaf set, re-walked 2026-08-26 after P2 exited. **Both siblings have since decomposed.** P1 landed `108d1d9226` with 15 leaves (#6689–#6704); P2 decomposed in `961228d217` with 19 leaves (#6668–#6687). All the edges below are therefore resolved: ζ #6711 → **P1 γ #6691**, η #6712 → **P1 κ #6699**, θ #6713 → **P2 ι #6677** are real `add_dependency` edges; γ #6708 ← **P2 ε #6672** is deliberately left UNWIRED because it is soft (see the row below). P2's steward has also executed the two corrections this section recorded as owed (`9e5662ad51`).

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| **P2** `geometry-algebra-solver-unification.md` | consumes | The unified trust-region Gauss–Newton core, and — decisively for §3.3 — **residual/Jacobian access for branch-and-bound**, which P2 §6 names as a P3 deliverable. P2 leaf **ε #6672** (forward-mode dual numbers over `CompiledExpr`) is what makes the interval Newton / Krawczyk existence-and-uniqueness test cheap; P2 leaf **μ #6680** preserves `eval_objective_set` as the single objective fold *because this PRD depends on it*. **γ #6708 is deliberately NOT dep-wired to #6672**: the dependency is soft, finite differences are an unconditional sound fallback (C8 — a failed uniqueness test just keeps splitting), and a hard edge would block the enumerator on a whole sibling PRD for an accuracy improvement it does not need. Consume #6672 if it has landed at γ's dispatch; do not wait for it. | P2 owns the core; **this PRD** owns enumeration | decomposed `961228d217` |
| **P2** | adopts ownership | **`E_SOLVER_DISJUNCTION_UNSUPPORTED`** — P2 leaf δ **#6671** mints the code and names P3 as the owner of the semantics. Accepted: a top-level `Or` is genuinely multimodal, and §3.3's subdivision is structurally the right answer (each disjunct is a branch). **Not scoped as a leaf here** — the code cannot fire until P2 lands, and there is no author-facing disjunction surface to serve yet. Recorded as an owned follow-up in §10. | **this PRD** owns the semantics; P2 owns the code and the refusal | accepted, deferred |
| **P2** | **corrects** | P2 §10 states *"`#5388`, `#5711`, `#5472`, `#5474` live there [P3]"*. **#5388 and #5711 do** (D8, §6). **#5472 and #5474 do not** — they are PRD 2 `discrete-cost-minimisation.md` leaves ζ and θ, and Leo's 2026-08-26 ruling is that this PRD sequences around them rather than absorbing them (§0.5). A one-line correction to P2 §10 is a companion obligation on this PRD's decompose steward. | PRD 2 owns ζ/θ | corrected (`9e5662ad51`) |
| **P2** | **corrects** | P2 §5 lists *"the interval substrate"* among the things P3 consumes from P2. P2 builds none — it holds #6655 as a *soft* edge on leaf κ and says "wire the edge, do not absorb". **The interval substrate is #6655's**, and this PRD binds #6655 directly (D9). Same one-line companion correction. | **#6655** | corrected (`9e5662ad51`) |
| **P2** | coordinates | P2 leaf **κ #6678** replaces `PENALTY_WEIGHT` with squared slacks (`g(x) − s² = 0`), adding `Slack` unknowns to the variable vector. **The enumeration envelope counts `auto` params, never slack or `Aux` unknowns** — slacks are determined by the autos, so subdividing over them would blow the envelope for no information. Stated so a later implementer does not measure the wrong dimension. | each owns its side | coordinate |
| **P2** | superseded-by | P2 leaf **ξ #6682** deletes `SolveSpaceSolver`, `GeometricPattern` and `recognize_pattern` outright (keeping `SystemBuilder` / `solve_sketch` / `slvs_sys` for the 2D sketch path), and leaf **ι #6677** routes relate-solve through the unified problem. Both of θ's targets are therefore P2's to move. θ is re-scoped accordingly (§8) from "fix two drivers" to "C1 conformance over whatever writes `unique` after P2 ι", and dep-wired to **#6677**. | P2 owns the deletion; **this PRD** owns C1 conformance | θ #6713 → #6677, WIRED |
| **P2** | **co-tenant on `reify-ir/ranked.rs`** | P2 leaf **μ #6680** adds `BestFoundReason::FirstOrderStationary` **replacing `Unreported`**, in the same file α #6706 adds the `completeness` field to. Two consequences. (a) **α's default trait lift must not hard-code `Unreported`** — §2.2 lists it as an existing variant and §3.6's table names it, both true only until μ lands; whichever lands second reconciles, and α should key its lift off "whatever the trait default reports" rather than a named variant. (b) The two PRDs are independently retiring their axis's *no-answer sentinel* — μ retires `Unreported` on the optimality axis because it now has a real answer, θ retires `Partial{NotAttempted}` on the completeness axis for the INV-SF-4 reason in §8.1. That symmetry is deliberate; neither axis should reacquire one. | each owns its own field | coordinate; no edge |
| **P2** | **`FirstOrderStationary` must NOT be read as satisfying C2** | This is the load-bearing one. P2 μ #6680's certificate is `‖Zᵀ∇f‖` — a **first-order stationarity** result, and stationarity is **local**: it says the objective's projected gradient vanishes at *this* point, and says nothing whatever about other basins. P2 correctly keeps it a `BestFound` *reason* rather than promoting it to `ProvenOptimal`. Once both PRDs land, "we have an optimality certificate now, so we can report `ProvenOptimal`" is a plausible-looking and **wrong** refactor: C2 requires `Exhaustive`, and a local certificate cannot supply it. A stationary point in one basin plus an unenumerated domain is exactly the false-completeness claim this PRD exists to prevent, arriving from the optimality side. | **this PRD** owns C2 | pin at α and μ |
| **P2** | coordinates on `registry.rs` | P2 leaf **ν #6681** replaces name-based classification with capability-based routing (and absorbed the now-cancelled #6523). ι #6715 rewrites the per-component `unique` conjunction in the same file into the §3.5 `Completeness` meet. Different functions, but there is a real semantic touchpoint: once ν can make a component's solver **decline**, the meet needs a `Completeness` for a declined component. It is `Partial`, with the reason carrying P2 δ #6671's typed per-constraint refusal — **not** `NotAttempted`, which θ retires. Without this stated, an ι implementer meets a component with no verdict and improvises. | each owns its own function | coordinate; no edge |
| **P1** `solver-driver-parity.md` | **depends on** | **P1 leaf γ (invariant I4 / determinism tier T1)** is a hard prerequisite of C6. `SubProblem.auto_params` is a `HashSet` whose iteration order becomes the solver's *axis order*, and `im::HashMap`/`HashSet` over `RandomState` iterate differently **per instance**, not merely per process. Until γ lands, no subdivision order derived from that axis order is reproducible, and this PRD's multi-`auto` determinism claim would be false. This PRD's own single-`auto` leaves (γ, δ, ε) are deliberately unaffected — a one-auto problem has no axis order. | P1 owns ordering | **hard dependency of ζ** |
| **P1** | seam drawn | P1 leaf **κ**'s signal B10 — *"an objective-bearing, ≥2-auto, multi-basin model resolves to the same optimum cold and warm"* — and this PRD's **η** are adjacent and must not fight. **The split: P1 owns entry-point parity** (the *same* problem must give the same answer regardless of driver, warm or cold — today cold calls `solve_ranked_with_dispatch` and warm calls `solve_with_dispatch` unconditionally). **This PRD owns basin identity as a value, and the diagnostic when a *changed* problem's solution lands in a different basin.** η therefore depends on P1 κ: without entry-point parity a basin "change" cannot be distinguished from an entry-point artefact. | P1 owns parity; **this PRD** owns identity + `W_BASIN_CHANGED` | **hard dependency of η** |
| **P1** | corroborates | P1 §2.4 independently root-causes the basin flip this PRD exists to surface: *"two equal-scoring starts in different basins hand the win to whichever got the lower index"*, and notes the existing BT5 determinism tests miss it (one uses a spy solver; the other bypasses `SolverRegistry`). §3.3's box-derived, declaration-ordered subdivision removes the tie-break's dependence on axis order entirely. | — | corroborating evidence |
| **P1** | **corrects** | P1 §11 cites **`#5655`** for interval seeding; the task is **#6655**. A wrong id reads as a real edge. Flagged to P1's steward and executed in `9e5662ad51` (2026-08-26) — P1 §11 now cites #6655. | P1 | corrected (`9e5662ad51`) |
| **P4** `solver-legibility-telemetry.md` | produces-for | P4 renders the solution set and completeness verdict in the GUI and debug MCP, both of which drop eval diagnostics wholesale today (§9). This PRD produces the carrier and the `reify explain` rendering; P4 owns the other surfaces. **Seam now claimed from both sides:** P4's own record slot for these verdicts was left with its live-task citation unstamped at its decompose — and its INV-SF-5 G7 row certifies a citation that was never made — so this session filed **#6751** against P4 to stamp α **#6706** / ζ **#6711** and correct the row. #6751 is dep-wired so P4-ω **#6739** cannot stamp a terminal status over the false claim. | P4 owns surfaces; **#6751** binds the seam | decomposed `b8a7680bac`; fix filed |
| **P4** | **co-tenants on `reify explain`** | P4 ξ **#6733** ("reify explain — the dropped fields, slack, and a failure vocabulary") and this PRD's ζ **#6711** both extend `ObjectiveProvenance` and both render through `reify explain`. The completeness verdict is precisely one of the fields P4's record should carry, so these two are co-tenants on one surface, not competitors. Which leaf adds the field and which renders it is left to **#6751** to decide and write down — neither DAG holds the seam today. | jointly; resolved by #6751 | coordinate |

**Reciprocal-ambiguity check.** P2 explicitly assigns basin enumeration, multistart policy and global uniqueness to P3, and this PRD accepts them — no reciprocal claim. **P1's cross-PRD table does not name a P3 multimodality PRD at all**, so both P1 rows above are bound unilaterally from this side and are a decompose-time obligation to mirror into P1 before it lands.

---

## §8 — Decomposition plan

**Task IDs stamped at decompose, 2026-08-26:** α #6706 · β #6707 · γ #6708 · δ #6709 · ε #6710 · ζ #6711 · η #6712 · θ #6713 · ι #6715 · λ #6716 · κ #6718 · μ #6719. Rows below carry the ID; they deliberately say nothing about task *status*, which rots the moment a task moves.

**B + H shape:** α is the contract linchpin; β is the honesty floor and lands standalone value before any engine exists; γ–ε are the vertical slice; ζ–θ are incremental slices; ι is the cross-domain law; κ–λ are the companion docs-truth and calibration phases; μ is the PRD-close leaf.

**Phase 1 — carrier + honesty floor**

- **α (#6706) — `Completeness` / `SolutionSet` carrier in `reify-ir`.** Modules: `reify-ir`. The enum pair of §3.1, the additive `completeness` field on `RankedSolveResult::Ranked`, C1's derived-`unique` helper, and the default `Partial { NotAttempted }` lift. *Intermediate — unlocks β, γ, ι.* **BT13.**

- **β (#6707) — honesty floor: stop *asserting* unproven uniqueness (adopts #5388).** Modules: `reify-constraints` (`solver.rs`), `reify-eval` (`engine_eval.rs`, `engine_edit.rs`), `reify-core` (`diagnostics.rs`). Three parts: (1) retype `verify_uniqueness`'s non-convergence arm to `Partial { ProbeOnly }` in the carrier instead of `true`; (2) fix the message and cause on the path that **already fires** — the strict-auto probe that *did* find a second solution — so it names the alternative and carries a typed non-uniqueness cause instead of "consider using auto(free)" + `SolveFailed { detail: "infeasible" }`; (3) code the five free-auto warning sites (INV-SF-6) and emit once per component naming its params, not once per param.

  **Two scope rails, both design-level, both easy to get wrong.** *(a)* β must **not** change the determinacy of solved cells: a strict auto whose set is `Partial` still resolves and is still `Determined`, or every strict auto in the corpus stops realizing geometry. β changes what is *claimed*, never what is *computed*. *(b)* β must **not** surface the `ProbeOnly` downgrade as a new user diagnostic. Because the probe is near-inert (§0.2 wrong #3), almost every strict auto would report "unverified" the moment β lands — a warning storm carrying no information, on a path that has no better answer available until γ exists. The downgrade is carried in the carrier and surfaced at **δ**, where the enumerator can distinguish "unverified because above envelope" from "verified, and here are both roots". **#5388's acceptance (b) therefore completes at δ, not β.**

  *LEAF · signal:* `reify eval` on a committed two-root strict-auto fixture names the alternative the probe found and reports a typed non-uniqueness cause — where today it says "consider using auto(free)" and mislabels the cell `undef` as `infeasible`. Prereqs: **α**; coordinate with **#5711** (same arm, in progress — do not race).

**Phase 2 — the engine (vertical slice)**

- **γ (#6708) — box branch-and-bound enumerator.** Modules: `reify-constraints` (new `enumerate.rs`; consumes #6655's `interval.rs` / `hc4.rs`). §3.3 steps 1–6, C6/C7/C8. The envelope counts `auto` params only — never slack or `Aux` unknowns (§7.1, P2 κ #6678). *Intermediate — unlocks δ, ε, λ.* Prereqs: **α**, **#6655**; soft, deliberately unwired: **P2 ε #6672**.

- **δ (#6709) — verdict policy: both roots reach the user.** Modules: `reify-constraints` (`solver.rs`, `registry.rs`), `reify-eval`, `reify-core`. Wire γ into the resolution path; implement §3.4's table; retire the unconditional free-auto warning. *LEAF · signal:* `reify eval` on a committed two-root fixture names **both** roots and which was selected (`auto(free)`), and on the strict variant errors naming both instead of "consider using auto(free)". **BT2, BT3, BT4.** Prereqs: **γ**, **#6653**.

  **Why every δ fixture is single-`auto` by design.** C6's cross-run ordering claim rests on P1 leaf γ (deterministic axis order), which is neither committed nor stamped. A one-auto problem has no axis order to permute, so δ's signals are reproducible today without that edge. The multi-auto ordering claim is asserted at **ζ**, which carries the P1 dependency.

- **ε (#6710) — refutation by subdivision.** Modules: `reify-constraints`, `reify-core`. `Refuted { narrowing }` from an all-sub-boxes-empty subdivision, rendered as a typed proven-infeasible diagnostic naming the constraint, distinct from the residual-based search failure (C3).

  **Scope rail — do not claim #6655's case as this leaf's.** `tests/prd-gate/fixtures/ssc_refuted_pair.ri` (`L == 10mm` **and** `L == 20mm`) is refuted by HC4 at the **root box**, so **#6655 alone turns it green**; it is committed here as baseline evidence for C3 and for #6655's arm, not as ε's signal. ε owns the strictly harder case: a root box that HC4 does **not** empty, whose every sub-box empties under subdivision. Such a fixture cannot be authored and verified before the engine exists — HC4 is strong in one dimension, and whether a candidate survives root contraction is an empirical question about the propagator #6655 delivers. **ε's acceptance therefore includes constructing it and demonstrating that root-box HC4 leaves it non-empty**; a fixture that #6655 already refutes does not satisfy this leaf. *LEAF · signal:* `reify eval` on that fixture emits the typed refutation naming the narrowing constraint with **zero** Nelder-Mead iterations, and a companion assertion shows root-box contraction alone does not decide it. **BT5.** Prereqs: **γ**, **#6655**.

**Phase 3 — set exposure, stability, driver conformance**

- **ζ (#6711) — stop discarding the alternatives.** Modules: `reify-eval` (both `swap_remove(0)` seams), `reify-cli` (`cmd_explain`). Dedup `solve_ranked` candidates by basin box (C5), attach the completeness verdict, extend `ObjectiveProvenance`, render in `reify explain`. *LEAF · signal:* `reify explain` on a multi-basin objective fixture prints the deduplicated solution set with its completeness verdict — a surface that has never existed. **BT10.** Prereqs: **δ**; cross-PRD hard: **P1 γ #6691** (wired) — ζ is the first leaf whose fixture is multi-`auto`, so it is the first that can be permuted by the `HashSet`-derived axis order, and the first that can honestly assert C6 in full.

- **η (#6712) — basin stability across warm re-solves.** Modules: `reify-eval` (`engine_edit.rs`, `eval_cached`). Record the winning basin's box; seed a warm re-solve from the incumbent; emit a coded `W_BASIN_CHANGED` when the re-solve lands in a different box. Closes the cold-only emission hole for these warnings on the warm path. **Scope boundary (§7.1):** P1 κ owns *entry-point parity* — the same problem giving the same answer cold vs warm. η owns *basin identity* and the diagnostic for a **changed** problem whose solution moved. η does not re-implement parity. *LEAF · signal:* editing a param on a two-basin model via the GUI/`edit_param` path either keeps the basin or names the move. **BT11.** Prereqs: **δ**; cross-PRD hard: **P1 κ #6699** (wired).

- **θ (#6713) — C1 conformance: no `unique` written from anything but a completeness verdict.** Modules: `reify-constraints`. **Re-scoped at authoring (§7.1):** the two over-claiming producers this leaf originally targeted are both P2's to move — leaf **ξ** deletes `SolveSpaceSolver` outright and leaf **ι** routes relate-solve through the unified problem — so fixing them here would be work P2 deletes. θ instead sweeps every remaining `unique:` write site *after* P2 ι and makes each derive from `Completeness` per C1, with `Partial { ProbeOnly }` the honest default for a driver that only probes. *LEAF · signal:* a geometric/`relate` fixture that lands in one of several configurations no longer reports fully determined; no `unique: true` literal survives outside the C1 helper. **BT9.** Prereqs: **α**; cross-PRD hard: **P2 ι #6677** (wired); sequence after **#6659**.

**Phase 4 — cross-domain**

- **ι (#6715) — the composition law at the registry.** Modules: `reify-constraints` (`registry.rs`). `solve_inner`'s per-component `unique` conjunction becomes the §3.5 meet over `Completeness`, with the reason naming the responsible component. Specifies — and does not implement — PRD 2 ζ's reporting contract. *LEAF · signal:* a two-component fixture with one `Exhaustive` and one `Partial` component reports `Partial` naming the responsible component. **BT12.** Prereqs: **δ**; sequence after **#6659**. Coordinate with P2 ν #6681 on `registry.rs`: a component whose solver *declines* under ν's capability screen meets as `Partial` carrying P2 δ #6671's typed refusal reason — never `NotAttempted` (§7.1).

**Phase 5 — calibration + companion docs**

- **λ (#6716) — envelope calibration.** Modules: `reify-constraints`, `docs/notes/`. Measure the dimension/node envelope on representative components, set the cap constant from the measurement, and commit the measurement as a dated note. Refutation and enumeration get **separate** caps — refutation needs no uniqueness test and prunes harder, so its envelope is genuinely larger, and pretending otherwise would under-serve the printer's infeasibility-theorem case. *LEAF · signal:* a fixture at the boundary degrades to `Partial { DimensionAboveEnvelope }` within the node budget rather than hanging, and the committed note records the numbers the cap came from. **BT7.** Prereqs: **γ**.

- **κ (#6718) — docs-truth (overlay gate: all four parts).** Modules: `crates/reify-mcp/src/tools/chunks/constraints.md`, `examples/best_practices/discrete_choice.ri` + `INDEX.md`, `.claude/skills/reify-design/SKILL.md`. (1) `constraints.md` gains a multiplicity section — it has **zero** content on uniqueness or multiple solutions today, verified by grep. (2) `discrete_choice.ri`'s caveat is rewritten: the "CP-SAT will bring branch-and-bound" claim is false for a continuous `Real` auto and must say rung 2 instead; its INDEX.md row follows (build-gated by `best_practices_index_matches_corpus_directory`). (3) the reify-design cheatsheet index line. (4) **Discoverability:** an author who knows the *goal* — "is there more than one answer to my model?" — finds the mechanism from the chunk or the corpus index without knowing the words "basin" or "completeness". *LEAF · signal:* the updated chunk's documented behaviour is exercised by a smoke `.ri`, and the corpus index check stays green. Prereqs: **δ**, **ζ**.

- **μ (#6719) — PRD close.** Set the Status marker to the terminal token, name the landed leaf IDs, add the AS-AUTHORED freeze paragraph and the LIVE/AS-AUTHORED map, and apply the matching header to the capability manifest. *LEAF · signal:* the committed header. Prereqs: **every other leaf** (real `add_dependency` edges; a `cancelled` sibling counts as satisfied).

**Dependency view**

```
   α#6706 ─┬─→ β#6707
            ├─→ γ#6708 ─┬─→ δ#6709 ─┬─→ ζ#6711 ──→ κ#6718
            │           │           ├─→ η#6712
            │           │           └─→ ι#6715
            │           ├─→ ε#6710
            │           └─→ λ#6716
            └─→ θ#6713
  #6655 ──→ γ#6708, ε#6710     #6653 ──→ δ#6709     #6659 ──→ θ#6713, ι#6715
  cross-PRD hard:  #6691 ──→ ζ#6711    #6699 ──→ η#6712    #6677 ──→ θ#6713   (all WIRED)
  cross-PRD soft:  #6672 ┈┈→ γ#6708   (Jacobian; deliberately UNWIRED — finite differences are the sound fallback)
  all leaves ──→ μ#6719
```

**Gate-test drift-guard registration.** γ, δ, ε, ι and θ each add gate-resident `crates/*/tests/*.rs` integration tests; their nextest heavy/smoke partition entries in `.config/nextest.toml` are **same-diff** with the test that needs them, per the overlay rule. No leaf adds a `tests/infra/test_*.sh`, and **no leaf adds a wall-clock assertion** — C7 makes every budget a node count, so `tests/infra/test_no_new_wallclock_upper_bounds.sh` needs no new registration. λ is the leaf most at risk of violating this and is explicitly chartered to assert on node counts.

### §8.1 — Design-invariant walk (G7)

Walked against `docs/legibility/design-invariants.md` (reify's normative list: the silent-failure family INV-SF-1..**7** and the angle-crossing family INV-AD-1..4), every task in the batch, not only leaves. Two hits, both resolved in the design above rather than waived.

| Invariant | Verdict |
|---|---|
| INV-SF-1 `undef-has-provenance` | **Addressed.** Every new path that leaves a cell `undef` records a cause: β replaces the mislabelled `SolveFailed { detail: "infeasible" }` on the strict-auto non-uniqueness path with a typed non-uniqueness cause, and ε's `Refuted` gets its own cause naming the narrowing constraint. No new causeless root undef. |
| INV-SF-2 `error-severity-exits-nonzero` | **Addressed.** The one new Error (strict `auto`, `Exhaustive`, n > 1) rides the existing eval diagnostic channel, which `cmd_eval` already exits nonzero on. The corollary drove §3.4's severity split: `Partial` is expected on a healthy above-envelope model, so it is a Warning, not an Error. |
| INV-SF-3 `declared-intent-consumed-or-diagnosed` | **Addressed.** Every path where the enumerator declines to establish the set emits a named `PartialReason` — above envelope, unbounded domain, not interval-representable, budget exhausted. λ's cap is a decline that is diagnosed, never a silent skip. |
| INV-SF-4 `indeterminate-attributable-transient` | **HIT — resolved.** `Partial { NotAttempted }` is *permanently* unattributable for a driver that never reasons about the set: nothing at runtime clears it, which is the structural-indeterminacy shape this invariant forbids. **Resolution:** `NotAttempted` is a **migration state only**. Every in-tree producer is moved off it by θ, and θ's diff carries an assertion that no in-tree `ConstraintSolver` impl reports it. The variant survives solely for the defaulted trait lift, where it correctly means "this solver was never asked". Every other `PartialReason` names a runtime condition that can clear (raise the budget, bound the domain, land an interval form). |
| INV-SF-5 `placeholders-owned-and-loud` | **Addressed via the above.** `NotAttempted` is a sentinel default, so it carries a PTODO cite to θ's task for as long as any in-tree producer still reports it. |
| INV-SF-6 `diagnostics-carry-codes` | **Addressed, and repays a standing debt.** Every diagnostic this PRD adds carries a `DiagnosticCode`, and β additionally codes the five pre-existing code-less free-auto warning sites — a live violation sitting in exactly the vocabulary this PRD extends. |
| INV-SF-7 `parse-is-value-faithful` | **N/A.** No grammar added (D2, §2.3). |
| INV-AD-1 `angle-crossings-explicit` | No angle↔dimensionless crossing introduced. The enumerator carries `DimensionVector` through unchanged. |
| INV-AD-2 `quotient-pure-derivative-algebra` | **HIT — resolved.** Subdividing an angle-typed auto over `default_bounds_for`'s `(−τ, τ)` **double-covers the circle**, so an "exhaustive" enumeration would report every rotational solution twice and stamp `Exhaustive` on a doubled count — the completeness machinery manufacturing a false completeness claim. **Resolution:** §3.3 canonicalises angle domains to one period before subdivision and identifies roots modulo the period, as a stated correctness precondition of `Exhaustive`. |
| INV-AD-3 `tensor-single-quantity` | N/A — no tensor-valued residuals introduced. |
| INV-AD-4 `boundaries-declare-angle-convention` | **Addressed by INV-AD-2's resolution:** the canonical period is the enumerator's declared angle convention at its own boundary, and `Completeness` crossing the `reify-ir` seam carries no bare angle. |

**No G7 waivers.** Both hits changed the design.

---

## §9 — Out of scope for this PRD

- The `solutions()` language surface, purposes-as-solve-scopes, and any solution-set *value* type in the language → PRD 2 bookmark **θ #5474**.
- The mixed discrete×continuous outer loop → PRD 2 **ζ #5472**. MINLP / SCIP / `russcip` remain rejected (M-WHOLE §3.1, binding).
- The HC4 propagator and the whole-box pre-solve refutation → **#6655** (this PRD consumes both).
- CP-SAT registry population, `Int` bound-mining, `discrete_set` wiring → PRD 2 **γ #5469** / **δ #5470**.
- **Integrality for `Int` autos.** `param n : Int = auto(free)` resolves to `3.3` with the type annotation erased and no diagnostic. Real defect, adjacent, not this PRD — §10.
- **Enum autos reaching a solver at all** (`solve failed: infeasible` at residual `1.00e0` today) → PRD 2.
- `@solver_hint` consumption of any kind → PRD 2 δ / `solver-hint-payloads.md`.
- `minimize … where …` guard semantics (silently discarded) → **#6647**.
- Making `reify check` / `reify build` run the solver → P1 / **#6631**.
- Exact-`f64` post-solve verdict re-checking → **#6653** (hard dependency, not scope).
- A stochastic global optimiser for above-envelope components (D5) → future PRD, triggered only if real models need it.
- Pareto-front exploration → spec §10.4, a tooling concern.
- GUI rendering of the solution set. The GUI drops eval diagnostics wholesale (`get_diagnostics` reads `compiled.diagnostics` only; `CheckResult.diagnostics` / `EvalResult.diagnostics` are never read in `gui/src-tauri/`), so surfacing anything there is a GUI-plumbing PRD, not this one → **P4 solver legibility**.
- MCP exposure of solve verdicts (`CliToolContext::get_constraints` hardcodes `status: "unknown"`; `load_file`/`update_source` discard `EvalResult` entirely) → **P4**.

---

## §10 — Follow-ups this PRD deliberately does not absorb

Each is a real defect surfaced by this PRD's groundwork probes, verified at the anchor commit, with no existing owner found. They are named here so the decompose session files them rather than losing them. *(Dispositioned 2026-08-26, post-decompose: item 1 → filed as #6755; item 2 → subsumed by #6631 / P1-δ #6693, not separately filed; item 3 → owned by P1-ε #6692; item 4 → filed as triage #6756; item 5 → curator-combined into δ #6709; item 6 → filed as #6754, dep-wired on P2 δ #6671.)*

1. **`Int` autos are silently relaxed to reals.** `param n : Int = auto` → `n = 6.999999999999999 dimensionless` (type erased, then adjudicated *violated*); `auto(free)` + `n >= 3` → `3.3`. No integrality projection, no diagnostic.
2. **`reify build` hard-fails on a top-level `auto` param** — `argument 'height' for cylinder is unresolved (Undef)` — where `check`'s no-solve posture is at least documented. Possibly subsumed by #6631; check before filing.
3. **An inequality-only strict `auto` with no upper bound parks at the `10 m` default-box corner with no warning.** #6655's derived box changes the value; the *silence* is the separate defect.
4. **`maximize` against a `<= 40mm` bound returns `24mm`**; `minimize` against `>= 8mm` returns `8.8mm`. Objectives look soft-penalised rather than bound-seeking. Needs triage before it is called a bug. *(Triaged 2026-08-28 by #6756: **it is a bug** — a correctness defect, not a tolerance one, because the objective's *sense* has no effect on the answer; mechanism = the silent seed-fallback, not soft-penalisation. **Wording —** the reported `24mm` needs a **two-sided** `8mm..40mm` shape; a genuinely one-sided `<= 40mm` returns `36mm`. So this is one defect, not two. **No new fix task:** loudness → #6654 arm 3; the `floor_applied` clamp gate → #5711; the 5e-7 root cause → #6678. Full verdict, the mechanism chain, the probe table and the boundary with item 3: `docs/notes/objective-seed-parking-triage-2026-08-27.md`.)*
5. **The merged-cluster free-auto warning fans one cluster-wide flag out to every free auto in the cluster**, so it can name a parameter that is not the culprit. β fixes the code-less half; the fan-out is a separate correctness question.
6. **Top-level disjunction semantics (owned, deferred).** P2 leaf δ **#6671** mints `E_SOLVER_DISJUNCTION_UNSUPPORTED` and names P3 as the owner of what a top-level `Or` should *mean*; today it silently becomes `min(violation_left, violation_right)`, which picks a branch without saying so — the same dishonesty this PRD exists to remove, one level up. §3.3's subdivision is structurally the right answer (each disjunct is a branch, and the union of their solution sets is the model's). Not scoped as a leaf because the diagnostic cannot fire until P2 lands and there is no author-facing disjunction surface to serve yet. File gated on P2 δ #6671.

---

## §11 — Open questions (tactical; deferred by design)

1. **Jacobian source for the interval Newton / Krawczyk test** (§3.3). **P2 leaf ε #6672** builds forward-mode dual numbers over `CompiledExpr` in `reify-expr` — there is no autodiff in the workspace today and no external crate is adopted — with acceptance that its Jacobian columns agree with central differences to 1e-6 relative. Sound either way: a failed uniqueness test simply keeps splitting, so a finite-difference fallback costs accuracy of the *test*, never soundness of the *verdict* (C8). **Suggested:** consume P2 ε #6672 if it has landed at γ's dispatch, else finite differences, and do not block on it. Decide during **γ**.
2. **Initial values for the two envelope caps** (§8 λ). Deliberately not guessed here: λ *is* the measurement. **Suggested:** start conservative (enumeration well below the Nelder-Mead simplex knee of ~10–15 vars that M-WHOLE measured, refutation higher) and let λ's data set them. Decide during **λ**.
3. **Whether the discriminating-constraint idiom (D2) is sufficient in practice.** If the §8 fixtures or the printer's Q1/Q2/Q3 show authors cannot express a selection cleanly, a `select` clause returns as its own design question. **Suggested:** revisit after κ, with fixture evidence. Not a blocker.
4. **Where the `Refuted` narrowing constraint is attributed when several constraints jointly empty the box.** §3.3 step 4 specifies a deterministic tie-break; whether the *most useful* attribution is the most-frequent or the first-to-empty is an ergonomics question. Decide during **ε**.
5. **Whether `θ`'s downgrade of `SolveSpaceSolver` to `Partial { ProbeOnly }` reds existing geometric fixtures** that currently rely on `unique: true`. Expected to be a small set; enumerate at **θ**'s dispatch and fix forward rather than pre-emptively weakening C1.
