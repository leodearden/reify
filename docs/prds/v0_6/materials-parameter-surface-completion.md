# PRD: `std.materials` §6 — restore the documented parameter surface & constraints

**Status:** draft · **Authored:** 2026-06-02 · **Milestone:** v0_6
**Closes gap-register cluster:** P12 materials-breadth (`docs/architecture-audit/stdlib-reference-gap-register-2026-06-01.md`)
**Source doc:** `docs/reify-stdlib-reference.md` §6 (`std.materials`)
**Approach:** bare **B** (self-contained declarative stdlib feature; not a high-stakes seam — see §5 G5).

---

## 0. Supersession / relationship to prior work

This PRD completes the material-trait **parameter surface** that the April PRD
`docs/prds/stdlib-trait-breadth.md` (tasks #2347/#2349/#2352/#2354) deliberately
shipped in a **collapsed, placeholder** form. That PRD's explicit scope was to
*declare the named traits exist* with correct inheritance edges; it explicitly
deferred (a) the full per-trait param surface and (b) dimensional types,
because at the time **`undef` did not exist in the grammar** (so optional
params could not be expressed) and dimensional aliases were unbuilt. The
deviations are recorded in `docs/notes/stdlib-trait-breadth-audit-v01.md`
(rows tagged "Param collapse (OPEN)", "Param rename", "Default gap (OPEN)",
"Constraint gap").

**The blocking precondition has since cleared.** Task **#3918** landed a
first-class `undef` literal (grammar rule `undef_literal`, commit `856f1dd711`)
and the conformance checker now treats a trait/structure param with **any**
default (including `= undef`) as **optional** for conformers. The stale "the
Reify grammar has no `undef` keyword" comments in `materials_electrical.ri`
(Decision #3) and `io.ri` (lines 11–12) predate #3918 and are **false today**
(see §3). This PRD restores the doc-faithful surface that #3918 unblocked.

**Nothing to supersede as phantom-done.** `search_tasks` (2026-06-02) surfaced
no task implementing the param-surface restoration. The nearest tasks are the
**deferred** Real→dimensional tightening tasks #3111 (mechanical), #3112
(thermal), #3113 (optical) — a *different* axis of work (types, not
names/optionality) that this PRD explicitly does **not** do; the G4 seam to
them is in §6.

**Out of this PRD's re-implementation scope, by deliberate prior decision**
(per the task brief): the base `Material`→`MaterialSpec` rename (#1876/#2411),
`Elastic`/`Strong`/`Hard`/`Ductile` staying free-standing instead of refining
the base (DRIFT-by-design, #3487), and `Insulating`'s `determined(...)`→`> 0`
weakening (#2484). These are **doc-reconcile only** and are folded into the
doc-reconcile task ε (§7), not re-implemented.

---

## 1. Consumer & user-observable surface (G1)

**Consumer:** end-user Reify `.ri` source — engineering designs that declare or
conform to material-capability traits. Concretely: the existing
`examples/m8_materials.ri`, `examples/io_export.ri`,
`examples/drivebelt_trait_bounds.ri` (all migrated to the restored names), a new
worked conformer per restored trait, and `docs/reify-stdlib-reference.md` §6
itself (the surface users read). LSP hover/completion lists these trait params.

These are **trait declarations in `.ri` stdlib files** — pure compiler
front-end (trait/param/constraint resolution + conformance checking). There is
**no in-engine kernel/dispatch/ComputeNode seam** to orphan: a trait param is
consumed by the existing conformance checker (`crates/reify-compiler/src/conformance/`)
and constraint evaluator, both pre-existing seams. No production solver/eval
code reads any of the renamed param names (verified: `grep` of the changing
identifiers across non-test `crates/` finds only `tests/` and
`reify-test-support` fixtures — §3.6).

**User-observable surface:** after this batch —
- a user writes `trait def X : TemperatureDependent` (or supplies
  `reference_temperature`) and `reify check` accepts it;
- the doc's exact param names (`ultimate_tensile_strength`,
  `elongation_at_break`, `fatigue_limit`/`fatigue_strength_at`/`fatigue_cycles`,
  `charpy_impact`/`izod_impact`) are declarable; the old abbreviated/collapsed
  names (`uts`, `elongation`, `endurance_limit`, `impact_energy`) emit
  `unresolved` / `missing required member`;
- `reify check` **rejects** an `Elastic` material with `poissons_ratio = 0.7`
  (or `-0.1`) with a constraint-violation diagnostic, and **accepts** `0.3`;
- a conformer may **omit** any `= undef` optional param (`shear_modulus`,
  `compressive_strength`, `reduction_of_area`, the thermal/electrical/optical
  optionals) and `reify check` is clean;
- every form shown in the reconciled doc §6 parses + checks.

---

## 2. Sketch of approach

Pure declarative edits to the five `materials_*.ri` stdlib files plus atomic
migration of the breaking renames' consumers. No grammar work, no Rust
front-end changes (the substrate is already shipped — §3).

| Concern | Trait(s) | File | Change |
|---|---|---|---|
| Add missing base trait | `TemperatureDependent` (new, §6.1) | `materials_mechanical.ri` | new trait, `reference_temperature : Temperature = 293.15K` |
| Add missing constraint + optionality | `Elastic` (§6.2) | `materials_mechanical.ri` | `constraint 0 < poissons_ratio < 0.5`; `shear_modulus … = undef` |
| Optionality | `Strong`, `Ductile` (§6.2) | `materials_mechanical.ri` | `compressive_strength = undef`, `reduction_of_area = undef` |
| Rename to doc names | `Strong`, `Ductile` (§6.2) | `materials_mechanical.ri` + consumers | `uts`→`ultimate_tensile_strength`, `elongation`→`elongation_at_break` |
| Restore collapsed params | `FatigueRated`, `ImpactResistant` (§6.2) | `materials_mechanical.ri` + consumers | 3 fatigue params (drop `endurance_limit`); `charpy_impact`+`izod_impact` (drop `impact_energy`) |
| Optionality sweep | `ThermallyCharacterized`, `OpticallyCharacterized` (§6.3/§6.5) | `materials_thermal.ri`, `materials_optical.ri` | mark doc-`= undef` params optional |
| Optionality + interaction | `ElectricallyCharacterized`/`Insulating` (§6.4) | `materials_electrical.ri` | mark `dielectric_*`/`magnetic_permeability` optional; document Insulating constraint degradation |
| Doc reconcile | all §6 | `docs/reify-stdlib-reference.md` | confirm restored surface + annotate deliberate deviations |

- **Types stay `Real`/`Int`** for the restored mechanical params, matching the
  module's `// all Real pending dimensional type wiring` convention. Dimensional
  tightening (`Pressure`/`Energy`) is the **separate, deferred** #3111 effort
  (§6 seam). The two exceptions are doc-faithful and substrate-verified:
  `reference_temperature : Temperature` (a brand-new param with no Real legacy;
  `293.15K` *requires* a temperature-compatible type) and `fatigue_cycles : Int`
  (a dimensionless count — `Int` is its permanent correct type).
- **Both `charpy_impact` and `izod_impact` are optional** (doc marks both
  `= undef`), and `ImpactResistant` carries **no** "at least one" cross-param
  constraint — faithful to the doc; a material may declare neither.
- The breaking renames ship **with their consumer migration in the same task**
  (γ→… atomicity): a rename that left a consumer referencing the old name would
  break `examples_smoke.rs` (walks every `examples/*.ri`) and the trait tests.

---

## 3. Pre-conditions — verified substrate facts (G3)

All probes run 2026-06-02 against `target/debug/reify` (the **real binary**, per
the project G3 note that the tree-sitter CLI is stale). Fixtures under
`/tmp/prd-gate-fixtures/`.

1. **`= undef` on a trait/structure param makes it OPTIONAL for conformers.**
   `param_declaration` (grammar.js:578-585) accepts `= <binding_value>`;
   `undef_literal` (grammar.js:1488) is a first-class expression (task #3918).
   The conformance checker satisfies a `param` requirement from a `param`
   default (`conformance/checker.rs:3-12`). **Probe:** a `structure def` conforming
   to `trait ImpactResistant { param charpy_impact : Real = undef  param izod_impact : Real = undef }`
   that supplies only `charpy_impact` ⇒ `reify check` "All constraints satisfied".
   **Negative control:** omitting a *non*-defaulted (`izod_impact : Real`) param
   ⇒ `error: missing required member 'izod_impact'`. ⇒ optionality is genuine,
   not cosmetic.

2. **Chained comparison `0 < poissons_ratio < 0.5` parses AND enforces both
   bounds.** **Probe:** `constraint 0 < poissons_ratio < 0.5` —
   `poissons_ratio = 0.3` ⇒ `OK`; `0.7` ⇒ `VIOLATED … error: constraint … violated`;
   `-0.1` ⇒ `VIOLATED`. ⇒ it is not mis-parsed as `(0 < x) < 0.5`; both bounds bite.

3. **`Temperature` dimension alias + dimensioned default `293.15K` work** (even
   under `#no_prelude`). **Probe:** `trait TemperatureDependent { param reference_temperature : Temperature = 293.15K }`
   with a conformer that omits the param ⇒ `reify check` clean (defaults applied);
   supplying `350.0K` ⇒ clean. `Int` likewise resolves as a param type
   (`fatigue_cycles : Int = undef`, conformer supplies `1000000` ⇒ clean).

4. **A sub-trait CANNOT "re-require" an optional parent param.** **Probe:**
   re-declaring `param dielectric_strength : Real` (no default) inside
   `trait Insulating : ElectricallyCharacterized` where the parent declares it
   `= undef` does **not** force a conformer to supply it — the conformer still
   passes. The parent default wins chain-wide. ⇒ once `dielectric_strength` is
   optional on `ElectricallyCharacterized`, it is optional everywhere.

5. **Omitting `dielectric_strength` degrades Insulating's `> 0` constraint to a
   user-visible INDETERMINATE warning, not a pass or an error.** **Probe (same
   fixture):** an `Insulating` conformer that omits `dielectric_strength` ⇒
   `INDETERMINATE OmitsDielectric#constraint[1]` + `warning: constraint …
   indeterminate: undefined inputs` + `No constraints violated (1 indeterminate)`
   (exit 0). This is the Kleene-`undef`-does-not-falsify rule (arch §2.5). It is
   **fully consistent with task #2484's accepted weakening** (the strong
   `determined(...)` guarantee was already conceded as ungrammatical); the
   `> 0` bound survives as a positive-when-supplied check that *nudges* (via the
   warning) when omitted. This is the load-bearing G6 design point for task δ.

6. **No production code reads the renamed names.** `grep -nE '\b(uts|elongation|impact_energy|endurance_limit)\b'`
   across `examples/` + `crates/` finds them only in: 3 example `.ri` files
   (`m8_materials.ri`, `io_export.ri`, `drivebelt_trait_bounds.ri`), the stdlib
   `.ri` defs themselves, 6 test files, and `reify-test-support/src/fixtures.rs`.
   **No solver/eval/kernel production `.rs` reads them.** ⇒ renames break only
   tests + examples, all migrated in the same task (§7 β; per-file map in the
   capability manifest).

7. **The new `Elastic` constraint breaks no existing conformer (G6).** Every
   `poissons_ratio` literal in the corpus is in `(0, 0.5)`: `0.29`, `0.3`,
   `0.33`. The doc bound deliberately excludes auxetic (`ν < 0`) and
   incompressible (`ν = 0.5`) materials — a documented, intended exclusion, not
   an over-tight guess. Making `shear_modulus`/`compressive_strength`/
   `reduction_of_area` optional cannot break a conformer that already supplies
   them (required→optional is a relaxation).

---

## 4. Resolved design decisions

- **D1 — Restore is unblocked by #3918, not blocked.** The April collapse was a
  *grammar-limitation workaround* ("no `undef`"), now obsolete. Verified §3.1.
- **D2 — Hard rename, no alias** (`uts`→`ultimate_tensile_strength`,
  `elongation`→`elongation_at_break`; drop `endurance_limit`, `impact_energy`).
  Reify has **no** param-alias mechanism; the project precedent is hard-rename +
  atomic consumer migration (`Material`→`MaterialSpec`, #1876, "No deprecation
  alias is provided"). Consumers migrate in the same task (§3.6 map).
- **D3 — Types stay `Real`/`Int`; dimensional tightening is #3111's job.** Out
  of scope by the task brief, the sibling PRD, and the gap-register (row 104 =
  separate deferred A-implement). Exceptions `Temperature` (new) + `Int` (count)
  per §2. Wiring #3111→β so it retypes the *restored* names (§6).
- **D4 — `dielectric_strength` (and the other §6.4 doc-optionals) become
  optional per doc; Insulating's `> 0` constraint stays and degrades to an
  indeterminate **warning** when omitted** (verified §3.5). This is doc-faithful
  *and* consistent with #2484. Re-requiring on the sub-trait is impossible
  (§3.4), so this is the only coherent behavior — not a free choice.
- **D5 — `TemperatureDependent` is a free-standing §6.1 base trait** (does not
  refine `MaterialSpec`), matching the doc, and lives in `materials_mechanical.ri`
  beside `MaterialSpec` (the de-facto base-trait home — there is no separate base
  file). Its consumer is a worked conformer in α's test + doc §6.1.
- **D6 — `glass_transition` sentinel retired.** The `0.0`-means-N/A sentinel
  convention (a workaround for required-ness) is replaced by genuine `= undef`
  optionality; the source comment is updated (task γ).

---

## 5. Out of scope

- **Real→dimensional type tightening** of the restored mechanical params
  (`youngs_modulus`/`shear_modulus`/`yield_strength`/`ultimate_tensile_strength`/
  `compressive_strength`/`fatigue_*`/`charpy_impact`/`izod_impact` →
  `Pressure`/`Energy`) — deferred **#3111** (mechanical), **#3112** (thermal),
  **#3113** (optical). This PRD ships `Real`; #3111-family retypes. Seam in §6.
- **Re-implementing the deliberate deviations** (`Material`→`MaterialSpec`
  rename; free-standing `Elastic`/`Strong`/`Hard`/`Ductile`; `Insulating`
  `determined()`→`> 0`). Doc-reconcile only (task ε).
- **`Refractory` `1500.0` (K-equiv) vs doc `1500degC`** — a unit-semantics
  mismatch tied to the deferred Temperature-typing (#3112); ε annotates the doc,
  the literal fix rides #3112. (gap-register P12 low row.)
- **A new dimensionless-but-dimensionful builtin path / type-system work.** None
  needed.

### G5 — why bare B (not B+H)

Self-contained declarative stdlib feature. Blast radius ≈ 1 crate
(`reify-compiler`: 5 `.ri` files + tests) plus 3 examples + 1 doc; **no Rust
front-end change**. Touches **none** of the project's high-stakes seams (FEA,
ComputeNode dispatch, persistent-naming, multi-kernel, grammar/parser — the
grammar it relies on is already shipped). Mechanism count low; no cross-PRD
*consumer*. The only cross-PRD relationship is a **producer-side dependency
ordering** with the deferred #3111-family (§6), handled by a dependency edge,
not a contract. ⇒ contracts + two-way boundary tests (H) are not warranted. The
atomic consumer-migration requirement of the renames is met by task-level
atomicity + the `examples_smoke` regression pin, not by H boundary tests.

---

## 6. Cross-PRD relationship & seam ownership (G4)

Files owned by this PRD: `crates/reify-compiler/stdlib/materials_mechanical.ri`,
`materials_thermal.ri`, `materials_optical.ri`, `materials_electrical.ri`;
`docs/reify-stdlib-reference.md` §6; the 3 example `.ri` + the consumer test
files (β); a new `materials_param_surface_tests.rs`.

| Seam | Owner | Notes |
|---|---|---|
| §6.1/§6.2 mechanical trait surface | this PRD (α, β) | additive (α) + breaking renames/restores (β) |
| §6.3/§6.5 thermal+optical optionality | this PRD (γ) | additive `= undef` |
| §6.4 electrical optionality + Insulating | this PRD (δ) | additive `= undef` + documented constraint degradation |
| §6 doc | this PRD (ε) | confirm restored surface + annotate deliberate deviations |
| **Real→dimensional tightening (mechanical)** | **#3111** (deferred) | **must run AFTER β** (retypes the *renamed* params). Edge `3111 → β` wired; #3111 details annotated with the rename map. |
| **Real→Temperature (thermal)** | **#3112** (deferred) | edits `materials_thermal.ri` — **after γ** to avoid same-file rebase. Edge `3112 → γ`. `= undef` is type-agnostic so they compose. |
| **Real→Length (optical `reference_thickness`)** | **#3113** (deferred) | edits `materials_optical.ri` — **after γ**. Edge `3113 → γ`. |

No contested-ownership pair (overlay G4 list: persistent-naming/multi-kernel,
imported-field-source/multi-kernel, topology-selectors/persistent-naming) is
touched. The sibling **`stdlib-trait-breadth.md`** PRD is **complete**; this PRD
extends its declarations and does not share an open task with it. The in-flight
geometry/FEA decompositions share **no file** with this batch (compiler-IR /
kernel / eval-op layers vs `.ri` trait defs) — safe to run concurrently.

---

## 7. Decomposition plan — one task per slice, each with a user-observable signal (G2)

Roots: α, γ, δ (independent). β → α (same file `materials_mechanical.ri`;
serialized to avoid contention + renames build on the additive surface).
ε → {α, β, γ, δ} (doc reflects all shipped surface). Cross-PRD edges
`3111→β`, `3112→γ`, `3113→γ` wired (§6). All tasks `grammar_confirmed: true`
(substrate shipped, §3).

- **α — mechanical *additive* surface.** *(`materials_mechanical.ri` + new
  `crates/reify-compiler/tests/materials_param_surface_tests.rs`)*
  Add `trait TemperatureDependent { param reference_temperature : Temperature = 293.15K }`
  (§6.1). On `Elastic`: add `constraint 0 < poissons_ratio < 0.5` and change
  `shear_modulus : Real` → `shear_modulus : Real = undef`. On `Strong`/`Ductile`:
  `compressive_strength : Real = undef` / `reduction_of_area : Real = undef`.
  **No renames, no collapses** — purely additive; existing conformers/tests stay
  valid (§3.7). New test conforms a worked material to `TemperatureDependent`
  (omitting + supplying `reference_temperature`) and asserts the `Elastic`
  constraint fires.
  **Signal:** `reify check` of the new fixtures — a `TemperatureDependent`
  conformer omitting `reference_temperature` is clean; an `Elastic` conformer
  with `poissons_ratio = 0.7` ⇒ constraint-violated error, `0.3` ⇒ clean; a
  conformer omitting `shear_modulus`/`compressive_strength`/`reduction_of_area`
  is clean. `cargo test -p reify-compiler materials_param_surface` green; the
  pre-existing `materials_mechanical_tests.rs` still green (additive). *(grammar_confirmed: true)*

- **β — mechanical *breaking* renames + collapsed-param restores + atomic
  consumer migration.** *(`materials_mechanical.ri` + `examples/m8_materials.ri`,
  `examples/io_export.ri`, `examples/drivebelt_trait_bounds.ri`,
  `crates/reify-compiler/tests/materials_mechanical_tests.rs`,
  `crates/reify-compiler/tests/stdlib_loader_tests.rs`,
  `crates/reify-compiler/tests/parametric_tensor_resolution_tests.rs`,
  `crates/reify-compiler/tests/cross_module_alias_propagation_tests.rs`,
  `crates/reify-eval/tests/m8_3_stdlib_integration.rs`,
  `crates/reify-eval/tests/drivebelt_trait_bounds.rs`,
  `crates/reify-test-support/src/fixtures.rs`)* — **depends on α.**
  `Strong`: `uts`→`ultimate_tensile_strength` (constraint becomes
  `ultimate_tensile_strength >= yield_strength`). `Ductile`:
  `elongation`→`elongation_at_break`. `FatigueRated`: replace `endurance_limit`
  with `fatigue_limit : Real = undef`, `fatigue_strength_at : Real = undef`,
  `fatigue_cycles : Int = undef`. `ImpactResistant`: replace `impact_energy` with
  `charpy_impact : Real = undef`, `izod_impact : Real = undef`. Migrate **every**
  consumer in the file set above (per-file identifier map in the manifest) in the
  same commit.
  **Signal:** `ultimate_tensile_strength`/`elongation_at_break`/`fatigue_limit`/
  `fatigue_strength_at`/`fatigue_cycles`/`charpy_impact`/`izod_impact` all
  declarable; `uts`/`elongation`/`endurance_limit`/`impact_energy` ⇒
  `unresolved`/`missing required member`; a `FatigueRated`/`ImpactResistant`
  conformer may supply any subset (incl. none); the drivebelt integration test
  + `examples_smoke` + all migrated trait tests pass (`cargo test -p reify-compiler`,
  `-p reify-eval`). *(grammar_confirmed: true)*

- **γ — §6.3/§6.5 thermal + optical optionality.** *(`materials_thermal.ri` +
  `materials_optical.ri` + a `tests/` section, e.g. extend
  `materials_param_surface_tests.rs` region or a new fixture)*
  `ThermallyCharacterized`: `melting_point`/`max_service_temperature`/
  `glass_transition` → `= undef` (retire the `0.0` sentinel comment, D6).
  `OpticallyCharacterized`: `absorption_coefficient`/`transmittance`/
  `reference_thickness` → `= undef`. Additive — no consumer breakage.
  **Signal:** a `ThermallyCharacterized` conformer omitting the three thermal
  optionals and an `OpticallyCharacterized` conformer omitting the three optical
  optionals both `reify check` clean; an existing `Refractory` conformer that
  omits `max_service_temperature` produces an **indeterminate warning** on its
  `>= 1500.0` constraint (not an error) — asserted. *(grammar_confirmed: true)*

- **δ — §6.4 electrical optionality + Insulating interaction.** *(`materials_electrical.ri`
  + a `tests/` fixture)*
  `ElectricallyCharacterized`: `dielectric_constant`/`dielectric_strength`/
  `magnetic_permeability` → `= undef`. Keep `Insulating`'s
  `dielectric_strength > 0` constraint; update the file header to document the
  degrade-to-indeterminate-warning behavior (D4) replacing the stale "no `undef`
  keyword" Decision-#3 text.
  **Signal:** a `Conductive`/`ElectricallyCharacterized` conformer omitting the
  three optionals ⇒ clean; an `Insulating` conformer omitting
  `dielectric_strength` ⇒ clean **with** `warning: constraint … indeterminate`
  (user-observable nudge); an `Insulating` conformer supplying
  `dielectric_strength = 0` ⇒ constraint violated. *(grammar_confirmed: true)*

- **ε — doc reconcile §6.** *(`docs/reify-stdlib-reference.md` §6)* — **depends
  on α, β, γ, δ.**
  Confirm the restored param **names/constraints/optionality** now match the
  shipped `.ri` (the doc was the target — mostly a verification pass). Annotate
  the **deliberate deviations** so §6 stops misleading: §6.1 base trait is
  `MaterialSpec` (+ the canonical `Material` struct), cite #1876/#2411; §6.2
  `Elastic`/`Strong`/`Hard`/`Ductile` are free-standing (DRIFT-by-design,
  density/name via a `material : MaterialSpec` slot), cite #3487; §6.4
  `Insulating` uses `dielectric_strength > 0` (degrade-to-indeterminate when
  omitted), cite #2484; §6.3 `Refractory` threshold is `1500.0` (K-equiv)
  pending Temperature typing (#3112). Add a one-line note that the dimensioned
  param types (`Pressure`/`Energy`/`Temperature`/`Length`) shown remain the
  **target** of the deferred #3111-family and are currently `Real` placeholders
  (do **not** downgrade the doc types — they are the tracked aspiration).
  **Signal:** every trait/param/constraint form shown in reconciled §6 can be
  pasted into a `.ri` file and `reify check` accepts it; no surviving
  `trait Material { … }` base-trait line where the impl has `MaterialSpec`, no
  surviving `determined(dielectric_strength)`. *(grammar_confirmed: true)*

---

## 8. Open (tactical) questions

- **Q1 (β):** whether `materials_mechanical_tests.rs` has a single
  "all-mechanical-traits" assertion touching several renamed params (raising
  intra-task diff size) or per-trait tests — either way β migrates them; tactical.
- **Q2 (γ/δ):** test-fixture placement — extend `materials_param_surface_tests.rs`
  (one new test module for the whole batch) vs per-module fixtures. Default:
  one shared module with per-trait `#[test]` fns; tactical.
- **Q3 (β):** `examples/m8_materials.ri` / `io_export.ri` carry illustrative
  comment blocks echoing the old param names (e.g. `// uts = …`); migrate the
  comments too so the examples don't re-teach the dead names. Tactical.
