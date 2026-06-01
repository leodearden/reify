# PRD: `std.units.constants` — complete the named physical constants

**Status:** draft · **Authored:** 2026-06-01 · **Milestone:** v0_6
**Closes gap-register cluster:** P8 units-constants (`docs/architecture-audit/stdlib-reference-gap-register-2026-06-01.md`)
**Source doc:** `docs/reify-stdlib-reference.md` §2.4 (and §2.2)
**Approach:** bare **B** (self-contained stdlib feature; not a high-stakes seam — see §5 G5).

---

## 0. Supersession / relationship to prior work

This PRD completes the `std.units.constants` surface that task **#4026** (`docs/prds/v0_6/stdlib-reconstruction.md` task ζ) started. #4026 shipped `SPEED_OF_LIGHT()` + `BOLTZMANN_CONSTANT()` (and #3647 shipped `STANDARD_GRAVITY()`) as the first three dimensionful constants. Eight documented constants remain entirely absent (`e`, `avogadro`, `planck`, `stefan_boltzmann`, `vacuum_permittivity`, `vacuum_permeability`, `gas_constant`, `elementary_charge`), and the **form** of the existing three diverges from the doc (`STANDARD_GRAVITY()` vs the doc's `let g : Acceleration = …`).

No phantom-done or in-progress task covers any of the eight. `search_tasks` (2026-06-01) surfaced only #4026/#3647/#1762 (pi/tau) — all done and scoped to the existing four. Nothing to supersede; this batch is purely additive.

---

## 1. Consumer & user-observable surface (G1)

**Consumer:** end-user Reify `.ri` source — engineering designs that need physical constants (thermal, EM, relativistic, molar). Concretely the existing `examples/stdlib/constants.ri` (extended to all 12) and its CI regression test, plus LSP hover/completion which lists stdlib symbols.

These are **prelude values**, not an in-engine mechanism — there is no kernel/dispatch seam to orphan. The only engine touch-points are existing resolution seams: the dimensionless `e` plugs into the `constants.rs` builtin-constant resolver (same path as `pi`/`tau`); the bare `A`/`mol`/`cd` base units plug into the existing `unit_to_scalar` fallback in the unit-literal resolution path. Both are pre-existing seams; nothing new is introduced that needs a downstream integration task beyond the example/test (task δ).

**User-observable surface:** after this batch, a user writes `PLANCK_CONSTANT()`, `e`, `ELEMENTARY_CHARGE()`, etc. in a `.ri` file and `reify check` accepts it / `reify eval` prints the correct value with its SI dimension; the doc §2.4 shows forms that actually work.

---

## 2. Sketch of approach

The 12 constants split into **two categories**, each mapped to the idiom the substrate forces (see §3, §4):

| Category | Members | Idiom | Where |
|---|---|---|---|
| Dimensionless math constants | `pi`, `tau`, `e` | **bare identifier** (compiler builtin) | `crates/reify-compiler/src/constants.rs` |
| Dimensionful physical constants | `g`, `c`, `boltzmann`, `avogadro`, `planck`, `stefan_boltzmann`, `vacuum_permittivity`, `vacuum_permeability`, `gas_constant`, `elementary_charge` | **zero-arg `pub fn` UPPER_SNAKE** | `crates/reify-compiler/stdlib/units.ri` |

- `pi`/`tau` already ship (bare builtins); `STANDARD_GRAVITY()`/`SPEED_OF_LIGHT()`/`BOLTZMANN_CONSTANT()` already ship (fns). This PRD adds the **one** missing dimensionless constant (`e`) and the **seven** missing dimensionful fns, plus the **two** missing bare base units (`A`, `mol`; `cd` for completeness) that five of the new bodies depend on.
- Dimensionful fn bodies are written in **base-unit-expanded** form (the `BOLTZMANN_CONSTANT()` precedent) because stdlib fn bodies compile in a registry-less bootstrap scope that only sees the hardcoded base units (§3).
- Composite dimensions appear in fn return-type position via `pub type` aliases (the esc-4026-121 `Velocity`/`HeatCapacity` precedent), written left-associative with no parens / no `^` (§3).
- An integration example (task δ) ties the surface to **physics-identity cross-checks** that self-validate value *and* dimension (§3 explains why this is the real checksum).
- The doc is reconciled to the shipped reality (task ε).

---

## 3. Pre-conditions — verified substrate facts (G3)

All verified 2026-06-01 against `target/debug/reify` and source (the **real binary**, per the project G3 note that the tree-sitter CLI is stale).

1. **Idiom is forced, not chosen.**
   - Bare lowercase dimensionful names collide with **unit suffixes**: `g` is the gram (`units.ri:29 pub unit g : Mass`), `h` is hours, `s` is seconds. A bare `g`-for-gravity is impossible. This is *why* the fn idiom exists.
   - The builtin-constant path (`constants.rs::resolve_builtin_constant`) emits only **dimensionless** `Value::Real` (`Type::Real`). It cannot carry a dimension, so the dimensionful constants cannot live there.
   - ⇒ dimensionful ⇒ `pub fn` (carries a dimension via return type); dimensionless math ⇒ bare builtin. (Decision confirmed by user 2026-06-01 — see §4.)

2. **Scientific notation now parses in value position.** `let a = 6.62607015e-34` / `6.02214076e23` `reify check` clean (task **#3087** landed; the `BOLTZMANN_CONSTANT()` decimal-expansion comment is now stale). New bodies are written in natural sci-notation.

3. **Bare `e` parses as an identifier** (no scientific-notation collision — sci-notation `e` only occurs *inside* a number literal). `let x = e` currently yields `unresolved name: e`, exactly the gap adding it to `constants.rs` closes. The reverse-exhaustiveness test in `constants.rs` already lists `"e"` as a probe expecting non-resolution; adding `e` to `BUILTIN_NAMES` keeps both guard tests green.

4. **`A`, `mol`, `cd` are missing as bare units.** `SI_PREFIX_BASES` (si_units.rs) emits only *prefixed* variants (`mA`, `kA`, `mmol`, `mcd` all resolve); the **bare** base must come from `units.ri`, which declares only `m`/`rad`/`kg`/`s`/`K` — never `A`/`mol`/`cd`. Empirically `1A`/`1mol`/`1cd` ⇒ `error: unknown unit`. §2.2's "complete SI base units (m,kg,s,A,K,rad,mol,cd)" is therefore **false** for `A`/`mol`/`cd`.

5. **Stdlib fn bodies see only hardcoded base units.** `expr.rs:770-773` resolves a unit literal as `registry.lookup(...).or_else(|| unit_to_scalar(...))`. In the stdlib bootstrap scope the registry is unseeded, so bodies fall to `unit_to_scalar` (units.rs:442), whose entire table is `{mm, cm, m, in, deg, rad, kg, g, s, K}`. Derived units (`J`, `W`, `F`, `H`, `C`) and `A`/`mol`/`cd` are **not** available inside a fn body (verified: `1J` in a fn body ⇒ `unknown unit: J`). ⇒ the `A`/`mol`/`cd` fix must land in the **Rust `unit_to_scalar` table** (not merely a `.ri pub unit` line) so the electrical/molar constant bodies compile; this fix also closes the §2.2 gap everywhere (the fallback is consulted on every unit literal).

6. **Composite dimension aliases: no parens, no `^`.** `pub type X = Energy * Time` ✓, `Capacitance / Length` ✓, `Energy / AmountOfSubstance / Temperature` ✓ (left-associative), but `… / (A * B)` ✗ (`syntax error: )`) and `Temperature^4` ✗ (`syntax error: ^4`). ⇒ powers are written as repeated factors (`/ T / T / T / T`) and denominators flattened left-associatively.

7. **Inverse-amount dimension resolves as `Dimensionless / AmountOfSubstance`** (✓), **not** `Real / AmountOfSubstance` (`cannot resolve 'Real' to a dimension type`). Avogadro's `Real / Amount` doc-dimension reconciles to `Dimensionless / AmountOfSubstance`.

8. **Return-type is NOT a dimensional checksum.** `reify check` does **not** verify that a fn body's dimension matches its declared return type — a wrong exponent (`PLANCK() -> Action { … kg·m / s }`) and even a pure `1.0` body both pass clean. **But binary `+`/`-` *do* enforce dimensions** (`1m + 1s` ⇒ `dimension mismatch in addition`). ⇒ correctness of the bodies cannot rely on the return annotation; it is validated by (a) **physics-identity cross-checks** in the example (subtraction dimension-errors if any dimension is wrong), and (b) **`reify eval` value assertions** (eval prints value + dimension, e.g. `S.v = 299792458 m·s^-1`). This is the load-bearing G6 mitigation (see §4, manifest).

9. **`reify eval` evaluates concrete `let` bindings and prints value + dimension.** (Constants are concrete, not solver-driven, so the no-solver CLI limitation does not apply.)

---

## 4. Resolved design decisions

- **D1 — Idiom = principled two-category split** *(user-confirmed 2026-06-01)*. Dimensionless math constants `pi`/`tau`/`e` stay bare identifiers; the ten dimensionful constants are zero-arg `UPPER_SNAKE()` fns. Rationale in §3.1. "All zero-arg fns" (wrapping `PI()`/`E()`) was rejected — it breaks every existing `2 * pi` and gains nothing. "All bare identifiers" is infeasible (`g`=gram collision; dimensionless-only builtin path). The split maps each constant to a real category and the substrate that category forces.
- **D2 — Avogadro is dimensionful** *(user-confirmed 2026-06-01)*. `AVOGADRO_CONSTANT() -> InverseAmount` where `pub type InverseAmount = Dimensionless / AmountOfSubstance`, body `6.02214076e23 / 1mol`. Matches the doc's `Real / Amount` and SI. The `mol` unit is added regardless (gas_constant needs it), so the marginal cost is one alias.
- **D3 — Names** follow the shipped descriptive UPPER_SNAKE convention (`STANDARD_GRAVITY`, `SPEED_OF_LIGHT`, `BOLTZMANN_CONSTANT`): `AVOGADRO_CONSTANT`, `PLANCK_CONSTANT`, `STEFAN_BOLTZMANN_CONSTANT`, `VACUUM_PERMITTIVITY`, `VACUUM_PERMEABILITY`, `MOLAR_GAS_CONSTANT`, `ELEMENTARY_CHARGE`.
- **D4 — Bodies in base-unit-expanded sci-notation**, validated by physics-identity cross-checks rather than the (inert) return annotation. The existing `BOLTZMANN_CONSTANT()` decimal literal is modernized to sci-notation for consistency (value-preserving).
- **D5 — `A`/`mol`/`cd` fix lands in the Rust `unit_to_scalar` table** (units.rs), the only change that makes them usable inside stdlib fn bodies; this simultaneously closes the §2.2 doc gap project-wide.

### Final constant table

| Doc name (§2.4) | Shipped form | Return dim | Value (CODATA 2018 / 2019-SI exact) | Body (base-unit) |
|---|---|---|---|---|
| `pi` | `pi` (exists) | Real | π | builtin |
| `tau` | `tau` (exists, not in §2.4) | Real | 2π | builtin |
| `e` | `e` **(new)** | Real | 2.718281828459045 (`std::f64::consts::E`) | builtin |
| `g` | `STANDARD_GRAVITY()` (exists) | Acceleration | 9.80665 m/s² | `9.80665 * 1m / (1s * 1s)` |
| `c` | `SPEED_OF_LIGHT()` (exists) | Velocity | 299792458 m/s | `299792458.0 * 1m / 1s` |
| `boltzmann` | `BOLTZMANN_CONSTANT()` (exists; modernize literal) | HeatCapacity | 1.380649e-23 J/K | `1.380649e-23 * 1kg * 1m * 1m / (1s * 1s * 1K)` |
| `avogadro` | `AVOGADRO_CONSTANT()` **(new)** | InverseAmount | 6.02214076e23 /mol | `6.02214076e23 / 1mol` |
| `planck` | `PLANCK_CONSTANT()` **(new)** | Action (`Energy*Time`) | 6.62607015e-34 J·s | `6.62607015e-34 * 1kg * 1m * 1m / 1s` |
| `stefan_boltzmann` | `STEFAN_BOLTZMANN_CONSTANT()` **(new)** | `Power/Area/T⁴` alias | 5.670374419e-8 W/(m²·K⁴) | `5.670374419e-8 * 1kg / 1s / 1s / 1s / 1K / 1K / 1K / 1K` |
| `vacuum_permittivity` | `VACUUM_PERMITTIVITY()` **(new)** | Permittivity (`Capacitance/Length`) | 8.8541878128e-12 F/m | `8.8541878128e-12 * 1s * 1s * 1s * 1s * 1A * 1A / 1kg / 1m / 1m / 1m` |
| `vacuum_permeability` | `VACUUM_PERMEABILITY()` **(new)** | Permeability (`Inductance/Length`) | 1.25663706212e-6 H/m | `1.25663706212e-6 * 1kg * 1m / 1s / 1s / 1A / 1A` |
| `gas_constant` | `MOLAR_GAS_CONSTANT()` **(new)** | MolarGasConstant (`Energy/Amount/T`) | 8.314462618 J/(mol·K) | `8.314462618 * 1kg * 1m * 1m / 1s / 1s / 1mol / 1K` |
| `elementary_charge` | `ELEMENTARY_CHARGE()` **(new)** | Charge | 1.602176634e-19 C | `1.602176634e-19 * 1A * 1s` |

New aliases in `units.ri` (names tactical; `StefanBoltzmann` dim has no standard quantity name): `Action`, `Permittivity`, `Permeability`, `MolarGasConstant`, an alias for `Power/Area/T⁴`, `InverseAmount`. (`Charge`, `Acceleration` are named dimensions; `Velocity`, `HeatCapacity` already aliased.)

**Value provenance (G6):** all values match `docs/reify-stdlib-reference.md` §2.4 and CODATA. Exact-by-definition (2019 SI): `c`, `boltzmann`, `avogadro`, `planck`, `elementary_charge`, and `gas_constant` (= N_A·k_B). CODATA-measured: `stefan_boltzmann`, `vacuum_permittivity`, `vacuum_permeability` (doc values retained). `elementary_charge` `1.602176634e-19` equals the `eV` factor in `si_units.rs:151` — a built-in consistency anchor.

---

## 5. Out of scope

- **§2.1 dimension-count fix** ("48 not 34" + stale `Section 3.2` xref) — a separate low-sev P8 row, not part of the constants cluster.
- **Generalizing the no-return-dim-check** (§3.8) — a latent gap affecting *all* stdlib fns, not just constants; noted for a future review. This PRD mitigates it locally via cross-checks.
- **A new dimensionless-but-dimensionful builtin path** (would have enabled all-bare) — rejected in D1.
- **`tau` in §2.4** — `tau` already ships; the doc reconcile (ε) may mention it but it is not new work.

### G5 — why bare B (not B+H)

Self-contained stdlib feature. Blast radius ≈ 1 crate (reify-compiler: 2 `src` files + 1 `.ri` + 1 example + 1 test) plus a doc. Touches **none** of the high-stakes seams (FEA, ComputeNode dispatch, persistent-naming, multi-kernel, grammar/parser). Mechanism count low; no cross-PRD consumer. ⇒ contracts + two-way boundary tests (H) are not warranted.

---

## 6. Cross-PRD relationship & seam ownership (G4)

**Fully standalone.** Files: `crates/reify-compiler/src/units.rs`, `crates/reify-compiler/src/constants.rs`, `crates/reify-compiler/stdlib/units.ri`, `examples/stdlib/constants.ri`, `crates/reify-compiler/tests/constants_example_tests.rs`, `docs/reify-stdlib-reference.md`.

| Seam | Owner | Notes |
|---|---|---|
| Builtin-constant resolver (`constants.rs`) | this PRD (task β) | additive: one new arm + one `BUILTIN_NAMES` entry |
| Unit-literal fallback (`unit_to_scalar`, units.rs) | this PRD (task α) | additive: three new arms; consulted by all callers |
| `units.ri` constant region | this PRD (task γ) | extends the #4026 region |

No contested-ownership pair (overlay G4 list) is touched. The in-flight **geometry** decomposition (`geometry-primitive-constructors.md` etc.) operates on the compiler IR / kernel / eval-op layers (`types.rs`, kernel crates, `engine_*`); it shares **no file** with this batch. Safe to run concurrently (confirmed by the task prompt).

---

## 7. Decomposition plan — one task per slice, each with a user-observable signal (G2)

Roots α, β have no deps; γ→α; δ→{β,γ}; ε→γ. Each task touches a disjoint file set ⇒ no narrow-lock contention.

- **α — `unit_to_scalar`: bare `A`/`mol`/`cd` base units.** *(`crates/reify-compiler/src/units.rs`)*
  Add three match arms (`A`→Current, `mol`→AmountOfSubstance, `cd`→LuminousIntensity, factor 1.0) mirroring the existing `kg`/`g` arms.
  **Signal:** `reify eval` of a one-liner `structure def P { let q = 1.5 * 1A * 1s }` prints a Charge-dimensioned value (`A·s`/`s·A`) instead of `error: unknown unit: A`; `1mol`/`1cd` likewise resolve. A `units.rs` unit test asserts `unit_to_scalar("A"|"mol"|"cd")` returns the matching `DimensionVector`. *(grammar_confirmed: true)*

- **β — `constants.rs`: `e` (Euler's number) bare builtin.** *(`crates/reify-compiler/src/constants.rs`)*
  Add `"e" => Value::Real(std::f64::consts::E)` arm; add `"e"` to `BUILTIN_NAMES`; the two exhaustiveness guard tests stay green (the probe already lists `"e"`).
  **Signal:** `reify eval` of `structure def P { let x = e }` prints `2.718281828…` (dimensionless); a `let x = E` misspelling emits the case-variant hint suggesting `e` (same diagnostic path as `pi`). *(grammar_confirmed: true)*

- **γ — `units.ri`: seven new dimensionful constant fns + aliases.** *(`crates/reify-compiler/stdlib/units.ri`)* — **depends on α.**
  Add the composite-dimension aliases (§4) then the seven `pub fn` constants (`AVOGADRO_CONSTANT`, `PLANCK_CONSTANT`, `STEFAN_BOLTZMANN_CONSTANT`, `VACUUM_PERMITTIVITY`, `VACUUM_PERMEABILITY`, `MOLAR_GAS_CONSTANT`, `ELEMENTARY_CHARGE`) with base-unit-expanded sci-notation bodies + CODATA/SI doc-comments. Modernize the `BOLTZMANN_CONSTANT()` literal to `1.380649e-23 …` (value-preserving).
  **Signal:** `reify check` of a probe `.ri` referencing all seven passes with zero Error diagnostics; `reify eval` prints each with its correct magnitude **and** dimension (e.g. `PLANCK_CONSTANT() = 6.62607015e-34 m^2·kg·s^-1`, `ELEMENTARY_CHARGE() = 1.602176634e-19 s·A`). *(grammar_confirmed: true)*

- **δ — integration gate: example + regression test.** *(`examples/stdlib/constants.ri` + `crates/reify-compiler/tests/constants_example_tests.rs`)* — **depends on β, γ.**
  Extend the example to reference all 12 constants and add **physics-identity cross-check** bindings: `r_check = MOLAR_GAS_CONSTANT() - AVOGADRO_CONSTANT() * BOLTZMANN_CONSTANT()` (≈ 0), `em_check = VACUUM_PERMITTIVITY() * VACUUM_PERMEABILITY() * SPEED_OF_LIGHT() * SPEED_OF_LIGHT()` (≈ 1, dimensionless), plus `e`/`pi` usage. Extend the test to: (1) compile-clean (a wrong dimension makes a cross-check `-` dimension-error ⇒ a Error diagnostic ⇒ fails), (2) **eval the module and assert each cross-check is within tolerance** of its identity target (catches value/exponent errors the return annotation cannot), (3) name-presence pins for all 12, (4) no-inline-magic-number pins for the new values.
  **Signal:** `cargo test -p reify-compiler constants_example` passes; `reify check examples/stdlib/constants.ri` ⇒ zero errors; the cross-check fields evaluate within tolerance. *(grammar_confirmed: true)*

- **ε — doc reconcile.** *(`docs/reify-stdlib-reference.md` §2.4, §2.2)* — **depends on γ.**
  Rewrite §2.4 to the shipped two-category split: `pi`/`tau`/`e` as bare `Real` identifiers; the ten dimensionful constants as their `UPPER_SNAKE()` fns with dimension return types, exact values, and CODATA/SI source notes — retiring the `let g : Acceleration = …` fiction (and the Bucket-B g/c/boltzmann form rows). Update §2.2 to state `A`/`mol`/`cd` are now usable bare units.
  **Signal:** every form shown in the reconciled §2.4 can be pasted into a `.ri` file and `reify check` accepts it (no surviving `let <name> : Dim = …` top-level-const fiction). *(grammar_confirmed: true)*

---

## 8. Open (tactical) questions

- **Q1 (γ):** alias name for the Stefan-Boltzmann dimension (`Power / Area / Temperature / Temperature / Temperature / Temperature`). No standard quantity name exists; pick a clear public name (e.g. `StefanBoltzmannDim`) or keep it private to the constant if a non-`pub` alias is permissible in return position. Tactical — does not change the surface.
- **Q2 (α):** whether to *also* add `pub unit A : Current` / `mol` / `cd` to `units.ri` for LSP/completion parity. The `unit_to_scalar` fallback already resolves them everywhere; the `.ri` decl is purely for registry-listing ergonomics. Default: skip (keep scope minimal) unless completion-listing regression is observed.
- **Q3 (δ):** exact `reify eval` print format for tiny/huge magnitudes (sci-notation vs decimal) — the test should assert on the **cross-check identities within tolerance**, not on brittle per-constant stdout strings.
