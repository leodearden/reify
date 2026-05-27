# Tolerance Stack-Up Analysis

> A designer who has dimensioned a stacked/assembled set of parts wants one question
> answered before release: **does the accumulated ±tolerance keep a critical gap or fit
> within spec?** Reify already lets you *declare* per-feature dimensional tolerances
> (`stdlib/tolerancing.ri`: `DimensionalTolerance`, GD&T traits, `Fit`). What is missing is
> the *analysis* that propagates those tolerances along a dimension chain and reports the
> resulting gap distribution — worst-case, statistical (RSS), and Monte-Carlo. This PRD adds
> that analysis as a set of stdlib builtins surfaced through `reify eval`, mirroring the
> existing stress-analysis builtin pattern (`stdlib/analysis.ri` + `reify-stdlib/src/analysis.rs`).

Status: contract (B+H). Authored 2026-05-27 in a `/prd` spec-gap-filling batch.
Resolves spec §18 deferred item #7 ("Tolerance stack-up analysis — RSS, worst-case, Monte
Carlo; requires assembly graph + statistics").

---

## §0 — Scope boundary: this is *design dimensional* tolerancing, NOT *kernel realization* tolerancing

Reify has **two unrelated "tolerance" subsystems**; this PRD is firmly in the first and
must not touch the second:

| Concern | Owner | This PRD? |
|---|---|---|
| **Design dimensional tolerance stack-up** — does accumulated ±tol keep a gap/fit in spec | this PRD + `stdlib/tolerancing.ri` (GD&T types) | **YES** |
| **Geometry-conversion / realization tolerance budgeting** — how tightly the kernel must mesh/convert | `reify-eval/src/tolerance_budget.rs`, `tolerance_combine.rs`, `tolerance_scope.rs`, `engine_tolerance.rs`, `per-purpose-tolerance.md` | **NO — do not modify** |

The two share the word "tolerance" and nothing else. `tolerance_budget.rs` allocates a
realization-error budget across kernel conversion stages (an RSS-*looking* combine in
`tolerance_combine.rs` is over **conversion errors**, not design dimensions). This PRD's RSS
is over **design dimensional deviations**. Code lives in a **new** module
`reify-stdlib/src/stackup.rs`; it neither imports nor is imported by the budget machinery.

`per-purpose-tolerance.md` (v0.2) explicitly lists stack-up analysis as out of its scope and
points here (its "Out of scope" §). We build *on top of* the `stdlib/tolerancing.ri`
declaration types, not on the kernel-tolerance machinery.

---

## §1 — Consumer (G1)

**Named consumer: a mechanical designer running `reify eval <assembly>.ri` to check that a
stacked fit/gap stays in spec across accumulated tolerance.** The user-observable surface is
the `reify eval` CLI, which "evaluates and prints every top-level value cell"
(`reify-cli/src/main.rs:350`, `cmd_eval`). After this PRD, a `.ri` model with

```reify
let result = stackup_rss(chain)
```

prints `result = <map of nominal_gap / rss_sigma / worst_case_min / worst_case_max>` — a
number the designer reads directly and compares to spec.

Every mechanism this PRD introduces names that consumer:

| Mechanism | Consumer |
|---|---|
| `stackup_worst_case` builtin | `reify eval` prints the worst-case gap min/max |
| `stackup_rss` builtin | `reify eval` prints the RSS σ and the ±k·σ gap band |
| `monte_carlo_stackup` builtin | `reify eval` prints MC mean / σ / percentiles / yield fraction |
| `Contributor` / `StackupResult` / `Distribution` / `StackupMethod` stdlib types | the `.ri` author writes them; the builtins consume them; `reify eval` surfaces the result |
| `examples/tolerance-stackup-*.ri` | runs in CI; is itself the integration-gate observable |

No mechanism is an in-engine seam (kernel module / dispatcher / realization-kind / ComputeNode
dispatch), so the engine-integration-norm §3 sub-check does **not** apply — these are pure
stdlib builtins on the `eval_builtin` dispatch chain (`reify-stdlib/src/lib.rs:47`), exactly
like `von_mises` / `safety_factor` (`reify-stdlib/src/analysis.rs:12`). That dispatch chain is
the existing, catalogued surface; this PRD adds one arm (`stackup::eval_stackup`).

---

## §2 — Spec & substrate grounding (G3): no novel syntax

The analysis is expressed entirely with **grammar that already parses today**. Verified via
the grammar gate (`tree-sitter parse --quiet`, exit 0) on five fixtures covering every
syntactic shape this PRD uses:

| Fixture | Shape | Result |
|---|---|---|
| `stackup-1.ri` | `structure def Contributor { param … : Length … let half_band = … }` | parses |
| `stackup-2.ri` | `trait StackupResult { param … : Length  constraint … >= 0mm }` | parses |
| `stackup-3.ri` | `let chain = [contributor(10mm,0.1mm), …]  let r = stackup_rss(chain)` (list literal of calls; free-function call) | parses |
| `stackup-4.ri` | `enum Distribution { Normal, Uniform, Triangular }` | parses |
| `stackup-5.ri` | `monte_carlo_stackup(chain, samples: 10000, seed: 42)` (named-arg colon form) | parses |

`grammar_confirmed = true` for **every** task. There is **no** grammar work in this PRD.
GR-040 preserved: no method-call syntax (`x.foo()`); all analysis is free-function form
(`stackup_rss(chain)`), matching the stdlib builtin convention. Named args use colon form
(`seed: 42`), per the language convention.

**Substrate that already exists** (each verified in-tree):
- `reify eval` value-cell printing — `reify-cli/src/main.rs:350` (`cmd_eval` prints
  `id = value` for every top-level cell, sorted).
- `eval_builtin` free-function dispatch chain — `reify-stdlib/src/lib.rs:47`; analysis arm
  pattern at `analysis::eval_analysis` (`reify-stdlib/src/analysis.rs:12`), returning
  `Value::from_real_scalar`, `Value::List`, `Value::Map`.
- `Value::Map(BTreeMap<Value,Value>)`, `Value::List`, `Value::Scalar` (dimensioned) —
  `reify-ir/src/value.rs:366+`. The multi-field result is a `Value::Map` keyed by string
  (deterministic `BTreeMap` ordering, so `reify eval` prints stable output).
- `stdlib/tolerancing.ri` `DimensionalTolerance` (`nominal`, `upper_deviation`,
  `lower_deviation`, `tolerance_band`) — the declaration substrate this PRD's `Contributor`
  reuses / aligns with.
- `Length` / dimensioned literals (`0.1mm`), `enum`, `trait`, `constraint`, list literals —
  all in `examples/*.ri`.

**Substrate that is NOT assumed (the de-risking choice):**
- **No struct-constructor runtime evaluation (GR-001 / SIR) is required.** The builtins take
  *primitive* values: a `Contributor` is passed as a `Value::Map`/record `{ nominal,
  plus_tol, minus_tol, sign }` (or the author may use the convenience builder builtin
  `contributor(nominal, tol)`), and the chain is a plain `Value::List`. So this PRD does
  **not** gate on `structure-instance-runtime.md`. If/when SIR lands, `Contributor(...)`
  struct-ctor sugar becomes available as an ergonomic improvement (§9 follow-up), but it is
  not on the critical path. This is deliberate: GR-001 is the highest-cardinality grammar-
  fiction cluster in the audit; we route around it.

---

## §3 — The three stack-up methods (G6: premise validation)

A **dimension chain** is an ordered list of *contributors*. Each contributor `i` has a
nominal dimension `dᵢ`, a signed direction `sᵢ ∈ {+1, −1}` (does this dimension add to or
subtract from the gap), and a symmetric or asymmetric tolerance (`+tᵢ⁺ / −tᵢ⁻`; for the
symmetric common case `tᵢ⁺ = tᵢ⁻ = tᵢ`). The **gap** (the measured closing dimension) is

> `gap_nominal = Σ sᵢ · dᵢ`

### 3.1 Worst-case (arithmetic) — `stackup_worst_case`
Every contributor simultaneously at its worst extreme in the gap-opening (and gap-closing)
direction:

> `worst_case_max = gap_nominal + Σ |sᵢ| · tᵢ⁺`
> `worst_case_min = gap_nominal − Σ |sᵢ| · tᵢ⁻`

**Premise (true, no configuration caveat).** Pure arithmetic sum of absolute tolerance
contributions — exact, no statistical assumption. Hand-calculable; the G2 fixture asserts it
to machine precision. *(G6 branch 2: closed-form; the identity is the triangle-inequality
extreme of the linear gap function — always achievable by construction.)*

### 3.2 Statistical RSS (root-sum-square) — `stackup_rss`
Treat each contributor's deviation as an independent random variable. For a **symmetric**
tolerance interpreted at `±3σ` (the standard convention; configurable via a `sigma_level`
arg defaulting to 3), `σᵢ = tᵢ / sigma_level`. Because the gap is a **linear** combination
`gap = Σ sᵢ dᵢ` and the deviations are independent, variances add:

> `σ_gap = √( Σ sᵢ² · σᵢ² ) = √( Σ σᵢ² )`  (since sᵢ² = 1)
> `rss_band = sigma_level · σ_gap`   (the ±band at the same sigma level)

**Premise (true; G6 branch 2 — name the configuration that earns exactness).** RSS variance
addition `σ²_gap = Σ σᵢ²` is **exact for a linear gap function with independent contributors**
— which the dimension-chain gap always is (it is linear in the `dᵢ` by construction). The
*only* configuration assumptions are (a) independence (designer's modelling choice, documented,
not enforced) and (b) the tolerance→σ mapping (`σ = t / sigma_level`), which is a stated
convention surfaced as the `sigma_level` arg. We do **not** claim the gap is Gaussian (it is
only asymptotically so); the σ_gap value itself is exact regardless of the marginal shapes,
because variance addition for a linear form needs only finite variances, not normality. The
fixture asserts `σ_gap = √(Σσᵢ²)` to machine precision — a basis that holds unconditionally.

### 3.3 Monte-Carlo — `monte_carlo_stackup`
Sample each contributor's deviation from its declared distribution (`Distribution::{Normal,
Uniform, Triangular}`, default `Normal` with `σ = t/sigma_level`), sum the signed
contributions per draw, and report empirical statistics over `samples` draws: mean, σ,
min/max, percentiles (p₀.₁₃, p₉₉.₈₇ ≈ ±3σ band by default), and **yield fraction** (the
fraction of draws whose gap lies within a supplied `[spec_min, spec_max]`).

**Premise (G6 branch 3 — determinism; this is the load-bearing one).** Monte-Carlo needs an
RNG and sampling. The premise to validate was: *"does Reify have a determinism constraint?
`#deterministic` exists — how does MC interact?"* **Finding (validated against the spec and
compiler): `#deterministic` does NOT exist.** The spec defines exactly four toolchain pragmas
— `#precision`, `#solver`, `#kernel`, `#version` — plus `#no_prelude`
(`docs/reify-language-spec.md` §13; recognized set in `reify-compiler/src/module_pragmas.rs`:
`precision`/`solver`/`kernel`/`version`). There is **no** `#deterministic` pragma anywhere in
the spec, grammar, or compiler. Reify's determinism is instead an *architectural invariant*:
"Identity equality compares specification identity… cheap, exact, **deterministic**" (spec
§5), and tie-breaks are "deterministic (lexicographic by fully-qualified name)" (spec §4.4).
The whole evaluation graph is content-addressed and reproducible.

**Therefore MC must preserve that invariant with an explicit, required `seed:` argument and a
self-contained deterministic PRNG** — *not* a process/wall-clock-seeded RNG and *not* an
external `rand` crate (the workspace has **no** `rand` dependency — verified; introducing one
risks platform-variant streams). The contract:

1. `seed:` is a **required** named arg (no implicit default) — a `monte_carlo_stackup` call
   with no seed is a compile/eval error (`E_StackupSeedRequired`). This forces reproducibility
   into the source text, consistent with Reify's "no hidden global frame / no hidden state"
   posture.
2. The PRNG is a vendored, fixed-algorithm, platform-independent generator (SplitMix64 →
   xoshiro256** seeded deterministically from `seed`; pure integer arithmetic, no float
   transcendentals in the stream itself). Same `(chain, samples, seed)` ⇒ **bit-identical**
   result on every platform and every run.
3. Distribution sampling uses deterministic transforms of the uniform stream (Box–Muller for
   Normal with a fixed pairing order; inverse-CDF for Uniform/Triangular). Sampling order is
   contributor-index ascending, then draw-index ascending — fixed and documented.

**Achievability basis (G6 branch 1 — the MC↔RSS convergence numeric bound).** The MC mean
must approach `gap_nominal` and MC σ must approach the RSS `σ_gap` as `samples → ∞` (for
Normal contributors). The fixture asserts convergence at a *defensible, derived* tolerance,
**not a guessed one**: the standard error of the sample σ is `σ_gap / √(2·samples)`, so at
`samples = 100_000` the expected relative error on σ is ≈ `1/√(200_000)` ≈ **0.22%**. The
fixture asserts MC-σ within **2%** of RSS-σ at a fixed seed (≈9× the 1-sigma SE — a robust,
non-flaky margin that the derived SE justifies), and pins the *exact* MC-σ value at that seed
as a regression golden (bit-stable by §3.3 contract). This is the calibration discipline the
G6 cautionary precedents (esc-3453 guessed-5%, esc-3770 impossible-exactness) demand.

---

## §4 — Contract (the H component: signatures + invariants)

### 4.1 Builtin signatures (Rust side, `reify-stdlib/src/stackup.rs`)

```text
eval_stackup(name, args) -> Option<Value>     // dispatch arm, mirrors eval_analysis
  "stackup_worst_case"  (chain: List<Contributor>)                        -> Map
  "stackup_rss"         (chain: List<Contributor>, sigma_level?: Real=3)  -> Map
  "monte_carlo_stackup" (chain: List<Contributor>,
                         samples: Int, seed: Int,
                         spec_min?: Length, spec_max?: Length,
                         sigma_level?: Real=3)                            -> Map
  "contributor"         (nominal: Length, tol: Length, sign?: Int=+1)     -> Map   // symmetric builder
  "contributor_asym"    (nominal: Length, plus_tol: Length,
                         minus_tol: Length, sign?: Int=+1,
                         distribution?: Enum=Normal)                      -> Map   // asymmetric builder
```

A **`Contributor` value** is a `Value::Map` with keys `nominal` (Scalar<LENGTH>), `plus_tol`,
`minus_tol` (Scalar<LENGTH>), `sign` (Int ±1), `distribution` (Enum, default Normal). The
stdlib `.ri` `structure def Contributor` (and the `contributor*` builders) is the authoring
front; the builtins read the map shape. Decoupling the *value shape* (map) from the *authoring
type* (structure) is what lets this PRD skip GR-001.

### 4.2 Result shape (`Value::Map`, deterministic key order)

| Method | Result keys |
|---|---|
| worst_case | `nominal_gap`, `worst_case_min`, `worst_case_max`, `worst_case_band` |
| rss | `nominal_gap`, `rss_sigma`, `rss_band`, `rss_min`, `rss_max`, `sigma_level` |
| monte_carlo | `nominal_gap`, `mc_mean`, `mc_sigma`, `mc_min`, `mc_max`, `mc_p_low`, `mc_p_high`, `mc_yield_fraction` (present iff spec bounds given), `samples`, `seed` |

All Length-valued outputs carry `Scalar<LENGTH>` dimension; `mc_yield_fraction`, `sigma_level`
are dimensionless Real; `samples`/`seed` are Int. `reify eval` prints each cell; a Map prints
its entries.

### 4.3 Invariants (asserted by boundary tests)
- **INV-1 (worst-case ⊇ rss ⊇ mc band, same sigma_level).** `worst_case_band ≥ rss_band` and,
  for Normal contributors, `mc_band ≈ rss_band` within MC SE. Worst-case is always the widest.
- **INV-2 (RSS exactness).** `rss_sigma = √(Σ (tᵢ/sigma_level)²)` to 1e-12 relative.
- **INV-3 (MC determinism).** identical `(chain, samples, seed)` ⇒ bit-identical Map on
  repeated eval and across platforms.
- **INV-4 (sign handling).** flipping all `sign`s negates `nominal_gap` but leaves all *bands*
  (worst_case_band, rss_sigma, mc_sigma) invariant.
- **INV-5 (degenerate chain).** empty chain ⇒ `E_StackupEmptyChain`; zero-tolerance chain ⇒
  σ = 0, band = 0, MC σ = 0 exactly.
- **INV-6 (dimensional consistency).** all `nominal`/`tol` must be `Length`; a non-Length
  contributor field ⇒ `E_StackupDimMismatch`.

### 4.4 Error semantics
Diagnostic codes (user-facing, surfaced via `reify eval` stderr like other eval diagnostics):
`E_StackupSeedRequired`, `E_StackupEmptyChain`, `E_StackupDimMismatch`,
`E_StackupBadSign` (sign ∉ {+1,−1}), `E_StackupBadSamples` (samples ≤ 0). Malformed
contributor maps ⇒ the builtin returns `Value::Undef` *and* emits the matching diagnostic
(mirrors the `Value::Undef` fall-through in `analysis.rs`).

---

## §5 — Dimension-chain identification (G4 seam — declare, do NOT wire)

**The decisive scoping decision.** A dimension chain can be obtained two ways:

| Approach | v1 disposition |
|---|---|
| **(A) Explicit chain declaration** — the author writes the ordered contributor list literally (`let chain = [contributor(10mm, 0.1mm), contributor(-5mm, 0.05mm), …]`). | **THIS PRD. Sole v1 mechanism.** |
| **(B) Derived from the assembly graph** — walk `connect`/`at` sub-placement poses, project onto a measurement axis, auto-extract `(nominal, tolerance)` per hop. | **Declared, NOT wired. Future follow-up.** |

**Rationale for explicit-only v1.** Auto-derivation (B) needs: (1) per-sub placement poses —
which are exactly what `sub-placement-and-surfacing.md` (this same v0_6 batch) is *adding*
and which do **not** exist as a stable consumable surface yet; (2) a per-feature tolerance
attached to a geometric dimension — `stdlib/tolerancing.ri` currently parks `feature` on a
`Real` placeholder pending a Geometry/DatumRef type (task #3116, see tolerancing.ri header);
(3) a designer-specified **measurement axis/datum** to project the chain onto. None of those
three is settled. Forcing (B) now would build the analysis against a moving substrate — the
exact G3/G6 failure these gates exist to prevent.

So v1 owns the **math + surfacing** with an explicit chain; **(B) auto-derivation is a
separate future PRD** that will *consume* this PRD's `stackup_*` builtins (the math is
identical; only the chain-construction front-end differs). This PRD's contributor-value shape
(§4.1) is the stable seam B will produce into.

### Cross-PRD relationship (G4)

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `v0_6/sub-placement-and-surfacing.md` | this PRD *could later consume* its placement poses for auto-derivation (B) | per-sub `world`/composed `Transform` (its §4) | **sub-placement owns the pose; auto-derivation owns the chain extraction — neither is this PRD** | **deferred** (B is a future PRD; v1 explicit chain has **no** dependency on sub-placement) |
| `v0_3/structure-instance-runtime.md` (GR-001) | this PRD *optionally consumes* if/when it lands | `Value::StructureInstance` for `Contributor(...)` struct-ctor sugar | **SIR owns**; this PRD uses primitive maps until then | **not on critical path** (ergonomic-only; §9 follow-up) |
| `stdlib/tolerancing.ri` | this PRD *builds on* | `DimensionalTolerance` / GD&T declaration types | tolerancing.ri owns the types; this PRD owns the *analysis over them* | **wired** (read-only reuse) |
| `v0_2/per-purpose-tolerance.md` | reciprocal scope-boundary note | (none — disjoint subsystems) | n/a | per-purpose explicitly defers stack-up here; no shared mechanism |

**No reciprocal-ownership ambiguity:** v1 declares **zero hard cross-PRD dependency**. The
sub-placement seam is *forward-declared as future consumption*, not a prerequisite — v1 ships
standalone on existing substrate. This is intentional decoupling, not a deferred-consumer
orphan (the consumer of *this* PRD's mechanisms is `reify eval`, named and present today).

---

## §6 — Approach: B+H (design-first)

G5 heuristic check: blast radius is **2 crates** (`reify-stdlib` for the builtins + stdlib
`.ri`; `reify-cli`/examples for the observable) — below the ≥3 trigger; mechanism count ≈ 6
builtins + 4 types ≈ 10 (above the ~8 coarse trigger); it touches **no** load-bearing seam
(not FEA / ComputeNode / persistent-naming / multi-kernel / grammar). Net: **borderline**.
We adopt **B+H lite** — the contract (§4) and the boundary-test sketch (§7) — because the
numeric premises (RSS exactness, MC determinism/convergence) are precisely the G6 hazard class
that has burned execution-time escalations before, and a contract-first pin is cheap insurance.
The decomposition is a vertical slice (§8) with a final integration-gate example task.

---

## §7 — Boundary-test sketch (facing both ways)

| # | Seam | Producer side | Consumer side |
|---|---|---|---|
| 7.1 | builtin ↔ eval dispatch | `eval_stackup` returns `Some(Value::Map)` for each name; `None` for unknown (so `eval_builtin` falls through) | `eval_builtin("stackup_rss", …)` (`lib.rs:47` chain) routes to it and yields the Map; an unknown stackup-ish name ⇒ `Value::Undef` |
| 7.2 | stdlib `.ri` ↔ builtin | a `Contributor`/`contributor(...)` value is a Map with the §4.1 keys | the builtin reads the map; a malformed map ⇒ `E_StackupDimMismatch` + `Value::Undef` |
| 7.3 | math ↔ hand-calc (RSS/worst-case) | builtins compute per §3.1/§3.2 | a 3-part golden `.ri` evaluated by `reify eval` prints `worst_case_band`, `rss_sigma` equal to hand-computed values to 1e-12 |
| 7.4 | MC determinism | `monte_carlo_stackup` uses the vendored seeded PRNG | two evals at the same seed print bit-identical `mc_sigma`/`mc_mean`; a different seed differs; convergence: MC-σ within 2% of RSS-σ at samples=100k (derived SE basis §3.3) |
| 7.5 | eval ↔ CLI surface | the result Map is a top-level `let` cell | `reify eval <f>.ri` prints `result = {…}` to stdout, exit 0; a seed-less MC call prints `E_StackupSeedRequired` to stderr, exit 1 |

7.3 and 7.4 are the **rigorous** verification consumers (exact / reproducible, runnable from
CLI). The example task (T7) is the integration gate that ties them end-to-end.

---

## §8 — Integration DAG (proposed; not yet filed)

Each leaf names its **user-observable signal** (G2). Minimum end-to-end vertical slice
(C-as-integration-gate spine): **T1 → T2 → T3 → T7** (value shape → worst-case+rss math →
eval-dispatch wiring → CLI example proving a hand-calc match). MC (T4/T5) and stdlib authoring
(T6) hang off the spine.

### Phase 1 — Value shape + dispatch foundation
- **T1 — `Contributor` value shape + `contributor`/`contributor_asym` builders + `eval_stackup`
  dispatch arm (stub math).** Independent. *grammar_confirmed=true.* Signal: `reify eval` of a
  `.ri` with `let c = contributor(10mm, 0.1mm)` prints a `c = {nominal: 10mm, plus_tol: 0.1mm,
  minus_tol: 0.1mm, sign: 1, …}` Map; `eval_builtin("contributor", …)` returns that Map (not
  `Undef`); unknown stackup name still falls through to `Undef`. *(Intermediate — unlocks
  T2/T4.)*

### Phase 2 — Worst-case + RSS math (the exact, hand-calc'able core)
- **T2 — `stackup_worst_case` + `stackup_rss` builtins.** Depends T1. *grammar_confirmed=true.*
  Signal: `reify eval` of a 3-contributor chain prints `worst_case_min/max` = arithmetic
  hand-calc and `rss_sigma` = √(Σ(tᵢ/3)²), each matching a comment-documented hand calculation
  to 1e-12; `sigma_level: 6` arg changes `rss_band` accordingly; INV-1/2/4/5/6 unit-asserted.

### Phase 3 — Eval-surface integration (spine joint)
- **T3 — Wire `stackup::eval_stackup` into `eval_builtin` chain + diagnostic emission.**
  Depends T2. *grammar_confirmed=true.* Signal: from a clean checkout, `reify eval
  examples/…rss…ri` (a fixture committed in this task) exits 0 and prints the `rss`/`worst_case`
  result Maps to stdout; an empty chain prints `E_StackupEmptyChain` to stderr, exit 1.
  *(This is the integration joint that proves the builtins reach the real CLI consumer.)*

### Phase 4 — Monte-Carlo (the determinism-critical method)
- **T4 — Vendored deterministic PRNG + `Distribution` sampling primitives.** Depends T1.
  *grammar_confirmed=true.* Signal: a Rust unit test shows the seeded stream is bit-identical
  across two constructions from the same seed and matches a hard-coded golden first-16-draws
  vector; Normal/Uniform/Triangular inverse-transforms hit documented mean/variance on a large
  sample. *(Intermediate — foundation; roped into T5 as its integration consumer per G2 escape
  hatch.)*
- **T5 — `monte_carlo_stackup` builtin + required-`seed` enforcement + yield fraction.**
  Depends T2, T4. *grammar_confirmed=true.* Signal: `reify eval` of an MC `.ri` (seed fixed)
  prints bit-identical `mc_sigma`/`mc_mean` on two runs (INV-3); `mc_sigma` within 2% of the
  same chain's `rss_sigma` at samples=100k (derived-SE basis §3.3); a seed-less call prints
  `E_StackupSeedRequired`, exit 1; `mc_yield_fraction` for given `[spec_min,spec_max]` matches
  a closed-form Normal-CDF expectation within MC SE.

### Phase 5 — Stdlib authoring surface
- **T6 — `stdlib/tolerancing.ri` (or new `stdlib/stackup.ri`) declares `Contributor`,
  `StackupResult` trait, `Distribution`, `StackupMethod`.** Depends T1 (value-shape alignment).
  *grammar_confirmed=true.* Signal: the new `.ri` declarations parse (`tree-sitter parse
  --quiet` exit 0, no ERROR nodes) and `reify eval` of a file that uses
  `Distribution.Triangular` and the `StackupResult` trait compiles with no diagnostics; the
  declared types' field names match the §4.1/§4.2 builtin shapes.

### Phase 6 — Integration gate (the leaf observable)
- **T7 — End-to-end example + spec update.** Depends T3, T5, T6. *grammar_confirmed=true.*
  Signal: an `examples/tolerance-stackup-3part.ri` committed and run in CI — a 3-part stacked
  assembly (e.g. shaft + spacer + retaining-ring in a bore) declares an explicit chain and
  prints, via `reify eval`, worst-case / RSS / Monte-Carlo gap results whose numbers match the
  in-file hand-calc comments (worst-case & RSS exact to 1e-12; MC σ within 2% at fixed seed);
  `docs/reify-language-spec.md` §18 row 7 marked realized with a pointer; the example contains
  a header comment contrasting design-stack-up vs kernel-realization tolerance (§0). *(The
  single user-observable leaf that proves the whole chain.)*

### Dependency view
```
T1 ─┬─ T2 ─ T3 ───────────┐
    │       │             │
    └─ T4 ──┴─ T5 ────────┼─ T7
    └─ T6 ────────────────┘
            (T6 dep T1; T5 dep T2,T4; T3 dep T2; T7 dep T3,T5,T6)
```

---

## §9 — Out of scope (each gets a follow-up only if listed here)

- **Auto-derivation of the chain from the assembly graph (`connect`/`at` poses).** The §5
  Approach-B future PRD; consumes this PRD's builtins. Not filed here.
- **`Contributor(...)` struct-constructor sugar** (vs the `contributor(...)` builder builtin).
  Ergonomic-only; lands free once GR-001/SIR is in. Tactical follow-up, not a v1 task.
- **Geometric/GD&T tolerance zone propagation** (position/profile zones into the chain). The
  chain here is 1-D dimensional. Vector/3-D stack-up (e.g. tolerance-DOF along a measurement
  axis derived from frames) is future, and rides on §5-B.
- **Sensitivity / contribution analysis** (per-contributor % of total variance, Pareto). A
  natural follow-up on `stackup_rss` output; not v1.
- **Non-independent (correlated) contributors.** v1 assumes independence (documented modelling
  assumption). Correlation matrices are future.
- **Distribution fitting from measured data / process capability (Cp/Cpk).** Out — needs a
  measurement-data ingestion path.

---

## §10 — Open (tactical) questions

1. **Result container: `Value::Map` vs a `StackupResult` `Value::List`.** PRD picks `Value::Map`
   (keyed, self-describing in `reify eval` output, deterministic `BTreeMap` order). If `reify
   eval`'s Map printing proves unreadable, a parallel ordered-`List` accessor is a tactical add
   in T2/T5. (Pin the Map-print format in T2.)
2. **`sigma_level` default (3 vs 6).** PRD defaults to **3** (the ±3σ ⇒ tolerance convention,
   most common in mechanical practice). Some shops use 6σ. It is an explicit arg either way;
   the default is documentation, not a design fork. (Confirm in T2.)
3. **PRNG algorithm choice** (SplitMix64→xoshiro256** vs PCG). Both are tiny, deterministic,
   platform-independent, public-domain. PRD picks xoshiro256** for stream quality; the choice
   is encapsulated behind §4.3 and swappable as long as the golden-draw vector is regenerated.
   (Pin the golden vector in T4.)
4. **Box–Muller vs Ziggurat for Normal sampling.** PRD picks Box–Muller (no lookup tables →
   trivially portable/deterministic; MC perf is not a v1 concern at 100k samples). Tactical.
5. **Where the stdlib types live** (`tolerancing.ri` vs new `stackup.ri`). PRD leans new
   `stackup.ri` (keeps the GD&T file focused) but T6 may co-locate if the prelude wiring is
   simpler in `tolerancing.ri`. Tactical.

---

## DESIGN FORKS FOR LEO

These were resolved with reasoned defaults (AskUserQuestion does not route here); flagged for
override:

- **MC determinism (the big one).** `#deterministic` **does not exist** (validated — spec has
  only `#precision/#solver/#kernel/#version/#no_prelude`). I resolved MC determinism via a
  **required `seed:` arg + vendored platform-independent PRNG** (no `rand` dep — the workspace
  has none), giving bit-identical results and preserving Reify's architectural determinism
  invariant. *Fork:* if you'd rather MC be allowed nondeterministic (wall-clock seed) when no
  seed is given, say so — but that breaks the reproducible-eval-graph invariant, so I made
  `seed:` **required** (seed-less MC is an error). Alternative considered & rejected: a
  *future* `#deterministic` pragma — rejected as scope creep onto the pragma subsystem.
- **Dimension-chain identification.** Resolved to **explicit chain declaration only** in v1;
  auto-derivation from `connect`/`at` sub-placement is **declared as a future PRD**, not wired
  (the placement substrate is itself mid-flight in `sub-placement-and-surfacing.md`, and
  `feature` is a `Real` placeholder pending the Geometry/DatumRef type #3116). *Fork:* if you
  want auto-derivation in-scope now, this PRD grows a hard dependency on sub-placement landing
  first and a new measurement-axis/datum design — materially larger and riskier.
- **GR-001 routing.** Resolved to **primitive `Value::Map` contributors + builder builtins**
  so the PRD does **not** gate on structure-instance-runtime. *Fork:* if you'd prefer to wait
  for SIR and use `Contributor(...)` struct-ctor values throughout, the PRD gains a hard SIR
  prerequisite (cleaner authoring, slower to land). I chose de-risked-now.
- **RSS σ-mapping convention.** Tolerance interpreted at **±3σ** by default (`sigma_level=3`),
  configurable. This is a convention, surfaced as an arg, not buried.

## RESOLVED DECISIONS

- Builtins live in a **new `reify-stdlib/src/stackup.rs`**, dispatched via a new
  `stackup::eval_stackup` arm in `eval_builtin` (`lib.rs:47`) — mirrors `analysis.rs` exactly.
  **Zero contact** with the kernel-tolerance budget machinery (§0).
- Observable surface is **`reify eval`** value-cell printing (no new CLI subcommand needed).
- Result is a **`Value::Map`** (deterministic key order, self-describing).
- **Worst-case & RSS are exact** (machine-precision hand-calc fixtures); **MC** asserts
  bit-stable determinism + a **derived-SE-justified 2% convergence band** (not a guessed bound).
- Grammar gate: **all five fixtures parse; `grammar_confirmed=true` for every task; no grammar
  work.**

---

*Decompose note:* each task files with `planning_mode=True`, carries `user_observable_signal` /
`consumer_ref` / `grammar_confirmed` metadata, wires the §8 edges, and (per the orchestrator-
stopped instruction for this batch) **stays `deferred`** — it is **not** flipped to `pending`.
The orchestrator does not yet read those metadata fields (F-infra follow-up substrate).
